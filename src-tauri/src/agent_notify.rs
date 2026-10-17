//! Canale di notifica degli agenti: fine lavoro / richiesta di risposta di
//! una sessione di coding agent in un tab di cli-companion.
//!
//! Gli hook dei coding agent invocano questo stesso eseguibile:
//! `cli-companion --notify <evento> <agente>` (il JSON dell'hook arriva su
//! stdin, l'id del tab nella variabile `CLI_COMPANION_SESSION`).
//! La modalita' --notify spedisce una riga JSON su TCP 127.0.0.1:NOTIFY_PORT
//! all'istanza GUI, che emette l'evento Tauri `agent-event` verso il frontend.
//!
//! Nota WSL: l'exe gira sempre sull'host Windows (interop), quindi il TCP
//! resta locale e non serve alcun networking cross-VM. Solo std + serde.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Porta del listener locale. Solo 127.0.0.1: nessun prompt del firewall.
pub const NOTIFY_PORT: u16 = 17421;
/// Eventi accettati sul socket e in --notify: `test` e' il self-test, gli
/// altri arrivano dagli hook dei coding agent (vedi `agents/`).
const KNOWN_EVENTS: [&str; 5] = [
    "test",
    "session-start",
    "session-end",
    "finished",
    "needs-input",
];
/// Variabile d'ambiente impostata dal pty con l'id del tab: gli hook la
/// ereditano (anche da WSL, via WSLENV) e cosi' l'evento si lega al tab.
pub const SESSION_ENV: &str = "CLI_COMPANION_SESSION";
/// Nome evento Tauri verso il frontend.
const FRONTEND_EVENT: &str = "agent-event";
/// Tetti: stdin degli hook (il JSON di Claude Code puo' contenere l'ultimo
/// messaggio dell'assistente), riga sul socket, campi del payload.
const STDIN_MAX_BYTES: u64 = 1024 * 1024;
const WIRE_MAX_BYTES: u64 = 64 * 1024;
const MESSAGE_MAX_CHARS: usize = 300;
const FIELD_MAX_CHARS: usize = 128;
const TAB_ID_MAX: usize = 64;

/// Payload sul socket e verso il frontend (una riga JSON + `\n`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyPayload {
    pub event: String,
    pub agent: String,
    pub tab_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub notification_type: Option<String>,
    pub message: Option<String>,
}

/// Campi utili del JSON che l'agente passa all'hook su stdin.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HookInput {
    pub agent_session_id: Option<String>,
    pub notification_type: Option<String>,
    pub message: Option<String>,
}

pub fn is_known_event(event: &str) -> bool {
    KNOWN_EVENTS.contains(&event)
}

/// Id tab ammesso: ASCII alfanumerico, `-` e `_`, 1..=64 caratteri. Finisce
/// in una variabile d'ambiente e in WSLENV, quindi niente caratteri strani.
pub fn is_valid_tab_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= TAB_ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Decide se `--notify` deve spedire l'evento. Gli eventi sconosciuti vanno
/// ignorati in silenzio (exit 0): i vecchi plugin Muse ancora installati
/// continuano a chiamare `--notify stop|session-end`. Gli eventi degli agenti
/// senza id tab vengono da sessioni fuori da cli-companion: ignorati di
/// proposito. Solo `test` passa sempre. Solo un invio fallito di un evento
/// da inviare e' un vero errore.
pub fn should_send(event: &str, tab_id: Option<&str>) -> bool {
    if !is_known_event(event) {
        return false;
    }
    event == "test" || tab_id.is_some_and(is_valid_tab_id)
}

fn clip(s: &str, max_chars: usize) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.chars().take(max_chars).collect())
    }
}

/// Estrae i campi noti dal JSON dell'hook. Tollerante: JSON invalido, non
/// oggetto o campi di tipo sbagliato danno semplicemente `None`.
pub fn parse_hook_stdin(raw: &str) -> HookInput {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}'))
    else {
        return HookInput::default();
    };
    let field = |k: &str, max: usize| v.get(k).and_then(|x| x.as_str()).and_then(|s| clip(s, max));
    HookInput {
        agent_session_id: field("session_id", FIELD_MAX_CHARS),
        notification_type: field("notification_type", FIELD_MAX_CHARS),
        message: field("message", MESSAGE_MAX_CHARS),
    }
}

