//! Canale di notifica degli agenti: fine lavoro / richiesta di risposta di
//! una sessione di coding agent in un tab di cli-companion.
//!
//! Gli hook dei coding agent invocano questo stesso eseguibile:
//! `cli-companion --notify <evento> <agente>` (il JSON dell'hook arriva su
//! stdin, l'id del tab nella variabile `CLI_COMPANION_SESSION`).
//! La modalita' --notify spedisce una riga JSON su TCP 127.0.0.1:NOTIFY_PORT
//! all'istanza GUI, che emette l'evento Tauri `agent-event` verso il frontend.
//!
//! Il listener risponde a ogni notifica con una riga JSON (`{}` oppure
//! `{"block":"<motivo>"}`), scritta dopo che l'automazione ha elaborato
//! l'hook. Solo `--notify finished` (con il token) la legge, con un timeout
//! breve: se c'e' `block` stampa su stdout `{"decision":"block","reason":..}`
//! e Claude Code prosegue il turno. I client vecchi chiudono senza leggere:
//! scrivere su un socket chiuso e' innocuo. Su stdout, `--notify` non scrive
//! nient'altro.
//!
//! Sullo stesso socket passano anche le chiamate RPC delle sessioni
//! automatiche (vedi `automation`): una riga JSON con la chiave `rpc`, risposta
//! di una riga. Il server MCP (`cli-companion --mcp`) le spedisce con
//! `rpc_call`. Gli hook portano il token dell'app (`CLI_COMPANION_TOKEN`) e
//! l'automazione considera solo gli eventi col token giusto; il token non
//! arriva mai al frontend.
//!
//! Nota WSL: l'exe gira sempre sull'host Windows (interop), quindi il TCP
//! resta locale e non serve alcun networking cross-VM. Solo std + serde.

use serde::{Deserialize, Serialize};
use serde_json::Value;
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
const KNOWN_EVENTS: [&str; 6] = [
    "test",
    "session-start",
    "session-end",
    "finished",
    "needs-input",
    "permission-request",
];
/// Evento dell'hook `PermissionRequest`: il backend lo converte in
/// `needs-input` con il dettaglio di cosa aspetta il permesso.
const PERMISSION_REQUEST: &str = "permission-request";
/// Lunghezza massima del riassunto di `tool_input` (comando, path).
const TOOL_SUMMARY_MAX_CHARS: usize = 200;
/// Dopo un `permission-request` il `permission_prompt` del Notification hook
/// (arriva dopo ~6 s) per lo stesso tab e' un duplicato.
const PERMISSION_DEDUPE: Duration = Duration::from_secs(60);
/// Variabile d'ambiente impostata dal pty con l'id del tab: gli hook la
/// ereditano (anche da WSL, via WSLENV) e cosi' l'evento si lega al tab.
pub const SESSION_ENV: &str = "CLI_COMPANION_SESSION";
/// Token casuale dell'app, impostato dal pty accanto a `SESSION_ENV`: chi lo
/// conosce e' un processo figlio di un terminale di cli-companion.
pub const TOKEN_ENV: &str = "CLI_COMPANION_TOKEN";
/// Nome evento Tauri verso il frontend.
const FRONTEND_EVENT: &str = "agent-event";
/// Tetto di una riga RPC in ingresso e di una risposta letta dal client.
const RPC_MAX_BYTES: u64 = 1024 * 1024;
const RPC_RESPONSE_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Timeout di lettura della riga di richiesta (non della risposta).
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(2);
/// Tetti: stdin degli hook (il JSON di Claude Code puo' contenere l'ultimo
/// messaggio dell'assistente), riga sul socket, campi del payload.
const STDIN_MAX_BYTES: u64 = 1024 * 1024;
const WIRE_MAX_BYTES: u64 = 64 * 1024;
const MESSAGE_MAX_CHARS: usize = 300;
const FIELD_MAX_CHARS: usize = 128;
const TRANSCRIPT_PATH_MAX_CHARS: usize = 1024;
const TAB_ID_MAX: usize = 64;
/// Attesa massima della riga di risposta a `finished` (l'hook ha 10 s).
const REPLY_TIMEOUT: Duration = Duration::from_secs(4);

