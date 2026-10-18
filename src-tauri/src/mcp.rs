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
const DEFAULT_WAIT_SECS: u64 = 90;
const MAX_WAIT_SECS: u64 = 600;
/// Margine di lettura oltre il timeout di `wait_for_events`.
const WAIT_MARGIN_SECS: u64 = 30;
const RPC_TIMEOUT_SECS: u64 = 30;

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
            _ => return Err("ruolo sconosciuto nella risposta di whoami".to_string()),
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
            "Elenca i coding agent che possono essere avviati come sessioni automatiche (id e nome). \
             L'id va nel parametro `agent` di spawn_session; il default e' claude-code.",
            json!({}),
            &[],
        ),
        tool(
            "spawn_session",
            "Avvia una sessione automatica (worker): cli-companion apre un nuovo tab nel workspace dell'orchestratore \
             (il tuo: non si sceglie) con un coding agent che esegue il task in autonomia. Torna subito, con l'id della sessione, \
             senza aspettare che il lavoro finisca. Dopo l'avvio usa wait_for_events in un ciclo per \
             sapere cosa succede. Il worker comunica con te solo tramite i suoi tool (report_fact, \
             request_help, complete_task): non leggere il suo terminale. Scrivi il task in modo \
             autosufficiente, il worker non vede la tua conversazione. Massimo 16 worker attivi per \
             orchestratore. Isolamento (es. git worktree) e coda di lavoro li decidi tu, anche tramite `cwd`.",
            json!({
                "task": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100000,
                    "description": "Istruzioni complete per il worker: obiettivo, file o cartelle coinvolti, vincoli, criteri di successo."
                },
                "title": {
                    "type": "string",
                    "maxLength": 80,
                    "description": "Titolo breve mostrato sul tab (default: le prime parole del task)."
                },
                "agent": {
                    "type": "string",
                    "description": "Id del coding agent (vedi list_agents). Default: claude-code."
                },
                "cwd": {
                    "type": "string",
                    "description": "Cartella di lavoro: percorso assoluto ed esistente, uguale o dentro la cartella del tuo workspace (niente '..'), nel suo ambiente (es. C:\\progetti\\app\\.worktrees\\x oppure /home/utente/app/.worktrees/x per WSL). Per i git worktree usa una sottocartella del workspace, es. .worktrees/. Default: la cartella del workspace."
                },
                "permission_mode": {
                    "type": "string",
                    "enum": ["default", "accept-edits", "plan", "bypass"],
                    "description": "Permessi del worker. Default: accept-edits. 'default' chiede conferma per ogni azione (il worker resta fermo in attesa dell'utente), 'plan' sola lettura. 'bypass' salta tutti i permessi, usalo con cautela: al primo avvio Claude Code mostra una finestra di conferma, che non compare solo se in ~/.claude/settings.json c'e' skipDangerousModePermissionPrompt: true."
                },
                "model": {
                    "type": "string",
                    "pattern": "^[A-Za-z0-9._-]{1,64}$",
                    "description": "Modello da usare (es. sonnet, opus o un id completo). Default: quello del coding agent."
                },
                "keep_open": {
                    "type": "boolean",
                    "description": "Se true il tab resta aperto anche dopo complete_task (default false: si chiude da solo dopo pochi secondi)."
                }
            }),
            &["task"],
        ),
        tool(
            "list_sessions",
            "Elenca le sessioni automatiche avviate da te con il loro stato: starting (task non ancora letto), \
             running, needs_input (aspetta un permesso o una risposta dell'utente), idle (fermo senza aver \
             chiamato complete_task), completed, failed, blocked, closed. Include l'ultimo report. \
             Per le novita' preferisci wait_for_events.",
            json!({}),
            &[],
        ),
        tool(
            "send_message",
            "Invia un messaggio a una tua sessione automatica: viene scritto nel suo prompt come se lo \
             digitasse l'utente. Usalo per rispondere a una help_request o per dare nuove istruzioni.",
            json!({
                "session": { "type": "string", "description": "Id della sessione (da spawn_session o list_sessions)." },
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "Testo del messaggio." }
            }),
            &["session", "text"],
        ),
        tool(
            "close_session",
            "Chiude il tab di una tua sessione automatica, anche se non ha finito. Il lavoro in corso va perso.",
            json!({
                "session": { "type": "string", "description": "Id della sessione da chiudere." }
            }),
            &["session"],
        ),
        tool(
            "wait_for_events",
            "Attende eventi dalle tue sessioni automatiche e li restituisce (svuotando la coda). Torna subito \
             se ce ne sono gia', altrimenti blocca fino al primo evento o al timeout (in tal caso `events` e' \
             vuoto: nessuna novita'). Chiamalo in un ciclo finche' i worker non hanno finito; puo' (e deve) \
             essere richiamato piu' volte. Ogni evento ha: seq, session, title, kind, text (opzionale), \
             status del worker, at (unix ms). kind: started (il worker ha letto il task), fact (fatto \
             riferito), help_request (domanda: rispondi con send_message), completed (esito finale in \
             text), needs_input (il worker aspetta un permesso dell'utente nel suo tab), \
             stopped_without_report (si e' fermato senza complete_task), exited (il tab e' stato chiuso).",
            json!({
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_WAIT_SECS,
                    "description": "Attesa massima in secondi. Default 90. Claude Code sposta in background le chiamate oltre i 2 minuti: tieni il valore sotto i 110 e richiama il tool."
                }
            }),
            &[],
        ),
    ]
}

