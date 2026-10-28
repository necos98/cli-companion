//! Una esecuzione di uno scenario: preparazione del fixture, passi, verdetto, record.

use super::config::RunConfig;
use super::fixture_client::FixtureClient;
use super::output::Sink;
use crate::agent::SnapshotMode;
use crate::agent::{open_page, run_step, DoneReport, EndReason, StepInput, StepOutcome};
use crate::browser::session::BrowserSession;
use crate::browser::AgentBrowser;
use crate::fixture::state::{SeedAccount, StateSnapshot};
use crate::llama::client::LlamaClient;
use crate::metrics::records::{ActionRecord, RunRecord, StepEndReason, StepRecord};
use crate::scenario::test_data::{slug, TestData, KEY_EMAIL, KEY_NAME, KEY_PASSWORD};
use crate::scenario::verdict::{judge, CheckContext, Judgement, Verdict};
use crate::scenario::{Scenario, ScenarioStep, Seed};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Il seed di campionamento di una esecuzione e' `SEED_BASE + run`.
const SEED_BASE: u64 = 1000;
/// Pagina da cui parte ogni esecuzione (come il `baseURL` di un runner E2E).
const START_PATH: &str = "/";
/// Tentativi totali per aprire la pagina iniziale (l'apertura puo' fallire in modo transitorio).
const START_ATTEMPTS: u32 = 3;
/// Pausa tra due tentativi di apertura della pagina iniziale.
const START_RETRY_PAUSE: Duration = Duration::from_millis(500);
const HARNESS_CHECK: &str = "harness_observation";

pub struct ScenarioRun<'a> {
    pub config: &'a RunConfig,
    pub scenario: &'a Scenario,
    pub run: u32,
    pub llm: &'a LlamaClient,
    pub fixture: &'a FixtureClient,
    pub fixture_url: &'a str,
    pub browser_exe: &'a Path,
    pub headed: bool,
}

/// Pagina e stato del fixture dopo un passo.
struct After {
    state: StateSnapshot,
    url: String,
    page_text: String,
}

pub fn execute(job: &ScenarioRun, sink: &mut Sink) -> Result<(), String> {
    let data = TestData::generate(job.config.id, job.scenario.id, job.run, job.scenario.extras);
    job.fixture.reset()?;
    if job.scenario.seed == Seed::TestAccount {
        job.fixture.seed(&SeedAccount {
            name: data.get(KEY_NAME).to_string(),
            email: data.get(KEY_EMAIL).to_string(),
            password: data.get(KEY_PASSWORD).to_string(),
        })?;
    }
    let session_name = slug(
        &format!("e2e-{}-{}-{}", job.config.id, job.scenario.id, job.run),
        '-',
    );
    let browser = AgentBrowser::new(job.browser_exe.to_path_buf(), session_name, job.headed);
    let mut session = BrowserSession::new(browser, job.config.snapshot_mode == SnapshotMode::Delta);
    let started = Instant::now();
    // Un errore di apertura salta i passi ma non la chiusura del browser.
    let steps =
        open_start_page(job, &mut session).and_then(|()| run_steps(job, &data, &mut session, sink));
    // La chiusura va tentata anche dopo un errore; un suo fallimento non cambia l'esito.
    let _ = session.browser().close();
    let passed = steps?;
    sink.run(RunRecord {
        config: job.config.id.to_string(),
        scenario: job.scenario.id.to_string(),
        run: job.run,
        success: passed == job.scenario.steps.len(),
        steps_passed: passed,
        steps_total: job.scenario.steps.len(),
        duration_ms: elapsed_ms(started),
    })
}

/// Porta il browser sulla home del sito, cosi' il primo passo non parte da `about:blank`.
/// E' setup dell'harness: un abort transitorio del browser viene ritentato, mentre l'`open`
/// del modello resta a tentativo singolo.
fn open_start_page(job: &ScenarioRun, session: &mut BrowserSession) -> Result<(), String> {
    retry(START_ATTEMPTS, START_RETRY_PAUSE, || {
        open_page(session, START_PATH, job.fixture_url)
    })
    .map_err(|e| {
        format!(
            "apertura della pagina iniziale {START_PATH} (dopo {START_ATTEMPTS} tentativi): {e}"
        )
    })
}

/// Esegue `f` fino a `attempts` volte con `pause` tra i tentativi; restituisce l'ultimo errore.
fn retry<T>(
    attempts: u32,
    pause: Duration,
    mut f: impl FnMut() -> Result<T, String>,
) -> Result<T, String> {
    let mut last = String::from("nessun tentativo eseguito");
    for attempt in 1..=attempts {
        match f() {
            Ok(value) => return Ok(value),
            Err(e) => last = e,
        }
        if attempt < attempts {
            sleep(pause);
        }
    }
    Err(last)
}

/// Esegue i passi finche' uno fallisce; i restanti sono `skipped`. Restituisce quanti sono passati.
fn run_steps(
    job: &ScenarioRun,
    data: &TestData,
    session: &mut BrowserSession,
    sink: &mut Sink,
) -> Result<usize, String> {
    let mut completed: Vec<String> = Vec::new();
    for (index, step) in job.scenario.steps.iter().enumerate() {
        let number = index + 1;
        let failed_before = completed.len() < index;
        if failed_before {
            sink.step(skipped_record(job, number, step))?;
            continue;
        }
        if run_one_step(job, data, session, sink, number, step, &completed)? {
            completed.push(step.text.to_string());
        }
    }
    Ok(completed.len())
}

