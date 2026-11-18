//! Tabella delle transizioni: ogni evento in ogni stato, gli effetti sui campi
//! e i rifiuti (che lasciano il task com'era).

use super::*;
use crate::board::{build_report, E2eRun, FailReason, ReportFields, TaskSpec, WorkerStatus};

const W1: &str = "w1";

fn task() -> Task {
    Task::from_spec(TaskSpec { id: "a".into(), title: "A".into(), task: "do a".into(), ..TaskSpec::default() }, 10)
}

fn wt(branch: &str) -> WorktreeInfo {
    WorktreeInfo { path: format!("p/{branch}"), branch: branch.into(), base: "main".into(), setup_done: true }
}

fn report(summary: &str) -> TaskReport {
    build_report("completed", summary, ReportFields::default(), None)
}

fn prev(summary: &str) -> PreviousAttempt {
    PreviousAttempt { summary: Some(summary.into()), note: None }
}

fn run(phase: RunPhase) -> TaskState {
    TaskState::Running(Run { started_at: 20, previous_attempt: Some(prev("before")), phase })
}

fn finished(report: Option<TaskReport>) -> Finished {
    Finished { started_at: Some(20), finished_at: 30, report }
}

/// Le sei forme che uno stato puo' avere, con il tentativo 1 (la coda: 0).
fn states() -> Vec<(&'static str, Task)> {
    let at = |attempts: u32, note: Option<&str>, state: TaskState| {
        let mut t = task();
        t.set_state(state);
        t.set_attempts(attempts);
        t.set_note(note);
        t
    };
    let ended = FailCause::SessionEnded { session: W1.into() };
    vec![
        ("queued", at(0, None, TaskState::Queued { previous_attempt: Some(prev("before")) })),
        ("preparing", at(1, None, run(RunPhase::Preparing { step: Some("setup".into()), heartbeat: 25 }))),
        ("live", at(1, None, run(RunPhase::Live { session: W1.into(), worker: WorkerActivity::Running }))),
        ("done", at(1, None, TaskState::Done(finished(Some(report("delivered")))))),
        (
            "failed_ended",
            at(
                1,
                Some("ended"),
                TaskState::Failed { finished: finished(Some(report("partial"))), cause: ended, reason: None },
            ),
        ),
        (
            "failed_other",
            at(1, Some("boom"), TaskState::Failed { finished: finished(None), cause: FailCause::Other, reason: None }),
        ),
    ]
}

fn state_named(name: &str) -> Task {
    states().into_iter().find(|(n, _)| *n == name).map(|(_, t)| t).unwrap()
}

fn finish(session: Option<&str>) -> TaskEvent {
    TaskEvent::Finish { session: session.map(str::to_string), outcome: Outcome::Done, report: None, note: None }
}

