//! Tool MCP delle sessioni normali (Intake), del revisore e del pianificatore:
//! propongono task e consultano la coda del workspace del tab chiamante.

use super::commands::update_checked;
use crate::automation::worktrees::{check_e2e_configured, check_e2e_needs_git, ensure_repos};
use crate::automation::*;
use crate::board::{parse_proposal, SpecContext, TaskStatus};
use serde_json::json;

/// Workspace del tab che chiama un tool di sessione.
pub(in crate::automation) fn session_workspace<H: Host>(host: &H, session: &str) -> Result<Workspace, String> {
    host.tab_workspace(session).ok_or_else(|| "the workspace of this tab was not found".to_string())
}

/// Tool MCP delle sessioni normali (Intake compreso): la coda del workspace
/// del tab chiamante.
pub(in crate::automation) fn rpc_session<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let ws = session_workspace(host, session)?;
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    let now = now_millis();
    match method {
        "propose_tasks" => propose_tasks(st, host, &ws, session, None, params),
        "list_tasks" => list_tasks(st, host, &ws, params),
        "get_task_details" => task_details(st, host, &ws, params),
        "update_task" => {
            let id = req_str(params, "id")?.trim().to_string();
            update_checked(st, host, &ws, &id, params)?;
            let view = st.run(host, |r| r.task_view(&ws.id, &id));
            Ok(json!({ "task": view.map(|v| v.to_json(false)) }))
        }
        "delete_task" => {
            let id = req_str(params, "id")?.trim().to_string();
            st.run(host, |r| r.delete_task(&ws.id, &id, now))?;
            Ok(json!({ "deleted": id }))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}

/// `propose_tasks`: proposta di task per la coda di `ws`. `tab` e' il tab che la fa;
/// `source_task` il task di revisione o di pianificazione, se a farla e' il suo
/// worker.
pub(in crate::automation) fn propose_tasks<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    tab: &str,
    source_task: Option<String>,
    params: &Value,
) -> Result<Value, String> {
    let specs = parse_proposal(params)?;
    let kinds = ensure_repos(host, ws, specs.iter().map(|s| s.repo.as_deref()))?;
    check_e2e_needs_git(ws, &specs, &kinds)?;
    check_e2e_configured(ws, &specs)?;
    let now = now_millis();
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    let id = st.run(host, |r| r.propose(&ws.id, tab, source_task, specs, ctx, now))?;
    Ok(json!({
        "proposal": id,
        "status": "pending",
        "message": "The user will accept or discard it in the app. Do not wait for the decision."
    }))
}

/// `list_tasks`: la coda di `ws` in forma compatta, filtrata per stato.
pub(in crate::automation) fn list_tasks<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    params: &Value,
) -> Result<Value, String> {
    let statuses = parse_statuses(params)?;
    let minute = host.local_minute(now_millis());
    let view = st.run(host, |r| r.board_view(ws, minute));
    Ok(view.to_list_json(statuses.as_deref()))
}

/// `get_task_details`: il task di `ws` con il testo della specifica.
pub(in crate::automation) fn task_details<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    params: &Value,
) -> Result<Value, String> {
    let id = req_str(params, "id")?.trim();
    let view = st.run(host, |r| r.task_view(&ws.id, id)).ok_or_else(|| format!("task not found: {id}"))?;
    Ok(json!({ "task": view.to_json(true) }))
}

/// `status` di `list_tasks`: elenco opzionale di stati validi.
fn parse_statuses(params: &Value) -> Result<Option<Vec<TaskStatus>>, String> {
    let valid = || TaskStatus::ALL.map(TaskStatus::as_str).join(", ");
    let list = match params.get("status") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(format!("status must be an array of statuses ({})", valid())),
    };
    list.iter()
        .map(|v| {
            v.as_str().and_then(TaskStatus::parse).ok_or_else(|| format!("invalid status {v} (valid: {})", valid()))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
