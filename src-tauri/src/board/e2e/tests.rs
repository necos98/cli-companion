use super::*;
use crate::board::{Board, TaskReport, TaskStatus};
use serde_json::json;

fn fp(commit: &str, tree: &str, changes: &str) -> CodeFingerprint {
    CodeFingerprint { commit: commit.to_string(), tree: tree.to_string(), changes: changes.to_string() }
}

fn run(status: E2eStatus) -> E2eRun {
    let mut r = E2eRun::started("e2e-1-1".to_string(), "out/junit.xml".to_string(), 10);
    r.status = status;
    r
}

fn passed_run(code: Option<CodeFingerprint>) -> E2eRun {
    E2eRun { code, totals: Some(E2eTotals { tests: 5, passed: 5, ..E2eTotals::default() }), ..run(E2eStatus::Passed) }
}

fn flagged(run: Option<E2eRun>) -> Task {
    Task { id: "a".into(), e2e: true, e2e_run: run, ..Task::default() }
}

fn gate(task: &Task, current: &Result<CodeFingerprint, String>) -> Result<GateOutcome, String> {
    check_gate(task, true, true, Some(current))
}

#[test]
fn same_code_ignores_the_commit_but_not_tree_or_changes() {
    let a = fp("c1", "t1", "h1");
    assert!(a.same_code(&fp("c2", "t1", "h1")));
    assert!(!a.same_code(&fp("c1", "t2", "h1")));
    assert!(!a.same_code(&fp("c1", "t1", "h2")));
}

#[test]
fn run_round_trips_and_omits_empty_fields() {
    let started = E2eRun::started("e2e-1-1".into(), "j.xml".into(), 5);
    let v = serde_json::to_value(&started).unwrap();
    assert_eq!(v, json!({"run_id": "e2e-1-1", "status": "running", "started_at": 5, "junit": "j.xml"}));
    let finished = started.finished(
        E2eOutcome {
            status: E2eStatus::Failed,
            totals: Some(E2eTotals { tests: 3, passed: 1, failed: 1, skipped: 1, flaky: Some(1) }),
            failed_tests: vec![E2eFailure { name: "t".into(), file: Some("a.spec.ts".into()), message: "boom".into() }],
            error: None,
            output_tail: None,
        },
        Some(fp("c", "t", "h")),
        9,
        4,
    );
    assert_eq!((finished.finished_at, finished.duration_ms), (Some(9), Some(4)));
    let back: E2eRun = serde_json::from_value(serde_json::to_value(&finished).unwrap()).unwrap();
    assert_eq!(back, finished);
    assert_eq!(E2eStatus::Failed.as_str(), "failed");
}

#[test]
fn error_outcome_clips_the_message() {
    let o = E2eOutcome::error(&"x".repeat(E2E_ERROR_MAX + 50), Some("tail".into()));
    assert_eq!(o.status, E2eStatus::Error);
    assert_eq!(o.error.unwrap().chars().count(), E2E_ERROR_MAX);
    assert_eq!(o.output_tail.as_deref(), Some("tail"));
}

#[test]
fn strip_ansi_removes_colours_and_cursor_moves() {
    assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m ok"), "red ok");
    assert_eq!(strip_ansi("a\n\u{1b}[1A\u{1b}[2K[1/1] slow"), "a\n[1/1] slow");
}

#[test]
fn strip_ansi_removes_osc_hyperlinks_and_two_char_escapes() {
    assert_eq!(strip_ansi("\u{1b}]8;;http://x\u{7}text\u{1b}]8;;\u{7}"), "text");
    assert_eq!(strip_ansi("\u{1b}]0;title\u{1b}\\body"), "body");
    assert_eq!(strip_ansi("a\u{1b}Mb\u{1b}=c"), "abc");
}

#[test]
fn strip_ansi_keeps_plain_and_unicode_text() {
    assert_eq!(strip_ansi("plain\ttext\n"), "plain\ttext\n");
    assert_eq!(strip_ansi("\u{1b}[32mcaffè › テスト\u{1b}[0m"), "caffè › テスト");
}

