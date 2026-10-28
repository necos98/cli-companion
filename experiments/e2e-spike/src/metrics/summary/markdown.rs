//! Rendering Markdown del riepilogo.

use super::{Aggregate, ConfigSummary, Summary};

const OVERALL_LABEL: &str = "ALL";
const MISSING: &str = "-";

pub fn to_markdown(summary: &Summary) -> String {
    let mut out = String::from("# Risultati esperimento E2E\n\n");
    out.push_str("## Confronto tra configurazioni\n\n");
    out.push_str(&outcome_table(
        summary
            .configs
            .iter()
            .map(|c| (c.config.as_str(), &c.overall)),
        "Config",
    ));
    for config in &summary.configs {
        out.push_str(&config_section(config));
    }
    out
}

fn config_section(config: &ConfigSummary) -> String {
    let mut out = format!("\n## Config `{}`\n\n", config.config);
    if let Some(error) = &config.not_runnable {
        out.push_str(&format!("Non eseguibile: {error}\n"));
        return out;
    }
    out.push_str(&resources_line(config));
    let rows: Vec<(&str, &Aggregate)> = std::iter::once((OVERALL_LABEL, &config.overall))
        .chain(
            config
                .scenarios
                .iter()
                .map(|s| (s.scenario.as_str(), &s.aggregate)),
        )
        .collect();
    out.push_str(&outcome_table(rows.iter().copied(), "Scenario"));
    out.push_str(&performance_table(&rows));
    out.push_str(&breakdown_lines(&config.overall));
    out
}

fn resources_line(config: &ConfigSummary) -> String {
    let vram = &config.resources.vram;
    format!(
        "Risorse: VRAM base {} / picco {} / delta {} MiB; RAM picco llama-server {} MiB{}\n\n",
        opt_u64(vram.baseline_mib),
        opt_u64(vram.peak_mib),
        opt_u64(config.resources.vram_delta_mib()),
        opt_f(config.resources.peak_ram_mib, 0),
        vram.error
            .as_ref()
            .map(|e| format!(" (errore VRAM: {e})"))
            .unwrap_or_default(),
    )
}

fn outcome_table<'a>(rows: impl Iterator<Item = (&'a str, &'a Aggregate)>, first: &str) -> String {
    let mut out = format!(
        "| {first} | Scenari ok | Step ok | Azioni/step media (mediana, p95) | Azioni non valide | Falsi successi | Falsi fallimenti | Tempo step mediana / p95 (ms) |\n\
         |---|---|---|---|---|---|---|---|\n"
    );
    for (label, a) in rows {
        out.push_str(&format!(
            "| {label} | {}/{} | {}/{} | {} ({}, {}) | {} ({}) | {} | {} | {} / {} |\n",
            a.runs_passed,
            a.runs_total,
            a.steps_passed,
            a.steps_total,
            opt_f(a.actions_per_step.mean, 2),
            opt_f(a.actions_per_step.median, 1),
            opt_f(a.actions_per_step.p95, 0),
            a.invalid_actions,
            percent(a.invalid_rate),
            a.false_successes,
            a.false_failures,
            opt_f(a.step_ms.median, 0),
            opt_f(a.step_ms.p95, 0),
        ));
    }
    out.push('\n');
    out
}

fn performance_table(rows: &[(&str, &Aggregate)]) -> String {
    let mut out = String::from(
        "| Scenario | Prompt token media / max | Token generati media | Prefill tok/s mediana | Generazione tok/s mediana | Token snapshot mediana / p95 / max |\n\
         |---|---|---|---|---|---|\n",
    );
    for (label, a) in rows {
        out.push_str(&format!(
            "| {label} | {} / {} | {} | {} | {} | {} / {} / {} |\n",
            opt_f(a.prompt_tokens.mean, 0),
            opt_f(a.prompt_tokens.max, 0),
            opt_f(a.generated_tokens.mean, 0),
            opt_f(a.prefill_tokens_per_s.median, 0),
            opt_f(a.generation_tokens_per_s.median, 0),
            opt_f(a.snapshot_tokens.median, 0),
            opt_f(a.snapshot_tokens.p95, 0),
            opt_f(a.snapshot_tokens.max, 0),
        ));
    }
    out.push('\n');
    out
}

fn breakdown_lines(a: &Aggregate) -> String {
    format!(
        "Motivi di chiusura dei passi: {}\n\nAzioni non valide per tipo: {}\n\nTurni con piu' chiamate native: {}\n",
        counts(&a.end_reasons),
        counts(&a.invalid_by_kind),
        a.multiple_call_turns,
    )
}

fn counts(map: &std::collections::BTreeMap<String, usize>) -> String {
    if map.is_empty() {
        return MISSING.to_string();
    }
    map.iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn opt_f(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(|| MISSING.to_string(), |v| format!("{v:.decimals$}"))
}

fn opt_u64(value: Option<u64>) -> String {
    value.map_or_else(|| MISSING.to_string(), |v| v.to_string())
}

fn percent(rate: Option<f64>) -> String {
    rate.map_or_else(|| MISSING.to_string(), |r| format!("{:.1}%", r * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::summary::{Aggregate, ConfigSummary, ResourceUsage, ScenarioSummary};

    fn config(id: &str, not_runnable: Option<&str>) -> ConfigSummary {
        let overall = Aggregate {
            runs_total: 5,
            runs_passed: 4,
            invalid_rate: Some(0.125),
            ..Aggregate::default()
        };
        ConfigSummary {
            config: id.into(),
            not_runnable: not_runnable.map(str::to_string),
            resources: ResourceUsage::default(),
            overall: overall.clone(),
            scenarios: vec![ScenarioSummary {
                scenario: "S1".into(),
                aggregate: overall,
            }],
        }
    }

    #[test]
    fn renders_comparison_and_per_config_tables() {
        let md = to_markdown(&Summary {
            configs: vec![config("base", None)],
        });
        assert!(md.contains("## Confronto tra configurazioni"));
        assert!(md.contains("| base | 4/5 |"));
        assert!(md.contains("## Config `base`"));
        assert!(md.contains("| S1 | 4/5 |"));
        assert!(md.contains("12.5%"));
    }

    #[test]
    fn not_runnable_config_shows_the_error_only() {
        let md = to_markdown(&Summary {
            configs: vec![config("q4_k_m", Some("modello mancante"))],
        });
        assert!(md.contains("Non eseguibile: modello mancante"));
        assert!(!md.contains("Risorse:"));
    }
}
