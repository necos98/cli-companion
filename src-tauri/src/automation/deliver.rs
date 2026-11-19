//! Task di consegna lato automazione (docs/GOALS.MD, "Consegna del goal"): cosa
//! legge chi consegna il goal in `get_task` (istruzioni, ambiente, passi e i
//! report dei task del goal), i tool di sessione che gli sono concessi (quali
//! sono lo dice la tabella di `roles`) e l'elenco dei repository da consegnare
//! che l'avvio calcola. La creazione del task e' in `boards::delivery`, l'avvio
//! in `worktrees::start::deliver`, la pulizia in `worktrees::delivery_cleanup`;
//! le regole pure sono in `crate::board` (`deliver`, `goal::delivery`, `texts`).

use super::boards::{get_goal, task_details};
use super::rpc::misrouted;
use super::*;
use crate::board::texts::{self, DeliverInput, DeliverRepo};
use crate::board::{goal_task_entries, Task};
use serde_json::json;

/// Un repository da consegnare: il suo checkout, il worktree del task sul branch
/// del goal e la base. Lo calcola l'avvio (fuori dal lock) e lo tiene `WorkerEnv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliverTarget {
    /// Chiave del repository (`repo_key`: "." = la root del workspace).
    pub key: String,
    /// Checkout dell'utente.
    pub repo_dir: String,
    /// Worktree del task di consegna, sul branch del goal.
    pub workdir: String,
    /// `goal/<id>`.
    pub branch: String,
    /// Branch base, nome semplice.
    pub base: String,
}

/// Risposta di `get_task` per un task di consegna: istruzioni proprie del tipo,
/// ambiente e passi della modalita' di consegna del workspace e `goal_tasks`, i
/// report dei task `work` e `review` del goal (il dettaglio, con le specifiche,
/// lo legge con `get_task_details`). Il goal lo aggiunge `Handoff`.
pub(super) fn get_task_json(ws: &Workspace, env: &WorkerEnv, task: &Task, board: &Board, ask_user: bool) -> Value {
    let goal = task.goal().and_then(|id| board.goal(id));
    let has_e2e = |key: &str| ws.repos.get(key).is_some_and(|c| c.e2e.is_some());
    let repos: Vec<DeliverRepo> = env
        .deliver
        .iter()
        .map(|t| DeliverRepo {
            key: &t.key,
            repo_dir: &t.repo_dir,
            workdir: &t.workdir,
            branch: &t.branch,
            base: &t.base,
            has_e2e: has_e2e(&t.key),
        })
        .collect();
    let input = DeliverInput {
        delivery: &env.delivery,
        goal_title: goal.map_or(task.title.as_str(), |g| g.title.as_str()),
        root: &ws.path,
        workdir: &env.workdir,
        repos: &repos,
        e2e: task.e2e,
    };
    let goal_tasks = task.goal().map_or_else(|| json!([]), |id| goal_task_entries(board, id));
    json!({
        "task": task.task,
        "title": task.title,
        "instructions": texts::deliver_instructions(&input, &ws.worker.rules, ask_user),
        "environment": texts::deliver_environment(&input),
        "delivery": texts::deliver_delivery_steps(&input),
        "goal_tasks": goal_tasks,
    })
}

/// Tool di sessione chiamati da chi consegna, nel workspace del suo task: la
/// lettura del goal e dei suoi task.
pub(super) fn rpc_deliverer<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: Method,
    params: &Value,
) -> Result<Value, String> {
    let (ws_id, _) = st.worker_task_of(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
    let ws = host.workspace(&ws_id).ok_or_else(|| "workspace not found".to_string())?;
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    match method {
        Method::GetTaskDetails => task_details(st, host, &ws, params),
        Method::GetGoal => get_goal(st, host, &ws, params),
        other => Err(misrouted(other)),
    }
}
