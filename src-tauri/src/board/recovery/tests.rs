use super::*;
use crate::board::{
    E2eRun, E2eStatus, FailCause, Finished, Run, RunPhase, Task, TaskReport, TaskSpec, TaskState, WorkerActivity,
    INTERRUPTED_NOTE,
};
use serde_json::json;

const NOW: i64 = 5_000;

fn task_in(id: &str, state: TaskState) -> Task {
    let spec = TaskSpec { id: id.into(), title: id.into(), task: "t".into(), ..TaskSpec::default() };
    let mut t = Task::from_spec(spec, 1);
    t.set_state(state);
    t
}

fn running(phase: RunPhase) -> TaskState {
    TaskState::Running(Run { started_at: 100, previous_attempt: None, phase })
}

fn preparing() -> TaskState {
    running(RunPhase::Preparing { step: Some("setup".into()), heartbeat: 150 })
}

fn live() -> TaskState {
    running(RunPhase::Live { session: "w1".into(), worker: WorkerActivity::NeedsInput })
}

fn board(tasks: Vec<Task>) -> Board {
    Board { workspace_id: "ws-1".into(), tasks, ..Board::default() }
}

#[test]
fn running_tasks_fail_as_interrupted() {
    let mut b = board(vec![task_in("prep", preparing()), task_in("live", live())]);
    assert_eq!(b.recover_after_restart(NOW), 2);
    for id in ["prep", "live"] {
        let t = b.task(id).unwrap();
        assert_eq!(t.status(), TaskStatus::Failed, "{id}");
        assert_eq!(t.note(), Some(INTERRUPTED_NOTE), "{id}");
        assert_eq!((t.started_at(), t.finished_at(), t.updated_at), (Some(100), Some(NOW), NOW), "{id}");
        assert_eq!((t.session(), t.worker_status(), t.preparing()), (None, None, None), "{id}");
        assert!(matches!(t.state(), TaskState::Failed { cause: FailCause::Other, .. }), "{id}");
    }
}

#[test]
fn other_states_are_left_alone() {
    let done = Finished { started_at: Some(1), finished_at: 2, report: None };
    let mut b = board(vec![
        task_in("q", TaskState::default()),
        task_in("d", TaskState::Done(done.clone())),
        task_in(
            "f",
            TaskState::Failed { finished: done, cause: FailCause::SessionEnded { session: "w9".into() }, reason: None },
        ),
    ]);
    let before = b.clone();
    assert_eq!(b.recover_after_restart(NOW), 0);
    assert_eq!(b, before);
}

#[test]
fn recovering_twice_is_the_same_as_once() {
    let mut b = board(vec![task_in("live", live())]);
    assert_eq!(b.recover_after_restart(NOW), 1);
    let once = b.clone();
    assert_eq!(b.recover_after_restart(NOW + 1000), 0);
    assert_eq!(b, once);
}

fn running_e2e() -> E2eRun {
    E2eRun::started("run-1".into(), "junit.xml".into(), 50)
}

#[test]
fn running_e2e_runs_are_interrupted_in_every_state() {
    let report: TaskReport =
        serde_json::from_value(json!({"status": "failed", "summary": "s", "e2e": running_e2e()})).unwrap();
    let mut queued = task_in("q", TaskState::default());
    queued.e2e_run = Some(running_e2e());
    let failed_state = TaskState::Failed {
        finished: Finished { started_at: None, finished_at: 9, report: Some(report) },
        cause: FailCause::Other,
        reason: None,
    };
    let mut failed = task_in("f", failed_state);
    failed.e2e_run = Some(running_e2e());
    let mut live_task = task_in("live", live());
    live_task.e2e_run = Some(running_e2e());
    let mut b = board(vec![queued, failed, live_task]);
    b.recover_after_restart(NOW);
    let (q, f, l) = (b.task("q").unwrap(), b.task("f").unwrap(), b.task("live").unwrap());
    for run in [q.e2e_run.as_ref().unwrap(), f.e2e_run.as_ref().unwrap(), f.report().unwrap().e2e.as_ref().unwrap()] {
        assert_eq!((run.status, run.error.as_deref()), (E2eStatus::Error, Some(INTERRUPTED_NOTE)));
    }
    assert_eq!(l.e2e_run.as_ref().unwrap().status, E2eStatus::Error);
}

#[test]
fn finished_e2e_runs_keep_their_outcome() {
    let mut passed = running_e2e();
    passed.status = E2eStatus::Passed;
    let mut t = task_in("q", TaskState::default());
    t.e2e_run = Some(passed.clone());
    let mut b = board(vec![t]);
    b.recover_after_restart(NOW);
    assert_eq!(b.task("q").unwrap().e2e_run, Some(passed));
}

#[test]
fn from_json_recovers_after_migrating() {
    let raw = json!({
        "workspace_id": "ws-1",
        "tasks": [{"id": "a", "title": "A", "task": "t", "status": "running", "session": "w1", "worker_status": "idle"}]
    })
    .to_string();
    let b = Board::from_json(&raw, NOW).unwrap();
    let a = b.task("a").unwrap();
    assert_eq!((a.status(), a.note(), a.finished_at()), (TaskStatus::Failed, Some(INTERRUPTED_NOTE), Some(NOW)));
}
