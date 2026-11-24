//! La decisione: una sessione che sta per partire passa dal proxy Headroom?

use super::settings::{self, HeadroomSettings};
use super::{health, process};
use crate::agents::{registry, ProxyRouting, ProxyTarget};
use crate::net::env_override;
use crate::platform::TargetKind;

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

/// Quanto puo' aspettare chi lancia la sessione se il proxy non risponde.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Patience {
    /// Attende l'avvio del proxy (worker, fuori dal thread della UI).
    Wait,
    /// Non attende: avvia il proxy in background e parte senza (terminali interattivi).
    NoWait,
}

/// Esito del tentativo di avere un proxy che risponde.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ensure {
    Ready,
    /// Avviato in background, non risponde ancora.
    Starting,
    Failed(String),
}

/// Ambiente Headroom per una sessione di `agent_id` su `kind`, con le
/// impostazioni correnti e l'ambiente reale dell'app. Bloccante se serve il
/// controllo di rete. Un agente senza il contratto di instradamento parte diretto.
pub fn launch_env(agent_id: &str, kind: TargetKind, patience: Patience) -> HeadroomLaunch {
    let routing = registry::find(agent_id).ok().and_then(|integration| integration.capabilities().proxy_routing);
    decide(&settings::current(), kind, routing, |name| env_override(name).is_some(), |s| ensure_proxy(s, patience))
}

/// Il proxy risponde, o con `auto_start` l'app lo avvia; senza, l'errore e' vuoto.
fn ensure_proxy(settings: &HeadroomSettings, patience: Patience) -> Ensure {
    if health::is_reachable(settings.port) {
        return Ensure::Ready;
    }
    if !settings.auto_start {
        return Ensure::Failed(String::new());
    }
    match patience {
        Patience::Wait => match process::ensure_running(settings) {
            Ok(()) => Ensure::Ready,
            Err(e) => Ensure::Failed(e),
        },
        Patience::NoWait => {
            process::start_nonblocking(settings);
            Ensure::Starting
        }
    }
}

/// Parte pura di `launch_env`: `routing` e' il contratto dell'agente (`None` =
/// non instradabile), `is_set` dice se una variabile e' gia' impostata
/// nell'ambiente dell'app, `ensure` dice se il proxy risponde (avviandolo se
/// serve), se sta partendo o perche' non va.
fn decide(
    settings: &HeadroomSettings,
    kind: TargetKind,
    routing: Option<&dyn ProxyRouting>,
    is_set: impl Fn(&str) -> bool,
    ensure: impl FnOnce(&HeadroomSettings) -> Ensure,
) -> HeadroomLaunch {
    if !settings.enabled || kind != TargetKind::Windows {
        return HeadroomLaunch::default();
    }
    let url = settings.base_url();
    let target = ProxyTarget { base_url: url.clone(), openai_upstream: settings.effective_openai_upstream() };
    let Some(env) = routing.and_then(|r| r.proxy_env(&target)) else {
        return HeadroomLaunch::default();
    };
    if let Some((name, _)) = env.iter().find(|(name, _)| is_set(name)) {
        return HeadroomLaunch::warn(format!(
            "Headroom is enabled but {name} is already set; the session starts without the Headroom proxy."
        ));
    }
    match ensure(settings) {
        Ensure::Ready => {}
        Ensure::Starting => {
            return HeadroomLaunch::warn(format!(
                "Headroom proxy is starting at {url}: this session starts without it; new sessions will use it once it answers."
            ))
        }
        Ensure::Failed(err) => return HeadroomLaunch::warn(unreachable_warning(settings, &url, &err)),
    }
    HeadroomLaunch { env, warning: None }
}

