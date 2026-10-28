use super::tools::{session_tools, worker_tools};
use super::*;
use crate::agents::PermissionMode;
use crate::automation::settings::REASON_MAX;
use crate::worker_permissions::{MAX_RULES, MAX_RULE_CHARS};
use crate::workspaces::{
    DeliveryMode, MergeMethod, MAX_COMMAND_LEN, MAX_LAUNCH_NAME_LEN, MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS,
    MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_RULES_LEN,
};
use std::cell::RefCell;

/// Tool definitions and server instructions are sent to the model in every
/// session: the ceilings are intentional. Do not raise one to fit new text,
/// shorten the text.
/// 5_600 = the previous ceiling plus the three skill tools (mostly their validation limits).
/// 8_700 = 5_600 plus the two settings tools (propose_settings is a schema of eleven fields with
/// their limits and enums, measured 8_604 after trimming every description that was not needed).
const SESSION_TOOLS_CEILING: usize = 8_700;
const WORKER_TOOLS_CEILING: usize = 2_600;
const SESSION_INSTRUCTIONS_CEILING: usize = 700;

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
            if self.role == "session" {
                v["workspace"] = json!({"name":"Proj","kind":"wsl","path":"/home/u/app"});
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
    resp["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect()
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[test]
fn size_budget_of_the_model_facing_text() {
    let session = serde_json::to_string(&session_tools()).unwrap().chars().count();
    let worker = serde_json::to_string(&worker_tools()).unwrap().chars().count();
    let instr = instructions(Some(Role::Session), Some("Proj (wsl, /home/u/app)")).chars().count();
    assert!(session <= SESSION_TOOLS_CEILING, "session tools are {session} chars, ceiling {SESSION_TOOLS_CEILING}");
    assert!(worker <= WORKER_TOOLS_CEILING, "worker tools are {worker} chars, ceiling {WORKER_TOOLS_CEILING}");
    assert!(instr <= SESSION_INSTRUCTIONS_CEILING, "session instructions are {instr} chars, ceiling {SESSION_INSTRUCTIONS_CEILING}");
}

#[test]
fn initialize_negotiates_protocol_version() {
    let b = Fake::new("session");
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
    // La sessione vede il proprio workspace; il testo dice che i task si propongono e non si attende.
    let r = send(&mut Server::default(), &b, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(text.contains("Proj (wsl, /home/u/app)"));
    assert!(text.contains("propose_tasks") && text.contains("accepts or discards") && text.contains("Never wait"));
    assert!(text.contains("save_skill") && text.contains("global skills"));
    assert!(text.contains("propose_settings") && text.contains("approves them in the app"));
    let w = Fake::new("worker");
    let r = send(&mut Server::default(), &w, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(!text.contains("Proj") && text.contains("complete_task"));
    // Senza protocolVersion: default.
    let r = send(&mut Server::default(), &b, json!({"jsonrpc":"2.0","id":"a","method":"initialize"})).unwrap();
    assert_eq!(r["id"], "a");
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn tools_list_depends_on_role() {
    let mut s = Server::default();
    let r = send(&mut s, &Fake::new("session"), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
    assert_eq!(
        names(&r),
        [
            "propose_tasks", "list_tasks", "get_task_details", "update_task", "delete_task", "save_skill", "list_skills",
            "delete_skill", "get_workspace_settings", "propose_settings"
        ]
    );
    let mut s = Server::default();
    let r = send(&mut s, &Fake::new("worker"), json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    assert_eq!(names(&r), ["get_task", "complete_task"]);
    for t in r["result"]["tools"].as_array().unwrap() {
        assert!(!t["description"].as_str().unwrap().is_empty());
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn no_orchestrator_tool_survives() {
    let all: Vec<Value> = session_tools().into_iter().chain(worker_tools()).collect();
    for gone in [
        "spawn_session", "list_sessions", "send_message", "close_session", "get_events", "submit_plan", "get_board",
        "set_task_status", "set_board", "get_workflow", "save_workflow", "ask_user", "report_fact",
        "request_help", "list_agents",
    ] {
        assert!(all.iter().all(|t| t["name"] != gone), "{gone}");
    }
    // Tutto in inglese (ASCII) e ogni tool ha descrizione e schema a oggetto.
    for t in &all {
        assert!(t["description"].as_str().unwrap().is_ascii(), "{}", t["name"]);
        assert!(serde_json::to_string(&t["inputSchema"]).unwrap().is_ascii(), "{}", t["name"]);
        assert_eq!(t["inputSchema"]["type"], "object");
    }
}

#[test]
fn schemas_match_the_rust_validation() {
    let tools = session_tools();
    let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
    let propose = by("propose_tasks");
    let item = &propose["inputSchema"]["properties"]["tasks"]["items"];
    assert_eq!(keys(&item["properties"]), ["depends_on", "id", "model", "repo", "task", "title"]);
    assert_eq!(item["required"], json!(["id", "title", "task", "model"]));
    assert_eq!(item["properties"]["model"]["type"], "string");
    assert_eq!(item["additionalProperties"], false);
    assert_eq!(propose["inputSchema"]["required"], json!(["tasks"]));
    assert_eq!(propose["inputSchema"]["properties"]["tasks"]["maxItems"], 20);
    assert!(propose["description"].as_str().unwrap().contains("never wait"));
    let upd = by("update_task");
    assert_eq!(keys(&upd["inputSchema"]["properties"]), ["depends_on", "id", "model", "repo", "task", "title"]);
    assert_eq!(upd["inputSchema"]["required"], json!(["id"]));
    assert_eq!(upd["inputSchema"]["properties"]["model"]["type"], json!(["string", "null"]));
    assert_eq!(keys(&by("list_tasks")["inputSchema"]["properties"]), ["status"]);
    assert_eq!(by("list_tasks")["inputSchema"]["properties"]["status"]["items"]["enum"], json!(["queued", "running", "done", "failed"]));
    assert_eq!(by("delete_task")["inputSchema"]["required"], json!(["id"]));
    assert_eq!(by("get_task_details")["inputSchema"]["required"], json!(["id"]));

    let save = by("save_skill");
    assert_eq!(keys(&save["inputSchema"]["properties"]), ["body", "description", "files", "name"]);
    assert_eq!(save["inputSchema"]["required"], json!(["name", "description", "body"]));
    assert_eq!(save["inputSchema"]["properties"]["name"]["pattern"], "^[a-z0-9][a-z0-9-]{0,39}$");
    assert_eq!(save["inputSchema"]["properties"]["files"]["maxItems"], 20);
    assert_eq!(save["inputSchema"]["properties"]["files"]["items"]["required"], json!(["path", "content"]));
    assert_eq!(save["inputSchema"]["additionalProperties"], false);
    assert_eq!(keys(&by("list_skills")["inputSchema"]["properties"]), Vec::<String>::new());
    assert_eq!(by("delete_skill")["inputSchema"]["required"], json!(["name"]));

    assert_eq!(keys(&by("get_workspace_settings")["inputSchema"]["properties"]), Vec::<String>::new());
    assert_eq!(by("get_workspace_settings")["inputSchema"]["required"], json!([]));
    let settings = by("propose_settings");
    let schema = &settings["inputSchema"];
    let props = &schema["properties"];
    assert_eq!(
        keys(props),
        [
            "default_launch", "delivery", "launches", "reason", "repos", "use_schedule", "worker_allow", "worker_deny",
            "worker_model", "worker_permission", "worker_rules"
        ]
    );
    assert_eq!(schema["required"], json!(["reason"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(props["reason"]["maxLength"], REASON_MAX);
    assert_eq!(props["worker_rules"]["maxLength"], MAX_RULES_LEN);
    assert_eq!(props["worker_permission"]["enum"], json!(["plan", "default", "accept-edits", "bypass"]));
    assert_eq!(props["worker_permission"]["enum"], json!(PermissionMode::ALL.map(PermissionMode::as_str)));
    for list in ["worker_allow", "worker_deny"] {
        assert_eq!(props[list]["maxItems"], MAX_RULES, "{list}");
        assert_eq!(props[list]["items"]["maxLength"], MAX_RULE_CHARS, "{list}");
    }
    let delivery = &props["delivery"];
    assert_eq!(keys(&delivery["properties"]), ["merge_method", "mode", "parallel"]);
    assert_eq!(delivery["additionalProperties"], false);
    assert_eq!(delivery["properties"]["mode"]["enum"], json!(DeliveryMode::ALL.map(DeliveryMode::as_str)));
    assert_eq!(delivery["properties"]["merge_method"]["enum"], json!(MergeMethod::ALL.map(MergeMethod::as_str)));
    assert_eq!(props["launches"]["maxItems"], MAX_PATCH_LAUNCHES);
    let launch = &props["launches"]["items"];
    assert_eq!(keys(&launch["properties"]), ["command", "id", "name"]);
    assert_eq!(launch["required"], json!(["name", "command"]));
    assert_eq!(launch["properties"]["name"]["maxLength"], MAX_LAUNCH_NAME_LEN);
    assert_eq!(launch["properties"]["command"]["maxLength"], MAX_COMMAND_LEN);
    assert_eq!(props["repos"]["maxProperties"], MAX_PATCH_REPOS);
    let repo = &props["repos"]["additionalProperties"];
    assert_eq!(repo["type"], json!(["object", "null"]));
    assert_eq!(repo["additionalProperties"], false);
    assert_eq!(
        keys(&repo["properties"]),
        ["base_branch", "cleanup", "container_root", "copy_ignored", "remove_force", "setup"]
    );
    for list in ["copy_ignored", "setup", "cleanup"] {
        assert_eq!(repo["properties"][list]["maxItems"], MAX_REPO_COMMANDS, "{list}");
    }
    for list in ["setup", "cleanup"] {
        assert_eq!(repo["properties"][list]["items"]["maxLength"], MAX_REPO_COMMAND_LEN, "{list}");
    }
    let description = settings["description"].as_str().unwrap();
    assert!(description.contains("NO feedback") && description.contains("never wait") && description.contains("sensitive"));

    let done = &worker_tools()[1];
    assert_eq!(
        keys(&done["inputSchema"]["properties"]),
        ["commit", "files_changed", "follow_ups", "pr_url", "status", "summary", "tests"]
    );
    assert_eq!(done["inputSchema"]["required"], json!(["status", "summary"]));
    assert_eq!(done["inputSchema"]["properties"]["status"]["enum"], json!(["completed", "failed"]));
    assert_eq!(
        done["inputSchema"]["properties"]["tests"]["items"]["properties"]["result"]["enum"],
        json!(["passed", "failed", "not_run"])
    );
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
    let r = send(&mut s, &none, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_task","arguments":{}}})).unwrap();
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("not available") && text.contains("app non in ascolto"));
}

#[test]
fn tools_call_success_returns_pretty_json_text() {
    let mut b = Fake::new("worker");
    b.reply = Ok(json!({ "closing": true }));
    let mut s = Server::default();
    let args = json!({"status":"completed","summary":"ok"});
    let r = send(
        &mut s,
        &b,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"complete_task","arguments":args}}),
    )
    .unwrap();
    assert_eq!(r["id"], 7);
    assert!(r["result"].get("isError").is_none());
    assert_eq!(r["result"]["content"][0]["type"], "text");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), json!({"closing": true}));
    assert!(text.contains('\n'), "JSON indentato");
    assert_eq!(b.calls.borrow().last().unwrap(), &("complete_task".to_string(), args));
}

#[test]
fn tools_call_rpc_error_and_bad_calls_are_is_error() {
    let mut b = Fake::new("session");
    b.reply = Err("task not found: x".to_string());
    let mut s = Server::default();
    let call = |s: &mut Server, params: Value| send(s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params})).unwrap();
    let r = call(&mut s, json!({"name":"get_task_details","arguments":{"id":"x"}}));
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(r["result"]["content"][0]["text"], "task not found: x");
    // Tool del ruolo sbagliato / inesistente / argomenti non oggetto / senza nome.
    for p in [
        json!({"name":"get_task"}),
        json!({"name":"complete_task"}),
        json!({"name":"boh"}),
        json!({"name":"list_tasks","arguments":[1]}),
        json!({"arguments":{}}),
    ] {
        let r = call(&mut s, p);
        assert_eq!(r["result"]["isError"], true);
    }
    // Nessuna di queste ha raggiunto l'app, a parte la prima.
    assert_eq!(b.calls.borrow().iter().filter(|(m, _)| m != "whoami").count(), 1);
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
