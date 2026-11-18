//! Le domande del worker: ciclo domanda -> risposta, rifiuti e cronologia.

use super::*;
use crate::board::{FailCause, FailReason, Failure, Outcome, TaskEvent, TaskSpec, TaskStatus};

const W1: &str = "w1";

fn spec(id: &str) -> TaskSpec {
    TaskSpec { id: id.into(), title: id.to_uppercase(), task: format!("do {id}"), ..TaskSpec::default() }
}

/// Task al tentativo 1 con il worker `w1` vivo e al lavoro.
fn live() -> Task {
    let mut t = Task::from_spec(spec("a"), 1);
    t.apply(TaskEvent::Start, 2).unwrap();
    let spawned = TaskEvent::WorkerSpawned { attempt: 1, session: W1.into(), worktree: None, note: None };
    t.apply(spawned, 3).unwrap();
    t.apply(TaskEvent::WorkerChanged { session: W1.into(), activity: WorkerActivity::Running }, 4).unwrap();
    t
}

fn ask(t: &mut Task, session: &str, text: &str, now: i64) -> Result<(), LifecycleError> {
    t.apply(TaskEvent::QuestionAsked { session: session.into(), text: text.into(), options: vec![] }, now)
}

fn answer(t: &mut Task, session: &str, id: &str, text: Option<&str>, now: i64) -> Result<(), LifecycleError> {
    let event = TaskEvent::QuestionAnswered {
        session: session.into(),
        question_id: id.into(),
        answer: text.map(str::to_string),
    };
    t.apply(event, now)
}

fn undelivered(t: &mut Task, session: &str, id: &str) -> Result<(), LifecycleError> {
    t.apply(TaskEvent::AnswerUndelivered { session: session.into(), question_id: id.into() }, 99)
}

fn worker(t: &Task) -> Option<WorkerActivity> {
    t.live_activity(W1)
}

#[test]
fn a_question_is_pending_with_its_id_and_options() {
    let mut t = live();
    let options = vec!["Postgres".to_string(), "SQLite".to_string()];
    let event = TaskEvent::QuestionAsked { session: W1.into(), text: "Which db?".into(), options: options.clone() };
    t.apply(event, 10).unwrap();
    let q = t.pending_question().expect("pending");
    assert_eq!((q.id.as_str(), q.attempt, q.text.as_str(), q.asked_at), ("q-1-1", 1, "Which db?", 10));
    assert_eq!((&q.options, q.answered_at, &q.answer), (&options, None, &None));
    assert_eq!(t.updated_at, 10);
}

#[test]
fn a_second_question_is_refused_while_the_first_waits() {
    let mut t = live();
    ask(&mut t, W1, "first", 10).unwrap();
    assert_eq!(ask(&mut t, W1, "second", 11), Err(LifecycleError::Stale));
    assert_eq!(t.questions().len(), 1);
}

#[test]
fn only_the_live_worker_can_ask() {
    let mut t = live();
    assert_eq!(ask(&mut t, "other", "q", 10), Err(LifecycleError::Stale));
    let mut queued = Task::from_spec(spec("b"), 1);
    assert_eq!(ask(&mut queued, W1, "q", 10), Err(LifecycleError::Stale));
    let mut preparing = Task::from_spec(spec("c"), 1);
    preparing.apply(TaskEvent::Start, 2).unwrap();
    assert_eq!(ask(&mut preparing, W1, "q", 10), Err(LifecycleError::Stale));
    assert!(t.questions().is_empty() && queued.questions().is_empty() && preparing.questions().is_empty());
}

#[test]
fn answering_closes_the_question_and_resumes_the_worker() {
    let mut t = live();
    ask(&mut t, W1, "Which db?", 10).unwrap();
    t.apply(TaskEvent::WorkerChanged { session: W1.into(), activity: WorkerActivity::AwaitingAnswer }, 11).unwrap();
    answer(&mut t, W1, "q-1-1", Some("Postgres"), 20).unwrap();
    let q = &t.questions()[0];
    assert_eq!((q.answered_at, q.answer.as_deref()), (Some(20), Some("Postgres")));
    assert_eq!(worker(&t), Some(WorkerActivity::Running));
    assert!(t.pending_question().is_none());
    // Rispondere due volte: la seconda e' rifiutata.
    assert_eq!(answer(&mut t, W1, "q-1-1", Some("again"), 21), Err(LifecycleError::Stale));
    // Un'altra domanda e' di nuovo ammessa, con il numero successivo.
    ask(&mut t, W1, "More?", 30).unwrap();
    assert_eq!(t.pending_question().map(|q| q.id.as_str()), Some("q-1-2"));
}

