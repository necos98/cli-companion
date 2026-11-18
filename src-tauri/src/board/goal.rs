//! Goal (obiettivo): l'oggetto tracciato sopra i task. Nasce da una proposta
//! accettata, raggruppa i suoi task, ha una fase calcolata e una chiusura (vedi
//! docs/GOALS.MD). Qui il modello sul disco; i sottomoduli: `input` (il goal di
//! `propose_tasks` e i suoi limiti), `membership` (a quale goal vanno i task
//! nuovi e da chi possono dipendere), `closure` (chiusura, note, giri),
//! `phase` (fase e attesa, derivate: mai scritte su disco) e `archive` (goal
//! chiusi oltre il tetto). Funzioni pure, tempo passato dal chiamante.

use super::TITLE_MAX;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod archive;
mod closure;
mod input;
mod membership;
mod phase;

#[cfg(test)]
mod tests;

pub use archive::MAX_CLOSED_GOALS_KEPT;
pub use input::{parse_goal_id, parse_goal_spec, GoalSpec, ProposalGoal};
pub use phase::{GoalPhase, GoalProgress, GoalView, GoalWaiting};

/// Lunghezza massima del titolo di un goal (come quello di un task).
pub const GOAL_TITLE_MAX: usize = TITLE_MAX;
/// Lunghezza massima della richiesta dell'utente.
pub const GOAL_REQUEST_MAX: usize = 4000;
/// Criteri di riuscita di un goal.
pub const DONE_WHEN_MAX: usize = 20;
/// Lunghezza massima di un criterio di riuscita.
pub const DONE_WHEN_ITEM_MAX: usize = 300;
/// Voci del registro delle decisioni: oltre, le piu' vecchie cadono.
pub const DECISIONS_KEPT: usize = 50;
/// Lunghezza massima di una decisione.
pub const DECISION_MAX: usize = 500;
/// Goal aperti di una board.
pub const MAX_OPEN_GOALS: usize = 50;

/// Chi ha scritto una voce del registro delle decisioni.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DecisionSource {
    /// L'Intake, con le assunzioni e le scelte dell'utente al momento della proposta.
    Intake,
    /// L'utente, con una nota dal dettaglio del goal.
    User,
    /// L'app.
    App,
}

/// Una voce del registro delle decisioni di un goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GoalDecision {
    pub at: i64,
    pub source: DecisionSource,
    pub text: String,
}

/// Come si e' chiuso un goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GoalOutcome {
    /// Il lavoro e' finito (fino alla consegna del goal: tutti i task `done`).
    Delivered,
    /// L'utente l'ha chiuso a mano.
    Abandoned,
}

/// Esito dell'ultima review del goal, scritto alla chiusura.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GoalReview {
    Passed,
    Issues,
    /// Nessuna review conclusa con un verdetto.
    #[serde(rename = "none")]
    NotReviewed,
}

/// Un goal della board. Fase e attesa non sono qui: si derivano (`GoalView`).
/// Giro e chiusura li scrive solo la board (`closure`), mai un tool.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Goal {
    /// Stesso formato degli id dei task, unico fra i goal della board.
    pub id: String,
    pub title: String,
    /// La richiesta dell'utente, il piu' possibile con le sue parole.
    pub request: String,
    /// Criteri di riuscita dell'obiettivo.
    pub done_when: Vec<String>,
    /// Registro delle decisioni, dalla piu' vecchia.
    pub decisions: Vec<GoalDecision>,
    /// 1 = il lavoro iniziale; +1 a ogni proposta di correzione accettata.
    pub(in crate::board) round: u32,
    pub created_at: i64,
    /// Presente = chiuso. Un goal chiuso non si riapre.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::board) closed_at: Option<i64>,
    /// Solo se chiuso.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::board) outcome: Option<GoalOutcome>,
    /// Solo se chiuso.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::board) review: Option<GoalReview>,
}

impl Goal {
    /// Goal nuovo, al primo giro, dalla proposta accettata `spec`: le decisioni
    /// dell'Intake entrano nel registro con l'ora dell'accettazione.
    pub(in crate::board) fn from_spec(spec: GoalSpec, now: i64) -> Goal {
        let decisions =
            spec.decisions.into_iter().map(|text| GoalDecision { at: now, source: DecisionSource::Intake, text });
        Goal {
            id: spec.id,
            title: spec.title,
            request: spec.request,
            done_when: spec.done_when,
            decisions: decisions.collect(),
            round: 1,
            created_at: now,
            ..Goal::default()
        }
    }

    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }

    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn closed_at(&self) -> Option<i64> {
        self.closed_at
    }

    pub fn outcome(&self) -> Option<GoalOutcome> {
        self.outcome
    }

    pub fn review(&self) -> Option<GoalReview> {
        self.review
    }

    /// Aggiunge una voce al registro; oltre `DECISIONS_KEPT` cadono le piu' vecchie.
    pub(in crate::board) fn record(&mut self, source: DecisionSource, text: String, now: i64) {
        self.decisions.push(GoalDecision { at: now, source, text });
        let surplus = self.decisions.len().saturating_sub(DECISIONS_KEPT);
        self.decisions.drain(..surplus);
    }

    /// Invarianti dopo la lettura: giro almeno 1, esito e review solo (e
    /// sempre) su un goal chiuso, registro nei limiti.
    pub(in crate::board) fn sanitize(&mut self) {
        self.round = self.round.max(1);
        if self.closed_at.is_some() {
            self.outcome.get_or_insert(GoalOutcome::Abandoned);
            self.review.get_or_insert(GoalReview::NotReviewed);
        } else {
            self.outcome = None;
            self.review = None;
        }
        let surplus = self.decisions.len().saturating_sub(DECISIONS_KEPT);
        self.decisions.drain(..surplus);
    }
}
