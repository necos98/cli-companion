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

/// Campi di un task della board (`submit_plan` li usa per ogni task,
/// `update_task` li accetta tutti; `id` identifica il task).
fn task_spec_properties() -> Value {
    json!({
        "id": {
            "type": "string",
            "pattern": "^[a-z0-9][a-z0-9-]{0,39}$",
            "description": "Unique id on the board: lowercase letters, digits and '-', 1..40 characters (e.g. auth-api). It also names the worktree folder and the branch task/<id>."
        },
        "title": {
            "type": "string",
            "maxLength": 80,
            "description": "Short title shown on the board (default: the first words of the task)."
        },
        "task": {
            "type": "string",
            "minLength": 1,
            "maxLength": 100000,
            "description": "Complete instructions for the worker, in English: goal, files or folders involved, constraints, success criteria. The worker cannot see your conversation."
        },
        "depends_on": {
            "type": "array",
            "items": { "type": "string" },
            "maxItems": 50,
            "description": "Ids of the tasks that must be done (status done: reviewed, merged and marked by you) before this one starts. They may be in the same call or already on the board. No cycles. A cancelled dependency never satisfies: update the dependencies."
        },
        "touches": {
            "type": "array",
            "items": { "type": "string", "maxLength": 256 },
            "maxItems": 50,
            "description": "Areas of the repository this task will modify, as paths relative to the workspace root (e.g. src/auth/ or package.json). Two tasks whose areas overlap (equal, or one inside the other) never run at the same time. Be accurate: too broad serializes the plan, too narrow lets conflicting work run in parallel. Empty = no declared area."
        },
        "isolation": {
            "type": "string",
            "enum": ["worktree", "none"],
            "description": "worktree (default): the app creates a git worktree .worktrees/<id> on the branch task/<id> for the worker; use it for anything that changes code. none: the worker uses the workspace folder directly, with no branch; at most one non read-only 'none' task (permission_mode other than plan) runs at a time."
        },
        "base_branch": {
            "type": ["string", "null"],
            "description": "Branch the worktree starts from, read when the task starts. Default: the integration branch of the board when integration_branch is on, else the board's base_branches entry for the task's repo, else the board's base_branch (root tasks), else for a repo task the default branch of its origin, else the branch currently checked out in the workspace folder (so dependents start from a base that already contains the merged work of their dependencies)."
        },
        "repo": {
            "type": ["string", "null"],
            "maxLength": 256,
            "description": "Multi-repository workspaces (the workspace folder is not a git repo but contains several): the repository this task works in, as a folder relative to the workspace root (e.g. freud-manager). The worktree is created in that repository (<repo>/.worktrees/<id>), 'touches' are relative to it and tasks of different repositories never conflict. Omit (or '.') for the workspace root itself. See the repos field of get_board. Cannot change once the task has a worktree."
        },
        "agent": { "type": "string", "description": "Id of the coding agent (see list_agents). Default: claude-code." },
        "model": {
            "type": ["string", "null"],
            "pattern": "^[A-Za-z0-9._-]{1,64}$",
            "description": "Model to use (e.g. sonnet, opus or a full id). Default: the workspace's worker model (see the instructions), else the coding agent's own."
        },
        "permission_mode": {
            "type": ["string", "null"],
            "enum": ["default", "accept-edits", "plan", "bypass", null],
            "description": "Worker permissions, same rules as spawn_session: the default is the workspace maximum and you can only restrict it. 'plan' makes the worker read-only."
        },
        "priority": {
            "type": "integer",
            "description": "Higher starts first among the tasks that are ready (default 0)."
        },
        "held": {
            "type": "boolean",
            "description": "If true the scheduler does not start the task until you release it with update_task (held: false) or the user does from the board view."
        },
        "orchestrator_note": {
            "type": ["string", "null"],
            "maxLength": 2000,
            "description": "Private note for yourself (e.g. merge order, what to check at review). Shown on the board and returned by get_board and get_task_details, but NEVER sent to the worker. null clears it."
        }
    })
}

