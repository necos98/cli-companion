//! Goal attraverso l'automazione: proposta dell'Intake con un goal, eredita'
//! della proposta del revisore, tool `list_goals`/`get_goal`, comandi della UI
//! (chiusura, note, task creato in un goal), chiusura provvisoria e archivio.

use super::*;
use crate::board::{Goal, GoalOutcome, GoalPhase, GoalReview, MAX_CLOSED_GOALS_KEPT};

fn goal_json(id: &str) -> Value {
    json!({ "id": id, "title": "Export", "request": "export the orders", "done_when": ["a CSV file"],
            "decisions": ["CSV, not Excel"] })
}

/// Propone dall'Intake con `extra` (goal o goal_id) e accetta.
fn queue_in(st: &AutomationState, h: &FakeHost, tasks: Vec<Value>, extra: Value) {
    let mut params = json!({ "tasks": tasks });
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    let r = call(st, h, INTAKE, "propose_tasks", params).unwrap();
    boards::proposal_decide(st, h, "ws-1", r["proposal"].as_str().unwrap(), true).unwrap();
}

fn review_spec(id: &str, deps: &[&str]) -> Value {
    let mut s = spec(id, deps);
    s["kind"] = json!("review");
    s
}

fn goal_of(st: &AutomationState, h: &FakeHost, id: &str) -> crate::board::GoalView {
    view(st, h).goals.into_iter().find(|g| g.goal.id == id).expect("goal")
}

#[test]
fn the_intake_proposes_a_goal_and_reads_it_back() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_in(&st, &h, vec![spec("a", &[]), spec("b", &["a"])], json!({ "goal": goal_json("export") }));
    let g = goal_of(&st, &h, "export");
    assert_eq!((g.phase, g.goal.round(), g.tasks.total), (GoalPhase::Executing, 1, 2));
    assert_eq!(task(&st, &h, "a")["goal"], "export");
    let list = call(&st, &h, INTAKE, "list_goals", json!({})).unwrap();
    assert_eq!(list["goals"][0]["id"], "export");
    assert_eq!(list["goals"][0]["tasks"], json!({ "done": 0, "total": 2 }));
    let rows = call(&st, &h, INTAKE, "list_tasks", json!({})).unwrap();
    assert!(rows["tasks"].as_array().unwrap().iter().all(|t| t["goal"] == "export"));
    let detail = call(&st, &h, INTAKE, "get_goal", json!({ "id": "export" })).unwrap();
    assert_eq!(detail["goal"]["request"], "export the orders");
    assert_eq!(detail["goal"]["decisions"][0]["source"], "intake");
    assert_eq!(detail["goal"]["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(call(&st, &h, INTAKE, "get_goal", json!({ "id": "nope" })).unwrap_err(), "unknown goal: nope");
    let err = call(&st, &h, INTAKE, "list_goals", json!({ "open_only": "yes" })).unwrap_err();
    assert_eq!(err, "open_only must be a boolean");
    // Un task aggiunto al goal entra nel giro corrente; update_task non cambia il goal.
    queue_in(&st, &h, vec![spec("c", &[])], json!({ "goal_id": "export" }));
    assert_eq!((task(&st, &h, "c")["goal"].clone(), task(&st, &h, "c")["round"].clone()), (json!("export"), json!(1)));
    let err = call(&st, &h, INTAKE, "update_task", json!({ "id": "c", "goal": "other" })).unwrap_err();
    assert_eq!(err, "the goal of a task cannot be changed");
}

#[test]
fn the_reviewer_reads_its_goal_and_its_proposal_inherits_it() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.goal_branch_exists("g");
    queue_in(&st, &h, vec![spec("a", &[]), review_spec("rev", &["a"])], json!({ "goal": goal_json("g") }));
    let a = session_of(&st, &h, "a");
    call(&st, &h, &a, "get_task", json!({})).unwrap();
    call(&st, &h, &a, "complete_task", json!({ "status": "completed", "summary": "done a" })).unwrap();
    let r = session_of(&st, &h, "rev");
    assert_eq!(goal_of(&st, &h, "g").phase, GoalPhase::Verifying);
    assert_eq!(call(&st, &h, &r, "get_goal", json!({ "id": "g" })).unwrap()["goal"]["phase"], "verifying");
    let denied = call(&st, &h, &r, "list_goals", json!({})).unwrap_err();
    assert!(denied.contains("not available to automated sessions"), "{denied}");
    let named = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [spec("x", &["a"])], "goal_id": "g" }));
    assert!(named.unwrap_err().contains("cannot be set by an automated session"));
    let p = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [spec("fix", &["a"])] })).unwrap();
    let pid = p["proposal"].as_str().unwrap().to_string();
    let pending = view(&st, &h).proposals.into_iter().find(|p| p.id == pid).unwrap();
    assert_eq!((pending.goal_id.as_deref(), pending.source_task.as_deref()), (Some("g"), Some("rev")));
    let issues = json!({ "status": "completed", "summary": "r", "verdict": "issues", "findings": ["broken"] });
    call(&st, &h, &r, "complete_task", issues).unwrap();
    assert_eq!(goal_of(&st, &h, "g").phase, GoalPhase::Deciding);
    boards::proposal_decide(&st, &h, "ws-1", &pid, true).unwrap();
    let g = goal_of(&st, &h, "g");
    assert_eq!((g.goal.round(), g.phase), (2, GoalPhase::Executing));
    assert_eq!(task(&st, &h, "fix")["round"], 2);
}

