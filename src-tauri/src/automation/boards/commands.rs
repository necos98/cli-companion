//! Logica dei comandi Tauri delle board: la UI modifica la coda del workspace
//! e riceve la vista aggiornata.

use crate::automation::worktrees::{check_e2e_configured, check_e2e_needs_git, ensure_repos, FolderKinds};
use crate::automation::*;
use crate::board::{self, parse_spec, SpecContext, TaskAction, TaskSpec};

pub(in crate::automation) fn workspace_of<H: Host>(host: &H, ws_id: &str) -> Result<Workspace, String> {
    let ws = host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
    if ws.archived {
        return Err("workspace archiviato".to_string());
    }
    Ok(ws)
}

pub(in crate::automation) fn board_set<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    if max_parallel.is_none() && paused.is_none() {
        return Err("nessuna modifica richiesta".to_string());
    }
    if max_parallel.is_some_and(|m| !(1..=board::MAX_PARALLEL_LIMIT).contains(&m)) {
        return Err(format!("max_parallel deve essere tra 1 e {}", board::MAX_PARALLEL_LIMIT));
    }
    let now = now_millis();
    st.run(host, |r| r.set_board(ws_id, max_parallel, paused, now))?;
    st.view(host, ws_id)
}

pub(in crate::automation) fn task_create<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    spec: &Value,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    let mut spec = parse_spec(spec, None)?;
    let kinds = ensure_repos(host, &ws, [spec.repo.as_deref()])?;
    check_e2e_needs_git(&ws, std::slice::from_ref(&spec), &kinds)?;
    let now = now_millis();
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| {
        if spec.id.is_empty() {
            spec.id = r.free_id(ws_id, &spec.title);
        }
        check_e2e_configured(&ws, std::slice::from_ref(&spec))?;
        r.create_task(ws_id, spec, ctx, now)?;
        r.note_no_git(ws_id, &kinds);
        Ok::<(), String>(())
    })?;
    st.view(host, ws_id)
}

/// Modifica di un task: il repository nuovo si verifica fuori dal lock.
pub(super) fn update_checked<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    id: &str,
    patch: &Value,
) -> Result<(), String> {
    let current = st.run(host, |r| r.task_view(&ws.id, id)).ok_or_else(|| format!("task not found: {id}"))?;
    let spec = parse_spec(patch, Some(&TaskSpec::from_task(&current.task)))?;
    let recheck = spec.repo != current.task.repo || (spec.e2e && !current.task.e2e);
    let kinds = if recheck { ensure_repos(host, ws, [spec.repo.as_deref()])? } else { FolderKinds::new() };
    check_e2e_needs_git(ws, std::slice::from_ref(&spec), &kinds)?;
    check_e2e_configured(ws, std::slice::from_ref(&spec))?;
    let now = now_millis();
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| {
        r.update_task(&ws.id, id, patch, ctx, now)?;
        r.note_no_git(&ws.id, &kinds);
        Ok(())
    })
}

pub(in crate::automation) fn task_update<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    patch: &Value,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    update_checked(st, host, &ws, task_id, patch)?;
    st.view(host, ws_id)
}

/// Come `task_update` ma senza cambiare nulla: lo stato del task, il grafo delle
/// dipendenze e il modello si verificano sulla board com'e' adesso.
pub(in crate::automation) fn task_update_check<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    patch: &Value,
) -> Result<(), String> {
    let ws = workspace_of(host, ws_id)?;
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| r.check_task_update(&ws.id, task_id, patch, ctx, now_millis()))
}

pub(in crate::automation) fn task_action<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    action: TaskAction,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    let now = now_millis();
    st.run(host, |r| match action {
        TaskAction::Retry => r.retry_task(ws_id, task_id, now),
        TaskAction::MarkDone => r.mark_task_done(ws_id, task_id, now),
        TaskAction::Stop => r.stop_task(ws_id, task_id, now),
        TaskAction::Delete => r.delete_task(ws_id, task_id, now),
        TaskAction::DeleteCascade => r.delete_task_cascade(ws_id, task_id, now),
        TaskAction::ToTop => r.task_to_top(ws_id, task_id, now),
    })?;
    st.view(host, ws_id)
}

pub(in crate::automation) fn task_move<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    before: Option<&str>,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    let now = now_millis();
    st.run(host, |r| r.move_task(ws_id, task_id, before, now))?;
    st.view(host, ws_id)
}

pub(in crate::automation) fn proposal_decide<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    proposal_id: &str,
    accept: bool,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    let mut kinds = FolderKinds::new();
    if accept {
        // Nuova validazione: la modalita' di consegna o i repository possono essere cambiati.
        let specs = st.run(host, |r| r.proposal_specs(ws_id, proposal_id))?;
        kinds = ensure_repos(host, &ws, specs.iter().map(|s| s.repo.as_deref()))?;
        check_e2e_needs_git(&ws, &specs, &kinds)?;
        check_e2e_configured(&ws, &specs)?;
    }
    let now = now_millis();
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| {
        r.decide_proposal(ws_id, proposal_id, accept, ctx, now)?;
        r.note_no_git(ws_id, &kinds);
        Ok::<(), String>(())
    })?;
    st.view(host, ws_id)
}
