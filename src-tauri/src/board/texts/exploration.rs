//! Il paragrafo di `get_task` sulla delega dell'esplorazione a un sotto-agente economico.

/// Il paragrafo, con `{invocation}` sostituito da come l'agente del worker avvia
/// l'esploratore (vedi `agents::ExplorationDelegation`).
pub fn exploration_delegation(invocation: &str) -> String {
    EXPLORATION_DELEGATION.trim_end().replace("{invocation}", invocation)
}

const EXPLORATION_DELEGATION: &str = include_str!("../prompts/exploration_delegation.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_invocation_replaces_the_placeholder() {
        let text = exploration_delegation("call the Agent tool with subagent_type `x:explorer`");
        assert!(text.starts_with("Exploration delegation."), "{text}");
        assert!(
            text.contains("to delegate, call the Agent tool with subagent_type `x:explorer`. Use it for"),
            "{text}"
        );
        assert!(!text.contains("{invocation}") && !text.ends_with('\n'));
    }
}
