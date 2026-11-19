//! Goal (obiettivo): l'oggetto tracciato sopra i task. Nasce da una proposta
//! accettata, raggruppa i suoi task, ha una fase calcolata e una chiusura (vedi
//! docs/GOALS.MD). Qui il modello sul disco; i sottomoduli: `input` (il goal di
//! `propose_tasks` e i suoi limiti), `membership` (a quale goal vanno i task
//! nuovi e da chi possono dipendere), `closure` (chiusura, note, giri),
//! `phase` (fase e attesa, derivate: mai scritte su disco), `delivery` (consegna:
//! impostazioni, rapporto e task di consegna), `advance` (chiusura e consegna
//! automatica al publish) e `archive` (goal chiusi oltre il tetto). Funzioni
//! pure, tempo passato dal chiamante.

use super::TITLE_MAX;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

mod advance;
mod archive;
mod branches;
mod closure;
mod delivery;
mod input;
mod membership;
mod phase;

#[cfg(test)]
mod tests;

pub use advance::{DeliveredGoal, GoalAdvance};
pub use archive::MAX_CLOSED_GOALS_KEPT;
pub use branches::{goal_branch_name, GoalBranch, GOAL_BRANCH_PREFIX};
pub use delivery::{GoalReport, DEFAULT_DELIVER_BOT};
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
    /// Il goal e' stato consegnato (task di consegna `done`), o non ha nulla da
    /// consegnare (cartella senza git).
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
/// Giro e chiusura li scrive solo la board (`closure`), i branch solo l'app
/// (`branches`), le impostazioni di consegna solo l'utente (`delivery`), mai un
/// tool.
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
    /// Branch `goal/<id>` per repository (chiave = `repo_key`) con la base da cui
    /// sono nati. Li scrive solo l'app, all'avvio del primo task del goal in quel
    /// repository: vuoto finche' nessun task e' partito (o senza git).
    pub branches: BTreeMap<String, GoalBranch>,
    /// Il task di consegna si crea da solo quando il goal e' pronto (e l'ultima
    /// review non ha trovato problemi). Lo cambia solo l'utente, mai un tool.
    pub(in crate::board) auto_deliver: bool,
    /// Il Bot del task di consegna; `None` = `DEFAULT_DELIVER_BOT`. Lo cambia solo
    /// l'utente, mai un tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::board) deliver_bot: Option<String>,
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
    /// Cosa e' stato consegnato: solo se chiuso `delivered` con un task di consegna.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::board) report: Option<GoalReport>,
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

    pub fn report(&self) -> Option<&GoalReport> {
        self.report.as_ref()
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
        self.sanitize_branches();
        self.sanitize_delivery();
    }
}
