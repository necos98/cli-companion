//! Agente che esegue un passo di test in linguaggio naturale pilotando il browser.
//! Generico: nessuna logica dell'esperimento (scenari, verdetti, metriche).

pub mod action;
mod catalog;
mod conversation;
mod execute;
mod observation;
mod parse;
mod prompt;
mod step;
mod turn;

pub use execute::open_page;
pub use step::{run_step, DoneReport, EndReason, StepInput, StepOutcome};
pub use turn::TurnLog;

use serde::{Deserialize, Serialize};

/// Come il modello esprime l'azione.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionMode {
    Native,
    Schema,
}

/// Come l'albero degli elementi viene mostrato al modello.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    Full,
    Delta,
}

/// Parametri dell'agente per un passo.
#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub action_mode: ActionMode,
    pub snapshot_mode: SnapshotMode,
    pub thinking: bool,
    pub max_actions: u32,
    pub temperature: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub max_tokens: u32,
    pub page_text_max_chars: usize,
    /// URL base del sito sotto test (per i path relativi e l'URL mostrato).
    pub base_url: String,
    pub seed: Option<u64>,
}
