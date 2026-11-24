//! Definizione dei tool MCP e delle istruzioni del server (le legge il
//! modello: inglese, concise). Quattro insiemi: i tool del worker di un task, quelli
//! del revisore (il worker di un task `review`), del pianificatore (task `plan`,
//! in `planner`) e quelli di una sessione normale (Intake compreso). Quali metodi
//! spettano a ogni ruolo lo decide la tabella di
//! `crate::automation::roles`: un test verifica che i tool coincidano.

use crate::agents::{registry, tier_models, PermissionMode, TierModels};
use crate::automation::settings::REASON_MAX;
use crate::automation::{Method, Role};
use crate::board::{OPTIONS_MAX, OPTION_MAX, QUESTION_MAX};
use crate::workspaces::{
    DeliveryMode, MergeMethod, MAX_COMMAND_LEN, MAX_E2E_TIMEOUT_MIN, MAX_LAUNCH_NAME_LEN, MAX_PATCH_AGENTS,
    MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS, MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_REPO_PATH_LEN, MAX_RULES_LEN,
    MIN_E2E_TIMEOUT_MIN,
};
use serde_json::{json, Value};

mod planner;

use planner::planner_instructions;
pub use planner::planner_tools;

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

/// Descrizione del campo `model` di un task. I nomi vengono dall'agente del task
/// (qui quelli del predefinito, come esempio).
pub(super) fn model_description(models: Option<TierModels>) -> String {
    match models {
        Some(TierModels { standard, strong }) => format!(
            "Model of the task's agent (e.g. {standard} for well-defined work, {strong} for design or risk). \
             Omit for the workspace default."
        ),
        None => "Model of the task's agent, as that agent names it. Omit for the workspace default.".to_string(),
    }
}

/// Descrizione del campo `model` di `propose_tasks`: li' e' un override facoltativo
/// del modello del Bot.
pub(super) fn proposed_model_description(models: Option<TierModels>) -> String {
    match models {
        Some(TierModels { standard, strong }) => format!(
            "Optional Bot model override (ids: get_workspace_settings.agents[].models): {standard} \
             for well-defined work, {strong} for design, risky."
        ),
        None => {
            "Optional Bot model override, as its agent names it (get_workspace_settings.agents[].models).".to_string()
        }
    }
}

/// Descrizione del campo `bot` di un task (obbligatorio in `propose_tasks`).
const BOT_DESCRIPTION: &str = "Bot id (get_workspace_settings.bots) with the task kind in `kinds`, by `task`; \
    `general` only for work.";

