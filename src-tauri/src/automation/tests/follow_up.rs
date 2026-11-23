//! Follow-up dei goal attraverso l'automazione (docs/GOALS.MD, "Follow-up"): il
//! report di un worker finisce nel goal alla chiusura (a mano o per consegna),
//! `get_goal` li restituisce, e solo l'utente ne cambia lo stato, dal comando
//! della UI.

use super::deliver::{goal_json, goal_view, host_with_commits};
use super::*;
use crate::board::{FollowUp, FollowUpState, GoalOutcome};

/// Il worker di `task_id` conclude con i `follow_ups` indicati.
fn complete_with_follow_ups(st: &AutomationState, h: &FakeHost, task_id: &str, follow_ups: &[&str]) {
    let session = session_of(st, h, task_id);
    call(st, h, &session, "get_task", json!({})).unwrap();
    let report = json!({ "status": "completed", "summary": "done", "follow_ups": follow_ups });
    call(st, h, &session, "complete_task", report).unwrap();
}

fn follow_ups_of(st: &AutomationState, h: &FakeHost) -> Vec<FollowUp> {
    goal_view(st, h, "g").goal.follow_ups().to_vec()
}

/// Goal `g` con il solo task `a` concluso con `follow_ups`, ancora aperto.
fn goal_with_follow_ups(follow_ups: &[&str]) -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::LocalMerge, "g", 2));
    let r = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": [spec("a", &[])], "goal": goal_json("g") }));
    boards::proposal_decide(&st, &h, "ws-1", r.unwrap()["proposal"].as_str().unwrap(), true).unwrap();
    complete_with_follow_ups(&st, &h, "a", follow_ups);
    (st, h)
}

#[test]
fn closing_by_hand_collects_the_reports_and_get_goal_returns_them() {
    let (st, h) = goal_with_follow_ups(&["add pagination", "Add Pagination", "log the exports"]);
    assert!(follow_ups_of(&st, &h).is_empty(), "collected at the closing, not before");
    let open = call(&st, &h, INTAKE, "get_goal", json!({ "id": "g" })).unwrap();
    assert_eq!(open["goal"]["follow_ups"], json!([]));

    let closed = boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(closed.goals[0].goal.outcome(), Some(GoalOutcome::Abandoned));
    let expected = json!([
        { "text": "add pagination", "source_task": "a", "state": "open" },
        { "text": "log the exports", "source_task": "a", "state": "open" }
    ]);
    let detail = call(&st, &h, INTAKE, "get_goal", json!({ "id": "g" })).unwrap();
    assert_eq!(detail["goal"]["follow_ups"], expected);
}

#[test]
fn the_user_dismisses_or_promotes_an_entry_and_the_state_is_kept() {
    let (st, h) = goal_with_follow_ups(&["one", "two", "three"]);
    boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    boards::goal_set_follow_up(&st, &h, "ws-1", "g", 0, FollowUpState::Dismissed).unwrap();
    let v = boards::goal_set_follow_up(&st, &h, "ws-1", "g", 2, FollowUpState::Promoted).unwrap();
    let states = |goal: &crate::board::GoalView| goal.goal.follow_ups().iter().map(|f| f.state).collect::<Vec<_>>();
    assert_eq!(states(&v.goals[0]), [FollowUpState::Dismissed, FollowUpState::Open, FollowUpState::Promoted]);
    assert_eq!(states(&goal_view(&st, &h, "g")), states(&v.goals[0]), "the next view has the same states");

    let again = boards::goal_set_follow_up(&st, &h, "ws-1", "g", 0, FollowUpState::Promoted).unwrap_err();
    assert_eq!(again, "follow-up 0 is already dismissed");
    let open = boards::goal_set_follow_up(&st, &h, "ws-1", "g", 1, FollowUpState::Open).unwrap_err();
    assert_eq!(open, "a follow-up can only be dismissed or promoted");
    let unknown = boards::goal_set_follow_up(&st, &h, "ws-1", "nope", 0, FollowUpState::Dismissed).unwrap_err();
    assert_eq!(unknown, "unknown goal: nope");
}

#[test]
fn no_tool_changes_the_state_of_an_entry() {
    let (st, h) = goal_with_follow_ups(&["one"]);
    boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    for method in ["set_follow_up", "goal_set_follow_up", "promote_follow_up", "update_goal"] {
        let params = json!({ "id": "g", "index": 0, "state": "promoted" });
        assert!(call(&st, &h, INTAKE, method, params).is_err(), "{method}");
    }
    assert_eq!(follow_ups_of(&st, &h)[0].state, FollowUpState::Open);
}