/// Costruisce il payload da evento, agente, id tab e (se presente) stdin.
pub fn build_payload(
    event: &str,
    agent: &str,
    tab_id: Option<&str>,
    stdin_raw: Option<&str>,
) -> NotifyPayload {
    let input = stdin_raw.map(parse_hook_stdin).unwrap_or_default();
    NotifyPayload {
        event: event.to_string(),
        agent: agent.to_string(),
        tab_id: tab_id.filter(|t| is_valid_tab_id(t)).map(str::to_string),
        agent_session_id: input.agent_session_id,
        notification_type: input.notification_type,
        message: input.message,
    }
}

fn build_line(payload: &NotifyPayload) -> Result<String, String> {
    if !is_known_event(&payload.event) {
        return Err(format!("evento sconosciuto: {}", payload.event));
    }
    serde_json::to_string(payload).map_err(|e| e.to_string())
}

fn parse_line(line: &str) -> Result<NotifyPayload, String> {
    let payload: NotifyPayload =
        serde_json::from_str(line.trim()).map_err(|_| "payload non valido".to_string())?;
    if !is_known_event(&payload.event) {
        return Err(format!("evento sconosciuto: {}", payload.event));
    }
    Ok(payload)
}

/// Spedisce un payload al listener locale (usato dalla modalita' --notify).
pub fn send_to(port: u16, payload: &NotifyPayload) -> Result<(), String> {
    let line = build_line(payload)?;
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(1500),
    )
    .map_err(|_| "app non in ascolto (avvia cli-companion)".to_string())?;
    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("invio fallito: {e}"))?;
    stream.flush().map_err(|e| format!("invio fallito: {e}"))?;
    Ok(())
}

/// Legge stdin (tetto 1 MB) solo se non e' un terminale; errori = nessun dato.
fn read_stdin_capped() -> Option<String> {
    use std::io::IsTerminal;
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = Vec::new();
    stdin
        .lock()
        .take(STDIN_MAX_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Esito di `--notify`: Ok se l'evento e' stato ignorato o inviato, Err solo
/// se l'invio di un evento da inviare fallisce.
pub fn run_notify(event: &str, agent: &str) -> Result<(), String> {
    let tab_id = std::env::var(SESSION_ENV).ok();
    if !should_send(event, tab_id.as_deref()) {
        return Ok(());
    }
    // La stdin si legge solo dopo la decisione: un evento ignorato non deve
    // restare appeso su una pipe che nessuno chiude.
    let raw = read_stdin_capped();
    let payload = build_payload(event, agent, tab_id.as_deref(), raw.as_deref());
    send_to(NOTIFY_PORT, &payload)
}

/// Legge una riga di payload dal socket (tetto 64 KB).
fn read_payload(stream: &mut TcpStream) -> Result<NotifyPayload, String> {
    let mut line = String::new();
    BufReader::new(stream.take(WIRE_MAX_BYTES))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    parse_line(&line)
}

fn handle_conn(stream: &mut TcpStream, app: &AppHandle) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let payload = read_payload(stream)?;
    app.emit(FRONTEND_EVENT, &payload).map_err(|e| e.to_string())?;
    Ok(())
}

/// Avvia il listener in un thread dedicato. Se la porta e' occupata
/// (altra istanza gia' in ascolto) esce in silenzio.
pub fn start_listener(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", NOTIFY_PORT)) {
            Ok(l) => l,
            Err(_) => return,
        };
        for stream in listener.incoming() {
            match stream {
                // Un thread per connessione: un client che non scrive nulla
                // non blocca gli altri per tutto il read timeout.
                Ok(mut s) => {
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let _ = handle_conn(&mut s, &app);
                    });
                }
                Err(_) => continue,
            }
        }
    });
}

/// Self-test del canale: lancia questo stesso exe in modalita' --notify
/// (stesso percorso degli hook, stdin nulla) e verifica l'exit code. Il ding
/// arriva poi via evento al frontend come prova udibile.
#[tauri::command]
pub async fn test_agent_notify() -> Result<String, String> {
    crate::blocking(self_test_blocking).await
}