/// Ogni evento con gli stati in cui e' ammesso; negli altri e' rifiutato con
/// l'errore indicato e il task non cambia.
fn table() -> Vec<(&'static str, TaskEvent, &'static [&'static str])> {
    let failed: &[&str] = &["failed_ended", "failed_other"];
    vec![
        ("start", TaskEvent::Start, &["queued"]),
        ("prepare step", TaskEvent::PrepareStep { attempt: 1, step: Some("x".into()) }, &["preparing"]),
        ("prepare ended", TaskEvent::PrepareEnded { attempt: 1 }, &["preparing"]),
        ("worktree", TaskEvent::WorktreeCreated { attempt: 1, worktree: wt("t") }, &["preparing"]),
        (
            "spawned",
            TaskEvent::WorkerSpawned { attempt: 1, session: "w2".into(), worktree: None, note: None },
            &["preparing"],
        ),
        ("changed", TaskEvent::WorkerChanged { session: W1.into(), activity: WorkerActivity::Idle }, &["live"]),
        ("finish", finish(None), &["preparing", "live"]),
        ("finish for the session", finish(Some(W1)), &["live"]),
        ("requeue", TaskEvent::Requeue { note: "n".into() }, &["preparing", "live"]),
        ("retry", TaskEvent::Retry, failed),
        ("mark done", TaskEvent::MarkDone, failed),
        ("reopen", TaskEvent::Reopen { session: W1.into(), attempt: 1, note: "r".into() }, &["failed_ended"]),
    ]
}

#[test]
fn every_event_is_allowed_only_in_its_states_and_a_rejection_changes_nothing() {
    for (event_name, event, allowed) in table() {
        for (state_name, task) in states() {
            let mut t = task.clone();
            let result = t.apply(event.clone(), 99);
            if allowed.contains(&state_name) {
                assert!(result.is_ok(), "{event_name} in {state_name}: {result:?}");
                assert_ne!(t, task, "{event_name} in {state_name}: nothing changed");
                continue;
            }
            let expected = match event {
                TaskEvent::Retry => LifecycleError::NotAllowed {
                    task: "a".into(),
                    status: task.status(),
                    op: Op::Action(TaskAction::Retry),
                },
                TaskEvent::MarkDone => LifecycleError::NotAllowed {
                    task: "a".into(),
                    status: task.status(),
                    op: Op::Action(TaskAction::MarkDone),
                },
                _ => LifecycleError::Stale,
            };
            assert_eq!(result, Err(expected), "{event_name} in {state_name}");
            assert_eq!(t, task, "{event_name} in {state_name}: rejected but changed");
        }
    }
}

#[test]
fn start_runs_a_queued_task_carrying_the_previous_attempt() {
    let mut t = state_named("queued");
    t.e2e_run = Some(E2eRun::started("r".into(), "j.xml".into(), 1));
    t.set_note(Some("old"));
    t.set_last_report(Some("old report"));
    t.apply(TaskEvent::Start, 50).unwrap();
    let TaskState::Running(run) = t.state() else { panic!("{:?}", t.state()) };
    assert_eq!(run.started_at, 50);
    assert_eq!(run.previous_attempt, Some(prev("before")));
    assert_eq!(run.phase, RunPhase::Preparing { step: None, heartbeat: 50 });
    assert_eq!((t.attempts(), t.note(), t.last_report(), t.updated_at), (1, None, None, 50));
    assert!(t.e2e_run.is_none());
    assert_eq!((t.worker_status(), t.session()), (Some(WorkerStatus::Starting), None));
}

#[test]
fn prepare_events_follow_the_attempt_and_leave_updated_at_alone() {
    let mut t = state_named("preparing");
    t.apply(TaskEvent::PrepareStep { attempt: 1, step: None }, 60).unwrap();
    assert_eq!(t.preparing(), Some((Some("setup"), 60)), "senza etichetta resta il passo, il battito avanza");
    t.apply(TaskEvent::PrepareStep { attempt: 1, step: Some("npm ci".into()) }, 61).unwrap();
    assert_eq!(t.preparing(), Some((Some("npm ci"), 61)));
    t.apply(TaskEvent::PrepareEnded { attempt: 1 }, 62).unwrap();
    assert_eq!(t.preparing(), Some((None, 62)));
    t.apply(TaskEvent::WorktreeCreated { attempt: 1, worktree: wt("t") }, 63).unwrap();
    assert_eq!(t.worktree, Some(wt("t")));
    assert_eq!(t.updated_at, 10, "nessuno dei tre tocca updated_at");
    for stale in [0, 2] {
        assert_eq!(t.apply(TaskEvent::PrepareStep { attempt: stale, step: None }, 64), Err(LifecycleError::Stale));
        assert_eq!(t.apply(TaskEvent::PrepareEnded { attempt: stale }, 64), Err(LifecycleError::Stale));
        let created = TaskEvent::WorktreeCreated { attempt: stale, worktree: wt("x") };
        assert_eq!(t.apply(created, 64), Err(LifecycleError::Stale));
    }
    assert_eq!(t.preparing(), Some((None, 62)));
}

#[test]
fn a_spawned_worker_replaces_the_worktree_and_the_note_only_when_given() {
    let mut t = state_named("preparing");
    t.worktree = Some(wt("old"));
    t.set_note(Some("kept"));
    let spawned = |worktree, note| TaskEvent::WorkerSpawned { attempt: 1, session: "w2".into(), worktree, note };
    let mut bare = t.clone();
    bare.apply(spawned(None, None), 70).unwrap();
    assert_eq!((bare.worktree.clone(), bare.note()), (Some(wt("old")), Some("kept")));
    assert_eq!((bare.session(), bare.worker_status(), bare.updated_at), (Some("w2"), Some(WorkerStatus::Starting), 70));
    assert_eq!(bare.preparing(), None);
    t.apply(spawned(Some(wt("new")), Some("start warnings: x".into())), 71).unwrap();
    assert_eq!((t.worktree.clone(), t.note()), (Some(wt("new")), Some("start warnings: x")));
    assert_eq!(t.apply(spawned(None, None), 72), Err(LifecycleError::Stale), "gia' spawnato");
    let wrong_attempt = TaskEvent::WorkerSpawned { attempt: 2, session: "w3".into(), worktree: None, note: None };
    assert_eq!(state_named("preparing").apply(wrong_attempt, 72), Err(LifecycleError::Stale));
}

#[test]
fn a_worker_change_applies_only_to_the_live_session() {
    let mut t = state_named("live");
    let changed = |session: &str, activity| TaskEvent::WorkerChanged { session: session.into(), activity };
    t.apply(changed(W1, WorkerActivity::NeedsInput), 80).unwrap();
    assert_eq!((t.worker_status(), t.updated_at), (Some(WorkerStatus::NeedsInput), 80));
    assert!(t.needs_attention());
    assert_eq!(t.apply(changed("other", WorkerActivity::Running), 81), Err(LifecycleError::Stale));
    assert_eq!(t.worker_status(), Some(WorkerStatus::NeedsInput));
    assert_eq!(t.live_activity(W1), Some(WorkerActivity::NeedsInput));
    assert!(t.is_live_for(W1) && !t.is_live_for("other"));
}

#[test]
fn finish_records_the_outcome_the_report_and_the_note() {
    let long = "s".repeat(500);
    let mut t = state_named("live");
    t.set_note(Some("kept"));
    let event = TaskEvent::Finish {
        session: Some(W1.into()),
        outcome: Outcome::Done,
        report: Some(Box::new(report(&long))),
        note: None,
    };
    t.apply(event, 90).unwrap();
    let TaskState::Done(f) = t.state() else { panic!("{:?}", t.state()) };
    assert_eq!((f.started_at, f.finished_at), (Some(20), 90));
    assert_eq!(f.report.as_ref().map(|r| r.summary.as_str()), Some(long.as_str()));
    assert_eq!(t.last_report().map(|s| s.chars().count()), Some(200), "il riassunto breve e' tagliato");
    assert_eq!((t.note(), t.updated_at, t.previous_attempt()), (Some("kept"), 90, None), "la nota resta");
    assert_eq!((t.session(), t.worker_status()), (None, None));

    let cause = FailCause::SessionEnded { session: W1.into() };
    let mut t = state_named("preparing");
    let event = TaskEvent::Finish {
        session: None,
        outcome: Outcome::Failed(Failure { cause: cause.clone(), reason: FailReason::WorkerExited }),
        report: None,
        note: Some("n".into()),
    };
    t.apply(event, 91).unwrap();
    assert_eq!(
        t.state(),
        &TaskState::Failed { finished: finished_at(91), cause, reason: Some(FailReason::WorkerExited) }
    );
    assert_eq!((t.note(), t.last_report()), (Some("n"), None));
}

fn finished_at(at: i64) -> Finished {
    Finished { started_at: Some(20), finished_at: at, report: None }
}

#[test]
fn finish_for_a_session_needs_that_exact_live_worker() {
    for wrong in ["other", ""] {
        let mut t = state_named("live");
        assert_eq!(t.apply(finish(Some(wrong)), 5), Err(LifecycleError::Stale));
    }
    // In preparazione non c'e' nessun worker a cui riferirsi.
    assert_eq!(state_named("preparing").apply(finish(Some(W1)), 5), Err(LifecycleError::Stale));
}

#[test]
fn requeue_returns_to_the_queue_keeping_only_the_previous_attempt() {
    for name in ["preparing", "live"] {
        let mut t = state_named(name);
        t.apply(TaskEvent::Requeue { note: "archived".into() }, 40).unwrap();
        assert_eq!(t.state(), &TaskState::Queued { previous_attempt: Some(prev("before")) }, "{name}");
        assert_eq!((t.note(), t.updated_at, t.attempts()), (Some("archived"), 40, 1), "{name}");
    }
}

#[test]
fn retry_requeues_with_the_outcome_of_the_attempt() {
    let mut t = state_named("failed_ended");
    t.set_last_report(Some("partial"));
    t.apply(TaskEvent::Retry, 41).unwrap();
    let expected = PreviousAttempt { summary: Some("partial".into()), note: Some("ended".into()) };
    assert_eq!(t.state(), &TaskState::Queued { previous_attempt: Some(expected) });
    assert_eq!((t.note(), t.last_report(), t.updated_at, t.attempts()), (None, None, 41, 1));

    // Senza report ne' nota non c'e' niente da ricordare.
    let mut t = state_named("failed_other");
    t.set_note(None);
    t.apply(TaskEvent::Retry, 42).unwrap();
    assert_eq!(t.state(), &TaskState::Queued { previous_attempt: None });
}

#[test]
fn mark_done_keeps_the_report_and_drops_the_failure_note() {
    let mut t = state_named("failed_ended");
    t.set_last_report(Some("partial"));
    t.apply(TaskEvent::MarkDone, 43).unwrap();
    let expected = Finished { started_at: Some(20), finished_at: 43, report: Some(report("partial")) };
    assert_eq!(t.state(), &TaskState::Done(expected));
    assert_eq!((t.note(), t.last_report(), t.updated_at), (None, Some("partial"), 43));
}

#[test]
fn reopen_needs_the_same_session_attempt_and_a_session_end() {
    let reopen = |session: &str, attempt| TaskEvent::Reopen { session: session.into(), attempt, note: "r".into() };
    let mut t = state_named("failed_ended");
    t.apply(reopen(W1, 1), 44).unwrap();
    let expected = Run {
        started_at: 20,
        previous_attempt: None,
        phase: RunPhase::Live { session: W1.into(), worker: WorkerActivity::Running },
    };
    assert_eq!(t.state(), &TaskState::Running(expected));
    assert_eq!((t.note(), t.updated_at, t.attempts(), t.finished_at()), (Some("r"), 44, 1, None));
    assert_eq!(t.worker_status(), Some(WorkerStatus::Running));

    for (session, attempt) in [("other", 1), (W1, 2), (W1, 0)] {
        let mut t = state_named("failed_ended");
        assert_eq!(t.apply(reopen(session, attempt), 44), Err(LifecycleError::Stale), "{session} {attempt}");
    }
    // Un fallimento per un'altra causa, o gia' ripreso da un retry, non si riapre.
    assert_eq!(state_named("failed_other").apply(reopen(W1, 1), 44), Err(LifecycleError::Stale));
    let mut retried = state_named("failed_ended");
    retried.apply(TaskEvent::Retry, 44).unwrap();
    assert_eq!(retried.apply(reopen(W1, 1), 45), Err(LifecycleError::Stale));
    // Senza un inizio noto riparte da adesso.
    let mut t = state_named("failed_ended");
    t.set_state(TaskState::Failed {
        finished: Finished { started_at: None, finished_at: 30, report: None },
        cause: FailCause::SessionEnded { session: W1.into() },
        reason: None,
    });
    t.apply(reopen(W1, 1), 46).unwrap();
    assert_eq!(t.started_at(), Some(46));
}

#[test]
fn notes_are_appended_once_and_replaced_within_the_limit() {
    let mut t = task();
    assert!(t.append_note("first", 1));
    assert!(t.append_note("second", 2));
    assert!(!t.append_note("first", 3), "gia' presente");
    assert_eq!((t.note(), t.updated_at), (Some("first; second"), 2));
    t.replace_note(&"x".repeat(NOTE_MAX + 50), 4);
    assert_eq!(t.note().map(|n| n.chars().count()), Some(NOTE_MAX));
    assert_eq!(t.updated_at, 4);
}

#[test]
fn e2e_runs_cover_the_task_run_and_the_copy_in_the_report() {
    let running = E2eRun::started("r1".into(), "j.xml".into(), 1);
    let mut t = state_named("done");
    t.e2e_run = Some(running.clone());
    t.report_mut().unwrap().e2e = Some(running);
    assert_eq!(t.e2e_runs_mut().count(), 2);
    let mut queued = state_named("queued");
    assert_eq!(queued.e2e_runs_mut().count(), 0);
}

#[test]
fn lifecycle_errors_read_like_the_old_messages() {
    assert_eq!(LifecycleError::NotFound("zz".into()).to_string(), "task not found: zz");
    let e =
        LifecycleError::NotAllowed { task: "a".into(), status: TaskStatus::Done, op: Op::Action(TaskAction::Retry) };
    assert_eq!(String::from(e), "task 'a' is done: only failed tasks can be retried");
    assert!(!LifecycleError::Stale.to_string().is_empty());
}

#[test]
fn every_failure_records_its_reason_and_retry_clears_it() {
    for reason in [
        FailReason::WorkerFailed,
        FailReason::WorkerExited,
        FailReason::Stopped,
        FailReason::Interrupted,
        FailReason::StartFailed,
    ] {
        let mut t = state_named("live");
        let outcome = Outcome::Failed(Failure { cause: FailCause::Other, reason });
        t.apply(TaskEvent::Finish { session: None, outcome, report: None, note: None }, 50).unwrap();
        assert_eq!(t.fail_reason(), Some(reason));
        t.apply(TaskEvent::Retry, 51).unwrap();
        assert_eq!(t.fail_reason(), None, "{reason:?}: Riprova azzera il motivo");
    }
}

#[test]
fn a_failure_without_a_recorded_reason_is_unknown_and_reopening_drops_it() {
    assert_eq!(state_named("failed_other").fail_reason(), None);
    let mut t = state_named("live");
    let ended = FailCause::SessionEnded { session: W1.into() };
    let outcome = Outcome::Failed(Failure { cause: ended, reason: FailReason::WorkerExited });
    t.apply(TaskEvent::Finish { session: None, outcome, report: None, note: None }, 50).unwrap();
    let reopen = TaskEvent::Reopen { session: W1.into(), attempt: 1, note: "r".into() };
    t.apply(reopen, 51).unwrap();
    assert_eq!(t.fail_reason(), None);
}
