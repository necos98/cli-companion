//! Report di `complete_task`: campi strutturati opzionali e loro validazione.

use super::{E2eRun, Verdict};
use crate::automation::{clip_chars, req_text};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const COMMIT_MAX: usize = 100;
const PR_URL_MAX: usize = 500;
const LIST_MAX: usize = 100;
const LIST_ITEM_MAX: usize = 2000;
const FILES_MAX: usize = 500;
const FILE_MAX: usize = 512;
const TESTS_MAX: usize = 50;
const TEST_COMMAND_MAX: usize = 1000;
pub const DIFFSTAT_MAX: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRun {
    pub command: String,
    /// passed | failed | not_run
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReport {
    /// completed | failed
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Link della PR (modalita' `pr`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub tests: Vec<TestRun>,
    #[serde(default)]
    pub follow_ups: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffstat: Option<String>,
    /// Esito di una revisione (solo task `review` completati).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// Problemi trovati da una revisione (suggerimenti se il verdetto e' `passed`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
    /// Ultima esecuzione degli E2E del task al momento della conclusione.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e2e: Option<E2eRun>,
}

/// Campi strutturati opzionali di `complete_task`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportFields {
    pub commit: Option<String>,
    pub pr_url: Option<String>,
    pub files_changed: Vec<String>,
    pub tests: Vec<TestRun>,
    pub follow_ups: Vec<String>,
    pub verdict: Option<Verdict>,
    pub findings: Vec<String>,
}

fn str_list(v: &Value, label: &str, max_items: usize, max_len: usize) -> Result<Vec<String>, String> {
    let arr = v.as_array().ok_or_else(|| format!("{label} must be an array of strings"))?;
    if arr.len() > max_items {
        return Err(format!("{label} has too many entries (max {max_items})"));
    }
    arr.iter()
        .map(|e| {
            let s = e.as_str().ok_or_else(|| format!("{label} must be an array of strings"))?.trim();
            if s.is_empty() || s.chars().count() > max_len {
                return Err(format!("{label}: entries must be 1..={max_len} characters"));
            }
            Ok(s.to_string())
        })
        .collect()
}

/// Stringa opzionale senza spazi ne' caratteri di controllo (hash, link).
fn token(p: &Value, key: &str, max: usize, what: &str) -> Result<Option<String>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else if s.chars().count() > max || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
                Err(format!("invalid {key} ({what}, no spaces)"))
            } else {
                Ok(Some(s.to_string()))
            }
        }
        Some(_) => Err(format!("{key} must be a string")),
    }
}

fn parse_tests(arr: &[Value]) -> Result<Vec<TestRun>, String> {
    if arr.len() > TESTS_MAX {
        return Err(format!("tests has too many entries (max {TESTS_MAX})"));
    }
    let mut out = Vec::new();
    for t in arr {
        let o: &Map<String, Value> = t.as_object().ok_or_else(|| "tests entries must be objects".to_string())?;
        let command = req_text("tests[].command", o.get("command").and_then(Value::as_str).unwrap_or(""), TEST_COMMAND_MAX)?;
        let result = o.get("result").and_then(Value::as_str).unwrap_or("");
        if !["passed", "failed", "not_run"].contains(&result) {
            return Err("tests[].result must be passed, failed or not_run".to_string());
        }
        let details = match o.get("details") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::String(s)) => Some(clip_chars(s.trim(), LIST_ITEM_MAX)),
            Some(_) => return Err("tests[].details must be a string".to_string()),
        };
        out.push(TestRun { command, result: result.to_string(), details });
    }
    Ok(out)
}

pub fn parse_report_fields(p: &Value) -> Result<ReportFields, String> {
    let nonnull = |k: &str| p.get(k).filter(|v| !v.is_null());
    let commit = token(p, "commit", COMMIT_MAX, "a hash or ref")?;
    let pr_url = token(p, "pr_url", PR_URL_MAX, "a link")?;
    if pr_url.as_deref().is_some_and(|u| !(u.starts_with("https://") || u.starts_with("http://"))) {
        return Err("invalid pr_url (an http(s) link)".to_string());
    }
    let files_changed = nonnull("files_changed").map(|v| str_list(v, "files_changed", FILES_MAX, FILE_MAX)).transpose()?;
    let follow_ups = nonnull("follow_ups").map(|v| str_list(v, "follow_ups", LIST_MAX, LIST_ITEM_MAX)).transpose()?;
    let verdict = match nonnull("verdict") {
        None => None,
        Some(Value::String(s)) => Some(Verdict::parse(s).ok_or("verdict must be passed or issues")?),
        Some(_) => return Err("verdict must be passed or issues".to_string()),
    };
    let findings = nonnull("findings").map(|v| str_list(v, "findings", LIST_MAX, LIST_ITEM_MAX)).transpose()?;
    let tests = match nonnull("tests") {
        None => Vec::new(),
        Some(Value::Array(arr)) => parse_tests(arr)?,
        Some(_) => return Err("tests must be an array".to_string()),
    };
    Ok(ReportFields {
        commit,
        pr_url,
        files_changed: files_changed.unwrap_or_default(),
        tests,
        follow_ups: follow_ups.unwrap_or_default(),
        verdict,
        findings: findings.unwrap_or_default(),
    })
}

pub fn build_report(status: &str, summary: &str, f: ReportFields, diffstat: Option<String>) -> TaskReport {
    TaskReport {
        status: status.to_string(),
        summary: summary.to_string(),
        commit: f.commit,
        pr_url: f.pr_url,
        files_changed: f.files_changed,
        tests: f.tests,
        follow_ups: f.follow_ups,
        diffstat: diffstat.map(|d| clip_chars(&d, DIFFSTAT_MAX)),
        verdict: f.verdict,
        findings: f.findings,
        e2e: None,
    }
}
