//! Vista della board per il frontend e per i tool MCP: `TaskView`/`BoardView`
//! (campi dei task piu' cio' che la UI deriva dalla board) e righe compatte di
//! `list_tasks`.

use super::agent_gate::{AgentGate, AgentUnavailableView};
use super::cascade::{Blockers, CascadeDelete, Waiters};
use super::{
    permits, Board, GoalView, Op, Proposal, SettingsProposalView, Task, TaskAction, TaskKind, TaskQuestion, TaskStatus,
    MAX_PARALLEL_LIMIT, MIN_PARALLEL_LIMIT,
};
use crate::validate::clip_with_ellipsis;
use crate::workspaces::{Delivery, Workspace};
use serde::Serialize;
use serde_json::{json, Value};
use ts_rs::TS;

/// Lunghezza di `note` e `summary` nelle righe compatte di `list_tasks`.
const COMPACT_NOTE_MAX: usize = 200;
const COMPACT_SUMMARY_MAX: usize = 300;

/// Task esposto: tutti i campi piu' cio' che la UI deriva dalla board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    /// Dipendenze non `done` (solo per `queued`).
    pub waiting_on: Vec<String>,
    /// I task `failed` da cui il task dipende, direttamente o no (solo per
    /// `queued`): non partira' finche' non si risolvono.
    pub blocked_by: Vec<String>,
    /// Il worker aspetta l'utente (`needs_input` o `idle`).
    pub attention: bool,
    /// Le azioni di `task_action` che riuscirebbero adesso: la tabella degli stati
    /// (`permits`) piu' il contesto della board (dipendenti e task in corso).
    pub allowed_actions: Vec<TaskAction>,
    /// Il task si puo' modificare (`update_task`).
    pub can_edit: bool,
    /// Il task si puo' riordinare (`move_task`, trascinandolo nella coda).
    pub can_reorder: bool,
    /// Cosa toglierebbe "Elimina con i dipendenti"; vuoto se il task non si puo' eliminare.
    pub cascade: CascadeDelete,
    /// Il task e' in coda ma il suo agente non e' pronto nel target del
    /// workspace: parte da solo appena lo diventa. Assente se l'agente e'
    /// pronto o la sua prontezza si sta ancora controllando.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub agent_unavailable: Option<AgentUnavailableView>,
    /// Domanda del worker in attesa di risposta.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question: Option<TaskQuestion>,
}

/// Intervallo ammesso per un valore numerico, estremi inclusi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct Bounds {
    pub min: u8,
    pub max: u8,
}

/// Gli estremi dei valori che l'utente imposta sulla board: la UI li legge da
/// qui invece di duplicarli.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BoardLimits {
    pub max_parallel: Bounds,
}

impl BoardLimits {
    pub const CURRENT: BoardLimits =
        BoardLimits { max_parallel: Bounds { min: MIN_PARALLEL_LIMIT, max: MAX_PARALLEL_LIMIT } };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BoardView {
    pub workspace_id: String,
    /// Revisione monotona (cresce a ogni modifica della vista): il frontend
    /// scarta le risposte piu' vecchie di quella gia' vista.
    pub rev: u64,
    pub max_parallel: u8,
    /// Estremi dei valori impostabili (oggi solo `max_parallel`).
    pub limits: BoardLimits,
    pub paused: bool,
    pub updated_at: i64,
    pub tasks: Vec<TaskView>,
    pub proposals: Vec<Proposal>,
    pub settings_proposals: Vec<SettingsProposalView>,
    /// I goal della board con fase, attesa e avanzamento.
    pub goals: Vec<GoalView>,
    /// Il workspace usa le fasce orarie e adesso e' fuori fascia.
    pub schedule_blocked: bool,
    /// Consegna del workspace (comodita' per la UI).
    pub delivery: Delivery,
}

impl BoardView {
    /// Conteggi per stato (dell'intera board), proposte pendenti e, se la board
    /// e' in pausa, `paused`: l'intestazione di `list_tasks` e di `board_status`.
    pub fn summary_json(&self) -> Value {
        let mut counts = serde_json::Map::new();
        for s in TaskStatus::ALL {
            counts.insert(
                s.as_str().to_string(),
                Value::from(self.tasks.iter().filter(|t| t.task.status() == s).count()),
            );
        }
        let mut out = json!({ "counts": counts, "proposals_pending": self.proposals.len() });
        if self.paused {
            out["paused"] = Value::Bool(true);
        }
        out
    }

