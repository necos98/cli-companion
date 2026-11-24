use super::tools::{
    model_description, planner_tools, proposed_model_description, reviewer_tools, session_tools, settings_properties,
    tools_for, worker_tools,
};
use super::*;
use crate::agents::{PermissionMode, TierModels};
use crate::automation::settings::REASON_MAX;
use crate::automation::{Method, Role};
use crate::workspaces::{
    DeliveryMode, MergeMethod, MAX_COMMAND_LEN, MAX_E2E_TIMEOUT_MIN, MAX_LAUNCH_NAME_LEN, MAX_PATCH_AGENTS,
    MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS, MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_RULES_LEN,
};
use std::collections::HashSet;
use std::net::TcpListener;
use std::time::Instant;

/// Tool definitions and server instructions are sent to the model in every
/// session: the ceilings are intentional. Do not raise one to fit new text,
/// shorten the text.
/// 5_600 = the previous ceiling plus the three skill tools (mostly their validation limits).
/// 8_700 = 5_600 plus the two settings tools (propose_settings is a schema of eleven fields with
/// their limits and enums, measured 8_604 after trimming every description that was not needed).
/// 9_000 = 8_700 plus the `kind` field of the tasks in propose_tasks and update_task (measured 8_998).
/// 9_500 = 9_000 plus the `e2e` flag of the tasks and the `e2e` object of the repo settings (measured 9_477).
/// 9_700 = 9_500 plus `agent` and the nested `worker_agents` object of propose_settings, which replace the
/// top-level worker_model/worker_allow/worker_deny (measured 9_699 after trimming every description that was not needed).
/// 9_800 = 9_700 plus the longer generic model pattern (`MODEL_SHAPE_PATTERN`, twice; measured 9_748).
/// 10_100 = 9_800 plus the `bot` field of the tasks (propose_tasks, update_task), the model/agent overrides and the Bots
/// in get_workspace_settings (measured 10_038).
const SESSION_TOOLS_CEILING: usize = 10_100;
/// 3_050 = 2_600 plus run_e2e and get_e2e_result (measured 3_002).
const WORKER_TOOLS_CEILING: usize = 3_050;
/// The reviewer set (measured 4_005, with the `e2e` flag of propose_tasks): its own get_task and complete_task (verdict, findings, tests) plus the shared
/// get_task_details and propose_tasks definitions.
/// 5_200 = 4_050 plus run_e2e and get_e2e_result (measured 5_163).
/// 5_300 = 5_200 plus the `bot` field and the model/agent override descriptions of propose_tasks (measured 5_244).
/// The read-only get_workspace_settings joined the set under the same ceiling: other descriptions were shortened.
const REVIEWER_TOOLS_CEILING: usize = 5_300;
/// The worker set with the Agents HUB on: the hub-off set plus ask_user (measured 3_698).
const WORKER_HUB_TOOLS_CEILING: usize = 3_750;
const SESSION_INSTRUCTIONS_CEILING: usize = 700;
/// The worker server instructions with the Agents HUB on (measured 238).
const WORKER_HUB_INSTRUCTIONS_CEILING: usize = 300;
/// The reviewer server instructions (measured 345).
const REVIEWER_INSTRUCTIONS_CEILING: usize = 400;

/// Backend finto: registra le chiamate (con l'attesa concessa) e risponde con `reply`.
struct Fake {
    role: &'static str,
    /// L'Agents HUB che `whoami` dichiara.
    hub: bool,
    reply: Result<Value, String>,
    calls: Mutex<Vec<(String, Value)>>,
    timeouts: Mutex<Vec<(String, Duration)>>,
}

impl Fake {
    fn new(role: &'static str) -> Self {
        Self { role, hub: false, reply: Ok(json!({ "ok": 1 })), calls: Mutex::default(), timeouts: Mutex::default() }
    }

    /// Come `new`, ma `whoami` dichiara l'Agents HUB acceso.
    fn with_hub(role: &'static str) -> Self {
        Self { hub: true, ..Self::new(role) }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }

    fn timeouts(&self) -> Vec<(String, Duration)> {
        self.timeouts.lock().unwrap().clone()
    }
}

