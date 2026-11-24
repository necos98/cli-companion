//! Contratto `HookEvents` di Pi: legge il JSON che l'estensione passa su stdin
//! a `cli-companion --notify` e scrive la decisione di blocco di fine turno.
//!
//! Chiavi dello stdin (stesse dell'estensione): `session_id`, `source`,
//! `transcript_path`, `continuing`, `wait`, `message`, `reason`. Pi non chiede
//! permessi: `permission-request` non viene mai inviato.
//!
//! Il core conosce `source` = `clear`/`resume`/`compact` (la sessione utente
//! continua con un nuovo id) e `reason` = `clear`/`resume` (l'agente prosegue
//! nello stesso processo con un'altra sessione). I valori di Pi si portano a
//! questi nomi:
//! - `new` (nuova sessione, come `/clear`) diventa `clear`;
//! - `resume`, `fork` e `reload` (stessa conversazione, possibile nuovo id)
//!   diventano `resume`;
//! - `startup` e `quit` restano invariati.

use super::PiAgent;
use crate::agents::{HookEvent, HookEventKind, HookEvents, WaitKind};
use serde_json::Value;

/// Valore di `wait` scritto dall'estensione per una domanda all'utente.
const WAIT_QUESTION: &str = "question";

/// Chiave della decisione di blocco letta dall'estensione sull'ultima riga di stdout.
const CONTINUE_KEY: &str = "continue";

/// Tetti sui campi dello stdin.
const MESSAGE_MAX_CHARS: usize = 300;
const FIELD_MAX_CHARS: usize = 128;
const PATH_MAX_CHARS: usize = 1024;

impl HookEvents for PiAgent {
    fn normalize(&self, kind: HookEventKind, raw: &str) -> HookEvent {
        parse_hook_stdin(kind, raw)
    }

    /// `{"continue":"<testo>"}`: l'estensione lo invia come messaggio utente e il turno prosegue.
    fn stop_block_output(&self, reason: &str) -> String {
        serde_json::json!({ CONTINUE_KEY: reason }).to_string()
    }
}

fn clip(s: &str, max_chars: usize) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.chars().take(max_chars).collect())
    }
}

/// Tipo neutro di un `wait`; uno sconosciuto e' un'attesa generica.
fn wait_kind(wait: &str) -> WaitKind {
    match wait {
        WAIT_QUESTION => WaitKind::Question,
        _ => WaitKind::Other,
    }
}

/// Nome neutro, come lo legge il core, di `source`/`reason` di Pi.
fn neutral_transition(value: String) -> String {
    match value.as_str() {
        "new" => "clear".to_string(),
        "resume" | "fork" | "reload" => "resume".to_string(),
        _ => value,
    }
}

