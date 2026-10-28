//! Configurazioni dell'esperimento: variazione di un fattore alla volta rispetto a `base`.

use crate::agent::AgentConfig;
pub use crate::agent::{ActionMode, SnapshotMode};
use crate::artifacts::Quant;

const DEFAULT_MAX_ACTIONS: u32 = 15;
const TEMPERATURE: f32 = 0.1;
const TOP_K: u32 = 50;
const REPEAT_PENALTY: f32 = 1.1;
const MAX_TOKENS_PLAIN: u32 = 512;
/// Con il ragionamento attivo servono piu' token (visti ~1000 di ragionamento).
const MAX_TOKENS_THINKING: u32 = 4096;
const PAGE_TEXT_MAX_CHARS: usize = 3000;

#[derive(Clone, Debug)]
pub struct RunConfig {
    pub id: &'static str,
    pub quant: Quant,
    pub thinking: bool,
    pub action_mode: ActionMode,
    pub snapshot_mode: SnapshotMode,
    pub max_actions_per_step: u32,
    pub temperature: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub max_tokens: u32,
    pub page_text_max_chars: usize,
}

impl RunConfig {
    fn base() -> Self {
        RunConfig {
            id: "base",
            quant: Quant::Q8_0,
            thinking: false,
            action_mode: ActionMode::Schema,
            snapshot_mode: SnapshotMode::Full,
            max_actions_per_step: DEFAULT_MAX_ACTIONS,
            temperature: TEMPERATURE,
            top_k: TOP_K,
            repeat_penalty: REPEAT_PENALTY,
            max_tokens: MAX_TOKENS_PLAIN,
            page_text_max_chars: PAGE_TEXT_MAX_CHARS,
        }
    }

    /// Parametri dell'agente; `seed` e' per-esecuzione.
    pub fn agent_config(&self, base_url: &str, seed: u64) -> AgentConfig {
        AgentConfig {
            action_mode: self.action_mode,
            snapshot_mode: self.snapshot_mode,
            thinking: self.thinking,
            max_actions: self.max_actions_per_step,
            temperature: self.temperature,
            top_k: self.top_k,
            repeat_penalty: self.repeat_penalty,
            max_tokens: self.max_tokens,
            page_text_max_chars: self.page_text_max_chars,
            base_url: base_url.to_string(),
            seed: Some(seed),
        }
    }
}

pub fn matrix() -> Vec<RunConfig> {
    vec![
        RunConfig::base(),
        RunConfig {
            id: "q4_k_m",
            quant: Quant::Q4KM,
            ..RunConfig::base()
        },
        RunConfig {
            id: "thinking",
            thinking: true,
            max_tokens: MAX_TOKENS_THINKING,
            ..RunConfig::base()
        },
        RunConfig {
            id: "native",
            action_mode: ActionMode::Native,
            ..RunConfig::base()
        },
        RunConfig {
            id: "delta",
            snapshot_mode: SnapshotMode::Delta,
            ..RunConfig::base()
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_changes_one_factor_at_a_time() {
        let ids: Vec<&str> = matrix().iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["base", "q4_k_m", "thinking", "native", "delta"]);
        let configs = matrix();
        assert_eq!(configs[1].quant, Quant::Q4KM);
        assert!(configs[2].thinking && configs[2].max_tokens == 4096);
        assert_eq!(configs[3].action_mode, ActionMode::Native);
        assert_eq!(configs[4].snapshot_mode, SnapshotMode::Delta);
        assert_eq!(configs.iter().filter(|c| c.quant == Quant::Q4KM).count(), 1);
    }

    #[test]
    fn base_matches_the_specification() {
        let base = &matrix()[0];
        assert_eq!(base.quant, Quant::Q8_0);
        assert!(!base.thinking);
        assert_eq!(base.max_tokens, 512);
        assert_eq!(base.max_actions_per_step, 15);
    }

    #[test]
    fn agent_config_carries_the_seed() {
        let agent = matrix()[0].agent_config("http://127.0.0.1:1", 1003);
        assert_eq!(agent.seed, Some(1003));
        assert_eq!(agent.base_url, "http://127.0.0.1:1");
    }
}
