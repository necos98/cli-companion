//! Correzione automatica attraverso l'automazione (docs/GOALS.MD, "Giri e
//! correzione automatica"): la proposta del revisore entra in coda da sola con lo
//! stesso percorso dell'accettazione manuale, il tetto dei giri ferma il goal, il
//! percorso completo fino alla consegna senza interventi, la validazione fallita
//! e l'HUB che decide la stessa proposta.

use super::deliver::{complete_work, deliverer_of, goal_json, goal_view, host_with_commits, review_spec};
use super::*;
use crate::board::{Acceptance, AutoFixHold, DecisionSource, GoalOutcome, GoalPhase, GoalWaiting, TaskKind};

/// Goal `g` con il task `a` (concluso) e la review `rev`, accettato dall'utente con
/// `rounds` giri di correzione automatica e la sua scelta sulla consegna automatica.
fn reviewed_goal(rounds: u8, auto_deliver: bool) -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::Commit, "g", 2));
    let tasks = vec![spec("a", &[]), review_spec("rev", &["a"])];
    let r = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": tasks, "goal": goal_json("g") })).unwrap();
    let choices = Acceptance { auto_deliver: Some(auto_deliver), auto_fix: Some(rounds), automatic: false };
    boards::proposal_decide_with(&st, &h, "ws-1", r["proposal"].as_str().unwrap(), true, choices).unwrap();
    complete_work(&st, &h, "a");
    (st, h)
}

/// Il revisore di `review` propone la correzione `fix-<n>` con la review
/// `rev-<n>` e chiude con problemi; ritorna l'id della proposta.
fn review_with_issues(st: &AutomationState, h: &FakeHost, review: &str, n: u32) -> String {
    let r = session_of(st, h, review);
    call(st, h, &r, "get_task", json!({})).unwrap();
    let fix = format!("fix-{n}");
    let tasks = json!({ "tasks": [spec(&fix, &["a"]), review_spec(&format!("rev-{n}"), &[&fix])] });
    let p = call(st, h, &r, "propose_tasks", tasks).unwrap();
    let issues = json!({ "status": "completed", "summary": "r", "verdict": "issues", "findings": ["broken"] });
    call(st, h, &r, "complete_task", issues).unwrap();
    p["proposal"].as_str().unwrap().to_string()
}

fn pending(st: &AutomationState, h: &FakeHost, pid: &str) -> Option<crate::board::Proposal> {
    view(st, h).proposals.into_iter().find(|p| p.id == pid)
}

fn app_decisions(st: &AutomationState, h: &FakeHost) -> Vec<String> {
    let g = goal_view(st, h, "g");
    g.goal.decisions.iter().filter(|d| d.source == DecisionSource::App).map(|d| d.text.clone()).collect()
}

#[test]
fn without_auto_fix_the_correction_waits_for_the_user_as_before() {
    let (st, h) = reviewed_goal(0, false);
    let pid = review_with_issues(&st, &h, "rev", 2);
    let p = pending(&st, &h, &pid).expect("still pending");
    assert_eq!(p.auto_fix_hold, None);
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.waiting, g.goal.round()), (GoalPhase::Deciding, vec![GoalWaiting::Proposal], 1));
    assert!(app_decisions(&st, &h).is_empty());
}

#[test]
fn the_correction_enters_the_queue_by_itself_with_the_next_round() {
    let (st, h) = reviewed_goal(2, false);
    let pid = review_with_issues(&st, &h, "rev", 2);
    assert!(pending(&st, &h, &pid).is_none(), "accepted automatically");
    assert_eq!(
        (task(&st, &h, "fix-2")["round"].clone(), task(&st, &h, "rev-2")["round"].clone()),
        (json!(2), json!(2))
    );
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.goal.round(), g.goal.auto_rounds(), g.phase), (2, &[2][..], GoalPhase::Executing));
    assert_eq!(app_decisions(&st, &h), ["Round 2 accepted automatically: 2 tasks"]);
    assert!(st.registry().auto_fixing.is_empty(), "nothing left in flight");
}

#[test]
fn a_review_that_always_finds_issues_stops_at_the_last_automatic_round() {
    let (st, h) = reviewed_goal(2, true);
    let mut review = "rev".to_string();
    let mut stopped_at = None;
    // Il ciclo ha un tetto per costruzione: il test non dipende dal limite del `for`.
    for n in 2..=10 {
        let pid = review_with_issues(&st, &h, &review, n);
        if pending(&st, &h, &pid).is_some() {
            stopped_at = Some(n);
            break;
        }
        complete_work(&st, &h, &format!("fix-{n}"));
        review = format!("rev-{n}");
    }
    assert_eq!(stopped_at, Some(4), "rounds 2 and 3 are automatic, the fourth waits for the user");
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.goal.round(), g.goal.auto_rounds()), (3, &[2, 3][..]));
    assert_eq!((g.phase, g.waiting), (GoalPhase::Deciding, vec![GoalWaiting::RoundLimit]));
    assert_eq!(app_decisions(&st, &h).len(), 2);
    assert!(view(&st, &h).tasks.iter().all(|t| t.task.kind != TaskKind::Deliver), "issues: no delivery");
}

