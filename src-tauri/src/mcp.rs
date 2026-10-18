//! Server MCP (stdio) di cli-companion: `cli-companion --mcp`.
//!
//! Lanciato da Claude Code (plugin `cli-companion-notify`), parla JSON-RPC 2.0
//! a righe su stdin/stdout ed espone i tool per orchestrare le sessioni
//! automatiche. Ogni tool call e' una richiesta RPC al listener dell'app
//! (stesso socket delle notifiche, vedi `agent_notify::rpc_call`).
//!
//! L'insieme dei tool dipende dal ruolo (`whoami`): un orchestratore avvia e
//! controlla i worker, un worker legge il task e riferisce l'esito. Fuori da
//! cli-companion (variabili d'ambiente assenti o app non raggiungibile) la
//! lista dei tool e' vuota, cosi' il plugin non fa rumore altrove.
//!
//! Su stdout passano solo messaggi MCP (una riga JSON ciascuno): in release
//! l'exe non ha console e qualunque altro output sporcherebbe il protocollo.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{json, Value};

use crate::agent_notify::{rpc_call, RpcRequest, RpcResponse, NOTIFY_PORT, SESSION_ENV, TOKEN_ENV};

/// Versioni del protocollo MCP che sappiamo parlare (la prima e' il default).
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// Attesa massima della risposta di una RPC (nessun tool resta in attesa).
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Ruolo del chiamante, come riportato da `whoami`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Orchestrator,
    Worker,
}

/// Canale verso l'app: una chiamata RPC alla volta.
pub trait Backend {
    fn rpc(&self, method: &str, params: &Value) -> Result<Value, String>;
}

/// Stato del server: il ruolo e' risolto una volta sola (finche' `whoami`
/// fallisce si riprova alla richiesta successiva).
#[derive(Debug, Default)]
pub struct Server {
    role: Option<Role>,
    /// Workspace dell'orchestratore (da `whoami`), a parole: serve alle istruzioni.
    workspace: Option<String>,
    /// Tetto dei permessi dei worker nel workspace (da `whoami`).
    worker_permission: Option<String>,
}


impl Server {
    fn role(&mut self, backend: &dyn Backend) -> Result<Role, String> {
        if let Some(r) = self.role {
            return Ok(r);
        }
        let v = backend.rpc("whoami", &json!({}))?;
        let role = match v.get("role").and_then(Value::as_str) {
            Some("orchestrator") => Role::Orchestrator,
            Some("worker") => Role::Worker,
            _ => return Err("unknown role in the whoami response".to_string()),
        };
        self.workspace = v.get("workspace").and_then(|w| {
            let field = |k: &str| w.get(k).and_then(Value::as_str);
            Some(format!("{} ({}, {})", field("name")?, field("kind")?, field("path")?))
        });
        self.worker_permission = v
            .get("workspace")
            .and_then(|w| w.get("worker_permission"))
            .and_then(Value::as_str)
            .map(str::to_string);
        self.role = Some(role);
        Ok(role)
    }
}

// ---------------------------------------------------------------------------
// Definizione dei tool
// ---------------------------------------------------------------------------

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

