//! Server MCP (stdio) di cli-companion: `cli-companion --mcp`.
//!
//! Lanciato da Claude Code (plugin `cli-companion-notify`), parla JSON-RPC 2.0
//! a righe su stdin/stdout ed espone i tool della coda di task. Ogni tool call
//! e' una richiesta RPC al listener dell'app (stesso socket delle notifiche,
//! vedi `agent_notify::rpc_call`).
//!
//! L'insieme dei tool dipende dal ruolo (`whoami`): un worker legge il task e
//! riferisce l'esito, un revisore (worker di un task di revisione) verifica il
//! risultato integrato e puo' proporre task di correzione; una sessione normale
//! (Intake compreso) propone task e consulta la coda. Fuori da cli-companion
//! (variabili d'ambiente assenti o app non raggiungibile) la lista dei tool e' vuota,
//! cosi' il plugin non fa rumore altrove. Le definizioni dei tool sono in `tools`.
//!
//! Su stdout passano solo messaggi MCP (una riga JSON ciascuno): in release
//! l'exe non ha console e qualunque altro output sporcherebbe il protocollo.

mod tools;

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

use crate::agent_notify::{rpc_call, RpcRequest, RpcResponse, NOTIFY_PORT, SESSION_ENV, TOKEN_ENV};
use tools::{instructions, tools_for, Role};

/// Versioni del protocollo MCP che sappiamo parlare (la prima e' il default).
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// Attesa massima della risposta di una RPC (nessun tool resta in attesa).
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Canale verso l'app: una chiamata RPC alla volta.
pub trait Backend {
    fn rpc(&self, method: &str, params: &Value) -> Result<Value, String>;
}

/// Stato del server: il ruolo e' risolto una volta sola (finche' `whoami`
/// fallisce si riprova alla richiesta successiva).
#[derive(Debug, Default)]
pub struct Server {
    role: Option<Role>,
    /// Workspace della sessione (da `whoami`), a parole: serve alle istruzioni.
    workspace: Option<String>,
}

impl Server {
    fn role(&mut self, backend: &dyn Backend) -> Result<Role, String> {
        if let Some(r) = self.role {
            return Ok(r);
        }
        let v = backend.rpc("whoami", &json!({}))?;
        let role = match v.get("role").and_then(Value::as_str) {
            Some("session") => Role::Session,
            Some("worker") => Role::Worker,
            Some("reviewer") => Role::Reviewer,
            _ => return Err("unknown role in the whoami response".to_string()),
        };
        self.workspace = v.get("workspace").and_then(|w| {
            let field = |k: &str| w.get(k).and_then(Value::as_str);
            Some(format!("{} ({}, {})", field("name")?, field("kind")?, field("path")?))
        });
        self.role = Some(role);
        Ok(role)
    }
}

// ---------------------------------------------------------------------------
// Protocollo
// ---------------------------------------------------------------------------

fn ok_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Risultato di un tool: un blocco di testo (JSON indentato o messaggio).
fn tool_result(text: String, is_error: bool) -> Value {
    let mut v = json!({ "content": [ { "type": "text", "text": text } ] });
    if is_error {
        v["isError"] = Value::Bool(true);
    }
    v
}

/// Gestisce una riga in ingresso: `None` per le notifiche (nessuna risposta).
pub fn handle_line(server: &mut Server, line: &str, backend: &dyn Backend) -> Option<Value> {
    match serde_json::from_str::<Value>(line) {
        Ok(msg) => handle_message(server, &msg, backend),
        Err(e) => Some(error_response(&Value::Null, -32700, &format!("invalid JSON: {e}"))),
    }
}

/// Gestisce un messaggio JSON-RPC. Le notifiche (senza `id`) e le risposte
/// del client non producono output.
pub fn handle_message(server: &mut Server, msg: &Value, backend: &dyn Backend) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error_response(&Value::Null, -32600, "invalid request"));
    };
    let method = obj.get("method").and_then(Value::as_str)?;
    let id = obj.get("id").filter(|v| !v.is_null())?;
    let params = obj.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| PROTOCOL_VERSIONS.contains(v))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            let role = server.role(backend).ok();
            ok_response(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "cli-companion", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instructions(role, server.workspace.as_deref()),
                }),
            )
        }
        "ping" => ok_response(id, json!({})),
        "tools/list" => {
            let tools = server.role(backend).map(tools_for).unwrap_or_default();
            ok_response(id, json!({ "tools": tools }))
        }
        "tools/call" => ok_response(id, call_tool(server, params, backend)),
        _ => error_response(id, -32601, &format!("unsupported method: {method}")),
    })
}