#[test]
fn with_auto_fix_and_auto_deliver_the_goal_reaches_the_base_without_the_user() {
    let (st, h) = reviewed_goal(1, true);
    review_with_issues(&st, &h, "rev", 2);
    complete_work(&st, &h, "fix-2");
    let r = session_of(&st, &h, "rev-2");
    call(&st, &h, &r, "get_task", json!({})).unwrap();
    let passed = json!({ "status": "completed", "summary": "ok", "verdict": "passed" });
    call(&st, &h, &r, "complete_task", passed).unwrap();
    // La consegna parte da sola: l'ultima review e' passata.
    let w = deliverer_of(&st, &h, "g");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    let delivered = json!({ "status": "completed", "summary": "Merged", "commits": ["abc"] });
    call(&st, &h, &w, "complete_task", delivered).unwrap();
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.goal.outcome()), (GoalPhase::Closed, Some(GoalOutcome::Delivered)));
    assert_eq!((g.goal.round(), g.goal.auto_rounds()), (2, &[2][..]));
    assert_eq!(app_decisions(&st, &h), ["Round 2 accepted automatically: 2 tasks"]);
}

#[test]
fn a_failed_validation_keeps_the_proposal_pending_with_the_error() {
    let (st, h) = reviewed_goal(0, false);
    let r = session_of(&st, &h, "rev");
    let mut fix = spec("fix", &["a"]);
    fix["bot"] = json!("test-writer");
    let p = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [fix] })).unwrap();
    let pid = p["proposal"].as_str().unwrap();
    // Il Bot si spegne prima che l'utente alzi i giri: l'accettazione automatica fallisce.
    h.bots.0.lock().unwrap().iter_mut().filter(|b| b.id == "test-writer").for_each(|b| b.enabled = false);
    boards::goal_set_auto_fix(&st, &h, "ws-1", "g", 2).unwrap();
    let p = pending(&st, &h, pid).expect("still pending");
    match p.auto_fix_hold {
        Some(AutoFixHold::Invalid { error }) => assert!(error.contains("disabled"), "{error}"),
        other => panic!("expected a validation error, got {other:?}"),
    }
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.goal.round(), g.waiting.clone()), (1, vec![GoalWaiting::Proposal]));
    assert!(st.registry().auto_fixing.is_empty());
    assert!(app_decisions(&st, &h).is_empty());
}

#[test]
fn an_ineligible_correction_waits_with_its_reason() {
    let (st, h) = reviewed_goal(2, false);
    let r = session_of(&st, &h, "rev");
    let mut fix = spec("fix", &["a"]);
    fix["bot"] = json!("git-operator");
    h.bots.0.lock().unwrap().iter_mut().filter(|b| b.id == "git-operator").for_each(|b| b.enabled = true);
    let p = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [fix] })).unwrap();
    let p = pending(&st, &h, p["proposal"].as_str().unwrap()).expect("still pending");
    assert_eq!(p.auto_fix_hold, Some(AutoFixHold::Bot { task: "fix".into(), bot: "git-operator".into() }));
    assert_eq!(goal_view(&st, &h, "g").waiting, [GoalWaiting::Proposal]);
}

#[test]
fn the_hub_gets_a_clean_error_on_a_proposal_already_accepted_automatically() {
    let (st, h) = reviewed_goal(1, false);
    let pid = review_with_issues(&st, &h, "rev", 2);
    // `decide_hub_proposal` usa `proposal_decide`: la proposta non e' piu' pendente.
    let err = boards::proposal_decide(&st, &h, "ws-1", &pid, true).unwrap_err();
    assert_eq!(err, format!("proposal not found: {pid}"));
    assert_eq!(goal_view(&st, &h, "g").goal.round(), 2, "accepted once");
}

#[test]
fn a_proposal_decided_before_the_automatic_acceptance_runs_is_left_alone() {
    let (st, h) = reviewed_goal(1, false);
    h.hold_defers();
    let pid = review_with_issues(&st, &h, "rev", 2);
    // L'utente (o l'HUB) la accetta mentre l'accettazione automatica e' in coda.
    boards::proposal_decide(&st, &h, "ws-1", &pid, true).unwrap();
    h.run_held();
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.goal.round(), g.goal.auto_rounds()), (2, &[][..]), "a single round, decided by the user");
    assert!(app_decisions(&st, &h).is_empty());
    assert!(st.registry().auto_fixing.is_empty());
}

#[test]
fn the_rounds_are_set_by_the_user_from_the_goal_detail() {
    let (st, h) = reviewed_goal(0, false);
    let v = boards::goal_set_auto_fix(&st, &h, "ws-1", "g", 3).unwrap();
    assert_eq!(v.goals[0].goal.auto_fix().max_rounds, 3);
    let err = boards::goal_set_auto_fix(&st, &h, "ws-1", "g", 4).unwrap_err();
    assert_eq!(err, "auto_fix max_rounds must be between 0 and 3");
    assert_eq!(boards::goal_set_auto_fix(&st, &h, "ws-1", "nope", 1).unwrap_err(), "unknown goal: nope");
}

#[test]
fn the_auto_fix_choice_in_a_decision_needs_a_proposal_with_a_new_goal() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let r = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": [spec("loose", &[])] })).unwrap();
    let id = r["proposal"].as_str().unwrap();
    let two = Acceptance { auto_fix: Some(2), ..Acceptance::default() };
    let err = boards::proposal_decide_with(&st, &h, "ws-1", id, true, two).unwrap_err();
    assert!(err.contains("new goal"), "{err}");
    let automatic = boards::proposal_decide_with(&st, &h, "ws-1", id, true, Acceptance::AUTOMATIC).unwrap_err();
    assert_eq!(automatic, format!("proposal {id} is not accepted automatically"));
}
