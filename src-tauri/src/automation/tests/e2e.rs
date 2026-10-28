//! E2E eseguiti dall'app: `run_e2e`, `get_e2e_result`, blocco di `complete_task`,
//! annullamento ed esecuzione manuale. Il comando e il report JUnit sono finti
//! (`hook_rules`, `hook_writes`); `hold_defers` tiene in coda il job per
//! osservare un'esecuzione in corso.

use super::*;
use crate::automation::e2e::manual;
use crate::board::E2E_GATE_SKIPPED_NOTE;
use crate::workspaces::E2eConfig;

pub(super) const COMMAND: &str = "npm run e2e";
const JUNIT: &str = "out/junit.xml";
pub(super) const JUNIT_PASS: &str = r#"<testsuites><testsuite name="s" tests="2"><testcase name="a"/><testcase name="b"/></testsuite></testsuites>"#;
const JUNIT_FAIL: &str = r#"<testsuites><testsuite name="s" tests="2"><testcase name="a"/>
    <testcase name="b" file="b.spec.ts"><failure message="boom"/></testcase></testsuite></testsuites>"#;
const JUNIT_EMPTY: &str = r#"<testsuites><testsuite name="s" tests="0"/></testsuites>"#;
pub(super) const STATUS_OUT: &str = " M src/a.rs\n";

/// Cartella di lavoro dei task senza worktree (`commit`) e con (`pr` parallelo).
pub(super) const CHECKOUT: &str = "C:\\p";
const WORKTREE: &str = "C:\\p\\.worktrees\\a";

pub(super) fn e2e_repo() -> RepoConfig {
    RepoConfig { e2e: Some(E2eConfig { command: COMMAND.into(), junit: JUNIT.into(), timeout_min: 5 }), ..Default::default() }
}

fn junit_at(dir: &str) -> String {
    format!("{dir}\\out\\junit.xml")
}

/// Fa rispondere git con questo codice (commit, albero, modifiche).
pub(super) fn set_code(h: &FakeHost, commit: &str, tree: &str, changes: &str) {
    h.git_rules.lock().unwrap().clear();
    h.git_rule("rev-parse HEAD HEAD^{tree}", true, &format!("{commit}\n{tree}\n"), "");
    h.git_rule("status --porcelain", true, changes, "");
}

/// Il comando E2E scrive questo report.
pub(super) fn writes_junit(h: &FakeHost, dir: &str, xml: &str) {
    h.hook_writes.lock().unwrap().push((COMMAND.to_string(), junit_at(dir), xml.to_string()));
}

/// Il comando E2E esce con `ok` e questo output.
fn command_exits(h: &FakeHost, ok: bool, stdout: &str) {
    h.hook_rules.lock().unwrap().push((COMMAND.to_string(), GitOut { ok, stdout: stdout.into(), stderr: String::new() }));
}

/// Un worker con il task `a` marcato E2E e il repository configurato; il
/// comando scrive `junit` (nessun report se `None`).
fn flagged_with(mode: DeliveryMode, parallel: bool, junit: Option<&str>) -> (AutomationState, FakeHost, String) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
    h.repo_cfg.lock().unwrap().insert(".".into(), e2e_repo());
    let mut a = spec("a", &[]);
    a["e2e"] = json!(true);
    queue(&st, &h, vec![a]);
    let w = session_of(&st, &h, "a");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    set_code(&h, "c1", "t1", STATUS_OUT);
    if let Some(xml) = junit {
        writes_junit(&h, if parallel { WORKTREE } else { CHECKOUT }, xml);
    }
    (st, h, w)
}

/// Come `flagged_with`, con un report che passa.
pub(super) fn flagged(mode: DeliveryMode, parallel: bool) -> (AutomationState, FakeHost, String) {
    flagged_with(mode, parallel, Some(JUNIT_PASS))
}

pub(super) fn run_e2e(st: &AutomationState, h: &FakeHost, w: &str) -> Value {
    call(st, h, w, "run_e2e", json!({})).unwrap()
}

fn result(st: &AutomationState, h: &FakeHost, w: &str) -> Value {
    call(st, h, w, "get_e2e_result", json!({})).unwrap()
}

fn complete_ok(st: &AutomationState, h: &FakeHost, w: &str) -> Result<Value, String> {
    call(st, h, w, "complete_task", json!({"status": "completed", "summary": "done"}))
}

pub(super) fn complete_failed(st: &AutomationState, h: &FakeHost, w: &str) -> Result<Value, String> {
    call(st, h, w, "complete_task", json!({"status": "failed", "summary": "gave up"}))
}

