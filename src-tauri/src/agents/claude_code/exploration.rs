//! Contratto `ExplorationDelegation` di Claude Code: il sotto-agente
//! `explorer` (su Haiku, solo lettura) distribuito con il plugin.

use super::{ClaudeCode, EXPLORER_AGENT, PLUGIN};
use crate::agents::ExplorationDelegation;

impl ExplorationDelegation for ClaudeCode {
    fn explorer_invocation(&self) -> String {
        format!("call the Agent tool with subagent_type `{PLUGIN}:{EXPLORER_AGENT}`")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_explorer_is_started_through_the_agent_tool() {
        assert_eq!(
            ClaudeCode.explorer_invocation(),
            "call the Agent tool with subagent_type `cli-companion-notify:explorer`"
        );
    }
}
