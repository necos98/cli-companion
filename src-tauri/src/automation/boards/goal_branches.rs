//! Branch dei goal lato automazione: la registrazione all'avvio del primo task
//! e "Elimina branch" di un goal abbandonato. Git gira fuori dal lock.

use super::commands::workspace_of;
use crate::automation::worktrees::{git, repo_dir, repo_of_key};
use crate::automation::*;
use crate::board::{goal_branch_name, GoalBranch};

impl Registry {
    /// Registra il branch del goal creato all'avvio di un suo task; ritorna la
    /// voce che resta (vedi `Board::record_goal_branch`).
    pub fn record_goal_branch(
        &mut self,
        ws: &str,
        goal_id: &str,
        repo_key: &str,
        branch: GoalBranch,
        now: i64,
    ) -> Option<GoalBranch> {
        let stored = self.boards.get_mut(ws)?.record_goal_branch(goal_id, repo_key, branch, now)?;
        self.mark_dirty(ws);
        Some(stored)
    }

    /// I branch che "Elimina branch" toglie (vedi `Board::abandoned_goal_branches`).
    pub fn abandoned_goal_branches(&self, ws: &str, goal_id: &str) -> Result<Vec<(String, GoalBranch)>, String> {
        let board = self.boards.get(ws).ok_or_else(|| format!("unknown goal: {goal_id}"))?;
        board.abandoned_goal_branches(goal_id)
    }

    /// Dimentica i branch dei repository `keys`, cancellati da chi chiama.
    pub fn forget_goal_branches(&mut self, ws: &str, goal_id: &str, keys: &[String], now: i64) {
        if let Some(b) = self.boards.get_mut(ws) {
            b.forget_goal_branches(goal_id, keys, now);
            self.mark_dirty(ws);
        }
    }
}

/// Cancella `branch` (`git branch -D`) nel repository `dir`; se non c'e' piu'
/// vale come cancellato. Git rifiuta, per esempio, un branch in checkout.
fn delete_branch<H: Host>(host: &H, ws: &Workspace, dir: &str, branch: &str) -> Result<(), String> {
    let ref_name = format!("refs/heads/{branch}");
    let exists = host.git(ws, dir, &["rev-parse", "--verify", "--quiet", &ref_name]).map(|o| o.ok).unwrap_or(false);
    if !exists {
        return Ok(());
    }
    git(host, ws, dir, &["branch", "-D", branch]).map(|_| ())
}

/// "Elimina branch": cancella i branch `goal/<id>` di un goal abbandonato, in
/// ogni suo repository. Quelli cancellati si dimenticano anche se un altro
/// fallisce; i fallimenti tornano in un errore solo.
pub(in crate::automation) fn goal_delete_branches<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    goal_id: &str,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    let branches = st.run(host, |r| r.abandoned_goal_branches(ws_id, goal_id))?;
    // Il nome si ricostruisce dall'id: mai la stringa letta dal file.
    let name = goal_branch_name(goal_id);
    let (mut deleted, mut failures) = (Vec::new(), Vec::new());
    for (key, _) in branches {
        let dir = repo_dir(&ws, repo_of_key(&key));
        match delete_branch(host, &ws, &dir, &name) {
            Ok(()) => deleted.push(key),
            Err(e) => failures.push(format!("{key}: {e}")),
        }
    }
    st.run(host, |r| r.forget_goal_branches(ws_id, goal_id, &deleted, now_millis()));
    if failures.is_empty() {
        st.view(host, ws_id)
    } else {
        Err(format!("branch {name} not deleted everywhere: {}", failures.join("; ")))
    }
}