#[test]
fn strip_ansi_survives_truncated_escapes() {
    for tail in ["\u{1b}", "\u{1b}[", "\u{1b}[31", "\u{1b}]8;;http://x", "\u{1b}]0;t\u{1b}"] {
        assert_eq!(strip_ansi(&format!("ok{tail}")), "ok");
    }
}

#[test]
fn error_outcome_strips_ansi_before_clipping() {
    let o = E2eOutcome::error("cancelled\n\u{1b}[1A\u{1b}[2K[1/1] hangs", None);
    assert_eq!(o.error.as_deref(), Some("cancelled\n[1/1] hangs"));
}

#[test]
fn old_json_without_the_e2e_fields_loads() {
    let raw = r#"{"workspace_id":"w","tasks":[{"id":"a","title":"A","task":"t","status":"done",
        "report":{"status":"completed","summary":"s"}}]}"#;
    let b = Board::from_json(raw, 1).unwrap();
    let t = b.task("a").unwrap();
    assert!(t.e2e_run.is_none() && t.report.as_ref().unwrap().e2e.is_none());
    let out = serde_json::to_value(t).unwrap();
    assert!(out.get("e2e_run").is_none() && out["report"].get("e2e").is_none());
}

#[test]
fn loading_turns_a_running_run_into_an_interrupted_error() {
    let mut b = Board::new("w");
    let mut t = flagged(Some(run(E2eStatus::Running)));
    t.report = Some(TaskReport {
        status: "failed".into(),
        summary: "s".into(),
        e2e: Some(run(E2eStatus::Running)),
        ..serde_json::from_value(json!({"status": "failed", "summary": "s"})).unwrap()
    });
    t.status = TaskStatus::Failed;
    b.tasks.push(t);
    b.tasks.push(flagged(Some(passed_run(None))));
    b.tasks[1].id = "b".into();
    let loaded = Board::from_json(&serde_json::to_string(&b).unwrap(), 1).unwrap();
    let a = loaded.task("a").unwrap();
    for r in [a.e2e_run.as_ref().unwrap(), a.report.as_ref().unwrap().e2e.as_ref().unwrap()] {
        assert_eq!((r.status, r.error.as_deref()), (E2eStatus::Error, Some(INTERRUPTED_NOTE)));
    }
    assert_eq!(loaded.task("b").unwrap().e2e_run.as_ref().unwrap().status, E2eStatus::Passed);
}

#[test]
fn a_new_attempt_forgets_the_previous_run() {
    let mut b = Board::new("w");
    b.tasks.push(flagged(Some(passed_run(None))));
    b.start_task("a", 5);
    assert!(b.task("a").unwrap().e2e_run.is_none());
}

#[test]
fn gate_does_not_apply_without_the_flag_the_work_kind_or_a_completion() {
    let plain = Task { e2e: false, ..flagged(None) };
    assert_eq!(check_gate(&plain, true, true, None), Ok(GateOutcome::NotRequired));
    assert_eq!(check_gate(&flagged(None), false, true, None), Ok(GateOutcome::NotRequired));
    let review = Task { kind: TaskKind::Review, ..flagged(None) };
    assert_eq!(check_gate(&review, true, true, None), Ok(GateOutcome::NotRequired));
}

#[test]
fn gate_is_lifted_when_the_repository_lost_its_e2e_config() {
    assert_eq!(check_gate(&flagged(None), true, false, None), Ok(GateOutcome::ConfigRemoved));
}

#[test]
fn gate_rejects_without_a_run_with_instructions() {
    let e = gate(&flagged(None), &Ok(fp("c", "t", "h"))).unwrap_err();
    assert_eq!(
        e,
        "complete_task rejected: this task requires a passed E2E run on the delivered code. Call run_e2e, then get_e2e_result until it finishes. The task stays in progress."
    );
}

