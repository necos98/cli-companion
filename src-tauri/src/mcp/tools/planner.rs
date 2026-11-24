//! Tool e istruzioni del pianificatore (il worker di un task `plan`): i due del
//! worker con `get_task` e `complete_task` propri, piu' la lettura della coda e
//! delle impostazioni e `propose_tasks`. Niente E2E, niente modifiche.

use super::{get_task_details_tool, list_tasks_tool, propose_tasks_tool, read_settings_tool, tool};
use serde_json::{json, Value};

/// Istruzioni generali del server per il pianificatore (Agents HUB spento).
const PLANNER_INSTRUCTIONS: &str = "cli-companion: you are an automated planner. Call get_task first and \
    follow its instructions: explore the repository without changing anything, propose the tasks with a single \
    propose_tasks call (or list your questions for the user in the summary) and ALWAYS finish with complete_task. \
    Nobody reads your terminal: never wait for an answer.";

/// Le stesse con l'Agents HUB acceso: un dubbio si chiede con `ask_user`.
const PLANNER_INSTRUCTIONS_HUB: &str = "cli-companion: you are an automated planner. Call get_task first and follow its \
    instructions: explore the repository without changing anything, propose the tasks with a single propose_tasks call \
    and ALWAYS finish with complete_task. If a doubt blocks the plan, call ask_user and end your turn: the answer arrives \
    in this terminal.";

pub(super) fn planner_instructions(hub: bool) -> &'static str {
    if hub {
        PLANNER_INSTRUCTIONS_HUB
    } else {
        PLANNER_INSTRUCTIONS
    }
}

pub fn planner_tools() -> Vec<Value> {
    vec![
        tool(
            "get_task",
            "FIRST call of every automated session: returns the request to plan, the planner instructions and the \
             environment.",
            json!({}),
            &[],
        ),
        tool(
            "complete_task",
            "ALWAYS call it last: completed after the proposal, or with your numbered questions in summary if the \
             request is too ambiguous; failed if planning failed. The tab closes itself.",
            json!({
                "status": {
                    "type": "string",
                    "enum": ["completed", "failed"],
                    "description": "completed = proposal made or questions asked; failed = could not plan (say why in summary)."
                },
                "summary": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 50000,
                    "description": "The proposed tasks and your assumptions, or the numbered questions for the user."
                }
            }),
            &["status", "summary"],
        ),
        propose_tasks_tool(),
        list_tasks_tool(),
        get_task_details_tool(),
        read_settings_tool(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::Role;
    use crate::mcp::tools::{instructions, tools_for};

    /// The planner set (measured 4_117): its own get_task and complete_task plus the shared
    /// propose_tasks, list_tasks, get_task_details and a shorter get_workspace_settings.
    /// Do not raise it to fit new text, shorten the text.
    const PLANNER_TOOLS_CEILING: usize = 4_150;
    /// The planner server instructions (measured 339).
    const PLANNER_INSTRUCTIONS_CEILING: usize = 400;
    /// The planner set with the Agents HUB on: the set above plus ask_user (measured 4_813).
    const PLANNER_HUB_TOOLS_CEILING: usize = 4_850;
    /// The planner server instructions with the Agents HUB on (measured 342).
    const PLANNER_HUB_INSTRUCTIONS_CEILING: usize = 400;

    fn tool_named(name: &str) -> Value {
        planner_tools().into_iter().find(|t| t["name"] == name).unwrap()
    }

    #[test]
    fn size_budget_of_the_planner_text() {
        let tools = serde_json::to_string(&planner_tools()).unwrap().chars().count();
        assert!(tools <= PLANNER_TOOLS_CEILING, "planner tools are {tools} chars, ceiling {PLANNER_TOOLS_CEILING}");
        let hub_tools = serde_json::to_string(&tools_for(Role::Planner, true)).unwrap().chars().count();
        assert!(
            hub_tools <= PLANNER_HUB_TOOLS_CEILING,
            "planner tools with the hub are {hub_tools} chars, ceiling {PLANNER_HUB_TOOLS_CEILING}"
        );
        for (hub, ceiling) in [(false, PLANNER_INSTRUCTIONS_CEILING), (true, PLANNER_HUB_INSTRUCTIONS_CEILING)] {
            let text = instructions(Some(Role::Planner), None, hub);
            let n = text.chars().count();
            assert!(n <= ceiling, "planner instructions (hub {hub}) are {n} chars, ceiling {ceiling}");
            assert!(!text.contains('\n') && !text.contains("  "), "one line with single spaces");
            assert!(
                text.contains("automated planner") && text.contains("propose_tasks") && text.contains("complete_task")
            );
        }
    }

    #[test]
    fn the_planner_reports_no_delivery_and_gets_its_role_tools() {
        let complete = tool_named("complete_task");
        let props = complete["inputSchema"]["properties"].as_object().unwrap();
        let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["status", "summary"]);
        assert!(tool_named("get_task")["description"].as_str().unwrap().contains("request to plan"));
        let settings = tool_named("get_workspace_settings");
        assert!(!settings["description"].as_str().unwrap().contains("propose_settings"));
        let names: Vec<String> =
            tools_for(Role::Planner, false).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(
            names,
            ["get_task", "complete_task", "propose_tasks", "list_tasks", "get_task_details", "get_workspace_settings"]
        );
    }
}
