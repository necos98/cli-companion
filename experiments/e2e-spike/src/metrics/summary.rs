//! Riepilogo dei risultati: per configurazione e per configurazione x scenario.

mod markdown;
mod stats;

pub use markdown::to_markdown;
pub use stats::Dist;

use super::gpu::VramStats;
use super::records::{ActionRecord, RunRecord, StepEndReason, StepRecord};
use serde::Serialize;
use std::collections::BTreeMap;

const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;

/// Risorse misurate durante una configurazione.
#[derive(Serialize, Clone, Debug, Default)]
pub struct ResourceUsage {
    pub vram: VramStats,
    pub peak_ram_mib: Option<f64>,
}

impl ResourceUsage {
    pub fn peak_ram_from_bytes(bytes: Option<u64>) -> Option<f64> {
        bytes.map(|b| b as f64 / BYTES_PER_MIB)
    }

    pub fn vram_delta_mib(&self) -> Option<u64> {
        match (self.vram.baseline_mib, self.vram.peak_mib) {
            (Some(base), Some(peak)) => Some(peak.saturating_sub(base)),
            _ => None,
        }
    }
}

/// Tutti i record di una configurazione, in memoria.
pub struct ConfigData {
    pub id: String,
    pub not_runnable: Option<String>,
    pub resources: ResourceUsage,
    pub actions: Vec<ActionRecord>,
    pub steps: Vec<StepRecord>,
    pub runs: Vec<RunRecord>,
}