impl Backend for Fake {
    fn rpc(&self, method: &str, params: &Value, timeout: Duration) -> Result<Value, String> {
        self.calls.lock().unwrap().push((method.to_string(), params.clone()));
        self.timeouts.lock().unwrap().push((method.to_string(), timeout));
        if method == "whoami" {
            let mut v = json!({ "role": self.role, "session": "t1" });
            if self.hub {
                v["hub"] = json!(true);
            }
            if self.role == "session" {
                v["workspace"] = json!({"name":"Proj","kind":"wsl","path":"/home/u/app"});
            }
            return Ok(v);
        }
        self.reply.clone()
    }
}

fn send(server: &Server, b: &dyn Backend, msg: Value) -> Option<Value> {
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
    let reviewer = serde_json::to_string(&reviewer_tools()).unwrap().chars().count();
    let reviewer_instr = instructions(Some(Role::Reviewer), None, false).chars().count();
    let instr = instructions(Some(Role::Session), Some("Proj (wsl, /home/u/app)"), false).chars().count();
    assert!(session <= SESSION_TOOLS_CEILING, "session tools are {session} chars, ceiling {SESSION_TOOLS_CEILING}");
    assert!(worker <= WORKER_TOOLS_CEILING, "worker tools are {worker} chars, ceiling {WORKER_TOOLS_CEILING}");
    assert!(
        instr <= SESSION_INSTRUCTIONS_CEILING,
        "session instructions are {instr} chars, ceiling {SESSION_INSTRUCTIONS_CEILING}"
    );
    let worker_hub = serde_json::to_string(&tools_for(Role::Worker, true)).unwrap().chars().count();
    let worker_hub_instr = instructions(Some(Role::Worker), None, true).chars().count();
    assert!(
        worker_hub <= WORKER_HUB_TOOLS_CEILING,
        "worker tools with the hub are {worker_hub} chars, ceiling {WORKER_HUB_TOOLS_CEILING}"
    );
    assert!(
        worker_hub_instr <= WORKER_HUB_INSTRUCTIONS_CEILING,
        "worker instructions with the hub are {worker_hub_instr} chars, ceiling {WORKER_HUB_INSTRUCTIONS_CEILING}"
    );
    assert!(
        reviewer <= REVIEWER_TOOLS_CEILING,
        "reviewer tools are {reviewer} chars, ceiling {REVIEWER_TOOLS_CEILING}"
    );
    assert!(
        reviewer_instr <= REVIEWER_INSTRUCTIONS_CEILING,
        "reviewer instructions are {reviewer_instr} chars, ceiling {REVIEWER_INSTRUCTIONS_CEILING}"
    );
}

#[test]
fn model_descriptions_name_the_models_of_the_worker_agent() {
    let models = Some(TierModels { standard: "std-model", strong: "big-model" });
    let optional = model_description(models);
    assert!(optional.contains("std-model for well-defined work") && optional.contains("big-model for design or risk"));
    assert!(optional.ends_with("Omit for the workspace default."));
    let required = proposed_model_description(models);
    assert!(required.contains("std-model for well-defined work") && required.contains("big-model for design, risky"));
    assert!(required.starts_with("Optional Bot model override"), "{required}");
}

#[test]
fn model_descriptions_name_no_model_without_a_catalog() {
    assert!(model_description(None).starts_with("Model of the task's agent, as that agent names it"));
    assert!(proposed_model_description(None).starts_with("Optional Bot model override, as its agent names it"));
}

#[test]
fn model_descriptions_have_no_runs_of_spaces() {
    let models = Some(TierModels { standard: "std-model", strong: "big-model" });
    for text in [
        model_description(models),
        model_description(None),
        proposed_model_description(models),
        proposed_model_description(None),
    ] {
        assert!(!text.contains("  "), "{text}");
    }
}

