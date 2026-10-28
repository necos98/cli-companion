//! Task di revisione: validazione, ruolo del revisore, strumenti concessi,
//! `get_task`, verdetto di `complete_task`, righe di `list_tasks` e pulizia.

use super::*;
use crate::board::texts::{REVIEWER_INSTRUCTIONS, WORKER_GUARDRAILS};

fn review_spec(id: &str, deps: &[&str]) -> Value {
    let mut s = spec(id, deps);
    s["kind"] = json!("review");
    s
}

/// Board con il task `a` e la revisione `rev` che lo verifica; `a` finisce
/// con `report` e la revisione parte. Ritorna la sessione del revisore.
fn started_review(st: &AutomationState, h: &FakeHost, report: Value) -> String {
    queue(st, h, vec![spec("a", &[]), review_spec("rev", &["a"])]);
    assert_eq!(task(st, h, "rev")["status"], "queued");
    let a = session_of(st, h, "a");
    call(st, h, &a, "get_task", json!({})).unwrap();
    call(st, h, &a, "complete_task", report).unwrap();
    session_of(st, h, "rev")
}

fn plain_report() -> Value {
    json!({"status": "completed", "summary": "done a"})
}

fn reviewer(mode: DeliveryMode, parallel: bool) -> (AutomationState, FakeHost, String) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
    let r = started_review(&st, &h, plain_report());
    (st, h, r)
}

fn complete(st: &AutomationState, h: &FakeHost, session: &str, extra: Value) -> Result<Value, String> {
    let mut params = json!({"status": "completed", "summary": "reviewed"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    call(st, h, session, "complete_task", params)
}

#[test]
fn proposals_and_manual_tasks_validate_the_kind() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[]), spec("c", &[]), review_spec("rev", &["a"])]);
    assert_eq!(task(&st, &h, "rev")["kind"], "review");
    assert_eq!(task(&st, &h, "a")["kind"], "work");
    let err = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [review_spec("r2", &[])]})).unwrap_err();
    assert!(err.contains("dependency"), "{err}");
    let mut odd = spec("x", &[]);
    odd["kind"] = json!("audit");
    let err = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [odd]})).unwrap_err();
    assert!(err.contains("invalid kind 'audit'"), "{err}");
    // `c` e' in coda e senza dipendenze: non puo' diventare una revisione.
    assert_eq!(task(&st, &h, "c")["status"], "queued");
    let err = call(&st, &h, INTAKE, "update_task", json!({"id": "c", "kind": "review"})).unwrap_err();
    assert!(err.contains("dependency"), "{err}");
    assert_eq!(task(&st, &h, "c")["kind"], "work");
    call(&st, &h, INTAKE, "update_task", json!({"id": "c", "kind": "review", "depends_on": ["a"]})).unwrap();
    assert_eq!(task(&st, &h, "c")["kind"], "review");
    // Creazione manuale (UI).
    let manual = |deps: Value| json!({"title": "Check", "task": "verify", "kind": "review", "depends_on": deps});
    let err = boards::task_create(&st, &h, "ws-1", &manual(json!([]))).unwrap_err();
    assert!(err.contains("dependency"), "{err}");
    boards::task_create(&st, &h, "ws-1", &manual(json!(["a"]))).unwrap();
    assert_eq!(task(&st, &h, "check")["kind"], "review");
}

#[test]
fn a_review_starts_after_its_dependencies_and_its_worker_is_a_reviewer() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[]), review_spec("rev", &["a"])]);
    assert_eq!(task(&st, &h, "rev")["status"], "queued");
    let a = session_of(&st, &h, "a");
    assert_eq!(call(&st, &h, &a, "whoami", json!({})).unwrap()["role"], "worker");
    finish(&st, &h, &a, "completed");
    assert_eq!(task(&st, &h, "rev")["status"], "running");
    let r = session_of(&st, &h, "rev");
    let me = call(&st, &h, &r, "whoami", json!({})).unwrap();
    assert_eq!(me["role"], "reviewer");
    assert!(me.get("workspace").is_none());
}

