//! Fase e attesa di un goal (docs/GOALS.MD, "Fase e attesa"): derivate dai
//! suoi task e dalle sue proposte con una funzione pura, mai scritte su disco
//! ne' impostabili da un tool. `GoalView` le espone con il goal.

use super::{auto_fix_decision, AutoFixDecision, Goal, GoalReview};
use crate::board::{Board, Task, TaskKind, TaskStatus};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use ts_rs::TS;

/// Fase di un goal, lineare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GoalPhase {
    /// Almeno un task `work` del goal non e' `done`.
    Executing,
    /// I task `work` sono `done` e una review del goal non lo e' (in coda, in
    /// corso o fallita: in quel caso `waiting` riporta `failed`).
    Verifying,
    /// Nessun task aperto, ma una proposta del goal e' pendente.
    Deciding,
    /// Nessun task aperto ne' proposta pendente, consegna non ancora partita.
    /// Un goal senza branch (cartella senza git) non vi resta: la board lo
    /// chiude subito (`Board::advance_goals`).
    Ready,
    /// Il task di consegna del goal non e' `done` (in coda, in corso o fallito:
    /// in quel caso `waiting` riporta `failed`).
    Delivering,
    Closed,
}

impl GoalPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            GoalPhase::Executing => "executing",
            GoalPhase::Verifying => "verifying",
            GoalPhase::Deciding => "deciding",
            GoalPhase::Ready => "ready",
            GoalPhase::Delivering => "delivering",
            GoalPhase::Closed => "closed",
        }
    }
}

/// Cio' che di un goal aspetta l'utente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GoalWaiting {
    /// Una proposta del goal e' pendente e non sara' accettata in automatico.
    Proposal,
    /// Una proposta di correzione del goal e' pendente e i giri automatici sono finiti.
    RoundLimit,
    /// Il goal e' `ready` e la consegna non parte da sola: l'utente preme "Consegna".
    Deliver,
    /// Un task del goal e' `failed`.
    Failed,
    /// Un task del goal ha il worker in attesa dell'utente.
    Attention,
}

/// Avanzamento di un goal: task `done` sul totale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct GoalProgress {
    pub done: usize,
    pub total: usize,
}

/// Goal esposto: i suoi campi piu' fase, attesa e avanzamento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct GoalView {
    #[serde(flatten)]
    pub goal: Goal,
    pub phase: GoalPhase,
    /// Vuoto = il goal procede da solo.
    pub waiting: Vec<GoalWaiting>,
    pub tasks: GoalProgress,
    /// Verdetto dell'ultima review `done` del goal (`none` se non ce n'e'), anche
    /// a goal aperto: da qui l'interfaccia sceglie "Consegna" o "Consegna comunque".
    pub last_review: GoalReview,
}

/// Fase del goal dai suoi task e dalla presenza di una sua proposta pendente.
pub fn goal_phase(goal: &Goal, tasks: &[&Task], proposal_pending: bool) -> GoalPhase {
    let open = |kind: TaskKind| tasks.iter().any(|t| t.kind == kind && t.status() != TaskStatus::Done);
    if !goal.is_open() {
        GoalPhase::Closed
    } else if open(TaskKind::Deliver) {
        GoalPhase::Delivering
    } else if open(TaskKind::Work) {
        GoalPhase::Executing
    } else if open(TaskKind::Review) {
        GoalPhase::Verifying
    } else if proposal_pending {
        GoalPhase::Deciding
    } else {
        GoalPhase::Ready
    }
}

/// Verdetto della review `done` conclusa per ultima fra `tasks`.
pub(super) fn last_review_of(tasks: &[&Task]) -> GoalReview {
    tasks
        .iter()
        .filter(|t| t.kind == TaskKind::Review && t.status() == TaskStatus::Done)
        .filter_map(|t| Some((t.finished_at()?, t.report()?.verdict?)))
        .max_by_key(|(at, _)| *at)
        .map_or(GoalReview::NotReviewed, |(_, verdict)| verdict.into())
}

/// La consegna parte da sola: `auto_deliver` e l'ultima review non ha trovato
/// problemi (con `issues` la sceglie l'utente: "Consegna comunque").
pub(super) fn delivers_by_itself(goal: &Goal, last_review: GoalReview) -> bool {
    goal.auto_deliver && last_review != GoalReview::Issues
}

/// Le proposte pendenti di un goal, secondo cosa ne fara' l'app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PendingProposals {
    /// Almeno una proposta del goal e' pendente.
    pub any: bool,
    /// Almeno una la decide l'utente (non sara' accettata in automatico).
    pub for_user: bool,
    /// Almeno una e' una correzione ferma al tetto dei giri automatici.
    pub round_limit: bool,
}

