//! Task di revisione lato automazione: cosa legge il revisore in `get_task` e i
//! tool di sessione che gli sono concessi (`get_task_details`, `propose_tasks`;
//! quali sono lo dice la tabella di `roles`). Il resto del ciclo di vita e'
//! quello di ogni worker; i tipi e la validazione del verdetto sono in
//! `crate::board::review`.

use super::boards::{check_new_tasks, list_tasks, task_details};
use super::rpc::misrouted;
use super::*;
use crate::board::texts::{self, EnvInfo};
use serde_json::json;

/// Risposta di `get_task` per un task di revisione: niente passi di consegna,
/// invece i report dei task revisionati.
pub(super) fn get_task_json(info: &EnvInfo, workspace_rules: &str, board: &Board) -> Value {
    // Il revisore lancia gli E2E se almeno un task revisionato li richiede.
    let reviewed_e2e = info.task.depends_on.iter().any(|d| board.task(d).is_some_and(|t| t.e2e));
    let out = json!({
        "task": info.task.task,
        "title": info.task.title,
        "instructions": texts::reviewer_instructions(info, workspace_rules, reviewed_e2e),
        "environment": texts::environment(info),
        "reviewed_tasks": board::reviewed_tasks(board, info.task),
    });
    out
}

/// `propose_tasks` del revisore: a differenza dell'Intake non c'e' una proposta
/// da accettare, le correzioni entrano subito in coda (tutto o niente) con le
/// stesse validazioni di una proposta. `blocks` sono i task in coda da far aspettare.
fn queue_fixes<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    tab: &str,
    review_id: &str,
    params: &Value,
) -> Result<Value, String> {
    let (specs, blocks) = board::parse_review_proposal(params)?;
    check_new_tasks(host, ws, &specs)?;
    let bots = host.agent_bots();
    let ctx = board::SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    let now = now_millis();
    let out = st.run(host, |r| r.queue_review_fixes(&ws.id, tab, review_id, (specs, blocks), ctx, now))?;
    Ok(json!({
        "status": "accepted",
        "tasks": out.tasks,
        "waiting": out.waiting,
        "message": "The fix tasks are queued at the head of the queue and run before the tasks listed in `waiting`, \
                    which now also wait for the gate (the new review, or the fixes if you proposed no review). Do not wait for them: complete this review."
    }))
}

/// Tool di sessione chiamati dal revisore, nel workspace del suo task.
pub(super) fn rpc_reviewer<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: Method,
    params: &Value,
) -> Result<Value, String> {
    let (ws_id, task_id) = st.worker_task_of(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
    let ws = host.workspace(&ws_id).ok_or_else(|| "workspace not found".to_string())?;
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    match method {
        Method::ProposeTasks => queue_fixes(st, host, &ws, session, &task_id, params),
        Method::ListTasks => list_tasks(st, host, &ws, params),
        Method::GetTaskDetails => task_details(st, host, &ws, params),
        Method::GetWorkspaceSettings => Ok(settings::get_settings(st, host, &ws)),
        other => Err(misrouted(other)),
    }
}