fn worker_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "PRIMO passo di ogni sessione automatica: restituisce il task assegnato dall'orchestratore e le \
             istruzioni di lavoro. Chiamalo subito, prima di fare altro, e segui le istruzioni.",
            json!({}),
            &[],
        ),
        tool(
            "report_fact",
            "Segnala all'orchestratore un fatto utile emerso durante il lavoro (scoperta, decisione presa, \
             avanzamento importante). Non e' la conclusione: per quella usa complete_task. Nessuno legge \
             l'output del tuo terminale, quindi tutto cio' che l'orchestratore deve sapere passa da qui.",
            json!({
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "Il fatto da riferire, autosufficiente." }
            }),
            &["text"],
        ),
        tool(
            "request_help",
            "Fai una domanda all'orchestratore quando sei bloccato o serve una decisione. Dopo la chiamata \
             attendi: la risposta arriva come messaggio nel tuo prompt. Non usarlo per domande che puoi \
             risolvere da solo.",
            json!({
                "question": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "La domanda, con il contesto necessario per rispondere." }
            }),
            &["question"],
        ),
        tool(
            "complete_task",
            "Chiude il lavoro e consegna l'esito all'orchestratore. Chiamalo SEMPRE alla fine, anche se \
             fallisci o sei bloccato: senza questa chiamata l'orchestratore non sa com'e' andata. Di norma \
             il tab si chiude da solo poco dopo.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed", "blocked"],
                    "description": "completed = task riuscito; failed = fallito; blocked = non posso procedere senza intervento."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "Riassunto completo: cosa hai fatto, file toccati, esito dei test, problemi aperti."
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
fn instructions(role: Option<Role>, workspace: Option<&str>) -> String {
    match role {
        Some(Role::Orchestrator) => {
            let mut text = "cli-companion: puoi avviare sessioni automatiche, ma solo nel workspace del tuo tab: \
                i worker si aprono li' e spawn_session non permette di sceglierne un altro. Flusso: \
                spawn_session per ogni task, poi wait_for_events in ciclo; rispondi alle help_request \
                con send_message e usa close_session solo se serve."
                .to_string();
            if let Some(w) = workspace {
                text.push_str(&format!(" Il tuo workspace: {w}."));
            }
            text
        }
        Some(Role::Worker) => "cli-companion: sei una sessione automatica. Chiama get_task per primo, comunica solo con \
             report_fact e request_help e chiudi SEMPRE con complete_task."
            .to_string(),
        None => "cli-companion: tool disponibili solo nei terminali di cli-companion con l'app in esecuzione."
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
        Err(e) => Some(error_response(&Value::Null, -32700, &format!("JSON non valido: {e}"))),
    }
}

