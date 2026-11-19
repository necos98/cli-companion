//! Correzione automatica: la decisione su ogni proposta (ogni criterio di
//! ammissibilita', il tetto dei giri), l'attesa del goal, l'accettazione
//! automatica con la sua riga nel registro e l'impostazione dell'utente.

use super::super::tests::{board_with_goal, finish, in_goal, new_goal, review, spec, view, CTX};
use super::*;
use crate::board::{GoalWaiting, ProposalInput, SpecContext, TaskSpec, Verdict};
use crate::bots::{default_agent_bots, AgentBot};

/// Il goal `g` con `max_rounds` giri automatici: `a` e la review `rev` concluse
/// con problemi.
fn reviewed_goal(max_rounds: u8) -> Board {
    let mut b = board_with_goal();
    b.set_goal_auto_fix("g", max_rounds, 11).unwrap();
    finish(&mut b, "a", 12, None);
    finish(&mut b, "rev", 13, Some(Verdict::Issues));
    b
}

/// Il revisore di `rev` propone `tasks` (la proposta `pid` eredita il goal).
fn correction(b: &mut Board, pid: &str, tasks: Vec<TaskSpec>) {
    correction_with(b, pid, tasks, CTX);
}

fn correction_with(b: &mut Board, pid: &str, tasks: Vec<TaskSpec>, ctx: SpecContext<'_>) {
    b.add_proposal(pid.into(), "rev-tab", Some("rev".into()), ProposalInput::from(tasks), ctx, 20).unwrap();
}

fn decision(b: &Board, pid: &str) -> AutoFixDecision {
    auto_fix_decision(b, b.proposal(pid).unwrap())
}

fn hold(b: &Board, pid: &str) -> Option<AutoFixHold> {
    match decision(b, pid) {
        AutoFixDecision::Hold(h) => Some(h),
        _ => None,
    }
}

/// I Bot di serie, con Git Operator acceso dall'utente.
fn bots_with_git_operator() -> Vec<AgentBot> {
    let mut bots = default_agent_bots();
    bots.iter_mut().filter(|b| b.id == "git-operator").for_each(|b| b.enabled = true);
    bots
}

#[test]
fn without_auto_fix_every_proposal_is_decided_by_the_user_as_before() {
    let mut b = reviewed_goal(0);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Manual);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
    let advance = b.advance_goals(30);
    assert!(advance.auto_fix.is_empty());
    assert_eq!(b.proposal("p-fix").unwrap().auto_fix_hold, None);
    assert_eq!(b.goal("g").unwrap().auto_fix(), AutoFix::default(), "off by default");
}

#[test]
fn a_correction_with_rounds_left_is_accepted_automatically_and_recorded() {
    let mut b = reviewed_goal(2);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"]), review("rev-2", &["fix"])]);
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Accept);
    let v = view(&b, "g");
    assert!(v.waiting.is_empty(), "the goal goes on by itself: {:?}", v.waiting);
    assert_eq!(b.advance_goals(30).auto_fix, ["p-fix"]);
    let ids = b.accept_proposal_with("p-fix", Acceptance::AUTOMATIC, CTX, 31).unwrap();
    assert_eq!(ids, ["fix", "rev-2"]);
    let g = b.goal("g").unwrap();
    assert_eq!((g.round(), g.auto_rounds()), (2, &[2][..]));
    assert_eq!(b.task("fix").unwrap().round(), 2);
    let last = g.decisions.last().unwrap();
    assert_eq!(
        (last.source, last.text.as_str(), last.at),
        (DecisionSource::App, "Round 2 accepted automatically: 2 tasks", 31)
    );
}

#[test]
fn the_record_counts_a_single_task_in_the_singular() {
    let mut b = reviewed_goal(1);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    b.accept_proposal_with("p-fix", Acceptance::AUTOMATIC, CTX, 31).unwrap();
    let last = b.goal("g").unwrap().decisions.last().unwrap().text.clone();
    assert_eq!(last, "Round 2 accepted automatically: 1 task");
}

#[test]
fn past_the_rounds_the_correction_waits_for_the_user_with_round_limit() {
    let mut b = reviewed_goal(1);
    correction(&mut b, "p-1", vec![spec("fix", &["a"]), review("rev-2", &["fix"])]);
    b.accept_proposal_with("p-1", Acceptance::AUTOMATIC, CTX, 31).unwrap();
    finish(&mut b, "fix", 32, None);
    finish(&mut b, "rev-2", 33, Some(Verdict::Issues));
    b.add_proposal("p-2".into(), "rev-tab", Some("rev-2".into()), vec![spec("fix-2", &["fix"])].into(), CTX, 34)
        .unwrap();
    assert_eq!(decision(&b, "p-2"), AutoFixDecision::RoundLimit);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::RoundLimit]);
    assert!(b.advance_goals(35).auto_fix.is_empty());
    let err = b.accept_proposal_with("p-2", Acceptance::AUTOMATIC, CTX, 36).unwrap_err();
    assert_eq!(err, "proposal p-2 is not accepted automatically");
    // L'utente la accetta: il giro avanza ma non e' automatico.
    b.accept_proposal("p-2", CTX, 37).unwrap();
    let g = b.goal("g").unwrap();
    assert_eq!((g.round(), g.auto_rounds()), (3, &[2][..]));
    assert!(g.decisions.iter().all(|d| !d.text.starts_with("Round 3")));
}

