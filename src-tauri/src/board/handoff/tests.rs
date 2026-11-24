use super::*;
use crate::board::{build_report, Finished, ReportFields, TaskKind, TaskSpec, TaskState};
use serde_json::json;

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        kind: TaskKind::Work,
        ..TaskSpec::default()
    }
}

fn done(id: &str, fields: ReportFields) -> Task {
    let mut t = Task::from_spec(spec(id, &[]), 0);
    let report = build_report("completed", &format!("summary of {id}"), fields, None);
    t.set_state(TaskState::Done(Finished { started_at: None, finished_at: 0, report: Some(report) }));
    t
}

fn board(tasks: Vec<Task>) -> Board {
    Board { tasks, ..Board::new("ws-1") }
}

fn apply(h: Handoff) -> Value {
    let mut out = json!({ "task": "x" });
    h.add_to(&mut out);
    out
}

#[test]
fn a_loose_task_without_dependencies_has_an_empty_handoff_and_an_untouched_output() {
    let t = Task::from_spec(spec("a", &[]), 0);
    let b = board(vec![t.clone()]);
    let h = Handoff::of_worker(&b, &t);
    assert!(h.is_empty());
    assert_eq!(apply(h), json!({ "task": "x" }));
}

#[test]
fn dependencies_are_the_direct_done_ones_in_depends_on_order() {
    let fields = ReportFields { commit: Some("abc1234".into()), follow_ups: vec!["docs".into()], ..Default::default() };
    let queued = Task::from_spec(spec("q", &[]), 0);
    let work = Task::from_spec(spec("w", &["b", "q", "ghost", "a"]), 0);
    let b = board(vec![done("a", fields), done("b", ReportFields::default()), queued, work.clone()]);
    let h = Handoff::of_worker(&b, &work);
    assert!(!h.is_empty());
    let out = apply(h);
    let ids: Vec<&str> = out["dependencies"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(out["dependencies"][1]["commit"], "abc1234");
    assert_eq!(out["dependencies"][1]["follow_ups"], json!(["docs"]));
    assert_eq!(out["dependencies"][1]["summary"], "summary of a");
}

#[test]
fn dependencies_are_cut_like_the_reviewed_tasks() {
    let fields = ReportFields { files_changed: (0..100).map(|i| format!("f{i}")).collect(), ..Default::default() };
    let work = Task::from_spec(spec("w", &["a"]), 0);
    let b = board(vec![done("a", fields), work.clone()]);
    let out = apply(Handoff::of_worker(&b, &work));
    assert_eq!(out["dependencies"][0]["truncated"], true);
    assert!(out["dependencies"][0]["files_changed"].as_array().unwrap().len() < 100);
}
