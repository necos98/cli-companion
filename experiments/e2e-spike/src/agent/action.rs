//! Azioni del modello e loro validazione (input non fidato).

use super::catalog::{
    CLICK, DONE, FIELD_KEY, FIELD_NOTE, FIELD_PATH, FIELD_REF, FIELD_SUCCESS, FIELD_TEXT,
    FIELD_VALUE, FILL, OPEN, PRESS, SELECT,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_TEXT_CHARS: usize = 500;
const MAX_NOTE_CHARS: usize = 500;
const MAX_PATH_CHARS: usize = 300;
const MAX_KEY_CHARS: usize = 24;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Open {
        path: String,
    },
    Click {
        #[serde(rename = "ref")]
        element: String,
    },
    Fill {
        #[serde(rename = "ref")]
        element: String,
        text: String,
    },
    Select {
        #[serde(rename = "ref")]
        element: String,
        value: String,
    },
    Press {
        key: String,
    },
    Done {
        success: bool,
        note: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InvalidKind {
    NoAction,
    UnknownTool,
    BadArguments,
    BadJson,
    Truncated,
    InvalidRef,
    InvalidPath,
    InvalidKey,
}

impl InvalidKind {
    pub fn label(self) -> &'static str {
        match self {
            InvalidKind::NoAction => "no_action",
            InvalidKind::UnknownTool => "unknown_tool",
            InvalidKind::BadArguments => "bad_arguments",
            InvalidKind::BadJson => "bad_json",
            InvalidKind::Truncated => "truncated",
            InvalidKind::InvalidRef => "invalid_ref",
            InvalidKind::InvalidPath => "invalid_path",
            InvalidKind::InvalidKey => "invalid_key",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidAction {
    pub kind: InvalidKind,
    pub message: String,
}

impl InvalidAction {
    pub fn new(kind: InvalidKind, message: impl Into<String>) -> Self {
        InvalidAction {
            kind,
            message: message.into(),
        }
    }
}

/// Costruisce e valida un'azione da nome + argomenti.
pub fn from_call(name: &str, args: &Value, base_url: &str) -> Result<Action, InvalidAction> {
    if !is_known(name) {
        return Err(InvalidAction::new(
            InvalidKind::UnknownTool,
            format!("unknown action `{name}`"),
        ));
    }
    if !args.is_object() {
        return Err(bad_arguments("arguments must be a JSON object"));
    }
    match name {
        OPEN => Ok(Action::Open {
            path: valid_path(string_field(args, FIELD_PATH)?, base_url)?,
        }),
        CLICK => Ok(Action::Click {
            element: valid_ref(string_field(args, FIELD_REF)?)?,
        }),
        FILL => Ok(Action::Fill {
            element: valid_ref(string_field(args, FIELD_REF)?)?,
            text: bounded_text(string_field(args, FIELD_TEXT)?, FIELD_TEXT)?,
        }),
        SELECT => Ok(Action::Select {
            element: valid_ref(string_field(args, FIELD_REF)?)?,
            value: bounded_text(string_field(args, FIELD_VALUE)?, FIELD_VALUE)?,
        }),
        PRESS => Ok(Action::Press {
            key: valid_key(string_field(args, FIELD_KEY)?)?,
        }),
        _ => Ok(Action::Done {
            success: bool_field(args, FIELD_SUCCESS)?,
            note: string_field(args, FIELD_NOTE)?
                .chars()
                .take(MAX_NOTE_CHARS)
                .collect(),
        }),
    }
}

fn is_known(name: &str) -> bool {
    [OPEN, CLICK, FILL, SELECT, PRESS, DONE].contains(&name)
}

fn bad_arguments(message: impl Into<String>) -> InvalidAction {
    InvalidAction::new(InvalidKind::BadArguments, message)
}

fn string_field<'a>(args: &'a Value, name: &str) -> Result<&'a str, InvalidAction> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad_arguments(format!("missing or non-string field `{name}`")))
}

fn bool_field(args: &Value, name: &str) -> Result<bool, InvalidAction> {
    args.get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| bad_arguments(format!("missing or non-boolean field `{name}`")))
}

fn bounded_text(text: &str, field: &str) -> Result<String, InvalidAction> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(bad_arguments(format!(
            "`{field}` is longer than {MAX_TEXT_CHARS} characters"
        )));
    }
    Ok(text.to_string())
}

/// Un ref valido e' `e<cifre>` con `@` opzionale; restituisce la forma senza `@`.
fn valid_ref(raw: &str) -> Result<String, InvalidAction> {
    let element = raw.strip_prefix('@').unwrap_or(raw);
    let digits = element.strip_prefix('e').unwrap_or("");
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(InvalidAction::new(
            InvalidKind::InvalidRef,
            format!("`{raw}` is not a valid element ref (expected a ref like e12)"),
        ));
    }
    Ok(element.to_string())
}

fn valid_key(raw: &str) -> Result<String, InvalidAction> {
    let valid = !raw.is_empty()
        && raw.chars().count() <= MAX_KEY_CHARS
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '+');
    if valid {
        Ok(raw.to_string())
    } else {
        Err(InvalidAction::new(
            InvalidKind::InvalidKey,
            format!("`{raw}` is not a valid key name (expected e.g. Enter, Tab, Control+a)"),
        ))
    }
}