#[test]
fn the_model_fields_of_the_tools_carry_the_worker_agent_names() {
    let expected = crate::agents::tier_models(crate::agents::registry::default_agent());
    let model_of = |tools: Vec<Value>, name: &str| {
        let tool = tools.into_iter().find(|t| t["name"] == name).unwrap();
        let schema = &tool["inputSchema"]["properties"];
        let props = if name == "propose_tasks" { &schema["tasks"]["items"]["properties"] } else { schema };
        props["model"]["description"].as_str().unwrap().to_string()
    };
    assert_eq!(model_of(session_tools(), "propose_tasks"), proposed_model_description(expected));
    assert_eq!(model_of(session_tools(), "update_task"), model_description(expected));
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
            &Server::default(),
            &b,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":asked}}),
        )
        .unwrap();
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], expected);
        assert_eq!(r["result"]["serverInfo"]["name"], "cli-companion");
        assert_eq!(r["result"]["capabilities"]["tools"], json!({ "listChanged": true }));
        assert!(!r["result"]["instructions"].as_str().unwrap().is_empty());
    }
    // La sessione vede il proprio workspace; il testo dice che i task si propongono e non si attende.
    let r = send(&Server::default(), &b, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(text.contains("Proj (wsl, /home/u/app)"));
    assert!(text.contains("propose_tasks") && text.contains("accepts or discards") && text.contains("Never wait"));
    assert!(text.contains("save_skill") && text.contains("global skills"));
    assert!(text.contains("propose_settings") && text.contains("approves them in the app"));
    let w = Fake::new("worker");
    let r = send(&Server::default(), &w, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(!text.contains("Proj") && text.contains("complete_task"));
    let rv = Fake::new("reviewer");
    let r = send(&Server::default(), &rv, json!({"jsonrpc":"2.0","id":1,"method":"initialize"})).unwrap();
    let text = r["result"]["instructions"].as_str().unwrap();
    assert!(text.contains("automated reviewer") && text.contains("complete_task") && text.contains("verdict"));
    assert!(!text.contains('\n') && !text.contains("  "), "reviewer instructions must be one line with single spaces");
    assert!(!text.contains("Proj") && !text.contains("deliver"));
    // Senza protocolVersion: default.
    let r = send(&Server::default(), &b, json!({"jsonrpc":"2.0","id":"a","method":"initialize"})).unwrap();
    assert_eq!(r["id"], "a");
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
}

#[test]
fn tools_list_depends_on_role() {
    let s = Server::default();
    let r = send(&s, &Fake::new("session"), json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
    assert_eq!(
        names(&r),
        [
            "propose_tasks",
            "list_tasks",
            "get_task_details",
            "update_task",
            "delete_task",
            "save_skill",
            "list_skills",
            "delete_skill",
            "get_workspace_settings",
            "propose_settings"
        ]
    );
    let s = Server::default();
    let r = send(&s, &Fake::new("worker"), json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    assert_eq!(names(&r), ["get_task", "complete_task", "run_e2e", "get_e2e_result"]);
    let s = Server::default();
    let r = send(&s, &Fake::new("reviewer"), json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})).unwrap();
    assert_eq!(
        names(&r),
        [
            "get_task",
            "complete_task",
            "get_task_details",
            "propose_tasks",
            "get_workspace_settings",
            "run_e2e",
            "get_e2e_result"
        ]
    );
    for t in r["result"]["tools"].as_array().unwrap() {
        assert!(!t["description"].as_str().unwrap().is_empty());
        assert_eq!(t["inputSchema"]["type"], "object");
    }
    let s = Server::default();
    let r = send(&s, &Fake::new("planner"), json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})).unwrap();
    assert_eq!(
        names(&r),
        ["get_task", "complete_task", "propose_tasks", "list_tasks", "get_task_details", "get_workspace_settings"]
    );
    let r = send(&s, &Fake::new("planner"), json!({"jsonrpc":"2.0","id":5,"method":"initialize"})).unwrap();
    assert!(r["result"]["instructions"].as_str().unwrap().contains("automated planner"));
}

