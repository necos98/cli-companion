//! Definizione dei tool MCP e delle istruzioni del server (le legge il
//! modello: inglese, concise). Tre insiemi: i tool del worker di un task, quelli
//! del revisore (il worker di un task `review`) e quelli di una sessione normale
//! (Intake compreso).

use crate::agents::PermissionMode;
use crate::automation::settings::REASON_MAX;
use crate::workspaces::{
    DeliveryMode, MergeMethod, MAX_COMMAND_LEN, MAX_LAUNCH_NAME_LEN, MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS,
    MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_RULES_LEN,
};
use crate::worker_permissions::{MAX_RULES, MAX_RULE_CHARS};
use serde_json::{json, Value};

/// Ruolo del chiamante, come riportato da `whoami`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Qualunque tab di cli-companion che non e' il worker di un task.
    Session,
    Worker,
    /// Il worker di un task `review`: verifica il risultato integrato e propone le correzioni.
    Reviewer,
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
        },
        "kind": {
            "type": "string",
            "enum": ["work", "review"],
            "description": "review = a reviewer checks the integrated result of its depends_on (at least one) and proposes fixes; default work."
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

fn propose_tasks_tool() -> Value {
    let mut proposed = task_properties(true);
    proposed["model"] = json!({
        "type": "string",
        "pattern": "^[A-Za-z0-9._-]{1,64}$",
        "description": "Model for the worker: sonnet for well-defined work, opus for design, risky changes or hard bugs."
    });
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
    )
}

fn get_task_details_tool() -> Value {
    tool(
        "get_task_details",
        "Full detail of one task: specification, dependencies, status, report (summary, commit, PR link, tests), note, worktree.",
        json!({ "id": { "type": "string", "description": "Id of the task." } }),
        &["id"],
    )
}

pub fn session_tools() -> Vec<Value> {
    let mut update = task_properties(false);
    update["id"] = json!({ "type": "string", "description": "Id of the task to change." });
    vec![
        propose_tasks_tool(),
        tool(
            "list_tasks",
            "List the task queue of this workspace: counts per status (queued, running, done, failed), pending proposals and one \
             compact row per task (id, title, status, kind and verdict for reviews, attention, waiting_on, summary, note). Never waits.",
            json!({
                "status": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["queued", "running", "done", "failed"] },
                    "description": "Only tasks in these statuses (default: all)."
                }
            }),
            &[],
        ),
        get_task_details_tool(),
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
        tool(
            "save_skill",
            "Create or update a global user skill, usable in every project (Windows, WSL) as /<name>. The app generates \
             SKILL.md and its frontmatter from your fields. Fails on a hand-written skill with that name. Returns {name, \
             invocation, store_path, synced: [{env, ok, error?}]}.",
            skill_properties(),
            &["name", "description", "body"],
        ),
        tool("list_skills", "List the skills saved with save_skill: [{name, description}].", json!({}), &[]),
        tool(
            "delete_skill",
            "Delete a skill saved with save_skill everywhere; hand-written ones are never touched.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "get_workspace_settings",
            "Read the workspace settings that propose_settings can change, the repositories with their configuration (null = \
             none), the allowed values, the limits and the pending settings proposals.",
            json!({}),
            &[],
        ),
        tool(
            "propose_settings",
            "Propose changes to the workspace settings. Only the fields you pass change. The user approves or discards the \
             proposal in the app; fields that run commands or widen permissions are shown to the user as sensitive. Nothing \
             changes before the decision and you get NO feedback after it: never wait. All-or-nothing validation. Returns \
             {proposal, status: \"pending\"}.",
            settings_properties(),
            &["reason"],
        ),
    ]
}

