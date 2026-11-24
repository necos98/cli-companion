//! Contratto `EffortLevels` di Pi: il livello di ragionamento e' `--thinking`.

use super::PiAgent;
use crate::agents::{Effort, EffortLevels};

/// Flag di Pi per il livello di ragionamento.
const THINKING_FLAG: &str = "--thinking";

impl EffortLevels for PiAgent {
    /// Pi conosce anche `off` e `minimal`, che il vocabolario del core non ha.
    fn levels(&self) -> &'static [Effort] {
        &Effort::ALL
    }

    fn command_args(&self, effort: Effort) -> Option<String> {
        Some(format!("{THINKING_FLAG} {}", effort.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_every_level_from_the_lowest() {
        assert_eq!(PiAgent.levels(), [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh, Effort::Max]);
    }

    #[test]
    fn each_level_maps_to_the_thinking_flag() {
        let expected = [
            (Effort::Low, "--thinking low"),
            (Effort::Medium, "--thinking medium"),
            (Effort::High, "--thinking high"),
            (Effort::XHigh, "--thinking xhigh"),
            (Effort::Max, "--thinking max"),
        ];
        for (effort, args) in expected {
            assert_eq!(PiAgent.command_args(effort).as_deref(), Some(args));
        }
    }
}
