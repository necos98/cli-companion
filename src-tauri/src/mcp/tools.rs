//! Definizione dei tool MCP e delle istruzioni del server (le legge il
//! modello: inglese, concise). Due insiemi: i tool del worker di un task e
//! quelli di una sessione normale (Intake compreso).

use serde_json::{json, Value};

/// Ruolo del chiamante, come riportato da `whoami`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Qualunque tab di cli-companion che non e' il worker di un task.
    Session,
    Worker,
}

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

/// Campi di un task proposto (`propose_tasks`) o modificato (`update_task`).
fn task_properties(with_id: bool) -> Value {
    let mut props = json!({
        "title": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80,
            "description": "Short title shown in the queue."
        },
        "task": {
            "type": "string",
            "minLength": 1,
            "maxLength": 100000,
            "description": "Complete specification for a worker that cannot see this conversation: Goal, Context, Constraints, Done when, Verify (commands)."
        },
        "depends_on": {
            "type": "array",
            "items": { "type": "string" },
            "maxItems": 50,
            "description": "Ids that must be done first: tasks of the same proposal or already in the queue. No cycles."
        },
        "repo": {
            "type": ["string", "null"],
            "maxLength": 256,
            "description": "Multi-repository workspaces: repository folder relative to the workspace root. Omit or '.' for the root."
        },
        "model": {
            "type": ["string", "null"],
            "pattern": "^[A-Za-z0-9._-]{1,64}$",
            "description": "Model for the worker (e.g. sonnet for well-defined work, opus for design or risk). Omit for the workspace default."
        }
    });
    if with_id {
        props["id"] = json!({
            "type": "string",
            "pattern": "^[a-z0-9][a-z0-9-]{0,39}$",
            "description": "Unique id: lowercase letters, digits, '-', 1..40 chars. Names the branch task/<id>."
        });
    }
    props
}

pub fn session_tools() -> Vec<Value> {
    let mut update = task_properties(false);
    update["id"] = json!({ "type": "string", "description": "Id of the task to change." });
    let mut proposed = task_properties(true);
    proposed["model"] = json!({
        "type": "string",
        "pattern": "^[A-Za-z0-9._-]{1,64}$",
        "description": "Model for the worker: sonnet for well-defined work, opus for design, risky changes or hard bugs."
    });
    vec![
        tool(
            "propose_tasks",
            "Propose tasks for the queue of this workspace. The user accepts or discards the proposal in the app; nothing \
             starts before that and you get no answer: never wait for it. All-or-nothing validation: ids unique (also against \
             queued tasks and other pending proposals), dependencies on tasks of the proposal or already in the queue, no cycles, \
             existing repo. Limits: 20 tasks per proposal. Returns {proposal, status: \"pending\"}.",
            json!({
                "tasks": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 20,
                    "items": {
                        "type": "object",
                        "properties": proposed,
                        "required": ["id", "title", "task", "model"],
                        "additionalProperties": false
                    },
                    "description": "The tasks, in the order they should be queued."
                }
            }),
            &["tasks"],
        ),
        tool(
            "list_tasks",
            "List the task queue of this workspace: counts per status (queued, running, done, failed), pending proposals and one \
             compact row per task (id, title, status, attention, waiting_on, summary, note). Never waits.",
            json!({
                "status": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["queued", "running", "done", "failed"] },
                    "description": "Only tasks in these statuses (default: all)."
                }
            }),
            &[],
        ),
        tool(
            "get_task_details",
            "Full detail of one task: specification, dependencies, status, report (summary, commit, PR link, tests), note, worktree.",
            json!({ "id": { "type": "string", "description": "Id of the task." } }),
            &["id"],
        ),
        tool(
            "update_task",
            "Change a queued or failed task; `id` only selects it, omitted fields stay, null clears repo and model. `repo` is \
             fixed once the task has a worktree. Dependencies are revalidated. Returns the task without its text.",
            update,
            &["id"],
        ),
        tool(
            "delete_task",
            "Delete a queued or failed task that no unfinished task depends on.",
            json!({ "id": { "type": "string", "description": "Id of the task." } }),
            &["id"],
        ),
    ]
}

pub fn worker_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your task, the working instructions, the environment, the delivery \
             steps and, after a retry, `previous_attempt`.",
            json!({}),
            &[],
        ),
        tool(
            "complete_task",
            "ALWAYS call it at the end: completed once the work is delivered as the delivery steps say, failed if you cannot \
             complete or deliver (blocked is accepted as failed). The tab closes by itself afterwards.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "completed = delivered; failed = could not complete or deliver (say why in summary)."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "What you did, files touched, test results, open issues."
                },
                "commit": { "type": "string", "maxLength": 100, "description": "Hash of your last commit, if any." },
                "pr_url": { "type": "string", "maxLength": 500, "description": "Link of the pull request (PR mode)." },
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

pub fn tools_for(role: Role) -> Vec<Value> {
    match role {
        Role::Session => session_tools(),
        Role::Worker => worker_tools(),
    }
}

/// Istruzioni generali restituite in `initialize`.
pub fn instructions(role: Option<Role>, workspace: Option<&str>) -> String {
    match role {
        Some(Role::Session) => {
            let mut text = "cli-companion: you may propose tasks for this workspace's queue with propose_tasks. The user \
                accepts or discards them in the app and automated workers deliver them on their own. Never wait for workers \
                or for the decision (no sleep, polling or repeated calls): end your turn. list_tasks, get_task_details, \
                update_task and delete_task inspect and fix the queue. Write task texts in the language of the conversation."
                .to_string();
            if let Some(w) = workspace {
                text.push_str(&format!(" Your workspace: {w}."));
            }
            text
        }
        Some(Role::Worker) => "cli-companion: you are an automated worker. Call get_task first, deliver the work as the \
             delivery steps say and ALWAYS finish with complete_task. If you need a decision from the user, ask in plain text \
             and end your turn: the user answers in your terminal."
            .to_string(),
        None => "cli-companion: tools are only available in cli-companion terminals with the app running.".to_string(),
    }
}