/// Payload sul socket e verso il frontend (una riga JSON + `\n`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyPayload {
    pub event: String,
    pub agent: String,
    pub tab_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub notification_type: Option<String>,
    pub message: Option<String>,
    /// Token dell'app (da `CLI_COMPANION_TOKEN`). Serve solo al backend:
    /// prima di emettere verso il frontend va azzerato.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Solo `finished`: Claude Code sta gia' proseguendo perche' uno Stop
    /// hook ha bloccato (`stop_hook_active` dello stdin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,
    /// Solo `finished`: lavoro in background ancora attivo (task di
    /// background non finiti + cron di sessione). `None` = dato assente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_work: Option<u32>,
    /// Solo `session-start`: come e' iniziata la sessione dell'agente
    /// (`startup`, `resume`, `clear`, `compact`, `fork`; `source` dello stdin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Solo `session-end`: perche' e' finita la sessione dell'agente (`clear`,
    /// `resume`, `logout`, `prompt_input_exit`, `other`; `reason` dello stdin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Path del transcript della sessione, come lo vede l'agente (Windows o
    /// Linux/WSL): serve al frontend per leggere i consumi di token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    /// Solo `permission-request`: tool che aspetta il permesso (es. `Bash`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// Risposta del listener a una notifica: `{}` oppure `{"block":"<motivo>"}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

