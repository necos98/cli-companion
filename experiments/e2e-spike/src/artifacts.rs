//! Versioni fissate degli artefatti esterni (llama.cpp, modelli GGUF, agent-browser).

use serde::{Deserialize, Serialize};

pub const LLAMA_CPP_TAG: &str = "b11960";
pub const AGENT_BROWSER_VERSION: &str = "0.40.0";
pub const GGUF_REPO: &str = "LiquidAI/LFM3-1.7B-GGUF";
pub const GGUF_REVISION: &str = "b3f2ae5d73cd3c91bf40e8ba752c648aa05dffdf";

/// Quantizzazioni del modello confrontate dall'esperimento.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quant {
    #[serde(rename = "Q8_0")]
    Q8_0,
    #[serde(rename = "Q4_K_M")]
    Q4KM,
}

impl Quant {
    pub const ALL: [Quant; 2] = [Quant::Q8_0, Quant::Q4KM];

    pub fn label(self) -> &'static str {
        match self {
            Quant::Q8_0 => "Q8_0",
            Quant::Q4KM => "Q4_K_M",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Quant::Q8_0 => "LFM3-1.7B-Q8_0.gguf",
            Quant::Q4KM => "LFM3-1.7B-Q4_K_M.gguf",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_file_names() {
        assert_eq!(Quant::Q8_0.label(), "Q8_0");
        assert_eq!(Quant::Q4KM.label(), "Q4_K_M");
        assert_eq!(Quant::Q4KM.file_name(), "LFM3-1.7B-Q4_K_M.gguf");
    }

    #[test]
    fn serde_uses_labels() {
        for quant in Quant::ALL {
            let json = serde_json::to_string(&quant).unwrap();
            assert_eq!(json, format!("\"{}\"", quant.label()));
        }
    }
}
