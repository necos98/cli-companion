//! Passaggio di consegne a un worker: cosa hanno fatto i task da cui dipende
//! (`dependencies`). `get_task` le aggiunge alla risposta, omesse se vuote; il
//! revisore non le riceve (i report li ha gia' in `reviewed_tasks`).
//! Funzioni pure, nessun I/O.

use super::review::reviewed_entry;
use super::{Board, Task, TaskStatus};
use serde_json::Value;

/// Dipendenze consegnate di un task, gia' nella forma di `get_task`.
#[derive(Debug, Default)]
pub struct Handoff {
    dependencies: Vec<Value>,
}

impl Handoff {
    /// Consegne del task `task` di un worker `work`.
    pub fn of_worker(board: &Board, task: &Task) -> Handoff {
        Handoff { dependencies: delivered_dependencies(board, task) }
    }

    /// Niente da consegnare: `get_task` e istruzioni restano quelli di un task senza dipendenze `done`.
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty()
    }

    /// Aggiunge a `out`, un oggetto, i campi presenti.
    pub fn add_to(self, out: &mut Value) {
        if !self.dependencies.is_empty() {
            out["dependencies"] = Value::Array(self.dependencies);
        }
    }
}

/// Le dipendenze dirette `done` di `task`, nell'ordine di `depends_on`, con la
/// forma e i limiti di `reviewed_tasks`.
fn delivered_dependencies(board: &Board, task: &Task) -> Vec<Value> {
    task.depends_on
        .iter()
        .filter_map(|d| board.task(d))
        .filter(|d| d.status() == TaskStatus::Done)
        .map(reviewed_entry)
        .collect()
}

#[cfg(test)]
mod tests;
