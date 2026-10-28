//! Ciclo dell'agente per un passo di test: osserva, chiede al modello, esegue.

use super::action::{Action, InvalidAction};
use super::catalog::{native_tools, response_format};
use super::conversation::{Conversation, Feedback};
use super::execute::execute;
use super::observation::{ObservationRenderer, RenderedObservation};
use super::parse::{parse_response, ParsedTurn};
use super::prompt::{invalid_reply_notice, ok_prefix, result_error, step_header, system_prompt};
use super::turn::{TurnContext, TurnLog};
use super::{ActionMode, AgentConfig, SnapshotMode};
use crate::browser::session::BrowserSession;
use crate::llama::client::{ChatRequest, ChatResponse, LlamaClient, ToolCall};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Instant;

pub struct StepInput<'a> {
    pub test_data: &'a [(String, String)],
    pub completed: &'a [String],
    pub step_text: &'a str,
}

/// Esito dichiarato dal modello con `done`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DoneReport {
    pub success: bool,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    Done,
    BudgetExhausted,
    LlmError,
    HarnessError,
}

pub struct StepOutcome {
    pub end: EndReason,
    pub done: Option<DoneReport>,
    pub turns: u32,
    pub invalid_actions: u32,
    pub error: Option<String>,
}

/// Cio' che il modello vede nell'osservazione corrente.
struct Shown {
    snapshot_kind: &'static str,
    snapshot_tokens: Option<usize>,
}

struct Runner<'a> {
    llm: &'a LlamaClient,
    session: &'a mut BrowserSession,
    cfg: &'a AgentConfig,
    renderer: ObservationRenderer,
    conversation: Conversation,
    tools: Option<Vec<Value>>,
    response_format: Option<Value>,
    shown: Shown,
    outcome: StepOutcome,
}

/// Esegue un passo con una conversazione nuova. `on_turn` riceve ogni turno appena concluso.
pub fn run_step(
    llm: &LlamaClient,
    session: &mut BrowserSession,
    cfg: &AgentConfig,
    input: &StepInput,
    on_turn: &mut dyn FnMut(&TurnLog),
) -> StepOutcome {
    let mut outcome = StepOutcome {
        end: EndReason::HarnessError,
        done: None,
        turns: 0,
        invalid_actions: 0,
        error: None,
    };
    let observation = match session.observe() {
        Ok(observation) => observation,
        Err(e) => {
            outcome.error = Some(format!("osservazione iniziale: {e}"));
            return outcome;
        }
    };
    let mut renderer = ObservationRenderer::new(cfg);
    let rendered = renderer.render(&observation, true);
    let conversation = Conversation::start(
        system_prompt(cfg.action_mode),
        step_header(input.test_data, input.completed, input.step_text),
        &rendered.text,
        cfg.snapshot_mode == SnapshotMode::Full,
    );
    let (tools, response_format) = match cfg.action_mode {
        ActionMode::Native => (Some(native_tools()), None),
        ActionMode::Schema => (None, Some(response_format())),
    };
    let shown = shown_from(llm, &rendered);
    let runner = Runner {
        llm,
        session,
        cfg,
        renderer,
        conversation,
        tools,
        response_format,
        shown,
        outcome,
    };
    runner.run(on_turn)
}

fn shown_from(llm: &LlamaClient, rendered: &RenderedObservation) -> Shown {
    Shown {
        snapshot_kind: rendered.snapshot_kind,
        snapshot_tokens: llm.tokenize(&rendered.snapshot_section).ok(),
    }
}