/// Impostazioni multi-repository della board (`submit_plan` e `set_board`).
fn repo_settings_properties() -> Value {
    json!({
        "base_branches": {
            "type": "object",
            "additionalProperties": { "type": ["string", "null"] },
            "description": "Base branch per repository: {\"freud-manager\": \"master\", \"DDB\": \"main\"} (key '.' = the workspace root). Merged into the current entries; an empty string or null removes an entry. Tasks of a repo start from it (fetched from origin first when origin exists) unless integration_branch is on or the task has its own base_branch. Default for a repo: the default branch of its origin, else its current branch."
        },
        "integration_branch": {
            "type": ["string", "null"],
            "description": "Template of the integration branch, e.g. 'integration/{date}' ({date} = local date YYYY-MM-DD). When set, before the first task of a repository starts cli-companion creates that branch (from the repo base, after a fetch) with its own worktree <repo>/.worktrees/integration, and every worktree task of the repo without an explicit base_branch starts from it, so dependents see what you merged there. get_board returns `integrations` (repo -> {branch, path, base, setup}): merge and test in that worktree. If the branch name already exists and is not ours, -2, -3... is appended. The app never pushes and never switches branches in the main checkout. Empty string or null turns it off."
        }
    })
}

/// `props` piu' le impostazioni multi-repository.
fn with_repo_settings(mut props: Value) -> Value {
    if let (Some(p), Some(extra)) = (props.as_object_mut(), repo_settings_properties().as_object()) {
        p.extend(extra.clone());
    }
    props
}

