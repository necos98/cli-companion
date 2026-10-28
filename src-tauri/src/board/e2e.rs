//! Esito degli E2E di un task, verificato dall'app: l'ultima esecuzione
//! (`E2eRun`), l'impronta del codice su cui e' girata e il blocco della
//! consegna (`check_gate`). Funzioni pure, nessun I/O. Il contratto e' in
//! `docs/BOARD.MD`, "Test E2E".

use super::{Task, TaskKind, INTERRUPTED_NOTE};
use crate::automation::clip_chars;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// Nota del task completato quando il repository non ha piu' gli E2E
/// configurati: il blocco non si applica (la leggono anche i modelli).
pub const E2E_GATE_SKIPPED_NOTE: &str = "E2E check skipped: the repository no longer has E2E tests configured";

/// Lunghezza massima del testo d'errore di un'esecuzione.
pub const E2E_ERROR_MAX: usize = 1000;
/// Lunghezza massima della coda dell'output tenuta nell'esecuzione.
pub const E2E_OUTPUT_TAIL_MAX: usize = 4000;

const ESC: char = '\u{1b}';
const BEL: char = '\u{7}';
/// Byte finali di una sequenza CSI (`ESC [ ... <finale>`): da `@` a `~`.
const CSI_FINAL: std::ops::RangeInclusive<char> = '@'..='~';

/// Toglie le sequenze di escape ANSI dall'output dei comandi: CSI (colori,
/// movimenti del cursore), OSC (terminate da BEL o `ESC \`) e `ESC x` da due
/// caratteri. Una sequenza troncata a fine testo si scarta senza errori.
/// Va applicata prima di tagliare la coda, cosi' gli escape non consumano il budget.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != ESC {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if CSI_FINAL.contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == BEL {
                        break;
                    }
                    if c == ESC {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            // `ESC x`: l'altro carattere e' gia' consumato; `None` = ESC finale.
            _ => {}
        }
    }
    out
}

/// Test falliti elencati per nome nel rifiuto di `complete_task`.
const GATE_FAILED_NAMES: usize = 3;
/// Lunghezza massima dell'errore citato nel rifiuto di `complete_task`.
const GATE_ERROR_MAX: usize = 300;
/// Caratteri mostrati di un hash di commit.
const SHORT_COMMIT: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum E2eStatus {
    Running,
    Passed,
    Failed,
    /// Il comando non e' partito, e' scaduto, annullato, o il report non e' valido.
    Error,
}

impl E2eStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            E2eStatus::Running => "running",
            E2eStatus::Passed => "passed",
            E2eStatus::Failed => "failed",
            E2eStatus::Error => "error",
        }
    }
}

/// Codice su cui girano gli E2E: commit, albero e hash delle modifiche non
/// committate dei file tracciati (`git status --porcelain --untracked-files=no`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeFingerprint {
    pub commit: String,
    pub tree: String,
    pub changes: String,
}

impl CodeFingerprint {
    /// Stesso codice: stesso albero e stesse modifiche (uno squash cambia il
    /// commit ma non l'albero).
    pub fn same_code(&self, other: &Self) -> bool {
        self.tree == other.tree && self.changes == other.changes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct E2eTotals {
    pub tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flaky: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct E2eFailure {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub message: String,
}

/// Esito di un'esecuzione conclusa, prima di legarlo a un `E2eRun`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E2eOutcome {
    pub status: E2eStatus,
    pub totals: Option<E2eTotals>,
    pub failed_tests: Vec<E2eFailure>,
    pub error: Option<String>,
    pub output_tail: Option<String>,
}

impl E2eOutcome {
    /// Esecuzione finita male (`message` perde gli escape ANSI e si taglia a `E2E_ERROR_MAX`).
    pub fn error(message: &str, output_tail: Option<String>) -> E2eOutcome {
        E2eOutcome {
            status: E2eStatus::Error,
            totals: None,
            failed_tests: Vec::new(),
            error: Some(clip_chars(&strip_ansi(message), E2E_ERROR_MAX)),
            output_tail,
        }
    }
}

/// Un'esecuzione degli E2E. Quella corrente o l'ultima di un task sta in
/// `Task::e2e_run`; la copia nel report e' quella al momento della conclusione.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct E2eRun {
    pub run_id: String,
    pub status: E2eStatus,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Percorso del report JUnit configurato, relativo alla cartella.
    pub junit: String,
    /// Impronta del codice presa all'inizio dell'esecuzione (assente se git ha fallito).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<CodeFingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totals: Option<E2eTotals>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_tests: Vec<E2eFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tail: Option<String>,
}

impl E2eRun {
    pub fn started(run_id: String, junit: String, now: i64) -> E2eRun {
        E2eRun {
            run_id,
            status: E2eStatus::Running,
            started_at: now,
            finished_at: None,
            duration_ms: None,
            junit,
            code: None,
            totals: None,
            failed_tests: Vec::new(),
            error: None,
            output_tail: None,
        }
    }