/// Campi di un task proposto (`propose_tasks`) o modificato (`update_task`).
fn task_properties(with_id: bool) -> Value {
    let mut props = json!({
        "title": {
            "type": "string",
            "minLength": 1,
            "maxLength": 80,
            "description": "Short title."
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
            "description": "Ids that must be done first (same proposal or queue). No cycles."
        },
        "repo": {
            "type": ["string", "null"],
            "maxLength": 256,
            "description": "Multi-repo workspaces: repository folder relative to the root; omit or '.' for the root."
        },
        "agent": {
            "type": ["string", "null"],
            "description": "Agent id (get_workspace_settings.agents); omit for the workspace agent."
        },
        "bot": {
            "type": ["string", "null"],
            "description": "Bot id from get_workspace_settings.bots; null = none."
        },
        "model": {
            "type": ["string", "null"],
            "pattern": crate::validate::MODEL_SHAPE_PATTERN,
            "description": model_description(tier_models(registry::default_agent()))
        },
        "kind": {
            "type": "string",
            "enum": ["work", "review"],
            "description": "review = a reviewer checks the integrated result of its depends_on (at least one). Default work."
        },
        "e2e": {
            "type": "boolean",
            "description": "Must pass the repo E2E tests; needs the repo E2E config. Default false."
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
        "type": ["string", "null"],
        "pattern": crate::validate::MODEL_SHAPE_PATTERN,
        "description": proposed_model_description(tier_models(registry::default_agent()))
    });
    proposed["bot"] = json!({ "type": "string", "description": BOT_DESCRIPTION });
    proposed["agent"] = json!({ "type": "string", "description": "Must equal the Bot's agent: omit." });
    tool(
        "propose_tasks",
        "Propose tasks for this workspace's queue. The user accepts or discards it in the app; nothing starts \
         before and you get no answer: never wait. All-or-nothing validation: unique ids (also vs queue and \
         proposals), valid dependencies, no cycles, existing repo. Returns {proposal, status: \"pending\"}.",
        json!({
            "tasks": {
                "type": "array",
                "minItems": 1,
                "maxItems": 20,
                "items": {
                    "type": "object",
                    "properties": proposed,
                    "required": ["id", "title", "task", "bot"],
                    "additionalProperties": false
                },
                "description": "In queue order."
            }
        }),
        &["tasks"],
    )
}

fn get_task_details_tool() -> Value {
    tool(
        "get_task_details",
        "Full detail of one task: specification, dependencies, status, report, note, worktree.",
        json!({ "id": { "type": "string", "description": "Id of the task." } }),
        &["id"],
    )
}

/// `get_workspace_settings` for the automated readers (planner, reviewer): read-only,
/// so it does not mention `propose_settings`.
fn read_settings_tool() -> Value {
    tool(
        "get_workspace_settings",
        "Read the Bots, the enabled agents (models, readiness) and the repositories (e2e: null = none).",
        json!({}),
        &[],
    )
}

fn list_tasks_tool() -> Value {
    tool(
        "list_tasks",
        "List the task queue: counts per status, pending proposals and one \
         compact row per task (id, title, status, kind unless work, verdict for reviews, attention, waiting_on, summary, note, fail_reason, blocked_by). \
         fail_reason: worker_failed|worker_exited|stopped|interrupted|start_failed. \
         blocked_by: failed tasks it depends on.",
        json!({
            "status": {
                "type": "array",
                "items": { "type": "string", "enum": ["queued", "running", "done", "failed"] },
                "description": "Only tasks in these statuses (default: all)."
            }
        }),
        &[],
    )
}

pub fn session_tools() -> Vec<Value> {
    let mut update = task_properties(false);
    update["id"] = json!({ "type": "string", "description": "Id of the task to change." });
    vec![
        propose_tasks_tool(),
        list_tasks_tool(),
        get_task_details_tool(),
        tool(
            "update_task",
            "Change a queued or failed task: omitted fields stay, null clears repo, agent, model and bot (a new bot re-applies its settings). repo is fixed once \
             the task has a worktree. Returns the task without its text.",
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
            "Create or update a global user skill, usable in every project as /<name>. The app generates \
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
            "Read the workspace settings that propose_settings can change, the enabled agents (models, permission rule \
             syntax, readiness), the Bots (task, agent, model, effort, skill), the repositories with their configuration (null = none), the allowed values, the limits and the \
             pending settings proposals.",
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
/// `crate::workspaces`. Modelli e regole di permesso dipendono dall'agente:
/// li descrive `get_workspace_settings` in `agents`.
pub(super) fn settings_properties() -> Value {
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
        "agent": {
            "type": ["string", "null"],
            "description": "Worker agent of the workspace (id from get_workspace_settings.agents); null = the global default."
        },
        "worker_agents": {
            "type": "object",
            "maxProperties": MAX_PATCH_AGENTS,
            "additionalProperties": {
                "type": "object",
                "properties": {
                    "model": { "type": ["string", "null"], "description": "Null = agent default." },
                    "allow": { "type": "array", "items": { "type": "string" } },
                    "deny": { "type": "array", "items": { "type": "string" } }
                },
                "additionalProperties": false
            },
            "description": "Key = agent id. allow/deny replace the list. Models and rule syntax: get_workspace_settings.agents."
        },
        "worker_permission": { "type": "string", "enum": PermissionMode::ALL.map(PermissionMode::as_str) },
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
            "description": "\"shell\", a launch id, a builtin id or bot:<id> (see allowed.default_launch); null = inherit."
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
                    "remove_force": { "type": "boolean" },
                    "e2e": {
                        "type": ["object", "null"],
                        "properties": {
                            "command": { "type": "string", "maxLength": MAX_REPO_COMMAND_LEN },
                            "junit": { "type": "string", "maxLength": MAX_REPO_PATH_LEN },
                            "timeout_min": { "type": "integer", "minimum": MIN_E2E_TIMEOUT_MIN, "maximum": MAX_E2E_TIMEOUT_MIN }
                        },
                        "required": ["command", "junit"],
                        "additionalProperties": false
                    }
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

/// Attesa massima (secondi) di `get_e2e_result`: il server dell'app valida lo stesso limite.
pub(crate) const E2E_MAX_WAIT_SECONDS: u64 = 25;

/// `run_e2e` e `get_e2e_result`: eseguiti dall'app, per worker e revisore.
fn e2e_tools() -> [Value; 2] {
    [
        tool(
            "run_e2e",
            "Run the repository's configured E2E command through the app, in your working folder, and read its JUnit \
             report. Returns at once {run_id, status: \"running\"}; then call get_e2e_result. One run at a time per \
             folder. Fails without E2E configuration. Only the app's run counts for the E2E gate of complete_task.",
            json!({}),
            &[],
        ),
        tool(
            "get_e2e_result",
            "Result of the last E2E run of your task: status running, passed, failed or error; totals, failed tests \
             (file, message), duration, commit, JUnit path and, on error, the output tail. While running, call it again \
             with wait_seconds.",
            json!({
                "run_id": {
                    "type": "string",
                    "maxLength": 64,
                    "description": "Run to read; default the last run of your task."
                },
                "wait_seconds": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": E2E_MAX_WAIT_SECONDS,
                    "description": "Wait up to this many seconds for a running run to finish (default 0)."
                }
            }),
            &[],
        ),
    ]
}

pub fn worker_tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your task, the working instructions, the environment, the delivery \
             steps and, after a retry, `previous_attempt`. Also `dependencies` (reports of the tasks you \
             depend on), when present.",
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
    ];
    tools.extend(e2e_tools());
    tools
}

/// Tool del revisore: i due del worker (con `get_task` e `complete_task` propri)
/// piu' quelli di sessione che gli servono (anche la lettura delle impostazioni,
/// per scegliere Bot e modello dei task di correzione), piu' i due E2E.
pub fn reviewer_tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your review task, the reviewer instructions, the environment and \
             `reviewed_tasks` (the reports of the tasks under review).",
            json!({}),
            &[],
        ),
        tool(
            "complete_task",
            "ALWAYS call it last: completed with a verdict once the review is done (after propose_tasks for the fixes, if \
             any), failed if it could not be done. The tab then closes.",
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
        read_settings_tool(),
    ];
    tools.extend(e2e_tools());
    tools
}

