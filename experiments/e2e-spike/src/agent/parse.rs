//! Estrazione dell'azione dalla risposta del modello (modalita' nativa e a schema).

use super::action::{from_call, Action, InvalidAction, InvalidKind};
use super::catalog::DISCRIMINATOR;
use super::ActionMode;
use crate::llama::client::{ChatResponse, ToolCall};
use serde_json::Value;

const FINISH_LENGTH: &str = "length";

pub struct ParsedTurn {
    pub action: Result<Action, InvalidAction>,
    /// Chiamate oltre la prima (modalita' nativa): la prima viene eseguita.
    pub extra_calls: usize,
    /// Prima chiamata nativa, per riscriverla nella cronologia.
    pub first_call: Option<ToolCall>,
}

impl ParsedTurn {
    fn invalid(kind: InvalidKind, message: &str) -> Self {
        ParsedTurn {
            action: Err(InvalidAction::new(kind, message)),
            extra_calls: 0,
            first_call: None,
        }
    }
}

pub fn parse_response(mode: ActionMode, response: &ChatResponse, base_url: &str) -> ParsedTurn {
    if response.finish_reason.as_deref() == Some(FINISH_LENGTH) {
        let mut parsed = ParsedTurn::invalid(
            InvalidKind::Truncated,
            "the reply was cut off because it was too long; answer with a single short action",
        );
        parsed.first_call = response
            .message
            .tool_calls
            .as_ref()
            .and_then(|c| c.first().cloned());
        return parsed;
    }
    match mode {
        ActionMode::Native => parse_native(response, base_url),
        ActionMode::Schema => parse_schema(response, base_url),
    }
}

fn parse_native(response: &ChatResponse, base_url: &str) -> ParsedTurn {
    let calls = response.message.tool_calls.as_deref().unwrap_or_default();
    let Some(first) = calls.first() else {
        return ParsedTurn::invalid(
            InvalidKind::NoAction,
            "you replied with text but did not call any tool; call exactly one tool",
        );
    };
    let action = match serde_json::from_str::<Value>(&first.function.arguments) {
        Ok(args) => from_call(&first.function.name, &args, base_url),
        Err(e) => Err(InvalidAction::new(
            InvalidKind::BadJson,
            format!("the tool arguments are not valid JSON: {e}"),
        )),
    };
    ParsedTurn {
        action,
        extra_calls: calls.len() - 1,
        first_call: Some(first.clone()),
    }
}