fn unreachable_warning(settings: &HeadroomSettings, url: &str, err: &str) -> String {
    if settings.auto_start {
        format!("Headroom proxy not reachable at {url} ({err}): the session starts without it.")
    } else {
        format!("Headroom proxy not reachable at {url}: the session starts without it.")
    }
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

    fn on() -> HeadroomSettings {
        HeadroomSettings { enabled: true, ..HeadroomSettings::default() }
    }

    fn no_auto() -> HeadroomSettings {
        HeadroomSettings { auto_start: false, ..on() }
    }

    /// Agente finto: due variabili, la prima con il base URL, la seconda con l'upstream.
    struct Routed;

    impl ProxyRouting for Routed {
        fn proxy_env(&self, proxy: &ProxyTarget) -> Option<Vec<(String, String)>> {
            Some(vec![
                ("PROXY_URL".to_string(), proxy.base_url.clone()),
                ("PROXY_UPSTREAM".to_string(), proxy.openai_upstream.clone()),
            ])
        }
    }

    /// Agente che dichiara il contratto ma non e' instradabile.
    struct Unroutable;

    impl ProxyRouting for Unroutable {
        fn proxy_env(&self, _: &ProxyTarget) -> Option<Vec<(String, String)>> {
            None
        }
    }

    fn run(
        s: &HeadroomSettings,
        kind: TargetKind,
        routing: Option<&dyn ProxyRouting>,
        set: &[&str],
        up: bool,
    ) -> HeadroomLaunch {
        decide(
            s,
            kind,
            routing,
            |name| set.contains(&name),
            |_| if up { Ensure::Ready } else { Ensure::Failed("boom".into()) },
        )
    }

    #[test]
    fn disabled_does_nothing() {
        let off = HeadroomSettings::default();
        assert_eq!(run(&off, TargetKind::Windows, Some(&Routed), &[], true), HeadroomLaunch::default());
        assert_eq!(run(&off, TargetKind::Windows, Some(&Routed), &["PROXY_URL"], false), HeadroomLaunch::default());
    }

    #[test]
    fn agents_without_routing_and_wsl_are_untouched_without_warning() {
        assert_eq!(run(&on(), TargetKind::Windows, None, &[], true), HeadroomLaunch::default());
        assert_eq!(run(&on(), TargetKind::Windows, None, &["PROXY_URL"], false), HeadroomLaunch::default());
        assert_eq!(
            run(&on(), TargetKind::Windows, Some(&Unroutable), &["PROXY_URL"], false),
            HeadroomLaunch::default()
        );
        assert_eq!(run(&on(), TargetKind::Wsl, Some(&Routed), &[], false), HeadroomLaunch::default());
    }

    #[test]
    fn any_inherited_variable_wins_with_a_warning_naming_it() {
        for name in ["PROXY_URL", "PROXY_UPSTREAM"] {
            let out = run(&on(), TargetKind::Windows, Some(&Routed), &[name], true);
            assert!(out.env.is_empty());
            assert_eq!(
                out.warning.unwrap(),
                format!(
                    "Headroom is enabled but {name} is already set; the session starts without the Headroom proxy."
                )
            );
        }
    }

    #[test]
    fn failed_auto_start_warns_with_the_url_and_the_error() {
        let out = run(&on(), TargetKind::Windows, Some(&Routed), &[], false);
        assert!(out.env.is_empty());
        assert_eq!(
            out.warning.unwrap(),
            "Headroom proxy not reachable at http://127.0.0.1:8787 (boom): the session starts without it."
        );
    }

    #[test]
    fn a_proxy_that_is_starting_warns_without_waiting() {
        let out = decide(&on(), TargetKind::Windows, Some(&Routed), |_| false, |_| Ensure::Starting);
        assert!(out.env.is_empty());
        assert_eq!(
            out.warning.unwrap(),
            "Headroom proxy is starting at http://127.0.0.1:8787: this session starts without it; new sessions will use it once it answers."
        );
    }

    #[test]
    fn unreachable_proxy_without_auto_start_keeps_the_plain_warning() {
        let out = run(&no_auto(), TargetKind::Windows, Some(&Routed), &[], false);
        assert_eq!(
            out.warning.unwrap(),
            "Headroom proxy not reachable at http://127.0.0.1:8787: the session starts without it."
        );
    }

    #[test]
    fn reachable_proxy_returns_the_agent_env_for_the_configured_port_and_upstream() {
        let custom = HeadroomSettings { port: 9100, openai_upstream: "https://api.deepseek.com/".to_string(), ..on() };
        let seen = std::cell::Cell::new(0);
        let out = decide(
            &custom,
            TargetKind::Windows,
            Some(&Routed),
            |_| false,
            |s| {
                seen.set(s.port);
                Ensure::Ready
            },
        );
        assert_eq!(seen.get(), 9100);
        assert_eq!(
            out.env,
            vec![
                ("PROXY_URL".to_string(), "http://127.0.0.1:9100".to_string()),
                ("PROXY_UPSTREAM".to_string(), "https://api.deepseek.com".to_string()),
            ]
        );
        assert_eq!(out.warning, None);
    }

    #[test]
    fn the_default_upstream_is_passed_when_none_is_configured() {
        let out = run(&on(), TargetKind::Windows, Some(&Routed), &[], true);
        assert_eq!(out.env[1], ("PROXY_UPSTREAM".to_string(), "https://api.openai.com".to_string()));
    }

    #[test]
    fn default_settings_leave_the_launch_unchanged() {
        assert_eq!(launch_env("no-such-agent", TargetKind::Windows, Patience::Wait), HeadroomLaunch::default());
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
