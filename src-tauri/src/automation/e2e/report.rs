//! Apertura del report JUnit di un'esecuzione E2E (pulsante "Apri report"
//! della UI). Il percorso non arriva mai dal frontend: lo ricava il backend
//! dall'ultima esecuzione del task o del repository.

use super::manual::last_result;
use crate::automation::boards::workspace_of;
use crate::automation::hooks::fs_path;
use crate::automation::worktrees::{join_path, repo_dir};
use crate::automation::*;
use crate::board::{normalize_repo, E2eRun};
use std::path::PathBuf;

const NO_RUN: &str = "nessuna esecuzione E2E di cui aprire il report";
const REPORT_MISSING: &str = "il report E2E non esiste piu' (la cartella del task e' stata ripulita?)";

/// Cartella in cui e' girata l'esecuzione: il worktree del task, o il checkout
/// del repository (task senza worktree, esecuzione manuale).
fn run_dir<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    task_id: Option<&str>,
    repo: Option<&str>,
) -> Result<(String, E2eRun), String> {
    let Some(id) = task_id else {
        let run = last_result(st, host, &ws.id, repo)?.ok_or_else(|| NO_RUN.to_string())?;
        let repo = repo.map(normalize_repo).transpose()?.flatten();
        return Ok((repo_dir(ws, repo.as_deref()), run));
    };
    let task = st.run(host, |r| r.task_view(&ws.id, id)).ok_or_else(|| "task non trovato".to_string())?.task;
    let run = task.e2e_run.clone().ok_or_else(|| NO_RUN.to_string())?;
    let dir = match &task.worktree {
        Some(w) => w.path.clone(),
        None => repo_dir(ws, task.repo.as_deref()),
    };
    Ok((dir, run))
}

/// File JUnit dell'ultima esecuzione del task `task_id` o, senza, del
/// repository `repo`, come percorso del filesystem di Windows.
pub(in crate::automation) fn report_file<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: Option<&str>,
    repo: Option<&str>,
) -> Result<PathBuf, String> {
    let ws = workspace_of(host, ws_id)?;
    let (dir, run) = run_dir(st, host, &ws, task_id, repo)?;
    let file = join_path(&ws.kind, &dir, &run.junit);
    if !host.path_exists(&ws, &file) {
        return Err(REPORT_MISSING.to_string());
    }
    fs_path(&ws, &file)
}
