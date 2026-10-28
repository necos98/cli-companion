//! Record dei risultati grezzi e scrittura JSONL (una riga per record, flush immediato).

use crate::agent::{DoneReport, EndReason, TurnLog};
use crate::fixture::state::StateSnapshot;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Un turno del modello (riga di `actions.jsonl`).
#[derive(Serialize, Debug)]
pub struct ActionRecord {
    pub config: String,
    pub scenario: String,
    pub run: u32,
    pub step: usize,
    #[serde(flatten)]
    pub turn: TurnLog,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StepEndReason {
    Done,
    BudgetExhausted,
    LlmError,
    HarnessError,
    Skipped,
}

impl From<EndReason> for StepEndReason {
    fn from(reason: EndReason) -> Self {
        match reason {
            EndReason::Done => StepEndReason::Done,
            EndReason::BudgetExhausted => StepEndReason::BudgetExhausted,
            EndReason::LlmError => StepEndReason::LlmError,
            EndReason::HarnessError => StepEndReason::HarnessError,
        }
    }
}

impl StepEndReason {
    pub fn label(self) -> &'static str {
        match self {
            StepEndReason::Done => "done",
            StepEndReason::BudgetExhausted => "budget_exhausted",
            StepEndReason::LlmError => "llm_error",
            StepEndReason::HarnessError => "harness_error",
            StepEndReason::Skipped => "skipped",
        }
    }
}

/// Un passo di uno scenario (riga di `steps.jsonl`). `step` parte da 1.
#[derive(Serialize, Debug)]
pub struct StepRecord {
    pub config: String,
    pub scenario: String,
    pub run: u32,
    pub step: usize,
    pub text: String,
    pub success: bool,
    pub failed_checks: Vec<String>,
    pub end_reason: StepEndReason,
    pub done: Option<DoneReport>,
    pub false_success: bool,
    pub false_failure: bool,
    pub actions: u32,
    pub invalid_actions: u32,
    pub duration_ms: u64,
    pub url_after: Option<String>,
    pub state_after: Option<StateSnapshot>,
    pub error: Option<String>,
}

/// Una esecuzione di scenario (riga di `runs.jsonl`).
#[derive(Serialize, Debug)]
pub struct RunRecord {
    pub config: String,
    pub scenario: String,
    pub run: u32,
    pub success: bool,
    pub steps_passed: usize,
    pub steps_total: usize,
    pub duration_ms: u64,
}

pub struct JsonlWriter {
    file: File,
}

impl JsonlWriter {
    pub fn create(path: &Path) -> Result<Self, String> {
        let file =
            File::create(path).map_err(|e| format!("creazione di {}: {e}", path.display()))?;
        Ok(JsonlWriter { file })
    }

    pub fn append<T: Serialize>(&mut self, record: &T) -> Result<(), String> {
        let mut line =
            serde_json::to_string(record).map_err(|e| format!("serializzazione: {e}"))?;
        line.push('\n');
        self.file
            .write_all(line.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|e| format!("scrittura JSONL: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_one_json_object_per_line() {
        let path =
            std::env::temp_dir().join(format!("e2e-spike-jsonl-{}.jsonl", std::process::id()));
        let mut writer = JsonlWriter::create(&path).unwrap();
        let record = RunRecord {
            config: "base".into(),
            scenario: "S1".into(),
            run: 1,
            success: true,
            steps_passed: 1,
            steps_total: 1,
            duration_ms: 5,
        };
        writer.append(&record).unwrap();
        writer.append(&record).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        let value: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(value["scenario"], "S1");
    }

    #[test]
    fn end_reasons_map_and_label() {
        assert_eq!(
            StepEndReason::from(EndReason::BudgetExhausted).label(),
            "budget_exhausted"
        );
        assert_eq!(StepEndReason::Skipped.label(), "skipped");
    }
}
