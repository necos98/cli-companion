//! Contratto `SessionResume` di Pi: `pi --session <id>`.

use super::PiAgent;
use crate::agents::{ResumeFlag, ResumeSpec, SessionResume};

/// Nomi con cui si avvia Pi (su Windows anche lo shim npm); il primo e' quello canonico.
const COMMAND_NAMES: [&str; 3] = ["pi", "pi.exe", "pi.cmd"];
/// Modello del comando di ripresa: `{command}` e `{id}` sono i segnaposto del contratto.
const RESUME_TEMPLATE: &str = "{command} --session {id}";

impl SessionResume for PiAgent {
    fn resume_spec(&self) -> ResumeSpec {
        ResumeSpec {
            command_names: COMMAND_NAMES.to_vec(),
            strip_flags: vec![
                ResumeFlag { names: vec!["--continue", "-c"], takes_value: false },
                ResumeFlag { names: vec!["--resume", "-r"], takes_value: false },
                ResumeFlag { names: vec!["--session"], takes_value: true },
                ResumeFlag { names: vec!["--session-id"], takes_value: true },
                ResumeFlag { names: vec!["--fork"], takes_value: true },
                ResumeFlag { names: vec!["--no-session"], takes_value: false },
            ],
            resume_template: RESUME_TEMPLATE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{AgentIntegration, COMMAND_PLACEHOLDER, SESSION_ID_PLACEHOLDER};

    #[test]
    fn spec_has_the_expected_data() {
        let spec = PiAgent.resume_spec();
        assert_eq!(spec.command_names, vec!["pi", "pi.exe", "pi.cmd"]);
        assert_eq!(
            spec.strip_flags,
            vec![
                ResumeFlag { names: vec!["--continue", "-c"], takes_value: false },
                ResumeFlag { names: vec!["--resume", "-r"], takes_value: false },
                ResumeFlag { names: vec!["--session"], takes_value: true },
                ResumeFlag { names: vec!["--session-id"], takes_value: true },
                ResumeFlag { names: vec!["--fork"], takes_value: true },
                ResumeFlag { names: vec!["--no-session"], takes_value: false },
            ]
        );
        assert_eq!(spec.resume_template, "{command} --session {id}");
    }

    #[test]
    fn template_has_each_placeholder_once() {
        let template = PiAgent.resume_spec().resume_template;
        assert_eq!(template.matches(COMMAND_PLACEHOLDER).count(), 1);
        assert_eq!(template.matches(SESSION_ID_PLACEHOLDER).count(), 1);
    }

    #[test]
    fn names_are_lowercase_without_spaces_and_flags_start_with_a_dash() {
        let spec = PiAgent.resume_spec();
        for name in &spec.command_names {
            assert!(
                !name.is_empty() && *name == name.to_lowercase() && !name.contains(char::is_whitespace),
                "{name:?}"
            );
        }
        for flag in &spec.strip_flags {
            assert!(!flag.names.is_empty());
            assert!(flag.names.iter().all(|name| name.starts_with('-')), "{flag:?}");
        }
    }

    #[test]
    fn the_template_flag_is_stripped_so_it_is_never_duplicated() {
        let spec = PiAgent.resume_spec();
        assert!(spec.strip_flags.iter().any(|f| f.names.contains(&"--session") && f.takes_value));
    }

    #[test]
    fn interactive_command_runs_a_declared_program() {
        let interactive = PiAgent.interactive_command();
        let program = interactive.split_whitespace().next().unwrap();
        assert!(PiAgent.resume_spec().command_names.contains(&program));
    }

    #[test]
    fn spec_serializes_with_the_names_the_frontend_reads() {
        let v = serde_json::to_value(PiAgent.resume_spec()).unwrap();
        assert_eq!(v["command_names"], serde_json::json!(["pi", "pi.exe", "pi.cmd"]));
        assert_eq!(v["strip_flags"][2]["names"], serde_json::json!(["--session"]));
        assert_eq!(v["strip_flags"][2]["takes_value"], true);
        assert_eq!(v["resume_template"], "{command} --session {id}");
    }
}