impl ConfigData {
    pub fn new(id: &str) -> Self {
        ConfigData {
            id: id.to_string(),
            not_runnable: None,
            resources: ResourceUsage::default(),
            actions: Vec::new(),
            steps: Vec::new(),
            runs: Vec::new(),
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Aggregate {
    pub runs_total: usize,
    pub runs_passed: usize,
    pub steps_total: usize,
    pub steps_passed: usize,
    pub steps_skipped: usize,
    pub actions_per_step: Dist,
    pub total_actions: usize,
    pub invalid_actions: usize,
    pub invalid_rate: Option<f64>,
    pub invalid_by_kind: BTreeMap<String, usize>,
    pub multiple_call_turns: usize,
    pub false_successes: usize,
    pub false_failures: usize,
    pub step_ms: Dist,
    pub prompt_tokens: Dist,
    pub generated_tokens: Dist,
    pub prefill_tokens_per_s: Dist,
    pub generation_tokens_per_s: Dist,
    pub snapshot_tokens: Dist,
    pub end_reasons: BTreeMap<String, usize>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ScenarioSummary {
    pub scenario: String,
    pub aggregate: Aggregate,
}

#[derive(Serialize, Clone, Debug)]
pub struct ConfigSummary {
    pub config: String,
    pub not_runnable: Option<String>,
    pub resources: ResourceUsage,
    pub overall: Aggregate,
    pub scenarios: Vec<ScenarioSummary>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Summary {
    pub configs: Vec<ConfigSummary>,
}

pub fn summarize(configs: &[ConfigData]) -> Summary {
    Summary {
        configs: configs.iter().map(summarize_config).collect(),
    }
}

fn summarize_config(data: &ConfigData) -> ConfigSummary {
    let mut unique: Vec<&str> = Vec::new();
    for run in &data.runs {
        if !unique.contains(&run.scenario.as_str()) {
            unique.push(&run.scenario);
        }
    }
    let scenarios = unique
        .into_iter()
        .map(|id| ScenarioSummary {
            scenario: id.to_string(),
            aggregate: aggregate(data, Some(id)),
        })
        .collect();
    ConfigSummary {
        config: data.id.clone(),
        not_runnable: data.not_runnable.clone(),
        resources: data.resources.clone(),
        overall: aggregate(data, None),
        scenarios,
    }
}

fn aggregate(data: &ConfigData, scenario: Option<&str>) -> Aggregate {
    let keep = |s: &str| scenario.is_none_or(|wanted| wanted == s);
    let actions: Vec<&ActionRecord> = data.actions.iter().filter(|a| keep(&a.scenario)).collect();
    let steps: Vec<&StepRecord> = data.steps.iter().filter(|s| keep(&s.scenario)).collect();
    let runs: Vec<&RunRecord> = data.runs.iter().filter(|r| keep(&r.scenario)).collect();
    let mut aggregate = Aggregate {
        runs_total: runs.len(),
        runs_passed: runs.iter().filter(|r| r.success).count(),
        ..Aggregate::default()
    };
    fill_steps(&mut aggregate, &steps);
    fill_actions(&mut aggregate, &actions);
    aggregate
}

fn fill_steps(aggregate: &mut Aggregate, steps: &[&StepRecord]) {
    let executed: Vec<&&StepRecord> = steps
        .iter()
        .filter(|s| s.end_reason != StepEndReason::Skipped)
        .collect();
    aggregate.steps_total = steps.len();
    aggregate.steps_passed = steps.iter().filter(|s| s.success).count();
    aggregate.steps_skipped = steps.len() - executed.len();
    aggregate.actions_per_step =
        Dist::from_values(executed.iter().map(|s| f64::from(s.actions)).collect());
    aggregate.step_ms = Dist::from_values(executed.iter().map(|s| s.duration_ms as f64).collect());
    aggregate.false_successes = steps.iter().filter(|s| s.false_success).count();
    aggregate.false_failures = steps.iter().filter(|s| s.false_failure).count();
    for step in steps {
        *aggregate
            .end_reasons
            .entry(step.end_reason.label().to_string())
            .or_default() += 1;
    }
}

fn fill_actions(aggregate: &mut Aggregate, actions: &[&ActionRecord]) {
    aggregate.total_actions = actions.len();
    for action in actions {
        if let Some(kind) = action.turn.invalid_kind {
            aggregate.invalid_actions += 1;
            *aggregate
                .invalid_by_kind
                .entry(kind.label().to_string())
                .or_default() += 1;
        }
        if action.turn.multiple_calls {
            aggregate.multiple_call_turns += 1;
        }
    }
    if !actions.is_empty() {
        aggregate.invalid_rate = Some(aggregate.invalid_actions as f64 / actions.len() as f64);
    }
    // Solo i turni con una risposta del modello hanno token e velocita'.
    let answered: Vec<&&ActionRecord> = actions
        .iter()
        .filter(|a| a.turn.usage.prompt_tokens > 0)
        .collect();
    aggregate.prompt_tokens = Dist::from_values(
        answered
            .iter()
            .map(|a| a.turn.usage.prompt_tokens as f64)
            .collect(),
    );
    aggregate.generated_tokens = Dist::from_values(
        answered
            .iter()
            .map(|a| a.turn.usage.completion_tokens as f64)
            .collect(),
    );
    aggregate.prefill_tokens_per_s = Dist::from_values(positive(
        answered.iter().map(|a| a.turn.timings.prompt_per_second),
    ));
    aggregate.generation_tokens_per_s = Dist::from_values(positive(
        answered.iter().map(|a| a.turn.timings.predicted_per_second),
    ));
    aggregate.snapshot_tokens = Dist::from_values(
        actions
            .iter()
            .filter_map(|a| a.turn.snapshot_tokens)
            .map(|n| n as f64)
            .collect(),
    );
}

fn positive(values: impl Iterator<Item = f64>) -> Vec<f64> {
    values.filter(|v| *v > 0.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::action::InvalidKind;
    use crate::agent::{DoneReport, TurnLog};
    use crate::llama::client::{Timings, Usage};

    fn turn(invalid: Option<InvalidKind>, prompt: u64, snapshot: usize) -> TurnLog {
        TurnLog {
            turn: 1,
            content: None,
            tool_calls: None,
            reasoning: None,
            action: None,
            invalid_kind: invalid,
            invalid_message: None,
            outcome: "ok".into(),
            stale_ref_retry: false,
            multiple_calls: false,
            settle_warning: None,
            llm_ms: 10,
            browser_ms: 5,
            usage: Usage {
                prompt_tokens: prompt,
                completion_tokens: 20,
            },
            timings: Timings {
                prompt_per_second: 1000.0,
                predicted_per_second: 100.0,
                ..Timings::default()
            },
            snapshot_kind: "full".into(),
            snapshot_tokens: Some(snapshot),
            prompt_messages: 3,
            finish_reason: None,
        }
    }

    fn action(scenario: &str, invalid: Option<InvalidKind>, prompt: u64) -> ActionRecord {
        ActionRecord {
            config: "c".into(),
            scenario: scenario.into(),
            run: 1,
            step: 1,
            turn: turn(invalid, prompt, 100),
        }
    }

    fn step(
        scenario: &str,
        success: bool,
        end: StepEndReason,
        actions: u32,
        ms: u64,
    ) -> StepRecord {
        StepRecord {
            config: "c".into(),
            scenario: scenario.into(),
            run: 1,
            step: 1,
            text: "t".into(),
            success,
            failed_checks: vec![],
            end_reason: end,
            done: Some(DoneReport {
                success,
                note: String::new(),
            }),
            false_success: !success && end == StepEndReason::Done,
            false_failure: false,
            actions,
            invalid_actions: 0,
            duration_ms: ms,
            url_after: None,
            state_after: None,
            error: None,
        }
    }

    fn run(scenario: &str, success: bool) -> RunRecord {
        RunRecord {
            config: "c".into(),
            scenario: scenario.into(),
            run: 1,
            success,
            steps_passed: 0,
            steps_total: 1,
            duration_ms: 1,
        }
    }

    fn data() -> ConfigData {
        ConfigData {
            id: "c".into(),
            not_runnable: None,
            resources: ResourceUsage::default(),
            actions: vec![
                action("S1", None, 1000),
                action("S1", Some(InvalidKind::InvalidRef), 2000),
                action("S2", None, 3000),
            ],
            steps: vec![
                step("S1", true, StepEndReason::Done, 2, 100),
                step("S2", false, StepEndReason::Done, 1, 300),
                step("S2", false, StepEndReason::Skipped, 0, 0),
            ],
            runs: vec![run("S1", true), run("S2", false)],
        }
    }

    #[test]
    fn overall_aggregate_counts_everything() {
        let summary = summarize(&[data()]);
        let all = &summary.configs[0].overall;
        assert_eq!((all.runs_passed, all.runs_total), (1, 2));
        assert_eq!(
            (all.steps_passed, all.steps_total, all.steps_skipped),
            (1, 3, 1)
        );
        assert_eq!(all.total_actions, 3);
        assert_eq!(all.invalid_actions, 1);
        assert_eq!(all.invalid_by_kind.get("invalid_ref"), Some(&1));
        assert_eq!(all.false_successes, 1);
        assert_eq!(all.actions_per_step.mean, Some(1.5));
        assert_eq!(all.prompt_tokens.max, Some(3000.0));
        assert_eq!(all.snapshot_tokens.median, Some(100.0));
        assert_eq!(all.end_reasons.get("skipped"), Some(&1));
        assert!((all.invalid_rate.unwrap() - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn scenarios_are_split_in_run_order() {
        let summary = summarize(&[data()]);
        let ids: Vec<&str> = summary.configs[0]
            .scenarios
            .iter()
            .map(|s| s.scenario.as_str())
            .collect();
        assert_eq!(ids, vec!["S1", "S2"]);
        assert_eq!(summary.configs[0].scenarios[0].aggregate.total_actions, 2);
        assert_eq!(summary.configs[0].scenarios[1].aggregate.steps_skipped, 1);
    }

    #[test]
    fn vram_delta_needs_both_samples() {
        let mut usage = ResourceUsage::default();
        assert_eq!(usage.vram_delta_mib(), None);
        usage.vram.baseline_mib = Some(1000);
        usage.vram.peak_mib = Some(4300);
        assert_eq!(usage.vram_delta_mib(), Some(3300));
        assert_eq!(
            ResourceUsage::peak_ram_from_bytes(Some(1_048_576)),
            Some(1.0)
        );
    }
}
