//! Contratto `ProxyRouting` di Claude Code: sceglie l'endpoint dell'API con
//! la variabile `ANTHROPIC_BASE_URL`.

use super::ClaudeCode;
use crate::agents::{ProxyRouting, ProxyTarget};

/// Variabile con cui Claude Code sceglie l'endpoint dell'API.
const BASE_URL_VAR: &str = "ANTHROPIC_BASE_URL";

impl ProxyRouting for ClaudeCode {
    fn proxy_env(&self, proxy: &ProxyTarget) -> Option<Vec<(String, String)>> {
        Some(vec![(BASE_URL_VAR.to_string(), proxy.base_url.clone())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_base_url_variable_points_at_the_proxy() {
        let target = ProxyTarget {
            base_url: "http://127.0.0.1:9100".to_string(),
            openai_upstream: "https://api.openai.com".to_string(),
        };
        assert_eq!(
            ClaudeCode.proxy_env(&target),
            Some(vec![("ANTHROPIC_BASE_URL".to_string(), "http://127.0.0.1:9100".to_string())])
        );
    }
}
