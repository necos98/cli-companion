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
    /// Modello di default dei worker nel workspace (da `whoami`), se impostato.
    worker_model: Option<String>,
    /// Regole di permesso pre-approvate/vietate dei worker (da `whoami`), a parole.
    worker_rules: Option<String>,
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
        self.worker_model = v
            .get("workspace")
            .and_then(|w| w.get("worker_model"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let rules = |k: &str| -> Vec<String> {
            v.get("workspace")
                .and_then(|w| w.get(k))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        self.worker_rules = worker_rules_text(&rules("worker_allow"), &rules("worker_deny"));
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

/// Description used where a property repeats one defined elsewhere.
const SAME_AS_PLAN: &str = "Same as in submit_plan.";
const SAME_AS_SPAWN: &str = "Same as in spawn_session.";

/// Replaces the description of every property with `text` (schemas stay
/// identical, only the prose shrinks).
fn with_brief_descriptions(mut props: Value, text: &str) -> Value {
    if let Some(map) = props.as_object_mut() {
        for v in map.values_mut() {
            v["description"] = json!(text);
        }
    }
    props
}

/// Campi di un task della board (`submit_plan` li usa per ogni task,
/// `update_task` li accetta tutti; `id` identifica il task).
fn task_spec_properties() -> Value {
    json!({
        "id": {
            "type": "string",
            "pattern": "^[a-z0-9][a-z0-9-]{0,39}$",
            "description": "Unique id: lowercase letters, digits, '-', 1..40 chars. Names the worktree folder and branch task/<id>."
        },
        "title": {
            "type": "string",
            "maxLength": 80,
            "description": "Board title."
        },
        "task": {
            "type": "string",
            "minLength": 1,
            "maxLength": 100000,
            "description": "Worker instructions, in English; the worker cannot see your conversation."
        },
        "depends_on": {
            "type": "array",
            "items": { "type": "string" },
            "maxItems": 50,
            "description": "Ids that must be done first (same call or on the board). No cycles; a cancelled one never satisfies."
        },
        "touches": {
            "type": "array",
            "items": { "type": "string", "maxLength": 256 },
            "maxItems": 50,
            "description": "Paths modified, relative to the workspace root (or repo). Tasks with overlapping paths never run together."
        },
        "isolation": {
            "type": "string",
            "enum": ["worktree", "none"],
            "description": "worktree (default): own git worktree and branch task/<id>. none: the workspace folder, no branch; at most one non read-only 'none' task runs at a time."
        },
        "base_branch": {
            "type": ["string", "null"],
            "description": "Worktree start branch, read at start. Default: integration branch, else the repo/board base branch, else the origin default."
        },
        "repo": {
            "type": ["string", "null"],
            "maxLength": 256,
            "description": "Multi-repository workspaces: repo folder relative to the workspace root (see `repos` in get_board); worktree and 'touches' are relative to it. Omit or '.' for the root. Fixed once the task has a worktree."
        },
        "agent": { "type": "string", "description": SAME_AS_SPAWN },
        "model": {
            "type": ["string", "null"],
            "pattern": "^[A-Za-z0-9._-]{1,64}$",
            "description": SAME_AS_SPAWN
        },
        "permission_mode": {
            "type": ["string", "null"],
            "enum": ["default", "accept-edits", "plan", "bypass", null],
            "description": SAME_AS_SPAWN
        },
        "priority": {
            "type": "integer",
            "description": "Higher starts first among ready tasks."
        },
        "held": {
            "type": "boolean",
            "description": "If true the task is not started until released (held: false via update_task, or by the user). Shown only on queued tasks; a cancelled task keeps it, so a retry stays held."
        },
        "orchestrator_note": {
            "type": ["string", "null"],
            "maxLength": 2000,
            "description": "Private note, shown by get_board and get_task_details, NEVER sent to the worker. null clears it."
        }
    })
}

/// Impostazioni multi-repository della board (`submit_plan` e `set_board`).
fn repo_settings_properties() -> Value {
    json!({
        "base_branches": {
            "type": "object",
            "additionalProperties": { "type": ["string", "null"] },
            "description": "Base branch per repo, e.g. {\"api\": \"main\"} ('.' = root). Merged into existing entries; empty or null removes one. Default: origin default branch, else current."
        },
        "integration_branch": {
            "type": ["string", "null"],
            "description": "Template like 'integration/{date}' ({date} = YYYY-MM-DD). When set, each repo gets that branch with its own worktree <repo>/.worktrees/integration before its first task, and tasks without base_branch start from it (see `integrations` in get_board). A taken name gets -2, -3... Never pushed. Empty or null turns it off."
        }
    })
}

/// `props` piu' le impostazioni multi-repository (descrizioni brevi se `brief`).
fn with_repo_settings(mut props: Value, brief: bool) -> Value {
    let mut extra = repo_settings_properties();
    if brief {
        extra = with_brief_descriptions(extra, SAME_AS_PLAN);
    }
    if let (Some(p), Some(extra)) = (props.as_object_mut(), extra.as_object()) {
        p.extend(extra.clone());
    }
    props
}

/// Come `task_spec_properties`, con descrizioni di una riga.
fn update_task_properties() -> Value {
    let mut props = with_brief_descriptions(task_spec_properties(), SAME_AS_PLAN);
    props["id"]["description"] = json!("Id of the task to change.");
    props
}

fn orchestrator_tools() -> Vec<Value> {
    vec![
        tool(
            "list_agents",
            "List the coding agents you can start (id, name). The id goes in `agent`; default claude-code.",
            json!({}),
            &[],
        ),
        tool(
            "spawn_session",
            "Start one automated worker session in a new tab of your workspace (the workspace cannot be chosen). \
             Returns the session id immediately. The worker cannot see your conversation: make `task` \
             self-contained, in English. Max 16 active workers per orchestrator. For dependent batches use submit_plan.",
            json!({
                "task": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100000,
                    "description": "Complete worker instructions, in English."
                },
                "title": {
                    "type": "string",
                    "maxLength": 80,
                    "description": "Short tab title."
                },
                "agent": {
                    "type": "string",
                    "description": "Id of the coding agent (see list_agents). Default: claude-code."
                },
                "cwd": {
                    "type": "string",
                    "description": "Existing absolute path equal to or inside the workspace folder (no '..'), in its environment (Windows or WSL path). Default: the workspace folder."
                },
                "permission_mode": {
                    "type": "string",
                    "enum": ["default", "accept-edits", "plan", "bypass"],
                    "description": "plan = read-only; default = asks on every action (the worker stays stuck waiting for the user); accept-edits = approves file edits; bypass = no checks (Claude Code asks a one-time confirmation unless ~/.claude/settings.json has skipDangerousModePermissionPrompt: true). Default: the workspace maximum; you can only restrict it."
                },
                "model": {
                    "type": "string",
                    "pattern": "^[A-Za-z0-9._-]{1,64}$",
                    "description": "Model (e.g. sonnet, opus or a full id). Default: the workspace worker model, else the agent's own."
                },
                "keep_open": {
                    "type": "boolean",
                    "description": "Keep the tab open after complete_task (default: closes after a few seconds)."
                }
            }),
            &["task"],
        ),
        tool(
            "list_sessions",
            "List your sessions with status and last report. Status: starting (`starting_secs` since spawn), running, \
             background (waiting on a background command; resumes by itself), needs_input (waiting for the user), idle \
             (stopped without complete_task), completed, failed, blocked, closed. For news prefer get_events.",
            json!({}),
            &[],
        ),
        tool(
            "send_message",
            "Type a message into the prompt of one of your sessions, as if the user wrote it (answer a help_request, \
             new instructions). In English.",
            json!({
                "session": { "type": "string", "description": "Session id." },
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "Message text, in English." }
            }),
            &["session", "text"],
        ),
        tool(
            "close_session",
            "Close the tab of one of your sessions, even if unfinished. Work in progress is lost.",
            json!({
                "session": { "type": "string", "description": "Session id." }
            }),
            &["session"],
        ),
        tool(
            "get_events",
            "Drain and return the new events of your sessions. It never waits: `events` is empty if nothing happened. \
             Event: seq, session, title, kind, text (optional), status, at (unix ms), task (board workers), request. \
             kind: started, fact, help_request (answer with send_message), completed (outcome in text), needs_input \
             (waiting for a permission in its tab), stopped_without_report, exited, start_failed (board task could not \
             start; text = reason), plan_completed (every board task done or cancelled; text = counts), user_action \
             (informational), user_answer (text = the answer), settings_decision (text = approved or rejected, plus \
             ': comment'). Only started, fact and user_action do not wake you while idle; they wait for the next call.",
            json!({}),
            &[],
        ),
        tool(
            "submit_plan",
            "Add tasks to the persistent board of your workspace (you become its owner). The scheduler starts each task, in \
             its own git worktree, when its dependencies are done, a slot is free (max_parallel) and no running task \
             touches overlapping areas. All-or-nothing validation (unique ids, existing dependencies, no cycles, \
             permissions within the workspace maximum, git repo for worktree tasks). Limits: 50 tasks per call, 200 per \
             board. Returns the compact board (as get_board). Columns: backlog (waiting for dependencies), ready, \
             running, review (worker finished: you integrate it, then set_task_status done), done, failed, blocked, \
             cancelled.",
            with_repo_settings(json!({
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "properties": task_spec_properties(),
                        "required": ["id", "task"],
                        "additionalProperties": false
                    },
                    "description": "The tasks to add."
                },
                "max_parallel": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 16,
                    "description": "Concurrent board workers (default 3 for a new board)."
                },
                "base_branch": {
                    "type": "string",
                    "description": "Default base branch of root tasks (default: branch checked out when each task starts). Repo tasks use base_branches."
                }
            }), false),
            &["tasks"],
        ),
        tool(
            "get_board",
            "Compact summary of the board of your workspace (makes you its owner): settings, `integrations` (repo -> {branch, \
             path, base, setup}: where to merge and test; setup done | none = not configured | pending; missing: true if \
             the worktree is gone), `repos` (git repositories, '.' = root), `automation` (null for a normal workspace, \
             else execution-window state; while closed no new task starts), `counts` per column and one short row per \
             open task (long texts truncated). done and cancelled are only counted: pass `columns` to list them. held: \
             true only on queued tasks (in ready, not started); top-level `held` counts them. Never waits.",
            json!({
                "columns": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["backlog", "ready", "running", "review", "done", "failed", "blocked", "cancelled"]
                    },
                    "description": "List only tasks in these columns (default: all except done and cancelled)."
                }
            }),
            &[],
        ),
        tool(
            "get_task_details",
            "Full detail of one board task: text, dependencies, touches, isolation, status, column, worker status, \
             attempts, worktree, notes, external, and the structured report (summary, commit, files_changed, tests, \
             follow_ups, diffstat). Never waits.",
            json!({
                "id": { "type": "string", "description": "Id of the task." }
            }),
            &["id"],
        ),
        tool(
            "update_task",
            "Change a board task: any submit_plan field, `id` only selects it. Only for queued, failed, blocked or \
             cancelled tasks; isolation is fixed once there is a worktree. Dependencies are revalidated. Omitted \
             fields stay; null clears base_branch, model, permission_mode. Returns the task (without its text unless \
             `task` changed); `warnings` when the board is paused.",
            update_task_properties(),
            &["id"],
        ),
        tool(
            "set_task_status",
            "Move a task to a new status. done: from review, failed or blocked, after you integrated the work; unblocks the \
             dependents and, unless cleanup is false, removes the worktree and branch (never forced; problems come back \
             as warnings). Not allowed for a running task. done also works from queued (even held) or cancelled for work \
             done outside the board: a non-empty `note` saying how is then required and the task is marked `external`. \
             done clears `held`. cancelled: from any status except done; a running task's tab is closed and its work \
             lost; dependents stay in backlog until you update them. queued: retry a failed, blocked, cancelled or \
             review task (worktree reused; the note is kept unless you pass a new one). deleted: remove a task that \
             never started (queued or cancelled, never attempted, no worktree, no session) and that no other task \
             depends on; it disappears from the board and the counts. Returns the task; `warnings` when the board is \
             paused.",
            json!({
                "id": { "type": "string", "description": "Id of the task." },
                "status": {
                    "type": "string",
                    "enum": ["done", "cancelled", "queued", "deleted"],
                    "description": "done = integrated; cancelled = give up; queued = retry; deleted = remove a never-started task."
                },
                "note": {
                    "type": "string",
                    "maxLength": 2000,
                    "description": "Note stored on the task. Required (non-empty) for done from queued or cancelled. For queued it replaces the existing one."
                },
                "cleanup": {
                    "type": "boolean",
                    "description": "Only for done: remove the task worktree and branch (default true)."
                }
            }),
            &["id", "status"],
        ),
        tool(
            "set_board",
            "Change board settings: max_parallel (1..16), paused (true = start nothing new; running workers go on) and, \
             for multi-repository workspaces, base_branches, integration_branch and close_integration. Returns the \
             compact board.",
            with_repo_settings(json!({
                "max_parallel": { "type": "integer", "minimum": 1, "maximum": 16, "description": "Concurrent board workers." },
                "paused": { "type": "boolean", "description": "Pause or resume the scheduler." },
                "close_integration": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Repo key ('.' = root) or array: forget the registered integration; the next task of the repo creates a fresh integration branch from the updated base. Worktree and branch stay for you to remove, unless close_integration_cleanup."
                },
                "close_integration_cleanup": {
                    "type": "boolean",
                    "description": "Only with close_integration: also run the repo cleanup hooks in the integration worktree, remove the worktree and delete the local integration branch only if already contained in its upstream (never pushes, never forces). Problems come back as `warnings`."
                }
            }), true),
            &[],
        ),
        tool(
            "get_workflow",
            "Return the workflow of your workspace: markdown written by the user with the method for this project (branching \
             and merge policy, verification, task design, parallelism, boundaries, escalation, unattended behavior, \
             reporting). Call it BEFORE planning. `workflow` is null when none exists (then follow the orchestrate \
             skill); otherwise it takes precedence over the skill, never over safety. Also returns, read-only, \
             `workspace_rules` (when non-empty) and `repo_settings` (per repo key: base_branch, container_root, \
             copy_ignored, setup, cleanup, remove_force; non-empty only).",
            json!({}),
            &[],
        ),
        tool(
            "save_workflow",
            "Overwrite the workflow of your workspace. Only for the workflow wizard, never on your own initiative. \
             In English, max 50000 characters.",
            json!({
                "content": { "type": "string", "minLength": 1, "maxLength": 50000, "description": "The complete workflow, in markdown." }
            }),
            &["content"],
        ),
        tool(
            "ask_user",
            "Ask the user a question; the answer comes later as a user_answer event, so end your turn after calling. For \
             decisions only the user can make. With `task` (a board task), a queued task is held until the question is \
             answered or dismissed. Returns {request}.",
            json!({
                "question": { "type": "string", "minLength": 1, "maxLength": 2000, "description": "The question." },
                "options": {
                    "type": "array",
                    "maxItems": 8,
                    "items": { "type": "string", "minLength": 1, "maxLength": 100 },
                    "description": "Suggested answers (the user may type another)."
                },
                "task": { "type": "string", "description": "Board task that waits for the answer." }
            }),
            &["question"],
        ),
        tool(
            "propose_settings",
            "Propose changes to the worker rules and repo settings of this workspace; the user approves or rejects them \
             in the board view (settings_decision event), so end your turn after calling. Nothing changes until \
             approved; only the fields you pass. Never permissions or allowed commands. Returns {request}.",
            json!({
                "reason": { "type": "string", "minLength": 1, "maxLength": 2000, "description": "Why, shown to the user." },
                "workspace_rules": {
                    "type": "string",
                    "maxLength": 4000,
                    "description": "Full new text of the worker rules; empty clears."
                },
                "repos": {
                    "type": "object",
                    "description": "Per repo key ('.' = root), only the fields to change; a list replaces the whole list; null clears base_branch or container_root.",
                    "additionalProperties": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "base_branch": { "type": ["string", "null"] },
                            "container_root": { "type": ["string", "null"] },
                            "copy_ignored": { "type": "array", "items": { "type": "string" } },
                            "setup": { "type": "array", "items": { "type": "string" } },
                            "cleanup": { "type": "array", "items": { "type": "string" } },
                            "remove_force": { "type": "boolean" }
                        }
                    }
                }
            }),
            &["reason"],
        ),
    ]
}

