//! Contratto `ProxyRouting` di Pi. Pi non legge variabili d'ambiente per
//! l'endpoint dei provider (solo il `baseUrl` del provider, da `models.json` o da
//! un'estensione): l'indirizzo del proxy arriva all'estensione `cli-companion-notify`,
//! che a `session_start` ridireziona i provider (`pi.registerProvider(nome, { baseUrl })`)
//! senza toccare la configurazione dell'utente. Vedi `extension.ts`.
//!
//! I nomi non possono iniziare con `CLI_COMPANION_` (prefisso riservato al core).

use super::PiAgent;
use crate::agents::{ProxyRouting, ProxyTarget};

/// Base URL del proxy, letta dall'estensione.
const PROXY_URL_VAR: &str = "COMPANION_PROXY_URL";
/// Upstream OpenAI del proxy: i provider OpenAI con questo `baseUrl` vengono ridirezionati.
const PROXY_UPSTREAM_VAR: &str = "COMPANION_PROXY_OPENAI_UPSTREAM";

impl ProxyRouting for PiAgent {
    fn proxy_env(&self, proxy: &ProxyTarget) -> Option<Vec<(String, String)>> {
        Some(vec![
            (PROXY_URL_VAR.to_string(), proxy.base_url.clone()),
            (PROXY_UPSTREAM_VAR.to_string(), proxy.openai_upstream.clone()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extension_variables_carry_the_proxy_and_its_upstream() {
        let target = ProxyTarget {
            base_url: "http://127.0.0.1:9100".to_string(),
            openai_upstream: "https://api.deepseek.com".to_string(),
        };
        assert_eq!(
            PiAgent.proxy_env(&target),
            Some(vec![
                ("COMPANION_PROXY_URL".to_string(), "http://127.0.0.1:9100".to_string()),
                ("COMPANION_PROXY_OPENAI_UPSTREAM".to_string(), "https://api.deepseek.com".to_string()),
            ])
        );
    }
}
