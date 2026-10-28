//! Client HTTP sincrono per `llama-server` (API compatibile OpenAI).
//! Generico: nessuna logica dell'esperimento, pensato per spostarsi in `src-tauri`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Timeout di una singola chiamata (include la generazione).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Quanto del corpo di una risposta d'errore finisce nel messaggio.
const ERROR_BODY_MAX_CHARS: usize = 500;
const TOOL_CALL_TYPE: &str = "function";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FunctionCall {
    pub name: String,
    /// Argomenti come stringa JSON (formato OpenAI).
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default = "default_tool_call_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_tool_call_type() -> String {
    TOOL_CALL_TYPE.to_string()
}

#[cfg(test)]
impl ToolCall {
    pub fn new(id: &str, name: &str, arguments: &str) -> Self {
        ToolCall {
            id: id.to_string(),
            kind: default_tool_call_type(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: arguments.to_string(),
            },
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    fn plain(role: &str, content: String) -> Self {
        ChatMessage {
            role: role.to_string(),
            content,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn system(content: String) -> Self {
        Self::plain("system", content)
    }

    pub fn user(content: String) -> Self {
        Self::plain("user", content)
    }

    pub fn assistant(content: String) -> Self {
        Self::plain("assistant", content)
    }

    pub fn assistant_with_calls(content: String, calls: Vec<ToolCall>) -> Self {
        ChatMessage {
            tool_calls: Some(calls),
            ..Self::plain("assistant", content)
        }
    }

    pub fn tool(tool_call_id: &str, content: String) -> Self {
        ChatMessage {
            tool_call_id: Some(tool_call_id.to_string()),
            ..Self::plain("tool", content)
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<Value>,
    pub temperature: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub max_tokens: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Timings {
    #[serde(default)]
    pub cache_n: u64,
    #[serde(default)]
    pub prompt_n: u64,
    #[serde(default)]
    pub prompt_ms: f64,
    #[serde(default)]
    pub prompt_per_second: f64,
    #[serde(default)]
    pub predicted_n: u64,
    #[serde(default)]
    pub predicted_ms: f64,
    #[serde(default)]
    pub predicted_per_second: f64,
}

/// Messaggio dell'assistente come restituito dal server.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Deserialize, Clone, Debug)]
struct Choice {
    #[serde(default)]
    message: AssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RawResponse {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
    #[serde(default)]
    timings: Timings,
}

#[derive(Clone, Debug)]
pub struct ChatResponse {
    pub message: AssistantMessage,
    pub finish_reason: Option<String>,
    pub usage: Usage,
    pub timings: Timings,
}

#[derive(Deserialize)]
struct TokenizeResponse {
    tokens: Vec<Value>,
}

pub struct LlamaClient {
    agent: ureq::Agent,
    base_url: String,
}

impl LlamaClient {
    pub fn new(base_url: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(REQUEST_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        LlamaClient {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, String> {
        let raw: RawResponse = self.post_json("/v1/chat/completions", request)?;
        let choice = raw
            .choices
            .into_iter()
            .next()
            .ok_or("risposta senza choices")?;
        Ok(ChatResponse {
            message: choice.message,
            finish_reason: choice.finish_reason,
            usage: raw.usage,
            timings: raw.timings,
        })
    }

    /// Numero di token del testo secondo il modello caricato.
    pub fn tokenize(&self, text: &str) -> Result<usize, String> {
        let body = serde_json::json!({ "content": text });
        let response: TokenizeResponse = self.post_json("/tokenize", &body)?;
        Ok(response.tokens.len())
    }

    fn post_json<B: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<R, String> {
        let url = format!("{}{path}", self.base_url);
        let mut response = self
            .agent
            .post(&url)
            .send_json(body)
            .map_err(|e| format!("POST {path}: {e}"))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            let excerpt: String = text.chars().take(ERROR_BODY_MAX_CHARS).collect();
            return Err(format!("POST {path}: HTTP {status}: {excerpt}"));
        }
        response
            .body_mut()
            .read_json::<R>()
            .map_err(|e| format!("POST {path}: risposta non valida: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_tool_call_response() {
        let json = r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"",
            "tool_calls":[{"id":"c1","type":"function","function":{"name":"click","arguments":"{\"ref\":\"e1\"}"}}]}}],
            "usage":{"prompt_tokens":10,"completion_tokens":5},
            "timings":{"cache_n":1,"prompt_n":9,"prompt_ms":2.0,"prompt_per_second":4500.0,"predicted_n":5,"predicted_ms":50.0,"predicted_per_second":100.0}}"#;
        let raw: RawResponse = serde_json::from_str(json).unwrap();
        let choice = raw.choices.into_iter().next().unwrap();
        assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));
        let calls = choice.message.tool_calls.unwrap();
        assert_eq!(calls[0].function.name, "click");
        assert_eq!(raw.usage.completion_tokens, 5);
        assert_eq!(raw.timings.prompt_n, 9);
    }

    #[test]
    fn response_without_timings_uses_defaults() {
        let raw: RawResponse =
            serde_json::from_str(r#"{"choices":[{"message":{"content":"hi"}}]}"#).unwrap();
        assert_eq!(raw.timings, Timings::default());
        assert_eq!(raw.usage, Usage::default());
    }

    #[test]
    fn request_skips_unset_optionals() {
        let request = ChatRequest {
            messages: vec![ChatMessage::user("x".into())],
            ..ChatRequest::default()
        };
        let value = serde_json::to_value(&request).unwrap();
        assert!(value.get("tools").is_none());
        assert!(value.get("seed").is_none());
        assert!(value["messages"][0].get("tool_calls").is_none());
    }

    #[test]
    fn tool_message_carries_call_id() {
        let value = serde_json::to_value(ChatMessage::tool("c1", "ok".into())).unwrap();
        assert_eq!(value["tool_call_id"], "c1");
        assert_eq!(value["role"], "tool");
    }
}