/// `ask_user`: condiviso da worker e pianificatore, esiste solo con l'Agents HUB.
fn ask_user_tool() -> Value {
    tool(
        "ask_user",
        "Ask the user one blocking question through the app, only when the code cannot settle the doubt. One at a \
         time: a second question is refused until the first is answered. Returns at once; then end your turn: the \
         answer arrives as the user's next message in this terminal.",
        json!({
            "question": {
                "type": "string",
                "minLength": 1,
                "maxLength": QUESTION_MAX,
                "description": "One clear question, with the context needed to answer it."
            },
            "options": {
                "type": "array",
                "maxItems": OPTIONS_MAX,
                "items": { "type": "string", "minLength": 1, "maxLength": OPTION_MAX },
                "description": "Short choices the user can pick, when known."
            }
        }),
        &["question"],
    )
}

/// I tool del ruolo. I metodi che richiedono l'Agents HUB (`Method::requires_hub`)
/// compaiono solo con `hub` e se la tabella dei ruoli li concede; `ask_user`
/// segue `complete_task`.
pub fn tools_for(role: Role, hub: bool) -> Vec<Value> {
    let mut tools = match role {
        Role::Session => session_tools(),
        Role::Worker => worker_tools(),
        Role::Reviewer => reviewer_tools(),
        Role::Planner => planner_tools(),
    };
    if hub && Method::AskUser.requires_hub() && Method::AskUser.allows(role) {
        let after = tools.iter().position(|t| t["name"] == "complete_task").map_or(0, |i| i + 1);
        tools.insert(after, ask_user_tool());
    }
    tools
}

/// Istruzioni generali restituite in `initialize`.
pub fn instructions(role: Option<Role>, workspace: Option<&str>, hub: bool) -> String {
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
        Some(Role::Worker) => {
            let ask = if hub {
                "If a doubt blocks you, call ask_user and end your turn: the answer arrives in your terminal."
            } else {
                "If you need a decision from the user, ask in plain text and end your turn: the user answers in your \
                 terminal."
            };
            format!(
                "cli-companion: you are an automated worker. Call get_task first, deliver the work as the delivery steps \
                 say and ALWAYS finish with complete_task. {ask}"
            )
        }
        Some(Role::Reviewer) => "cli-companion: you are an automated reviewer. Call get_task first and follow its instructions: verify \
             the integrated result of the reviewed tasks without changing files, propose fix tasks with propose_tasks when needed \
             and ALWAYS finish with complete_task and a verdict. If you need a decision from the user, ask in plain text and end \
             your turn."
            .to_string(),
        Some(Role::Planner) => planner_instructions(hub).to_string(),
        None => "cli-companion: tools are only available in cli-companion terminals with the app running.".to_string(),
    }
}