/// Estrae i campi noti dal JSON dell'estensione. Tollerante: JSON invalido,
/// non oggetto o campi di tipo sbagliato danno semplicemente `None`.
fn parse_hook_stdin(kind: HookEventKind, raw: &str) -> HookEvent {
    let Ok(v) = serde_json::from_str::<Value>(raw.trim_start_matches('\u{feff}')) else {
        return HookEvent::default();
    };
    let field = |k: &str, max: usize| v.get(k).and_then(Value::as_str).and_then(|s| clip(s, max));
    HookEvent {
        agent_session_id: field("session_id", FIELD_MAX_CHARS),
        wait: field("wait", FIELD_MAX_CHARS).filter(|_| kind == HookEventKind::NeedsInput).map(|w| wait_kind(&w)),
        message: field("message", MESSAGE_MAX_CHARS),
        already_continuing: v.get("continuing").and_then(Value::as_bool),
        start_source: field("source", FIELD_MAX_CHARS).map(neutral_transition),
        end_reason: field("reason", FIELD_MAX_CHARS).map(neutral_transition),
        transcript_path: field("transcript_path", PATH_MAX_CHARS),
        ..HookEvent::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(kind: HookEventKind, raw: &str) -> HookEvent {
        PiAgent.normalize(kind, raw)
    }

    #[test]
    fn session_start_maps_each_source_to_the_core_name() {
        for (source, expected) in [
            ("startup", "startup"),
            ("new", "clear"),
            ("resume", "resume"),
            ("fork", "resume"),
            ("reload", "resume"),
            ("other", "other"),
        ] {
            let raw = format!(r#"{{"session_id":"u-1","source":"{source}","transcript_path":"/h/s.jsonl"}}"#);
            let e = parse(HookEventKind::SessionStart, &raw);
            assert_eq!(e.agent_session_id.as_deref(), Some("u-1"));
            assert_eq!(e.start_source.as_deref(), Some(expected), "{source}");
            assert_eq!(e.transcript_path.as_deref(), Some("/h/s.jsonl"));
            assert_eq!((e.wait, e.already_continuing, e.end_reason), (None, None, None));
        }
    }

    #[test]
    fn session_start_keeps_a_windows_transcript_path() {
        let raw = r#"{"session_id":"u","source":"startup","transcript_path":"C:\\p\\s.jsonl"}"#;
        let e = parse(HookEventKind::SessionStart, raw);
        assert_eq!(e.transcript_path.as_deref(), Some(r"C:\p\s.jsonl"));
        let e = parse(HookEventKind::SessionStart, r#"{"session_id":"u","source":"startup"}"#);
        assert_eq!(e.transcript_path, None);
    }

    #[test]
    fn finished_tells_if_the_session_was_already_continuing() {
        for continuing in [true, false] {
            let raw = format!(r#"{{"session_id":"u-1","continuing":{continuing},"transcript_path":"/h/s.jsonl"}}"#);
            let e = parse(HookEventKind::Finished, &raw);
            assert_eq!(e.already_continuing, Some(continuing));
            assert_eq!(e.agent_session_id.as_deref(), Some("u-1"));
            assert_eq!(e.transcript_path.as_deref(), Some("/h/s.jsonl"));
        }
    }

    #[test]
    fn needs_input_maps_each_wait_to_the_neutral_kind() {
        let wait = |w: &str| parse(HookEventKind::NeedsInput, &format!(r#"{{"session_id":"u","wait":"{w}"}}"#)).wait;
        assert_eq!(wait("question"), Some(WaitKind::Question));
        assert_eq!(wait("other"), Some(WaitKind::Other));
        assert_eq!(wait("something-new"), Some(WaitKind::Other));
        assert_eq!(parse(HookEventKind::NeedsInput, r#"{"session_id":"u"}"#).wait, None);
    }

    #[test]
    fn needs_input_carries_the_message() {
        let q = parse(HookEventKind::NeedsInput, r#"{"session_id":"s","wait":"question","message":"Which branch?"}"#);
        assert_eq!((q.wait, q.message.as_deref()), (Some(WaitKind::Question), Some("Which branch?")));
        let f = parse(HookEventKind::NeedsInput, r#"{"session_id":"s","wait":"other"}"#);
        assert_eq!((f.wait, f.message), (Some(WaitKind::Other), None));
    }

    #[test]
    fn wait_is_only_read_for_needs_input() {
        for kind in [
            HookEventKind::SessionStart,
            HookEventKind::Finished,
            HookEventKind::SessionEnd,
            HookEventKind::PermissionRequest,
        ] {
            assert_eq!(parse(kind, r#"{"wait":"question"}"#).wait, None, "{kind:?}");
        }
    }

    #[test]
    fn session_end_maps_each_reason_to_the_core_name() {
        for (reason, expected) in
            [("quit", "quit"), ("new", "clear"), ("resume", "resume"), ("fork", "resume"), ("reload", "resume")]
        {
            let raw = format!(r#"{{"session_id":"u-3","reason":"{reason}"}}"#);
            let e = parse(HookEventKind::SessionEnd, &raw);
            assert_eq!(e.agent_session_id.as_deref(), Some("u-3"));
            assert_eq!(e.end_reason.as_deref(), Some(expected), "{reason}");
        }
    }

    #[test]
    fn permission_request_has_no_details_to_read() {
        let e = parse(HookEventKind::PermissionRequest, r#"{"session_id":"u-3"}"#);
        assert_eq!(e.agent_session_id.as_deref(), Some("u-3"));
        assert_eq!((e.tool_name, e.tool_summary), (None, None));
    }

    #[test]
    fn malformed_input_gives_an_event_without_details() {
        for kind in [
            HookEventKind::SessionStart,
            HookEventKind::Finished,
            HookEventKind::NeedsInput,
            HookEventKind::SessionEnd,
            HookEventKind::PermissionRequest,
        ] {
            for raw in ["", "not json", "[]", "{}", "null", "\"x\"", "42"] {
                assert_eq!(parse(kind, raw), HookEvent::default(), "{kind:?} {raw:?}");
            }
        }
    }

    #[test]
    fn values_of_the_wrong_type_or_blank_are_ignored() {
        let raw = r#"{"session_id":7,"wait":false,"message":["x"],"continuing":"yes","source":"  ","transcript_path":{},"reason":""}"#;
        assert_eq!(parse(HookEventKind::NeedsInput, raw), HookEvent::default());
    }

    #[test]
    fn a_bom_is_tolerated_and_fields_are_trimmed_and_clipped() {
        let long = "x".repeat(2000);
        let raw = format!("\u{feff}{{\"session_id\":\"  s1 \",\"message\":\"{long}\",\"transcript_path\":\"{long}\"}}");
        let e = parse(HookEventKind::NeedsInput, &raw);
        assert_eq!(e.agent_session_id.as_deref(), Some("s1"));
        assert_eq!(e.message.unwrap().chars().count(), MESSAGE_MAX_CHARS);
        assert_eq!(e.transcript_path.unwrap().chars().count(), PATH_MAX_CHARS);
        let long_id = format!(r#"{{"session_id":"{long}"}}"#);
        let e = parse(HookEventKind::Finished, &long_id);
        assert_eq!(e.agent_session_id.unwrap().chars().count(), FIELD_MAX_CHARS);
    }

    #[test]
    fn the_block_output_is_the_json_the_extension_reads() {
        let out = PiAgent.stop_block_output("Keep going: \"quote\"\nnext");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["continue"], "Keep going: \"quote\"\nnext");
        assert!(!out.contains('\n'), "una sola riga: l'estensione legge l'ultima");
    }
}