#[test]
fn a_goal_closes_by_itself_when_its_last_proposal_is_discarded() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.goal_branch_exists("g");
    queue_in(&st, &h, vec![spec("a", &[]), review_spec("rev", &["a"])], json!({ "goal": goal_json("g") }));
    let a = session_of(&st, &h, "a");
    call(&st, &h, &a, "complete_task", json!({ "status": "completed", "summary": "done a" })).unwrap();
    let r = session_of(&st, &h, "rev");
    let p = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [spec("fix", &["a"])] })).unwrap();
    let issues = json!({ "status": "completed", "summary": "r", "verdict": "issues", "findings": ["broken"] });
    call(&st, &h, &r, "complete_task", issues).unwrap();
    assert!(goal_of(&st, &h, "g").goal.is_open());
    boards::proposal_decide(&st, &h, "ws-1", p["proposal"].as_str().unwrap(), false).unwrap();
    let g = goal_of(&st, &h, "g");
    assert_eq!(
        (g.phase, g.goal.outcome(), g.goal.review()),
        (GoalPhase::Closed, Some(GoalOutcome::Delivered), Some(GoalReview::Issues))
    );
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert!(!saved.goal("g").unwrap().is_open(), "the closure is saved");
    let err = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": [spec("late", &[])], "goal_id": "g" }));
    assert_eq!(err.unwrap_err(), "goal g is closed: propose a new goal");
}

#[test]
fn the_ui_creates_tasks_in_a_goal_adds_notes_and_closes_it() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    // In pausa: nessun task parte, cosi' la chiusura li trova in coda.
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    queue_in(&st, &h, vec![spec("a", &[])], json!({ "goal": goal_json("g") }));
    let manual = json!({ "title": "Manual step", "task": "do it", "goal": "g" });
    boards::task_create(&st, &h, "ws-1", &manual).unwrap();
    let created = view(&st, &h).tasks.into_iter().find(|t| t.task.title == "Manual step").unwrap();
    assert_eq!((created.task.goal(), created.task.round()), (Some("g"), 1));
    let unknown = json!({ "title": "Other", "task": "x", "goal": "nope" });
    assert_eq!(boards::task_create(&st, &h, "ws-1", &unknown).unwrap_err(), "unknown goal: nope");
    let v = boards::goal_add_note(&st, &h, "ws-1", "g", "ship it on Friday").unwrap();
    let last = v.goals[0].goal.decisions.last().unwrap();
    assert_eq!((last.text.as_str(), last.source), ("ship it on Friday", crate::board::DecisionSource::User));
    let v = boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(v.goals[0].goal.outcome(), Some(GoalOutcome::Abandoned));
    assert!(v.tasks.is_empty(), "queued tasks of the goal are deleted");
    assert_eq!(boards::goal_close(&st, &h, "ws-1", "g").unwrap_err(), "goal g is already closed");
    assert!(boards::goal_add_note(&st, &h, "ws-1", "g", "late").unwrap_err().contains("is closed"));
}

#[test]
fn closing_a_goal_with_a_running_task_is_refused() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_in(&st, &h, vec![spec("a", &[])], json!({ "goal": goal_json("g") }));
    session_of(&st, &h, "a");
    let err = boards::goal_close(&st, &h, "ws-1", "g").unwrap_err();
    assert_eq!(err, "cannot close goal 'g': a are running, stop them first");
    assert!(goal_of(&st, &h, "g").goal.is_open());
}

/// Board "ws-1" con `n` goal chiusi "c0".."c<n-1>" (il numero e' il tempo di chiusura).
fn board_with_closed_goals(n: usize) -> Board {
    let raw = json!({
        "workspace_id": "ws-1",
        "goals": (0..n).map(|i| json!({ "id": format!("c{i}"), "title": "C", "request": "r", "done_when": ["d"],
                                         "round": 1, "closed_at": i, "outcome": "delivered", "review": "none" }))
            .collect::<Vec<_>>()
    });
    Board::from_json(&raw.to_string(), 1).unwrap()
}

fn goal_ids(goals: &[Goal]) -> Vec<&str> {
    goals.iter().map(|g| g.id.as_str()).collect()
}

#[test]
fn closed_goals_beyond_the_limit_are_archived_before_the_board_is_saved() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    st.load_board(board_with_closed_goals(MAX_CLOSED_GOALS_KEPT + 2));
    st.run(&h, |r| r.mark_dirty("ws-1"));
    let archived: Vec<(String, String)> =
        h.goal_archive_log.lock().unwrap().iter().map(|(w, g)| (w.clone(), g.id.clone())).collect();
    assert_eq!(archived, [("ws-1".to_string(), "c0".to_string()), ("ws-1".to_string(), "c1".to_string())]);
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.goals.len(), MAX_CLOSED_GOALS_KEPT);
    assert!(saved.goal("c0").is_none() && saved.goal("c2").is_some());
}

#[test]
fn goals_the_archive_refused_are_kept_and_archived_by_the_next_change() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    *h.archive_err.lock().unwrap() = Some("disco pieno".to_string());
    st.load_board(board_with_closed_goals(MAX_CLOSED_GOALS_KEPT + 1));
    st.run(&h, |r| r.mark_dirty("ws-1"));
    assert!(h.goal_archive_log.lock().unwrap().is_empty());
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.goals.len(), MAX_CLOSED_GOALS_KEPT + 1, "{:?}", goal_ids(&saved.goals));
    assert_eq!(st.registry().boards.get("ws-1").unwrap().goals.len(), MAX_CLOSED_GOALS_KEPT + 1);
    *h.archive_err.lock().unwrap() = None;
    st.run(&h, |r| r.mark_dirty("ws-1"));
    let archived: Vec<String> = h.goal_archive_log.lock().unwrap().iter().map(|(_, g)| g.id.clone()).collect();
    assert_eq!(archived, ["c0"]);
    assert_eq!(h.saved.lock().unwrap().get("ws-1").unwrap().goals.len(), MAX_CLOSED_GOALS_KEPT);
}
