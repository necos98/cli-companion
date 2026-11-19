//! Follow-up: raccolta alla chiusura, doppioni, tetto, le due chiusure e le
//! transizioni di stato che decide l'utente.

use super::super::tests::{board_with_goal, report};
use super::*;
use crate::board::{Finished, GoalOutcome, TaskReport, TaskState, Verdict};

fn follow(text: &str, source: &str, state: FollowUpState) -> FollowUp {
    FollowUp { text: text.into(), source_task: source.into(), state }
}

fn open(text: &str, source: &str) -> FollowUp {
    follow(text, source, FollowUpState::Open)
}

/// Conclude il task `id` con il report `r`.
fn finish_with(b: &mut Board, id: &str, r: TaskReport) {
    let finished = Finished { started_at: None, finished_at: 20, report: Some(r) };
    b.task_mut(id).unwrap().set_state(TaskState::Done(finished));
}

fn work_report(follow_ups: &[&str]) -> TaskReport {
    TaskReport { follow_ups: follow_ups.iter().map(|s| s.to_string()).collect(), ..report(None) }
}

fn review_report(verdict: Verdict, findings: &[&str]) -> TaskReport {
    TaskReport { findings: findings.iter().map(|s| s.to_string()).collect(), ..report(Some(verdict)) }
}

/// Il goal `g` con il lavoro `a` e la review `rev` concluse, pronto a chiudersi.
fn finished_board(work: TaskReport, review: TaskReport) -> Board {
    let mut b = board_with_goal();
    finish_with(&mut b, "a", work);
    finish_with(&mut b, "rev", review);
    b
}

fn follow_ups_of(b: &Board) -> Vec<FollowUp> {
    b.goal("g").unwrap().follow_ups().to_vec()
}

#[test]
fn closing_collects_the_follow_ups_of_the_reports_and_the_suggestions_of_a_passed_review() {
    let mut b =
        finished_board(work_report(&["add pagination"]), review_report(Verdict::Passed, &["rename the helper"]));
    b.advance_goals(30);
    assert_eq!(
        follow_ups_of(&b),
        [open("add pagination", "a"), open("rename the helper", "rev")],
        "follow-ups of a task first, then its findings, in queue order"
    );
}

#[test]
fn the_findings_of_a_review_with_issues_are_not_follow_ups() {
    let mut b = finished_board(work_report(&[]), review_report(Verdict::Issues, &["broken"]));
    b.advance_goals(30);
    assert!(b.goal("g").unwrap().follow_ups().is_empty());
    // I `follow_ups` di una review con problemi restano follow-up.
    let mut review = review_report(Verdict::Issues, &["broken"]);
    review.follow_ups = vec!["tidy the tests".into()];
    let mut b = finished_board(work_report(&[]), review);
    b.advance_goals(30);
    assert_eq!(follow_ups_of(&b), [open("tidy the tests", "rev")]);
}

#[test]
fn findings_of_a_work_task_are_not_collected() {
    let mut work = work_report(&[]);
    work.findings = vec!["not a suggestion".into()];
    let mut b = finished_board(work, review_report(Verdict::Passed, &[]));
    b.advance_goals(30);
    assert!(b.goal("g").unwrap().follow_ups().is_empty());
}

#[test]
fn duplicates_are_dropped_comparing_the_normalized_text() {
    let mut b = finished_board(
        work_report(&["Add  Pagination", "", "   ", "add pagination"]),
        review_report(Verdict::Passed, &["  ADD pagination\n", "something else"]),
    );
    b.advance_goals(30);
    assert_eq!(
        follow_ups_of(&b),
        [open("Add  Pagination", "a"), open("something else", "rev")],
        "the first one stays, with its source; blanks are dropped"
    );
}

