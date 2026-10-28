//! Tool MCP `run_e2e` e `get_e2e_result`, dei worker e dei revisori.

use super::job::{launch, RunTarget};
use super::runs::{ClaimError, Owner};
use super::E2eTarget;
use crate::automation::*;
use crate::board::{E2eRun, E2eStatus};
use serde_json::json;

/// Attesa massima di `get_e2e_result`: ogni RPC deve chiudersi entro i 30 s
/// di `mcp::RPC_TIMEOUT`.
const MAX_WAIT_SECONDS: u64 = 25;

fn target_of(st: &AutomationState, session: &str) -> Result<E2eTarget, String> {
    st.e2e_target(session).ok_or_else(|| TASK_UNASSIGNED.to_string())
}

fn claim_error(e: ClaimError) -> String {
    match e {
        ClaimError::Busy(run_id) => {
            format!("an E2E run is already in progress in this folder (run_id {run_id}): wait for it with get_e2e_result")
        }
        ClaimError::NoFreePort => "no free local port is available for the E2E run: try again in a moment".to_string(),
    }
}

/// `run_e2e`: lancia gli E2E del repository del task nella cartella del
/// worker. Ritorna subito; l'esito si legge con `get_e2e_result`.
pub(in crate::automation) fn run_e2e<H: Host>(st: &AutomationState, host: &H, session: &str) -> Result<Value, String> {
    let target = target_of(st, session)?;
    let ws = host.workspace(&target.ws_id).ok_or_else(|| "workspace not found".to_string())?;
    let key = board::repo_key(target.repo.as_deref());
    let repo_cfg = ws.repos.get(&key).cloned().unwrap_or_default();
    let e2e = repo_cfg.e2e.clone().ok_or_else(|| {
        format!("E2E tests are not configured for repository '{key}': ask the user to configure them in the repository settings")
    })?;
    let owner = Owner::Task { ws: target.ws_id, task: target.task_id.clone() };
    let run_target = RunTarget {
        ws,
        dir: target.workdir,
        repo_dir: target.repo_dir,
        repo_cfg,
        task_id: target.task_id,
        worktree: target.worktree,
        owner,
    };
    let run = launch(st, host, run_target, e2e, |run| st.run(host, |r| r.begin_e2e(session, run.clone())), claim_error)?;
    Ok(json!({ "run_id": run.run_id, "status": E2eStatus::Running.as_str() }))
}

/// `wait_seconds`: intero da 0 a `MAX_WAIT_SECONDS` (assente = 0).
fn parse_wait(params: &Value) -> Result<Duration, String> {
    let invalid = || format!("wait_seconds must be an integer from 0 to {MAX_WAIT_SECONDS}");
    match params.get("wait_seconds") {
        None | Some(Value::Null) => Ok(Duration::ZERO),
        Some(v) => v.as_u64().filter(|s| *s <= MAX_WAIT_SECONDS).map(Duration::from_secs).ok_or_else(invalid),
    }
}

fn parse_run_id(params: &Value) -> Result<Option<&str>, String> {
    match params.get("run_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err("run_id must be a string".to_string()),
    }
}

/// L'ultima esecuzione del task del chiamante, controllata contro `run_id`.
fn last_run(st: &AutomationState, session: &str, run_id: Option<&str>) -> Result<E2eRun, String> {
    let run = target_of(st, session)?.last_run.ok_or_else(|| "no E2E run for this task: call run_e2e first".to_string())?;
    match run_id {
        Some(id) if id != run.run_id => {
            Err(format!("unknown run_id: the last E2E run of this task is {}", run.run_id))
        }
        _ => Ok(run),
    }
}

/// `get_e2e_result`: l'ultima esecuzione del task; se e' in corso aspetta
/// fino a `wait_seconds` che finisca.
pub(in crate::automation) fn get_e2e_result(st: &AutomationState, session: &str, params: &Value) -> Result<Value, String> {
    let wait = parse_wait(params)?;
    let run_id = parse_run_id(params)?;
    let mut run = last_run(st, session, run_id)?;
    if run.status == E2eStatus::Running && !wait.is_zero() {
        st.e2e.wait(&run.run_id, wait);
        run = last_run(st, session, run_id)?;
    }
    serde_json::to_value(run).map_err(|e| e.to_string())
}