    /// Righe compatte per `list_tasks`: conteggi per stato e una riga corta
    /// per task (solo gli stati richiesti, o tutti).
    pub fn to_list_json(&self, statuses: Option<&[TaskStatus]>) -> Value {
        let rows: Vec<Value> = self
            .tasks
            .iter()
            .filter(|t| statuses.is_none_or(|l| l.contains(&t.task.status())))
            .map(compact_row)
            .collect();
        let mut out = self.summary_json();
        out["tasks"] = Value::Array(rows);
        out
    }
}

/// La riga compatta di un task: id, titolo, stato e solo i campi che lo
/// distinguono (goal, attenzione, attesa, tipo, agente, E2E, cartella senza git, report, domanda, nota).
pub fn compact_row(v: &TaskView) -> Value {
    let t = &v.task;
    let mut o = json!({ "id": t.id, "title": t.title, "status": t.status().as_str() });
    if let Some(g) = t.goal() {
        o["goal"] = Value::from(g);
    }
    if v.attention {
        o["attention"] = Value::Bool(true);
    }
    if !v.waiting_on.is_empty() {
        o["waiting_on"] = json!(v.waiting_on);
    }
    if !v.blocked_by.is_empty() {
        o["blocked_by"] = json!(v.blocked_by);
    }
    if let Some(r) = t.fail_reason() {
        o["fail_reason"] = json!(r);
    }
    if t.kind != TaskKind::Work {
        o["kind"] = Value::from(t.kind.as_str());
    }
    if let Some(u) = &v.agent_unavailable {
        o["agent_unavailable"] = json!({ "agent": u.agent, "reason": u.reason });
    }
    if t.e2e {
        o["e2e"] = Value::Bool(true);
    }
    if t.no_git {
        o["no_git"] = Value::Bool(true);
    }
    if let Some(r) = t.report() {
        o["summary"] = Value::from(clip_with_ellipsis(&r.summary, COMPACT_SUMMARY_MAX));
        if let Some(v) = r.verdict {
            o["verdict"] = Value::from(v.as_str());
        }
    }
    if let Some(q) = &v.question {
        o["question"] = Value::from(clip_with_ellipsis(&q.text, COMPACT_NOTE_MAX));
    }
    if let Some(n) = t.note() {
        o["note"] = Value::from(clip_with_ellipsis(n, COMPACT_NOTE_MAX));
    }
    o
}

impl TaskView {
    /// JSON del task; senza `include_task` manca il testo della specifica.
    pub fn to_json(&self, include_task: bool) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if !include_task {
            if let Some(o) = v.as_object_mut() {
                o.remove("task");
            }
        }
        v
    }
}

impl Board {
    fn view_of(&self, t: &Task, waiters: &Waiters, blockers: &Blockers, gate: &AgentGate) -> TaskView {
        let status = t.status();
        let waiting_on = if status == TaskStatus::Queued {
            t.depends_on
                .iter()
                .filter(|d| !self.task(d).is_some_and(|x| x.status() == TaskStatus::Done))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let cascade = if permits(status, Op::Action(TaskAction::DeleteCascade)) {
            waiters.cascade_of(t)
        } else {
            CascadeDelete::default()
        };
        let allowed_actions = TaskAction::ALL
            .into_iter()
            .filter(|a| permits(status, Op::Action(*a)))
            .filter(|a| match a {
                TaskAction::Delete => cascade.dependents.is_empty(),
                TaskAction::DeleteCascade => !cascade.dependents.is_empty() && cascade.running.is_empty(),
                _ => true,
            })
            .collect();
        TaskView {
            task: t.clone(),
            waiting_on,
            blocked_by: blockers.get(t.id.as_str()).cloned().unwrap_or_default(),
            attention: t.needs_attention(),
            allowed_actions,
            can_edit: permits(status, Op::Edit),
            can_reorder: permits(status, Op::Reorder),
            cascade,
            agent_unavailable: gate.unavailable(t),
            question: t.pending_question().cloned(),
        }
    }

    pub fn task_view(&self, id: &str, gate: &AgentGate) -> Option<TaskView> {
        let waiters = self.waiters();
        self.task(id).map(|t| self.view_of(t, &waiters, &waiters.blockers(), gate))
    }

    /// Vista per il frontend; consegna e valori attuali delle impostazioni
    /// proposte vengono dal workspace `ws` (la board non lo conosce; `None` =
    /// consegna di default e nessun valore attuale), `schedule_blocked` dal gate
    /// delle fasce e `gate` dagli agenti non pronti.
    pub fn view_with(&self, ws: Option<&Workspace>, schedule_blocked: bool, gate: &AgentGate) -> BoardView {
        let waiters = self.waiters();
        let blockers = waiters.blockers();
        BoardView {
            workspace_id: self.workspace_id.clone(),
            rev: self.rev,
            max_parallel: self.max_parallel,
            limits: BoardLimits::CURRENT,
            paused: self.paused,
            updated_at: self.updated_at,
            tasks: self.tasks.iter().map(|t| self.view_of(t, &waiters, &blockers, gate)).collect(),
            proposals: self.proposals.clone(),
            settings_proposals: self.settings_proposals.iter().map(|p| p.view(ws)).collect(),
            goals: self.goal_views(),
            schedule_blocked,
            delivery: ws.map(|w| w.delivery).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests;
