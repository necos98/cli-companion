//! Contratto `WorkerAutomation` di Pi.
//!
//! Pi non ha un sistema di permessi (ne' un flag per approvare tutto): la sola
//! leva e' l'elenco degli strumenti, che la modalita' Plan riduce a quelli di
//! sola lettura (gli strumenti MCP restano). Modello ed effort sono flag.

use super::{PiAgent, INTERACTIVE_COMMAND};
use crate::agents::{
    AutomationLaunch, AutomationRequest, EffortLevels, ModelCatalog, PermissionMode, WorkerAutomation,
};
use crate::platform::TargetKind;

/// Strumenti ammessi in modalita' Plan: solo lettura e ricerca.
const READ_ONLY_TOOLS: &str = "read,grep,find,ls";

impl WorkerAutomation for PiAgent {
    /// `pi [--model <m>] [--thinking <livello>] [--tools read,grep,find,ls] "<prompt>"`.
    /// La stringa e' digitata in PowerShell, cmd o bash/zsh: un prompt con
    /// caratteri speciali per una qualunque di queste shell viene rifiutato.
    /// `permissions_file` e' ignorato: Pi non ha regole da preparare.
    fn command(&self, _env_kind: TargetKind, req: &AutomationRequest) -> Option<AutomationLaunch> {
        let unsafe_char = |c: char| matches!(c, '"' | '$' | '`' | '%' | '!' | '\\') || c.is_control();
        if req.bootstrap_prompt.is_empty() || req.bootstrap_prompt.chars().any(unsafe_char) {
            return None;
        }
        let mut command = INTERACTIVE_COMMAND.to_string();
        // Modello non valido: nessun avvio (mai un ripiego silenzioso).
        match req.model {
            Some(m) if self.is_valid(m) => command.push_str(&format!(" --model {m}")),
            Some(_) => return None,
            None => {}
        }
        if let Some(args) = req.effort.and_then(|e| self.command_args(e)) {
            command.push(' ');
            command.push_str(&args);
        }
        if req.permission_mode == PermissionMode::Plan {
            command.push_str(&format!(" --tools {READ_ONLY_TOOLS}"));
        }
        command.push_str(&format!(" \"{}\"", req.bootstrap_prompt));
        Some(AutomationLaunch { command, env: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Effort;

    const PROMPT: &str = "You are an automated session. Call get_task.";

    fn req<'a>(mode: PermissionMode, model: Option<&'a str>, prompt: &'a str) -> AutomationRequest<'a> {
        AutomationRequest {
            bootstrap_prompt: prompt,
            permission_mode: mode,
            model,
            permissions_file: None,
            effort: None,
        }
    }

    #[test]
    fn every_mode_and_target_has_the_exact_command() {
        let cases = [
            (PermissionMode::Plan, " --tools read,grep,find,ls"),
            (PermissionMode::Default, ""),
            (PermissionMode::AcceptEdits, ""),
            (PermissionMode::Bypass, ""),
        ];
        for kind in [TargetKind::Windows, TargetKind::Wsl] {
            for (mode, tools) in cases {
                let launch = PiAgent.command(kind, &req(mode, None, PROMPT)).unwrap();
                assert_eq!(launch.command, format!("pi{tools} \"{PROMPT}\""), "{mode:?}");
                assert!(launch.env.is_empty());
            }
        }
    }

    #[test]
    fn model_and_effort_are_flags_before_the_tools_and_prompt() {
        let mut r = req(PermissionMode::Plan, Some("deepseek/deepseek-v4-flash"), PROMPT);
        r.effort = Some(Effort::XHigh);
        let launch = PiAgent.command(TargetKind::Windows, &r).unwrap();
        assert_eq!(
            launch.command,
            format!("pi --model deepseek/deepseek-v4-flash --thinking xhigh --tools read,grep,find,ls \"{PROMPT}\"")
        );
        r.permission_mode = PermissionMode::Bypass;
        r.effort = Some(Effort::Low);
        let launch = PiAgent.command(TargetKind::Wsl, &r).unwrap();
        assert_eq!(launch.command, format!("pi --model deepseek/deepseek-v4-flash --thinking low \"{PROMPT}\""));
    }

    #[test]
    fn every_effort_level_uses_the_effort_contract_args() {
        for level in PiAgent.levels() {
            let mut r = req(PermissionMode::Default, None, PROMPT);
            r.effort = Some(*level);
            let args = PiAgent.command_args(*level).unwrap();
            let launch = PiAgent.command(TargetKind::Windows, &r).unwrap();
            assert_eq!(launch.command, format!("pi {args} \"{PROMPT}\""));
        }
    }

    #[test]
    fn an_invalid_model_is_refused() {
        let long = format!("p/{}", "m".repeat(200));
        for bad in ["", "m x", "m;ls", "m\"", "x:high", "/x", long.as_str()] {
            let r = req(PermissionMode::Default, Some(bad), PROMPT);
            assert!(PiAgent.command(TargetKind::Windows, &r).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn unsafe_prompts_are_refused() {
        for bad in ["a\"b", "a$b", "a`b", "a%b", "a!b", "a\\b", "a\nb", "a\tb", ""] {
            let r = req(PermissionMode::Default, None, bad);
            assert!(PiAgent.command(TargetKind::Windows, &r).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_permissions_file_is_ignored() {
        let mut r = req(PermissionMode::Default, None, PROMPT);
        r.permissions_file = Some("C:/data/ws.json");
        let launch = PiAgent.command(TargetKind::Windows, &r).unwrap();
        assert_eq!(launch.command, format!("pi \"{PROMPT}\""));
        assert!(launch.env.is_empty());
    }
}
