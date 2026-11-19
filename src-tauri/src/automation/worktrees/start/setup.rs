//! Setup di un worktree appena creato: configurazione del repository e hook
//! (file ignorati, comandi).

use super::StartJob;
use crate::automation::hooks;
use crate::automation::worktrees::repo_cfg;
use crate::automation::*;
use crate::board::{repo_key, WorktreeInfo};

/// Configurazione del repository se il worktree del task va preparato: un
/// task `isolated` legge solo il codice e non ha setup.
pub(super) fn setup_cfg(job: &StartJob) -> Option<&crate::workspaces::RepoConfig> {
    let task = &job.task;
    repo_cfg(&job.ws, task.repo.as_deref()).filter(|c| c.has_setup() && !task.kind.isolated())
}

/// Setup di un worktree appena creato (file ignorati, comandi), se il
/// repository ne ha uno. Ogni passo e' pubblicato e fa da battito del guardiano.
pub(super) fn setup_worktree<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    wt: &WorktreeInfo,
) -> Result<Vec<String>, String> {
    let (ws, task) = (&job.ws, &job.task);
    let Some(cfg) = setup_cfg(job) else {
        return Ok(Vec::new());
    };
    let key = repo_key(task.repo.as_deref());
    let ctx = hooks::SetupCtx {
        ws,
        cfg,
        repo: &key,
        repo_dir: dir,
        worktree: &wt.path,
        task_id: &task.id,
        branch: &wt.branch,
        base: &wt.base,
    };
    let report = |label: &str| {
        st.run(host, |r| r.touch_preparing(&ws.id, &task.id, job.attempt, Some(label), now_millis(), true));
    };
    hooks::run_setup(host, &ctx, &report)
}
