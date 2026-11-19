//! Branch del goal: registrazione, lettura del file, "Elimina branch" e chiusura.

use super::*;
use crate::board::Board;
use serde_json::json;

fn gb(base: &str) -> GoalBranch {
    GoalBranch { branch: "goal/g".into(), base: base.into() }
}

fn board_with_goal() -> Board {
    let mut b = Board::new("ws-1");
    b.goals.push(Goal { id: "g".into(), title: "G".into(), created_at: 1, round: 1, ..Goal::default() });
    b
}

fn close(b: &mut Board, outcome: GoalOutcome) {
    let g = b.goal_mut("g").unwrap();
    (g.closed_at, g.outcome) = (Some(5), Some(outcome));
}

#[test]
fn the_name_of_a_goal_branch_is_prefixed_with_the_goal_id() {
    assert_eq!(goal_branch_name("export"), "goal/export");
    assert!(goal_branch_name("export").starts_with(GOAL_BRANCH_PREFIX));
}

#[test]
fn a_goal_branch_is_recorded_once_and_the_first_base_wins() {
    let mut b = board_with_goal();
    assert_eq!(b.goal_branch("g", "."), None);
    assert_eq!(b.record_goal_branch("g", ".", gb("main"), 7), Some(gb("main")));
    assert_eq!(b.updated_at, 7);
    // Un secondo avvio concorrente usa la base del vincitore e non scrive.
    assert_eq!(b.record_goal_branch("g", ".", gb("develop"), 9), Some(gb("main")));
    assert_eq!((b.goal_branch("g", "."), b.updated_at), (Some(&gb("main")), 7));
    // Un altro repository ha il suo.
    assert_eq!(b.record_goal_branch("g", "api", gb("develop"), 9), Some(gb("develop")));
    assert_eq!(b.record_goal_branch("nope", ".", gb("main"), 9), None);
}

#[test]
fn closing_a_goal_keeps_its_branches() {
    let mut b = board_with_goal();
    b.record_goal_branch("g", ".", gb("main"), 7);
    b.close_goal("g", 8).unwrap();
    let g = b.goal("g").unwrap();
    assert_eq!(g.outcome(), Some(GoalOutcome::Abandoned));
    assert_eq!(g.branches.get("."), Some(&gb("main")));
}

#[test]
fn only_an_abandoned_goal_with_branches_can_have_them_deleted() {
    let mut b = board_with_goal();
    let err = |b: &Board| b.abandoned_goal_branches("g").unwrap_err();
    assert_eq!(b.abandoned_goal_branches("x").unwrap_err(), "unknown goal: x");
    assert_eq!(err(&b), "goal g is open: close it first");
    close(&mut b, GoalOutcome::Abandoned);
    assert_eq!(err(&b), "goal g has no branches");
    b.record_goal_branch("g", ".", gb("main"), 7);
    b.record_goal_branch("g", "api", gb("dev"), 7);
    let keys: Vec<String> = b.abandoned_goal_branches("g").unwrap().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys, [".", "api"]);
    close(&mut b, GoalOutcome::Delivered);
    assert_eq!(err(&b), "goal g was delivered: its branches are kept");
}

#[test]
fn forgetting_branches_removes_only_the_listed_repositories() {
    let mut b = board_with_goal();
    b.record_goal_branch("g", ".", gb("main"), 7);
    b.record_goal_branch("g", "api", gb("dev"), 7);
    b.forget_goal_branches("g", &["api".to_string()], 11);
    let g = b.goal("g").unwrap();
    assert_eq!(g.branches.keys().collect::<Vec<_>>(), ["."]);
    assert_eq!(b.updated_at, 11);
}

#[test]
fn an_old_board_file_loads_without_branches_and_a_tampered_one_is_cleaned() {
    let goal = |branches: serde_json::Value| json!({ "id": "g", "title": "G", "request": "r", "done_when": ["d"], "round": 1, "branches": branches });
    let load = |g: serde_json::Value| {
        let raw = json!({ "workspace_id": "ws-1", "goals": [g] });
        Board::from_json(&raw.to_string(), 1).unwrap()
    };
    let mut old = goal(json!({}));
    old.as_object_mut().unwrap().remove("branches");
    assert!(load(old).goal("g").unwrap().branches.is_empty());
    let tampered = goal(json!({
        ".": { "branch": "goal/g", "base": "main" },
        "api": { "branch": "goal/other", "base": "main" },
        "web": { "branch": "goal/g", "base": "bad base; rm -rf" },
        "../out": { "branch": "goal/g", "base": "main" },
        "/abs": { "branch": "goal/g", "base": "main" },
        "./x": { "branch": "goal/g", "base": "main" },
    }));
    let kept: Vec<String> = load(tampered).goal("g").unwrap().branches.keys().cloned().collect();
    assert_eq!(kept, ["."]);
    // I branch si scrivono sempre, anche vuoti.
    let saved = serde_json::to_value(load(goal(json!({})))).unwrap();
    assert_eq!(saved["goals"][0]["branches"], json!({}));
}
