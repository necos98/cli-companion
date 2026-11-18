//! Gli argomenti dei comandi Tauri `task_create` e `task_update` come li invia
//! l'interfaccia: descrivono al frontend la forma del JSON che legge `parse_spec`
//! (con il goal di `task_create`, letto da `parse_new_task`).

use super::{parse_goal_id, parse_spec, TaskKind, TaskSpec, GOAL_KEY};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// Argomento di `task_create` come lo invia l'interfaccia. Il comando riceve JSON
/// e lo legge `parse_spec`: questo tipo ne descrive la forma per il frontend.
/// L'id lo genera il backend dal titolo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct NewTaskSpec {
    pub title: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub depends_on: Option<Vec<String>>,
    /// Repository; `null` o assente = la root.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub repo: Option<String>,
    /// `null` o assente = l'agente dei worker del workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub agent: Option<String>,
    /// `null` o assente = modello dei worker del workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub model: Option<String>,
    /// `null` o assente = nessun Bot.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub bot: Option<String>,
    /// Assente = `work`. Una `review` richiede almeno una dipendenza; `plan` e' riservato all'app.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<TaskKind>,
    /// Assente = `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub e2e: Option<bool>,
    /// Goal aperto in cui entra il task; `null` o assente = task sciolto.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub goal: Option<String>,
}

/// Legge l'argomento di `task_create` (forma di `NewTaskSpec`): la specifica,
/// letta da `parse_spec`, e il goal facoltativo, che non ne fa parte.
pub fn parse_new_task(v: &Value) -> Result<(TaskSpec, Option<String>), String> {
    let mut fields = v.as_object().ok_or_else(|| "each task must be an object".to_string())?.clone();
    let goal = match fields.remove(GOAL_KEY) {
        None | Some(Value::Null) => None,
        Some(id) => Some(parse_goal_id(&id)?),
    };
    Ok((parse_spec(&Value::Object(fields), None)?, goal))
}

/// Argomento di `task_update` come lo invia l'interfaccia (letto da `parse_spec`
/// con la specifica attuale come base): i campi assenti non cambiano e `null`
/// azzera `repo`, `agent` e `model`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TaskPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub depends_on: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub repo: Option<String>,
    /// `null` = torna all'agente del workspace (o al predefinito globale).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub model: Option<String>,
    /// `null` = toglie il Bot (e la copia di effort e skill).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub bot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<TaskKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub e2e: Option<bool>,
}
