use super::*;
use crate::board::{build_report, DecisionSource, Finished, GoalDecision, ReportFields, TaskKind, TaskSpec, TaskState};

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

fn goal() -> Goal {
    Goal {
        id: "g".into(),
        title: "Export orders".into(),
        request: "Export the orders to Excel".into(),
        done_when: vec!["the file opens in Excel".into()],
        decisions: vec![
            GoalDecision { at: 1, source: DecisionSource::Intake, text: "use xlsx".into() },
            GoalDecision { at: 2, source: DecisionSource::User, text: "no CSV".into() },
        ],
        ..Goal::default()
    }
}

fn task_in_goal(id: &str, deps: &[&str]) -> Task {
    let mut t = Task::from_spec(spec(id, deps), 0);
    t.join_goal("g", 1);
    t
}

fn board(tasks: Vec<Task>, goals: Vec<Goal>) -> Board {
    Board { tasks, goals, ..Board::new("ws-1") }
}

fn apply(h: Handoff) -> Value {
    let mut out = json!({ "task": "x" });
    h.add_to(&mut out);
    out
}

#[test]
fn a_loose_task_without_dependencies_has_an_empty_handoff_and_an_untouched_output() {
    let t = Task::from_spec(spec("a", &[]), 0);
    let b = board(vec![t.clone()], vec![]);
    let h = Handoff::of_worker(&b, &t);
    assert!(h.is_empty());
    assert_eq!(apply(h), json!({ "task": "x" }));
}

#[test]
fn dependencies_are_the_direct_done_ones_in_depends_on_order() {
    let fields = ReportFields { commit: Some("abc1234".into()), follow_ups: vec!["docs".into()], ..Default::default() };
    let queued = Task::from_spec(spec("q", &[]), 0);
    let work = Task::from_spec(spec("w", &["b", "q", "ghost", "a"]), 0);
    let b = board(vec![done("a", fields), done("b", ReportFields::default()), queued, work.clone()], vec![]);
    let h = Handoff::of_worker(&b, &work);
    assert!(!h.is_empty());
    let out = apply(h);
    let ids: Vec<&str> = out["dependencies"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(out["dependencies"][1]["commit"], "abc1234");
    assert_eq!(out["dependencies"][1]["follow_ups"], json!(["docs"]));
    assert_eq!(out["dependencies"][1]["summary"], "summary of a");
    assert!(out.get("goal").is_none());
}

#[test]
fn dependencies_are_cut_like_the_reviewed_tasks() {
    let fields = ReportFields { files_changed: (0..100).map(|i| format!("f{i}")).collect(), ..Default::default() };
    let work = Task::from_spec(spec("w", &["a"]), 0);
    let b = board(vec![done("a", fields), work.clone()], vec![]);
    let out = apply(Handoff::of_worker(&b, &work));
    assert_eq!(out["dependencies"][0]["truncated"], true);
    assert!(out["dependencies"][0]["files_changed"].as_array().unwrap().len() < 100);
}

#[test]
fn the_goal_carries_title_request_criteria_and_decisions() {
    let t = task_in_goal("a", &[]);
    let b = board(vec![t.clone()], vec![goal()]);
    let out = apply(Handoff::of_worker(&b, &t));
    assert_eq!(
        out["goal"],
        json!({
            "title": "Export orders",
            "request": "Export the orders to Excel",
            "done_when": ["the file opens in Excel"],
            "decisions": [{ "source": "intake", "text": "use xlsx" }, { "source": "user", "text": "no CSV" }],
        })
    );
    assert!(out.get("dependencies").is_none());
}

#[test]
fn a_task_pointing_at_a_missing_goal_has_no_goal() {
    let t = task_in_goal("a", &[]);
    let b = board(vec![t.clone()], vec![]);
    assert!(Handoff::of_worker(&b, &t).is_empty());
}

#[test]
fn the_reviewer_gets_the_goal_but_never_the_dependencies() {
    let a = done("a", ReportFields::default());
    let mut rev = task_in_goal("rev", &["a"]);
    rev.kind = TaskKind::Review;
    let b = board(vec![a, rev.clone()], vec![goal()]);
    let out = apply(Handoff::of_reviewer(&b, &rev));
    assert_eq!(out["goal"]["title"], "Export orders");
    assert!(out.get("dependencies").is_none());
    let loose = Task::from_spec(spec("l", &["a"]), 0);
    assert!(Handoff::of_reviewer(&b, &loose).is_empty());
}
