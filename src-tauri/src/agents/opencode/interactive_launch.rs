//! Contratto `InteractiveLauncher` di OpenCode.
//!
//! Il TUI non ha `--model`: il modello passa da `OPENCODE_CONFIG_CONTENT`
//! (impostata dal core sul tab, mai digitata) con il solo modello, cosi' un tab
//! interattivo conserva permessi e agente di partenza dell'utente. Il file di
//! istruzioni di sistema va nella stessa variabile, come chiave `instructions`
//! (documentazione di OpenCode: "Instructions", path assoluti usati come
//! scritti, caricati dopo gli `AGENTS.md`). La skill e' il primo prompt, come
//! per i preset. L'effort non c'e': OpenCode non dichiara `EffortLevels`,
//! quindi il core lo toglie prima (con un avviso) e qui si ignora.

use super::automation::ENV_CONFIG_CONTENT;
use super::mode_rules::interactive_content;
use super::plugin_skills::skill_command;
use super::{OpenCode, INTERACTIVE_COMMAND};
use crate::agents::{
    is_safe_launch_token, is_shell_safe_path, InteractiveLaunch, InteractiveLauncher, InteractiveRequest, LaunchSkill,
    ModelCatalog,
};

impl InteractiveLauncher for OpenCode {
    /// `opencode --standalone [--prompt "<carica la skill>"]`. `restore_command`
    /// e' sempre l'interattivo: riprende la conversazione; modello e istruzioni
    /// restano nell'ambiente, che `restore_env` rimette al ripristino del tab.
    fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch> {
        // Modello o file non validi: nessun avvio (mai un ripiego silenzioso).
        if req.model.is_some_and(|model| !self.is_valid(model))
            || req.instructions_file.is_some_and(|file| !is_shell_safe_path(file))
        {
            return None;
        }
        let env: Vec<(String, String)> = interactive_content(req.model, req.instructions_file)
            .map(|content| (ENV_CONFIG_CONTENT.to_string(), content))
            .into_iter()
            .collect();
        let command = match req.skill {
            Some(LaunchSkill::Plugin(id) | LaunchSkill::User(id)) if is_safe_launch_token(id) => skill_command(id),
            Some(_) => return None,
            None => INTERACTIVE_COMMAND.to_string(),
        };
        Some(InteractiveLaunch {
            command,
            restore_command: INTERACTIVE_COMMAND.to_string(),
            restore_env: env.clone(),
            env,
        })
    }

    fn applies_instructions(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{first_invalid_env_name, Effort};

    fn launch(model: Option<&str>, effort: Option<Effort>, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        OpenCode.launch(&InteractiveRequest { model, effort, skill, instructions_file: None })
    }

    fn launch_with_file(model: Option<&str>, file: &str, skill: Option<LaunchSkill>) -> Option<InteractiveLaunch> {
        OpenCode.launch(&InteractiveRequest { model, effort: None, skill, instructions_file: Some(file) })
    }

    fn config(env: &[(String, String)]) -> serde_json::Value {
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "OPENCODE_CONFIG_CONTENT");
        serde_json::from_str(&env[0].1).unwrap()
    }
    #[test]
    fn an_empty_request_is_the_interactive_command_without_environment() {
        let l = launch(None, None, None).unwrap();
        assert_eq!(
            l,
            InteractiveLaunch {
                command: "opencode --standalone".into(),
                restore_command: "opencode --standalone".into(),
                env: vec![],
                restore_env: vec![]
            }
        );
    }

    #[test]
    fn the_model_travels_in_the_environment_not_in_the_command() {
        let l = launch(Some("opencode/gpt-5.5"), None, None).unwrap();
        assert_eq!(l.command, "opencode --standalone");
        assert_eq!(l.restore_command, "opencode --standalone");
        assert_eq!(l.env, [("OPENCODE_CONFIG_CONTENT".to_string(), r#"{"model":"opencode/gpt-5.5"}"#.to_string())]);
        assert_eq!(l.restore_env, l.env);
        assert_eq!(first_invalid_env_name(&l.restore_env), None);
    }

    #[test]
    fn a_skill_is_loaded_by_the_first_prompt_and_never_restored() {
        for skill in [LaunchSkill::Plugin("task-intake"), LaunchSkill::User("task-intake")] {
            let l = launch(None, None, Some(skill)).unwrap();
            assert_eq!(
                l.command,
                "opencode --standalone --prompt \"Use the skill tool to load the skill task-intake, then follow its instructions.\""
            );
            assert_eq!(l.restore_command, "opencode --standalone");
            assert!(l.env.is_empty() && l.restore_env.is_empty());
        }
    }

    #[test]
    fn a_plugin_skill_is_loaded_by_the_first_prompt() {
        let preset = &crate::agents::presets::PRESETS[0];
        let l = launch(None, None, Some(LaunchSkill::Plugin(preset.id))).unwrap();
        assert_eq!(l.command, skill_command(preset.id));
    }

    #[test]
    fn model_and_skill_combine() {
        let l = launch(Some("ollama/llama3:8b"), None, Some(LaunchSkill::User("my-skill"))).unwrap();
        assert!(l.command.ends_with("skill my-skill, then follow its instructions.\""));
        assert_eq!(l.env.len(), 1);
    }

    #[test]
    fn effort_is_ignored() {
        let with = launch(Some("opencode/gpt-5.5"), Some(Effort::Max), None).unwrap();
        assert_eq!(Some(with), launch(Some("opencode/gpt-5.5"), None, None));
    }

    #[test]
    fn unsafe_values_are_rejected() {
        let long = "a".repeat(65);
        for bad in ["a b", "a$b", "a\"b", "a%b", "a!b", "a`b", "", long.as_str()] {
            assert!(launch(None, None, Some(LaunchSkill::Plugin(bad))).is_none(), "plugin {bad:?}");
            assert!(launch(None, None, Some(LaunchSkill::User(bad))).is_none(), "user {bad:?}");
        }
        for bad in ["sonnet", "a b/c", "p/m;ls", "p/m\"", ""] {
            assert!(launch(Some(bad), None, None).is_none(), "model {bad:?}");
        }
    }

    #[test]
    fn the_instructions_file_travels_in_the_environment_and_is_restored() {
        let file = r"C:\Users\Mario Rossi\AppData\Roaming\cli-companion\bot-instructions\engineering-standard.md";
        let l = launch_with_file(Some("opencode/gpt-5.5"), file, Some(LaunchSkill::Plugin("task-intake"))).unwrap();
        assert_eq!(config(&l.env), serde_json::json!({ "model": "opencode/gpt-5.5", "instructions": [file] }));
        assert_eq!(l.restore_env, l.env);
        assert_eq!(l.restore_command, "opencode --standalone");
        assert!(!l.command.contains(file) && l.command.contains("--prompt"));
        let wsl = "/mnt/c/Users/me/AppData/Roaming/cli-companion/bot-instructions/engineering-standard.md";
        let l = launch_with_file(None, wsl, None).unwrap();
        assert_eq!(config(&l.restore_env), serde_json::json!({ "instructions": [wsl] }));
        assert_eq!(first_invalid_env_name(&l.restore_env), None);
    }

    #[test]
    fn an_unsafe_instructions_file_is_rejected() {
        for bad in [r"C:\a$b\x.md", r"C:\a`b\x.md", r"C:\100%\x.md", r#"C:\a"b\x.md"#, r"C:\hi!\x.md", "C:/a\nb", ""] {
            assert!(launch_with_file(None, bad, None).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn instructions_are_declared() {
        assert!(OpenCode.applies_instructions());
    }
}
