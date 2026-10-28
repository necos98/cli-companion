//! Client HTTP del fixture (`/__state`, `/__reset`, `/__seed`).

use crate::fixture::state::{SeedAccount, StateSnapshot};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

pub struct FixtureClient {
    agent: ureq::Agent,
    base_url: String,
}

impl FixtureClient {
    pub fn new(base_url: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        FixtureClient {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn state(&self) -> Result<StateSnapshot, String> {
        self.agent
            .get(format!("{}/__state", self.base_url))
            .call()
            .map_err(|e| format!("GET /__state: {e}"))?
            .body_mut()
            .read_json::<StateSnapshot>()
            .map_err(|e| format!("GET /__state: risposta non valida: {e}"))
    }

    pub fn reset(&self) -> Result<(), String> {
        self.agent
            .post(format!("{}/__reset", self.base_url))
            .send_empty()
            .map(drop)
            .map_err(|e| format!("POST /__reset: {e}"))
    }

    pub fn seed(&self, account: &SeedAccount) -> Result<(), String> {
        self.agent
            .post(format!("{}/__seed", self.base_url))
            .send_json(account)
            .map(drop)
            .map_err(|e| format!("POST /__seed: {e}"))
    }
}
