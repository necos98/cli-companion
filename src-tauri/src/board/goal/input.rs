//! Il goal di una proposta come arriva da `propose_tasks`: un goal nuovo
//! (`goal`) o uno aperto esistente (`goal_id`), validati nei limiti di
//! docs/GOALS.MD, "Modello dati". Funzioni pure.

use super::{DECISIONS_KEPT, DECISION_MAX, DONE_WHEN_ITEM_MAX, DONE_WHEN_MAX, GOAL_REQUEST_MAX, GOAL_TITLE_MAX};
use crate::board::{valid_id, ID_MAX};
use crate::validate::{clip_chars, req_text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// Campi di un goal nuovo proposti dall'Intake.
const GOAL_KEYS: [&str; 5] = ["id", "title", "request", "done_when", "decisions"];

/// Goal nuovo di una proposta: si crea quando l'utente la accetta.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct GoalSpec {
    pub id: String,
    pub title: String,
    pub request: String,
    pub done_when: Vec<String>,
    /// Assunzioni e scelte dell'utente: entrano nel registro come voci `intake`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub decisions: Vec<String>,
}

/// A quale goal vanno i task di una proposta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalGoal {
    /// Un goal nuovo, creato all'accettazione.
    New(GoalSpec),
    /// Un goal aperto esistente.
    Existing(String),
}

impl ProposalGoal {
    /// Id del goal a cui vanno i task.
    pub fn id(&self) -> &str {
        match self {
            ProposalGoal::New(spec) => &spec.id,
            ProposalGoal::Existing(id) => id,
        }
    }
}

fn invalid_id(label: &str, s: &str) -> String {
    format!(
        "invalid {label} '{}' (lowercase letters, digits and '-', 1..={ID_MAX} characters, not starting with '-')",
        clip_chars(s, 50)
    )
}

/// `goal_id` di `propose_tasks`: l'id di un goal (che esista lo verifica la board).
pub fn parse_goal_id(v: &Value) -> Result<String, String> {
    let id = v.as_str().ok_or_else(|| "goal_id must be a string".to_string())?.trim();
    if !valid_id(id) {
        return Err(invalid_id("goal_id", id));
    }
    Ok(id.to_string())
}

/// Lista di testi non vuoti, al massimo `max_items` di `max_chars` caratteri ciascuno.
fn text_list(label: &str, v: &Value, max_items: usize, max_chars: usize) -> Result<Vec<String>, String> {
    let items = v.as_array().ok_or_else(|| format!("{label} must be an array of strings"))?;
    if items.len() > max_items {
        return Err(format!("{label} has too many entries (max {max_items})"));
    }
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let text = item.as_str().ok_or_else(|| format!("{label} must be an array of strings"))?;
            req_text(&format!("{label}[{i}]"), text, max_chars)
        })
        .collect()
}

/// `goal` di `propose_tasks`: un goal nuovo. Campi sconosciuti = errore.
pub fn parse_goal_spec(v: &Value) -> Result<GoalSpec, String> {
    let obj = v.as_object().ok_or_else(|| "goal must be an object".to_string())?;
    if let Some(k) = obj.keys().find(|k| !GOAL_KEYS.contains(&k.as_str())) {
        return Err(format!("goal: unknown field '{k}' (allowed: {})", GOAL_KEYS.join(", ")));
    }
    let text = |k: &str, max: usize| -> Result<String, String> {
        match obj.get(k) {
            None | Some(Value::Null) => Err(format!("goal.{k} is missing")),
            Some(Value::String(s)) => req_text(&format!("goal.{k}"), s, max),
            Some(_) => Err(format!("goal.{k} must be a string")),
        }
    };
    let id = text("id", ID_MAX)?;
    if !valid_id(&id) {
        return Err(invalid_id("goal.id", &id));
    }
    let done_when = match obj.get("done_when") {
        None | Some(Value::Null) => return Err("goal.done_when is missing".to_string()),
        Some(v) => text_list("goal.done_when", v, DONE_WHEN_MAX, DONE_WHEN_ITEM_MAX)?,
    };
    if done_when.is_empty() {
        return Err("goal.done_when is empty: list the success criteria of the goal".to_string());
    }
    let decisions = match obj.get("decisions") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => text_list("goal.decisions", v, DECISIONS_KEPT, DECISION_MAX)?,
    };
    Ok(GoalSpec {
        id,
        title: text("title", GOAL_TITLE_MAX)?,
        request: text("request", GOAL_REQUEST_MAX)?,
        done_when,
        decisions,
    })
}
