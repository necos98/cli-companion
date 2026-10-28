//! File di output della run: JSONL grezzi, riepilogo e metadati.

use crate::metrics::records::{ActionRecord, JsonlWriter, RunRecord, StepRecord};
use crate::metrics::summary::{summarize, to_markdown, ConfigData};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const ACTIONS_FILE: &str = "actions.jsonl";
const STEPS_FILE: &str = "steps.jsonl";
const RUNS_FILE: &str = "runs.jsonl";
const SUMMARY_JSON_FILE: &str = "summary.json";
const SUMMARY_MD_FILE: &str = "summary.md";
const META_FILE: &str = "meta.json";

pub struct Output {
    dir: PathBuf,
    actions: JsonlWriter,
    steps: JsonlWriter,
    runs: JsonlWriter,
}

impl Output {
    pub fn create(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("creazione di {}: {e}", dir.display()))?;
        Ok(Output {
            dir: dir.to_path_buf(),
            actions: JsonlWriter::create(&dir.join(ACTIONS_FILE))?,
            steps: JsonlWriter::create(&dir.join(STEPS_FILE))?,
            runs: JsonlWriter::create(&dir.join(RUNS_FILE))?,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Riscrive riepilogo JSON e Markdown con tutto cio' che e' stato raccolto finora.
    pub fn write_summary(&self, configs: &[ConfigData]) -> Result<(), String> {
        let summary = summarize(configs);
        let json =
            serde_json::to_string_pretty(&summary).map_err(|e| format!("summary.json: {e}"))?;
        self.write(SUMMARY_JSON_FILE, &json)?;
        self.write(SUMMARY_MD_FILE, &to_markdown(&summary))
    }

    pub fn write_meta(&self, meta: &Value) -> Result<(), String> {
        let json = serde_json::to_string_pretty(meta).map_err(|e| format!("meta.json: {e}"))?;
        self.write(META_FILE, &json)
    }

    fn write(&self, name: &str, content: &str) -> Result<(), String> {
        let path = self.dir.join(name);
        fs::write(&path, content).map_err(|e| format!("scrittura di {}: {e}", path.display()))
    }
}

/// Scrive un record su disco e lo tiene in memoria per il riepilogo.
pub struct Sink<'a> {
    pub output: &'a mut Output,
    pub data: &'a mut ConfigData,
}

impl Sink<'_> {
    pub fn action(&mut self, record: ActionRecord) -> Result<(), String> {
        self.output.actions.append(&record)?;
        self.data.actions.push(record);
        Ok(())
    }

    pub fn step(&mut self, record: StepRecord) -> Result<(), String> {
        self.output.steps.append(&record)?;
        self.data.steps.push(record);
        Ok(())
    }

    pub fn run(&mut self, record: RunRecord) -> Result<(), String> {
        self.output.runs.append(&record)?;
        self.data.runs.push(record);
        Ok(())
    }
}