#[test]
fn raising_the_rounds_releases_a_correction_held_at_the_limit() {
    let mut b = reviewed_goal(0);
    b.goal_mut("g").unwrap().round = 2;
    b.set_goal_auto_fix("g", 1, 20).unwrap();
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::RoundLimit);
    b.set_goal_auto_fix("g", 2, 21).unwrap();
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Accept);
}

#[test]
fn a_proposal_of_the_intake_is_never_accepted_automatically() {
    let mut b = reviewed_goal(3);
    b.add_proposal("p-more".into(), "intake", None, in_goal("g", vec![spec("more", &[])]), CTX, 20).unwrap();
    assert_eq!(decision(&b, "p-more"), AutoFixDecision::Manual);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
    let mut loose = Board::new("ws-1");
    loose.add_proposal("p-new".into(), "intake", None, new_goal("n", vec![spec("x", &[])]), CTX, 1).unwrap();
    assert_eq!(decision(&loose, "p-new"), AutoFixDecision::Manual);
}

#[test]
fn a_proposal_whose_review_is_gone_is_held() {
    let mut b = reviewed_goal(3);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    b.proposals[0].source_task = Some("a".into());
    assert_eq!(hold(&b, "p-fix"), Some(AutoFixHold::NotFromReview));
    b.proposals[0].source_task = Some("gone".into());
    assert_eq!(hold(&b, "p-fix"), Some(AutoFixHold::NotFromReview));
}

#[test]
fn only_work_and_review_tasks_are_accepted_automatically() {
    let mut b = reviewed_goal(3);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"]), review("rev-2", &["fix"])]);
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Accept);
    for kind in [TaskKind::Plan, TaskKind::Deliver] {
        b.proposals[0].tasks[0].kind = kind;
        assert_eq!(hold(&b, "p-fix"), Some(AutoFixHold::Kind { task: "fix".into(), kind }));
    }
}

#[test]
fn a_repository_the_goal_does_not_use_holds_the_proposal() {
    let mut b = reviewed_goal(3);
    let mut api = spec("fix", &[]);
    api.repo = Some("api".into());
    correction(&mut b, "p-fix", vec![api]);
    let h = hold(&b, "p-fix");
    assert_eq!(h, Some(AutoFixHold::Repo { task: "fix".into(), repo: "api".into() }));
    // Un repository gia' usato da un task del goal va bene.
    b.tasks.iter_mut().find(|t| t.id == "a").unwrap().repo = Some("api".into());
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Accept);
}

#[test]
fn a_bot_off_by_default_holds_the_proposal_even_when_enabled() {
    let mut b = reviewed_goal(3);
    let bots = bots_with_git_operator();
    let ctx = SpecContext { workspace_agent: None, bots: &bots };
    let git = TaskSpec { bot: Some("git-operator".into()), ..spec("fix", &["a"]) };
    correction_with(&mut b, "p-fix", vec![git], ctx);
    assert_eq!(hold(&b, "p-fix"), Some(AutoFixHold::Bot { task: "fix".into(), bot: "git-operator".into() }));
    let general = TaskSpec { bot: Some("general".into()), ..spec("fix-2", &["a"]) };
    correction_with(&mut b, "p-ok", vec![general], ctx);
    assert_eq!(decision(&b, "p-ok"), AutoFixDecision::Accept);
}

#[test]
fn a_held_proposal_waits_for_the_user_with_its_reason_on_the_card() {
    let mut b = reviewed_goal(3);
    let mut api = spec("fix", &[]);
    api.repo = Some("api".into());
    correction(&mut b, "p-fix", vec![api]);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
    assert!(b.advance_goals(30).auto_fix.is_empty());
    let expected = AutoFixHold::Repo { task: "fix".into(), repo: "api".into() };
    assert_eq!(b.proposal("p-fix").unwrap().auto_fix_hold, Some(expected));
    // Senza correzione automatica il motivo non serve piu'.
    b.set_goal_auto_fix("g", 0, 31).unwrap();
    b.advance_goals(32);
    assert_eq!(b.proposal("p-fix").unwrap().auto_fix_hold, None);
}