#[test]
fn an_answer_given_in_the_terminal_has_no_text() {
    let mut t = live();
    ask(&mut t, W1, "Which db?", 10).unwrap();
    answer(&mut t, W1, "q-1-1", None, 20).unwrap();
    let q = &t.questions()[0];
    assert_eq!((q.answered_at, &q.answer), (Some(20), &None));
}

#[test]
fn an_answer_for_another_session_or_question_is_stale() {
    let mut t = live();
    ask(&mut t, W1, "q", 10).unwrap();
    assert_eq!(answer(&mut t, "other", "q-1-1", None, 20), Err(LifecycleError::Stale));
    assert_eq!(answer(&mut t, W1, "q-1-2", None, 20), Err(LifecycleError::Stale));
    assert!(t.pending_question().is_some());
}

#[test]
fn an_undelivered_answer_puts_the_question_back_to_waiting() {
    let mut t = live();
    ask(&mut t, W1, "q", 10).unwrap();
    answer(&mut t, W1, "q-1-1", Some("yes"), 20).unwrap();
    undelivered(&mut t, W1, "q-1-1").unwrap();
    let q = t.pending_question().expect("pending again");
    assert_eq!((q.answered_at, &q.answer), (None, &None));
    assert_eq!(worker(&t), Some(WorkerActivity::AwaitingAnswer));
    // Non c'e' piu' nulla da annullare.
    assert_eq!(undelivered(&mut t, W1, "q-1-1"), Err(LifecycleError::Stale));
}

#[test]
fn only_an_answer_of_the_app_can_be_undone() {
    let mut t = live();
    ask(&mut t, W1, "q", 10).unwrap();
    answer(&mut t, W1, "q-1-1", None, 20).unwrap();
    assert_eq!(undelivered(&mut t, W1, "q-1-1"), Err(LifecycleError::Stale));
    ask(&mut t, W1, "q2", 30).unwrap();
    answer(&mut t, W1, "q-1-2", Some("x"), 31).unwrap();
    assert_eq!(undelivered(&mut t, "other", "q-1-2"), Err(LifecycleError::Stale));
    assert_eq!(undelivered(&mut t, W1, "q-1-1"), Err(LifecycleError::Stale));
    assert!(t.questions()[1].answered_at.is_some());
}

#[test]
fn the_history_keeps_the_latest_questions_with_unique_ids() {
    let mut t = live();
    let total = QUESTIONS_KEPT + 5;
    for n in 1..=total {
        ask(&mut t, W1, &format!("question {n}"), n as i64).unwrap();
        answer(&mut t, W1, &format!("q-1-{n}"), Some("ok"), n as i64).unwrap();
    }
    assert_eq!(t.questions().len(), QUESTIONS_KEPT);
    assert_eq!(t.questions().first().map(|q| q.id.as_str()), Some("q-1-6"));
    assert_eq!(t.questions().last().map(|q| q.id.as_str()), Some("q-1-25"));
    ask(&mut t, W1, "one more", 100).unwrap();
    assert_eq!(t.pending_question().map(|q| q.id.as_str()), Some("q-1-26"));
}

#[test]
fn a_finished_task_has_no_pending_question() {
    let mut t = live();
    ask(&mut t, W1, "q", 10).unwrap();
    let finish = TaskEvent::Finish { session: Some(W1.into()), outcome: Outcome::Done, report: None, note: None };
    t.apply(finish, 20).unwrap();
    assert_eq!(t.status(), TaskStatus::Done);
    assert!(t.pending_question().is_none());
    assert_eq!(t.questions().len(), 1, "the history stays");
    assert_eq!(answer(&mut t, W1, "q-1-1", None, 30), Err(LifecycleError::Stale));
}

#[test]
fn a_new_attempt_does_not_inherit_the_pending_question() {
    let mut t = live();
    ask(&mut t, W1, "q", 10).unwrap();
    let failed = TaskEvent::Finish {
        session: Some(W1.into()),
        outcome: Outcome::Failed(Failure { cause: FailCause::Other, reason: FailReason::WorkerFailed }),
        report: None,
        note: None,
    };
    t.apply(failed, 20).unwrap();
    t.apply(TaskEvent::Retry, 21).unwrap();
    t.apply(TaskEvent::Start, 22).unwrap();
    assert_eq!(t.attempts(), 2);
    assert!(t.pending_question().is_none(), "the question belongs to attempt 1");
    let spawned = TaskEvent::WorkerSpawned { attempt: 2, session: "w2".into(), worktree: None, note: None };
    t.apply(spawned, 23).unwrap();
    ask(&mut t, "w2", "again", 24).unwrap();
    assert_eq!(t.pending_question().map(|q| q.id.as_str()), Some("q-2-1"));
}
