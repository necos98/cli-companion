//! Quali task impediscono di cambiare la cartella di un workspace: quelli in
//! esecuzione e quelli con un worktree ancora presente, il cui percorso
//! assoluto punta alla cartella attuale. Puro: decide solo, senza I/O.

use super::{Task, TaskStatus};

/// I task che legano il workspace alla sua cartella attuale, nell'ordine della coda.
pub fn relocation_blockers(tasks: &[Task]) -> Vec<&Task> {
    tasks.iter().filter(|t| t.status() == TaskStatus::Running || t.worktree.is_some()).collect()
}

/// Il rifiuto per l'utente, se qualche task blocca il cambio: elenca i titoli
/// e dice cosa fare per sbloccarlo.
pub fn relocation_refusal(blockers: &[&Task]) -> Option<String> {
    if blockers.is_empty() {
        return None;
    }
    let titles: Vec<String> = blockers.iter().map(|t| format!("«{}»", t.title)).collect();
    Some(format!(
        "Impossibile cambiare cartella: questi task usano ancora quella attuale: {}. \
         Fermali, eliminali o segnali come fatti, poi riprova.",
        titles.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::WorktreeInfo;

    fn task(id: &str, status: TaskStatus, worktree: bool) -> Task {
        let mut t = Task { id: id.to_string(), title: format!("Task {id}"), ..Task::default() };
        t.set_status(status);
        t.worktree = worktree.then(|| WorktreeInfo {
            path: format!("C:\\p\\.worktrees\\{id}"),
            branch: format!("task/{id}"),
            base: "main".to_string(),
            setup_done: true,
        });
        t
    }

    fn ids<'a>(tasks: &[&'a Task]) -> Vec<&'a str> {
        tasks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn running_tasks_and_tasks_with_a_worktree_block_the_change() {
        let tasks = vec![
            task("queued", TaskStatus::Queued, false),
            task("running", TaskStatus::Running, true),
            task("running-no-git", TaskStatus::Running, false),
            task("failed-with-worktree", TaskStatus::Failed, true),
            task("done-cleaned", TaskStatus::Done, false),
            task("done-with-worktree", TaskStatus::Done, true),
        ];
        assert_eq!(
            ids(&relocation_blockers(&tasks)),
            ["running", "running-no-git", "failed-with-worktree", "done-with-worktree"]
        );
    }

    #[test]
    fn a_board_without_blockers_allows_the_change() {
        assert!(relocation_blockers(&[]).is_empty());
        let tasks = vec![task("q", TaskStatus::Queued, false), task("d", TaskStatus::Done, false)];
        let blockers = relocation_blockers(&tasks);
        assert!(blockers.is_empty());
        assert_eq!(relocation_refusal(&blockers), None);
    }

    #[test]
    fn the_refusal_lists_the_titles_and_what_to_do() {
        let tasks = vec![task("a", TaskStatus::Running, false), task("b", TaskStatus::Failed, true)];
        let msg = relocation_refusal(&relocation_blockers(&tasks)).unwrap();
        assert!(msg.contains("«Task a», «Task b»"), "{msg}");
        assert!(msg.contains("Fermali, eliminali o segnali come fatti"), "{msg}");
    }
}
