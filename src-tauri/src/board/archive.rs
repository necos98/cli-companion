//! Archivio dei task conclusi: la board tiene gli ultimi `MAX_FINISHED_KEPT`
//! task `done` e `failed`, i piu' vecchi escono (li scrive su disco chi
//! possiede il file, vedi `persist::archive_line`). Qui solo la scelta, pura.

use super::{Board, Goal, Task, TaskStatus};
use std::collections::HashSet;

/// Task conclusi (`done` e `failed`) che restano sulla board.
pub const MAX_FINISHED_KEPT: usize = 200;

impl Board {
    /// Toglie dalla board i task conclusi piu' vecchi (per `finished_at`)
    /// oltre i `MAX_FINISHED_KEPT` piu' recenti e li ritorna dal piu' vecchio.
    /// Restano al loro posto i task con un worktree da pulire, quelli da cui
    /// dipende un task non concluso o una proposta pendente e quelli di un goal
    /// aperto (i goal chiusi si archiviano a parte: `goal`). I riferimenti ai
    /// task tolti spariscono dai `depends_on` degli altri.
    pub fn take_archivable(&mut self) -> Vec<Task> {
        let gone = self.archivable();
        if gone.is_empty() {
            return Vec::new();
        }
        let mut slots: Vec<Option<Task>> = std::mem::take(&mut self.tasks).into_iter().map(Some).collect();
        let archived: Vec<Task> = gone.iter().filter_map(|&i| slots[i].take()).collect();
        self.tasks = slots.into_iter().flatten().collect();
        let ids: HashSet<&str> = archived.iter().map(|t| t.id.as_str()).collect();
        for task in &mut self.tasks {
            task.depends_on.retain(|d| !ids.contains(d.as_str()));
        }
        archived
    }

    /// Rimette in fondo i task e i goal che l'archivio ha rifiutato, se la
    /// board non li ha gia': non si perdono e non si duplicano.
    pub fn restore_archived(&mut self, tasks: Vec<Task>, goals: Vec<Goal>) {
        for t in tasks {
            if self.task(&t.id).is_none() {
                self.tasks.push(t);
            }
        }
        for g in goals {
            if self.goal(&g.id).is_none() {
                self.goals.push(g);
            }
        }
    }

    /// Indici dei task da archiviare, dal piu' vecchio.
    fn archivable(&self) -> Vec<usize> {
        let mut finished: Vec<(i64, usize)> =
            self.tasks.iter().enumerate().filter_map(|(i, t)| Some((t.finished_at()?, i))).collect();
        let Some(surplus) = finished.len().checked_sub(MAX_FINISHED_KEPT).filter(|n| *n > 0) else {
            return Vec::new();
        };
        finished.sort_unstable();
        let needed = self.needed_ids();
        finished[..surplus]
            .iter()
            .map(|&(_, i)| i)
            .map(|i| (i, &self.tasks[i]))
            .filter(|(_, t)| t.worktree.is_none() && !needed.contains(t.id.as_str()) && !self.in_open_goal(t))
            .map(|(i, _)| i)
            .collect()
    }

    /// Id da cui dipende ancora qualcosa: un task non `done` o una proposta pendente.
    fn needed_ids(&self) -> HashSet<&str> {
        let from_tasks = self.tasks.iter().filter(|t| t.status() != TaskStatus::Done).flat_map(|t| &t.depends_on);
        let from_proposals = self.proposals.iter().flat_map(|p| &p.tasks).flat_map(|s| &s.depends_on);
        from_tasks.chain(from_proposals).map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests;
