//! Tool e istruzioni di chi consegna un goal (il worker di un task `deliver`):
//! `get_task` e `complete_task` propri (il rapporto porta i commit e i link delle
//! PR, uno per repository), la lettura del goal e dei suoi task e i due E2E.
//! Niente proposte, niente modifiche della coda.

use super::{e2e_tools, get_goal_tool, get_task_details_tool, tool};
use crate::board::DELIVERED_MAX;
use serde_json::{json, Value};

/// Istruzioni generali del server per chi consegna (Agents HUB spento).
const DELIVERER_INSTRUCTIONS: &str = "cli-companion: you are an automated deliverer. Call get_task first, bring the goal \
    to its base as the delivery steps say and ALWAYS finish with complete_task. If you need a decision from the user, ask \
    in plain text and end your turn: the user answers in your terminal.";

/// Le stesse con l'Agents HUB acceso: un dubbio si chiede con `ask_user`.
const DELIVERER_INSTRUCTIONS_HUB: &str = "cli-companion: you are an automated deliverer. Call get_task first, bring the \
    goal to its base as the delivery steps say and ALWAYS finish with complete_task. If a doubt blocks you, call ask_user \
    and end your turn: the answer arrives in your terminal.";

pub(super) fn deliverer_instructions(hub: bool) -> &'static str {
    if hub {
        DELIVERER_INSTRUCTIONS_HUB
    } else {
        DELIVERER_INSTRUCTIONS
    }
}

pub fn deliverer_tools() -> Vec<Value> {
    let mut tools = vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns your delivery task, the deliverer instructions, the environment, \
             the delivery steps, `goal` (title, request, done_when, decisions) and `goal_tasks` (reports of the goal's \
             tasks) and, after a retry, `previous_attempt`.",
            json!({}),
            &[],
        ),
        tool(
            "complete_task",
            "ALWAYS call it last: completed once the goal is on its base as the delivery steps say, failed if you cannot \
             deliver it (blocked is accepted as failed). The tab closes by itself afterwards.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "completed = delivered; failed = could not deliver (say why in summary)."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "What was delivered, for the user."
                },
                "commits": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 100 },
                    "maxItems": DELIVERED_MAX,
                    "description": "Final commit hash, one per repository."
                },
                "pr_urls": {
                    "type": "array",
                    "items": { "type": "string", "maxLength": 500 },
                    "maxItems": DELIVERED_MAX,
                    "description": "Links of the pull requests (PR mode), one per repository."
                }
            }),
            &["status", "summary"],
        ),
        get_task_details_tool(),
        get_goal_tool(),
    ];
    tools.extend(e2e_tools());
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::Role;
    use crate::mcp::tools::{instructions, tools_for};

    /// The deliverer set (measured 3_000): its own get_task and complete_task plus get_task_details,
    /// get_goal and the two E2E tools. Do not raise it to fit new text, shorten the text.
    const DELIVERER_TOOLS_CEILING: usize = 4_300;
    /// The deliverer server instructions (measured 300).
    const DELIVERER_INSTRUCTIONS_CEILING: usize = 400;

    fn tool_named(name: &str) -> Value {
        deliverer_tools().into_iter().find(|t| t["name"] == name).unwrap()
    }

    #[test]
    fn size_budget_of_the_deliverer_text() {
        let tools = serde_json::to_string(&deliverer_tools()).unwrap().chars().count();
        assert!(
            tools <= DELIVERER_TOOLS_CEILING,
            "deliverer tools are {tools} chars, ceiling {DELIVERER_TOOLS_CEILING}"
        );
        for hub in [false, true] {
            let text = instructions(Some(Role::Deliverer), None, hub);
            let n = text.chars().count();
            assert!(n <= DELIVERER_INSTRUCTIONS_CEILING, "deliverer instructions (hub {hub}) are {n} chars");
            assert!(!text.contains('\n') && !text.contains("  "), "one line with single spaces");
            assert!(text.contains("automated deliverer") && text.contains("complete_task"));
        }
        assert!(instructions(Some(Role::Deliverer), None, true).contains("call ask_user"));
        assert!(instructions(Some(Role::Deliverer), None, false).contains("ask in plain text"));
    }

    #[test]
    fn the_deliverer_reports_commits_and_pr_urls_and_gets_its_role_tools() {
        let complete = tool_named("complete_task");
        let props = complete["inputSchema"]["properties"].as_object().unwrap();
        let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["commits", "pr_urls", "status", "summary"]);
        assert_eq!(complete["inputSchema"]["required"], json!(["status", "summary"]));
        assert_eq!(props["commits"]["maxItems"], DELIVERED_MAX);
        assert!(tool_named("get_task")["description"].as_str().unwrap().contains("goal_tasks"));
        let names: Vec<String> =
            tools_for(Role::Deliverer, false).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["get_task", "complete_task", "get_task_details", "get_goal", "run_e2e", "get_e2e_result"]);
        let hub: Vec<String> =
            tools_for(Role::Deliverer, true).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(hub[2], "ask_user", "ask_user follows complete_task");
    }
}