impl NotifyReply {
    /// Riga JSON senza il `\n` finale.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Unica riga che `--notify` scrive su stdout: la decisione di blocco dello
/// Stop hook di Claude Code.
pub fn block_stdout_line(reason: &str) -> String {
    serde_json::json!({ "decision": "block", "reason": reason }).to_string()
}

/// Richiesta RPC sul socket: `{"rpc":1,"token":..,"session":..,"method":..,"params":{..}}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcRequest {
    pub rpc: u32,
    #[serde(default)]
    pub token: String,
    /// Id del tab chiamante.
    #[serde(default)]
    pub session: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Risposta RPC: `{"ok":true,"result":..}` oppure `{"ok":false,"error":".."}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub result: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RpcResponse {
    pub fn success(result: Value) -> Self {
        Self { ok: true, result, error: None }
    }

    pub fn failure(error: impl Into<String>) -> Self {
        Self { ok: false, result: Value::Null, error: Some(error.into()) }
    }

    /// Esito di un dispatch -> risposta.
    pub fn from_result(r: Result<Value, String>) -> Self {
        match r {
            Ok(v) => Self::success(v),
            Err(e) => Self::failure(e),
        }
    }

    /// Riga JSON senza il `\n` finale.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"ok":false,"error":"risposta non serializzabile"}"#.to_string())
    }
}

/// Una riga letta dal socket: notifica di un hook o chiamata RPC.
#[derive(Debug)]
pub enum Wire {
    Notify(NotifyPayload),
    Rpc(RpcRequest),
}

/// Campi utili del JSON che l'agente passa all'hook su stdin.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HookInput {
    pub agent_session_id: Option<String>,
    pub notification_type: Option<String>,
    pub message: Option<String>,
    pub stop_hook_active: Option<bool>,
    pub background_work: Option<u32>,
    pub source: Option<String>,
    pub reason: Option<String>,
    pub transcript_path: Option<String>,
    pub tool_name: Option<String>,
    /// Riassunto breve di `tool_input` (comando di Bash, path dei tool di file, URL).
    pub tool_summary: Option<String>,
}

/// Riassunto di `tool_input` di un tool: il comando di Bash/PowerShell, il
/// path dei tool di file, l'URL di WebFetch; altrimenti `None`.
fn tool_summary(tool: &str, input: &Value) -> Option<String> {
    let key = match tool {
        "Bash" | "PowerShell" => "command",
        "Edit" | "Write" | "MultiEdit" | "Read" => "file_path",
        "NotebookEdit" => "notebook_path",
        "WebFetch" => "url",
        _ => return None,
    };
    let raw = input.get(key)?.as_str()?;
    // Una riga sola: il comando puo' avere a capo.
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    clip(&one_line, TOOL_SUMMARY_MAX_CHARS)
}

/// Testo di `needs_input` per un permesso richiesto.
pub fn permission_text(tool: &str, summary: Option<&str>) -> String {
    match summary {
        Some(s) => format!("Permesso richiesto: {tool} \u{2014} {s}"),
        None => format!("Permesso richiesto: {tool}"),
    }
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

/// Stati di un background task che lo danno per concluso (minuscolo).
const FINISHED_TASK_STATES: [&str; 8] = [
    "completed", "failed", "killed", "stopped", "cancelled", "canceled", "done", "error",
];

/// Lavoro in background ancora attivo nel JSON di uno Stop hook: task con
/// stato non concluso (stato assente o sconosciuto = attivo) piu' i cron di
/// sessione. `None` se nessuno dei due array e' presente.
fn background_work(v: &Value) -> Option<u32> {
    let tasks = v.get("background_tasks").and_then(Value::as_array);
    let crons = v.get("session_crons").and_then(Value::as_array);
    if tasks.is_none() && crons.is_none() {
        return None;
    }
    let active = tasks.map_or(0, |t| {
        t.iter()
            .filter(|task| {
                let status = task.get("status").and_then(Value::as_str).unwrap_or("");
                !FINISHED_TASK_STATES.contains(&status.trim().to_lowercase().as_str())
            })
            .count()
    });
    Some((active + crons.map_or(0, Vec::len)).min(u32::MAX as usize) as u32)
}

/// Estrae i campi noti dal JSON dell'hook. Tollerante: JSON invalido, non
/// oggetto o campi di tipo sbagliato danno semplicemente `None`.
pub fn parse_hook_stdin(raw: &str) -> HookInput {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}'))
    else {
        return HookInput::default();
    };
    let field = |k: &str, max: usize| v.get(k).and_then(|x| x.as_str()).and_then(|s| clip(s, max));
    let tool_name = field("tool_name", FIELD_MAX_CHARS);
    let tool_summary = tool_name
        .as_deref()
        .and_then(|t| tool_summary(t, v.get("tool_input").unwrap_or(&Value::Null)));
    HookInput {
        tool_name,
        tool_summary,
        agent_session_id: field("session_id", FIELD_MAX_CHARS),
        notification_type: field("notification_type", FIELD_MAX_CHARS),
        message: field("message", MESSAGE_MAX_CHARS),
        stop_hook_active: v.get("stop_hook_active").and_then(Value::as_bool),
        background_work: background_work(&v),
        source: field("source", FIELD_MAX_CHARS),
        reason: field("reason", FIELD_MAX_CHARS),
        transcript_path: field("transcript_path", TRANSCRIPT_PATH_MAX_CHARS),
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
    let message = if event == PERMISSION_REQUEST {
        input.tool_name.as_deref().map(|t| permission_text(t, input.tool_summary.as_deref()))
    } else {
        input.message
    };
    NotifyPayload {
        tool_name: input.tool_name,
        event: event.to_string(),
        agent: agent.to_string(),
        tab_id: tab_id.filter(|t| is_valid_tab_id(t)).map(str::to_string),
        agent_session_id: input.agent_session_id,
        notification_type: input.notification_type,
        message,
        token: None,
        stop_hook_active: input.stop_hook_active,
        background_work: input.background_work,
        source: input.source,
        reason: input.reason,
        transcript_path: input.transcript_path,
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

/// Distingue una riga RPC (chiave `rpc`) da una notifica. Le notifiche
/// restano limitate a 64 KB, le RPC a 1 MB (gia' garantito dalla lettura).
fn parse_wire(line: &str) -> Result<Wire, String> {
    let v: Value = serde_json::from_str(line.trim()).map_err(|_| "payload non valido".to_string())?;
    if v.get("rpc").is_some() {
        let req: RpcRequest =
            serde_json::from_value(v).map_err(|e| format!("richiesta RPC non valida: {e}"))?;
        return Ok(Wire::Rpc(req));
    }
    if line.len() as u64 > WIRE_MAX_BYTES {
        return Err("payload troppo grande".to_string());
    }
    parse_line(line).map(Wire::Notify)
}

/// Connette al listener e spedisce la riga del payload.
fn connect_and_send(port: u16, payload: &NotifyPayload) -> Result<TcpStream, String> {
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
    Ok(stream)
}

/// Spedisce un payload al listener locale senza attendere la risposta.
pub fn send_to(port: u16, payload: &NotifyPayload) -> Result<(), String> {
    connect_and_send(port, payload).map(|_| ())
}

/// Spedisce il payload e attende la riga di risposta (al massimo `timeout`).
/// Un errore di invio e' un errore vero; timeout, chiusura o risposta
/// illeggibile danno `Ok(None)`: l'hook si comporta come senza risposta.
pub fn send_for_reply(
    port: u16,
    payload: &NotifyPayload,
    timeout: Duration,
) -> Result<Option<NotifyReply>, String> {
    let stream = connect_and_send(port, payload)?;
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return Ok(None);
    }
    let mut line = String::new();
    let read = BufReader::new(stream.take(WIRE_MAX_BYTES)).read_line(&mut line);
    if read.is_err() {
        return Ok(None);
    }
    Ok(serde_json::from_str(line.trim()).ok())
}

/// Corpo di `--notify` dopo la costruzione del payload. Per `finished` con
/// token attende la risposta del listener e, se contiene `block`, scrive su
/// `out` la decisione di blocco; altrimenti non scrive nulla.
fn notify_via(
    port: u16,
    payload: &NotifyPayload,
    timeout: Duration,
    out: &mut dyn Write,
) -> Result<(), String> {
    if payload.event != "finished" || payload.token.is_none() {
        return send_to(port, payload);
    }
    if let Some(reason) = send_for_reply(port, payload, timeout)?.and_then(|r| r.block) {
        let _ = writeln!(out, "{}", block_stdout_line(&reason));
        let _ = out.flush();
    }
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
    let mut payload = build_payload(event, agent, tab_id.as_deref(), raw.as_deref());
    payload.token = std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty());
    notify_via(NOTIFY_PORT, &payload, REPLY_TIMEOUT, &mut std::io::stdout())
}

/// Client RPC: spedisce `line` (una riga JSON, `\n` aggiunto qui) al listener
/// su `port` e ritorna la riga di risposta. `timeout` e' l'attesa massima
/// della risposta.
pub fn rpc_call(port: u16, line: &str, timeout: Duration) -> Result<String, String> {
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(1500),
    )
    .map_err(|_| "the cli-companion app is not listening (start it)".to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(Duration::from_secs(10))))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("{}\n", line.trim_end_matches(['\r', '\n'])).as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|e| format!("send failed: {e}"))?;
    let mut resp = String::new();
    BufReader::new(stream.take(RPC_RESPONSE_MAX_BYTES))
        .read_line(&mut resp)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                "timeout waiting for the response".to_string()
            }
            _ => format!("reading the response failed: {e}"),
        })?;
    if resp.trim().is_empty() {
        return Err("the app closed the connection without answering".to_string());
    }
    Ok(resp.trim_end().to_string())
}

/// Legge una riga dal socket (tetto `max` byte) e la classifica. Il timeout
/// di lettura vale solo qui, non per il tempo di elaborazione.
fn read_wire(stream: &mut TcpStream) -> Result<Wire, String> {
    stream
        .set_read_timeout(Some(REQUEST_READ_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new((&*stream).take(RPC_MAX_BYTES))
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    stream.set_read_timeout(None).map_err(|e| e.to_string())?;
    parse_wire(&line)
}

/// Serve una connessione: legge una riga, inoltra le RPC a `on_rpc` e scrive
/// la risposta; le notifiche vanno a `on_notify`, che decide la riga di
/// risposta.
fn serve_conn<R, N>(stream: &mut TcpStream, on_rpc: R, on_notify: N) -> Result<(), String>
where
    R: FnOnce(RpcRequest) -> RpcResponse,
    N: FnOnce(NotifyPayload) -> Result<NotifyReply, String>,
{
    match read_wire(stream)? {
        Wire::Notify(p) => {
            let result = on_notify(p);
            // Risposta a best effort: i client vecchi hanno gia' chiuso.
            let reply = result.as_ref().cloned().unwrap_or_default();
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = stream
                .write_all(format!("{}\n", reply.to_line()).as_bytes())
                .and_then(|_| stream.flush());
            result.map(|_| ())
        }
        Wire::Rpc(req) => {
            let resp = on_rpc(req);
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .map_err(|e| e.to_string())?;
            stream
                .write_all(format!("{}\n", resp.to_line()).as_bytes())
                .and_then(|_| stream.flush())
                .map_err(|e| e.to_string())
        }
    }
}

/// `finished` che non e' una vera fine: lo Stop e' stato bloccato (il worker
/// continua) o la sessione aspetta lavoro in background e riprendera' da
/// sola. Al frontend non va, altrimenti ding e "finito" sarebbero falsi.
fn is_paused_stop(payload: &NotifyPayload, blocked: bool) -> bool {
    payload.event == "finished" && (blocked || payload.background_work.unwrap_or(0) > 0)
}

/// Tab con un `permission-request` recente (per scartare il duplicato).
#[derive(Default)]
struct RecentPermissions(std::collections::HashMap<String, std::time::Instant>);

impl RecentPermissions {
    fn note(&mut self, tab: Option<&str>) {
        let Some(tab) = tab else { return };
        if self.0.len() > 256 {
            self.0.clear();
        }
        self.0.insert(tab.to_string(), std::time::Instant::now());
    }

    fn seen(&self, tab: Option<&str>) -> bool {
        tab.and_then(|t| self.0.get(t)).is_some_and(|at| at.elapsed() < PERMISSION_DEDUPE)
    }
}

fn recent_permission_requests() -> &'static std::sync::Mutex<RecentPermissions> {
    static R: std::sync::OnceLock<std::sync::Mutex<RecentPermissions>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

fn handle_conn(stream: &mut TcpStream, app: &AppHandle) -> Result<(), String> {
    serve_conn(
        stream,
        |req| crate::automation::handle_rpc(app, &req),
        |mut payload| {
            // Il token resta nel backend: al frontend non deve arrivare.
            let token_ok = crate::automation::token_matches(payload.token.as_deref());
            payload.token = None;
            let mut payload = payload;
            if payload.event == PERMISSION_REQUEST {
                // Per il resto dell'app e' un `needs-input` con il dettaglio.
                payload.event = "needs-input".to_string();
                payload.notification_type = Some("permission_prompt".to_string());
                recent_permission_requests().lock().unwrap().note(payload.tab_id.as_deref());
            } else if payload.event == "needs-input"
                && payload.notification_type.as_deref() == Some("permission_prompt")
                && recent_permission_requests().lock().unwrap().seen(payload.tab_id.as_deref())
            {
                // Gia' segnalato con il dettaglio da `permission-request`.
                return Ok(NotifyReply::default());
            }
            let block = if token_ok {
                crate::automation::on_hook_event(app, &payload)
            } else {
                None
            };
            if !is_paused_stop(&payload, block.is_some()) {
                app.emit(FRONTEND_EVENT, &payload).map_err(|e| e.to_string())?;
            }
            Ok(NotifyReply { block })
        },
    )
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
    fn permission_request_stdin_carries_tool_and_summary() {
        let raw = r#"{"session_id":"s1","hook_event_name":"PermissionRequest","tool_name":"Bash",
            "tool_input":{"command":"docker exec freud-manager\n  composer dump-autoload","description":"x"}}"#;
        let i = parse_hook_stdin(raw);
        assert_eq!(i.tool_name.as_deref(), Some("Bash"));
        assert_eq!(i.tool_summary.as_deref(), Some("docker exec freud-manager composer dump-autoload"));
        let p = build_payload("permission-request", "claude-code", Some("tab-1"), Some(raw));
        assert!(is_known_event(&p.event));
        assert_eq!(p.tool_name.as_deref(), Some("Bash"));
        assert_eq!(
            p.message.as_deref(),
            Some("Permesso richiesto: Bash \u{2014} docker exec freud-manager composer dump-autoload")
        );
        // Il payload sopravvive al giro sul socket.
        assert_eq!(parse_line(&build_line(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn tool_summary_by_tool_and_clipped() {
        let s = |tool: &str, input: serde_json::Value| tool_summary(tool, &input);
        assert_eq!(s("Edit", serde_json::json!({"file_path":"/a/b.rs"})).as_deref(), Some("/a/b.rs"));
        assert_eq!(s("Write", serde_json::json!({"file_path":"C:\\a.txt"})).as_deref(), Some("C:\\a.txt"));
        assert_eq!(s("WebFetch", serde_json::json!({"url":"https://x.dev"})).as_deref(), Some("https://x.dev"));
        assert_eq!(s("Task", serde_json::json!({"prompt":"x"})), None);
        assert_eq!(s("Bash", serde_json::json!({})), None);
        let long = "x".repeat(500);
        assert_eq!(s("Bash", serde_json::json!({"command": long})).unwrap().chars().count(), TOOL_SUMMARY_MAX_CHARS);
        // Tool senza riassunto: solo il nome; stdin senza tool_name: nessun messaggio.
        assert_eq!(permission_text("Task", None), "Permesso richiesto: Task");
        let p = build_payload("permission-request", "claude-code", Some("t"), Some(r#"{"tool_name":"Task","tool_input":{}}"#));
        assert_eq!(p.message.as_deref(), Some("Permesso richiesto: Task"));
        assert_eq!(build_payload("permission-request", "claude-code", Some("t"), None).message, None);
    }

    #[test]
    fn recent_permission_requests_dedupe_per_tab() {
        let mut r = RecentPermissions::default();
        assert!(!r.seen(Some("t1")));
        r.note(Some("t1"));
        assert!(r.seen(Some("t1")));
        assert!(!r.seen(Some("t2")));
        assert!(!r.seen(None));
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
        let Wire::Notify(got) = read_wire(&mut stream).unwrap() else {
            panic!("attesa una notifica");
        };
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
    fn session_start_source_and_session_end_reason_reach_the_payload() {
        let start = build_payload("session-start", "claude-code", Some("t1"), Some(r#"{"session_id":"s","source":"clear"}"#));
        assert_eq!((start.source.as_deref(), start.reason.as_deref()), (Some("clear"), None));
        let end = build_payload("session-end", "claude-code", Some("t1"), Some(r#"{"session_id":"s","reason":" other "}"#));
        assert_eq!((end.source.as_deref(), end.reason.as_deref()), (None, Some("other")));
        assert_eq!(parse_line(&build_line(&end).unwrap()).unwrap(), end);
        // Tipi sbagliati o vuoti: assenti, e assenti anche nel JSON.
        let bad = build_payload("session-end", "claude-code", Some("t1"), Some(r#"{"source":5,"reason":"  "}"#));
        assert_eq!((bad.source.as_ref(), bad.reason.as_ref()), (None, None));
        let ser = serde_json::to_string(&bad).unwrap();
        assert!(!ser.contains("\"source\"") && !ser.contains("\"reason\""), "{ser}");
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
    fn token_is_optional_and_not_serialized_when_absent() {
        let p = sample("finished");
        assert!(!serde_json::to_string(&p).unwrap().contains("token"));
        let with = parse_line(r#"{"event":"finished","agent":"x","token":"abc"}"#).unwrap();
        assert_eq!(with.token.as_deref(), Some("abc"));
        let without = parse_line(r#"{"event":"finished","agent":"x"}"#).unwrap();
        assert_eq!(without.token, None);
    }

    #[test]
    fn wire_classifies_rpc_and_notify() {
        let rpc = r#"{"rpc":1,"token":"t","session":"tab-1","method":"whoami","params":{}}"#;
        match parse_wire(rpc).unwrap() {
            Wire::Rpc(r) => {
                assert_eq!((r.rpc, r.token.as_str(), r.session.as_str()), (1, "t", "tab-1"));
                assert_eq!(r.method, "whoami");
            }
            Wire::Notify(_) => panic!("attesa una RPC"),
        }
        // Params e token assenti: parsing ok (e' l'auth a rifiutare).
        let Wire::Rpc(r) = parse_wire(r#"{"rpc":1,"method":"x"}"#).unwrap() else {
            panic!("attesa una RPC");
        };
        assert!(r.token.is_empty() && r.params.is_null());
        assert!(matches!(
            parse_wire(r#"{"event":"finished","agent":"x"}"#).unwrap(),
            Wire::Notify(_)
        ));
        assert!(parse_wire(r#"{"rpc":1}"#).is_err()); // manca method
        assert!(parse_wire("non json").is_err());
        // Le notifiche hanno il tetto di 64 KB, le RPC no.
        let big = "x".repeat(WIRE_MAX_BYTES as usize);
        let notify = format!(r#"{{"event":"finished","agent":"{big}"}}"#);
        assert!(parse_wire(&notify).is_err());
        let rpc = format!(r#"{{"rpc":1,"method":"m","params":{{"a":"{big}"}}}}"#);
        assert!(matches!(parse_wire(&rpc), Ok(Wire::Rpc(_))));
    }

    #[test]
    fn rpc_response_shapes() {
        let ok = RpcResponse::success(serde_json::json!({"a":1}));
        assert_eq!(ok.to_line(), r#"{"ok":true,"result":{"a":1}}"#);
        let err = RpcResponse::failure("no");
        assert_eq!(err.to_line(), r#"{"ok":false,"error":"no"}"#);
        let back: RpcResponse = serde_json::from_str(&ok.to_line()).unwrap();
        assert_eq!(back, ok);
        assert_eq!(RpcResponse::from_result(Err("x".into())), RpcResponse::failure("x"));
    }

    /// Roundtrip completo su una porta effimera: il lato server e' il vero
    /// `serve_conn` con un handler finto che dura piu' del timeout di lettura
    /// della richiesta (2 s), per provare che non si applica alla risposta.
    #[test]
    fn rpc_roundtrip_over_tcp_with_slow_handler() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            serve_conn(
                &mut s,
                |req| {
                    std::thread::sleep(Duration::from_millis(2300));
                    RpcResponse::success(serde_json::json!({"echo": req.method, "session": req.session}))
                },
                |_| panic!("attesa una RPC"),
            )
        });
        let line = r#"{"rpc":1,"token":"t","session":"tab-1","method":"ping","params":{}}"#;
        let resp = rpc_call(port, line, Duration::from_secs(10)).unwrap();
        let r: RpcResponse = serde_json::from_str(&resp).unwrap();
        assert!(r.ok);
        assert_eq!(r.result["echo"], "ping");
        assert_eq!(r.result["session"], "tab-1");
        server.join().unwrap().unwrap();
    }

    #[test]
    fn rpc_call_errors() {
        assert!(rpc_call(1, "{}", Duration::from_secs(1)).is_err()); // porta chiusa
        // Server che non risponde mai: timeout.
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let _keep = std::thread::spawn(move || {
            let (_s, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(1500));
        });
        let e = rpc_call(port, "{}", Duration::from_millis(300)).unwrap_err();
        assert!(e.contains("timeout"), "{e}");
    }

    #[test]
    fn notify_goes_to_notify_handler() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut sent = sample("needs-input");
        sent.token = Some("secret".to_string());
        send_to(port, &sent).unwrap();
        let (mut s, _) = listener.accept().unwrap();
        let mut got = None;
        serve_conn(&mut s, |_| panic!("attesa una notifica"), |p| {
            got = Some(p);
            Ok(NotifyReply::default())
        })
        .unwrap();
        assert_eq!(got.unwrap().token.as_deref(), Some("secret"));
    }

    #[test]
    fn background_work_counts_active_tasks_and_crons() {
        let n = |raw: &str| parse_hook_stdin(raw).background_work;
        assert_eq!(n(r#"{"session_id":"s"}"#), None);
        assert_eq!(n(r#"{"background_tasks":[],"session_crons":[]}"#), Some(0));
        assert_eq!(n(r#"{"background_tasks":[{"id":"a","status":"running"},{"id":"b"},{"status":"Pending"}]}"#), Some(3));
        assert_eq!(
            n(r#"{"background_tasks":[{"status":"completed"},{"status":" FAILED "},{"status":"killed"},{"status":"stopped"},{"status":"cancelled"},{"status":"done"},{"status":"error"}]}"#),
            Some(0)
        );
        assert_eq!(
            n(r#"{"background_tasks":[{"status":"in_progress"},{"status":"completed"}],"session_crons":[{"id":"c"},{"id":"d"}]}"#),
            Some(3)
        );
        assert_eq!(n(r#"{"session_crons":[{"id":"c"}]}"#), Some(1));
        // Tipi sbagliati: ignorati.
        assert_eq!(n(r#"{"background_tasks":"x","session_crons":5}"#), None);
        let p = parse_hook_stdin(r#"{"stop_hook_active":true}"#);
        assert_eq!(p.stop_hook_active, Some(true));
        assert_eq!(parse_hook_stdin(r#"{"stop_hook_active":"si"}"#).stop_hook_active, None);
        // Retrocompatibilita': payload vecchio senza i nuovi campi.
        let old = parse_line(r#"{"event":"finished","agent":"x"}"#).unwrap();
        assert_eq!((old.stop_hook_active, old.background_work), (None, None));
        let ser = serde_json::to_string(&old).unwrap();
        assert!(!ser.contains("stop_hook_active") && !ser.contains("background_work"));
    }

    #[test]
    fn reply_and_stdout_line_shapes() {
        assert_eq!(NotifyReply::default().to_line(), "{}");
        assert_eq!(NotifyReply { block: Some("x".into()) }.to_line(), r#"{"block":"x"}"#);
        assert_eq!(block_stdout_line("x"), r#"{"decision":"block","reason":"x"}"#);
        assert_eq!(
            block_stdout_line("say \"hi\""),
            r#"{"decision":"block","reason":"say \"hi\""}"#
        );
    }

    /// Server finto su porta effimera: legge la notifica e risponde `reply`
    /// (se `Some`), poi chiude; con `None` resta in silenzio per `hold`.
    fn fake_listener(reply: Option<&'static str>, hold: Duration) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let Wire::Notify(p) = read_wire(&mut s).unwrap() else {
                panic!("attesa una notifica");
            };
            assert_eq!(p.event, "finished");
            match reply {
                Some(r) => {
                    s.write_all(format!("{r}\n").as_bytes()).unwrap();
                }
                None => std::thread::sleep(hold),
            }
        });
        (port, h)
    }

    fn finished_with_token() -> NotifyPayload {
        let mut p = sample("finished");
        p.token = Some("tok".to_string());
        p
    }

    #[test]
    fn notify_prints_block_decision_from_reply() {
        let (port, h) = fake_listener(Some(r#"{"block":"x"}"#), Duration::ZERO);
        let mut out = Vec::new();
        notify_via(port, &finished_with_token(), Duration::from_secs(2), &mut out).unwrap();
        h.join().unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{\"decision\":\"block\",\"reason\":\"x\"}\n");
    }

    #[test]
    fn notify_prints_nothing_without_block_or_on_timeout() {
        for reply in [Some("{}"), Some("garbage"), Some(r#"{"block":null}"#)] {
            let (port, h) = fake_listener(reply, Duration::ZERO);
            let mut out = Vec::new();
            notify_via(port, &finished_with_token(), Duration::from_secs(2), &mut out).unwrap();
            h.join().unwrap();
            assert!(out.is_empty(), "{reply:?}");
        }
        // Nessuna risposta entro il timeout: Ok e stdout vuoto.
        let (port, h) = fake_listener(None, Duration::from_millis(600));
        let mut out = Vec::new();
        let t0 = std::time::Instant::now();
        notify_via(port, &finished_with_token(), Duration::from_millis(200), &mut out).unwrap();
        assert!(t0.elapsed() < Duration::from_millis(550));
        h.join().unwrap();
        assert!(out.is_empty());
        // Porta chiusa: errore di invio come prima.
        assert!(notify_via(1, &finished_with_token(), Duration::from_millis(100), &mut Vec::new()).is_err());
    }

    #[test]
    fn serve_conn_replies_to_notify_and_tolerates_legacy_client() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // Client nuovo: legge la riga con il blocco.
        let client = std::thread::spawn(move || {
            send_for_reply(port, &finished_with_token(), Duration::from_secs(2)).unwrap()
        });
        let (mut s, _) = listener.accept().unwrap();
        serve_conn(&mut s, |_| panic!("attesa una notifica"), |_| {
            Ok(NotifyReply { block: Some("r".into()) })
        })
        .unwrap();
        assert_eq!(client.join().unwrap(), Some(NotifyReply { block: Some("r".into()) }));
        // Client vecchio: invia e chiude subito; il server non deve fallire.
        send_to(port, &finished_with_token()).unwrap();
        let (mut s, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        serve_conn(&mut s, |_| panic!("attesa una notifica"), |_| {
            Ok(NotifyReply { block: Some("r".into()) })
        })
        .unwrap();
    }

    #[test]
    fn paused_stop_is_not_forwarded_to_frontend() {
        let mut p = sample("finished");
        assert!(!is_paused_stop(&p, false));
        assert!(is_paused_stop(&p, true));
        p.background_work = Some(0);
        assert!(!is_paused_stop(&p, false));
        p.background_work = Some(2);
        assert!(is_paused_stop(&p, false));
        // Solo `finished`: gli altri eventi passano sempre.
        let mut n = sample("needs-input");
        n.background_work = Some(2);
        assert!(!is_paused_stop(&n, true));
    }

    #[test]
    fn transcript_path_is_extracted_clipped_and_optional() {
        let raw = r#"{"session_id":"s","transcript_path":"  C:\\Users\\me\\.claude\\projects\\p\\s.jsonl "}"#;
        let p = build_payload("finished", "claude-code", Some("t1"), Some(raw));
        assert_eq!(p.transcript_path.as_deref(), Some(r"C:\Users\me\.claude\projects\p\s.jsonl"));
        let wire: NotifyPayload = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(wire, p);
        // Assente, vuoto o di tipo sbagliato: None e non serializzato.
        for raw in [r#"{"session_id":"s"}"#, r#"{"transcript_path":"  "}"#, r#"{"transcript_path":5}"#] {
            let p = build_payload("finished", "claude-code", Some("t1"), Some(raw));
            assert_eq!(p.transcript_path, None, "{raw}");
            assert!(!serde_json::to_string(&p).unwrap().contains("transcript_path"));
        }
        let long = format!(r#"{{"transcript_path":"{}"}}"#, "a".repeat(2000));
        assert_eq!(parse_hook_stdin(&long).transcript_path.unwrap().chars().count(), 1024);
        // Payload vecchio senza il campo: valido.
        let old = parse_line(r#"{"event":"finished","agent":"x"}"#).unwrap();
        assert_eq!(old.transcript_path, None);
    }

    #[test]
    fn long_message_is_truncated() {
        let raw = format!(r#"{{"message":"{}"}}"#, "è".repeat(1000));
        let p = parse_hook_stdin(&raw);
        assert_eq!(p.message.unwrap().chars().count(), MESSAGE_MAX_CHARS);
    }
}
