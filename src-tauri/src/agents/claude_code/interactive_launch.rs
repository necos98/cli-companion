//! Contratto `InteractiveLauncher` di Claude Code:
//! `claude --dangerously-skip-permissions [--model <m>] [--effort <livello>]
//! [--append-system-prompt-file "<file>"] ["/<skill>"]`.
//! Claude Code si configura solo con i flag: nessuna variabile d'ambiente.
//! `--append-system-prompt-file` e' nell'help della CLI (`claude --help`, voce
//! `--bare`) e la CLI rifiuta un file che non esiste.

use super::plugin_skills::plugin_skill_invocation;
use super::{ClaudeCode, INTERACTIVE_COMMAND};
use crate::agents::{
    is_safe_launch_token, is_shell_safe_path, EffortLevels, InteractiveLaunch, InteractiveLauncher, InteractiveRequest,
    LaunchSkill,
};

/// Flag della CLI che sceglie il modello.
const MODEL_FLAG: &str = "--model";

/// Flag della CLI che aggiunge un file al prompt di sistema predefinito.
const APPEND_SYSTEM_PROMPT_FILE_FLAG: &str = "--append-system-prompt-file";

/// Il prompt che avvia la skill: `/<plugin>:<id>` per quelle del plugin,
/// `/<nome>` per quelle dell'utente. `None` se il nome non e' sicuro.
fn skill_invocation(skill: LaunchSkill) -> Option<String> {
    match skill {
        LaunchSkill::Plugin(id) if is_safe_launch_token(id) => Some(plugin_skill_invocation(id)),
        LaunchSkill::User(name) if is_safe_launch_token(name) => Some(format!("/{name}")),
        _ => None,
    }
}

impl InteractiveLauncher for ClaudeCode {
    /// Il prompt della skill va in coda, fra doppi apici (come i preset);
    /// `restore_command` e' la stessa riga senza di lui, quindi conserva le
    /// istruzioni di sistema.
    fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch> {
        let mut restore_command = INTERACTIVE_COMMAND.to_string();
        if let Some(model) = req.model {
            if !is_safe_launch_token(model) {
                return None;
            }
            restore_command.push_str(&format!(" {MODEL_FLAG} {model}"));
        }
        if let Some(effort) = req.effort {
            restore_command.push(' ');
            restore_command.push_str(&self.command_args(effort)?);
        }
        if let Some(file) = req.instructions_file {
            // Fra doppi apici: valido in PowerShell, cmd e bash se privo di `" $ ` % !`.
            if !is_shell_safe_path(file) {
                return None;
            }
            restore_command.push_str(&format!(" {APPEND_SYSTEM_PROMPT_FILE_FLAG} \"{file}\""));
        }
        let command = match req.skill {
            Some(skill) => format!("{restore_command} \"{}\"", skill_invocation(skill)?),
            None => restore_command.clone(),
        };
        Some(InteractiveLaunch { command, restore_command, env: Vec::new(), restore_env: Vec::new() })
    }

    fn applies_instructions(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentIntegration, Effort};

    const BASE: &str = "claude --dangerously-skip-permissions";
    const FILE: &str = r"C:\Users\Mario Rossi\AppData\Roaming\cli-companion\bot-instructions\engineering-standard.md";

    fn launch(model: Option<&str>, effort: Option<Effort>, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        ClaudeCode.launch(&InteractiveRequest { model, effort, skill, instructions_file: None })
    }

    fn launch_with_file(file: &str, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        ClaudeCode.launch(&InteractiveRequest {
            model: Some("opus"),
            effort: Some(Effort::High),
            skill,
            instructions_file: Some(file),
        })
    }
    #[test]
    fn an_empty_request_is_the_interactive_command() {
        let l = launch(None, None, None).unwrap();
        assert_eq!(
            l,
            InteractiveLaunch { command: BASE.into(), restore_command: BASE.into(), env: vec![], restore_env: vec![] }
        );
        assert_eq!(l.command, ClaudeCode.interactive_command());
    }

