//! Verdetto di un'esecuzione E2E, deciso dall'app e senza modelli: passa solo
//! se il comando esce con 0 E il report JUnit esiste, ha almeno un test e
//! nessun fallimento. Piu' l'impronta del codice (`git`).

use super::junit::{self, JunitReport};
use crate::automation::hooks::tail;
use crate::automation::worktrees::GitOut;
use crate::board::{CodeFingerprint, E2eFailure, E2eOutcome, E2eStatus, E2eTotals, E2E_OUTPUT_TAIL_MAX};
use crate::board::strip_ansi;

/// Offset e primo di FNV-1a a 64 bit: stabile fra versioni di Rust (a
/// differenza di `DefaultHasher`).
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a a 64 bit, in 16 cifre esadecimali.
fn fnv1a_hex(text: &str) -> String {
    let hash = text.bytes().fold(FNV_OFFSET, |h, b| (h ^ u64::from(b)).wrapping_mul(FNV_PRIME));
    format!("{hash:016x}")
}

/// Impronta da `git rev-parse HEAD HEAD^{tree}` (due righe: commit e albero)
/// e da `git status --porcelain --untracked-files=no`.
pub(super) fn parse_fingerprint(rev_parse: &str, porcelain: &str) -> Result<CodeFingerprint, String> {
    let mut lines = rev_parse.lines().map(str::trim).filter(|l| !l.is_empty());
    match (lines.next(), lines.next()) {
        (Some(commit), Some(tree)) => {
            Ok(CodeFingerprint { commit: commit.to_string(), tree: tree.to_string(), changes: fnv1a_hex(porcelain) })
        }
        _ => Err("git rev-parse did not return a commit and a tree".to_string()),
    }
}

fn output_tail(out: &GitOut) -> Option<String> {
    let text = tail(&strip_ansi(&format!("{}\n{}", out.stdout.trim(), out.stderr.trim())), E2E_OUTPUT_TAIL_MAX);
    (!text.is_empty()).then_some(text)
}

fn totals_of(report: &JunitReport) -> E2eTotals {
    E2eTotals {
        tests: report.tests,
        passed: report.passed(),
        failed: report.failed,
        skipped: report.skipped,
        flaky: report.flaky,
    }
}

fn failures_of(report: &JunitReport) -> Vec<E2eFailure> {
    report
        .failures
        .iter()
        .map(|f| E2eFailure { name: f.name.clone(), file: f.file.clone(), message: f.message.clone() })
        .collect()
}