/// Path del sito (`/x`, non `//x`) oppure URL assoluto sotto l'URL base.
fn valid_path(raw: &str, base_url: &str) -> Result<String, InvalidAction> {
    let clean = raw.chars().count() <= MAX_PATH_CHARS
        && !raw
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\');
    let site_path = raw.starts_with('/') && !raw.starts_with("//");
    if clean && (site_path || is_under_base(raw, base_url)) {
        Ok(raw.to_string())
    } else {
        Err(InvalidAction::new(
            InvalidKind::InvalidPath,
            format!("`{raw}` is not a valid path (use a site-relative path such as /register)"),
        ))
    }
}

fn is_under_base(url: &str, base_url: &str) -> bool {
    url.strip_prefix(base_url)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
}

/// URL completo da aprire per un path gia' validato.
pub fn resolve_url(path: &str, base_url: &str) -> String {
    if path.starts_with('/') {
        format!("{base_url}{path}")
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE: &str = "http://127.0.0.1:8790";

    fn call(name: &str, args: Value) -> Result<Action, InvalidAction> {
        from_call(name, &args, BASE)
    }

    fn kind(name: &str, args: Value) -> InvalidKind {
        call(name, args).unwrap_err().kind
    }

    #[test]
    fn click_accepts_ref_with_or_without_at() {
        let expected = Action::Click {
            element: "e12".into(),
        };
        assert_eq!(call("click", json!({"ref": "e12"})), Ok(expected.clone()));
        assert_eq!(call("click", json!({"ref": "@e12"})), Ok(expected));
    }

    #[test]
    fn invalid_refs_are_rejected() {
        for bad in ["12", "e", "ex1", "button", "@@e1", "e1 ", ""] {
            assert_eq!(
                kind("click", json!({"ref": bad})),
                InvalidKind::InvalidRef,
                "{bad}"
            );
        }
    }

    #[test]
    fn open_accepts_site_paths_and_base_urls() {
        assert!(call("open", json!({"path": "/register"})).is_ok());
        assert!(call("open", json!({"path": "/app?x=1"})).is_ok());
        assert!(call("open", json!({"path": format!("{BASE}/login")})).is_ok());
        assert!(call("open", json!({"path": BASE})).is_ok());
    }

    #[test]
    fn open_rejects_foreign_and_malformed_targets() {
        for bad in [
            "register",
            "//evil.test/x",
            "http://evil.test/",
            "http://127.0.0.1:87900/",
            "http://127.0.0.1:8790.evil.test/",
            "/a b",
            "/a\\b",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(
                kind("open", json!({"path": bad})),
                InvalidKind::InvalidPath,
                "{bad}"
            );
        }
    }

    #[test]
    fn press_validates_key_names() {
        assert!(call("press", json!({"key": "Control+a"})).is_ok());
        assert_eq!(
            kind("press", json!({"key": "Ctrl A"})),
            InvalidKind::InvalidKey
        );
        assert_eq!(kind("press", json!({"key": ""})), InvalidKind::InvalidKey);
        assert_eq!(
            kind("press", json!({"key": "A".repeat(25)})),
            InvalidKind::InvalidKey
        );
    }

    #[test]
    fn fill_text_is_bounded() {
        assert!(call("fill", json!({"ref": "e1", "text": "a".repeat(500)})).is_ok());
        assert_eq!(
            kind("fill", json!({"ref": "e1", "text": "a".repeat(501)})),
            InvalidKind::BadArguments
        );
    }

    #[test]
    fn missing_or_mistyped_fields_are_bad_arguments() {
        assert_eq!(
            kind("fill", json!({"ref": "e1"})),
            InvalidKind::BadArguments
        );
        assert_eq!(
            kind("done", json!({"success": "yes", "note": "x"})),
            InvalidKind::BadArguments
        );
        assert_eq!(kind("click", json!(["e1"])), InvalidKind::BadArguments);
    }

    #[test]
    fn unknown_action_is_reported() {
        assert_eq!(kind("scroll", json!({})), InvalidKind::UnknownTool);
    }

    #[test]
    fn done_keeps_flag_and_truncates_note() {
        let action = call("done", json!({"success": false, "note": "n".repeat(900)})).unwrap();
        match action {
            Action::Done { success, note } => {
                assert!(!success);
                assert_eq!(note.chars().count(), 500);
            }
            other => panic!("azione inattesa: {other:?}"),
        }
    }

    #[test]
    fn action_serializes_like_the_schema() {
        let value = serde_json::to_value(Action::Click {
            element: "e3".into(),
        })
        .unwrap();
        assert_eq!(value, json!({"action": "click", "ref": "e3"}));
    }

    #[test]
    fn resolve_url_joins_site_paths_only() {
        assert_eq!(resolve_url("/x", BASE), "http://127.0.0.1:8790/x");
        assert_eq!(
            resolve_url("http://127.0.0.1:8790/y", BASE),
            "http://127.0.0.1:8790/y"
        );
    }
}