fn orchestrator_tools() -> Vec<Value> {
    vec![
        tool(
            "list_agents",
            "List the coding agents that can be started as automated sessions (id and name). \
             The id goes in the `agent` parameter of spawn_session; the default is claude-code.",
            json!({}),
            &[],
        ),
        tool(
            "spawn_session",
            "Start an automated session (worker): cli-companion opens a new tab in the orchestrator's workspace \
             (yours: it cannot be chosen) with a coding agent that carries out the task autonomously. Returns \
             immediately with the session id, without waiting for the work to finish. After starting the \
             workers, end your turn: cli-companion types a message into your prompt when they need you \
             (then call get_events). Do not wait, sleep or poll. The worker talks to you only through its \
             tools (report_fact, request_help, complete_task): do not read its terminal. Write the task so \
             it is self-contained, since the worker cannot see your conversation. Write the `task` text (and \
             any later send_message text) in English. At most 16 active workers per orchestrator. You decide \
             isolation (e.g. git worktrees) and the work queue, also through `cwd`.",
            json!({
                "task": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100000,
                    "description": "Complete instructions for the worker, in English: goal, files or folders involved, constraints, success criteria."
                },
                "title": {
                    "type": "string",
                    "maxLength": 80,
                    "description": "Short title shown on the tab (default: the first words of the task)."
                },
                "agent": {
                    "type": "string",
                    "description": "Id of the coding agent (see list_agents). Default: claude-code."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working folder: an existing absolute path, equal to or inside your workspace folder (no '..'), in its environment (e.g. C:\\projects\\app\\.worktrees\\x or /home/user/app/.worktrees/x for WSL). For git worktrees use a subfolder of the workspace, e.g. .worktrees/. Default: the workspace folder."
                },
                "permission_mode": {
                    "type": "string",
                    "enum": ["default", "accept-edits", "plan", "bypass"],
                    "description": "Worker permissions. Default: the workspace setting (`worker_permission`, see the instructions). You can only restrict it, never widen it: above the workspace maximum the call fails. From most restrictive: 'plan' (read-only), 'default' (asks for confirmation on every action: the worker stays stuck waiting for the user), 'accept-edits' (approves file edits), 'bypass' (skips all permissions, use with care). With 'bypass' Claude Code shows a confirmation dialog on first start, which does not appear only if ~/.claude/settings.json has skipDangerousModePermissionPrompt: true."
                },
                "model": {
                    "type": "string",
                    "pattern": "^[A-Za-z0-9._-]{1,64}$",
                    "description": "Model to use (e.g. sonnet, opus or a full id). Default: the coding agent's own."
                },
                "keep_open": {
                    "type": "boolean",
                    "description": "If true the tab stays open after complete_task (default false: it closes by itself after a few seconds)."
                }
            }),
            &["task"],
        ),
        tool(
            "list_sessions",
            "List the automated sessions you started, with their status: starting (task not read yet), \
             running, background (turn ended while waiting for a background command: it resumes by itself), \
             needs_input (waiting for a permission or an answer from the user), idle (stopped without having \
             called complete_task), completed, failed, blocked, closed. Includes the last report. \
             For news prefer get_events.",
            json!({}),
            &[],
        ),
        tool(
            "send_message",
            "Send a message to one of your automated sessions: it is typed into its prompt as if the user \
             wrote it. Use it to answer a help_request or to give new instructions. Write the text in English.",
            json!({
                "session": { "type": "string", "description": "Session id (from spawn_session or list_sessions)." },
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "Message text, in English." }
            }),
            &["session", "text"],
        ),
        tool(
            "close_session",
            "Close the tab of one of your automated sessions, even if it has not finished. Work in progress is lost.",
            json!({
                "session": { "type": "string", "description": "Id of the session to close." }
            }),
            &["session"],
        ),
        tool(
            "get_events",
            "Return the new events from your automated sessions and drain the queue. It never waits: \
             if nothing happened `events` is empty. Call it when cli-companion types \"New events from \
             your automated sessions\" into your prompt, handle the events, then end your turn: do not \
             call it in a loop and do not wait, sleep or poll for workers. You are woken while idle at \
             the prompt only by help_request, completed, needs_input, stopped_without_report and exited; \
             started and fact stay queued until the next call. Each event has: seq, session, title, \
             kind, text (optional), worker status, at (unix ms). kind: started (the worker read the \
             task), fact (reported fact), help_request (question: answer with send_message), completed \
             (final outcome in text), needs_input (the worker is waiting for a permission from the user \
             in its tab), stopped_without_report (it stopped without complete_task, even after being \
             nudged), exited (the tab was closed).",
            json!({}),
            &[],
        ),
    ]
}