/// Campi di `propose_settings`; i limiti rispecchiano la validazione di
/// `crate::workspaces` e `crate::worker_permissions`.
fn settings_properties() -> Value {
    let rules = json!({
        "type": "array",
        "items": { "type": "string", "maxLength": MAX_RULE_CHARS },
        "maxItems": MAX_RULES,
        "description": "Replaces the list, e.g. Bash(npm test:*)."
    });
    let commands = json!({
        "type": "array",
        "items": { "type": "string", "maxLength": MAX_REPO_COMMAND_LEN },
        "maxItems": MAX_REPO_COMMANDS,
        "description": "Single-line commands in order; replaces the list."
    });
    json!({
        "reason": { "type": "string", "minLength": 1, "maxLength": REASON_MAX, "description": "Why, shown to the user." },
        "worker_rules": {
            "type": "string",
            "maxLength": MAX_RULES_LEN,
            "description": "Full text for the workers; empty clears."
        },
        "worker_model": {
            "type": ["string", "null"],
            "pattern": "^[A-Za-z0-9._-]{1,64}$",
            "description": "Null = agent default."
        },
        "worker_permission": { "type": "string", "enum": PermissionMode::ALL.map(PermissionMode::as_str) },
        "worker_allow": rules,
        "worker_deny": rules,
        "delivery": {
            "type": "object",
            "properties": {
                "mode": { "type": "string", "enum": DeliveryMode::ALL.map(DeliveryMode::as_str) },
                "parallel": { "type": "boolean" },
                "merge_method": { "type": "string", "enum": MergeMethod::ALL.map(MergeMethod::as_str) }
            },
            "additionalProperties": false,
            "description": "Omitted fields stay."
        },
        "use_schedule": { "type": "boolean" },
        "default_launch": {
            "type": ["string", "null"],
            "description": "\"shell\", a launch id or a builtin id; null = inherit."
        },
        "launches": {
            "type": "array",
            "maxItems": MAX_PATCH_LAUNCHES,
            "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string", "maxLength": MAX_LAUNCH_NAME_LEN },
                    "command": { "type": "string", "maxLength": MAX_COMMAND_LEN }
                },
                "required": ["name", "command"],
                "additionalProperties": false
            },
            "description": "Full list replacing the current one; keep the id of existing launches."
        },
        "repos": {
            "type": "object",
            "maxProperties": MAX_PATCH_REPOS,
            "additionalProperties": {
                "type": ["object", "null"],
                "properties": {
                    "base_branch": { "type": ["string", "null"] },
                    "container_root": { "type": ["string", "null"] },
                    "copy_ignored": { "type": "array", "items": { "type": "string" }, "maxItems": MAX_REPO_COMMANDS },
                    "setup": commands,
                    "cleanup": commands,
                    "remove_force": { "type": "boolean" }
                },
                "additionalProperties": false
            },
            "description": "Key = repository folder ('.' = root); null removes its config."
        }
    })
}

/// Campi di `save_skill`; i limiti rispecchiano `user_skills::validate`.
fn skill_properties() -> Value {
    json!({
        "name": { "type": "string", "pattern": "^[a-z0-9][a-z0-9-]{0,39}$", "description": "Slug, 1..40 chars." },
        "description": {
            "type": "string",
            "minLength": 1,
            "maxLength": 1024,
            "description": "One line: what the skill does and when to use it."
        },
        "body": { "type": "string", "minLength": 1, "maxLength": 102400, "description": "Markdown instructions, no frontmatter." },
        "files": {
            "type": "array",
            "maxItems": 20,
            "items": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative, '/' separated." },
                    "content": { "type": "string", "maxLength": 204800 }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            },
            "description": "Extra text files."
        }
    })
}

/// Campo `tests` di `complete_task` (worker e revisore).
fn tests_property() -> Value {
    json!({
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
    })
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
                "tests": tests_property(),
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

/// Tool del revisore: i due del worker (con `get_task` e `complete_task` propri)
/// piu' quelli di sessione che gli servono.
pub fn reviewer_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your review task, the reviewer instructions, the environment and \
             `reviewed_tasks` (the reports of the tasks under review).",
            json!({}),
            &[],
        ),
        tool(
            "complete_task",
            "ALWAYS call it at the end: completed with a verdict once the review is done (after propose_tasks for the fixes, if \
             any), failed if the review could not be done. The tab closes by itself afterwards.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "completed = review done; failed = could not review (say why in summary)."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "What you checked and what you found."
                },
                "verdict": {
                    "type": "string",
                    "enum": ["passed", "issues"],
                    "description": "Required with completed."
                },
                "findings": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 2000 },
                    "maxItems": 100,
                    "description": "The problems found: required (non-empty) with issues; suggestions are allowed with passed."
                },
                "tests": tests_property()
            }),
            &["status", "summary"],
        ),
        get_task_details_tool(),
        propose_tasks_tool(),
    ]
}

pub fn tools_for(role: Role) -> Vec<Value> {
    match role {
        Role::Session => session_tools(),
        Role::Worker => worker_tools(),
        Role::Reviewer => reviewer_tools(),
    }
}

/// Istruzioni generali restituite in `initialize`.
pub fn instructions(role: Option<Role>, workspace: Option<&str>) -> String {
    match role {
        Some(Role::Session) => {
            let mut text = "cli-companion: you may propose tasks for this workspace's queue with propose_tasks. The user \
                accepts or discards them in the app and automated workers deliver them on their own. Never wait for workers \
                or for the decision (no sleep, polling or repeated calls): end your turn. list_tasks, get_task_details, \
                update_task and delete_task inspect and fix the queue. Write task texts in the language of the conversation. \
                save_skill, list_skills and delete_skill manage the user's global skills. get_workspace_settings and \
                propose_settings read and propose changes to the workspace settings; the user approves them in the app."
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
        Some(Role::Reviewer) => "cli-companion: you are an automated reviewer. Call get_task first and follow its instructions: verify \
             the integrated result of the reviewed tasks without changing files, propose fix tasks with propose_tasks when needed \
             and ALWAYS finish with complete_task and a verdict. If you need a decision from the user, ask in plain text and end \
             your turn."
            .to_string(),
        None => "cli-companion: tools are only available in cli-companion terminals with the app running.".to_string(),
    }
}
