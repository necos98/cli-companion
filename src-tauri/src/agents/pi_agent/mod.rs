//! Integrazione Pi (pi coding agent 1.x, `@earendil-works/pi-coding-agent`),
//! autocontenuta: tutto cio' che il core sa di questo agente passa dai
//! contratti implementati qui (un file per contratto).
//!
//! Progetto:
//! - nessun file dell'utente viene modificato: installiamo SOLO la cartella
//!   `<agent dir>/extensions/cli-companion-notify/` (un'estensione che Pi
//!   scopre da solo), riconosciuta da una riga di intestazione fissa;
//!   `<agent dir>` e' `$PI_CODING_AGENT_DIR` o `~/.pi/agent`;
//! - l'estensione registra a runtime il server MCP e le skill dei preset, e
//!   inoltra gli eventi della sessione a `cli-companion --notify`; resta
//!   inerte fuori da un terminale gestito (`CLI_COMPANION_SESSION`);
//! - Pi non chiede permessi: nessun contratto `WorkerPermissions`; la modalita'
//!   Plan limita gli strumenti con `--tools`.
//!
//! Layout generato in `<app_data>/agent-plugins/pi-agent/<env>/` (copia
//! dell'estensione piu' `destination.txt`, la cartella in cui e' installata):
//! ```text
//! index.ts
//! skills/<preset id>/SKILL.md   (uno per preset)
//! ```

mod agent_dir;
mod automation;
mod effort;
mod extension;
mod hook_events;
mod interactive_launch;
mod models;
mod package;
mod plugin_skills;
mod proxy_routing;
mod resume;
mod session_usage;
mod skills;
mod version;

use super::contracts::{AgentIntegration, Capabilities, Detection};
use super::detection::{resolve_wsl_command_cached, DETECT_TIMEOUT};
use crate::platform::Target;

pub struct PiAgent;

const AGENT_ID: &str = "pi-agent";
/// Programma da lanciare (su Windows lo shim npm `pi.cmd`).
const PROGRAM: &str = "pi";
/// Comando che avvia Pi in modo interattivo (base anche dei preset).
const INTERACTIVE_COMMAND: &str = "pi";
/// Nome dell'estensione e della sua cartella dentro `<agent dir>/extensions/`.
const EXTENSION: &str = "cli-companion-notify";
/// Nome del server MCP registrato dall'estensione (gli strumenti diventano
/// `mcp__cli_companion__<nome>`).
const MCP_SERVER: &str = "cli-companion";
/// Unica versione maggiore con l'API delle estensioni che usiamo.
const SUPPORTED_MAJOR: u32 = 1;

impl AgentIntegration for PiAgent {
    fn id(&self) -> &'static str {
        AGENT_ID
    }

    fn display_name(&self) -> &'static str {
        "Pi"
    }

    fn detect(&self, target: &Target) -> Detection {
        let path = match target {
            Target::Windows => Some(PROGRAM.to_string()),
            Target::Wsl { distro } => resolve_wsl_command_cached(distro, PROGRAM),
        };
        let Some(path) = path else {
            return Detection::default();
        };
        match target.run_timeout(&path, &["--version"], DETECT_TIMEOUT) {
            Ok(out) => Detection { found: true, version: version::parse_version(&out), path: Some(path) },
            Err(_) => Detection::default(),
        }
    }

    fn interactive_command(&self) -> &'static str {
        INTERACTIVE_COMMAND
    }

    fn capabilities(&self) -> Capabilities<'_> {
        Capabilities {
            package: Some(self),
            hook_events: Some(self),
            session_resume: Some(self),
            automation: Some(self),
            // Pi non chiede permessi: nessuna regola da preparare.
            worker_permissions: None,
            plugin_skills: Some(self),
            user_skills: Some(self),
            // Chiavi API di piu' provider: nessuna finestra di abbonamento da leggere.
            usage_limits: None,
            session_usage: Some(self),
            // Incolla immagini non verificato: vale il comportamento predefinito del terminale.
            input_profile: None,
            models: Some(self),
            effort: Some(self),
            interactive_launch: Some(self),
            proxy_routing: Some(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{registry, Capability};

    #[test]
    fn identity_and_declared_contracts() {
        assert_eq!((PiAgent.id(), PiAgent.display_name()), ("pi-agent", "Pi"));
        assert_eq!(PiAgent.interactive_command(), "pi");
        assert_eq!(
            PiAgent.capabilities().declared(),
            [
                Capability::Package,
                Capability::HookEvents,
                Capability::SessionResume,
                Capability::Automation,
                Capability::PluginSkills,
                Capability::UserSkills,
                Capability::SessionUsage,
                Capability::Models,
                Capability::Effort,
                Capability::InteractiveLaunch,
                Capability::ProxyRouting,
            ]
        );
    }

    #[test]
    fn it_is_registered_after_opencode() {
        let ids: Vec<_> = registry::all().iter().map(|i| i.id()).collect();
        assert_eq!(ids.iter().position(|id| *id == AGENT_ID), Some(2), "{ids:?}");
    }
}
