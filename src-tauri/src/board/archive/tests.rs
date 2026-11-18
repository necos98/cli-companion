use super::*;
use crate::board::{FailCause, Finished, SpecContext, TaskSpec, TaskState, WorktreeInfo};

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        title: id.into(),
        task: "t".into(),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..TaskSpec::default()
    }
}

/// Task concluso (`done`, o `failed` se `failed`) al tempo `at`.
fn finished(id: &str, at: i64, failed: bool) -> Task {
    let mut t = Task::from_spec(spec(id, &[]), 1);
    let finished = Finished { started_at: None, finished_at: at, report: None };
    t.set_state(if failed {
        TaskState::Failed { finished, cause: FailCause::Other, reason: None }
    } else {
        TaskState::Done(finished)
    });
    t
}

/// `n` task conclusi `t0..`, il numero e' il tempo di fine (meta' falliti).
fn board_of(n: i64) -> Board {
    Board {
        workspace_id: "ws-1".into(),
        tasks: (0..n).map(|i| finished(&format!("t{i}"), i, i % 2 == 1)).collect(),
        ..Board::default()
    }
}

fn ids(tasks: &[Task]) -> Vec<&str> {
    tasks.iter().map(|t| t.id.as_str()).collect()
}

fn worktree() -> WorktreeInfo {
    WorktreeInfo { path: "p".into(), branch: "task/x".into(), base: "main".into(), setup_done: true }
}

#[test]
fn up_to_the_limit_nothing_is_archived() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64);
    let before = b.clone();
    assert!(b.take_archivable().is_empty());
    assert_eq!(b, before);
}

#[test]
fn the_oldest_finished_tasks_beyond_the_limit_go_first() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 3);
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t0", "t1", "t2"], "done and failed count alike, oldest first");
    assert_eq!(b.tasks.len(), MAX_FINISHED_KEPT);
    assert_eq!(b.tasks[0].id, "t3");
    assert!(b.take_archivable().is_empty(), "idempotent");
}

#[test]
fn age_is_finished_at_not_the_position_in_the_queue() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 2);
    b.tasks.reverse();
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t0", "t1"]);
    // I rimasti conservano l'ordine della coda.
    assert_eq!(b.tasks.first().map(|t| t.id.as_str()), Some("t201"));
    assert_eq!(b.tasks.last().map(|t| t.id.as_str()), Some("t2"));
}

#[test]
fn queued_and_running_tasks_are_neither_counted_nor_archived() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 1);
    let mut running = Task::from_spec(spec("run", &[]), 1);
    running.set_status(crate::board::TaskStatus::Running);
    b.tasks.insert(0, running);
    b.tasks.insert(0, Task::from_spec(spec("queued", &[]), 1));
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t0"]);
    assert!(b.task("queued").is_some() && b.task("run").is_some());
}

#[test]
fn a_task_with_a_worktree_stays() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 3);
    b.tasks[1].worktree = Some(worktree());
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t0", "t2"]);
    assert!(b.task("t1").is_some());
    // Resta oltre il tetto finche' il worktree non viene ripulito.
    assert_eq!(b.tasks.len(), MAX_FINISHED_KEPT + 1);
    assert!(b.take_archivable().is_empty());
    b.task_mut("t1").unwrap().worktree = None;
    assert_eq!(ids(&b.take_archivable()), ["t1"]);
}

#[test]
fn a_task_needed_by_an_unfinished_task_stays() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 3);
    let mut queued = Task::from_spec(spec("next", &["t0"]), 1);
    queued.depends_on.push("t2".into());
    b.tasks.push(queued);
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t1"]);
    assert_eq!(b.task("next").unwrap().depends_on, ["t0", "t2"]);
}

#[test]
fn a_task_needed_by_a_failed_task_stays_but_a_done_one_does_not_hold_it() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 3);
    // t3 e' `failed` (numero dispari), t4 `done`: entrambi fra i recenti.
    b.task_mut("t3").unwrap().depends_on = vec!["t0".into()];
    b.task_mut("t4").unwrap().depends_on = vec!["t1".into()];
    assert_eq!(b.task("t3").unwrap().status(), crate::board::TaskStatus::Failed);
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t1", "t2"]);
    assert!(b.task("t4").unwrap().depends_on.is_empty(), "riferimento rimosso");
    assert_eq!(b.task("t3").unwrap().depends_on, ["t0"]);
}

#[test]
fn a_task_a_pending_proposal_depends_on_stays() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 2);
    b.add_proposal("p-1".into(), "tab-1", None, vec![spec("new", &["t0"])].into(), SpecContext::default(), 5).unwrap();
    let gone = b.take_archivable();
    assert_eq!(ids(&gone), ["t1"]);
    assert!(b.task("t0").is_some());
}

#[test]
fn references_to_archived_tasks_are_dropped_from_the_remaining_ones() {
    let mut b = board_of(MAX_FINISHED_KEPT as i64 + 2);
    // t200 e' `done` (numero pari): non trattiene i task da cui dipende.
    b.task_mut("t200").unwrap().depends_on = vec!["t0".into(), "t7".into(), "t1".into()];
    assert_eq!(ids(&b.take_archivable()), ["t0", "t1"]);
    assert_eq!(b.task("t200").unwrap().depends_on, ["t7"]);
}
