//! Prompt inviati al modello (sempre in inglese).

use super::catalog::describe_actions;
use super::ActionMode;

const RULES: &str = "\
You are a QA agent executing ONE step of an end-to-end test in a web browser.
The page is shown as an accessibility snapshot of its interactive elements; each element has a ref such as e12. The visible page text is shown as well.

Rules:
- Do exactly one action per turn.
- Use only refs from the latest snapshot.
- Use only values from the TEST DATA block for names, emails and passwords; never invent them.
- Paths are site-relative, for example /register.
- When the step is complete, call done with success=true and a short note of what you observed.
- If the step cannot be completed, call done with success=false and explain why. If the page shows an error message, quote it in the note.
- Do not do more than the step asks.";

const NATIVE_FORMAT: &str = "Respond by calling exactly one of the tools.";
const SCHEMA_FORMAT: &str =
    "Reply with one JSON object only, for example {\"action\":\"click\",\"ref\":\"e12\"}. \
The \"action\" field is the action name; the other fields are its arguments.";

pub const OBSERVATION_HEADING: &str = "OBSERVATION:";
pub const RESULT_OK: &str = "RESULT: ok";

pub fn system_prompt(mode: ActionMode) -> String {
    let format = match mode {
        ActionMode::Native => NATIVE_FORMAT,
        ActionMode::Schema => SCHEMA_FORMAT,
    };
    format!("{RULES}\n\nActions:\n{}\n\n{format}", describe_actions())
}

/// Parte fissa del primo messaggio utente; segue l'osservazione.
pub fn step_header(
    test_data: &[(String, String)],
    completed: &[String],
    step_text: &str,
) -> String {
    let data: Vec<String> = test_data.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let done = if completed.is_empty() {
        "none".to_string()
    } else {
        completed
            .iter()
            .enumerate()
            .map(|(i, text)| format!("{}. {text}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "TEST DATA:\n{}\n\nCOMPLETED STEPS:\n{done}\n\nCURRENT STEP: {step_text}\n\n{OBSERVATION_HEADING}\n",
        data.join("\n")
    )
}

pub fn result_error(message: &str) -> String {
    format!("RESULT: error: {message}\n")
}

pub fn invalid_reply_notice(message: &str) -> String {
    format!(
        "RESULT: error: invalid reply: {message}. No action was taken and the page is unchanged."
    )
}

pub fn ok_prefix() -> String {
    format!("{RESULT_OK}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_prompt_has_format_hint_and_native_does_not() {
        assert!(system_prompt(ActionMode::Schema).contains("JSON object only"));
        assert!(!system_prompt(ActionMode::Native).contains("JSON object only"));
        assert!(system_prompt(ActionMode::Native).contains("- open(path)"));
    }

    #[test]
    fn header_lists_data_and_completed_steps() {
        let data = vec![("name".to_string(), "Dana".to_string())];
        let header = step_header(&data, &["open home".to_string()], "do it");
        assert!(header.contains("TEST DATA:\nname: Dana"));
        assert!(header.contains("COMPLETED STEPS:\n1. open home"));
        assert!(header.ends_with("OBSERVATION:\n"));
        assert!(step_header(&data, &[], "x").contains("COMPLETED STEPS:\nnone"));
    }
}