/// `tools/call`: gli errori (anche di RPC) tornano come risultato con
/// `isError`, cosi' l'LLM li legge e puo' reagire.
fn call_tool(server: &mut Server, params: &Value, backend: &dyn Backend) -> Value {
    let role = match server.role(backend) {
        Ok(r) => r,
        Err(e) => {
            return tool_result(
                format!(
                    "The cli-companion tools are not available: {e}. They only work in a cli-companion \
                     terminal with the app running."
                ),
                true,
            )
        }
    };
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_result("the tool name is missing".to_string(), true);
    };
    if !tools_for(role).iter().any(|t| t["name"] == name) {
        return tool_result(format!("unknown tool, or not available for this session: {name}"), true);
    }
    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(a) if a.is_object() => a.clone(),
        Some(_) => return tool_result("arguments must be an object".to_string(), true),
    };
    match backend.rpc(name, &args) {
        Ok(result) => tool_result(serde_json::to_string_pretty(&result).unwrap_or_default(), false),
        Err(e) => tool_result(e, true),
    }
}

// ---------------------------------------------------------------------------
// Backend reale (TCP verso l'app)
// ---------------------------------------------------------------------------

/// Riga RPC da spedire al listener dell'app.
fn request_line(session: &str, token: &str, method: &str, params: &Value) -> String {
    let req = RpcRequest {
        rpc: 1,
        token: token.to_string(),
        session: session.to_string(),
        method: method.to_string(),
        params: params.clone(),
    };
    serde_json::to_string(&req).unwrap_or_default()
}

/// Interpreta la riga di risposta dell'app.
fn parse_response(line: &str) -> Result<Value, String> {
    let resp: RpcResponse = serde_json::from_str(line).map_err(|e| format!("invalid response from the app: {e}"))?;
    if resp.ok {
        Ok(resp.result)
    } else {
        Err(resp.error.unwrap_or_else(|| "unknown error from the app".to_string()))
    }
}

/// Parla con l'app sulla porta locale, con sessione e token dell'ambiente.
struct TcpBackend {
    session: String,
    token: String,
}

impl TcpBackend {
    fn from_env() -> Result<Self, String> {
        let get = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        match (get(SESSION_ENV), get(TOKEN_ENV)) {
            (Some(session), Some(token)) => Ok(Self { session, token }),
            _ => Err(format!("{SESSION_ENV}/{TOKEN_ENV} not set (terminal not managed by cli-companion)")),
        }
    }
}

impl Backend for TcpBackend {
    fn rpc(&self, method: &str, params: &Value) -> Result<Value, String> {
        let line = request_line(&self.session, &self.token, method, params);
        let answer = rpc_call(NOTIFY_PORT, &line, RPC_TIMEOUT)?;
        parse_response(&answer)
    }
}

/// Backend che fallisce sempre con lo stesso motivo (ambiente mancante).
struct UnavailableBackend(String);

impl Backend for UnavailableBackend {
    fn rpc(&self, _method: &str, _params: &Value) -> Result<Value, String> {
        Err(self.0.clone())
    }
}

/// Ciclo del server: una richiesta per riga su stdin, risposta su stdout.
/// Termina (exit 0 dal chiamante) a fine stdin.
pub fn run_stdio() {
    let backend: Box<dyn Backend> = match TcpBackend::from_env() {
        Ok(b) => Box::new(b),
        Err(e) => Box::new(UnavailableBackend(e)),
    };
    let mut server = Server::default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Some(resp) = handle_line(&mut server, &line, backend.as_ref()) else {
            continue;
        };
        let text = serde_json::to_string(&resp).unwrap_or_default();
        if writeln!(out, "{text}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