/// Gestisce un messaggio JSON-RPC. Le notifiche (senza `id`) e le risposte
/// del client non producono output.
pub fn handle_message(server: &mut Server, msg: &Value, backend: &dyn Backend) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error_response(&Value::Null, -32600, "richiesta non valida"));
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
        _ => error_response(id, -32601, &format!("metodo non supportato: {method}")),
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
                    "I tool di cli-companion non sono disponibili: {e}. Funzionano solo in un terminale \
                     di cli-companion con l'app in esecuzione."
                ),
                true,
            )
        }
    };
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return tool_result("manca il nome del tool".to_string(), true);
    };
    if !tools_for(role).iter().any(|t| t["name"] == name) {
        return tool_result(format!("tool sconosciuto o non disponibile per questa sessione: {name}"), true);
    }
    let args = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(a) if a.is_object() => a.clone(),
        Some(_) => return tool_result("arguments deve essere un oggetto".to_string(), true),
    };
    match backend.rpc(name, &args) {
        Ok(result) => tool_result(serde_json::to_string_pretty(&result).unwrap_or_default(), false),
        Err(e) => tool_result(e, true),
    }
}

// ---------------------------------------------------------------------------
// Backend reale (TCP verso l'app)
// ---------------------------------------------------------------------------

/// Attesa massima della risposta RPC: per `wait_for_events` il suo timeout
/// (limitato a 1..=600, default 90) piu' un margine, altrimenti 30 s.
fn read_timeout(method: &str, params: &Value) -> Duration {
    if method == "wait_for_events" {
        let secs = params
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_WAIT_SECS)
            .clamp(1, MAX_WAIT_SECS);
        Duration::from_secs(secs + WAIT_MARGIN_SECS)
    } else {
        Duration::from_secs(RPC_TIMEOUT_SECS)
    }
}

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
    let resp: RpcResponse = serde_json::from_str(line).map_err(|e| format!("risposta dell'app non valida: {e}"))?;
    if resp.ok {
        Ok(resp.result)
    } else {
        Err(resp.error.unwrap_or_else(|| "errore sconosciuto dell'app".to_string()))
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
            _ => Err(format!("{SESSION_ENV}/{TOKEN_ENV} non impostate (terminale non gestito da cli-companion)")),
        }
    }
}

impl Backend for TcpBackend {
    fn rpc(&self, method: &str, params: &Value) -> Result<Value, String> {
        let line = request_line(&self.session, &self.token, method, params);
        let answer = rpc_call(NOTIFY_PORT, &line, read_timeout(method, params))?;
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
                    v["workspace"] = json!({"id":"w1","name":"Proj","kind":"wsl","path":"/home/u/app"});
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
            ["list_agents", "spawn_session", "list_sessions", "send_message", "close_session", "wait_for_events"]
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
        assert!(props["permission_mode"]["description"].as_str().unwrap().contains("skipDangerousModePermissionPrompt"));
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
        assert!(text.contains("non sono disponibili") && text.contains("app non in ascolto"));
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
    fn wait_for_events_params_pass_through_and_timeout_is_clamped() {
        let b = Fake::new("orchestrator");
        let mut s = Server::default();
        send(
            &mut s,
            &b,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wait_for_events","arguments":{"timeout_secs":5000}}}),
        );
        // Il valore grezzo arriva all'app (che lo valida); l'attesa del client e' limitata.
        assert_eq!(b.calls.borrow().last().unwrap().1, json!({"timeout_secs":5000}));
        let wait = |p: Value| read_timeout("wait_for_events", &p).as_secs();
        assert_eq!(wait(json!({})), 90 + 30);
        assert_eq!(wait(json!({"timeout_secs": 5000})), 600 + 30);
        assert_eq!(wait(json!({"timeout_secs": 0})), 1 + 30);
        assert_eq!(wait(json!({"timeout_secs": 45})), 45 + 30);
        assert_eq!(wait(Value::Null), 90 + 30);
        assert_eq!(read_timeout("spawn_session", &json!({"timeout_secs": 500})).as_secs(), 30);
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