fn status_of(st: &AutomationState, h: &FakeHost) -> String {
    task(st, h, "a")["status"].as_str().unwrap().to_string()
}

/// Archivia il workspace come fa l'app: prima il flag, poi il registro.
pub(super) fn archive(st: &AutomationState, h: &FakeHost) {
    *h.archived.lock().unwrap() = true;
    st.run(h, |r| r.archive_workspace("ws-1", 1));
}

// --- blocco di complete_task ---

#[test]
fn a_flagged_task_completes_after_a_passed_run() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    assert_eq!(run_e2e(&st, &h, &w)["status"], "running");
    complete_ok(&st, &h, &w).unwrap();
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "done");
    assert_eq!(a["e2e_run"]["status"], "passed");
    let report = &a["report"]["e2e"];
    assert_eq!((report["status"].as_str(), report["totals"]["tests"].as_u64()), (Some("passed"), Some(2)));
    assert_eq!(report["run_id"], a["e2e_run"]["run_id"]);
    assert!(a["note"].is_null());
}

#[test]
fn a_flagged_task_cannot_complete_after_a_failed_run_but_can_fail() {
    let (st, h, w) = flagged_with(DeliveryMode::Commit, false, Some(JUNIT_FAIL));
    run_e2e(&st, &h, &w);
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.starts_with("complete_task rejected: the last E2E run failed: 1 of 2 tests failed (b)."), "{e}");
    assert!(e.ends_with("The task stays in progress."), "{e}");
    assert_eq!(status_of(&st, &h), "running");
    complete_failed(&st, &h, &w).unwrap();
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["report"]["e2e"]["status"].as_str()), (Some("failed"), Some("failed")));
}

#[test]
fn a_flagged_task_without_a_run_is_rejected() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.contains("requires a passed E2E run") && e.contains("Call run_e2e"), "{e}");
    assert_eq!(status_of(&st, &h), "running");
}

#[test]
fn a_flagged_task_with_a_running_run_is_rejected_until_it_ends() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    let run_id = run_e2e(&st, &h, &w)["run_id"].as_str().unwrap().to_string();
    assert_eq!(task(&st, &h, "a")["e2e_run"]["status"], "running");
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.contains(&format!("the E2E run {run_id} is still running")), "{e}");
    h.run_held();
    complete_ok(&st, &h, &w).unwrap();
    assert_eq!(status_of(&st, &h), "done");
}

#[test]
fn changed_code_rejects_but_a_squash_with_the_same_tree_is_accepted() {
    for (commit, tree, changes, accepted, expected) in [
        ("c1", "t2", STATUS_OUT, false, "the code changed after the last passed E2E run (run on commit c1, now c1)"),
        ("c1", "t1", " M src/b.rs\n", false, "now c1, uncommitted changes differ)"),
        ("c9", "t1", STATUS_OUT, true, ""),
    ] {
        let (st, h, w) = flagged(DeliveryMode::Commit, false);
        run_e2e(&st, &h, &w);
        set_code(&h, commit, tree, changes);
        match (accepted, complete_ok(&st, &h, &w)) {
            (true, r) => assert!(r.is_ok() && status_of(&st, &h) == "done", "{r:?}"),
            (false, Err(e)) => assert!(e.contains(expected) && status_of(&st, &h) == "running", "{e}"),
            (false, Ok(_)) => panic!("changed code must be rejected"),
        }
    }
}

#[test]
fn an_unreadable_fingerprint_rejects_the_completion() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    run_e2e(&st, &h, &w);
    h.git_rules.lock().unwrap().clear();
    h.git_rule("rev-parse HEAD HEAD^{tree}", false, "", "fatal: not a git repository");
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.contains("cannot verify the code against the last E2E run") && e.contains("not a git repository"), "{e}");
}

#[test]
fn a_run_without_a_fingerprint_cannot_be_verified() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.git_rules.lock().unwrap().clear();
    h.git_rule("rev-parse HEAD HEAD^{tree}", false, "", "fatal");
    run_e2e(&st, &h, &w);
    assert!(task(&st, &h, "a")["e2e_run"].get("code").is_none());
    set_code(&h, "c1", "t1", STATUS_OUT);
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.contains("the run recorded no code fingerprint"), "{e}");
}

#[test]
fn an_unflagged_task_completes_without_any_run_or_fingerprint() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    h.repo_cfg.lock().unwrap().insert(".".into(), e2e_repo());
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    complete_ok(&st, &h, &w).unwrap();
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "done");
    assert!(a.get("e2e_run").is_none() && a["report"].get("e2e").is_none());
    let git = h.git_log();
    assert!(git.iter().all(|l| !l.contains("status --porcelain") && !l.contains("HEAD^{tree}")), "{git:?}");
}

