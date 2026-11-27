//! `propose_tasks` del revisore: le correzioni entrano subito in testa alla
//! coda, i task che aspettano il lavoro revisionato aspettano la nuova revisione,
//! l'utente riceve la notifica `fixes`. Intake e pianificatore restano con la
//! proposta pendente.

use super::review::reviewer;
use super::*;

fn review_spec(id: &str, deps: &[&str]) -> Value {
    let mut s = spec(id, deps);
    s["kind"] = json!("review");
    s
}

/// Revisione `rev` in corso sul task `a` (concluso); `b` dipende da `rev`, `c` e' libero.
fn started() -> (AutomationState, FakeHost, String) {
    let (st, h, r) = reviewer(DeliveryMode::Commit, false);
    queue(&st, &h, vec![spec("b", &["rev"]), spec("c", &[])]);
    (st, h, r)
}

fn fixes_params() -> Value {
    json!({"tasks": [spec("fix", &["a"]), review_spec("rev2", &["fix"])]})
}

fn queued_order(st: &AutomationState, h: &FakeHost) -> Vec<String> {
    view(st, h).tasks.iter().map(|t| t.task.id.clone()).collect()
}

#[test]
fn the_reviewer_fixes_are_queued_at_once_at_the_head() {
    let (st, h, r) = started();
    let out = call(&st, &h, &r, "propose_tasks", fixes_params()).unwrap();
    assert_eq!(out["status"], "accepted");
    assert_eq!(out["tasks"], json!(["fix", "rev2"]));
    assert_eq!(out["waiting"], json!(["b"]));
    assert!(out.get("proposal").is_none());
    assert!(view(&st, &h).proposals.is_empty());
    assert_eq!(queued_order(&st, &h), ["a", "rev", "fix", "rev2", "b", "c"]);
    assert_eq!(task(&st, &h, "fix")["origin"], "rev");
    assert_eq!(task(&st, &h, "rev2")["origin"], "rev");
    assert!(task(&st, &h, "c").get("origin").is_none());
    assert_eq!(task(&st, &h, "b")["depends_on"], json!(["rev", "rev2"]));
}

#[test]
fn the_reviewer_blocks_make_unrelated_tasks_wait() {
    let (st, h, r) = started();
    let mut params = fixes_params();
    params["blocks"] = json!(["c"]);
    let out = call(&st, &h, &r, "propose_tasks", params).unwrap();
    assert_eq!(out["waiting"], json!(["b", "c"]));
    assert_eq!(task(&st, &h, "c")["depends_on"], json!(["rev2"]));
    let mut bad = fixes_params();
    bad["blocks"] = json!(["ghost"]);
    let e = call(&st, &h, &r, "propose_tasks", bad).unwrap_err();
    assert_eq!(e, "blocks: unknown task 'ghost'");
}

#[test]
fn a_rejected_reviewer_call_changes_nothing() {
    let (st, h, r) = started();
    let before = queued_order(&st, &h);
    let e = call(&st, &h, &r, "propose_tasks", json!({"tasks": [spec("fix", &["a"]), spec("c", &[])]})).unwrap_err();
    assert!(e.contains("duplicate task id: c"), "{e}");
    assert_eq!(queued_order(&st, &h), before);
    assert!(h.notices().is_empty());
}

#[test]
fn queueing_the_fixes_notifies_the_user() {
    let (st, h, r) = started();
    call(&st, &h, &r, "propose_tasks", fixes_params()).unwrap();
    let notices = h.notices();
    assert_eq!(notices.len(), 1);
    let n = &notices[0];
    assert_eq!((n.kind, n.task_id.as_str(), n.tab_id.as_deref()), ("fixes", "rev", Some(r.as_str())));
    assert_eq!(n.body, "Task rev: 2 correzioni in coda, 1 task in attesa");
}

#[test]
fn sessions_still_get_a_pending_proposal_and_cannot_use_blocks() {
    let (st, h, _r) = started();
    let p = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("x", &[])]})).unwrap();
    assert_eq!(p["status"], "pending");
    assert_eq!(view(&st, &h).proposals.len(), 1);
    let e = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("y", &[])], "blocks": ["c"]})).unwrap_err();
    assert_eq!(e, "unknown field 'blocks' (allowed: tasks)");
    assert!(h.notices().is_empty());
}

#[test]
fn the_reviewer_can_read_the_queue() {
    let (st, h, r) = started();
    let list = call(&st, &h, &r, "list_tasks", json!({})).unwrap();
    let ids: Vec<&str> = list["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a", "rev", "b", "c"]);
}

#[test]
fn a_waiting_task_does_not_start_before_the_fixes_and_the_new_review_are_done() {
    let (st, h, r) = started();
    call(&st, &h, &r, "propose_tasks", fixes_params()).unwrap();
    call(&st, &h, &r, "get_task", json!({})).unwrap();
    call(
        &st,
        &h,
        &r,
        "complete_task",
        json!({"status": "completed", "summary": "s", "verdict": "issues", "findings": ["bug"]}),
    )
    .unwrap();
    // `rev` e' done con verdetto issues: senza il cancello `b` partirebbe adesso.
    assert_eq!(task(&st, &h, "rev")["status"], "done");
    assert_eq!(task(&st, &h, "fix")["status"], "running");
    assert_eq!(task(&st, &h, "b")["status"], "queued");
    finish(&st, &h, &session_of(&st, &h, "fix"), "completed");
    assert_eq!(task(&st, &h, "rev2")["status"], "running");
    assert_eq!(task(&st, &h, "b")["status"], "queued");
    let r2 = session_of(&st, &h, "rev2");
    call(&st, &h, &r2, "get_task", json!({})).unwrap();
    call(&st, &h, &r2, "complete_task", json!({"status": "completed", "summary": "ok", "verdict": "passed"})).unwrap();
    assert_eq!(task(&st, &h, "rev2")["status"], "done");
    assert_eq!(task(&st, &h, "b")["status"], "running");
}