impl Runner<'_> {
    fn run(mut self, on_turn: &mut dyn FnMut(&TurnLog)) -> StepOutcome {
        for turn in 1..=self.cfg.max_actions {
            self.outcome.turns = turn;
            if let Some(end) = self.play_turn(turn, on_turn) {
                self.outcome.end = end;
                return self.outcome;
            }
        }
        self.outcome.end = EndReason::BudgetExhausted;
        self.outcome
    }

    /// Un turno; `Some` se il passo e' concluso.
    fn play_turn(&mut self, turn: u32, on_turn: &mut dyn FnMut(&TurnLog)) -> Option<EndReason> {
        let request = self.build_request();
        let started = Instant::now();
        let result = self.llm.chat(&request);
        let ctx = TurnContext {
            turn,
            snapshot_kind: self.shown.snapshot_kind,
            snapshot_tokens: self.shown.snapshot_tokens,
            prompt_messages: request.messages.len(),
            llm_ms: elapsed_ms(started),
        };
        let response = match result {
            Ok(response) => response,
            Err(e) => {
                on_turn(&TurnLog::llm_error(&ctx, &e));
                self.outcome.error = Some(e);
                return Some(EndReason::LlmError);
            }
        };
        let parsed = parse_response(self.cfg.action_mode, &response, &self.cfg.base_url);
        let mut log = TurnLog::from_response(&ctx, &response, &parsed);
        let end = match &parsed.action {
            Err(invalid) => {
                self.on_invalid(&response, &parsed, invalid);
                None
            }
            Ok(Action::Done { success, note }) => {
                self.outcome.done = Some(DoneReport {
                    success: *success,
                    note: note.clone(),
                });
                Some(EndReason::Done)
            }
            Ok(action) => self.on_action(&response, &parsed, action, &mut log),
        };
        on_turn(&log);
        end
    }

    fn build_request(&self) -> ChatRequest {
        ChatRequest {
            messages: self.conversation.messages().to_vec(),
            tools: self.tools.clone(),
            response_format: self.response_format.clone(),
            chat_template_kwargs: Some(json!({ "enable_thinking": self.cfg.thinking })),
            temperature: self.cfg.temperature,
            top_k: self.cfg.top_k,
            repeat_penalty: self.cfg.repeat_penalty,
            seed: self.cfg.seed,
            max_tokens: self.cfg.max_tokens,
        }
    }

    fn on_invalid(
        &mut self,
        response: &ChatResponse,
        parsed: &ParsedTurn,
        invalid: &InvalidAction,
    ) {
        self.outcome.invalid_actions += 1;
        let feedback = self.record_reply(response, parsed);
        self.conversation
            .push_notice(feedback, invalid_reply_notice(&invalid.message));
    }

    fn on_action(
        &mut self,
        response: &ChatResponse,
        parsed: &ParsedTurn,
        action: &Action,
        log: &mut TurnLog,
    ) -> Option<EndReason> {
        let feedback = self.record_reply(response, parsed);
        let started = Instant::now();
        let executed = execute(self.session, action, &self.cfg.base_url);
        let prefix = match &executed.result {
            Ok(()) => ok_prefix(),
            Err(e) => {
                log.outcome = format!("browser_error:{e}");
                result_error(&e.to_string())
            }
        };
        log.stale_ref_retry = executed.stale_ref_retry;
        log.settle_warning = executed.settle_warning;
        let observed = self.session.observe();
        log.browser_ms = elapsed_ms(started);
        match observed {
            Ok(observation) => {
                let rendered = self.renderer.render(&observation, false);
                self.shown = shown_from(self.llm, &rendered);
                self.conversation
                    .push_observed(feedback, prefix, &rendered.text);
                None
            }
            Err(e) => {
                log.outcome = format!("harness_error:{e}");
                self.outcome.error = Some(format!("osservazione dopo l'azione: {e}"));
                Some(EndReason::HarnessError)
            }
        }
    }

    /// Riporta la risposta del modello nella cronologia e dice come rispondere.
    fn record_reply(&mut self, response: &ChatResponse, parsed: &ParsedTurn) -> Feedback {
        let content = response.message.content.clone().unwrap_or_default();
        if self.cfg.action_mode == ActionMode::Schema {
            self.conversation.push_assistant(content);
            return Feedback::User;
        }
        match parsed
            .first_call
            .as_ref()
            .filter(|call| is_replayable(call))
        {
            Some(call) => {
                self.conversation.push_assistant_call(content, call.clone());
                Feedback::Tool(call.id.clone())
            }
            None => {
                self.conversation
                    .push_assistant(describe_reply(content, parsed.first_call.as_ref()));
                Feedback::User
            }
        }
    }
}

/// Una chiamata si puo' rimandare al server solo con id e argomenti JSON validi.
fn is_replayable(call: &ToolCall) -> bool {
    !call.id.is_empty()
        && serde_json::from_str::<Value>(&call.function.arguments).is_ok_and(|v| v.is_object())
}

fn describe_reply(content: String, call: Option<&ToolCall>) -> String {
    match call {
        Some(call) => format!(
            "{content}\n{}({})",
            call.function.name, call.function.arguments
        )
        .trim()
        .to_string(),
        None => content,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replayable_calls_need_id_and_json_object_arguments() {
        assert!(is_replayable(&ToolCall::new(
            "c1",
            "click",
            r#"{"ref":"e1"}"#
        )));
        assert!(!is_replayable(&ToolCall::new(
            "",
            "click",
            r#"{"ref":"e1"}"#
        )));
        assert!(!is_replayable(&ToolCall::new("c1", "click", "{ref: e1")));
        assert!(!is_replayable(&ToolCall::new("c1", "click", "[1]")));
    }

    #[test]
    fn unreplayable_call_is_described_as_text() {
        let call = ToolCall::new("c1", "click", "{ref");
        assert_eq!(describe_reply(String::new(), Some(&call)), "click({ref)");
        assert_eq!(describe_reply("hi".into(), None), "hi");
    }
}
