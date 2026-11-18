//! Chi aspetta chi: le dipendenze inverse dei task non `done`, per la
//! cancellazione a cascata (`delete_with_dependents` e `TaskView::cascade`) e
//! per i task bloccati da un fallito (`TaskView::blocked_by`).

use super::{Board, Task, TaskStatus};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use ts_rs::TS;

/// Cio' che toglierebbe "Elimina con i dipendenti" su un task.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct CascadeDelete {
    /// Task non `done` che dipendono dal task, direttamente o no (il task escluso).
    pub dependents: Vec<String>,
    /// Tra il task e i suoi dipendenti, quelli `running`: la cascata si rifiuta.
    pub running: Vec<String>,
}

/// Indice delle dipendenze inverse, costruito una volta per board: ogni
/// chiusura costa quanto i task che raggiunge, non un giro sull'intera coda.
pub(super) struct Waiters<'a> {
    tasks: &'a [Task],
    /// Id di un task -> posizioni in coda dei task non `done` che lo aspettano.
    by_dep: HashMap<&'a str, Vec<usize>>,
}

/// Id di un task `queued` -> i task `failed` che lo bloccano.
pub(super) type Blockers<'a> = HashMap<&'a str, Vec<String>>;

impl Board {
    pub(super) fn waiters(&self) -> Waiters<'_> {
        let mut by_dep: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, t) in self.tasks.iter().enumerate().filter(|(_, t)| t.status() != TaskStatus::Done) {
            for d in &t.depends_on {
                by_dep.entry(d.as_str()).or_default().push(i);
            }
        }
        Waiters { tasks: &self.tasks, by_dep }
    }
}

impl<'a> Waiters<'a> {
    /// I task non `done` che dipendono da `id`, anche indirettamente, in ordine
    /// di coda. `id` escluso. Non si attraversa un task `done`: resta com'e'.
    pub(super) fn dependents_of(&self, id: &str) -> Vec<&'a Task> {
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut frontier: Vec<&str> = vec![id];
        while let Some(current) = frontier.pop() {
            for &i in self.by_dep.get(current).into_iter().flatten() {
                if seen.insert(i) {
                    frontier.push(self.tasks[i].id.as_str());
                }
            }
        }
        seen.into_iter().filter_map(|i| self.tasks.get(i)).filter(|t| t.id != id).collect()
    }

    /// Per ogni task `queued` bloccato, i task `failed` da cui dipende (anche
    /// indirettamente), in ordine di coda. Un task `done` nel mezzo spezza la catena.
    pub(super) fn blockers(&self) -> Blockers<'a> {
        let mut blocked = Blockers::new();
        for failed in self.tasks.iter().filter(|t| t.status() == TaskStatus::Failed) {
            for waiting in self.dependents_of(&failed.id).into_iter().filter(|t| t.status() == TaskStatus::Queued) {
                blocked.entry(waiting.id.as_str()).or_default().push(failed.id.clone());
            }
        }
        blocked
    }

    /// Cosa toglierebbe la cascata partita da `task`.
    pub(super) fn cascade_of(&self, task: &Task) -> CascadeDelete {
        let dependents = self.dependents_of(&task.id);
        let running = std::iter::once(task)
            .chain(dependents.iter().copied())
            .filter(|t| t.status() == TaskStatus::Running)
            .map(|t| t.id.clone())
            .collect();
        CascadeDelete { dependents: dependents.into_iter().map(|t| t.id.clone()).collect(), running }
    }
}