#[test]
fn gate_rejects_while_running() {
    let e = gate(&flagged(Some(run(E2eStatus::Running))), &Ok(fp("c", "t", "h"))).unwrap_err();
    assert!(e.contains("the E2E run e2e-1-1 is still running") && e.ends_with("The task stays in progress."), "{e}");
}

#[test]
fn gate_rejects_a_failed_run_naming_at_most_three_tests() {
    let names = ["one", "two", "three", "four"];
    let r = E2eRun {
        totals: Some(E2eTotals { tests: 9, passed: 5, failed: 4, ..E2eTotals::default() }),
        failed_tests: names.iter().map(|n| E2eFailure { name: n.to_string(), file: None, message: "m".into() }).collect(),
        ..run(E2eStatus::Failed)
    };
    let e = gate(&flagged(Some(r)), &Ok(fp("c", "t", "h"))).unwrap_err();
    assert!(e.contains("4 of 9 tests failed (one, two, three)."), "{e}");
    assert!(!e.contains("four") && e.contains("status \"failed\""), "{e}");
}

#[test]
fn gate_rejects_a_failed_run_without_failures_citing_the_error() {
    let r = E2eRun {
        totals: Some(E2eTotals { tests: 9, passed: 9, ..E2eTotals::default() }),
        error: Some("the command exited with an error".into()),
        ..run(E2eStatus::Failed)
    };
    let e = gate(&flagged(Some(r)), &Ok(fp("c", "t", "h"))).unwrap_err();
    assert!(e.contains("0 of 9 tests failed (the command exited with an error)."), "{e}");
}

#[test]
fn gate_rejects_an_errored_run_with_a_clipped_error() {
    let r = E2eRun { error: Some("e".repeat(1000)), ..run(E2eStatus::Error) };
    let e = gate(&flagged(Some(r)), &Ok(fp("c", "t", "h"))).unwrap_err();
    assert!(e.contains("ended with an error: eee") && e.contains(&"e".repeat(GATE_ERROR_MAX)), "{e}");
    assert!(!e.contains(&"e".repeat(GATE_ERROR_MAX + 1)), "{e}");
}

#[test]
fn gate_accepts_a_passed_run_on_the_same_code_even_after_a_squash() {
    let t = flagged(Some(passed_run(Some(fp("aaaaaaaa", "tree", "h")))));
    assert_eq!(gate(&t, &Ok(fp("aaaaaaaa", "tree", "h"))), Ok(GateOutcome::Passed));
    assert_eq!(gate(&t, &Ok(fp("bbbbbbbb", "tree", "h"))), Ok(GateOutcome::Passed));
}

#[test]
fn gate_rejects_changed_code_naming_the_commits() {
    let t = flagged(Some(passed_run(Some(fp("aaaaaaaa1", "tree1", "h")))));
    let e = gate(&t, &Ok(fp("bbbbbbbb2", "tree2", "h"))).unwrap_err();
    assert!(e.contains("(run on commit aaaaaaa, now bbbbbbb). Run run_e2e again on the current code."), "{e}");
    let e = gate(&t, &Ok(fp("aaaaaaaa1", "tree1", "other"))).unwrap_err();
    assert!(e.contains("now aaaaaaa, uncommitted changes differ)"), "{e}");
}

#[test]
fn gate_rejects_when_the_code_cannot_be_verified() {
    let no_code = flagged(Some(passed_run(None)));
    let e = gate(&no_code, &Ok(fp("c", "t", "h"))).unwrap_err();
    assert!(e.contains("cannot verify the code against the last E2E run: the run recorded no code fingerprint. Run run_e2e again."), "{e}");
    let t = flagged(Some(passed_run(Some(fp("c", "t", "h")))));
    let e = gate(&t, &Err("git is not available".into())).unwrap_err();
    assert!(e.contains("cannot verify the code against the last E2E run: git is not available."), "{e}");
    let e = check_gate(&t, true, true, None).unwrap_err();
    assert!(e.contains("the current code could not be read"), "{e}");
}
