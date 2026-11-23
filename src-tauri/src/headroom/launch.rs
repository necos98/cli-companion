//! La decisione: una sessione che sta per partire passa dal proxy Headroom?

use super::health;
use super::settings::{self, HeadroomSettings};
use crate::agents::CLAUDE_CODE_ID;
use crate::net::env_override;
use crate::platform::TargetKind;

/// Variabile con cui Claude Code sceglie l'endpoint dell'API.
const BASE_URL_VAR: &str = "ANTHROPIC_BASE_URL";

/// Ambiente da aggiungere al lancio e avviso per chi lo legge (in inglese:
/// arriva anche ai modelli, attraverso gli avvisi dei worker).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeadroomLaunch {
    pub env: Vec<(String, String)>,
    pub warning: Option<String>,
}

impl HeadroomLaunch {
    fn warn(text: String) -> Self {
        Self { env: Vec::new(), warning: Some(text) }
    }
}

/// Ambiente Headroom per una sessione di `agent_id` su `kind`, con le
/// impostazioni correnti e l'ambiente reale dell'app. Bloccante se serve il
/// controllo di rete.
pub fn launch_env(agent_id: &str, kind: TargetKind) -> HeadroomLaunch {
    decide(&settings::current(), agent_id, kind, env_override(BASE_URL_VAR), health::is_reachable)
}

/// Parte pura di `launch_env`: `parent_base_url` e' il valore di
/// `ANTHROPIC_BASE_URL` nell'ambiente dell'app, `reachable` dice se il proxy
/// risponde sulla porta data.
fn decide(
    settings: &HeadroomSettings,
    agent_id: &str,
    kind: TargetKind,
    parent_base_url: Option<String>,
    reachable: impl FnOnce(u16) -> bool,
) -> HeadroomLaunch {
    if !settings.enabled || agent_id != CLAUDE_CODE_ID || kind != TargetKind::Windows {
        return HeadroomLaunch::default();
    }
    if parent_base_url.is_some() {
        return HeadroomLaunch::warn(format!(
            "Headroom is enabled but {BASE_URL_VAR} is already set; the session starts without the Headroom proxy."
        ));
    }
    let url = settings.base_url();
    if !reachable(settings.port) {
        return HeadroomLaunch::warn(format!("Headroom proxy not reachable at {url}: the session starts without it."));
    }
    HeadroomLaunch { env: vec![(BASE_URL_VAR.to_string(), url)], warning: None }
}

/// Aggiunge a `env` quello di `launch`, senza duplicare una variabile gia' presente.
pub fn merge_env(env: &mut Vec<(String, String)>, launch: &HeadroomLaunch) {
    for (name, value) in &launch.env {
        match env.iter_mut().find(|(n, _)| n == name) {
            Some(existing) => existing.1.clone_from(value),
            None => env.push((name.clone(), value.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ON: HeadroomSettings = HeadroomSettings { enabled: true, port: 8787 };

    fn run(s: &HeadroomSettings, agent: &str, kind: TargetKind, parent: Option<&str>, up: bool) -> HeadroomLaunch {
        decide(s, agent, kind, parent.map(String::from), |_| up)
    }

    #[test]
    fn disabled_does_nothing() {
        let off = HeadroomSettings::default();
        assert_eq!(run(&off, CLAUDE_CODE_ID, TargetKind::Windows, None, true), HeadroomLaunch::default());
        assert_eq!(run(&off, CLAUDE_CODE_ID, TargetKind::Windows, Some("x"), false), HeadroomLaunch::default());
    }

    #[test]
    fn other_agents_and_wsl_are_untouched_without_warning() {
        assert_eq!(run(&ON, "opencode", TargetKind::Windows, None, true), HeadroomLaunch::default());
        assert_eq!(run(&ON, CLAUDE_CODE_ID, TargetKind::Wsl, None, false), HeadroomLaunch::default());
    }

    #[test]
    fn inherited_base_url_wins_with_a_warning() {
        let out = run(&ON, CLAUDE_CODE_ID, TargetKind::Windows, Some("http://x"), true);
        assert!(out.env.is_empty());
        assert!(out.warning.unwrap().contains("ANTHROPIC_BASE_URL is already set"));
    }

    #[test]
    fn unreachable_proxy_warns_with_the_url() {
        let out = run(&ON, CLAUDE_CODE_ID, TargetKind::Windows, None, false);
        assert!(out.env.is_empty());
        assert_eq!(
            out.warning.unwrap(),
            "Headroom proxy not reachable at http://127.0.0.1:8787: the session starts without it."
        );
    }

    #[test]
    fn reachable_proxy_sets_the_base_url_on_the_configured_port() {
        let custom = HeadroomSettings { enabled: true, port: 9100 };
        let seen = std::cell::Cell::new(0);
        let out = decide(&custom, CLAUDE_CODE_ID, TargetKind::Windows, None, |p| {
            seen.set(p);
            true
        });
        assert_eq!(seen.get(), 9100);
        assert_eq!(out.env, vec![("ANTHROPIC_BASE_URL".to_string(), "http://127.0.0.1:9100".to_string())]);
        assert_eq!(out.warning, None);
    }

    #[test]
    fn default_settings_leave_the_launch_unchanged() {
        assert_eq!(launch_env(CLAUDE_CODE_ID, TargetKind::Windows), HeadroomLaunch::default());
    }

    #[test]
    fn merge_env_never_duplicates_a_variable() {
        let launch = HeadroomLaunch { env: vec![("A".into(), "new".into())], warning: None };
        let mut env = vec![("A".to_string(), "old".to_string()), ("B".to_string(), "b".to_string())];
        merge_env(&mut env, &launch);
        merge_env(&mut env, &launch);
        assert_eq!(env, vec![("A".to_string(), "new".to_string()), ("B".to_string(), "b".to_string())]);
    }
}