#[test]
fn only_the_reviewer_gets_the_two_session_tools() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    let a = session_of_done(&st, &h);
    // Un worker normale non le ha.
    for m in ["get_task_details", "propose_tasks"] {
        let e = call(&st, &h, &a, m, json!({"id": "a"})).unwrap_err();
        assert!(e.contains("not available to automated sessions"), "{m}: {e}");
    }
    // Il revisore legge la specifica dei task e propone.
    let d = call(&st, &h, &r, "get_task_details", json!({"id": "a"})).unwrap();
    assert_eq!(d["task"]["task"], "do a");
    assert_eq!(d["task"]["id"], "a");
    let p = call(&st, &h, &r, "propose_tasks", json!({"tasks": [spec("fix", &[])]})).unwrap();
    assert_eq!(p["status"], "pending");
    assert!(call(&st, &h, &r, "get_task_details", json!({"id": "nope"})).unwrap_err().contains("task not found"));
    // Tutto il resto resta vietato.
    for m in ["list_tasks", "update_task", "delete_task", "get_workspace_settings", "propose_settings", "list_skills", "save_skill", "delete_skill"] {
        let e = call(&st, &h, &r, m, json!({"id": "a"})).unwrap_err();
        assert!(e.contains("not available to automated sessions"), "{m}: {e}");
    }
    assert!(call(&st, &h, &r, "get_events", json!({})).unwrap_err().contains("unknown method"));
}

/// Sessione (chiusa) del task `a` gia' concluso: serve solo come chiamante worker.
fn session_of_done(st: &AutomationState, h: &FakeHost) -> String {
    st.run(h, |r| r.workers.values().find(|w| w.task_id == "a").map(|w| w.session.clone())).expect("worker di a")
}

#[test]
fn proposals_remember_the_review_that_made_them() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    call(&st, &h, &r, "propose_tasks", json!({"tasks": [spec("fix", &[]), review_spec("rev2", &["fix"])]})).unwrap();
    call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("other", &[])]})).unwrap();
    let proposals = view(&st, &h).proposals;
    assert_eq!(proposals.len(), 2);
    assert_eq!((proposals[0].source_task.as_deref(), proposals[0].source_tab.as_str()), (Some("rev"), r.as_str()));
    assert_eq!((proposals[1].source_task.as_deref(), proposals[1].source_tab.as_str()), (None, INTAKE));
    // Accettata, la revisione di seguito entra in coda dopo le correzioni.
    boards::proposal_decide(&st, &h, "ws-1", &proposals[0].id, true).unwrap();
    assert_eq!(task(&st, &h, "rev2")["kind"], "review");
    assert_eq!(task(&st, &h, "rev2")["depends_on"], json!(["fix"]));
    // Una revisione senza dipendenze non passa nemmeno dal revisore.
    let err = call(&st, &h, &r, "propose_tasks", json!({"tasks": [review_spec("bad", &[])]})).unwrap_err();
    assert!(err.contains("dependency"), "{err}");
}

