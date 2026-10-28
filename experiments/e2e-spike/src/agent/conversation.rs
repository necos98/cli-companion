//! Cronologia dei messaggi di un passo, con la sostituzione delle vecchie osservazioni.

use super::observation::SUPERSEDED;
use crate::llama::client::{ChatMessage, ToolCall};

/// Ruolo del messaggio che porta il risultato di un'azione.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feedback {
    /// Risposta a una chiamata di tool nativa (stesso id).
    Tool(String),
    User,
}

/// Un messaggio la cui parte di osservazione puo' essere sostituita.
struct ObservationSlot {
    index: usize,
    prefix: String,
}

pub struct Conversation {
    messages: Vec<ChatMessage>,
    slots: Vec<ObservationSlot>,
    supersede_old: bool,
}

impl Conversation {
    /// `supersede_old`: modalita' snapshot completo, solo l'ultima osservazione resta intera.
    pub fn start(system: String, header: String, observation: &str, supersede_old: bool) -> Self {
        let mut conversation = Conversation {
            messages: vec![ChatMessage::system(system)],
            slots: Vec::new(),
            supersede_old,
        };
        conversation.push_observed(Feedback::User, header, observation);
        conversation
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn push_assistant(&mut self, content: String) {
        self.messages.push(ChatMessage::assistant(content));
    }

    pub fn push_assistant_call(&mut self, content: String, call: ToolCall) {
        self.messages
            .push(ChatMessage::assistant_with_calls(content, vec![call]));
    }

    /// Risultato di un'azione seguito dalla nuova osservazione.
    pub fn push_observed(&mut self, feedback: Feedback, prefix: String, observation: &str) {
        if self.supersede_old {
            self.supersede_previous();
        }
        let content = format!("{prefix}{observation}");
        self.slots.push(ObservationSlot {
            index: self.messages.len(),
            prefix,
        });
        self.messages.push(feedback_message(feedback, content));
    }

    /// Messaggio senza osservazione (risposta non valida: la pagina non e' cambiata).
    pub fn push_notice(&mut self, feedback: Feedback, content: String) {
        self.messages.push(feedback_message(feedback, content));
    }

    fn supersede_previous(&mut self) {
        for slot in self.slots.drain(..) {
            self.messages[slot.index].content = format!("{}{SUPERSEDED}", slot.prefix);
        }
    }
}

fn feedback_message(feedback: Feedback, content: String) -> ChatMessage {
    match feedback {
        Feedback::Tool(id) => ChatMessage::tool(&id, content),
        Feedback::User => ChatMessage::user(content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation(supersede: bool) -> Conversation {
        Conversation::start("sys".into(), "HEADER\n".into(), "obs1", supersede)
    }

    #[test]
    fn start_builds_system_and_first_user_message() {
        let c = conversation(true);
        assert_eq!(c.messages().len(), 2);
        assert_eq!(c.messages()[1].content, "HEADER\nobs1");
    }

    #[test]
    fn full_mode_supersedes_old_observations_but_keeps_prefix() {
        let mut c = conversation(true);
        c.push_assistant("{}".into());
        c.push_observed(Feedback::User, "RESULT: ok\n".into(), "obs2");
        c.push_assistant("{}".into());
        c.push_observed(Feedback::User, "RESULT: ok\n".into(), "obs3");
        let contents: Vec<&str> = c.messages().iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents[1], "HEADER\n[observation superseded]");
        assert_eq!(contents[3], "RESULT: ok\n[observation superseded]");
        assert_eq!(contents[5], "RESULT: ok\nobs3");
    }

    #[test]
    fn delta_mode_keeps_history() {
        let mut c = conversation(false);
        c.push_observed(Feedback::User, "RESULT: ok\n".into(), "obs2");
        assert_eq!(c.messages()[1].content, "HEADER\nobs1");
    }

    #[test]
    fn notices_do_not_supersede() {
        let mut c = conversation(true);
        c.push_notice(Feedback::User, "RESULT: error".into());
        assert_eq!(c.messages()[1].content, "HEADER\nobs1");
    }

    #[test]
    fn tool_feedback_carries_call_id() {
        let mut c = conversation(true);
        c.push_assistant_call("".into(), ToolCall::new("c9", "click", "{}"));
        c.push_observed(Feedback::Tool("c9".into()), "RESULT: ok\n".into(), "o");
        assert_eq!(c.messages()[3].tool_call_id.as_deref(), Some("c9"));
        assert_eq!(c.messages()[2].tool_calls.as_ref().unwrap().len(), 1);
    }
}