#[test]
fn the_entries_over_the_cap_are_discarded() {
    let many: Vec<String> = (0..MAX_FOLLOW_UPS + 5).map(|i| format!("idea {i}")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let mut b = finished_board(work_report(&refs), review_report(Verdict::Passed, &["one more"]));
    let collected = collect(b.goal_tasks("g"));
    assert_eq!((collected.items.len(), collected.dropped), (MAX_FOLLOW_UPS, 6));
    b.advance_goals(30);
    let kept = follow_ups_of(&b);
    assert_eq!(kept.len(), MAX_FOLLOW_UPS);
    assert_eq!(kept.last().unwrap().text, format!("idea {}", MAX_FOLLOW_UPS - 1));
}

#[test]
fn a_delivered_and_an_abandoned_goal_both_keep_their_follow_ups() {
    let mut delivered = finished_board(work_report(&["x"]), review_report(Verdict::Passed, &["y"]));
    delivered.advance_goals(30);
    assert_eq!(delivered.goal("g").unwrap().outcome(), Some(GoalOutcome::Delivered));
    assert_eq!(follow_ups_of(&delivered).len(), 2);

    let mut abandoned = board_with_goal();
    finish_with(&mut abandoned, "a", work_report(&["x"]));
    abandoned.close_goal("g", 30).unwrap();
    assert_eq!(abandoned.goal("g").unwrap().outcome(), Some(GoalOutcome::Abandoned));
    assert_eq!(follow_ups_of(&abandoned), [open("x", "a")], "the queued review is removed, the done task stays");
}

#[test]
fn an_open_goal_has_no_follow_ups_and_a_closed_one_keeps_them_across_a_reload() {
    let mut b = finished_board(work_report(&["x"]), review_report(Verdict::Passed, &[]));
    assert!(b.goal("g").unwrap().follow_ups().is_empty());
    b.advance_goals(30);
    b.set_follow_up_state("g", 0, FollowUpState::Promoted, 31).unwrap();
    let json = serde_json::to_string(&b.goal("g").unwrap()).unwrap();
    let mut back: Goal = serde_json::from_str(&json).unwrap();
    back.sanitize();
    assert_eq!(back.follow_ups(), [follow("x", "a", FollowUpState::Promoted)]);
    // Su un goal aperto le voci lette dal file non valgono.
    back.closed_at = None;
    back.sanitize();
    assert!(back.follow_ups().is_empty());
}

#[test]
fn a_file_with_blank_or_too_many_follow_ups_is_brought_within_the_limits() {
    let mut b = finished_board(work_report(&[]), review_report(Verdict::Passed, &[]));
    b.advance_goals(30);
    let goal = b.goal_mut("g").unwrap();
    goal.follow_ups = vec![open("  ", "a"), open(&"z".repeat(LIST_ITEM_MAX + 10), "a")];
    goal.follow_ups.extend((0..MAX_FOLLOW_UPS + 3).map(|i| open(&format!("n{i}"), "a")));
    goal.sanitize();
    assert_eq!(goal.follow_ups().len(), MAX_FOLLOW_UPS);
    assert_eq!(goal.follow_ups()[0].text.chars().count(), LIST_ITEM_MAX);
}

#[test]
fn only_the_user_moves_an_open_entry_to_dismissed_or_promoted() {
    let mut b = finished_board(work_report(&["x", "y", "z"]), review_report(Verdict::Passed, &[]));
    b.advance_goals(30);
    b.set_follow_up_state("g", 0, FollowUpState::Dismissed, 40).unwrap();
    b.set_follow_up_state("g", 2, FollowUpState::Promoted, 41).unwrap();
    let states: Vec<FollowUpState> = follow_ups_of(&b).iter().map(|f| f.state).collect();
    assert_eq!(states, [FollowUpState::Dismissed, FollowUpState::Open, FollowUpState::Promoted]);
    assert_eq!(b.updated_at, 41);

    let err = |b: &mut Board, i: usize, s: FollowUpState| b.set_follow_up_state("g", i, s, 50).unwrap_err();
    assert_eq!(err(&mut b, 0, FollowUpState::Promoted), "follow-up 0 is already dismissed");
    assert_eq!(err(&mut b, 2, FollowUpState::Dismissed), "follow-up 2 is already promoted");
    assert_eq!(err(&mut b, 1, FollowUpState::Open), "a follow-up can only be dismissed or promoted");
    assert_eq!(err(&mut b, 3, FollowUpState::Dismissed), "unknown follow-up: 3");
    assert_eq!(b.set_follow_up_state("nope", 0, FollowUpState::Dismissed, 50).unwrap_err(), "unknown goal: nope");
    assert_eq!(b.updated_at, 41, "a refused change does not touch the board");
}

#[test]
fn the_state_serializes_in_snake_case() {
    let f = follow("x", "a", FollowUpState::Promoted);
    assert_eq!(
        serde_json::to_value(&f).unwrap(),
        serde_json::json!({ "text": "x", "source_task": "a", "state": "promoted" })
    );
}

#[test]
fn a_task_without_a_report_leaves_nothing() {
    let mut b = board_with_goal();
    b.close_goal("g", 30).unwrap();
    assert!(b.goal("g").unwrap().follow_ups().is_empty());
}
