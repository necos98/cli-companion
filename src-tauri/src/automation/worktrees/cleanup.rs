//! Pulizia di un worktree a consegna avvenuta (o task eliminato, o orfana):
//! comandi di pulizia del repository, rimozione del worktree e del branch.

use super::{git, repo_cfg, repo_dir, CleanupTarget};
use crate::automation::hooks;
use crate::automation::*;
use crate::workspaces::{DeliveryMode, RepoConfig};

/// Prefisso dei branch dei goal, funzione eliminata: quei branch non si cancellano.
const LEGACY_GOAL_BRANCH_PREFIX: &str = "goal/";

/// Esito della pulizia: gli avvisi non bloccano mai.
struct CleanupOutcome {
    removed: bool,
    warnings: Vec<String>,
}

/// Rimuove il worktree `path` del repository `root`: se esiste, prima i comandi di
/// pulizia del repository (con `vars`, se dati), poi `git worktree remove` (con
/// `--force` se l'impostazione lo chiede); se non c'e' piu' ne ripulisce solo
/// la registrazione. `false` (con un avviso) se git non l'ha tolto.
pub(super) fn remove_worktree<H: Host>(
    host: &H,
    ws: &Workspace,
    root: &str,
    cfg: &RepoConfig,
    path: &str,
    vars: Option<&hooks::HookVars>,
    warnings: &mut Vec<String>,
) -> bool {
    if !host.dir_exists(ws, path) {
        let _ = git(host, ws, root, &["worktree", "prune"]);
        return true;
    }
    if let Some(vars) = vars.filter(|_| !cfg.cleanup.is_empty()) {
        warnings.extend(hooks::run_cleanup(host, ws, cfg, vars));
    }
    let mut args = vec!["worktree", "remove"];
    if cfg.remove_force {
        args.push("--force");
    }
    args.push(path);
    match git(host, ws, root, &args) {
        Ok(_) => true,
        Err(e) => {
            warnings.push(format!("worktree not removed: {e}"));
            false
        }
    }
}

/// Esegue i comandi di pulizia del repository (se ci sono), poi rimuove il
/// worktree (con `--force` se l'impostazione lo chiede) e il branch del task:
/// `-D` per `pr` (il merge e' squash/rebase lato server, la consegna l'ha
/// confermata il worker), `-d` altrimenti (non cancella lavoro non unito);
/// sempre `-d` se il target ha `keep_unmerged`.
fn cleanup_worktree<H: Host>(host: &H, ws: &Workspace, target: &CleanupTarget) -> CleanupOutcome {
    let (wt, mut warnings) = (&target.wt, Vec::new());
    let root = repo_dir(ws, target.repo.as_deref());
    let cfg = repo_cfg(ws, target.repo.as_deref()).cloned().unwrap_or_default();
    let vars = hooks::HookVars {
        repo_dir: &root,
        worktree: &wt.path,
        task_id: &target.task_id,
        branch: &wt.branch,
        base: &wt.base,
    };
    if !remove_worktree(host, ws, &root, &cfg, &wt.path, Some(&vars), &mut warnings) {
        return CleanupOutcome { removed: false, warnings };
    }
    // Difesa: un worktree rimasto da una versione con i goal puo' stare su un
    // branch `goal/<id>`, che puo' contenere lavoro integrato e mai consegnato.
    // Il worktree si toglie, il branch non si cancella mai.
    if wt.branch.starts_with(LEGACY_GOAL_BRANCH_PREFIX) {
        warnings.push(format!("branch {} kept: it is a goal branch left by an older version", wt.branch));
        return CleanupOutcome { removed: true, warnings };
    }
    let flag = if ws.delivery.mode == DeliveryMode::Pr && !target.keep_unmerged { "-D" } else { "-d" };
    if let Err(e) = git(host, ws, &root, &["branch", flag, &wt.branch]) {
        warnings.push(format!("branch not deleted: {e}"));
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
