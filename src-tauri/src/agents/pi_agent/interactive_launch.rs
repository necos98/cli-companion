//! Contratto `InteractiveLauncher` di Pi: tutto passa da flag.
//!
//! `--model`, `--thinking` e `--append-system-prompt <file>` (le istruzioni di
//! sistema dei Bot) si applicano sia all'avvio sia al ripristino. La skill e'
//! il primo prompt `/skill:<nome>` e non si ripristina mai. Nessuna variabile
//! d'ambiente.

use super::{PiAgent, INTERACTIVE_COMMAND};
use crate::agents::{
    is_safe_launch_token, is_shell_safe_path, EffortLevels, InteractiveLaunch, InteractiveLauncher, InteractiveRequest,
    LaunchSkill, ModelCatalog,
};

impl InteractiveLauncher for PiAgent {
    /// `pi [--model <m>] [--thinking <l>] [--append-system-prompt "<file>"] ["/skill:<nome>"]`.
    /// `restore_command` e' lo stesso senza la skill: riprende la conversazione.
    fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch> {
        // Valori non validi: nessun avvio (mai un ripiego silenzioso).
        if req.model.is_some_and(|model| !self.is_valid(model))
            || req.instructions_file.is_some_and(|file| !is_shell_safe_path(file))
        {
            return None;
        }
        let skill = match req.skill {
            // Il nome della skill di un preset e' il suo id.
            Some(LaunchSkill::Plugin(name) | LaunchSkill::User(name)) if is_safe_launch_token(name) => Some(name),
            Some(_) => return None,
            None => None,
        };
        let mut restore_command = INTERACTIVE_COMMAND.to_string();
        if let Some(model) = req.model {
            restore_command.push_str(&format!(" --model {model}"));
        }
        if let Some(args) = req.effort.and_then(|e| self.command_args(e)) {
            restore_command.push(' ');
            restore_command.push_str(&args);
        }
        if let Some(file) = req.instructions_file {
            restore_command.push_str(&format!(" --append-system-prompt \"{file}\""));
        }
        let command = match skill {
            Some(name) => format!("{restore_command} \"/skill:{name}\""),
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
    use crate::agents::Effort;

    fn launch(model: Option<&str>, effort: Option<Effort>, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        PiAgent.launch(&InteractiveRequest { model, effort, skill, instructions_file: None })
    }

    fn launch_with_file(model: Option<&str>, file: &str, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        PiAgent.launch(&InteractiveRequest { model, effort: None, skill, instructions_file: Some(file) })
    }

    #[test]
    fn an_empty_request_is_the_bare_command() {
        assert_eq!(
            launch(None, None, None).unwrap(),
            InteractiveLaunch { command: "pi".into(), restore_command: "pi".into(), env: vec![], restore_env: vec![] }
        );
    }

    #[test]
    fn model_and_effort_are_flags_in_both_lines() {
        let l = launch(Some("deepseek/deepseek-v4-flash"), Some(Effort::High), None).unwrap();
        assert_eq!(l.command, "pi --model deepseek/deepseek-v4-flash --thinking high");
        assert_eq!(l.restore_command, l.command);
        assert!(l.env.is_empty() && l.restore_env.is_empty());
    }

    #[test]
    fn a_skill_is_the_first_prompt_and_never_restored() {
        for skill in [LaunchSkill::Plugin("task-intake"), LaunchSkill::User("task-intake")] {
            let l = launch(None, None, Some(skill)).unwrap();
            assert_eq!(l.command, "pi \"/skill:task-intake\"");
            assert_eq!(l.restore_command, "pi");
        }
    }

    #[test]
    fn everything_combines_in_a_fixed_order() {
        let file = "C:/Users/Mario Rossi/AppData/Roaming/cli-companion/bot-instructions/engineering-standard.md";
        let l = PiAgent
            .launch(&InteractiveRequest {
                model: Some("p/m"),
                effort: Some(Effort::Max),
                skill: Some(LaunchSkill::User("my-skill")),
                instructions_file: Some(file),
            })
            .unwrap();
        let base = format!("pi --model p/m --thinking max --append-system-prompt \"{file}\"");
        assert_eq!(l.restore_command, base);
        assert_eq!(l.command, format!("{base} \"/skill:my-skill\""));
    }

    #[test]
    fn the_instructions_file_is_applied_at_launch_and_restore() {
        let wsl = "/mnt/c/Users/me/AppData/Roaming/cli-companion/bot-instructions/engineering-standard.md";
        let l = launch_with_file(None, wsl, None).unwrap();
        assert_eq!(l.command, format!("pi --append-system-prompt \"{wsl}\""));
        assert_eq!(l.restore_command, l.command);
    }

    #[test]
    fn unsafe_values_are_rejected() {
        let long = "a".repeat(65);
        for bad in ["a b", "a$b", "a\"b", "a%b", "a!b", "a`b", "", long.as_str()] {
            assert!(launch(None, None, Some(LaunchSkill::Plugin(bad))).is_none(), "plugin {bad:?}");
            assert!(launch(None, None, Some(LaunchSkill::User(bad))).is_none(), "user {bad:?}");
        }
        for bad in ["a b/c", "p/m;ls", "p/m\"", "x:high", ""] {
            assert!(launch(Some(bad), None, None).is_none(), "model {bad:?}");
        }
        for bad in [r"C:\a$b\x.md", r"C:\a`b\x.md", r"C:\100%\x.md", r#"C:\a"b\x.md"#, r"C:\hi!\x.md", "C:/a\nb", ""] {
            assert!(launch_with_file(None, bad, None).is_none(), "file {bad:?}");
        }
    }

    #[test]
    fn instructions_are_declared() {
        assert!(PiAgent.applies_instructions());
    }
}
