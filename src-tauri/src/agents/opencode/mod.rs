//! Integrazione OpenCode (v2), autocontenuta: tutto cio' che il core sa di
//! questo agente passa dai contratti implementati qui (un file per contratto).
//!
//! Progetto:
//! - nessun file dell'utente viene modificato: installiamo SOLO la cartella
//!   `<config OpenCode>/plugins/cli-companion-notify/` (un plugin che OpenCode
//!   carica da solo), riconosciuta da una riga di intestazione fissa;
//! - il plugin registra a runtime il server MCP e le skill dei preset, e
//!   inoltra gli eventi della sessione a `cli-companion --notify`;
//! - OpenCode v2 di norma parla con un servizio condiviso che non vede
//!   l'ambiente del tab: ogni comando generato usa `--standalone` (server
//!   privato, figlio del TUI) e il plugin resta inerte fuori da un terminale
//!   gestito (`CLI_COMPANION_SESSION`) e dentro un servizio condiviso;
//! - modello e permessi dei worker passano da `OPENCODE_CONFIG_CONTENT` e
//!   `OPENCODE_CONFIG` (variabili impostate dal core sul tab, mai digitate).
//!
//! Layout generato in `<app_data>/agent-plugins/opencode/<env>/` (copia del
//! plugin piu' `destination.txt`, la cartella in cui e' stato installato):
//! ```text
//! index.ts
//! skills/<preset id>/SKILL.md   (uno per preset)
//! ```

mod automation;
mod config_dir;
mod hook_events;
mod interactive_launch;
mod mode_rules;
mod models;
mod package;
mod permissions;
mod plugin;
mod plugin_skills;
mod readable_dirs;
mod resume;
mod skills;
mod version;

use super::contracts::{AgentIntegration, Capabilities, Detection};
use super::detection::{resolve_wsl_command_cached, DETECT_TIMEOUT};
use crate::platform::Target;

pub struct OpenCode;

const AGENT_ID: &str = "opencode";
/// Programma da lanciare (su Windows lo shim npm `opencode.cmd` o `opencode.exe`).
const PROGRAM: &str = "opencode";
/// Server privato figlio del TUI: eredita l'ambiente del terminale.
const STANDALONE_FLAG: &str = "--standalone";
/// Comando che avvia OpenCode in modo interattivo (base anche dei preset).
const INTERACTIVE_COMMAND: &str = "opencode --standalone";
/// Nome del plugin e della sua cartella dentro `<config>/plugins/`.
const PLUGIN: &str = "cli-companion-notify";
/// Nome del server MCP registrato dal plugin (prefisso delle sue azioni di permesso).
const MCP_SERVER: &str = "cli-companion";
/// Unica versione maggiore con l'API dei plugin che usiamo.
const SUPPORTED_MAJOR: u32 = 2;

impl AgentIntegration for OpenCode {
    fn id(&self) -> &'static str {
        AGENT_ID
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
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
            worker_permissions: Some(self),
            plugin_skills: Some(self),
            user_skills: Some(self),
            // Nessuna finestra di abbonamento da leggere.
            usage_limits: None,
            // Le sessioni stanno in SQLite, non in un transcript leggibile.
            session_usage: None,
            // Incolla immagini non verificato: vale il comportamento predefinito del terminale.
            input_profile: None,
            models: Some(self),
            // La CLI non ha un flag di effort (solo `--model provider/modello#variante` in `run`).
            effort: None,
            interactive_launch: Some(self),
            // Nessun instradamento verificato su un proxy: parte sempre diretto.
            proxy_routing: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{registry, Capability};

    #[test]
    fn identity_and_declared_contracts() {
        assert_eq!((OpenCode.id(), OpenCode.display_name()), ("opencode", "OpenCode"));
        assert_eq!(OpenCode.interactive_command(), "opencode --standalone");
        assert!(INTERACTIVE_COMMAND.contains(STANDALONE_FLAG));
        assert_eq!(
            OpenCode.capabilities().declared(),
            [
                Capability::Package,
                Capability::HookEvents,
                Capability::SessionResume,
                Capability::Automation,
                Capability::WorkerPermissions,
                Capability::PluginSkills,
                Capability::UserSkills,
                Capability::Models,
                Capability::InteractiveLaunch,
            ]
        );
    }

    #[test]
    fn it_is_registered_after_the_first_integration() {
        let ids: Vec<_> = registry::all().iter().map(|i| i.id()).collect();
        assert_eq!(ids.iter().position(|id| *id == AGENT_ID), Some(1), "{ids:?}");
    }
}
