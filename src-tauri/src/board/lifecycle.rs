//! Ciclo di vita dei task: l'unico posto che scrive `Task::state`, `attempts`,
//! `note`, `last_report` e `questions`. Ogni transizione e' un `TaskEvent` applicato con
//! `Task::apply`; `actions` ha la tabella delle azioni dell'utente e `ops` i
//! metodi di `Board` che trovano il task e applicano l'evento.

use super::{
    E2eRun, FailCause, Failure, Finished, PreviousAttempt, Run, RunPhase, Task, TaskReport, TaskState, TaskStatus,
    WorkerActivity, WorktreeInfo, NOTE_MAX,
};
use crate::validate::clip_chars;
use std::fmt;

mod actions;
mod ops;
mod question;

pub use actions::{permits, Op, TaskAction};

const LAST_REPORT_MAX: usize = 200;

/// Come si conclude un task in corso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Failed(Failure),
}

/// Una transizione del ciclo di vita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    /// `queued` -> `running` (in preparazione): scelto dallo scheduler.
    Start,
    /// Segno di vita della preparazione di `attempt`; `step` sostituisce il passo mostrato.
    PrepareStep { attempt: u32, step: Option<String> },
    /// Fine della preparazione (setup finito o fallito): via il passo, battito a `now`.
    PrepareEnded { attempt: u32 },
    /// Il worktree appena creato, prima del setup.
    WorktreeCreated { attempt: u32, worktree: WorktreeInfo },
    /// Il worker di `attempt` e' stato spawnato nel tab `session`.
    WorkerSpawned { attempt: u32, session: String, worktree: Option<WorktreeInfo>, note: Option<String> },
    /// Il worker `session` cambia attivita'.
    WorkerChanged { session: String, activity: WorkerActivity },
    /// Fine di un task in corso (report del worker, avvio fallito, tab chiuso,
    /// stop). Se `session` e' dato deve essere quella del task.
    Finish { session: Option<String>, outcome: Outcome, report: Option<Box<TaskReport>>, note: Option<String> },
    /// Archiviazione del workspace: `running` -> `queued`.
    Requeue { note: String },
    /// "Riprova": `failed` -> `queued`.
    Retry,
    /// "Segna come fatto": `failed` -> `done`.
    MarkDone,
    /// Il worker `session` del tentativo `attempt`, la cui sessione era finita
    /// senza report, riferisce ancora: `failed` -> `running`.
    Reopen { session: String, attempt: u32, note: String },
    /// Il worker `session` fa una domanda all'utente (`ask_user`): serve un worker
    /// vivo e nessuna domanda ancora in attesa.
    QuestionAsked { session: String, text: String, options: Vec<String> },
    /// La domanda `question_id` del worker `session` ha avuto risposta: `answer` e' il
    /// testo consegnato dall'app, `None` se l'utente ha risposto nel terminale. Il
    /// worker riprende a lavorare.
    QuestionAnswered { session: String, question_id: String, answer: Option<String> },
    /// La consegna nel terminale di una risposta data dall'app e' fallita: la
    /// domanda torna in attesa (annulla `QuestionAnswered`).
    AnswerUndelivered { session: String, question_id: String },
}

/// Perche' una transizione o un'azione e' stata rifiutata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    NotFound(String),
    /// L'azione non e' permessa nello stato attuale del task.
    NotAllowed {
        task: String,
        status: TaskStatus,
        op: Op,
    },
    /// L'evento riguarda un tentativo o una sessione che non e' piu' quella del task.
    Stale,
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LifecycleError::NotFound(id) => write!(f, "task not found: {id}"),
            LifecycleError::NotAllowed { task, status, op } => {
                write!(f, "task '{task}' is {}: only {} tasks can be {}", status.as_str(), op.allowed(), op.verb())
            }
            LifecycleError::Stale => f.write_str("the task is no longer in the state this refers to"),
        }
    }
}

impl std::error::Error for LifecycleError {}

impl From<LifecycleError> for String {
    fn from(e: LifecycleError) -> String {
        e.to_string()
    }
}