/// Come `task_spec_properties`, senza `id` obbligatorio nel testo.
fn update_task_properties() -> Value {
    let mut props = task_spec_properties();
    props["id"] = json!({ "type": "string", "description": "Id of the task to change." });
    props
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
                    "description": "Model to use (e.g. sonnet, opus or a full id). Default: the workspace's worker model (see the instructions), else the coding agent's own."
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
             called complete_task), completed, failed, blocked, closed. A starting session also has \
             `starting_secs` (seconds since it was spawned) so you can tell a slow start from a stuck one. \
             Includes the last report. \
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
             nudged), exited (the tab was closed), start_failed (a board task could not be started, e.g. \
             a git error: text has the reason), plan_completed (every task on the board is done or \
             cancelled; text gives the counts: done, of which outside the board, failed, cancelled; not sent \
             when every task was cancelled without ever starting), user_action (the user acted from the board view; informational, it does not \
             wake you). Events of board workers also carry the id of their task in `task`.",
            json!({}),
            &[],
        ),
        tool(
            "submit_plan",
            "Add a batch of tasks to the task board of your workspace and let cli-companion run them. The board \
             is persistent and its scheduler starts the workers by itself: a task starts as soon as all its \
             dependencies are done, a slot is free (max_parallel) and no running task touches the same areas. \
             For a single ad-hoc session use spawn_session instead. cli-companion creates the git worktree of \
             each task and tells the worker where to work. Everything is validated before anything changes \
             (unique ids, dependencies that exist, no cycles, permissions within the workspace maximum, a git \
             repository for worktree tasks): on error nothing is added. Dependencies may point to tasks of \
             the same call or already on the board. You become the owner of the board (the last orchestrator \
             tab calling a board tool): worker events arrive in your queue. Returns the compact board (as get_board); each task has a \
             derived column: backlog (waiting for dependencies), ready (can start, waiting for a slot or for a \
             conflicting task), running, review (the worker finished: you integrate it), done, failed, \
             blocked, cancelled. After submitting, end your turn: never wait, sleep or poll; cli-companion \
             types a message into your prompt when something needs you (then call get_events). Integration is \
             yours: when a task reaches review, check its report and diffstat, merge its branch task/<id> into \
             the base branch, run the regression tests, then call set_task_status with done (this unblocks the \
             dependents and removes the worktree). Limits: 50 tasks per call, 200 per board.",
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
                    "description": "How many board workers run at the same time (default 3 for a new board). Also capped by the 16 active workers per orchestrator."
                },
                "base_branch": {
                    "type": "string",
                    "description": "Default base branch of the board for the worktrees of root tasks (default: the branch currently checked out, read when each task starts). For repo tasks use base_branches."
                }
            })),
            &["tasks"],
        ),
        tool(
            "get_board",
            "Return a compact summary of the task board of your workspace (and make you its owner): settings \
             (max_parallel, paused, base_branch, plus base_branches, integration_branch and `integrations` \
             (repo -> {branch, path, base, setup}: where to merge and test; setup is done, none = no setup configured for the repo so nothing was prepared, or pending = setup not completed; missing: true when the worktree folder no longer exists; an entry appears after the first worktree task of the repo starts) when set, and `repos`: the git repositories \
             found in the workspace, \".\" being the workspace root itself), `automation`, `counts` (tasks per derived column: backlog, \
             ready, running, review, done, failed, blocked, cancelled; empty columns left out) and `tasks`, one \
             short row per open task (id, title, column, attempts, plus held, external, worker_status, waiting_on for a \
             backlog task, priority, model, note, orchestrator_note and the report summary when they apply; note, \
             orchestrator_note and summary are truncated). A held task is in the ready column but does not start: \
             it has `held: true` (only on queued tasks), and the top-level `held` is the number of held tasks (left out when 0). Done and cancelled tasks are only counted: pass `columns` to list specific ones, e.g. \
             [\"done\"]. Use get_task_details for the full text, report (commit, files_changed, tests, \
             follow_ups, diffstat), worktree and note of one task. `automation` is null for a normal workspace; \
             for an automatic one it tells whether execution windows are enabled, whether the window is open \
             now and when it changes (next_change_at, unix ms): while it is closed the scheduler starts no new \
             task. It never waits. Do not call it in a loop: events tell you when something changes.",
            json!({
                "columns": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["backlog", "ready", "running", "review", "done", "failed", "blocked", "cancelled"]
                    },
                    "description": "List only the tasks in these columns (default: every column except done and cancelled)."
                }
            }),
            &[],
        ),
        tool(
            "get_task_details",
            "Return the full detail of one task of the board: its text, dependencies, touches, isolation, \
             status and column, worker status, attempts, worktree, last report, note, your private orchestrator_note, \
             external (completed outside the board) and the structured report \
             (summary, commit, files_changed, tests, follow_ups, diffstat). Use it when you review a task in \
             review, failed or blocked, before merging or retrying it. It never waits.",
            json!({
                "id": { "type": "string", "description": "Id of the task." }
            }),
            &["id"],
        ),
        tool(
            "update_task",
            "Change a task of the board: any field of submit_plan except that id only identifies the task. Only \
             for tasks that are queued, failed, blocked or cancelled (not running, review or done); isolation \
             cannot change once the task has a worktree. Dependencies are validated again (existing tasks, no \
             cycles). Use it to release a held task (held: false), fix the instructions before a retry, or \
             re-point the dependencies of a task whose dependency was cancelled. Fields you leave out stay as \
             they are; null clears base_branch, model and permission_mode. Returns the updated task (without its text, unless the patch changes `task`); `warnings` when the board is paused.",
            update_task_properties(),
            &["id"],
        ),
        tool(
            "set_task_status",
            "Move a task to a new status. done: from review, failed or blocked, after you integrated the work \
             (merged the branch task/<id>, ran the tests); it unblocks the dependents and, unless cleanup is \
             false, removes the task worktree and branch (git worktree remove and git branch -D when the branch is already merged into the integration branch of the repo (or the worktree base), else -d with a warning, without \
             force: problems come back as warnings and in the task note, they never block the change). Not \
             allowed for a running task. done also works from queued (including held) or cancelled for work completed \
             outside the board (e.g. you or the user did it by hand): a non-empty `note` saying how is then \
             required, the task is marked as completed outside the board (`external`) and the dependents are \
             unblocked. Moving to done clears `held`. cancelled: from any status except done; a running task has its tab \
             closed and its work is lost; tasks depending on it stay in backlog until you update their \
             dependencies. queued: retry a failed, blocked, cancelled or review task; the scheduler starts it \
             again when possible (the existing worktree is reused); the existing note is kept unless you pass a new `note`. Returns the task (without its text); `warnings` when the board is paused.",
            json!({
                "id": { "type": "string", "description": "Id of the task." },
                "status": {
                    "type": "string",
                    "enum": ["done", "cancelled", "queued"],
                    "description": "done = integrated; cancelled = give up; queued = retry."
                },
                "note": {
                    "type": "string",
                    "maxLength": 2000,
                    "description": "Note stored on the task (e.g. why it was cancelled). Required (non-empty) for done from queued or cancelled. For queued a new note replaces the existing one."
                },
                "cleanup": {
                    "type": "boolean",
                    "description": "Only for done: remove the worktree and the branch of the task (default true)."
                }
            }),
            &["id", "status"],
        ),
        tool(
            "set_board",
            "Change the settings of the board: max_parallel (how many board workers run at once, 1..16) and/or \
             paused (true = the scheduler starts nothing new; running workers go on), and for multi-repository \
             workspaces base_branches, integration_branch and close_integration. Returns the compact board (as get_board).",
            with_repo_settings(json!({
                "max_parallel": { "type": "integer", "minimum": 1, "maximum": 16, "description": "Board workers running at the same time." },
                "paused": { "type": "boolean", "description": "Pause or resume the scheduler." },
                "close_integration": {
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ],
                    "description": "Repo key ('.' = root) or array of keys: forget the registered integration of that repo. The worktree and the branch are not touched (you remove them); the next task of the repo creates a fresh integration branch (next free suffix, e.g. -3) from the updated base."
                }
            })),
            &[],
        ),
        tool(
            "get_workflow",
            "Return the workflow of your workspace: a markdown document written by the user (usually with the 
             workflow wizard) that gives you an identity and a method for this project: goals, branching and merge 
             policy, verification commands, task design, parallelism and models, boundaries, when to escalate, what 
             to do unattended, reporting. Call it BEFORE planning anything. `workflow` is null when the workspace 
             has none: then follow the generic method of the orchestrate skill. When it exists it takes precedence 
             over that generic method (but never over safety: do not push or merge what the workflow forbids). It also returns, read-only, `workspace_rules` (the \"Workspace rules\" text the user set; only when non-empty) and `repo_settings` (per repo key: base_branch, container_root, copy_ignored, setup, cleanup, remove_force; only non-empty ones). Only the user edits them, in the workspace settings.",
            json!({}),
            &[],
        ),
        tool(
            "save_workflow",
            "Save (overwrite) the workflow of your workspace. Used by the workflow wizard, which interviews the 
             user and saves the confirmed document: do not call it on your own initiative. Write it in English. 
             Max 50000 characters.",
            json!({
                "content": { "type": "string", "minLength": 1, "maxLength": 50000, "description": "The complete workflow, in markdown." }
            }),
            &["content"],
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
             Normally the tab closes by itself shortly after. Besides status and summary, fill the optional \
             structured fields whenever they apply: they become the structured report of your task on the \
             board, which the orchestrator uses to review and merge your work.",
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
                },
                "commit": {
                    "type": "string",
                    "maxLength": 100,
                    "description": "Hash of your last commit, if you committed."
                },
                "files_changed": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 512 },
                    "maxItems": 500,
                    "description": "Paths of the files you changed, relative to your working folder."
                },
                "tests": {
                    "type": "array",
                    "maxItems": 50,
                    "items": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "maxLength": 1000, "description": "The test command you ran." },
                            "result": { "type": "string", "enum": ["passed", "failed", "not_run"] },
                            "details": { "type": "string", "maxLength": 2000, "description": "Optional: failures, or why it was not run." }
                        },
                        "required": ["command", "result"],
                        "additionalProperties": false
                    },
                    "description": "The test commands you ran and their results."
                },
                "follow_ups": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 2000 },
                    "maxItems": 100,
                    "description": "Things left to do, or worth checking, that are outside your task."
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
            let mut text = "cli-companion: you can start automated sessions, but only in the workspace of your own tab: \
                workers open there and spawn_session cannot pick another one. Flow: spawn_session for \
                each task, then end your turn. Never wait for workers (no sleep, no polling, no \
                repeated tool calls): when they need you cli-companion types a message into your \
                prompt; then call get_events, handle the events (answer help_requests with \
                send_message) and end your turn again. Use close_session only when needed. Write task \
                and message texts for workers in English. For a batch of tasks with dependencies use the \
                task board: submit_plan adds the tasks, cli-companion starts the workers when they are \
                ready (it creates their git worktrees) and, when a task reaches review, you integrate it \
                (merge, test) and call set_task_status done; get_board shows a compact state, get_task_details the full detail of one task. If the workspace folder holds several git repositories (get_board lists them in `repos`) set `repo` on each task and consider integration_branch. Tool results are already structured: read them directly, never parse them with shell scripts. For a single \
                ad-hoc session use spawn_session. Before orchestrating a batch of tasks, load \
                the orchestrate skill (cli-companion-notify:orchestrate) if your agent supports skills: \
                it has the full method (planning, task template, isolation, review and merge). Before planning call \
                get_workflow: the workflow of this workspace, when it exists, takes precedence over the generic \
                method of the skill. You may be the automatic orchestrator that cli-companion opens in a \
                workspace during its execution windows: the board scheduler only starts tasks while a window is \
                open; when cli-companion types that the execution window is closed, finish integrating the tasks \
                that complete, do not plan new work, and end your turn. Every worker (board or spawn_session) \
                already receives the workspace rules the user set in the workspace settings (as \"Workspace rules\" \
                in its instructions): do not repeat them in task texts; you can read them, and the per-repo settings, with get_workflow."
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
                match worker_model {
                    Some(m) => text.push_str(&format!(
                        " Default worker model in this workspace: {m} (used when a task or spawn_session sets no model)."
                    )),
                    None => text.push_str(
                        " No default worker model is set in this workspace: workers use the coding agent's default \
                         model unless a task or spawn_session sets one.",
                    ),
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
                "save_workflow"
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
        assert!(props["cwd"]["description"].as_str().unwrap().contains(".worktrees"));
        let pm = props["permission_mode"]["description"].as_str().unwrap();
        assert!(pm.contains("skipDangerousModePermissionPrompt"));
        assert!(pm.contains("workspace setting") && pm.contains("only restrict it"));
        let desc = spawn["description"].as_str().unwrap();
        assert!(desc.contains("in English") && desc.contains("send_message"));
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
        assert_eq!(by("set_task_status")["inputSchema"]["properties"]["status"]["enum"], json!(["done", "cancelled", "queued"]));
        assert_eq!(
            keys(&by("set_board")["inputSchema"]["properties"]),
            ["base_branches", "close_integration", "integration_branch", "max_parallel", "paused"]
        );
        assert!(item["properties"]["repo"]["description"].as_str().unwrap().contains("Multi-repository"));
        assert_eq!(keys(&by("get_board")["inputSchema"]["properties"]), ["columns"]);
        let sts = by("set_task_status")["description"].as_str().unwrap().to_string();
        assert!(sts.contains("from queued") && sts.contains("outside the board") && sts.contains("note"));
        let on = &plan["inputSchema"]["properties"]["tasks"]["items"]["properties"]["orchestrator_note"];
        assert_eq!(on["maxLength"], 2000);
        assert!(on["description"].as_str().unwrap().contains("NEVER sent to the worker"));
        assert!(instructions(Some(Role::Orchestrator), None, None, None, None).contains("workspace rules"));
        assert_eq!(keys(&by("get_task_details")["inputSchema"]["properties"]), ["id"]);
        assert_eq!(by("get_task_details")["inputSchema"]["required"], json!(["id"]));
        // Il modello non deve attendere; la colonna e l'integrazione sono spiegate.
        let d = plan["description"].as_str().unwrap();
        for t in ["never wait, sleep or poll", "review", "set_task_status", "owner", "backlog", "ready", "merge"] {
            assert!(d.contains(t), "{t}");
        }
        assert!(by("get_board")["description"].as_str().unwrap().contains("Do not call it in a loop"));
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
        assert!(text.contains("submit_plan") && text.contains("spawn_session") && text.contains("set_task_status"));
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
        assert!(text.contains("execution window is closed") && text.contains("end your turn"));
        // I tool del workflow non esistono per i worker.
        assert!(worker_tools().iter().all(|t| t["name"] != "get_workflow" && t["name"] != "save_workflow"));
    }

    #[test]
    fn orchestrator_is_told_to_end_its_turn_instead_of_waiting() {
        let tools = orchestrator_tools();
        let get = tools.iter().find(|t| t["name"] == "get_events").unwrap();
        assert_eq!(get["inputSchema"]["properties"], json!({}));
        let desc = get["description"].as_str().unwrap();
        assert!(desc.contains("never waits") && desc.contains("end your turn"));
        let text = instructions(Some(Role::Orchestrator), None, None, None, None);
        assert!(text.contains("end your turn") && text.contains("Never wait"));
        assert!(text.contains("cli-companion-notify:orchestrate"));
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
