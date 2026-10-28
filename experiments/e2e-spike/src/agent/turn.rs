//! Registro di un singolo turno del modello (una chiamata a llama-server).

use super::action::{Action, InvalidAction, InvalidKind};
use super::parse::ParsedTurn;
use crate::llama::client::{ChatResponse, Timings, Usage};
use serde::Serialize;

/// Lunghezza massima di output grezzo e ragionamento nei log.
const LOG_TEXT_MAX_CHARS: usize = 4000;

pub const OUTCOME_OK: &str = "ok";

#[derive(Serialize, Clone, Debug)]
pub struct TurnLog {
    pub turn: u32,
    pub content: Option<String>,
    pub tool_calls: Option<String>,
    pub reasoning: Option<String>,
    pub action: Option<Action>,
    pub invalid_kind: Option<InvalidKind>,
    pub invalid_message: Option<String>,
    pub outcome: String,
    pub stale_ref_retry: bool,
    pub multiple_calls: bool,
    pub settle_warning: Option<String>,
    pub llm_ms: u64,
    pub browser_ms: u64,
    pub usage: Usage,
    pub timings: Timings,
    /// Tipo dello snapshot mostrato al modello in questo turno.
    pub snapshot_kind: String,
    pub snapshot_tokens: Option<usize>,
    pub prompt_messages: usize,
    pub finish_reason: Option<String>,
}

/// Contesto dell'osservazione che alimenta il turno.
pub struct TurnContext {
    pub turn: u32,
    pub snapshot_kind: &'static str,
    pub snapshot_tokens: Option<usize>,
    pub prompt_messages: usize,
    pub llm_ms: u64,
}

impl TurnLog {
    pub fn from_response(ctx: &TurnContext, response: &ChatResponse, parsed: &ParsedTurn) -> Self {
        let (action, invalid) = split_action(&parsed.action);
        TurnLog {
            turn: ctx.turn,
            content: response.message.content.as_deref().map(truncate),
            tool_calls: response
                .message
                .tool_calls
                .as_ref()
                .and_then(|calls| serde_json::to_string(calls).ok())
                .map(|text| truncate(&text)),
            reasoning: response.message.reasoning_content.as_deref().map(truncate),
            action,
            invalid_kind: invalid.map(|i| i.kind),
            invalid_message: invalid.map(|i| i.message.clone()),
            outcome: invalid
                .map(|i| format!("invalid:{}", i.kind.label()))
                .unwrap_or_else(|| OUTCOME_OK.to_string()),
            stale_ref_retry: false,
            multiple_calls: parsed.extra_calls > 0,
            settle_warning: None,
            llm_ms: ctx.llm_ms,
            browser_ms: 0,
            usage: response.usage.clone(),
            timings: response.timings.clone(),
            snapshot_kind: ctx.snapshot_kind.to_string(),
            snapshot_tokens: ctx.snapshot_tokens,
            prompt_messages: ctx.prompt_messages,
            finish_reason: response.finish_reason.clone(),
        }
    }

    /// Turno finito per un errore della chiamata al modello.
    pub fn llm_error(ctx: &TurnContext, message: &str) -> Self {
        TurnLog {
            turn: ctx.turn,
            content: None,
            tool_calls: None,
            reasoning: None,
            action: None,
            invalid_kind: None,
            invalid_message: None,
            outcome: format!("llm_error:{}", truncate(message)),
            stale_ref_retry: false,
            multiple_calls: false,
            settle_warning: None,
            llm_ms: ctx.llm_ms,
            browser_ms: 0,
            usage: Usage::default(),
            timings: Timings::default(),
            snapshot_kind: ctx.snapshot_kind.to_string(),
            snapshot_tokens: ctx.snapshot_tokens,
            prompt_messages: ctx.prompt_messages,
            finish_reason: None,
        }
    }
}

fn split_action(
    result: &Result<Action, InvalidAction>,
) -> (Option<Action>, Option<&InvalidAction>) {
    match result {
        Ok(action) => (Some(action.clone()), None),
        Err(invalid) => (None, Some(invalid)),
    }
}

fn truncate(text: &str) -> String {
    text.chars().take(LOG_TEXT_MAX_CHARS).collect()
}
