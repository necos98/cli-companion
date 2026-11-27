//! `get_proposal`, `update_proposal` e `discard_proposal`: l'Intake corregge o scarta
//! le proposte pendenti senza accettarle; gli altri ruoli non possono.

use super::plan::started_plan;
use super::review::{reviewer, session_of_done};
use super::*;

const TOOLS: [&str; 3] = ["get_proposal", "update_proposal", "discard_proposal"];

fn pending(st: &AutomationState, h: &FakeHost, tasks: Vec<Value>) -> String {
    let r = call(st, h, INTAKE, "propose_tasks", json!({ "tasks": tasks })).unwrap();
    r["proposal"].as_str().unwrap().to_string()
}

fn pending_ids(st: &AutomationState, h: &FakeHost, proposal: &str) -> Vec<String> {
    let p = call(st, h, INTAKE, "get_proposal", json!({ "id": proposal })).unwrap();
    p["proposal"]["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn get_returns_the_full_specs_of_a_pending_proposal() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    let pid = pending(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    let p = call(&st, &h, INTAKE, "get_proposal", json!({ "id": pid })).unwrap();
    assert_eq!(p["proposal"]["id"], pid.as_str());
    assert_eq!(p["proposal"]["tasks"][1]["task"], "do b");
    assert_eq!(p["proposal"]["tasks"][1]["depends_on"], json!(["a"]));
    let e = call(&st, &h, INTAKE, "get_proposal", json!({ "id": "p-0" })).unwrap_err();
    assert_eq!(e, "proposal not found: p-0");
    assert!(call(&st, &h, INTAKE, "get_proposal", json!({})).is_err());
}

#[test]
fn update_replaces_the_tasks_and_the_user_decides_the_new_ones() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    let pid = pending(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    let r = call(&st, &h, INTAKE, "update_proposal", json!({"id": pid, "tasks": [spec("c", &[])]})).unwrap();
    assert_eq!(
        (r["proposal"].as_str(), r["status"].as_str(), r["tasks"].as_u64()),
        (Some(pid.as_str()), Some("pending"), Some(1))
    );
    assert_eq!(pending_ids(&st, &h, &pid), ["c"]);
    // La board e' pubblicata e salvata con la proposta corretta.
    assert_eq!(h.board_events.lock().unwrap().last().unwrap().proposals[0].tasks[0].id, "c");
    assert_eq!(h.saved.lock().unwrap()["ws-1"].proposals[0].tasks[0].id, "c");
    boards::proposal_decide(&st, &h, "ws-1", &pid, true).unwrap();
    assert_ne!(task(&st, &h, "c"), Value::Null);
    assert_eq!(task(&st, &h, "a"), Value::Null);
}

#[test]
fn an_invalid_update_leaves_the_proposal_as_it_was() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    let pid = pending(&st, &h, vec![spec("a", &[])]);
    let other = pending(&st, &h, vec![spec("z", &[])]);
    let update = |tasks: Value| call(&st, &h, INTAKE, "update_proposal", json!({"id": pid, "tasks": tasks}));
    assert!(update(json!([spec("a", &["ghost"])])).is_err(), "unknown dependency");
    assert!(update(json!([spec("z", &[])])).unwrap_err().contains("already used by a pending proposal"));
    let rooted = json!([{"id": "r", "title": "T", "task": "x", "bot": "general", "repo": "manca"}]);
    assert!(update(rooted).unwrap_err().contains("does not exist"));
    assert!(update(json!([])).is_err());
    assert!(call(&st, &h, INTAKE, "update_proposal", json!({"id": pid})).unwrap_err().contains("tasks is missing"));
    assert_eq!(pending_ids(&st, &h, &pid), ["a"]);
    assert_eq!(pending_ids(&st, &h, &other), ["z"]);
    let e = call(&st, &h, INTAKE, "update_proposal", json!({"id": "p-0", "tasks": [spec("q", &[])]})).unwrap_err();
    assert_eq!(e, "proposal not found: p-0");
}

#[test]
fn discard_removes_the_proposal_and_publishes_the_board() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    let pid = pending(&st, &h, vec![spec("a", &[])]);
    let keep = pending(&st, &h, vec![spec("k", &[])]);
    let r = call(&st, &h, INTAKE, "discard_proposal", json!({ "id": pid })).unwrap();
    assert_eq!(r["discarded"], pid.as_str());
    let v = view(&st, &h);
    assert_eq!(v.proposals.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [keep.as_str()]);
    assert_eq!(h.board_events.lock().unwrap().last().unwrap().proposals.len(), 1);
    assert_eq!(h.saved.lock().unwrap()["ws-1"].proposals.len(), 1);
    assert!(v.tasks.is_empty());
    let e = call(&st, &h, INTAKE, "discard_proposal", json!({ "id": pid })).unwrap_err();
    assert_eq!(e, format!("proposal not found: {pid}"));
    // Gli id liberati si possono riproporre.
    pending(&st, &h, vec![spec("a", &[])]);
}

#[test]
fn only_the_session_can_edit_pending_proposals() {
    let (st, h, reviewer_tab) = reviewer(DeliveryMode::LocalMerge, true);
    let worker_tab = session_of_done(&st, &h);
    let (_, planner_tab) = started_plan(&st, &h);
    let pid = pending(&st, &h, vec![spec("zz", &[])]);
    let params = json!({"id": pid, "tasks": [spec("yy", &[])]});
    for tab in [&worker_tab, &reviewer_tab, &planner_tab] {
        for tool in TOOLS {
            let e = call(&st, &h, tab, tool, params.clone()).unwrap_err();
            assert!(e.contains("automated sessions"), "{tool}: {e}");
        }
    }
    assert_eq!(pending_ids(&st, &h, &pid), ["zz"]);
}