fn worker_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "FIRST step of every automated session: returns the task assigned by the orchestrator and the \
             working instructions. Call it right away, before doing anything else, and follow the instructions.",
            json!({}),
            &[],
        ),
        tool(
            "report_fact",
            "Tell the orchestrator about a useful fact that emerged during the work (finding, decision made, \
             important progress). It is not the conclusion: use complete_task for that. Nobody reads your \
             terminal output, so everything the orchestrator needs to know goes through here.",
            json!({
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "The fact to report, self-contained." }
            }),
            &["text"],
        ),
        tool(
            "request_help",
            "Ask the orchestrator a question when you are blocked or a decision is needed. After the call \
             wait: the answer arrives as a message in your prompt. Do not use it for questions you can \
             resolve yourself.",
            json!({
                "question": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "The question, with the context needed to answer it." }
            }),
            &["question"],
        ),
        tool(
            "complete_task",
            "Finish the work and hand the outcome to the orchestrator. ALWAYS call it at the end, even if \
             you fail or are blocked: without this call the orchestrator does not know how it went. \
             Normally the tab closes by itself shortly after.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed", "blocked"],
                    "description": "completed = task succeeded; failed = task failed; blocked = cannot proceed without intervention."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "Full summary: what you did, files touched, test results, open issues."
                }
            }),
            &["status", "summary"],
        ),
    ]
}

fn tools_for(role: Role) -> Vec<Value> {
    match role {
        Role::Orchestrator => orchestrator_tools(),
        Role::Worker => worker_tools(),
    }
}