impl Task {
    /// Applica una transizione. Se e' rifiutata il task non cambia.
    pub fn apply(&mut self, event: TaskEvent, now: i64) -> Result<(), LifecycleError> {
        match event {
            TaskEvent::Start => self.start(now),
            TaskEvent::PrepareStep { attempt, step } => {
                let (current, heartbeat) = self.preparing_of(attempt)?;
                *heartbeat = now;
                if step.is_some() {
                    *current = step;
                }
                Ok(())
            }
            TaskEvent::PrepareEnded { attempt } => {
                let (current, heartbeat) = self.preparing_of(attempt)?;
                *current = None;
                *heartbeat = now;
                Ok(())
            }
            TaskEvent::WorktreeCreated { attempt, worktree } => {
                self.preparing_of(attempt)?;
                self.worktree = Some(worktree);
                Ok(())
            }
            TaskEvent::WorkerSpawned { attempt, session, worktree, note } => {
                self.spawned(attempt, session, now)?;
                if worktree.is_some() {
                    self.worktree = worktree;
                }
                if note.is_some() {
                    self.note = note;
                }
                Ok(())
            }
            TaskEvent::WorkerChanged { session, activity } => {
                match &mut self.state {
                    TaskState::Running(Run { phase: RunPhase::Live { session: s, worker }, .. }) if *s == session => {
                        *worker = activity;
                    }
                    _ => return Err(LifecycleError::Stale),
                }
                self.updated_at = now;
                Ok(())
            }
            TaskEvent::Finish { session, outcome, report, note } => self.finish(session, outcome, report, note, now),
            TaskEvent::Requeue { note } => self.requeue(note, now),
            TaskEvent::Retry => self.retry(now),
            TaskEvent::MarkDone => self.mark_done(now),
            TaskEvent::Reopen { session, attempt, note } => self.reopen(session, attempt, note, now),
            TaskEvent::QuestionAsked { session, text, options } => self.question_asked(&session, text, options, now),
            TaskEvent::QuestionAnswered { session, question_id, answer } => {
                self.question_answered(&session, &question_id, answer, now)
            }
            TaskEvent::AnswerUndelivered { session, question_id } => {
                self.answer_undelivered(&session, &question_id, now)
            }
        }
    }

    fn not_allowed<T>(&self, op: Op) -> Result<T, LifecycleError> {
        Err(LifecycleError::NotAllowed { task: self.id.clone(), status: self.status(), op })
    }

    /// Passo e battito della preparazione del tentativo `attempt`.
    fn preparing_of(&mut self, attempt: u32) -> Result<(&mut Option<String>, &mut i64), LifecycleError> {
        match &mut self.state {
            TaskState::Running(Run { phase: RunPhase::Preparing { step, heartbeat }, .. })
                if self.attempts == attempt =>
            {
                Ok((step, heartbeat))
            }
            _ => Err(LifecycleError::Stale),
        }
    }

    fn start(&mut self, now: i64) -> Result<(), LifecycleError> {
        let TaskState::Queued { previous_attempt } = &mut self.state else {
            return Err(LifecycleError::Stale);
        };
        let run = Run {
            started_at: now,
            previous_attempt: previous_attempt.take(),
            phase: RunPhase::Preparing { step: None, heartbeat: now },
        };
        self.state = TaskState::Running(run);
        self.attempts += 1;
        self.note = None;
        self.last_report = None;
        self.e2e_run = None;
        self.updated_at = now;
        Ok(())
    }

    fn spawned(&mut self, attempt: u32, session: String, now: i64) -> Result<(), LifecycleError> {
        match &mut self.state {
            TaskState::Running(Run { phase, .. })
                if self.attempts == attempt && matches!(phase, RunPhase::Preparing { .. }) =>
            {
                *phase = RunPhase::Live { session, worker: WorkerActivity::Starting };
            }
            _ => return Err(LifecycleError::Stale),
        }
        self.updated_at = now;
        Ok(())
    }

    fn finish(
        &mut self,
        session: Option<String>,
        outcome: Outcome,
        report: Option<Box<TaskReport>>,
        note: Option<String>,
        now: i64,
    ) -> Result<(), LifecycleError> {
        let TaskState::Running(run) = &self.state else {
            return Err(LifecycleError::Stale);
        };
        if let Some(s) = &session {
            if !matches!(&run.phase, RunPhase::Live { session: live, .. } if live == s) {
                return Err(LifecycleError::Stale);
            }
        }
        let finished = Finished { started_at: Some(run.started_at), finished_at: now, report: report.map(|r| *r) };
        if let Some(r) = &finished.report {
            self.last_report = Some(clip_chars(&r.summary, LAST_REPORT_MAX));
        }
        if note.is_some() {
            self.note = note;
        }
        self.state = match outcome {
            Outcome::Done => TaskState::Done(finished),
            Outcome::Failed(Failure { cause, reason }) => TaskState::Failed { finished, cause, reason: Some(reason) },
        };
        self.updated_at = now;
        Ok(())
    }