fn parse_schema(response: &ChatResponse, base_url: &str) -> ParsedTurn {
    let content = response.message.content.as_deref().unwrap_or("").trim();
    if content.is_empty() {
        return ParsedTurn::invalid(
            InvalidKind::NoAction,
            "your reply was empty; reply with one JSON action object",
        );
    }
    let value = match serde_json::from_str::<Value>(content) {
        Ok(value) => value,
        Err(e) => {
            return ParsedTurn::invalid(
                InvalidKind::BadJson,
                &format!("your reply is not valid JSON: {e}"),
            )
        }
    };
    let action = match value.get(DISCRIMINATOR).and_then(Value::as_str) {
        Some(name) => from_call(name, &value, base_url),
        None => Err(InvalidAction::new(
            InvalidKind::BadArguments,
            format!("the JSON object has no string field `{DISCRIMINATOR}`"),
        )),
    };
    ParsedTurn {
        action,
        extra_calls: 0,
        first_call: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::client::{AssistantMessage, Timings, Usage};

    const BASE: &str = "http://127.0.0.1:8790";

    fn response(content: Option<&str>, calls: Vec<ToolCall>, finish: &str) -> ChatResponse {
        ChatResponse {
            message: AssistantMessage {
                content: content.map(str::to_string),
                reasoning_content: None,
                tool_calls: if calls.is_empty() { None } else { Some(calls) },
            },
            finish_reason: Some(finish.to_string()),
            usage: Usage::default(),
            timings: Timings::default(),
        }
    }

    fn kind(parsed: &ParsedTurn) -> InvalidKind {
        parsed.action.clone().unwrap_err().kind
    }

    #[test]
    fn native_valid_call() {
        let calls = vec![ToolCall::new("c1", "click", r#"{"ref":"e5"}"#)];
        let parsed = parse_response(
            ActionMode::Native,
            &response(Some(""), calls, "tool_calls"),
            BASE,
        );
        assert_eq!(
            parsed.action,
            Ok(Action::Click {
                element: "e5".into()
            })
        );
        assert_eq!(parsed.extra_calls, 0);
        assert_eq!(parsed.first_call.unwrap().id, "c1");
    }

    #[test]
    fn native_multiple_calls_executes_first_and_flags_rest() {
        let calls = vec![
            ToolCall::new("c1", "press", r#"{"key":"Enter"}"#),
            ToolCall::new("c2", "click", r#"{"ref":"e1"}"#),
        ];
        let parsed = parse_response(
            ActionMode::Native,
            &response(None, calls, "tool_calls"),
            BASE,
        );
        assert_eq!(
            parsed.action,
            Ok(Action::Press {
                key: "Enter".into()
            })
        );
        assert_eq!(parsed.extra_calls, 1);
    }

    #[test]
    fn native_text_only_is_no_action() {
        let parsed = parse_response(
            ActionMode::Native,
            &response(Some("I will click"), vec![], "stop"),
            BASE,
        );
        assert_eq!(kind(&parsed), InvalidKind::NoAction);
    }

    #[test]
    fn native_bad_json_and_unknown_tool() {
        let bad = vec![ToolCall::new("c", "click", "{ref: e1")];
        let parsed = parse_response(ActionMode::Native, &response(None, bad, "tool_calls"), BASE);
        assert_eq!(kind(&parsed), InvalidKind::BadJson);
        let unknown = vec![ToolCall::new("c", "scroll", "{}")];
        let parsed = parse_response(
            ActionMode::Native,
            &response(None, unknown, "tool_calls"),
            BASE,
        );
        assert_eq!(kind(&parsed), InvalidKind::UnknownTool);
    }

    #[test]
    fn length_finish_is_truncated_in_both_modes() {
        for mode in [ActionMode::Native, ActionMode::Schema] {
            let parsed = parse_response(
                mode,
                &response(Some("{\"action\":\"cli"), vec![], "length"),
                BASE,
            );
            assert_eq!(kind(&parsed), InvalidKind::Truncated);
        }
    }

    #[test]
    fn schema_valid_pretty_printed_json() {
        let content = "{\n  \"action\": \"fill\",\n  \"ref\": \"e6\",\n  \"text\": \"Dana\"\n}";
        let parsed = parse_response(
            ActionMode::Schema,
            &response(Some(content), vec![], "stop"),
            BASE,
        );
        assert_eq!(
            parsed.action,
            Ok(Action::Fill {
                element: "e6".into(),
                text: "Dana".into()
            })
        );
    }

    #[test]
    fn schema_errors_are_classified() {
        let cases = [
            ("", InvalidKind::NoAction),
            ("click e1", InvalidKind::BadJson),
            ("{\"ref\":\"e1\"}", InvalidKind::BadArguments),
            ("{\"action\":\"scroll\"}", InvalidKind::UnknownTool),
            (
                "{\"action\":\"click\",\"ref\":\"button\"}",
                InvalidKind::InvalidRef,
            ),
            (
                "{\"action\":\"open\",\"path\":\"register\"}",
                InvalidKind::InvalidPath,
            ),
            (
                "{\"action\":\"press\",\"key\":\"a b\"}",
                InvalidKind::InvalidKey,
            ),
        ];
        for (content, expected) in cases {
            let parsed = parse_response(
                ActionMode::Schema,
                &response(Some(content), vec![], "stop"),
                BASE,
            );
            assert_eq!(kind(&parsed), expected, "{content}");
        }
    }
}