#[test]
fn removing_the_repository_config_lifts_the_gate_with_a_note() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.repo_cfg.lock().unwrap().clear();
    complete_ok(&st, &h, &w).unwrap();
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["note"].as_str()), (Some("done"), Some(E2E_GATE_SKIPPED_NOTE)));
}

// --- run_e2e ---

#[test]
fn run_e2e_needs_a_configured_repository_and_an_automated_session() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    let e = call(&st, &h, &w, "run_e2e", json!({})).unwrap_err();
    assert_eq!(e, "E2E tests are not configured for repository '.': ask the user to configure them in the repository settings");
    for m in ["run_e2e", "get_e2e_result"] {
        let e = call(&st, &h, INTAKE, m, json!({})).unwrap_err();
        assert_eq!(e, format!("{m} is reserved for automated sessions"));
    }
}

#[test]
fn a_second_run_in_the_same_folder_is_rejected_until_the_first_ends() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    let first = run_e2e(&st, &h, &w)["run_id"].as_str().unwrap().to_string();
    let e = call(&st, &h, &w, "run_e2e", json!({})).unwrap_err();
    assert_eq!(e, format!("an E2E run is already in progress in this folder (run_id {first}): wait for it with get_e2e_result"));
    assert_eq!(manual::run_repo(&st, &h, "ws-1", None).unwrap_err(), "esecuzione E2E gia' in corso per questo repository");
    h.run_held();
    assert_ne!(run_e2e(&st, &h, &w)["run_id"], json!(first));
}

#[test]
fn run_e2e_is_refused_once_the_task_is_no_longer_running() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    complete_failed(&st, &h, &w).unwrap();
    assert!(call(&st, &h, &w, "run_e2e", json!({})).is_err());
    assert!(st.e2e.reserved_ports().is_empty(), "the folder and the port are released on refusal");
    assert!(manual::run_repo(&st, &h, "ws-1", None).is_ok());
}

#[test]
fn a_reviewer_can_run_the_e2e_and_its_report_carries_the_run() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.repo_cfg.lock().unwrap().insert(".".into(), e2e_repo());
    let mut review = spec("rev", &["a"]);
    review["kind"] = json!("review");
    queue(&st, &h, vec![spec("a", &[]), review]);
    let a = session_of(&st, &h, "a");
    call(&st, &h, &a, "get_task", json!({})).unwrap();
    complete_ok(&st, &h, &a).unwrap();
    let r = session_of(&st, &h, "rev");
    call(&st, &h, &r, "get_task", json!({})).unwrap();
    set_code(&h, "c1", "t1", STATUS_OUT);
    writes_junit(&h, CHECKOUT, JUNIT_PASS);
    run_e2e(&st, &h, &r);
    call(&st, &h, &r, "complete_task", json!({"status": "completed", "summary": "ok", "verdict": "passed"})).unwrap();
    let rev = task(&st, &h, "rev");
    assert_eq!((rev["status"].as_str(), rev["report"]["e2e"]["status"].as_str()), (Some("done"), Some("passed")));
}

#[test]
fn the_old_report_is_removed_before_the_command_and_never_counts() {
    let (st, h, w) = flagged_with(DeliveryMode::Commit, false, None);
    h.set_text_file(&junit_at(CHECKOUT), JUNIT_PASS);
    run_e2e(&st, &h, &w);
    let log = h.hook_log.lock().unwrap().clone();
    let rm = log.iter().position(|l| l == &format!("rm {}", junit_at(CHECKOUT))).expect("old report removed");
    let run = log.iter().position(|l| l.starts_with(&format!("{CHECKOUT}|{COMMAND}|"))).expect("command run in the checkout");
    assert!(rm < run, "{log:?}");
    // Il comando non ha scritto un report nuovo: quello vecchio non vale.
    assert_eq!(result(&st, &h, &w)["error"], "JUnit report not found at out/junit.xml");
}

#[test]
fn the_command_runs_in_the_worktree_with_task_variables_and_a_port() {
    let (st, h, w) = flagged(DeliveryMode::Pr, true);
    run_e2e(&st, &h, &w);
    let log = h.hook_log.lock().unwrap().clone();
    let line = log.iter().find(|l| l.starts_with(&format!("{WORKTREE}|{COMMAND}|"))).unwrap_or_else(|| panic!("{log:?}"));
    let vars = line.rsplit('|').next().unwrap();
    for expected in [format!("WORKTREE={WORKTREE}"), "TASK_ID=a".to_string(), "BRANCH=task/a".to_string(), "REPO=C:\\p".to_string()] {
        assert!(vars.split(',').any(|v| v == expected), "{expected} in {vars}");
    }
    let port = vars.split(',').find_map(|v| v.strip_prefix("CLI_COMPANION_E2E_PORT=")).expect("port variable");
    assert!(port.parse::<u16>().is_ok_and(|p| p > 0), "{port}");
    assert!(log.contains(&format!("rm {}", junit_at(WORKTREE))));
    assert_eq!(result(&st, &h, &w)["status"], "passed");
}

