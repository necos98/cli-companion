//! Passaggio di consegne a un worker (docs/GOALS.MD, "Passaggio di consegne"):
//! cosa hanno fatto i task da cui dipende (`dependencies`) e il contesto del
//! suo goal (`goal`). `get_task` li aggiunge alla risposta, omessi se vuoti;
//! il revisore riceve solo il `goal` (i report li ha gia' in `reviewed_tasks`).
//! Funzioni pure, nessun I/O.

use super::review::reviewed_entry;
use super::{Board, Goal, Task, TaskStatus};
use serde_json::{json, Value};

/// Dipendenze consegnate e goal di un task, gia' nella forma di `get_task`.
#[derive(Debug, Default)]
pub struct Handoff {
    dependencies: Vec<Value>,
    goal: Option<Value>,
}

impl Handoff {
    /// Consegne del task `task` di un worker `work`.
    pub fn of_worker(board: &Board, task: &Task) -> Handoff {
        Handoff { dependencies: delivered_dependencies(board, task), goal: goal_context(board, task) }
    }

    /// Consegne del task `task` di un revisore: solo il goal.
    pub fn of_reviewer(board: &Board, task: &Task) -> Handoff {
        Handoff { dependencies: Vec::new(), goal: goal_context(board, task) }
    }

    /// Niente da consegnare: `get_task` e istruzioni restano quelli di un task sciolto.
    pub fn is_empty(&self) -> bool {
        self.dependencies.is_empty() && self.goal.is_none()
    }

    /// Aggiunge a `out`, un oggetto, i campi presenti.
    pub fn add_to(self, out: &mut Value) {
        if !self.dependencies.is_empty() {
            out["dependencies"] = Value::Array(self.dependencies);
        }
        if let Some(goal) = self.goal {
            out["goal"] = goal;
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

/// `{ title, request, done_when, decisions }` del goal del task, se ne ha uno.
fn goal_context(board: &Board, task: &Task) -> Option<Value> {
    let goal: &Goal = board.goal(task.goal()?)?;
    let decisions: Vec<Value> = goal.decisions.iter().map(|d| json!({ "source": d.source, "text": d.text })).collect();
    Some(json!({
        "title": goal.title,
        "request": goal.request,
        "done_when": goal.done_when,
        "decisions": decisions,
    }))
}

#[cfg(test)]
mod tests;
