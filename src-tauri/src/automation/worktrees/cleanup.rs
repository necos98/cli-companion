//! Pulizia di un worktree a consegna avvenuta (o task eliminato, o orfana):
//! comandi di pulizia del repository, rimozione del worktree e del branch.

use super::{git, repo_cfg, repo_dir, CleanupTarget};
use crate::automation::hooks;
use crate::automation::*;
use crate::workspaces::DeliveryMode;

/// Esito della pulizia: gli avvisi non bloccano mai.
struct CleanupOutcome {
    removed: bool,
    warnings: Vec<String>,
}

/// Branch di un task di un goal: si cancella (`-D`, il lavoro e' gia' nel goal)
/// solo se e' contenuto in `goal`; altrimenti resta, con un avviso.
fn delete_goal_task_branch<H: Host>(
    host: &H,
    ws: &Workspace,
    root: &str,
    branch: &str,
    goal: &str,
    warnings: &mut Vec<String>,
) {
    let contained = host.git(ws, root, &["merge-base", "--is-ancestor", branch, goal]).map(|o| o.ok).unwrap_or(false);
    if !contained {
        warnings.push(format!("branch {branch} kept: not contained in {goal}"));
    } else if let Err(e) = git(host, ws, root, &["branch", "-D", branch]) {
        warnings.push(format!("branch not deleted: {e}"));
    }
}

/// Esegue i comandi di pulizia del repository (se ci sono), poi rimuove il
/// worktree (con `--force` se l'impostazione lo chiede) e il branch del task:
/// `-D` per `pr` (il merge e' squash/rebase lato server, la consegna l'ha
/// confermata il worker), `-d` altrimenti (non cancella lavoro non unito);
/// sempre `-d` se il target ha `keep_unmerged`. Per un task di un goal il branch
/// si cancella (`-D`) solo se e' contenuto in `goal/<id>`, altrimenti resta.
fn cleanup_worktree<H: Host>(host: &H, ws: &Workspace, target: &CleanupTarget) -> CleanupOutcome {
    let (wt, mut warnings) = (&target.wt, Vec::new());
    let root = repo_dir(ws, target.repo.as_deref());
    let cfg = repo_cfg(ws, target.repo.as_deref()).cloned().unwrap_or_default();
    if host.dir_exists(ws, &wt.path) {
        if !cfg.cleanup.is_empty() {
            let vars = hooks::HookVars {
                repo_dir: &root,
                worktree: &wt.path,
                task_id: &target.task_id,
                branch: &wt.branch,
                base: &wt.base,
            };
            warnings.extend(hooks::run_cleanup(host, ws, &cfg, &vars));
        }
        let mut args = vec!["worktree", "remove"];
        if cfg.remove_force {
            args.push("--force");
        }
        args.push(&wt.path);
        if let Err(e) = git(host, ws, &root, &args) {
            warnings.push(format!("worktree not removed: {e}"));
            return CleanupOutcome { removed: false, warnings };
        }
    } else {
        let _ = git(host, ws, &root, &["worktree", "prune"]);
    }
    match &target.goal_branch {
        Some(goal) => delete_goal_task_branch(host, ws, &root, &wt.branch, goal, &mut warnings),
        None => {
            let flag = if ws.delivery.mode == DeliveryMode::Pr && !target.keep_unmerged { "-D" } else { "-d" };
            if let Err(e) = git(host, ws, &root, &["branch", flag, &wt.branch]) {
                warnings.push(format!("branch not deleted: {e}"));
            }
        }
    }
    CleanupOutcome { removed: true, warnings }
}

/// Pulizia di un worktree (thread del timer). Una sola per volta per task, e
/// solo se serve ancora: chi l'ha chiesta (chiusura del tab, riallineamento
/// periodico) puo' essere arrivato prima di un'altra richiesta uguale.
pub(in crate::automation) fn run_cleanup<H: Host>(st: &AutomationState, host: &H, target: CleanupTarget) {
    let Some(ws) = target.workspace.clone().or_else(|| host.workspace(&target.ws_id)) else {
        log::warn!("worktree {}: workspace {} not found, nothing cleaned", target.wt.path, target.ws_id);
        return;
    };
    let Some(_turn) = st.reconciler.begin_cleanup(&target.ws_id, &target.task_id) else {
        return;
    };
    if !st.run(host, |r| r.cleanup_needed(&target)) {
        return;
    }
    let outcome = cleanup_worktree(host, &ws, &target);
    for w in &outcome.warnings {
        log::warn!("worktree {} of task {}/{}: {w}", target.wt.path, target.ws_id, target.task_id);
    }
    st.run(host, |r| r.cleanup_done(&target.ws_id, &target.task_id, outcome.removed, &outcome.warnings, now_millis()));
}
