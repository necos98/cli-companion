//! Task di revisione lato automazione: cosa legge il revisore in `get_task` e i
//! tool di sessione che gli sono concessi (`get_task_details`, `propose_tasks`).
//! Il resto del ciclo di vita e' quello di ogni worker; i tipi e la validazione
//! del verdetto sono in `crate::board::review`.

use super::boards::{propose_tasks, task_details};
use super::*;
use crate::board::texts::{self, EnvInfo};
use serde_json::json;

/// Metodi di sessione concessi al revisore: leggere le specifiche dei task
/// revisionati e proporre i task di correzione.
pub(super) const REVIEWER_METHODS: [&str; 2] = ["get_task_details", "propose_tasks"];

/// Risposta di `get_task` per un task di revisione: niente passi di consegna,
/// invece i report dei task revisionati.
pub(super) fn get_task_json(info: &EnvInfo, workspace_rules: &str, board: &Board) -> Value {
    json!({
        "task": info.task.task,
        "title": info.task.title,
        "instructions": texts::reviewer_instructions(info, workspace_rules),
        "environment": texts::environment(info),
        "reviewed_tasks": board::reviewed_tasks(board, info.task),
    })
}

/// Tool di sessione chiamati dal revisore, nel workspace del suo task. Le
/// proposte portano il task di revisione come origine.
pub(super) fn rpc_reviewer<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let (ws_id, task_id) = st.worker_task_of(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
    let ws = host.workspace(&ws_id).ok_or_else(|| "workspace not found".to_string())?;
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    match method {
        "propose_tasks" => propose_tasks(st, host, &ws, session, Some(task_id), params),
        "get_task_details" => task_details(st, host, &ws, params),
        _ => Err(format!("unknown method: {method}")),
    }
}