fn worker_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your task and the working instructions.",
            json!({}),
            &[],
        ),
        tool(
            "report_fact",
            "Tell the orchestrator a useful fact or progress (finding, decision). Not the conclusion: use complete_task. \
             Nobody reads your terminal output.",
            json!({
                "text": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "The fact, self-contained." }
            }),
            &["text"],
        ),
        tool(
            "request_help",
            "Ask the orchestrator a question when blocked or a decision is needed, then wait: the answer arrives as a \
             message in your prompt. Not for what you can resolve yourself.",
            json!({
                "question": { "type": "string", "minLength": 1, "maxLength": 20000, "description": "The question, with the context to answer it." }
            }),
            &["question"],
        ),
        tool(
            "complete_task",
            "ALWAYS call it at the end, even on failure or when blocked: it hands the outcome to the orchestrator. The tab \
             then closes by itself. Fill the optional fields when they apply: they form the structured report used to \
             review and merge your work.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed", "blocked"],
                    "description": "completed = succeeded; failed; blocked = cannot proceed without intervention."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "What you did, files touched, test results, open issues."
                },
                "commit": {
                    "type": "string",
                    "maxLength": 100,
                    "description": "Hash of your last commit, if any."
                },
                "files_changed": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 512 },
                    "maxItems": 500,
                    "description": "Changed files, relative to your working folder."
                },
                "tests": {
                    "type": "array",
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "maxLength": 1000, "description": "The test command." },
                            "result": { "type": "string", "enum": ["passed", "failed", "not_run"] },
                            "details": { "type": "string", "maxLength": 2000, "description": "Failures, or why not run." }
                        },
                        "required": ["command", "result"],
                        "additionalProperties": false
                    },
                    "description": "Test commands run and their results."
                },
                "follow_ups": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 2000 },
                    "maxItems": 100,
                    "description": "Things left to do or check outside your task."
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

