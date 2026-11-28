//! Integrazione Claude Code, autocontenuta: tutto cio' che il core sa di questo
//! agente passa dai contratti implementati qui (un file per contratto).
//!
//! Layout generato in `<app_data>/agent-plugins/claude-code/<env>/`: vedi `plugin.rs`.

mod automation;
mod config_dir;
mod effort;
mod exploration;
mod hook_events;
mod input_profile;
mod interactive_launch;
mod models;
mod package;
mod permissions;
mod plugin;
mod plugin_skills;
mod proxy_routing;
mod readable_dirs;
mod resume;
mod session_usage;
mod skills;
mod usage_limits;

use super::contracts::{AgentIntegration, Capabilities, Detection};
use super::detection::{parse_version, resolve_wsl_command_cached, DETECT_TIMEOUT};
use crate::platform::Target;

pub struct ClaudeCode;

pub const AGENT_ID: &str = "claude-code";
/// Comando che avvia Claude Code in modo interattivo (base anche dei preset).
const INTERACTIVE_COMMAND: &str = "claude --dangerously-skip-permissions";
const MARKETPLACE: &str = "cli-companion-local";
const PLUGIN: &str = "cli-companion-notify";
/// Nome del sotto-agente esploratore distribuito con il plugin (`agents/<nome>.md`).
const EXPLORER_AGENT: &str = "explorer";
/// Id come lo riporta `claude plugin list --json`.
const PLUGIN_ID: &str = "cli-companion-notify@cli-companion-local";
/// Regola di permesso che copre tutti i tool del server MCP del plugin
/// (`mcp__plugin_<plugin>_<server>__<tool>`; senza nome tool = tutti).
/// `acceptEdits` non approva i tool MCP, quindi serve sempre.
const MCP_ALLOW_RULE: &str = "mcp__plugin_cli-companion-notify_cli-companion";

impl AgentIntegration for ClaudeCode {
    fn id(&self) -> &'static str {
        AGENT_ID
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, target: &Target) -> Detection {
        let path = match target {
            Target::Windows => Some("claude".to_string()),
            Target::Wsl { distro } => resolve_wsl_command_cached(distro, "claude"),
        };
        let Some(path) = path else {
            return Detection::default();
        };
        match target.run_timeout(&path, &["--version"], DETECT_TIMEOUT) {
            Ok(out) => Detection { found: true, version: parse_version(&out), path: Some(path) },
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
            usage_limits: Some(self),
            session_usage: Some(self),
            input_profile: Some(self),
            models: Some(self),
            effort: Some(self),
            interactive_launch: Some(self),
            proxy_routing: Some(self),
            exploration_delegation: Some(self),
        }
    }
}