#[test]
fn a_failed_automatic_acceptance_keeps_the_proposal_with_the_error_and_is_not_retried() {
    let mut b = reviewed_goal(3);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    assert!(b.hold_auto_fix("p-fix", "task 'fix': invalid model", 30));
    let invalid = AutoFixHold::Invalid { error: "task 'fix': invalid model".into() };
    assert_eq!(hold(&b, "p-fix"), Some(invalid.clone()));
    assert!(b.advance_goals(31).auto_fix.is_empty(), "no retry");
    assert_eq!(b.proposal("p-fix").unwrap().auto_fix_hold, Some(invalid));
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
    // Gia' bloccata o non piu' pendente: niente da scrivere.
    assert!(!b.hold_auto_fix("p-fix", "again", 32));
    assert!(!b.hold_auto_fix("nope", "x", 32));
    // L'utente la accetta comunque.
    b.accept_proposal("p-fix", CTX, 33).unwrap();
    assert_eq!(b.goal("g").unwrap().round(), 2);
}

#[test]
fn a_hold_is_not_written_on_a_proposal_the_app_would_not_accept() {
    let mut b = reviewed_goal(0);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    assert!(!b.hold_auto_fix("p-fix", "x", 30));
    assert_eq!(b.proposal("p-fix").unwrap().auto_fix_hold, None);
}

#[test]
fn a_closed_goal_has_no_automatic_correction() {
    let mut b = reviewed_goal(3);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    b.goal_mut("g").unwrap().closed_at = Some(40);
    assert_eq!(decision(&b, "p-fix"), AutoFixDecision::Manual);
}

#[test]
fn the_user_sets_the_rounds_on_an_open_goal_only() {
    let mut b = board_with_goal();
    assert_eq!(b.set_goal_auto_fix("g", 4, 5).unwrap_err(), "auto_fix max_rounds must be between 0 and 3");
    assert_eq!(b.set_goal_auto_fix("nope", 1, 5).unwrap_err(), "unknown goal: nope");
    b.set_goal_auto_fix("g", 3, 6).unwrap();
    assert_eq!((b.goal("g").unwrap().auto_fix().max_rounds, b.updated_at), (3, 6));
    b.close_goal("g", 7).unwrap();
    assert!(b.set_goal_auto_fix("g", 1, 8).unwrap_err().contains("is closed"));
}

#[test]
fn a_new_goal_takes_the_auto_fix_choice_of_the_user_at_acceptance() {
    let mut b = Board::new("ws-1");
    b.add_proposal("p-1".into(), "intake", None, new_goal("g", vec![spec("a", &[])]), CTX, 1).unwrap();
    let too_many = Acceptance { auto_fix: Some(4), ..Acceptance::default() };
    assert_eq!(
        b.accept_proposal_with("p-1", too_many, CTX, 2).unwrap_err(),
        "auto_fix max_rounds must be between 0 and 3"
    );
    let two = Acceptance { auto_fix: Some(2), ..Acceptance::default() };
    b.accept_proposal_with("p-1", two, CTX, 3).unwrap();
    let g = b.goal("g").unwrap();
    assert_eq!((g.auto_fix().max_rounds, g.auto_rounds()), (2, &[][..]), "a user acceptance is not automatic");
    // Su una proposta che non crea un goal la scelta e' un errore (0 = nessuna scelta).
    b.add_proposal("p-2".into(), "intake", None, in_goal("g", vec![spec("b", &[])]), CTX, 4).unwrap();
    let err = b.accept_proposal_with("p-2", two, CTX, 5).unwrap_err();
    assert_eq!(err, "auto_fix applies only to a proposal that creates a new goal");
    let none = Acceptance { auto_fix: Some(0), ..Acceptance::default() };
    b.accept_proposal_with("p-2", none, CTX, 6).unwrap();
    assert_eq!(b.goal("g").unwrap().auto_fix().max_rounds, 2, "unchanged");
}

#[test]
fn the_rounds_read_from_disk_are_kept_within_the_limits() {
    let mut b = board_with_goal();
    let g = b.goal_mut("g").unwrap();
    g.auto_fix.max_rounds = 9;
    g.auto_rounds = vec![5, 3, 1, 3, 2, 0];
    g.sanitize();
    assert_eq!((g.auto_fix.max_rounds, g.auto_rounds.as_slice()), (MAX_AUTO_FIX_ROUNDS, &[2, 3][..]));
}

#[test]
fn the_goal_on_disk_carries_auto_fix_and_its_automatic_rounds() {
    let mut b = reviewed_goal(1);
    correction(&mut b, "p-fix", vec![spec("fix", &["a"])]);
    b.accept_proposal_with("p-fix", Acceptance::AUTOMATIC, CTX, 31).unwrap();
    let json = serde_json::to_value(b.goal("g").unwrap()).unwrap();
    assert_eq!(
        (json["auto_fix"].clone(), json["auto_rounds"].clone()),
        (serde_json::json!({ "max_rounds": 1 }), serde_json::json!([2]))
    );
    let old: Goal = serde_json::from_value(serde_json::json!({ "id": "x", "title": "X", "round": 1 })).unwrap();
    assert_eq!((old.auto_fix(), old.auto_rounds()), (AutoFix::default(), &[][..]), "a goal written before T7");
}