#[test]
fn no_orchestrator_tool_survives() {
    let all: Vec<Value> =
        session_tools().into_iter().chain(worker_tools()).chain(reviewer_tools()).chain(planner_tools()).collect();
    for gone in [
        "spawn_session",
        "list_sessions",
        "send_message",
        "close_session",
        "get_events",
        "submit_plan",
        "get_board",
        "set_task_status",
        "set_board",
        "get_workflow",
        "save_workflow",
        "ask_user",
        "report_fact",
        "request_help",
        "list_agents",
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
    assert_eq!(
        keys(&item["properties"]),
        ["agent", "bot", "depends_on", "e2e", "id", "kind", "model", "repo", "task", "title"]
    );
    assert_eq!(item["properties"]["kind"]["enum"], json!(["work", "review"]));
    assert_eq!(item["required"], json!(["id", "title", "task", "bot"]));
    assert_eq!(item["properties"]["bot"]["type"], "string");
    assert_eq!(item["properties"]["model"]["type"], json!(["string", "null"]));
    assert_eq!(item["additionalProperties"], false);
    assert_eq!(propose["inputSchema"]["required"], json!(["tasks"]));
    assert_eq!(propose["inputSchema"]["properties"]["tasks"]["maxItems"], 20);
    assert!(propose["description"].as_str().unwrap().contains("never wait"));
    let upd = by("update_task");
    assert_eq!(
        keys(&upd["inputSchema"]["properties"]),
        ["agent", "bot", "depends_on", "e2e", "id", "kind", "model", "repo", "task", "title"]
    );
    assert_eq!(upd["inputSchema"]["properties"]["bot"]["type"], json!(["string", "null"]));
    assert_eq!(upd["inputSchema"]["required"], json!(["id"]));
    assert_eq!(upd["inputSchema"]["properties"]["model"]["type"], json!(["string", "null"]));
    assert_eq!(keys(&by("list_tasks")["inputSchema"]["properties"]), ["status"]);
    assert_eq!(
        by("list_tasks")["inputSchema"]["properties"]["status"]["items"]["enum"],
        json!(["queued", "running", "done", "failed"])
    );
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
            "agent",
            "default_launch",
            "delivery",
            "launches",
            "reason",
            "repos",
            "use_schedule",
            "worker_agents",
            "worker_permission",
            "worker_rules"
        ]
    );
    assert_eq!(schema["required"], json!(["reason"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(props["reason"]["maxLength"], REASON_MAX);
    assert_eq!(props["worker_rules"]["maxLength"], MAX_RULES_LEN);
    assert_eq!(props["worker_permission"]["enum"], json!(["plan", "default", "accept-edits", "bypass"]));
    assert_eq!(props["worker_permission"]["enum"], json!(PermissionMode::ALL.map(PermissionMode::as_str)));
    assert_eq!(props["agent"]["type"], json!(["string", "null"]));
    let agents = &props["worker_agents"];
    assert_eq!(agents["maxProperties"], MAX_PATCH_AGENTS);
    let per_agent = &agents["additionalProperties"];
    assert_eq!(keys(&per_agent["properties"]), ["allow", "deny", "model"]);
    assert_eq!(per_agent["additionalProperties"], false);
    assert_eq!(per_agent["properties"]["model"]["type"], json!(["string", "null"]));
    assert_eq!(per_agent["properties"]["allow"]["type"], "array");
    assert!(agents["description"].as_str().unwrap().contains("get_workspace_settings.agents"));
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
        ["base_branch", "cleanup", "container_root", "copy_ignored", "e2e", "remove_force", "setup"]
    );
    for list in ["copy_ignored", "setup", "cleanup"] {
        assert_eq!(repo["properties"][list]["maxItems"], MAX_REPO_COMMANDS, "{list}");
    }
    for list in ["setup", "cleanup"] {
        assert_eq!(repo["properties"][list]["items"]["maxLength"], MAX_REPO_COMMAND_LEN, "{list}");
    }
    let e2e = &repo["properties"]["e2e"];
    assert_eq!(e2e["type"], json!(["object", "null"]));
    assert_eq!(e2e["required"], json!(["command", "junit"]));
    assert_eq!(keys(&e2e["properties"]), ["command", "junit", "timeout_min"]);
    assert_eq!(e2e["properties"]["timeout_min"]["maximum"], MAX_E2E_TIMEOUT_MIN);
    let description = settings["description"].as_str().unwrap();
    assert!(
        description.contains("NO feedback") && description.contains("never wait") && description.contains("sensitive")
    );

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
    let s = Server::default();
    for id in 1..=3 {
        send(&s, &b, json!({"jsonrpc":"2.0","id":id,"method":"tools/list"}));
    }
    assert_eq!(b.calls().iter().filter(|(m, _)| m == "whoami").count(), 1);

    let none = UnavailableBackend("app non in ascolto".to_string());
    let s = Server::default();
    let r = send(&s, &none, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
    assert_eq!(r["result"]["tools"], json!([]));
    // initialize funziona comunque, e senza ambiente non chiede tentativi.
    let r = send(&s, &none, json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{}})).unwrap();
    assert!(r["result"]["capabilities"]["tools"].is_object());
    assert!(!s.claim_retry());
    // tools/call: errore leggibile.
    let r = send(
        &s,
        &none,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_task","arguments":{}}}),
    )
    .unwrap();
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("not available") && text.contains("app non in ascolto"));
}

