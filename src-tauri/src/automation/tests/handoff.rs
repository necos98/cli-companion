//! Passaggio di consegne in `get_task`: `dependencies` del worker e la riga
//! delle istruzioni, presente solo con le dipendenze.

use super::*;
use crate::board::texts::HANDOFF_NOTE;

const LOOSE_KEYS: [&str; 5] = ["delivery", "environment", "instructions", "task", "title"];

fn host() -> (AutomationState, FakeHost) {
    let h = FakeHost::with_delivery(DeliveryMode::Commit, false);
    (AutomationState::default(), h)
}

fn get_task(st: &AutomationState, h: &FakeHost, id: &str) -> Value {
    call(st, h, &session_of(st, h, id), "get_task", json!({})).unwrap()
}

fn complete(st: &AutomationState, h: &FakeHost, id: &str, report: Value) {
    let session = session_of(st, h, id);
    call(st, h, &session, "get_task", json!({})).unwrap();
    let mut params = json!({ "status": "completed", "summary": format!("done {id}") });
    params.as_object_mut().unwrap().extend(report.as_object().unwrap().clone());
    call(st, h, &session, "complete_task", params).unwrap();
}

fn note_count(t: &Value) -> usize {
    t["instructions"].as_str().unwrap().matches(HANDOFF_NOTE).count()
}

#[test]
fn a_loose_task_without_dependencies_keeps_its_get_task_and_instructions() {
    let (st, h) = host();
    queue(&st, &h, vec![spec("a", &[])]);
    let t = get_task(&st, &h, "a");
    assert_eq!(keys(&t), LOOSE_KEYS);
    assert_eq!(note_count(&t), 0);
}

#[test]
fn a_worker_gets_the_delivered_dependencies_and_the_note() {
    let (st, h) = host();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    complete(&st, &h, "a", json!({ "commit": "abc1234", "files_changed": ["src/a.rs"], "follow_ups": ["docs"] }));
    let t = get_task(&st, &h, "b");
    assert_eq!(keys(&t), ["delivery", "dependencies", "environment", "instructions", "task", "title"]);
    let deps = t["dependencies"].as_array().unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!((deps[0]["id"].as_str(), deps[0]["summary"].as_str()), (Some("a"), Some("done a")));
    assert_eq!(deps[0]["commit"], "abc1234");
    assert_eq!(deps[0]["files_changed"], json!(["src/a.rs"]));
    assert_eq!(deps[0]["follow_ups"], json!(["docs"]));
    assert!(deps[0].get("truncated").is_none());
    assert_eq!(note_count(&t), 1);
}

#[test]
fn a_dependency_report_is_truncated_like_the_reviewed_tasks() {
    let (st, h) = host();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    let many: Vec<String> = (0..100).map(|i| format!("src/f{i}.rs")).collect();
    complete(&st, &h, "a", json!({ "files_changed": many }));
    let t = get_task(&st, &h, "b");
    assert_eq!(t["dependencies"][0]["truncated"], true);
    assert!(t["dependencies"][0]["files_changed"].as_array().unwrap().len() < 100);
}
