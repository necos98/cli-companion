//! Scheduler: i task di un goal (nel loro worktree) non occupano il repository.

use super::*;
use crate::board::{Goal, SpecContext, TaskSpec};
use crate::workspaces::{DeliveryMode, MergeMethod};
use std::collections::HashSet;

fn ctx(mode: DeliveryMode, parallel: bool) -> SchedCtx {
    SchedCtx { delivery: Delivery { mode, parallel, merge_method: MergeMethod::Squash }, gate_open: true }
}

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..TaskSpec::default()
    }
}

/// Board con i task `specs`; quelli in `in_goal` appartengono al goal "g".
fn board(specs: Vec<TaskSpec>, in_goal: &[&str]) -> Board {
    let mut b = Board::new("ws-1");
    b.check_specs(&specs, &HashSet::new(), None, SpecContext::default()).unwrap();
    b.push_specs(specs, None, 1);
    for id in in_goal {
        b.task_mut(id).unwrap().join_goal("g", 1);
    }
    b
}

fn pick(b: &Board, mode: DeliveryMode, parallel: bool) -> Vec<String> {
    schedule(b, &ctx(mode, parallel), &AgentGate::default())
}

#[test]
fn goal_tasks_of_the_same_repository_run_together_in_every_mode() {
    let mut b = board((0..4).map(|i| spec(&format!("t{i}"), &[])).collect(), &["t0", "t1", "t2", "t3"]);
    b.max_parallel = 4;
    for (mode, parallel) in [
        (DeliveryMode::Commit, false),
        (DeliveryMode::Push, false),
        (DeliveryMode::LocalMerge, false),
        (DeliveryMode::Pr, false),
        (DeliveryMode::Pr, true),
    ] {
        assert_eq!(pick(&b, mode, parallel), ["t0", "t1", "t2", "t3"], "{mode:?} {parallel}");
    }
    let mut capped = b.clone();
    capped.max_parallel = 2;
    assert_eq!(pick(&capped, DeliveryMode::Commit, false), ["t0", "t1"]);
    capped.start_task("t0", 1).unwrap();
    assert_eq!(pick(&capped, DeliveryMode::Commit, false), ["t1"]);
}

#[test]
fn a_running_goal_task_does_not_block_loose_tasks_but_loose_ones_stay_exclusive() {
    let mut b = board(vec![spec("g1", &[]), spec("l1", &[]), spec("l2", &[])], &["g1"]);
    b.start_task("g1", 1).unwrap();
    // `g1` non occupa la root: uno solo dei due sciolti parte.
    assert_eq!(pick(&b, DeliveryMode::Commit, false), ["l1"]);
    b.start_task("l1", 2).unwrap();
    assert!(pick(&b, DeliveryMode::Commit, false).is_empty());
}

#[test]
fn a_goal_task_without_git_keeps_the_folder_exclusive() {
    let mut b = board(vec![spec("a", &[]), spec("b", &[]), spec("c", &[])], &["a", "b", "c"]);
    b.task_mut("a").unwrap().no_git = true;
    b.task_mut("b").unwrap().no_git = true;
    // `a` e `b` condividono la cartella senza git: parte `a`; `c` (git) parte.
    assert_eq!(pick(&b, DeliveryMode::Commit, false), ["a", "c"]);
    b.start_task("a", 1).unwrap();
    assert_eq!(pick(&b, DeliveryMode::Commit, false), ["c"]);
}

#[test]
fn dependencies_still_gate_goal_tasks() {
    let b = board(vec![spec("a", &[]), spec("b", &["a"]), spec("c", &[])], &["a", "b", "c"]);
    assert_eq!(pick(&b, DeliveryMode::LocalMerge, false), ["a", "c"]);
}

#[test]
fn only_goal_tasks_with_git_and_plans_run_in_their_own_worktree() {
    let mut b = board(vec![spec("a", &[]), spec("l", &[])], &["a"]);
    assert!(b.runs_in_own_worktree(b.task("a").unwrap()));
    assert!(!b.runs_in_own_worktree(b.task("l").unwrap()));
    b.task_mut("a").unwrap().no_git = true;
    assert!(!b.runs_in_own_worktree(b.task("a").unwrap()));
}

/// Board con il goal "g" (`in_place` o no) e i task `ids` al suo interno.
fn goal_board(ids: &[&str], in_place: bool) -> Board {
    let mut b = board(ids.iter().map(|i| spec(i, &[])).collect(), ids);
    b.goals.push(Goal { id: "g".into(), round: 1, in_place, ..Goal::default() });
    b
}

#[test]
fn in_place_goal_tasks_run_one_at_a_time_like_loose_ones() {
    let mut b = goal_board(&["a", "b", "c"], true);
    b.max_parallel = 3;
    assert!(!b.runs_in_own_worktree(b.task("a").unwrap()));
    // In place anche se il workspace e' (poi) passato ai worktree paralleli.
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::Pr, false), (DeliveryMode::Pr, true)] {
        assert_eq!(pick(&b, mode, parallel), ["a"], "{mode:?} {parallel}");
    }
    b.start_task("a", 1).unwrap();
    assert!(pick(&b, DeliveryMode::Commit, false).is_empty());
    assert!(pick(&b, DeliveryMode::Pr, true).is_empty());
}

#[test]
fn a_goal_with_worktrees_is_unchanged_by_the_workspace_config() {
    let mut b = goal_board(&["a", "b"], false);
    assert!(b.runs_in_own_worktree(b.task("a").unwrap()));
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::Pr, true)] {
        assert_eq!(pick(&b, mode, parallel), ["a", "b"], "{mode:?} {parallel}");
    }
    b.max_parallel = 2;
    b.start_task("a", 1).unwrap();
    assert_eq!(pick(&b, DeliveryMode::Commit, false), ["b"]);
}
