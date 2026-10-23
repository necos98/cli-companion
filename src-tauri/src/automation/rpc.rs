//! Dispatch delle RPC dei tool MCP. Due ruoli: il worker di un task
//! (`get_task`, `complete_task`) e la sessione normale, Intake compreso
//! (`propose_tasks`, `list_tasks`, `get_task_details`, `update_task`,
//! `delete_task`, piu' le skill globali di `skills`). `whoami` e' l'handshake del server MCP, non un tool.

use super::skills::SKILL_METHODS;
use super::*;
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
    let worker = st.inner.lock().unwrap().is_worker(session);
    if WORKER_METHODS.contains(&method) && !worker {
        return Err(format!("{method} is reserved for automated sessions"));
    }
    if (SESSION_METHODS.contains(&method) || SKILL_METHODS.contains(&method)) && worker {
        return Err(format!("{method} is not available to automated sessions"));
    }
    // Workspace archiviato: congelato, anche per un tab sopravvissuto.
    if host.tab_workspace(session).is_some_and(|w| w.archived) {
        return Err("workspace is archived".to_string());
    }
    match method {
        "whoami" => Ok(whoami(host, session, worker)),
        "get_task" => get_task(st, host, session),
        "complete_task" => complete_task(st, host, session, params),
        m if SKILL_METHODS.contains(&m) => skills::rpc(host, m, params),
        m if SESSION_METHODS.contains(&m) => boards::rpc_session(st, host, session, m, params),
        _ => Err(format!("unknown method: {method}")),
    }
}

/// Ruolo del chiamante e, per una sessione normale, il suo workspace: lo usa
/// il server MCP per scegliere i tool e scrivere le istruzioni.
fn whoami<H: Host>(host: &H, session: &str, worker: bool) -> Value {
    let mut out = json!({ "role": if worker { "worker" } else { "session" }, "session": session });
    if !worker {
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
