//! Stato del ciclo di vita di un task, con i dati che esistono solo in quello
//! stato. Si cambia solo con `Task::apply` (vedi `lifecycle`).

use super::{PreviousAttempt, TaskReport, TaskStatus, WorkerActivity};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Stato del ciclo di vita con i dati che esistono solo in quello stato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Queued {
        /// Esito del tentativo precedente (dopo un "Riprova").
        previous_attempt: Option<PreviousAttempt>,
    },
    Running(Run),
    Done(Finished),
    Failed {
        finished: Finished,
        cause: FailCause,
        /// Perche' e' fallito; `None` per un task fallito prima che si registrasse.
        reason: Option<FailReason>,
    },
}

impl Default for TaskState {
    fn default() -> TaskState {
        TaskState::Queued { previous_attempt: None }
    }
}

impl TaskState {
    pub fn status(&self) -> TaskStatus {
        match self {
            TaskState::Queued { .. } => TaskStatus::Queued,
            TaskState::Running(_) => TaskStatus::Running,
            TaskState::Done(_) => TaskStatus::Done,
            TaskState::Failed { .. } => TaskStatus::Failed,
        }
    }
}

/// Un tentativo in corso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub started_at: i64,
    /// Esito del tentativo precedente (dopo un "Riprova"), letto da `get_task`.
    pub previous_attempt: Option<PreviousAttempt>,
    pub phase: RunPhase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPhase {
    /// Scelto dallo scheduler: l'app prepara worktree e setup, nessun worker ancora.
    Preparing {
        /// Passo di preparazione in corso (setup del repository), da mostrare.
        step: Option<String>,
        /// Ultimo segno di vita della preparazione (ms): il guardiano dell'avvio
        /// lo usa per scoprire un avvio bloccato. All'inizio e' `started_at`.
        heartbeat: i64,
    },
    /// Worker spawnato: il suo tab e' `session`.
    Live { session: String, worker: WorkerActivity },
}

/// Dati di un tentativo concluso.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Inizio dell'ultimo tentativo (assente per un task mai avviato).
    pub started_at: Option<i64>,
    pub finished_at: i64,
    pub report: Option<TaskReport>,
}

/// Perche' un task e' fallito, quando conta per una transizione successiva.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailCause {
    /// Il worker `session` ha chiuso la sessione senza report: se il suo tab
    /// e' ancora aperto e riferisce, il task si riapre (`Reopen`).
    SessionEnded {
        session: String,
    },
    Other,
}

/// Come e perche' un task e' fallito: la causa per la riapertura, il motivo per chi legge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub cause: FailCause,
    pub reason: FailReason,
}

/// Il motivo del fallimento mostrato a utente e agenti: dice se ha senso
/// riprovare. Scritto dalla transizione che porta il task a `failed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FailReason {
    /// Il worker ha chiuso con `complete_task` `failed`.
    WorkerFailed,
    /// Tab chiuso o agente uscito senza report.
    WorkerExited,
    /// Fermato dall'utente.
    Stopped,
    /// L'app si e' riavviata con il task in corso.
    Interrupted,
    /// L'avvio non e' riuscito (git, setup, precondizione, skill).
    StartFailed,
}

#[cfg(test)]
impl TaskState {
    /// Uno stato qualunque in `status`, con i dati vuoti (per costruire i dati di prova).
    pub(crate) fn of(status: TaskStatus) -> TaskState {
        let finished = || Finished { started_at: None, finished_at: 0, report: None };
        match status {
            TaskStatus::Queued => TaskState::default(),
            TaskStatus::Running => TaskState::Running(Run {
                started_at: 0,
                previous_attempt: None,
                phase: RunPhase::Preparing { step: None, heartbeat: 0 },
            }),
            TaskStatus::Done => TaskState::Done(finished()),
            TaskStatus::Failed => TaskState::Failed { finished: finished(), cause: FailCause::Other, reason: None },
        }
    }
}