    #[test]
    fn model_effort_and_skill_combine_in_a_fixed_order() {
        let plugin = Some(LaunchSkill::Plugin("task-intake"));
        let user = Some(LaunchSkill::User("my-skill"));
        let cases = [
            (Some("opus"), None, None, format!("{BASE} --model opus")),
            (None, Some(Effort::High), None, format!("{BASE} --effort high")),
            (None, None, plugin, format!("{BASE} \"/cli-companion-notify:task-intake\"")),
            (None, None, user, format!("{BASE} \"/my-skill\"")),
            (Some("opus"), Some(Effort::XHigh), None, format!("{BASE} --model opus --effort xhigh")),
            (
                Some("claude-opus-4.1"),
                Some(Effort::Low),
                plugin,
                format!("{BASE} --model claude-opus-4.1 --effort low \"/cli-companion-notify:task-intake\""),
            ),
            (Some("opus"), None, user, format!("{BASE} --model opus \"/my-skill\"")),
        ];
        for (model, effort, skill, expected) in cases {
            let l = launch(model, effort, skill).unwrap();
            assert_eq!(l.command, expected);
            assert!(l.env.is_empty());
        }
    }

    #[test]
    fn restore_keeps_model_and_effort_but_never_the_skill() {
        let l = launch(Some("opus"), Some(Effort::Max), Some(LaunchSkill::User("my-skill"))).unwrap();
        assert_eq!(l.restore_command, format!("{BASE} --model opus --effort max"));
        let l = launch(None, None, Some(LaunchSkill::Plugin("task-intake"))).unwrap();
        assert_eq!(l.restore_command, BASE);
    }

    #[test]
    fn a_plugin_skill_is_the_first_prompt() {
        let preset = &crate::agents::presets::PRESETS[0];
        let l = launch(None, None, Some(LaunchSkill::Plugin(preset.id))).unwrap();
        assert_eq!(l.command, format!("{BASE} \"/cli-companion-notify:{}\"", preset.id));
    }

    #[test]
    fn unsafe_values_are_rejected() {
        let long = "a".repeat(65);
        for bad in ["a b", "a$b", "a\"b", "a%b", "a!b", "a`b", "", long.as_str()] {
            assert!(launch(Some(bad), None, None).is_none(), "model {bad:?}");
            assert!(launch(None, None, Some(LaunchSkill::Plugin(bad))).is_none(), "plugin {bad:?}");
            assert!(launch(None, None, Some(LaunchSkill::User(bad))).is_none(), "user {bad:?}");
        }
    }

    #[test]
    fn the_instructions_file_is_appended_to_the_system_prompt_and_kept_on_restore() {
        let flag = format!("--append-system-prompt-file \"{FILE}\"");
        let l = launch_with_file(FILE, Some(LaunchSkill::Plugin("task-intake"))).unwrap();
        assert_eq!(
            l.command,
            format!("{BASE} --model opus --effort high {flag} \"/cli-companion-notify:task-intake\"")
        );
        assert_eq!(l.restore_command, format!("{BASE} --model opus --effort high {flag}"));
        assert!(l.env.is_empty() && l.restore_env.is_empty());
        let wsl = "/mnt/c/Users/me/AppData/Roaming/cli-companion/bot-instructions/engineering-standard.md";
        let l = launch_with_file(wsl, None).unwrap();
        assert!(l.command.ends_with(&format!("--append-system-prompt-file \"{wsl}\"")));
        assert_eq!(l.command, l.restore_command);
    }

    #[test]
    fn without_an_instructions_file_the_system_prompt_is_untouched() {
        let l = launch(Some("opus"), None, Some(LaunchSkill::User("my-skill"))).unwrap();
        assert!(!l.command.contains(APPEND_SYSTEM_PROMPT_FILE_FLAG));
        assert!(!l.restore_command.contains(APPEND_SYSTEM_PROMPT_FILE_FLAG));
    }

    #[test]
    fn an_unsafe_instructions_file_is_rejected() {
        for bad in [r"C:\a$b\x.md", r"C:\a`b\x.md", r"C:\100%\x.md", r#"C:\a"b\x.md"#, r"C:\hi!\x.md", "C:/a\nb", ""] {
            assert!(launch_with_file(bad, None).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn instructions_are_declared() {
        assert!(ClaudeCode.applies_instructions());
    }
}
