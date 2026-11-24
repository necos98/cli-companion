//! Task di pianificazione lato automazione: la creazione da parte dell'app
//! (`create_plan_task`, l'unico modo di avere un task `plan`), cosa legge il
//! pianificatore in `get_task`, i tool di sessione che gli sono concessi (quali
//! sono lo dice la tabella di `roles`) e il vincolo di una sola proposta. Il
//! resto del ciclo di vita e' quello di ogni worker; le regole pure sono in
//! `crate::board::plan`.

use super::boards::{list_tasks, propose_tasks, task_details, workspace_of};
use super::rpc::misrouted;
use super::worktrees::ensure_repos;
use super::*;
use crate::board::texts::{self, EnvInfo};
use crate::board::{parse_spec, SpecContext, TaskSpec};
use serde_json::json;

pub(super) mod root;

/// Errore di una seconda proposta dello stesso pianificatore (lo legge il modello).
pub(crate) const PLAN_PROPOSED_ONCE: &str =
    "this plan task already made its proposal: a planner calls propose_tasks once. Call complete_task now.";

/// Richiesta di pianificazione creata dall'app (dall'HUB): il task `plan`
/// entra direttamente in coda, senza proposta.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanRequest {
    /// Titolo mostrato nella coda.
    pub title: String,
    /// La richiesta da pianificare, completa: il pianificatore non vede altro.
    pub task: String,
    /// Modello del pianificatore; `None` = quello del Bot o, senza Bot, quello
    /// dei worker del workspace.
    pub model: Option<String>,
    /// Repository (cartella relativa alla root); `None` = la root.
    pub repo: Option<String>,
    /// Bot che pianifica: agente, modello, effort e skill si copiano dal Bot
    /// alla creazione (vedi `board::resolve_spec`).
    pub bot: Option<String>,
}

impl PlanRequest {
    /// Specifica validata come quella di ogni task (`parse_spec`), con il tipo
    /// `plan` che dall'esterno non si puo' chiedere. Senza id: lo assegna la coda.
    fn to_spec(&self) -> Result<TaskSpec, String> {
        let input = json!({
            "title": self.title,
            "task": self.task,
            "model": self.model,
            "repo": self.repo,
            "bot": self.bot,
        });
        Ok(TaskSpec { kind: TaskKind::Plan, ..parse_spec(&input, None)? })
    }
}

/// Crea un task `plan` in coda nel workspace `ws_id` e ritorna il suo id. Lo
/// scheduler lo avvia come ogni task: in un worktree del suo repository se e'
/// git, altrimenti (la root del workspace o una sua sottocartella senza git)
/// sul posto, in sola lettura (vedi `root`).
pub fn create_plan_task<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    req: &PlanRequest,
) -> Result<String, String> {
    let ws = workspace_of(host, ws_id)?;
    let mut spec = req.to_spec()?;
    let kinds = ensure_repos(host, &ws, [spec.repo.as_deref()])?;
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    let now = now_millis();
    st.run(host, |r| {
        spec.id = r.free_id(ws_id, &spec.title);
        let id = r.create_task(ws_id, spec, ctx, now)?;
        r.note_no_git(ws_id, &kinds);
        Ok(id)
    })
}

/// Crea il task `plan` chiesto dall'Agents HUB: repository radice e il Bot
/// `bot` scelto dal segretario, senza ripieghi (un Bot sconosciuto o
/// disabilitato e' un errore). Il modello e' quello del Bot: `general` lascia
/// il predefinito dell'agente.
pub fn create_hub_plan_task<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    title: &str,
    task: &str,
    bot: &str,
) -> Result<String, String> {
    let req = PlanRequest {
        title: title.to_string(),
        task: task.to_string(),
        model: None,
        repo: None,
        bot: Some(bot.to_string()),
    };
    create_plan_task(st, host, ws_id, &req)
}

/// Risposta di `get_task` per un task di pianificazione: niente passi di
/// consegna, l'ambiente e' quello di sola lettura.
pub(super) fn get_task_json(info: &EnvInfo, workspace_rules: &str, ask_user: bool) -> Value {
    json!({
        "task": info.task.task,
        "title": info.task.title,
        "instructions": texts::planner_instructions(info, workspace_rules, ask_user),
        "environment": texts::planner_environment(info),
    })
}

/// Tool di sessione chiamati dal pianificatore, nel workspace del suo task. La
/// proposta porta il task di pianificazione come origine.
pub(super) fn rpc_planner<H: Host>(
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
        Method::ProposeTasks => propose_tasks(st, host, &ws, session, Some(task_id), params),
        Method::ListTasks => list_tasks(st, host, &ws, params),
        Method::GetTaskDetails => task_details(st, host, &ws, params),
        Method::GetWorkspaceSettings => Ok(settings::get_settings(st, host, &ws)),
        other => Err(misrouted(other)),
    }
}

impl Registry {
    /// Un pianificatore che ha gia' una proposta registrata non ne fa un'altra.
    /// Gli altri chiamanti non hanno limiti.
    pub(super) fn check_planner_can_propose(&self, tab: &str) -> Result<(), String> {
        match self.workers.get(tab) {
            Some(w) if w.kind == TaskKind::Plan && w.proposed => Err(PLAN_PROPOSED_ONCE.to_string()),
            _ => Ok(()),
        }
    }

    /// La proposta del pianificatore `tab` e' stata registrata.
    pub(super) fn note_planner_proposal(&mut self, tab: &str) {
        if let Some(w) = self.workers.get_mut(tab).filter(|w| w.kind == TaskKind::Plan) {
            w.proposed = true;
        }
    }
}
