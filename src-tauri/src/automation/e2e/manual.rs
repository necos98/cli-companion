//! Esecuzione a mano degli E2E di un repository (dalla UI), fuori da ogni
//! task: nella cartella del repository, una alla volta per repository. L'esito
//! resta in memoria; l'evento `e2e-update` segnala partenza e fine.

use super::job::{launch, RunTarget};
use super::runs::{ClaimError, Owner};
use super::E2eEvent;
use crate::automation::boards::workspace_of;
use crate::automation::worktrees::{ensure_repos, repo_dir};
use crate::automation::*;
use crate::board::{normalize_repo, repo_key, E2eRun};

const NOT_CONFIGURED: &str = "E2E non configurati per il repository";
const ALREADY_RUNNING: &str = "esecuzione E2E gia' in corso per questo repository";
const NO_FREE_PORT: &str = "nessuna porta locale libera per gli E2E: riprova fra un attimo";

fn claim_error(e: ClaimError) -> String {
    match e {
        ClaimError::Busy(_) => ALREADY_RUNNING.to_string(),
        ClaimError::NoFreePort => NO_FREE_PORT.to_string(),
    }
}

/// Avvia gli E2E del repository `repo` (`None` = la root). Ritorna
/// l'esecuzione `running`.
pub(in crate::automation) fn run_repo<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    repo: Option<&str>,
) -> Result<E2eRun, String> {
    let ws = workspace_of(host, ws_id)?;
    let repo = repo.map(normalize_repo).transpose()?.flatten();
    ensure_repos(host, &ws, [repo.as_deref()])?;
    let key = repo_key(repo.as_deref());
    let repo_cfg = ws.repos.get(&key).cloned().unwrap_or_default();
    let e2e = repo_cfg.e2e.clone().ok_or_else(|| NOT_CONFIGURED.to_string())?;
    let dir = repo_dir(&ws, repo.as_deref());
    let target = RunTarget {
        ws,
        dir: dir.clone(),
        repo_dir: dir,
        repo_cfg,
        task_id: String::new(),
        worktree: None,
        owner: Owner::Repo { ws: ws_id.to_string(), repo: key.clone() },
    };
    launch(
        st,
        host,
        target,
        e2e,
        |run| {
            st.e2e.set_last_manual(ws_id, &key, run.clone());
            host.emit_e2e(&E2eEvent { workspace_id: ws_id.to_string(), repo: key.clone(), run: run.clone() });
            Ok(())
        },
        claim_error,
    )
}

/// Ultimo esito manuale del repository, se c'e' (solo in memoria).
pub(in crate::automation) fn last_result<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    repo: Option<&str>,
) -> Result<Option<E2eRun>, String> {
    host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
    let repo = repo.map(normalize_repo).transpose()?.flatten();
    Ok(st.e2e.last_manual(ws_id, &repo_key(repo.as_deref())))
}