fn run_one_step(
    job: &ScenarioRun,
    data: &TestData,
    session: &mut BrowserSession,
    sink: &mut Sink,
    number: usize,
    step: &ScenarioStep,
    completed: &[String],
) -> Result<bool, String> {
    let before = job.fixture.state()?;
    let agent_cfg = job
        .config
        .agent_config(job.fixture_url, SEED_BASE + u64::from(job.run));
    let input = StepInput {
        test_data: data.entries(),
        completed,
        step_text: step.text,
    };
    let started = Instant::now();
    let mut write_error: Option<String> = None;
    let outcome = run_step(job.llm, session, &agent_cfg, &input, &mut |turn| {
        let record = ActionRecord {
            config: job.config.id.to_string(),
            scenario: job.scenario.id.to_string(),
            run: job.run,
            step: number,
            turn: turn.clone(),
        };
        if write_error.is_none() {
            write_error = sink.action(record).err();
        }
    });
    if let Some(error) = write_error {
        return Err(error);
    }
    let after = observe_after(job, session);
    let duration_ms = elapsed_ms(started);
    let (judgement, end_reason) = match &after {
        Ok(after) => (
            judge(step, &context(&before, after, outcome.done.as_ref(), data)),
            StepEndReason::from(outcome.end),
        ),
        Err(_) => (harness_failure(), StepEndReason::HarnessError),
    };
    let passed = judgement.verdict.passed;
    let evaluated = Evaluated {
        judgement,
        end_reason,
        duration_ms,
    };
    let record = step_record(job, number, step, &outcome, &evaluated, after);
    println!(
        "  [{}] {} #{} passo {number}: {} ({} azioni, {} ms)",
        job.config.id,
        job.scenario.id,
        job.run,
        if passed { "ok" } else { "FALLITO" },
        record.actions,
        duration_ms
    );
    sink.step(record)?;
    Ok(passed)
}

fn observe_after(job: &ScenarioRun, session: &BrowserSession) -> Result<After, String> {
    Ok(After {
        state: job.fixture.state()?,
        url: session.browser().url().map_err(|e| e.to_string())?,
        page_text: session.browser().page_text().map_err(|e| e.to_string())?,
    })
}

fn context<'a>(
    before: &'a StateSnapshot,
    after: &'a After,
    done: Option<&'a DoneReport>,
    data: &'a TestData,
) -> CheckContext<'a> {
    CheckContext {
        before,
        after: &after.state,
        url: &after.url,
        page_text: &after.page_text,
        done,
        data,
    }
}

/// Verdetto e motivo di chiusura di un passo concluso.
struct Evaluated {
    judgement: Judgement,
    end_reason: StepEndReason,
    duration_ms: u64,
}

/// Non si riesce a leggere pagina o stato: il passo non puo' essere giudicato.
fn harness_failure() -> Judgement {
    Judgement {
        verdict: Verdict {
            passed: false,
            failed_checks: vec![HARNESS_CHECK.to_string()],
        },
        false_success: false,
        false_failure: false,
    }
}

fn step_record(
    job: &ScenarioRun,
    number: usize,
    step: &ScenarioStep,
    outcome: &StepOutcome,
    evaluated: &Evaluated,
    after: Result<After, String>,
) -> StepRecord {
    let judgement = &evaluated.judgement;
    let error = match (&after, outcome.end) {
        (Err(e), _) => Some(e.clone()),
        (Ok(_), EndReason::LlmError | EndReason::HarnessError) => outcome.error.clone(),
        _ => None,
    };
    let (url_after, state_after) = match after {
        Ok(after) => (Some(after.url), Some(after.state)),
        Err(_) => (None, None),
    };
    StepRecord {
        config: job.config.id.to_string(),
        scenario: job.scenario.id.to_string(),
        run: job.run,
        step: number,
        text: step.text.to_string(),
        success: judgement.verdict.passed,
        failed_checks: judgement.verdict.failed_checks.clone(),
        end_reason: evaluated.end_reason,
        done: outcome.done.clone(),
        false_success: judgement.false_success,
        false_failure: judgement.false_failure,
        actions: outcome.turns,
        invalid_actions: outcome.invalid_actions,
        duration_ms: evaluated.duration_ms,
        url_after,
        state_after,
        error,
    }
}

fn skipped_record(job: &ScenarioRun, number: usize, step: &ScenarioStep) -> StepRecord {
    StepRecord {
        config: job.config.id.to_string(),
        scenario: job.scenario.id.to_string(),
        run: job.run,
        step: number,
        text: step.text.to_string(),
        success: false,
        failed_checks: Vec::new(),
        end_reason: StepEndReason::Skipped,
        done: None,
        false_success: false,
        false_failure: false,
        actions: 0,
        invalid_actions: 0,
        duration_ms: 0,
        url_after: None,
        state_after: None,
        error: None,
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_succeeds_on_second_attempt() {
        let mut calls = 0;
        let result = retry(3, Duration::ZERO, || {
            calls += 1;
            if calls < 2 {
                Err("abort".to_string())
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result, Ok(2));
    }

    #[test]
    fn retry_returns_last_error_after_all_attempts() {
        let mut calls = 0;
        let result: Result<(), String> = retry(3, Duration::ZERO, || {
            calls += 1;
            Err(format!("errore {calls}"))
        });
        assert_eq!(result, Err("errore 3".to_string()));
        assert_eq!(calls, 3);
    }
}
