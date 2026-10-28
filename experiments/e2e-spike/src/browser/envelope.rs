//! Parsing dell'envelope JSON `{"success","data","error"}` di agent-browser.

use serde_json::Value;
use std::fmt;

const STALE_REF_PREFIX: &str = "Unknown ref";
const ERROR_MAX_CHARS: usize = 400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserError {
    /// Il ref non esiste piu' (pagina cambiata).
    StaleRef(String),
    Command(String),
    Timeout,
    Spawn(String),
    BadOutput(String),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BrowserError::StaleRef(msg) | BrowserError::Command(msg) => write!(f, "{msg}"),
            BrowserError::Timeout => write!(f, "agent-browser: timeout"),
            BrowserError::Spawn(msg) => write!(f, "agent-browser non avviabile: {msg}"),
            BrowserError::BadOutput(msg) => write!(f, "output di agent-browser non valido: {msg}"),
        }
    }
}

/// Interpreta stdout/stderr di un comando; restituisce `data` (o `Null`).
pub fn parse_output(exit_ok: bool, stdout: &str, stderr: &str) -> Result<Value, BrowserError> {
    match parse_envelope(stdout) {
        Some(envelope) => interpret(&envelope),
        None if !exit_ok => Err(BrowserError::Command(truncate(first_text(stderr, stdout)))),
        None => Err(BrowserError::BadOutput(truncate(stdout.trim()))),
    }
}

fn interpret(envelope: &Value) -> Result<Value, BrowserError> {
    let success = envelope.get("success").and_then(Value::as_bool);
    match success {
        Some(true) => Ok(envelope.get("data").cloned().unwrap_or(Value::Null)),
        Some(false) => {
            let message = envelope
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("errore sconosciuto");
            if message.starts_with(STALE_REF_PREFIX) {
                Err(BrowserError::StaleRef(truncate(message)))
            } else {
                Err(BrowserError::Command(truncate(message)))
            }
        }
        None => Err(BrowserError::BadOutput(
            "envelope senza `success`".to_string(),
        )),
    }
}

/// L'envelope e' l'ultima riga JSON di stdout (o l'intero stdout).
fn parse_envelope(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .or_else(|| {
            trimmed
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        })
        .filter(Value::is_object)
}

fn first_text<'a>(primary: &'a str, fallback: &'a str) -> &'a str {
    if primary.trim().is_empty() {
        fallback.trim()
    } else {
        primary.trim()
    }
}

fn truncate(text: &str) -> String {
    text.chars().take(ERROR_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_returns_data() {
        let out = r#"{"success":true,"data":{"url":"http://x/"},"error":null}"#;
        let data = parse_output(true, out, "").unwrap();
        assert_eq!(data["url"], "http://x/");
    }

    #[test]
    fn unknown_ref_is_stale() {
        let out = r#"{"success":false,"data":null,"error":"Unknown ref: e1"}"#;
        assert_eq!(
            parse_output(false, out, ""),
            Err(BrowserError::StaleRef("Unknown ref: e1".to_string()))
        );
    }

    #[test]
    fn other_errors_are_commands() {
        let out = r#"{"success":false,"data":null,"error":"Element not visible"}"#;
        assert_eq!(
            parse_output(false, out, ""),
            Err(BrowserError::Command("Element not visible".to_string()))
        );
    }

    #[test]
    fn envelope_is_found_after_noise_lines() {
        let out = "warming up\n{\"success\":true,\"data\":null,\"error\":null}\n";
        assert_eq!(parse_output(true, out, ""), Ok(Value::Null));
    }

    #[test]
    fn failure_without_envelope_uses_stderr() {
        assert_eq!(
            parse_output(false, "", "boom"),
            Err(BrowserError::Command("boom".to_string()))
        );
    }

    #[test]
    fn success_exit_without_envelope_is_bad_output() {
        assert!(matches!(
            parse_output(true, "not json", ""),
            Err(BrowserError::BadOutput(_))
        ));
    }
}
