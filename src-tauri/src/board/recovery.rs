//! Recupero dopo un riavvio dell'app: separato dalle migrazioni dello schema
//! (quelle riguardano il formato del file, questo lo stato dei task) e
//! applicato a ogni caricamento, anche di un file gia' attuale.

use super::{Board, FailCause, FailReason, Failure, Outcome, TaskEvent, TaskStatus, INTERRUPTED_NOTE};

impl Board {
    /// Dopo un riavvio nessun worker e' piu' vivo ne' nessuna esecuzione E2E e'
    /// in corso: ogni task `running` fallisce come interrotto (`finished_at` =
    /// `now`) e ogni E2E in corso, anche la copia nel report, e' interrotta.
    /// Ritorna quanti task sono stati recuperati; chiamarlo di nuovo non fa nulla.
    pub fn recover_after_restart(&mut self, now: i64) -> usize {
        let mut recovered = 0;
        for task in &mut self.tasks {
            if task.status() == TaskStatus::Running {
                let interrupted = TaskEvent::Finish {
                    session: None,
                    outcome: Outcome::Failed(Failure { cause: FailCause::Other, reason: FailReason::Interrupted }),
                    report: None,
                    note: Some(INTERRUPTED_NOTE.to_string()),
                };
                match task.apply(interrupted, now) {
                    Ok(()) => recovered += 1,
                    Err(e) => log::error!("board {}, task {}: recupero impossibile: {e}", self.workspace_id, task.id),
                }
            }
            task.interrupt_e2e();
        }
        recovered
    }
}

#[cfg(test)]
mod tests;