#[test]
fn the_role_waits_briefly_and_tool_calls_keep_the_long_timeout() {
    assert_eq!(RPC_TIMEOUT, Duration::from_secs(30), "the E2E tools count on 30 s per call");
    let b = Fake::new("worker");
    let s = Server::default();
    send(&s, &b, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}));
    let args = json!({"run_id":"r1","wait_seconds":25});
    send(
        &s,
        &b,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_e2e_result","arguments":args}}),
    );
    assert_eq!(b.timeouts(), [("whoami".to_string(), ROLE_TIMEOUT), ("get_e2e_result".to_string(), RPC_TIMEOUT)]);
    // tools/list e la risoluzione dentro tools/call usano l'attesa breve.
    let s = Server::default();
    let b = Fake::new("session");
    send(&s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    let s = Server::default();
    send(&s, &b, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_tasks"}}));
    assert_eq!(
        b.timeouts(),
        [
            ("whoami".to_string(), ROLE_TIMEOUT),
            ("whoami".to_string(), ROLE_TIMEOUT),
            ("list_tasks".to_string(), RPC_TIMEOUT)
        ]
    );
}

#[test]
fn a_failed_whoami_in_initialize_asks_for_one_series_of_retries() {
    let mut b = Fake::new("worker");
    b.role = "nobody";
    let s = Server::default();
    // Nessun tentativo prima dell'handshake, anche se tools/list fallisce.
    send(&s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert!(!s.claim_retry());
    for id in 2..=3 {
        let r = send(&s, &b, json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{}})).unwrap();
        assert_eq!(r["result"]["instructions"], instructions(None, None, false));
    }
    assert!(s.claim_retry());
    assert!(!s.claim_retry(), "a single series per process");
    // Ruolo risolto all'handshake: nessun tentativo.
    let s = Server::default();
    send(&s, &Fake::new("worker"), json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}));
    assert!(!s.claim_retry());
}

#[test]
fn initialize_answers_quickly_when_the_app_accepts_but_never_answers() {
    // Il sistema accetta la connessione (backlog) e nessuno risponde mai.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let backend = TcpBackend { session: "t1".to_string(), token: "tok".to_string(), port };
    let s = Server::default();
    let started = Instant::now();
    let r = send(&s, &backend, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})).unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "initialize took {elapsed:?}");
    assert_eq!(r["result"]["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(r["result"]["instructions"], instructions(None, None, false));
    assert!(s.claim_retry(), "a real backend that does not answer is retried in the background");
    drop(listener);
}

#[test]
fn tools_call_success_returns_pretty_json_text() {
    let mut b = Fake::new("worker");
    b.reply = Ok(json!({ "closing": true }));
    let s = Server::default();
    let args = json!({"status":"completed","summary":"ok"});
    let r = send(
        &s,
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
    assert_eq!(b.calls().last().unwrap(), &("complete_task".to_string(), args));
}

#[test]
fn tools_call_rpc_error_and_bad_calls_are_is_error() {
    let mut b = Fake::new("session");
    b.reply = Err("task not found: x".to_string());
    let s = Server::default();
    let call = |s: &Server, params: Value| {
        send(s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":params})).unwrap()
    };
    let r = call(&s, json!({"name":"get_task_details","arguments":{"id":"x"}}));
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
        let r = call(&s, p);
        assert_eq!(r["result"]["isError"], true);
    }
    // Nessuna di queste ha raggiunto l'app, a parte la prima.
    assert_eq!(b.calls().iter().filter(|(m, _)| m != "whoami").count(), 1);
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
    let s = Server::default();
    let r = send(&s, &b, json!({"jsonrpc":"2.0","id":9,"method":"ping"})).unwrap();
    assert_eq!(r, json!({"jsonrpc":"2.0","id":9,"result":{}}));

    let r = send(&s, &b, json!({"jsonrpc":"2.0","id":10,"method":"resources/list"})).unwrap();
    assert_eq!(r["error"]["code"], -32601);
    assert_eq!(r["id"], 10);

    // Notifiche (note o ignote) e risposte del client: nessun output.
    for m in [
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}),
        json!({"jsonrpc":"2.0","method":"qualcosa/di/ignoto"}),
        json!({"jsonrpc":"2.0","id":3,"result":{}}),
    ] {
        assert!(send(&s, &b, m).is_none());
    }

    let r = handle_line(&s, "{non json", &b).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert!(r["id"].is_null());
    assert_eq!(handle_line(&s, "[1]", &b).unwrap()["error"]["code"], -32600);
    // Nulla di tutto cio' ha toccato l'app.
    assert!(b.calls().is_empty());
}