/// Frase sulle regole di permesso dei worker (`None` se non ce ne sono).
fn worker_rules_text(allow: &[String], deny: &[String]) -> Option<String> {
    if allow.is_empty() && deny.is_empty() {
        return None;
    }
    let mut text = String::new();
    if !allow.is_empty() {
        text.push_str(&format!(
            " Pre-approved for workers (they run these without asking, so do not tell them to ask the user): {}.",
            allow.join(", ")
        ));
    }
    if !deny.is_empty() {
        text.push_str(&format!(
            " Forbidden for workers (always refused, even in bypass mode): {}.",
            deny.join(", ")
        ));
    }
    Some(text)
}

/// Istruzioni generali restituite in `initialize`.
fn instructions(
    role: Option<Role>,
    workspace: Option<&str>,
    ceiling: Option<&str>,
    worker_model: Option<&str>,
    worker_rules: Option<&str>,
) -> String {
    match role {
        Some(Role::Orchestrator) => {
            let mut text = "cli-companion: you start automated sessions (workers) only in the workspace of your own tab: \
                spawn_session for one ad-hoc task, the task board (submit_plan) for a batch with dependencies. \
                Before orchestrating load the cli-companion-notify:orchestrate skill (if your agent supports skills) \
                and call get_workflow before planning: the workspace workflow takes precedence over the skill. \
                Never wait for workers (no sleep, polling or repeated calls): end your turn; cli-companion types a \
                message into your prompt when they need you, then call get_events. Board tasks start only while an \
                orchestrator is alive in the workspace, and cli-companion reopens one when needed, so ending your \
                turn is always safe. Write task and message texts in English."
                .to_string();
            if let Some(w) = workspace {
                text.push_str(&format!(" Your workspace: {w}."));
            }
            if let Some(c) = ceiling {
                text.push_str(&format!(
                    " Maximum worker permissions in this workspace: {c}. Without permission_mode workers start \
                     at this level; you can only restrict it."
                ));
                match worker_model {
                    Some(m) => text.push_str(&format!(" Default worker model: {m}.")),
                    None => text.push_str(" No default worker model: workers use the agent's own unless a task sets one."),
                }
                if let Some(r) = worker_rules {
                    text.push_str(r);
                }
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
                    "instructions": instructions(role, server.workspace.as_deref(), server.worker_permission.as_deref(), server.worker_model.as_deref(), server.worker_rules.as_deref()),
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

    const ORCH_TOOLS_CEILING: usize = 18_900; // +2_000: ask_user and propose_settings (+1_983 chars incl. get_events kinds)
    const WORKER_TOOLS_CEILING: usize = 2_800;
    const ORCH_INSTRUCTIONS_CEILING: usize = 1_250;

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

    fn orchestrator_instructions_full() -> String {
        let rules = worker_rules_text(&["Bash(git:*)".to_string()], &["Bash(rm:*)".to_string()]);
        instructions(
            Some(Role::Orchestrator),
            Some("Proj (wsl, /home/u/app)"),
            Some("accept-edits"),
            Some("sonnet"),
            rules.as_deref(),
        )
    }

    #[test]
    fn size_budget_of_the_model_facing_text() {
        let orch = serde_json::to_string(&orchestrator_tools()).unwrap().chars().count();
        let work = serde_json::to_string(&worker_tools()).unwrap().chars().count();
        let instr = orchestrator_instructions_full().chars().count();
        let worker_instr = instructions(Some(Role::Worker), None, None, None, None).chars().count();
        println!("SIZES orchestrator_tools={orch} worker_tools={work} orchestrator_instructions={instr} worker_instructions={worker_instr}");
        // The ceilings are intentional: tool definitions and server instructions are sent to the
        // model in every session. Procedural method belongs to the orchestrate skill
        // (agents/orchestrate_skill.md), loaded on demand. Do not raise a ceiling to fit new text:
        // shorten the text or move it to the skill.
        assert!(orch <= ORCH_TOOLS_CEILING, "orchestrator tools are {orch} chars, ceiling {ORCH_TOOLS_CEILING}: intentional budget, move method text to the skill");
        assert!(work <= WORKER_TOOLS_CEILING, "worker tools are {work} chars, ceiling {WORKER_TOOLS_CEILING}: intentional budget");
        assert!(instr <= ORCH_INSTRUCTIONS_CEILING, "orchestrator instructions are {instr} chars, ceiling {ORCH_INSTRUCTIONS_CEILING}: intentional budget, move method text to the skill");
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
        assert!(text.contains("in English") && text.contains("cli-companion-notify:orchestrate"));
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
            [
                "list_agents",
                "spawn_session",
                "list_sessions",
                "send_message",
                "close_session",
                "get_events",
                "submit_plan",
                "get_board",
                "get_task_details",
                "update_task",
                "set_task_status",
                "set_board",
                "get_workflow",
                "save_workflow",
                "ask_user",
                "propose_settings"
            ]
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
        assert!(props["cwd"]["description"].as_str().unwrap().contains("workspace folder"));
        let pm = props["permission_mode"]["description"].as_str().unwrap();
        assert!(pm.contains("skipDangerousModePermissionPrompt"));
        assert!(pm.contains("workspace maximum") && pm.contains("only restrict it"));
        let desc = spawn["description"].as_str().unwrap();
        assert!(desc.contains("in English") && desc.contains("submit_plan"));
        assert!(props["task"]["description"].as_str().unwrap().contains("English"));
    }

    #[test]
    fn board_tool_schemas_match_the_rust_validation() {
        let tools = orchestrator_tools();
        let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
        let keys = |v: &Value| -> Vec<String> {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        let mut spec_keys: Vec<String> = crate::board::SPEC_KEYS.iter().map(|s| s.to_string()).collect();
        spec_keys.sort();
        let plan = by("submit_plan");
        assert_eq!(
            keys(&plan["inputSchema"]["properties"]),
            ["base_branch", "base_branches", "integration_branch", "max_parallel", "tasks"]
        );
        let item = &plan["inputSchema"]["properties"]["tasks"]["items"];
        assert_eq!(keys(&item["properties"]), spec_keys);
        assert_eq!(item["required"], json!(["id", "task"]));
        assert_eq!(item["additionalProperties"], false);
        assert_eq!(plan["inputSchema"]["required"], json!(["tasks"]));
        let upd = by("update_task");
        assert_eq!(keys(&upd["inputSchema"]["properties"]), spec_keys);
        assert_eq!(upd["inputSchema"]["required"], json!(["id"]));
        assert_eq!(keys(&by("set_task_status")["inputSchema"]["properties"]), ["cleanup", "id", "note", "status"]);
        assert_eq!(by("set_task_status")["inputSchema"]["properties"]["status"]["enum"], json!(["done", "cancelled", "queued", "deleted"]));
        assert!(by("set_task_status")["description"].as_str().unwrap().contains("deleted"));
        assert_eq!(by("set_board")["inputSchema"]["properties"]["close_integration_cleanup"]["type"], "boolean");
        // update_task keeps the schema of submit_plan items, with one-line descriptions.
        assert_eq!(upd["inputSchema"]["properties"]["depends_on"]["maxItems"], item["properties"]["depends_on"]["maxItems"]);
        assert_eq!(upd["inputSchema"]["properties"]["depends_on"]["description"], SAME_AS_PLAN);
        assert_eq!(
            keys(&by("set_board")["inputSchema"]["properties"]),
            ["base_branches", "close_integration", "close_integration_cleanup", "integration_branch", "max_parallel", "paused"]
        );
        assert!(item["properties"]["repo"]["description"].as_str().unwrap().contains("Multi-repository") && item["properties"]["agent"]["description"] == SAME_AS_SPAWN);
        assert_eq!(keys(&by("get_board")["inputSchema"]["properties"]), ["columns"]);
        let sts = by("set_task_status")["description"].as_str().unwrap().to_string();
        assert!(sts.contains("from queued") && sts.contains("outside the board") && sts.contains("note"));
        let on = &plan["inputSchema"]["properties"]["tasks"]["items"]["properties"]["orchestrator_note"];
        assert_eq!(on["maxLength"], 2000);
        assert!(on["description"].as_str().unwrap().contains("NEVER sent to the worker"));
        
        assert_eq!(keys(&by("get_task_details")["inputSchema"]["properties"]), ["id"]);
        assert_eq!(by("get_task_details")["inputSchema"]["required"], json!(["id"]));
        // Il modello non deve attendere; la colonna e l'integrazione sono spiegate.
        let d = plan["description"].as_str().unwrap();
        for t in ["review", "set_task_status", "owner", "backlog", "ready"] {
            assert!(d.contains(t), "{t}");
        }
        assert!(by("get_board")["description"].as_str().unwrap().contains("Never waits"));
        assert!(item["properties"]["held"]["description"].as_str().unwrap().contains("cancelled task keeps it"));
        // Tutto in inglese (ASCII) e ogni tool ha descrizione e schema a oggetto.
        for t in &tools {
            assert!(t["description"].as_str().unwrap().is_ascii(), "{}", t["name"]);
            assert!(serde_json::to_string(&t["inputSchema"]).unwrap().is_ascii(), "{}", t["name"]);
            assert_eq!(t["inputSchema"]["type"], "object");
        }
        // get_events elenca i nuovi tipi di evento; complete_task i campi strutturati.
        let ev = by("get_events")["description"].as_str().unwrap().to_string();
        for k in ["start_failed", "plan_completed", "user_action"] {
            assert!(ev.contains(k), "{k}");
        }
        let done = &worker_tools()[3];
        assert_eq!(keys(&done["inputSchema"]["properties"]), ["commit", "files_changed", "follow_ups", "status", "summary", "tests"]);
        assert_eq!(done["inputSchema"]["required"], json!(["status", "summary"]));
        assert_eq!(
            done["inputSchema"]["properties"]["tests"]["items"]["properties"]["result"]["enum"],
            json!(["passed", "failed", "not_run"])
        );
        // Le istruzioni dell'orchestratore citano il flusso della board.
        let text = instructions(Some(Role::Orchestrator), None, None, None, None);
        assert!(text.contains("submit_plan") && text.contains("spawn_session"));
    }


    #[test]
    fn request_tool_schemas() {
        let tools = orchestrator_tools();
        let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
        let keys = |v: &Value| -> Vec<String> {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        let ask = by("ask_user");
        assert_eq!(keys(&ask["inputSchema"]["properties"]), ["options", "question", "task"]);
        assert_eq!(ask["inputSchema"]["required"], json!(["question"]));
        assert_eq!(ask["inputSchema"]["properties"]["options"]["maxItems"], 8);
        assert_eq!(ask["inputSchema"]["properties"]["options"]["items"]["maxLength"], 100);
        assert_eq!(ask["inputSchema"]["properties"]["question"]["maxLength"], 2000);
        let prop = by("propose_settings");
        assert_eq!(keys(&prop["inputSchema"]["properties"]), ["reason", "repos", "workspace_rules"]);
        assert_eq!(prop["inputSchema"]["required"], json!(["reason"]));
        // Same fields as the settings patch the app validates (never permissions or allowed commands).
        let repo = &prop["inputSchema"]["properties"]["repos"]["additionalProperties"];
        assert_eq!(
            keys(&repo["properties"]),
            ["base_branch", "cleanup", "container_root", "copy_ignored", "remove_force", "setup"]
        );
        assert_eq!(repo["additionalProperties"], false);
        for t in [&ask, &prop] {
            assert!(t["description"].as_str().unwrap().contains("end your turn"));
        }
        assert!(prop["description"].as_str().unwrap().contains("Never permissions"));
        // get_events lists the new kinds and the request field.
        let ev = by("get_events")["description"].as_str().unwrap().to_string();
        assert!(ev.contains("user_answer") && ev.contains("settings_decision") && ev.contains("request"));
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
    fn instructions_list_worker_rules() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(worker_rules_text(&[], &[]), None);
        let r = worker_rules_text(&v(&["Bash(git:*)"]), &v(&["Bash(rm:*)"]));
        let text = instructions(Some(Role::Orchestrator), Some("w"), Some("default"), None, r.as_deref());
        assert!(text.contains("Pre-approved for workers") && text.contains("Bash(git:*)"));
        assert!(text.contains("Forbidden for workers") && text.contains("Bash(rm:*)"));
    }

    #[test]
    fn workflow_tools_and_window_instructions() {
        let tools = orchestrator_tools();
        let by = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
        assert_eq!(by("get_workflow")["inputSchema"]["properties"], json!({}));
        assert!(by("get_workflow")["description"].as_str().unwrap().contains("takes precedence"));
        let save = by("save_workflow");
        assert_eq!(save["inputSchema"]["required"], json!(["content"]));
        assert_eq!(save["inputSchema"]["properties"]["content"]["maxLength"], 50000);
        assert!(by("get_board")["description"].as_str().unwrap().contains("`automation`"));
        let text = instructions(Some(Role::Orchestrator), None, None, None, None);
        assert!(text.contains("get_workflow") && text.contains("takes precedence"));
        assert!(text.contains("end your turn"));
        // I tool del workflow non esistono per i worker.
        assert!(worker_tools().iter().all(|t| t["name"] != "get_workflow" && t["name"] != "save_workflow"));
    }

    #[test]
    fn orchestrator_is_told_to_end_its_turn_instead_of_waiting() {
        let tools = orchestrator_tools();
        let get = tools.iter().find(|t| t["name"] == "get_events").unwrap();
        assert_eq!(get["inputSchema"]["properties"], json!({}));
        let desc = get["description"].as_str().unwrap();
        assert!(desc.contains("never waits"));
        let text = instructions(Some(Role::Orchestrator), None, None, None, None);
        assert!(text.contains("end your turn") && text.contains("Never wait"));
        assert!(text.contains("cli-companion-notify:orchestrate"));
        assert!(text.contains("ending your turn is always safe") || text.contains("turn is always safe"));
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
