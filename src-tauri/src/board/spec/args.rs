//! Gli argomenti dei comandi Tauri `task_create` e `task_update` come li invia
//! l'interfaccia: descrivono al frontend la forma del JSON che legge `parse_spec`.

use super::TaskKind;
use serde::Serialize;
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