#[test]
fn reviewer_complete_task_schema_has_verdict_and_findings() {
    let tools = reviewer_tools();
    let done = &tools[1];
    assert_eq!(done["name"], "complete_task");
    let props = &done["inputSchema"]["properties"];
    assert_eq!(keys(props), ["findings", "status", "summary", "tests", "verdict"]);
    assert_eq!(done["inputSchema"]["required"], json!(["status", "summary"]));
    assert_eq!(props["status"]["enum"], json!(["completed", "failed"]));
    assert_eq!(props["verdict"]["enum"], json!(["passed", "issues"]));
    assert_eq!(props["findings"]["maxItems"], 100);
    assert_eq!(props["findings"]["items"]["maxLength"], 2000);
    // I `tests` sono gli stessi del worker.
    assert_eq!(props["tests"], worker_tools()[1]["inputSchema"]["properties"]["tests"]);
    // propose_tasks e get_task_details sono le definizioni della sessione.
    let session = session_tools();
    for name in ["propose_tasks", "get_task_details"] {
        let of = |tools: &[Value]| tools.iter().find(|t| t["name"] == name).unwrap().clone();
        assert_eq!(of(&tools), of(&session), "{name}");
    }
    // Il worker non ha ne' verdetto ne' findings.
    let worker = &worker_tools()[1]["inputSchema"]["properties"];
    assert!(worker.get("verdict").is_none() && worker.get("findings").is_none());
}

#[test]
fn e2e_tools_belong_to_worker_and_reviewer_only() {
    let has = |tools: &[Value], name: &str| tools.iter().any(|t| t["name"] == name);
    for tools in [worker_tools(), reviewer_tools()] {
        assert!(has(&tools, "run_e2e") && has(&tools, "get_e2e_result"));
    }
    let session = session_tools();
    assert!(!has(&session, "run_e2e") && !has(&session, "get_e2e_result"));

    let get = worker_tools().into_iter().find(|t| t["name"] == "get_e2e_result").unwrap();
    assert_eq!(get["inputSchema"]["required"], json!([]));
    assert_eq!(get["inputSchema"]["additionalProperties"], false);
    let props = &get["inputSchema"]["properties"];
    assert_eq!(keys(props), ["run_id", "wait_seconds"]);
    assert_eq!(props["run_id"]["maxLength"], 64);
    assert_eq!(props["wait_seconds"]["type"], "integer");
    assert_eq!(props["wait_seconds"]["minimum"], 0);
    assert_eq!(props["wait_seconds"]["maximum"], 25);
    let run = worker_tools().into_iter().find(|t| t["name"] == "run_e2e").unwrap();
    assert_eq!(run["inputSchema"]["properties"], json!({}));
    // Stessa definizione per worker e revisore.
    for name in ["run_e2e", "get_e2e_result"] {
        let of = |tools: Vec<Value>| tools.into_iter().find(|t| t["name"] == name).unwrap();
        assert_eq!(of(worker_tools()), of(reviewer_tools()), "{name}");
    }
}

