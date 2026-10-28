//! Dispatch delle RPC dei tool MCP. Tre ruoli: il worker di un task
//! (`get_task`, `complete_task`), il revisore, cioe' il worker di un task
//! `review` (gli stessi due piu' `get_task_details` e `propose_tasks`), e la
//! sessione normale, Intake compreso (`propose_tasks`, `list_tasks`,
//! `get_task_details`, `update_task`, `delete_task`, piu' le skill globali di
//! `skills` e le impostazioni del workspace di `settings`). `whoami` e'
//! l'handshake del server MCP, non un tool.

use super::review::REVIEWER_METHODS;
use super::settings::SETTINGS_METHODS;
use super::skills::SKILL_METHODS;
use super::*;
use crate::board::TaskKind;
use serde_json::json;

const WORKER_METHODS: [&str; 2] = ["get_task", "complete_task"];
const SESSION_METHODS: [&str; 5] = ["propose_tasks", "list_tasks", "get_task_details", "update_task", "delete_task"];

/// Esegue un metodo RPC per `session`.
pub fn dispatch<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    if !(params.is_null() || params.is_object()) {
        return Err("params must be an object".to_string());
    }
    let kind = st.inner.lock().unwrap().worker_kind(session);
    let worker = kind.is_some();
    let reviewer = kind == Some(TaskKind::Review);
    if WORKER_METHODS.contains(&method) && !worker {
        return Err(format!("{method} is reserved for automated sessions"));
    }
    let session_only = SESSION_METHODS.contains(&method) || SKILL_METHODS.contains(&method) || SETTINGS_METHODS.contains(&method);
    let reviewer_call = reviewer && REVIEWER_METHODS.contains(&method);
    if session_only && worker && !reviewer_call {
        return Err(format!("{method} is not available to automated sessions"));
    }
    // Workspace archiviato: congelato, anche per un tab sopravvissuto.
    if host.tab_workspace(session).is_some_and(|w| w.archived) {
        return Err("workspace is archived".to_string());
    }
    match method {
        "whoami" => Ok(whoami(host, session, kind)),
        "get_task" => get_task(st, host, session),
        "complete_task" => complete_task(st, host, session, params),
        m if reviewer_call => review::rpc_reviewer(st, host, session, m, params),
        m if SKILL_METHODS.contains(&m) => skills::rpc(host, m, params),
        m if SETTINGS_METHODS.contains(&m) => settings::rpc(st, host, session, m, params),
        m if SESSION_METHODS.contains(&m) => boards::rpc_session(st, host, session, m, params),
        _ => Err(format!("unknown method: {method}")),
    }
}

/// Ruolo del chiamante (`kind` = tipo del task se e' un worker) e, per una
/// sessione normale, il suo workspace: lo usa il server MCP per scegliere i
/// tool e scrivere le istruzioni.
fn whoami<H: Host>(host: &H, session: &str, kind: Option<TaskKind>) -> Value {
    let role = match kind {
        Some(TaskKind::Review) => "reviewer",
        Some(TaskKind::Work) => "worker",
        None => "session",
    };
    let mut out = json!({ "role": role, "session": session });
    if kind.is_none() {
        if let Some(w) = host.tab_workspace(session) {
            out["workspace"] = json!({ "name": w.name, "kind": w.kind, "path": w.path });
        }
    }
    out
}

fn get_task<H: Host>(st: &AutomationState, host: &H, session: &str) -> Result<Value, String> {
    let (ws_id, _) = st.worker_task_of(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
    let ws = host.workspace(&ws_id).ok_or_else(|| "workspace not found".to_string())?;
    st.run(host, |r| r.get_task(session, &ws, now_millis()))
}

fn complete_task<H: Host>(st: &AutomationState, host: &H, session: &str, params: &Value) -> Result<Value, String> {
    let status = req_str(params, "status")?;
    let summary = req_str(params, "summary")?;
    let fields = board::parse_report_fields(params)?;
    // Diffstat dei task con worktree: git fuori dal lock.
    let diffstat = st.worktree_of(session).and_then(|(ws_id, wt)| {
        let ws = host.workspace(&ws_id)?;
        worktrees::compute_diffstat(host, &ws, &wt)
    });
    st.run(host, |r| r.complete_task(session, status, summary, fields, diffstat, now_millis()))?;
    Ok(json!({ "closing": true }))
}