impl PendingProposals {
    /// Aggiunge una proposta pendente con la sua decisione.
    pub fn add(&mut self, decision: &AutoFixDecision) {
        self.any = true;
        match decision {
            AutoFixDecision::Accept => {}
            AutoFixDecision::RoundLimit => self.round_limit = true,
            AutoFixDecision::Manual | AutoFixDecision::Hold(_) => self.for_user = true,
        }
    }
}

/// Cio' che il goal aspetta dall'utente, nell'ordine di `GoalWaiting`. Un goal
/// chiuso non aspetta nulla.
pub fn goal_waiting(
    goal: &Goal,
    tasks: &[&Task],
    proposals: PendingProposals,
    last_review: GoalReview,
) -> Vec<GoalWaiting> {
    if !goal.is_open() {
        return Vec::new();
    }
    // Pronto, con qualcosa da consegnare (senza branch il goal si chiude da solo),
    // senza un task di consegna e senza che parta da solo.
    let deliver = goal_phase(goal, tasks, proposals.any) == GoalPhase::Ready
        && !goal.branches.is_empty()
        && tasks.iter().all(|t| t.kind != TaskKind::Deliver)
        && !delivers_by_itself(goal, last_review);
    let checks = [
        (GoalWaiting::Proposal, proposals.for_user),
        (GoalWaiting::RoundLimit, proposals.round_limit),
        (GoalWaiting::Deliver, deliver),
        (GoalWaiting::Failed, tasks.iter().any(|t| t.status() == TaskStatus::Failed)),
        (GoalWaiting::Attention, tasks.iter().any(|t| t.needs_attention())),
    ];
    checks.into_iter().filter(|(_, on)| *on).map(|(w, _)| w).collect()
}

impl GoalView {
    fn of(goal: &Goal, tasks: &[&Task], proposals: PendingProposals) -> GoalView {
        let last_review = last_review_of(tasks);
        GoalView {
            goal: goal.clone(),
            phase: goal_phase(goal, tasks, proposals.any),
            waiting: goal_waiting(goal, tasks, proposals, last_review),
            tasks: GoalProgress {
                done: tasks.iter().filter(|t| t.status() == TaskStatus::Done).count(),
                total: tasks.len(),
            },
            last_review,
        }
    }

    /// Riga di `list_goals`.
    pub fn to_row_json(&self) -> Value {
        json!({
            "id": self.goal.id,
            "title": self.goal.title,
            "phase": self.phase,
            "round": self.goal.round,
            "waiting": self.waiting,
            "tasks": self.tasks,
        })
    }
}

impl Board {
    /// Le viste dei goal, nell'ordine della board: task e proposte si
    /// raggruppano per goal in un solo passaggio.
    pub fn goal_views(&self) -> Vec<GoalView> {
        let mut by_goal: HashMap<&str, Vec<&Task>> = HashMap::new();
        for t in &self.tasks {
            if let Some(g) = t.goal() {
                by_goal.entry(g).or_default().push(t);
            }
        }
        let mut pending: HashMap<&str, PendingProposals> = HashMap::new();
        for p in &self.proposals {
            if let Some(g) = p.goal_id.as_deref() {
                pending.entry(g).or_default().add(&auto_fix_decision(self, p));
            }
        }
        self.goals
            .iter()
            .map(|g| {
                let tasks = by_goal.get(g.id.as_str()).map_or(&[][..], Vec::as_slice);
                GoalView::of(g, tasks, pending.get(g.id.as_str()).copied().unwrap_or_default())
            })
            .collect()
    }

    /// La vista del goal `id`.
    pub fn goal_view(&self, id: &str) -> Option<GoalView> {
        let goal = self.goal(id)?;
        let tasks: Vec<&Task> = self.goal_tasks(id).collect();
        let mut pending = PendingProposals::default();
        for p in self.proposals.iter().filter(|p| p.goal_id.as_deref() == Some(id)) {
            pending.add(&auto_fix_decision(self, p));
        }
        Some(GoalView::of(goal, &tasks, pending))
    }
}

/// Riga di un task nel dettaglio di un goal (`get_goal`).
fn goal_task_row(t: &Task) -> Value {
    json!({ "id": t.id, "title": t.title, "status": t.status(), "kind": t.kind, "round": t.round() })
}

impl Board {
    /// Il goal `id` per `get_goal`: i suoi campi con fase e attesa,
    /// l'avanzamento (`progress`) e i suoi task in ordine di coda.
    pub fn goal_detail_json(&self, id: &str) -> Option<Value> {
        let view = self.goal_view(id)?;
        let mut out = serde_json::to_value(&view.goal).unwrap_or(Value::Null);
        out["phase"] = json!(view.phase);
        out["waiting"] = json!(view.waiting);
        out["progress"] = json!(view.tasks);
        out["tasks"] = Value::Array(self.goal_tasks(id).map(goal_task_row).collect());
        Some(out)
    }
}
