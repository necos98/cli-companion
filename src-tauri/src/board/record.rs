//! Formato sul filo del task (file salvato e frontend): un record piatto con
//! `status` e i campi che dipendono dallo stato (`worker_status`, `session`,
//! `report`, `started_at`, ...). Il modello in memoria e' `TaskState`; qui si
//! converte nei due versi. Il record letto e' tollerante: le combinazioni
//! impossibili (un `done` con una sessione) si normalizzano, i campi che non
//! appartengono allo stato cadono.

use super::{
    E2eRun, FailCause, FailReason, Finished, PreviousAttempt, Run, RunPhase, Task, TaskKind, TaskQuestion, TaskReport,
    TaskState, TaskStatus, WorkerActivity, WorkerStatus, WorktreeInfo,
};
use crate::agents::Effort;
use crate::bots::SkillRef;
use serde::{Deserialize, Serialize, Serializer};
use ts_rs::{Config, TypeVisitor, TS};

/// Il task come si legge dal filo (proprietario dei dati).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct TaskRecord {
    id: String,
    title: String,
    task: String,
    depends_on: Vec<String>,
    repo: Option<String>,
    agent: String,
    model: Option<String>,
    bot: Option<String>,
    effort: Option<Effort>,
    skill: Option<SkillRef>,
    kind: TaskKind,
    e2e: bool,
    no_git: bool,
    status: TaskStatus,
    fail_reason: Option<FailReason>,
    worker_status: Option<WorkerStatus>,
    session: Option<String>,
    attempts: u32,
    worktree: Option<WorktreeInfo>,
    last_report: Option<String>,
    note: Option<String>,
    report: Option<TaskReport>,
    e2e_run: Option<E2eRun>,
    questions: Vec<TaskQuestion>,
    previous_attempt: Option<PreviousAttempt>,
    created_at: i64,
    updated_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

impl TaskRecord {
    fn finished(&mut self) -> Finished {
        Finished {
            started_at: self.started_at,
            finished_at: self.finished_at.unwrap_or(self.updated_at),
            report: self.report.take(),
        }
    }

    fn state(&mut self) -> TaskState {
        match self.status {
            TaskStatus::Queued => TaskState::Queued { previous_attempt: self.previous_attempt.take() },
            TaskStatus::Running => {
                let started_at = self.started_at.unwrap_or(self.updated_at);
                let phase = match self.session.take() {
                    Some(session) => RunPhase::Live {
                        session,
                        worker: self.worker_status.and_then(WorkerStatus::activity).unwrap_or(WorkerActivity::Running),
                    },
                    None => RunPhase::Preparing { step: None, heartbeat: started_at },
                };
                TaskState::Running(Run { started_at, previous_attempt: self.previous_attempt.take(), phase })
            }
            TaskStatus::Done => TaskState::Done(self.finished()),
            TaskStatus::Failed => {
                TaskState::Failed { finished: self.finished(), cause: FailCause::Other, reason: self.fail_reason }
            }
        }
    }
}

impl From<TaskRecord> for Task {
    fn from(mut r: TaskRecord) -> Task {
        let state = r.state();
        Task {
            id: r.id,
            title: r.title,
            task: r.task,
            depends_on: r.depends_on,
            repo: r.repo,
            agent: r.agent,
            model: r.model,
            bot: r.bot,
            effort: r.effort,
            skill: r.skill,
            kind: r.kind,
            e2e: r.e2e,
            no_git: r.no_git,
            state,
            attempts: r.attempts,
            last_report: r.last_report,
            note: r.note,
            worktree: r.worktree,
            e2e_run: r.e2e_run,
            questions: r.questions,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

/// Il task come si scrive sul filo (prende in prestito: niente copia dei testi).
/// L'ordine dei campi e' quello del file e del frontend.
#[derive(Serialize, TS)]
struct TaskRecordRef<'a> {
    id: &'a str,
    title: &'a str,
    task: &'a str,
    depends_on: &'a [String],
    repo: &'a Option<String>,
    agent: &'a str,
    model: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<String>", optional)]
    bot: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<Effort>", optional)]
    effort: &'a Option<Effort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<SkillRef>", optional)]
    skill: &'a Option<SkillRef>,
    kind: TaskKind,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    e2e: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    no_git: bool,
    status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    fail_reason: Option<FailReason>,
    worker_status: Option<WorkerStatus>,
    session: Option<&'a str>,
    attempts: u32,
    worktree: &'a Option<WorktreeInfo>,
    last_report: &'a Option<String>,
    note: &'a Option<String>,
    report: Option<&'a TaskReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(as = "Option<E2eRun>", optional)]
    e2e_run: &'a Option<E2eRun>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    #[ts(as = "Option<Vec<TaskQuestion>>", optional)]
    questions: &'a [TaskQuestion],
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    previous_attempt: Option<&'a PreviousAttempt>,
    created_at: i64,
    updated_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    /// Passo di preparazione in corso: solo in uscita, mai riletto.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    preparing: Option<&'a str>,
}

/// Per ts-rs il task e' il suo record sul filo: lo stato e' privato e `status`,
/// `session`, `report`... non sono campi della struct. `TaskView` lo appiattisce.
impl TS for Task {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(cfg: &Config) -> String {
        <TaskRecordRef<'static> as TS>::name(cfg)
    }

    fn inline(cfg: &Config) -> String {
        <TaskRecordRef<'static> as TS>::inline(cfg)
    }

    fn inline_flattened(cfg: &Config) -> String {
        <TaskRecordRef<'static> as TS>::inline_flattened(cfg)
    }

    fn visit_dependencies(visitor: &mut impl TypeVisitor)
    where
        Self: 'static,
    {
        <TaskRecordRef<'static> as TS>::visit_dependencies(visitor);
    }
}

impl Serialize for Task {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TaskRecordRef {
            id: &self.id,
            title: &self.title,
            task: &self.task,
            depends_on: &self.depends_on,
            repo: &self.repo,
            agent: &self.agent,
            model: &self.model,
            bot: &self.bot,
            effort: &self.effort,
            skill: &self.skill,
            kind: self.kind,
            e2e: self.e2e,
            no_git: self.no_git,
            status: self.status(),
            fail_reason: self.fail_reason(),
            worker_status: self.worker_status(),
            session: self.session(),
            attempts: self.attempts,
            worktree: &self.worktree,
            last_report: &self.last_report,
            note: &self.note,
            report: self.report(),
            e2e_run: &self.e2e_run,
            questions: &self.questions,
            previous_attempt: self.previous_attempt(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            started_at: self.started_at(),
            finished_at: self.finished_at(),
            preparing: self.preparing().and_then(|(step, _)| step),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests;