    fn requeue(&mut self, note: String, now: i64) -> Result<(), LifecycleError> {
        let TaskState::Running(run) = &mut self.state else {
            return Err(LifecycleError::Stale);
        };
        self.state = TaskState::Queued { previous_attempt: run.previous_attempt.take() };
        self.note = Some(note);
        self.updated_at = now;
        Ok(())
    }

    fn retry(&mut self, now: i64) -> Result<(), LifecycleError> {
        let TaskState::Failed { finished, .. } = &mut self.state else {
            return self.not_allowed(Op::Action(TaskAction::Retry));
        };
        let previous = PreviousAttempt { summary: finished.report.take().map(|r| r.summary), note: self.note.take() };
        self.state =
            TaskState::Queued { previous_attempt: (previous != PreviousAttempt::default()).then_some(previous) };
        self.last_report = None;
        self.updated_at = now;
        Ok(())
    }

    fn mark_done(&mut self, now: i64) -> Result<(), LifecycleError> {
        let TaskState::Failed { finished, .. } = &mut self.state else {
            return self.not_allowed(Op::Action(TaskAction::MarkDone));
        };
        let done = Finished { started_at: finished.started_at, finished_at: now, report: finished.report.take() };
        self.state = TaskState::Done(done);
        self.note = None;
        self.updated_at = now;
        Ok(())
    }

    fn reopen(&mut self, session: String, attempt: u32, note: String, now: i64) -> Result<(), LifecycleError> {
        let TaskState::Failed { finished, cause: FailCause::SessionEnded { session: ended }, .. } = &self.state else {
            return Err(LifecycleError::Stale);
        };
        if *ended != session || self.attempts != attempt {
            return Err(LifecycleError::Stale);
        }
        let run = Run {
            started_at: finished.started_at.unwrap_or(now),
            previous_attempt: None,
            phase: RunPhase::Live { session, worker: WorkerActivity::Running },
        };
        self.state = TaskState::Running(run);
        self.note = Some(note);
        self.updated_at = now;
        Ok(())
    }

    /// Aggiunge `line` alla nota, separata da "; ", se non c'e' gia'. `true` se
    /// la nota e' cambiata.
    pub fn append_note(&mut self, line: &str, now: i64) -> bool {
        if self.note.as_deref().is_some_and(|n| n.contains(line)) {
            return false;
        }
        let note = match self.note.take() {
            Some(n) => format!("{n}; {line}"),
            None => line.to_string(),
        };
        self.note = Some(clip_chars(&note, NOTE_MAX));
        self.updated_at = now;
        true
    }

    /// Sostituisce la nota (tagliata a `NOTE_MAX`).
    pub fn replace_note(&mut self, note: &str, now: i64) {
        self.note = Some(clip_chars(note, NOTE_MAX));
        self.updated_at = now;
    }

    /// Le esecuzioni E2E del task: quella in corso e la copia nel report.
    pub fn e2e_runs_mut(&mut self) -> impl Iterator<Item = &mut E2eRun> {
        let in_report = match &mut self.state {
            TaskState::Done(f) | TaskState::Failed { finished: f, .. } => {
                f.report.as_mut().and_then(|r| r.e2e.as_mut())
            }
            TaskState::Queued { .. } | TaskState::Running(_) => None,
        };
        self.e2e_run.iter_mut().chain(in_report)
    }

    /// Il report del tentativo concluso, modificabile (solo per i test).
    #[cfg(test)]
    pub(crate) fn report_mut(&mut self) -> Option<&mut TaskReport> {
        match &mut self.state {
            TaskState::Done(f) | TaskState::Failed { finished: f, .. } => f.report.as_mut(),
            TaskState::Queued { .. } | TaskState::Running(_) => None,
        }
    }
}

#[cfg(test)]
mod tests;