/// Istruzioni generali restituite in `initialize`.
fn instructions(role: Option<Role>, workspace: Option<&str>, ceiling: Option<&str>) -> String {
    match role {
        Some(Role::Orchestrator) => {
            let mut text = "cli-companion: you can start automated sessions, but only in the workspace of your own tab: \
                workers open there and spawn_session cannot pick another one. Flow: spawn_session for \
                each task, then end your turn. Never wait for workers (no sleep, no polling, no \
                repeated tool calls): when they need you cli-companion types a message into your \
                prompt; then call get_events, handle the events (answer help_requests with \
                send_message) and end your turn again. Use close_session only when needed. Write task \
                and message texts for workers in English."
                .to_string();
            if let Some(w) = workspace {
                text.push_str(&format!(" Your workspace: {w}."));
            }
            if let Some(c) = ceiling {
                text.push_str(&format!(
                    " Maximum worker permissions in this workspace: {c}. Without permission_mode workers start \
                     at this level; you can only restrict it, not widen it (the user changes it in the \
                     workspace settings)."
                ));
            }
            text
        }
        Some(Role::Worker) => "cli-companion: you are an automated session. Call get_task first, communicate only through \
             report_fact and request_help, and ALWAYS finish with complete_task."
            .to_string(),
        None => "cli-companion: tools are only available in cli-companion terminals with the app running."
            .to_string(),
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
                    "instructions": instructions(role, server.workspace.as_deref(), server.worker_permission.as_deref()),
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
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Backend finto: registra le chiamate e risponde con `reply`.
    struct Fake {
        role: &'static str,
        reply: Result<Value, String>,
        calls: RefCell<Vec<(String, Value)>>,
    }

    impl Fake {
        fn new(role: &'static str) -> Self {
            Self { role, reply: Ok(json!({ "ok": 1 })), calls: RefCell::new(Vec::new()) }
        }
    }

    impl Backend for Fake {
        fn rpc(&self, method: &str, params: &Value) -> Result<Value, String> {
            self.calls.borrow_mut().push((method.to_string(), params.clone()));
            if method == "whoami" {
                let mut v = json!({ "role": self.role, "session": "t1" });
                if self.role == "orchestrator" {
                    v["workspace"] = json!({"id":"w1","name":"Proj","kind":"wsl","path":"/home/u/app","worker_permission":"accept-edits"});
                }
                return Ok(v);
            }
            self.reply.clone()
        }
    }

    fn send(server: &mut Server, b: &dyn Backend, msg: Value) -> Option<Value> {
        handle_message(server, &msg, b)
    }

    fn names(resp: &Value) -> Vec<String> {
        resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn initialize_negotiates_protocol_version() {
        let b = Fake::new("orchestrator");
        for (asked, expected) in [
            ("2025-03-26", "2025-03-26"),
            ("2024-11-05", "2024-11-05"),
            ("2025-06-18", "2025-06-18"),
            ("2099-01-01", "2025-06-18"),
        ] {
            let r = send(
                &mut Server::default(),
                &b,
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":asked}}),
            )
            .unwrap();
            assert_eq!(r["id"], 1);
            assert_eq!(r["result"]["protocolVersion"], expected);
            assert_eq!(r["result"]["serverInfo"]["name"], "cli-companion");
            assert!(r["result"]["capabilities"]["tools"].is_object());
            assert!(!r["result"]["instructions"].as_str().unwrap().is_empty());
        }
        // L'orchestratore vede il proprio workspace nelle istruzioni, il worker no.
        let r = send(&mut Server::default(), &b, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
        let text = r["result"]["instructions"].as_str().unwrap();
        assert!(text.contains("Proj (wsl, /home/u/app)") && !text.contains("list_workspaces"));
        assert!(text.contains("Maximum worker permissions in this workspace: accept-edits"));
        assert!(text.contains("in English"));
        let w = Fake::new("worker");
        let r = send(&mut Server::default(), &w, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
        assert!(!r["result"]["instructions"].as_str().unwrap().contains("Proj"));
        // Senza protocolVersion: default.
        let r = send(&mut Server::default(), &b, json!({"jsonrpc":"2.0","id":"a","method":"initialize"})).unwrap();
        assert_eq!(r["id"], "a");
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn tools_list_depends_on_role() {
        let mut s = Server::default();
        let r = send(&mut s, &Fake::new("orchestrator"), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        assert_eq!(
            names(&r),
            ["list_agents", "spawn_session", "list_sessions", "send_message", "close_session", "get_events"]
        );
        let mut s = Server::default();
        let r = send(&mut s, &Fake::new("worker"), json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert_eq!(names(&r), ["get_task", "report_fact", "request_help", "complete_task"]);
        for t in r["result"]["tools"].as_array().unwrap() {
            assert!(!t["description"].as_str().unwrap().is_empty());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn schemas_match_rpc_params() {
        let spawn = &orchestrator_tools()[1];
        let props = spawn["inputSchema"]["properties"].as_object().unwrap();
        for k in ["task", "title", "agent", "cwd", "permission_mode", "model", "keep_open"] {
            assert!(props.contains_key(k), "{k}");
        }
        assert_eq!(spawn["inputSchema"]["required"], json!(["task"]));
        assert!(!props.contains_key("workspace"));
        assert!(props["cwd"]["description"].as_str().unwrap().contains(".worktrees"));
        let pm = props["permission_mode"]["description"].as_str().unwrap();
        assert!(pm.contains("skipDangerousModePermissionPrompt"));
        assert!(pm.contains("workspace setting") && pm.contains("only restrict it"));
        let desc = spawn["description"].as_str().unwrap();
        assert!(desc.contains("in English") && desc.contains("send_message"));
        assert!(props["task"]["description"].as_str().unwrap().contains("English"));
    }

    #[test]
    fn role_is_cached_and_no_backend_gives_empty_list() {
        let b = Fake::new("worker");
        let mut s = Server::default();
        for id in 1..=3 {
            send(&mut s, &b, json!({"jsonrpc":"2.0","id":id,"method":"tools/list"}));
        }
        assert_eq!(b.calls.borrow().iter().filter(|(m, _)| m == "whoami").count(), 1);

        let none = UnavailableBackend("app non in ascolto".to_string());
        let mut s = Server::default();
        let r = send(&mut s, &none, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        assert_eq!(r["result"]["tools"], json!([]));
        // initialize funziona comunque.
        let r = send(&mut s, &none, json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{}})).unwrap();
        assert!(r["result"]["capabilities"]["tools"].is_object());
        // tools/call: errore leggibile.
        let r = send(
            &mut s,
            &none,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_task","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not available") && text.contains("app non in ascolto"));
    }

    #[test]
    fn tools_call_success_returns_pretty_json_text() {
        let mut b = Fake::new("worker");
        b.reply = Ok(json!({ "delivered": true }));
        let mut s = Server::default();
        let r = send(
            &mut s,
            &b,
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"report_fact","arguments":{"text":"ciao"}}}),
        )
        .unwrap();
        assert_eq!(r["id"], 7);
        assert!(r["result"].get("isError").is_none());
        assert_eq!(r["result"]["content"][0]["type"], "text");
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), json!({"delivered": true}));
        assert!(text.contains('\n'), "JSON indentato");
        let calls = b.calls.borrow();
        assert_eq!(calls.last().unwrap(), &("report_fact".to_string(), json!({"text":"ciao"})));
    }

    #[test]
    fn tools_call_rpc_error_and_bad_calls_are_is_error() {
        let mut b = Fake::new("orchestrator");
        b.reply = Err("workspace non ammesso: x".to_string());
        let mut s = Server::default();
        let call = |s: &mut Server, params: Value| {
            send(s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params})).unwrap()
        };
        let r = call(&mut s, json!({"name":"spawn_session","arguments":{"task":"t"}}));
        assert_eq!(r["result"]["isError"], true);
        assert_eq!(r["result"]["content"][0]["text"], "workspace non ammesso: x");
        // Tool del ruolo sbagliato / inesistente / argomenti non oggetto / senza nome.
        for p in [
            json!({"name":"get_task"}),
            json!({"name":"boh"}),
            json!({"name":"list_sessions","arguments":[1]}),
            json!({"arguments":{}}),
        ] {
            let r = call(&mut s, p);
            assert_eq!(r["result"]["isError"], true);
        }
        // Nessuna di queste ha raggiunto l'app.
        assert_eq!(b.calls.borrow().iter().filter(|(m, _)| m != "whoami").count(), 1);
    }

    #[test]
    fn orchestrator_is_told_to_end_its_turn_instead_of_waiting() {
        let tools = orchestrator_tools();
        let get = tools.iter().find(|t| t["name"] == "get_events").unwrap();
        assert_eq!(get["inputSchema"]["properties"], json!({}));
        let desc = get["description"].as_str().unwrap();
        assert!(desc.contains("never waits") && desc.contains("end your turn"));
        let text = instructions(Some(Role::Orchestrator), None, None);
        assert!(text.contains("end your turn") && text.contains("Never wait"));
        // Nessun testo per il modello nomina ancora il vecchio long-poll.
        let all = serde_json::to_string(&tools).unwrap() + &text;
        assert!(!all.contains("wait_for_events") && !all.contains("timeout_secs"));
    }

    #[test]
    fn request_line_and_response_parsing() {
        let line = request_line("t1", "tok", "get_task", &json!({}));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v, json!({"rpc":1,"token":"tok","session":"t1","method":"get_task","params":{}}));
        assert!(!line.contains('\n'));
        assert_eq!(parse_response(r#"{"ok":true,"result":{"a":1}}"#).unwrap(), json!({"a":1}));
        assert_eq!(parse_response(r#"{"ok":false,"error":"no"}"#).unwrap_err(), "no");
        assert!(parse_response("zzz").is_err());
    }

    #[test]
    fn ping_unknown_method_notifications_and_parse_error() {
        let b = Fake::new("worker");
        let mut s = Server::default();
        let r = send(&mut s, &b, json!({"jsonrpc":"2.0","id":9,"method":"ping"})).unwrap();
        assert_eq!(r, json!({"jsonrpc":"2.0","id":9,"result":{}}));

        let r = send(&mut s, &b, json!({"jsonrpc":"2.0","id":10,"method":"resources/list"})).unwrap();
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 10);

        // Notifiche (note o ignote) e risposte del client: nessun output.
        for m in [
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}),
            json!({"jsonrpc":"2.0","method":"qualcosa/di/ignoto"}),
            json!({"jsonrpc":"2.0","id":3,"result":{}}),
        ] {
            assert!(send(&mut s, &b, m).is_none());
        }

        let r = handle_line(&mut s, "{non json", &b).unwrap();
        assert_eq!(r["error"]["code"], -32700);
        assert!(r["id"].is_null());
        assert_eq!(handle_line(&mut s, "[1]", &b).unwrap()["error"]["code"], -32600);
        // Nulla di tutto cio' ha toccato l'app.
        assert!(b.calls.borrow().is_empty());
    }
}