#[test]
fn e2e_calls_are_gated_by_role_and_forwarded_for_workers() {
    let call = |s: &Server, b: &Fake, name: &str, args: Value| {
        send(s, b, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}))
            .unwrap()
    };
    let session = Fake::new("session");
    let s = Server::default();
    let r = call(&s, &session, "run_e2e", json!({}));
    assert_eq!(r["result"]["isError"], true);
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown tool, or not available for this session"));
    assert!(session.calls().iter().all(|(m, _)| m == "whoami"));

    for role in ["worker", "reviewer"] {
        let b = Fake::new(role);
        let s = Server::default();
        let r = call(&s, &b, "run_e2e", json!({}));
        assert!(r["result"].get("isError").is_none(), "{role}");
        assert_eq!(b.calls().last().unwrap(), &("run_e2e".to_string(), json!({})));
        let args = json!({"run_id":"r1","wait_seconds":10});
        call(&s, &b, "get_e2e_result", args.clone());
        assert_eq!(b.calls().last().unwrap(), &("get_e2e_result".to_string(), args));
    }
}

#[test]
fn a_reviewer_cannot_call_the_queue_or_settings_changing_tools() {
    let b = Fake::new("reviewer");
    let s = Server::default();
    for name in ["list_tasks", "update_task", "delete_task", "save_skill", "propose_settings"] {
        let r =
            send(&s, &b, json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":{}}}))
                .unwrap();
        assert_eq!(r["result"]["isError"], true, "{name}");
    }
    assert!(b.calls().iter().all(|(m, _)| m == "whoami"));
    // Quelli concessi arrivano all'app.
    let r = send(&s, &b, json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_task_details","arguments":{"id":"a"}}}))
        .unwrap();
    assert!(r["result"].get("isError").is_none());
    assert_eq!(b.calls().last().unwrap().0, "get_task_details");
    let r = send(
        &s,
        &b,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_workspace_settings","arguments":{}}}),
    )
    .unwrap();
    assert!(r["result"].get("isError").is_none());
    assert_eq!(b.calls().last().unwrap().0, "get_workspace_settings");
}

#[test]
fn propose_settings_has_no_agent_specific_fields_at_the_top_level() {
    let props = settings_properties();
    for legacy in ["worker_model", "worker_allow", "worker_deny"] {
        assert!(props.get(legacy).is_none(), "{legacy}");
    }
    assert!(props.get("worker_permission").is_some() && props.get("agent").is_some());
}

#[test]
fn get_workspace_settings_points_to_the_agents() {
    let tools = tools_for(Role::Session, false);
    let read = tools.iter().find(|t| t["name"] == "get_workspace_settings").unwrap();
    assert!(read["description"].as_str().unwrap().contains("enabled agents"));
}

/// Nomi dei tool di una lista, nell'ordine in cui sono definiti.
fn tool_names(tools: &[Value]) -> Vec<String> {
    tools.iter().map(|t| t["name"].as_str().expect("every tool has a name").to_string()).collect()
}

#[test]
fn every_mcp_tool_is_a_method_allowed_for_its_role() {
    for hub in [true, false] {
        for role in Role::ALL {
            let listed = tool_names(&tools_for(role, hub));
            let unique: HashSet<&String> = listed.iter().collect();
            assert_eq!(unique.len(), listed.len(), "{}: duplicate tool names {listed:?}", role.as_str());
            for name in &listed {
                assert!(Method::parse(name).is_some(), "{}: tool {name} is not an RPC method", role.as_str());
            }
            let expected: HashSet<&str> = Method::ALL
                .into_iter()
                .filter(|m| m.allows(role) && !m.is_handshake() && (hub || !m.requires_hub()))
                .map(|m| m.name())
                .collect();
            let actual: HashSet<&str> = listed.iter().map(String::as_str).collect();
            assert_eq!(actual, expected, "{} (hub {hub}): the tools differ from the role table", role.as_str());
        }
    }
}

#[test]
fn the_handshake_has_no_tool_for_any_role() {
    for role in Role::ALL {
        let listed = tool_names(&tools_for(role, true));
        for m in Method::ALL.into_iter().filter(|m| m.is_handshake()) {
            assert!(!listed.iter().any(|n| n == m.name()), "{}: {}", role.as_str(), m.name());
        }
    }
}

fn list_for(b: &Fake) -> Vec<String> {
    names(&send(&Server::default(), b, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap())
}

#[test]
fn ask_user_is_listed_only_with_the_hub_and_only_for_worker_and_planner() {
    assert_eq!(list_for(&Fake::new("worker")), ["get_task", "complete_task", "run_e2e", "get_e2e_result"]);
    assert_eq!(
        list_for(&Fake::with_hub("worker")),
        ["get_task", "complete_task", "ask_user", "run_e2e", "get_e2e_result"]
    );
    assert!(!list_for(&Fake::new("planner")).contains(&"ask_user".to_string()));
    assert_eq!(
        list_for(&Fake::with_hub("planner")),
        [
            "get_task",
            "complete_task",
            "ask_user",
            "propose_tasks",
            "list_tasks",
            "get_task_details",
            "get_workspace_settings"
        ]
    );
    for role in ["reviewer", "session"] {
        assert!(!list_for(&Fake::with_hub(role)).contains(&"ask_user".to_string()), "{role}");
    }
}

#[test]
fn whoami_without_the_hub_field_means_the_hub_is_off() {
    let server = Server::default();
    server.role(&Fake::new("worker")).unwrap();
    assert!(!server.hub());
    let server = Server::default();
    server.role(&Fake::with_hub("worker")).unwrap();
    assert!(server.hub());
}

#[test]
fn ask_user_is_unknown_without_the_hub_and_forwarded_with_it() {
    let args = json!({"question":"Which one?","options":["a","b"]});
    let msg = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ask_user","arguments":args}});
    let off = Fake::new("worker");
    let r = send(&Server::default(), &off, msg.clone()).unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(r["result"]["content"][0]["text"], "unknown tool, or not available for this session: ask_user");
    assert!(off.calls().iter().all(|(m, _)| m == "whoami"));
    let on = Fake::with_hub("worker");
    let r = send(&Server::default(), &on, msg).unwrap();
    assert!(r["result"].get("isError").is_none());
    assert!(on.calls().contains(&("ask_user".to_string(), args)));
}

#[test]
fn the_ask_user_schema_limits_are_the_board_ones() {
    use crate::board::{OPTIONS_MAX, OPTION_MAX, QUESTION_MAX};
    let tools = tools_for(Role::Worker, true);
    let t = tools.iter().find(|t| t["name"] == "ask_user").unwrap();
    let props = &t["inputSchema"]["properties"];
    assert_eq!(keys(props), ["options", "question"]);
    assert_eq!(props["question"]["minLength"], 1);
    assert_eq!(props["question"]["maxLength"], QUESTION_MAX);
    assert_eq!(props["options"]["maxItems"], OPTIONS_MAX);
    assert_eq!(props["options"]["items"]["minLength"], 1);
    assert_eq!(props["options"]["items"]["maxLength"], OPTION_MAX);
    assert_eq!(t["inputSchema"]["required"], json!(["question"]));
}

#[test]
fn the_instructions_of_worker_and_planner_follow_the_hub() {
    let worker = instructions(Some(Role::Worker), None, false);
    let worker_hub = instructions(Some(Role::Worker), None, true);
    assert!(worker.ends_with("ask in plain text and end your turn: the user answers in your terminal."));
    assert!(worker_hub.ends_with("call ask_user and end your turn: the answer arrives in your terminal."));
    assert_eq!(worker.split(" If ").next(), worker_hub.split(" If ").next());
    let planner = instructions(Some(Role::Planner), None, false);
    let planner_hub = instructions(Some(Role::Planner), None, true);
    assert!(planner.contains("list your questions for the user in the summary"));
    assert!(planner_hub.contains("call ask_user") && !planner_hub.contains("Nobody reads"));
    for role in [Role::Reviewer, Role::Session] {
        assert_eq!(instructions(Some(role), Some("W"), false), instructions(Some(role), Some("W"), true));
    }
    let r = send(&Server::default(), &Fake::with_hub("worker"), json!({"jsonrpc":"2.0","id":1,"method":"initialize"}));
    assert_eq!(r.unwrap()["result"]["instructions"], worker_hub);
}

#[test]
fn an_unknown_role_in_the_whoami_answer_is_an_error() {
    let server = Server::default();
    let b = Fake::new("admin");
    assert_eq!(server.role(&b), Err("unknown role in the whoami response".to_string()));
    for role in Role::ALL {
        assert_eq!(Server::default().role(&Fake::new(role.as_str())), Ok(role));
    }
}