#[test]
fn get_task_of_a_review_has_reviewer_instructions_and_the_reports() {
    let h = FakeHost::with_delivery(DeliveryMode::Pr, true);
    *h.worker_rules.lock().unwrap() = "never run e2e".to_string();
    let st = AutomationState::default();
    let report = json!({
        "status": "completed", "summary": "done a", "commit": "abc1234", "pr_url": "https://github.com/o/r/pull/7",
        "files_changed": ["src/a.rs"], "tests": [{"command": "cargo test", "result": "passed"}], "follow_ups": ["docs"]
    });
    let r = started_review(&st, &h, report);
    let t = call(&st, &h, &r, "get_task", json!({})).unwrap();
    assert_eq!(keys(&t), ["environment", "instructions", "reviewed_tasks", "task", "title"]);
    assert_eq!((t["task"].as_str(), t["title"].as_str()), (Some("do rev"), Some("Task rev")));
    let instr = t["instructions"].as_str().unwrap();
    assert!(instr.contains("Work only inside C:\\p\\.worktrees\\rev"), "{instr}");
    assert!(instr.contains(REVIEWER_INSTRUCTIONS.trim_end()));
    assert!(!instr.contains(WORKER_GUARDRAILS.trim_end()));
    assert!(instr.ends_with("never run e2e") && instr.contains("Workspace rules (always apply"));
    assert!(t["environment"].as_str().unwrap().contains("branch task/rev (created from main)"));
    let reviewed = t["reviewed_tasks"].as_array().unwrap();
    assert_eq!(reviewed.len(), 1);
    let a = &reviewed[0];
    assert_eq!((a["id"].as_str(), a["status"].as_str(), a["summary"].as_str()), (Some("a"), Some("done"), Some("done a")));
    assert_eq!(a["commit"], "abc1234");
    assert_eq!(a["pr_url"], "https://github.com/o/r/pull/7");
    assert_eq!(a["files_changed"], json!(["src/a.rs"]));
    assert_eq!(a["tests"], json!([{"command": "cargo test", "result": "passed"}]));
    assert_eq!(a["follow_ups"], json!(["docs"]));
    assert!(a["diffstat"].as_str().unwrap().contains("2 files changed"));
    assert!(a.get("task").is_none() && a.get("truncated").is_none());
    assert_eq!(task(&st, &h, "rev")["worker_status"], "running");
}

#[test]
fn get_task_of_a_review_asks_for_the_e2e_run_when_a_reviewed_task_requires_it() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    let marker = "E2E tests: at least one reviewed task requires them.";
    let instructions = |st: &AutomationState, h: &FakeHost| -> String {
        call(st, h, &r, "get_task", json!({})).unwrap()["instructions"].as_str().unwrap().to_string()
    };
    let flag = |st: &AutomationState, on: bool| {
        st.inner.lock().unwrap().boards.get_mut("ws-1").unwrap().task_mut("a").unwrap().e2e = on;
    };
    let e2e_cfg = RepoConfig {
        e2e: Some(crate::workspaces::E2eConfig { command: "npm run e2e".into(), junit: "junit.xml".into(), timeout_min: 20 }),
        ..Default::default()
    };
    let plain = instructions(&st, &h);
    assert!(!plain.contains(marker));
    // Task revisionato marcato ma repository senza configurazione: niente.
    flag(&st, true);
    assert_eq!(instructions(&st, &h), plain);
    // Configurazione presente: il paragrafo compare.
    h.repo_cfg.lock().unwrap().insert(".".into(), e2e_cfg.clone());
    let text = instructions(&st, &h);
    assert!(text.contains(marker) && text.contains("get_e2e_result") && text.contains("verdict \"issues\""), "{text}");
    // Task revisionato non marcato: niente, anche con la configurazione.
    flag(&st, false);
    assert!(!instructions(&st, &h).contains(marker));
}

#[test]
fn a_passed_review_is_done_with_its_verdict() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    call(&st, &h, &r, "get_task", json!({})).unwrap();
    complete(&st, &h, &r, json!({"verdict": "passed"})).unwrap();
    let t = task(&st, &h, "rev");
    assert_eq!((t["status"].as_str(), t["report"]["verdict"].as_str()), (Some("done"), Some("passed")));
    assert!(t["report"].get("findings").is_none());
}