/// Esito di un'esecuzione. `read_junit` legge il report e si chiama solo se il
/// comando e' partito e non e' stato interrotto (timeout, annullamento).
pub(super) fn evaluate(
    run: Result<GitOut, String>,
    read_junit: impl FnOnce() -> Result<Option<String>, String>,
    junit_path: &str,
) -> E2eOutcome {
    let out = match run {
        Ok(out) => out,
        Err(e) => return E2eOutcome::error(&e, None),
    };
    let tail = output_tail(&out);
    let raw = match read_junit() {
        Ok(Some(raw)) => raw,
        Ok(None) => {
            let why = if out.ok { "" } else { " (the command exited with an error)" };
            return E2eOutcome::error(&format!("JUnit report not found at {junit_path}{why}"), tail);
        }
        Err(e) => return E2eOutcome::error(&format!("JUnit report {junit_path} could not be read: {e}"), tail),
    };
    let report = match junit::parse(&raw) {
        Ok(report) => report,
        Err(e) => return E2eOutcome::error(&format!("JUnit report {junit_path} is not valid: {e}"), tail),
    };
    if report.tests == 0 {
        return E2eOutcome::error("the JUnit report contains no tests", tail);
    }
    let mut outcome = E2eOutcome {
        status: E2eStatus::Passed,
        totals: Some(totals_of(&report)),
        failed_tests: failures_of(&report),
        error: None,
        output_tail: None,
    };
    if report.failed > 0 {
        outcome.status = E2eStatus::Failed;
    } else if !out.ok {
        outcome.status = E2eStatus::Failed;
        outcome.error = Some("the command exited with an error although the report has no failures".to_string());
        outcome.output_tail = tail;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    const JUNIT_OK: &str = r#"<testsuites><testsuite name="s" tests="2"><testcase name="a"/><testcase name="b"/></testsuite></testsuites>"#;
    const JUNIT_FAIL: &str = r#"<testsuites><testsuite name="s" tests="2"><testcase name="a"/>
        <testcase name="b" file="b.spec.ts"><failure message="boom"/></testcase></testsuite></testsuites>"#;
    const JUNIT_EMPTY: &str = r#"<testsuites><testsuite name="s" tests="0"/></testsuites>"#;

    fn out(ok: bool) -> GitOut {
        GitOut { ok, stdout: "out line".into(), stderr: "err line".into() }
    }

    fn eval(run: Result<GitOut, String>, junit: Result<Option<String>, String>) -> E2eOutcome {
        evaluate(run, || junit, "out/junit.xml")
    }

    fn some(xml: &str) -> Result<Option<String>, String> {
        Ok(Some(xml.to_string()))
    }

    #[test]
    fn passes_only_with_exit_zero_and_a_clean_report() {
        let o = eval(Ok(out(true)), some(JUNIT_OK));
        assert_eq!(o.status, E2eStatus::Passed);
        let totals = o.totals.unwrap();
        assert_eq!((totals.tests, totals.passed, totals.failed, totals.skipped), (2, 2, 0, 0));
        assert!(o.error.is_none() && o.output_tail.is_none() && o.failed_tests.is_empty());
    }

    #[test]
    fn a_command_that_did_not_run_is_an_error_without_reading_the_report() {
        let o = evaluate(Err("timed out after 60 s".into()), || panic!("must not read"), "j.xml");
        assert_eq!((o.status, o.error.as_deref()), (E2eStatus::Error, Some("timed out after 60 s")));
        assert!(o.totals.is_none());
    }

    #[test]
    fn a_missing_report_is_an_error_with_the_output_tail() {
        let o = eval(Ok(out(true)), Ok(None));
        assert_eq!(o.status, E2eStatus::Error);
        assert_eq!(o.error.as_deref(), Some("JUnit report not found at out/junit.xml"));
        assert_eq!(o.output_tail.as_deref(), Some("out line\nerr line"));
        let o = eval(Ok(out(false)), Ok(None));
        assert_eq!(o.error.as_deref(), Some("JUnit report not found at out/junit.xml (the command exited with an error)"));
    }

    #[test]
    fn an_unreadable_or_invalid_report_is_an_error() {
        let o = eval(Ok(out(true)), Err("file too large".into()));
        assert_eq!(o.error.as_deref(), Some("JUnit report out/junit.xml could not be read: file too large"));
        assert!(o.output_tail.is_some());
        let o = eval(Ok(out(true)), some("not xml <"));
        assert_eq!(o.status, E2eStatus::Error);
        assert!(o.error.unwrap().starts_with("JUnit report out/junit.xml is not valid: "));
    }

    #[test]
    fn a_report_without_tests_is_an_error_even_with_exit_zero() {
        let o = eval(Ok(out(true)), some(JUNIT_EMPTY));
        assert_eq!((o.status, o.error.as_deref()), (E2eStatus::Error, Some("the JUnit report contains no tests")));
        assert!(o.output_tail.is_some());
    }

    #[test]
    fn failures_in_the_report_fail_the_run_whatever_the_exit_code() {
        for ok in [true, false] {
            let o = eval(Ok(out(ok)), some(JUNIT_FAIL));
            assert_eq!(o.status, E2eStatus::Failed);
            assert_eq!(o.totals.as_ref().map(|t| (t.tests, t.passed, t.failed)), Some((2, 1, 1)));
            assert_eq!(o.failed_tests.len(), 1);
            assert_eq!((o.failed_tests[0].file.as_deref(), o.failed_tests[0].message.as_str()), (Some("b.spec.ts"), "boom"));
            assert!(o.output_tail.is_none());
        }
    }

    #[test]
    fn a_failing_exit_code_with_a_clean_report_fails_the_run() {
        let o = eval(Ok(out(false)), some(JUNIT_OK));
        assert_eq!(o.status, E2eStatus::Failed);
        assert_eq!(o.error.as_deref(), Some("the command exited with an error although the report has no failures"));
        assert_eq!(o.totals.unwrap().tests, 2);
        assert!(o.output_tail.is_some());
    }

    #[test]
    fn the_output_tail_keeps_the_end_and_is_bounded() {
        let big = GitOut { ok: true, stdout: format!("{}END", "x".repeat(E2E_OUTPUT_TAIL_MAX * 2)), stderr: String::new() };
        let o = eval(Ok(big), Ok(None));
        let t = o.output_tail.unwrap();
        assert!(t.ends_with("END") && t.starts_with("...") && t.chars().count() <= E2E_OUTPUT_TAIL_MAX + 3);
        let quiet = GitOut { ok: true, stdout: String::new(), stderr: String::new() };
        assert!(eval(Ok(quiet), Ok(None)).output_tail.is_none());
    }

    #[test]
    fn the_output_tail_has_no_ansi_escapes() {
        let noisy = GitOut { ok: true, stdout: "\u{1b}[1A\u{1b}[2Krunning".into(), stderr: "\u{1b}[31mboom\u{1b}[0m".into() };
        assert_eq!(eval(Ok(noisy), Ok(None)).output_tail.as_deref(), Some("running\nboom"));
    }

    #[test]
    fn fingerprint_is_parsed_from_git_output() {
        let f = parse_fingerprint("abc123\ndef456\n", " M src/a.rs\n").unwrap();
        assert_eq!((f.commit.as_str(), f.tree.as_str()), ("abc123", "def456"));
        assert_eq!(f.changes.len(), 16);
        assert_eq!(f.changes, parse_fingerprint("x\ny\n", " M src/a.rs\n").unwrap().changes);
        assert_ne!(f.changes, parse_fingerprint("abc123\ndef456\n", " M src/b.rs\n").unwrap().changes);
        assert!(parse_fingerprint("abc123\n", "").is_err());
        assert!(parse_fingerprint("", "").is_err());
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a_hex(""), "cbf29ce484222325");
        assert_eq!(fnv1a_hex("a"), "af63dc4c8601ec8c");
        assert_eq!(fnv1a_hex("foobar"), "85944171f73967e8");
    }
}
