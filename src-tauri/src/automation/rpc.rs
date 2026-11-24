//! Dispatch delle RPC dei tool MCP. Chi puo' chiamare cosa e' deciso dalla
//! tabella di `roles` (`Method::roles`): qui si controlla l'accesso e si smista
//! per ruolo. Il worker di un task consegna il lavoro (`get_task`,
//! `complete_task`, `run_e2e`, `get_e2e_result`); il revisore, cioe' il worker di
//! un task `review`, fa lo stesso piu' `get_task_details` e `propose_tasks`
//! (`review`); il pianificatore,
//! worker di un task `plan`, legge il suo task, la coda e le impostazioni e
//! propone (`planner`), senza E2E; la sessione normale, Intake compreso, gestisce
//! la coda (`boards`), le skill globali (`skills`) e le impostazioni del
//! workspace (`settings`).
//! `whoami` e' l'handshake del server MCP, non un tool; dice anche se l'Agents
//! HUB e' acceso (`hub`), da cui dipendono i tool riservati all'HUB (`ask_user`).

use super::*;
use serde_json::json;

/// Punto d'ingresso autenticato di una richiesta RPC: controlla versione e
/// credenziali della sessione, poi smista. Senza `AppHandle`, cosi' si prova
/// con un host finto.
pub fn handle<H: Host>(st: &AutomationState, host: &H, req: &RpcRequest) -> RpcResponse {
    if let Err(e) = authenticate(req, &st.keys) {
        return RpcResponse::failure(e);
    }
    RpcResponse::from_result(dispatch(st, host, &req.session, &req.method, &req.params))
}

/// Errore di un metodo che il ruolo non puo' chiamare. La sessione normale non
/// ha un task: i tool dei worker le sono riservati al contrario.
pub(super) fn denied(method: Method, role: Role) -> String {
    match role {
        Role::Session => format!("{} is reserved for automated sessions", method.name()),
        Role::Worker | Role::Reviewer | Role::Planner => {
            format!("{} is not available to automated sessions", method.name())
        }
    }
}

/// Errore di un metodo arrivato a un modulo che non lo gestisce (smistamento sbagliato).
pub(super) fn misrouted(method: Method) -> String {
    format!("unknown method: {}", method.name())
}

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
    let method = Method::parse(method).ok_or_else(|| format!("unknown method: {method}"))?;
    let role = Role::of_worker(st.registry().worker_kind(session));
    if !method.allows(role) {
        return Err(denied(method, role));
    }
    // Workspace archiviato: congelato, anche per un tab sopravvissuto.
    if host.tab_workspace(session).is_some_and(|w| w.archived) {
        return Err("workspace is archived".to_string());
    }
    if method.requires_hub() && !host.hub_enabled() {
        return Err(questions::HUB_OFF.to_string());
    }
    match method {
        Method::Whoami => Ok(whoami(host, session, role)),
        Method::GetTask => get_task(st, host, session, role),
        Method::AskUser => questions::ask_user(st, host, session, params),
        Method::CompleteTask => complete_task(st, host, session, params),
        Method::RunE2e => e2e::tools::run_e2e(st, host, session),
        Method::GetE2eResult => e2e::tools::get_e2e_result(st, session, params),
        Method::ProposeTasks | Method::GetTaskDetails | Method::ListTasks | Method::GetWorkspaceSettings
            if role == Role::Planner =>
        {
            planner::rpc_planner(st, host, session, method, params)
        }
        Method::GetWorkspaceSettings if role == Role::Reviewer => {
            review::rpc_reviewer(st, host, session, method, params)
        }
        Method::ProposeTasks | Method::GetTaskDetails => match role {
            Role::Session => boards::rpc_session(st, host, session, method.name(), params),
            Role::Reviewer => review::rpc_reviewer(st, host, session, method, params),
            Role::Worker | Role::Planner => Err(denied(method, role)),
        },
        Method::ListTasks | Method::UpdateTask | Method::DeleteTask => {
            boards::rpc_session(st, host, session, method.name(), params)
        }
        Method::SaveSkill | Method::ListSkills | Method::DeleteSkill => skills::rpc(host, method, params),
        Method::GetWorkspaceSettings | Method::ProposeSettings => settings::rpc(st, host, session, method, params),
    }
}

/// Ruolo del chiamante e, per una sessione normale, il suo workspace: lo usa il
/// server MCP per scegliere i tool e scrivere le istruzioni.
fn whoami<H: Host>(host: &H, session: &str, role: Role) -> Value {
    let mut out = json!({ "role": role.as_str(), "session": session, "hub": host.hub_enabled() });
    if role == Role::Session {
        if let Some(w) = host.tab_workspace(session) {
            out["workspace"] = json!({ "name": w.name, "kind": w.kind, "path": w.path });
        }
    }
    out
}

fn get_task<H: Host>(st: &AutomationState, host: &H, session: &str, role: Role) -> Result<Value, String> {
    let (ws_id, _) = st.worker_task_of(session).ok_or_else(|| "reserved for automated sessions".to_string())?;
    let ws = host.workspace(&ws_id).ok_or_else(|| "workspace not found".to_string())?;
    // `ask_user` solo con l'HUB acceso e a chi puo' chiamarlo; calcolato fuori dal lock.
    let ask_user = host.hub_enabled() && Method::AskUser.allows(role);
    st.run(host, |r| r.get_task(session, &ws, ask_user, now_millis()))
}

fn complete_task<H: Host>(st: &AutomationState, host: &H, session: &str, params: &Value) -> Result<Value, String> {
    let status = req_str(params, "status")?;
    let summary = req_str(params, "summary")?;
    let fields = board::parse_report_fields(params)?;
    // Diffstat dei task con worktree (non dei piani, che non consegnano): git fuori dal lock.
    let plan = st.registry().worker_kind(session) == Some(TaskKind::Plan);
    let diffstat = st.worktree_of(session).filter(|_| !plan).and_then(|(ws_id, wt)| {
        let ws = host.workspace(&ws_id)?;
        worktrees::compute_diffstat(host, &ws, &wt)
    });
    // Il blocco E2E (git) si prepara fuori dal lock, solo se serve.
    let gate = e2e::gate_input(st, host, session, status);
    st.run(host, |r| r.complete_task(session, status, summary, Completion { fields, diffstat, gate }, now_millis()))?;
    Ok(json!({ "closing": true }))
}