fn self_test_blocking() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("percorso exe: {e}"))?;
    let mut cmd = Command::new(&exe);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let status = cmd
        .args(["--notify", "test", "self-test"])
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("self-test non avviabile: {e}"))?;
    if status.success() {
        Ok("segnale inviato: se senti il ding, il canale funziona.".to_string())
    } else {
        Err(format!(
            "self-test fallito (exit {:?}): l'app e' in ascolto?",
            status.code()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(event: &str) -> NotifyPayload {
        build_payload(event, "claude-code", Some("tab-1"), None)
    }

    #[test]
    fn wire_roundtrip_over_tcp() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let sent = build_payload(
            "needs-input",
            "claude-code",
            Some("tab-1"),
            Some(
                r#"{"session_id":"abc","notification_type":"permission_prompt","message":"Serve il permesso"}"#,
            ),
        );
        send_to(port, &sent).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let got = read_payload(&mut stream).unwrap();
        assert_eq!(got, sent);
        assert_eq!(got.agent_session_id.as_deref(), Some("abc"));
        assert_eq!(got.message.as_deref(), Some("Serve il permesso"));
    }

    #[test]
    fn wire_rejects_unknown_event_and_garbage() {
        assert!(build_line(&sample("nope")).is_err());
        assert!(parse_line("non json").is_err());
        assert!(parse_line(r#"{"event":"nope","agent":"x"}"#).is_err());
        assert!(send_to(1, &sample("test")).is_err()); // porta chiusa
        // Campi opzionali assenti: valido.
        let p = parse_line(r#"{"event":"finished","agent":"claude-code"}"#).unwrap();
        assert_eq!(p.tab_id, None);
    }

    #[test]
    fn should_send_decision() {
        // Legacy Muse e sconosciuti: mai.
        assert!(!should_send("stop", Some("tab-1")));
        // `session-end` ora e' un evento vero, ma il vecchio plugin Muse non
        // imposta CLI_COMPANION_SESSION: senza tab resta ignorato.
        assert!(!should_send("session-end", None));
        assert!(should_send("session-end", Some("tab-1")));
        assert!(!should_send("", Some("tab-1")));
        // Test: sempre, anche senza tab.
        assert!(should_send("test", None));
        assert!(should_send("test", Some("tab-1")));
        // Eventi degli agenti: solo con un id tab valido.
        for ev in ["session-start", "session-end", "finished", "needs-input"] {
            assert!(should_send(ev, Some("tab-1")), "{ev}");
            assert!(!should_send(ev, None), "{ev}");
            assert!(!should_send(ev, Some("")), "{ev}");
            assert!(!should_send(ev, Some("a b")), "{ev}");
        }
        // Eventi ignorati: Ok senza nemmeno tentare la connessione.
        assert!(run_notify("stop", "windows").is_ok());
        assert!(run_notify("session-end", "Ubuntu").is_ok());
    }

    #[test]
    fn tab_id_validation() {
        assert!(is_valid_tab_id("3f2b9c1e-aaaa-4bbb-8ccc-0123456789ab"));
        assert!(is_valid_tab_id("tab_1"));
        assert!(!is_valid_tab_id(""));
        assert!(!is_valid_tab_id("a:b"));
        assert!(!is_valid_tab_id("a/b"));
        assert!(!is_valid_tab_id(&"x".repeat(65)));
        assert!(is_valid_tab_id(&"x".repeat(64)));
    }

    #[test]
    fn payload_from_hook_stdin() {
        let raw = r#"{"session_id":"s-1","hook_event_name":"Notification","notification_type":"idle_prompt","message":"  Claude aspetta  ","cwd":"C:/x"}"#;
        let p = build_payload("needs-input", "claude-code", Some("t1"), Some(raw));
        assert_eq!(p.event, "needs-input");
        assert_eq!(p.agent, "claude-code");
        assert_eq!(p.tab_id.as_deref(), Some("t1"));
        assert_eq!(p.agent_session_id.as_deref(), Some("s-1"));
        assert_eq!(p.notification_type.as_deref(), Some("idle_prompt"));
        assert_eq!(p.message.as_deref(), Some("Claude aspetta"));
    }

    #[test]
    fn payload_tolerates_bad_stdin() {
        for raw in ["", "non json", "[1,2]", "null", r#"{"session_id":5,"message":null}"#] {
            let p = build_payload("finished", "claude-code", Some("t1"), Some(raw));
            assert_eq!(p.agent_session_id, None, "{raw}");
            assert_eq!(p.message, None, "{raw}");
        }
        // BOM iniziale tollerato; senza stdin nessun campo.
        let p = parse_hook_stdin("\u{feff}{\"session_id\":\"s\"}");
        assert_eq!(p.agent_session_id.as_deref(), Some("s"));
        let p = build_payload("test", "self-test", None, None);
        assert_eq!(p.tab_id, None);
        assert_eq!(p.message, None);
    }

    #[test]
    fn long_message_is_truncated() {
        let raw = format!(r#"{{"message":"{}"}}"#, "è".repeat(1000));
        let p = parse_hook_stdin(&raw);
        assert_eq!(p.message.unwrap().chars().count(), MESSAGE_MAX_CHARS);
    }
}