#[test]
fn issues_need_findings_and_end_the_review_as_done() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    for bad in [json!({"verdict": "issues"}), json!({"verdict": "issues", "findings": []}), json!({})] {
        assert!(complete(&st, &h, &r, bad.clone()).is_err(), "{bad}");
    }
    let e = complete(&st, &h, &r, json!({"verdict": "issues"})).unwrap_err();
    assert!(e.contains("findings is missing"), "{e}");
    assert_eq!(task(&st, &h, "rev")["status"], "running", "rejected report leaves the review running");
    assert_eq!(complete(&st, &h, &r, json!({"verdict": "maybe"})).unwrap_err(), "verdict must be passed or issues");
    complete(&st, &h, &r, json!({"verdict": "issues", "findings": ["login fails", "no test"]})).unwrap();
    let t = task(&st, &h, "rev");
    assert_eq!(t["status"], "done");
    assert_eq!((t["report"]["verdict"].as_str(), t["report"]["findings"].clone()), (Some("issues"), json!(["login fails", "no test"])));
}

#[test]
fn a_failed_review_is_failed_and_takes_no_verdict() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    let failed = |extra: Value| {
        let mut p = json!({"status": "failed", "summary": "cannot build"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        call(&st, &h, &r, "complete_task", p)
    };
    let e = failed(json!({"verdict": "passed"})).unwrap_err();
    assert!(e.contains("only accepted with status \"completed\""), "{e}");
    assert_eq!(task(&st, &h, "rev")["status"], "running");
    failed(json!({"findings": ["partial: no docker"]})).unwrap();
    let t = task(&st, &h, "rev");
    assert_eq!(t["status"], "failed");
    assert!(t["report"].get("verdict").is_none());
}

#[test]
fn a_work_task_cannot_report_a_verdict() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    for extra in [json!({"verdict": "passed"}), json!({"findings": ["x"]})] {
        let e = complete(&st, &h, &w, extra).unwrap_err();
        assert!(e.contains("only accepted for review tasks"), "{e}");
    }
    assert_eq!(task(&st, &h, "a")["status"], "running");
    complete(&st, &h, &w, json!({})).unwrap();
    assert_eq!(task(&st, &h, "a")["status"], "done");
}

#[test]
fn list_tasks_rows_show_kind_and_verdict_for_reviews_only() {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    let row = |id: &str| {
        let l = call(&st, &h, INTAKE, "list_tasks", json!({})).unwrap();
        l["tasks"].as_array().unwrap().iter().find(|t| t["id"] == id).cloned().unwrap()
    };
    assert_eq!(row("rev")["kind"], "review");
    assert!(row("rev").get("verdict").is_none());
    assert!(row("a").get("kind").is_none() && row("a").get("verdict").is_none());
    complete(&st, &h, &r, json!({"verdict": "passed"})).unwrap();
    assert_eq!((row("rev")["kind"].as_str(), row("rev")["verdict"].as_str()), (Some("review"), Some("passed")));
    assert!(row("a").get("kind").is_none());
}

#[test]
fn a_review_worktree_is_created_and_cleaned_like_any_task() {
    for (mode, branch_cmd) in [(DeliveryMode::Pr, "branch -D task/rev"), (DeliveryMode::LocalMerge, "branch -d task/rev")] {
        let (st, h, r) = reviewer(mode, true);
        let t = task(&st, &h, "rev");
        assert_eq!((t["worktree"]["path"].as_str(), t["worktree"]["branch"].as_str()), (Some("C:\\p\\.worktrees\\rev"), Some("task/rev")));
        h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\rev".to_string());
        call(&st, &h, &r, "get_task", json!({})).unwrap();
        complete(&st, &h, &r, json!({"verdict": "passed"})).unwrap();
        assert!(!h.git_log().iter().any(|l| l.contains("worktree remove C:\\p\\.worktrees\\rev")), "{:?}", h.git_log());
        tab_closed(&st, &h, &r);
        let git = h.git_log();
        assert!(git.contains(&"C:\\p|worktree remove C:\\p\\.worktrees\\rev".to_string()), "{mode:?}: {git:?}");
        assert!(git.contains(&format!("C:\\p|{branch_cmd}")), "{mode:?}: {git:?}");
        assert!(task(&st, &h, "rev")["worktree"].is_null());
    }
}
