//! Pulizia di un task di consegna: il suo worktree sta sul branch del goal, in
//! ogni repository da consegnare, quindi non passa dalla pulizia generica (che
//! cancellerebbe il branch). Si rimuovono i worktree `<repo>/.worktrees/<task>` di
//! ogni repository del goal e, solo se il goal e' stato consegnato, i suoi branch
//! locali `goal/<id>`: quelli cancellati si dimenticano nella board. Git gira su
//! un thread dell'host, mai sotto il lock del registro.

use super::cleanup::remove_worktree;
use super::{join_path, repo_cfg, repo_dir, repo_of_key, WORKTREES_DIR};
use crate::automation::boards::delete_branch;
use crate::automation::hooks::HookVars;
use crate::automation::*;
use crate::board::{goal_branch_name, repo_key};

/// Cosa ripulire di un task di consegna.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryCleanup {
    pub ws_id: String,
    pub goal_id: String,
    pub task_id: String,
    /// I repository del goal (chiavi `repo_key`): in ognuno c'e' un worktree.
    pub repos: Vec<String>,
    /// Il worktree registrato nel task, l'unico con il setup del repository: i
    /// comandi di pulizia girano solo li'.
    pub main: Option<WorktreeInfo>,
    /// Il goal e' stato consegnato: i branch `goal/<id>` locali si cancellano.
    pub delete_branches: bool,
    /// Niente da aspettare: nessun tab da chiudere (consegna senza worker).
    pub immediate: bool,
    /// Il workspace non e' piu' nello store: la pulizia usa questa copia.
    pub workspace: Option<Workspace>,
}

impl DeliveryCleanup {
    /// La pulizia del task `task` del workspace `ws_id`, se e' di consegna e
    /// appartiene a un goal. I repository sono quelli dei branch del goal in
    /// `board` (che puo' non averlo piu': resta il repository del task).
    pub fn of_task(
        ws_id: &str,
        task: &Task,
        board: Option<&Board>,
        delete_branches: bool,
        workspace: Option<&Workspace>,
    ) -> Option<Self> {
        let goal_id = task.goal().filter(|_| task.kind == TaskKind::Deliver)?;
        let mut repos: Vec<String> =
            board.and_then(|b| b.goal(goal_id)).map(|g| g.branches.keys().cloned().collect()).unwrap_or_default();
        if repos.is_empty() {
            repos.push(repo_key(task.repo.as_deref()));
        }
        Some(Self {
            ws_id: ws_id.to_string(),
            goal_id: goal_id.to_string(),
            task_id: task.id.clone(),
            repos,
            main: task.worktree.clone(),
            delete_branches,
            immediate: false,
            workspace: workspace.cloned(),
        })
    }
}

/// Esegue la pulizia (thread del timer). Una sola per volta per task. Un
/// worktree che git non toglie lascia il suo branch; gli avvisi non bloccano mai.
pub(in crate::automation) fn run_delivery_cleanup<H: Host>(st: &AutomationState, host: &H, cleanup: DeliveryCleanup) {
    let Some(ws) = cleanup.workspace.clone().or_else(|| host.workspace(&cleanup.ws_id)) else {
        log::warn!("delivery {}/{}: workspace not found, nothing cleaned", cleanup.ws_id, cleanup.task_id);
        return;
    };
    let Some(_turn) = st.reconciler.begin_cleanup(&cleanup.ws_id, &cleanup.task_id) else {
        return;
    };
    let name = goal_branch_name(&cleanup.goal_id);
    let (mut warnings, mut forgotten, mut all_removed) = (Vec::new(), Vec::new(), true);
    for key in &cleanup.repos {
        let root = repo_dir(&ws, repo_of_key(key));
        let path = join_path(ws.kind, &root, &format!("{WORKTREES_DIR}/{}", cleanup.task_id));
        let cfg = repo_cfg(&ws, repo_of_key(key)).cloned().unwrap_or_default();
        let vars = cleanup.main.as_ref().filter(|m| m.path == path).map(|m| HookVars {
            repo_dir: &root,
            worktree: &m.path,
            task_id: &cleanup.task_id,
            branch: &m.branch,
            base: &m.base,
        });
        if !remove_worktree(host, &ws, &root, &cfg, &path, vars.as_ref(), &mut warnings) {
            all_removed = false;
            warnings.push(format!("{key}: branch {name} kept: its worktree is still there"));
        } else if cleanup.delete_branches {
            match delete_branch(host, &ws, &root, &name) {
                Ok(()) => forgotten.push(key.clone()),
                Err(e) => warnings.push(format!("{key}: branch {name} not deleted: {e}")),
            }
        }
    }
    for w in &warnings {
        log::warn!("delivery {}/{}: {w}", cleanup.ws_id, cleanup.task_id);
    }
    st.run(host, |r| r.delivery_cleanup_done(&cleanup, all_removed, &forgotten, &warnings, now_millis()));
}