// --- verdetto ---

#[test]
fn a_missing_report_is_an_error_with_the_output_tail() {
    let (st, h, w) = flagged_with(DeliveryMode::Commit, false, None);
    command_exits(&h, true, "server crashed");
    run_e2e(&st, &h, &w);
    let r = result(&st, &h, &w);
    assert_eq!((r["status"].as_str(), r["error"].as_str()), (Some("error"), Some("JUnit report not found at out/junit.xml")));
    assert!(r["output_tail"].as_str().unwrap().contains("server crashed"), "{r}");
    let e = complete_ok(&st, &h, &w).unwrap_err();
    assert!(e.contains("ended with an error: JUnit report not found"), "{e}");
}

#[test]
fn zero_tests_with_exit_zero_is_an_error() {
    let (st, h, w) = flagged_with(DeliveryMode::Commit, false, Some(JUNIT_EMPTY));
    run_e2e(&st, &h, &w);
    let r = result(&st, &h, &w);
    assert_eq!((r["status"].as_str(), r["error"].as_str()), (Some("error"), Some("the JUnit report contains no tests")));
}

#[test]
fn a_failing_exit_code_with_a_clean_report_is_failed() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    command_exits(&h, false, "");
    run_e2e(&st, &h, &w);
    let r = result(&st, &h, &w);
    assert_eq!(r["status"], "failed");
    assert_eq!(r["totals"]["failed"], 0);
    assert!(r["error"].as_str().unwrap().contains("exited with an error"));
    assert!(complete_ok(&st, &h, &w).unwrap_err().contains("0 of 2 tests failed (the command exited with an error"));
}

// --- get_e2e_result ---

#[test]
fn get_e2e_result_validates_its_parameters() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    for bad in [json!(26), json!(-1), json!("5"), json!(2.5)] {
        let e = call(&st, &h, &w, "get_e2e_result", json!({"wait_seconds": bad})).unwrap_err();
        assert_eq!(e, "wait_seconds must be an integer from 0 to 25", "{bad}");
    }
    assert!(call(&st, &h, &w, "get_e2e_result", json!({"run_id": 5})).unwrap_err().contains("run_id must be a string"));
    let e = call(&st, &h, &w, "get_e2e_result", json!({})).unwrap_err();
    assert_eq!(e, "no E2E run for this task: call run_e2e first");
    let run_id = run_e2e(&st, &h, &w)["run_id"].as_str().unwrap().to_string();
    let e = call(&st, &h, &w, "get_e2e_result", json!({"run_id": "e2e-0-0"})).unwrap_err();
    assert_eq!(e, format!("unknown run_id: the last E2E run of this task is {run_id}"));
    assert!(call(&st, &h, &w, "get_e2e_result", json!({"run_id": run_id, "wait_seconds": 25})).is_ok());
}

#[test]
fn get_e2e_result_returns_the_recorded_run() {
    let (st, h, w) = flagged_with(DeliveryMode::Commit, false, Some(JUNIT_FAIL));
    run_e2e(&st, &h, &w);
    let r = result(&st, &h, &w);
    assert_eq!(r["status"], "failed");
    assert_eq!(r["totals"], json!({"tests": 2, "passed": 1, "failed": 1, "skipped": 0}));
    assert_eq!(r["failed_tests"], json!([{"name": "b", "file": "b.spec.ts", "message": "boom"}]));
    assert_eq!((r["code"]["commit"].as_str(), r["junit"].as_str()), (Some("c1"), Some(JUNIT)));
    assert!(r["duration_ms"].is_u64() && r["finished_at"].is_i64() && r["started_at"].is_i64());
    // Visibile anche nella board e nei dettagli del task.
    assert_eq!(task(&st, &h, "a")["e2e_run"], r);
}

#[test]
fn get_e2e_result_waits_for_a_running_run() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    run_e2e(&st, &h, &w);
    assert_eq!(result(&st, &h, &w)["status"], "running");
    let finisher = {
        let h = h.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            h.run_held();
        })
    };
    let r = call(&st, &h, &w, "get_e2e_result", json!({"wait_seconds": 20})).unwrap();
    finisher.join().unwrap();
    assert_eq!(r["status"], "passed");
}