    /// L'esecuzione conclusa con `outcome`.
    pub fn finished(self, outcome: E2eOutcome, code: Option<CodeFingerprint>, now: i64, duration_ms: u64) -> E2eRun {
        E2eRun {
            status: outcome.status,
            finished_at: Some(now),
            duration_ms: Some(duration_ms),
            code,
            totals: outcome.totals,
            failed_tests: outcome.failed_tests,
            error: outcome.error,
            output_tail: outcome.output_tail,
            ..self
        }
    }

    /// Un'esecuzione ancora `running` in un file salvato non esiste piu':
    /// l'app e' stata riavviata.
    pub(super) fn interrupt_if_running(&mut self) {
        if self.status == E2eStatus::Running {
            self.status = E2eStatus::Error;
            self.error = Some(INTERRUPTED_NOTE.to_string());
        }
    }
}

/// Esito del controllo di consegna di un task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// Il task non deve superare gli E2E (o non sta per essere completato).
    NotRequired,
    /// Ultima esecuzione passata sul codice consegnato.
    Passed,
    /// Il task e' marcato E2E ma il repository non li ha piu': non si blocca.
    ConfigRemoved,
}

fn reject(reason: &str) -> String {
    format!("complete_task rejected: {reason} The task stays in progress.")
}

fn cannot_verify(reason: &str) -> String {
    reject(&format!("cannot verify the code against the last E2E run: {}. Run run_e2e again.", clip_chars(reason, GATE_ERROR_MAX)))
}

fn short(commit: &str) -> String {
    commit.chars().take(SHORT_COMMIT).collect()
}

/// Un `complete_task` "completed" di un task `work` marcato E2E richiede
/// un'ultima esecuzione passata sul codice attuale. `current` e' l'impronta
/// del codice di adesso (`None` = non calcolata).
pub fn check_gate(
    task: &Task,
    completed: bool,
    repo_has_e2e: bool,
    current: Option<&Result<CodeFingerprint, String>>,
) -> Result<GateOutcome, String> {
    if !completed || task.kind != TaskKind::Work || !task.e2e {
        return Ok(GateOutcome::NotRequired);
    }
    if !repo_has_e2e {
        return Ok(GateOutcome::ConfigRemoved);
    }
    let run = task.e2e_run.as_ref().ok_or_else(|| {
        reject("this task requires a passed E2E run on the delivered code. Call run_e2e, then get_e2e_result until it finishes.")
    })?;
    match run.status {
        E2eStatus::Running => Err(reject(&format!(
            "the E2E run {} is still running. Wait for it with get_e2e_result, then call complete_task again.",
            run.run_id
        ))),
        E2eStatus::Failed => Err(reject(&failed_reason(run))),
        E2eStatus::Error => Err(reject(&format!(
            "the last E2E run ended with an error: {}. Fix it and run run_e2e again, or call complete_task with status \"failed\".",
            clip_chars(run.error.as_deref().unwrap_or("unknown error"), GATE_ERROR_MAX)
        ))),
        E2eStatus::Passed => verify_code(run, current).map(|()| GateOutcome::Passed),
    }
}

fn failed_reason(run: &E2eRun) -> String {
    let totals = run.totals.clone().unwrap_or_default();
    let names: Vec<&str> = run.failed_tests.iter().take(GATE_FAILED_NAMES).map(|f| f.name.as_str()).collect();
    let detail = if !names.is_empty() {
        format!(" ({})", names.join(", "))
    } else {
        run.error.as_deref().map(|e| format!(" ({})", clip_chars(e, GATE_ERROR_MAX))).unwrap_or_default()
    };
    format!(
        "the last E2E run failed: {} of {} tests failed{detail}. Fix the code, commit, run run_e2e again, or call complete_task with status \"failed\".",
        totals.failed, totals.tests
    )
}

/// L'esecuzione passata e' stata fatta sul codice che si sta consegnando?
fn verify_code(run: &E2eRun, current: Option<&Result<CodeFingerprint, String>>) -> Result<(), String> {
    let at_run = run.code.as_ref().ok_or_else(|| cannot_verify("the run recorded no code fingerprint"))?;
    let now = match current {
        Some(Ok(now)) => now,
        Some(Err(e)) => return Err(cannot_verify(e)),
        None => return Err(cannot_verify("the current code could not be read")),
    };
    if at_run.same_code(now) {
        return Ok(());
    }
    let dirty = if at_run.changes == now.changes { "" } else { ", uncommitted changes differ" };
    Err(reject(&format!(
        "the code changed after the last passed E2E run (run on commit {}, now {}{dirty}). Run run_e2e again on the current code.",
        short(&at_run.commit),
        short(&now.commit)
    )))
}
