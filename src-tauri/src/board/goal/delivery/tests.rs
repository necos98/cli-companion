//! Consegna del goal: impostazioni, task di consegna, fase `delivering`, attesa
//! `deliver`, avanzamento (`advance_goals`) e rifiuto dei task nuovi.

use super::super::tests::{accept, board_with_goal, finish, in_goal, new_goal, review, set_status, spec, view, CTX};
use super::*;
use crate::board::{
    parse_spec, DeliveredGoal, GoalAdvance, GoalBranch, GoalOutcome, GoalPhase, GoalReview, GoalWaiting, TaskReport,
    TaskStatus, Verdict,
};
use crate::bots::{default_agent_bots, AgentBot};
use serde_json::json;

fn bots() -> Vec<AgentBot> {
    default_agent_bots()
}

fn with_bots<R>(f: impl FnOnce(SpecContext<'_>) -> R) -> R {
    let list = bots();
    f(SpecContext { workspace_agent: None, bots: &list })
}

/// Branch `goal/g` nel repository `key`, nato da `main`.
fn branch(b: &mut Board, key: &str) {
    let branch = GoalBranch { branch: "goal/g".into(), base: "main".into() };
    b.record_goal_branch("g", key, branch, 11).unwrap();
}

/// Board con il goal `g` pronto (task `a` e review `rev` `done`, review `passed`)
/// e il branch nella root.
fn ready_board() -> Board {
    let mut b = board_with_goal();
    finish(&mut b, "a", 20, None);
    finish(&mut b, "rev", 21, Some(Verdict::Passed));
    branch(&mut b, ".");
    b
}

fn deliver(b: &mut Board) -> String {
    with_bots(|ctx| b.create_deliver_task("g", ctx, 30)).unwrap()
}

/// Conclude il task di consegna `id` con un report.
fn finish_delivery(b: &mut Board, id: &str, at: i64, report: TaskReport) {
    let finished = crate::board::Finished { started_at: None, finished_at: at, report: Some(report) };
    b.task_mut(id).unwrap().set_state(crate::board::TaskState::Done(finished));
}

fn delivery_report() -> TaskReport {
    TaskReport {
        summary: "Delivered both repositories".into(),
        commit: Some("abc".into()),
        pr_url: Some("https://x/pull/1".into()),
        commits: vec!["abc".into(), "def".into()],
        pr_urls: vec!["https://x/pull/2".into(), "https://x/pull/1".into()],
        ..super::super::tests::report(None)
    }
}

// --- fase e attesa ---

#[test]
fn a_ready_goal_with_branches_waits_for_the_user_to_deliver() {
    let mut b = ready_board();
    let v = view(&b, "g");
    assert_eq!(
        (v.phase, v.waiting.as_slice(), v.last_review),
        (GoalPhase::Ready, &[GoalWaiting::Deliver][..], GoalReview::Passed)
    );
    // Senza branch non c'e' nulla da consegnare: il goal si chiude da solo.
    b.goal_mut("g").unwrap().branches.clear();
    assert!(view(&b, "g").waiting.is_empty());
}

#[test]
fn the_delivery_starts_by_itself_unless_the_last_review_found_issues() {
    let auto = |b: &mut Board| with_bots(|ctx| b.set_goal_delivery("g", Some(true), None, ctx, 40)).unwrap();
    let mut b = ready_board();
    auto(&mut b);
    assert!(view(&b, "g").waiting.is_empty(), "passed review: it proceeds by itself");
    // Nessuna review: parte da solo.
    let mut plain = Board::new("ws-1");
    accept(&mut plain, "p-1", new_goal("g", vec![spec("a", &[])]), 10).unwrap();
    finish(&mut plain, "a", 20, None);
    branch(&mut plain, ".");
    assert_eq!(view(&plain, "g").waiting, [GoalWaiting::Deliver]);
    auto(&mut plain);
    let v = view(&plain, "g");
    assert_eq!((v.last_review, v.waiting.is_empty()), (GoalReview::NotReviewed, true));
    // Review con problemi e correzioni scartate: mai da solo.
    let mut issues = Board::new("ws-1");
    accept(&mut issues, "p-1", new_goal("g", vec![spec("a", &[]), review("rev", &["a"])]), 10).unwrap();
    finish(&mut issues, "a", 20, None);
    finish(&mut issues, "rev", 21, Some(Verdict::Issues));
    branch(&mut issues, ".");
    auto(&mut issues);
    let v = view(&issues, "g");
    assert_eq!((v.last_review, v.waiting.as_slice()), (GoalReview::Issues, &[GoalWaiting::Deliver][..]));
}

#[test]
fn a_goal_deciding_on_a_proposal_does_not_wait_to_deliver() {
    let mut b = ready_board();
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Deliver]);
    // Con una proposta pendente il goal decide: nessuna consegna da attendere.
    b.add_proposal("p-2".into(), "rev-tab", Some("rev".into()), vec![spec("fix", &["a"])].into(), CTX, 22).unwrap();
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
}

#[test]
fn a_goal_with_an_open_delivery_is_delivering_and_a_failed_one_waits_as_failed() {
    let mut b = ready_board();
    let id = deliver(&mut b);
    for status in [TaskStatus::Queued, TaskStatus::Running] {
        set_status(&mut b, &id, status);
        let v = view(&b, "g");
        assert_eq!((v.phase, v.waiting.is_empty()), (GoalPhase::Delivering, true), "{status:?}");
    }
    set_status(&mut b, &id, TaskStatus::Failed);
    let v = view(&b, "g");
    assert_eq!((v.phase, v.waiting.as_slice()), (GoalPhase::Delivering, &[GoalWaiting::Failed][..]));
    assert_eq!(b.goal_detail_json("g").unwrap()["phase"], "delivering");
}

// --- task di consegna ---

#[test]
fn the_delivery_task_is_built_from_the_goal_and_enters_at_the_top_of_the_queue() {
    let mut b = ready_board();
    b.create_task(spec("later", &[]), CTX, 25).unwrap();
    let id = deliver(&mut b);
    assert_eq!(id, "deliver-g");
    let order: Vec<&str> = b.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(order, ["a", "rev", "deliver-g", "later"], "before every queued task, after the finished ones");
    let t = b.task("deliver-g").unwrap();
    assert_eq!((t.kind, t.goal(), t.round(), t.status()), (TaskKind::Deliver, Some("g"), 1, TaskStatus::Queued));
    assert_eq!(t.title, "Consegna: Goal g");
    assert_eq!(
        t.task,
        "Deliver the goal \"Goal g\" to its base as a whole, following the instructions and the delivery steps."
    );
    assert_eq!(
        (t.bot.as_deref(), t.repo.as_deref(), t.e2e, t.depends_on.is_empty()),
        (Some("general"), None, false, true)
    );
    assert_eq!(b.updated_at, 30);
    assert_eq!(view(&b, "g").phase, GoalPhase::Delivering);
}

#[test]
fn the_title_of_the_delivery_task_is_cut_to_the_limit() {
    let mut b = ready_board();
    b.goal_mut("g").unwrap().title = "t".repeat(crate::board::GOAL_TITLE_MAX);
    let id = deliver(&mut b);
    assert_eq!(b.task(&id).unwrap().title.chars().count(), crate::board::TITLE_MAX);
}

#[test]
fn the_delivery_id_gets_a_free_suffix_when_taken() {
    let mut b = ready_board();
    b.create_task(spec("deliver-g", &[]), CTX, 25).unwrap();
    assert_eq!(deliver(&mut b), "deliver-g-2");
}

#[test]
fn the_delivery_is_refused_unless_the_goal_is_ready_with_branches_and_without_a_delivery() {
    let err = |b: &mut Board, goal: &str| with_bots(|ctx| b.create_deliver_task(goal, ctx, 30)).unwrap_err();
    let mut b = board_with_goal();
    branch(&mut b, ".");
    assert_eq!(err(&mut b, "nope"), "unknown goal: nope");
    assert!(err(&mut b, "g").starts_with("goal g is not ready to deliver: it is executing"));
    let mut b = ready_board();
    b.goal_mut("g").unwrap().branches.clear();
    assert!(err(&mut b, "g").contains("has no branches"));
    let mut b = ready_board();
    deliver(&mut b);
    assert_eq!(err(&mut b, "g"), "goal g already has a delivery task: retry it");
    set_status(&mut b, "deliver-g", TaskStatus::Failed);
    assert_eq!(err(&mut b, "g"), "goal g already has a delivery task: retry it");
    b.close_goal("g", 50).unwrap();
    assert_eq!(err(&mut b, "g"), "goal g is closed: propose a new goal");
}

#[test]
fn the_delivery_bot_is_the_goals_and_must_be_enabled_and_run_work() {
    let mut b = ready_board();
    let mut list = bots();
    list.iter_mut().find(|x| x.id == "general").unwrap().enabled = false;
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let off = b.create_deliver_task("g", ctx, 30).unwrap_err();
    assert!(off.contains("General is disabled"), "{off}");
    assert!(b.task("deliver-g").is_none(), "nothing changed");
    assert_eq!(b.goal("g").unwrap().deliver_bot(), DEFAULT_DELIVER_BOT);
    // Il Bot di un goal che non esegue `work` si rifiuta quando lo si sceglie...
    let refused = with_bots(|ctx| b.set_goal_delivery("g", None, Some("reviewer".into()), ctx, 31)).unwrap_err();
    assert!(refused.contains("cannot run work tasks"), "{refused}");
    // ...e uno sconosciuto o spento anche.
    let unknown = with_bots(|ctx| b.set_goal_delivery("g", None, Some("nope".into()), ctx, 31)).unwrap_err();
    assert!(unknown.contains("unknown bot: nope"), "{unknown}");
    let disabled = b.set_goal_delivery("g", None, Some("general".into()), ctx, 31).unwrap_err();
    assert!(disabled.contains("disabled"), "{disabled}");
    assert_eq!(b.goal("g").unwrap().deliver_bot, None);
}

#[test]
fn the_delivery_runs_in_the_repository_of_the_e2e_task_else_the_root_else_the_first_branch() {
    let work_in = |repo: &str, e2e: bool| TaskSpec { repo: Some(repo.into()), e2e, ..spec("api-work", &[]) };
    let make = |tasks: Vec<TaskSpec>, keys: &[&str]| {
        let mut b = Board::new("ws-1");
        accept(&mut b, "p-1", new_goal("g", tasks), 10).unwrap();
        for k in keys {
            branch(&mut b, k);
        }
        for t in b.tasks.iter_mut() {
            t.set_status(TaskStatus::Done);
        }
        let id = deliver(&mut b);
        let t = b.task(&id).unwrap();
        (t.repo.clone(), t.e2e)
    };
    // Un task `work` con E2E in `api`: la consegna e' la', con E2E.
    let plain = spec("root-work", &[]);
    assert_eq!(make(vec![plain.clone(), work_in("api", true)], &[".", "api"]), (Some("api".into()), true));
    // Senza E2E: la root, se e' fra i branch.
    assert_eq!(make(vec![plain.clone(), work_in("api", false)], &[".", "api"]), (None, false));
    // Senza la root fra i branch: il primo.
    assert_eq!(make(vec![work_in("api", false)], &["api", "web"]), (Some("api".into()), false));
    // Il repository dell'E2E senza branch non conta, e gli E2E non si richiedono altrove.
    assert_eq!(make(vec![plain.clone(), work_in("api", true)], &["."]), (None, false));
    // Con piu' task E2E vale il primo della coda che ha il branch.
    let root_e2e = TaskSpec { e2e: true, ..plain };
    assert_eq!(make(vec![root_e2e, work_in("api", true)], &["api", "."]), (None, true));
}

// --- impostazioni ---

#[test]
fn the_user_sets_auto_deliver_and_the_bot_on_an_open_goal() {
    let mut b = board_with_goal();
    let set = |b: &mut Board, auto: Option<bool>, bot: Option<&str>| {
        with_bots(|ctx| b.set_goal_delivery("g", auto, bot.map(str::to_string), ctx, 40))
    };
    assert_eq!(set(&mut b, None, None).unwrap_err(), "nothing to change: give auto_deliver or bot");
    set(&mut b, Some(true), None).unwrap();
    let g = b.goal("g").unwrap();
    assert_eq!((g.auto_deliver(), g.deliver_bot()), (true, "general"));
    set(&mut b, None, Some(" general ")).unwrap();
    assert_eq!(b.goal("g").unwrap().deliver_bot, Some("general".into()));
    set(&mut b, Some(false), None).unwrap();
    let g = b.goal("g").unwrap();
    assert_eq!((g.auto_deliver(), g.deliver_bot.as_deref(), b.updated_at), (false, Some("general"), 40));
    let err = with_bots(|ctx| b.set_goal_delivery("nope", Some(true), None, ctx, 41)).unwrap_err();
    assert_eq!(err, "unknown goal: nope");
    b.close_goal("g", 42).unwrap();
    assert_eq!(set(&mut b, Some(true), None).unwrap_err(), "goal g is closed: propose a new goal");
}

#[test]
fn a_new_goal_takes_the_auto_deliver_choice_of_the_user_at_acceptance() {
    let propose = |b: &mut Board, id: &str, input| b.add_proposal(id.into(), "intake", None, input, CTX, 5).unwrap();
    let mut b = Board::new("ws-1");
    propose(&mut b, "p-1", new_goal("g", vec![spec("a", &[])]));
    b.accept_proposal_with("p-1", Some(true), CTX, 6).unwrap();
    assert!(b.goal("g").unwrap().auto_deliver());
    propose(&mut b, "p-2", new_goal("h", vec![spec("b", &[])]));
    b.accept_proposal_with("p-2", Some(false), CTX, 7).unwrap();
    assert!(!b.goal("h").unwrap().auto_deliver());
    propose(&mut b, "p-3", new_goal("k", vec![spec("c", &[])]));
    b.accept_proposal("p-3", CTX, 8).unwrap();
    assert!(!b.goal("k").unwrap().auto_deliver(), "the default is off");
    // Senza un goal nuovo, `true` e' un errore e la proposta resta.
    for input in [vec![spec("x", &[])].into(), in_goal("g", vec![spec("y", &[])])] {
        propose(&mut b, "p-4", input);
        let err = b.accept_proposal_with("p-4", Some(true), CTX, 9).unwrap_err();
        assert_eq!(err, "auto_deliver applies only to a proposal that creates a new goal");
        assert!(b.proposal("p-4").is_ok());
        b.accept_proposal_with("p-4", Some(false), CTX, 9).unwrap();
    }
}

#[test]
fn delivery_settings_and_the_report_survive_a_save_and_a_load() {
    let mut b = ready_board();
    with_bots(|ctx| b.set_goal_delivery("g", Some(true), Some("general".into()), ctx, 40)).unwrap();
    let id = deliver(&mut b);
    finish_delivery(&mut b, &id, 50, delivery_report());
    b.advance_goals(51);
    let back = Board::from_json(&serde_json::to_string(&b).unwrap(), 60).unwrap();
    assert_eq!(back.goals, b.goals);
    let g = back.goal("g").unwrap();
    assert_eq!((g.auto_deliver(), g.deliver_bot.as_deref()), (true, Some("general")));
    assert!(g.report().is_some());
    // I file senza i campi nuovi si leggono come prima.
    let old = json!({ "schema_version": 3, "data": { "workspace_id": "w", "goals": [
        { "id": "o", "title": "O", "request": "r", "done_when": ["d"] } ] } });
    let g = Board::from_json(&old.to_string(), 1).unwrap().goal("o").cloned().unwrap();
    assert_eq!((g.auto_deliver(), g.deliver_bot(), g.report()), (false, DEFAULT_DELIVER_BOT, None));
}

#[test]
fn reading_keeps_the_report_only_of_a_delivered_goal_and_a_sane_bot() {
    let report = json!({ "summary": "s", "commits": ["a"], "pr_urls": [] });
    let goal = |extra: serde_json::Value| {
        let mut g = json!({ "id": "g", "title": "G", "request": "r", "done_when": ["d"], "report": report });
        g.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let raw = json!({ "schema_version": 3, "data": { "workspace_id": "w", "goals": [g] } });
        Board::from_json(&raw.to_string(), 1).unwrap().goal("g").cloned().unwrap()
    };
    assert_eq!(goal(json!({})).report(), None, "an open goal has none");
    assert_eq!(goal(json!({ "closed_at": 5, "outcome": "abandoned" })).report(), None);
    let delivered = goal(json!({ "closed_at": 5, "outcome": "delivered", "deliver_bot": " " }));
    assert_eq!(delivered.report().map(|r| r.summary.as_str()), Some("s"));
    assert_eq!(delivered.deliver_bot, None, "a blank bot falls back to the default");
    let long = "b".repeat(crate::board::BOT_ID_MAX + 1);
    assert_eq!(goal(json!({ "deliver_bot": long })).deliver_bot, None);
    let many: Vec<String> = (0..30).map(|i| i.to_string()).collect();
    let big = goal(json!({ "closed_at": 5, "outcome": "delivered", "report": { "summary": "s", "commits": many } }));
    assert_eq!(big.report().unwrap().commits.len(), crate::board::DELIVERED_MAX);
}

// --- avanzamento ---

#[test]
fn a_goal_with_branches_stays_ready_and_without_branches_closes_delivered() {
    let mut b = ready_board();
    assert_eq!(b.advance_goals(30), GoalAdvance::default());
    assert_eq!(view(&b, "g").phase, GoalPhase::Ready, "waiting for the delivery");
    b.goal_mut("g").unwrap().branches.clear();
    assert_eq!(b.advance_goals(31), GoalAdvance::default(), "no notice for a goal with nothing to deliver");
    let g = b.goal("g").unwrap();
    assert_eq!(
        (g.closed_at(), g.outcome(), g.review(), g.report()),
        (Some(31), Some(GoalOutcome::Delivered), Some(GoalReview::Passed), None)
    );
}

#[test]
fn a_goal_left_without_tasks_closes_abandoned_even_with_branches() {
    let mut b = ready_board();
    for id in ["a", "rev"] {
        b.task_mut(id).unwrap().set_status(TaskStatus::Queued);
    }
    b.delete_with_dependents("a", 25).unwrap();
    assert!(b.goal_tasks("g").next().is_none());
    assert_eq!(b.advance_goals(26), GoalAdvance::default());
    assert_eq!(b.goal("g").unwrap().outcome(), Some(GoalOutcome::Abandoned));
}

#[test]
fn a_ready_goal_that_delivers_by_itself_is_reported_and_left_untouched() {
    let mut b = ready_board();
    with_bots(|ctx| b.set_goal_delivery("g", Some(true), None, ctx, 40)).unwrap();
    let before = b.clone();
    let advance = b.advance_goals(41);
    assert_eq!(advance, GoalAdvance { delivered: Vec::new(), auto_deliver: vec!["g".into()] });
    assert_eq!(b, before, "the board does not create the task here: it needs the bots");
    // Ripetuto, lo riporta ancora: evitare il doppio avvio e' di chi chiama.
    assert_eq!(b.advance_goals(42).auto_deliver, ["g"]);
    // Con la consegna creata non e' piu' da avviare.
    deliver(&mut b);
    assert_eq!(b.advance_goals(43), GoalAdvance::default());
}

#[test]
fn a_done_delivery_closes_the_goal_with_the_merged_report_and_announces_it() {
    let mut b = ready_board();
    let id = deliver(&mut b);
    assert_eq!(b.advance_goals(40), GoalAdvance::default(), "still delivering");
    finish_delivery(&mut b, &id, 45, delivery_report());
    let advance = b.advance_goals(46);
    let delivered = DeliveredGoal { goal_id: "g".into(), title: "Goal g".into(), deliver_task_id: id.clone() };
    assert_eq!(advance, GoalAdvance { delivered: vec![delivered], auto_deliver: Vec::new() });
    let g = b.goal("g").unwrap();
    assert_eq!(
        (g.closed_at(), g.outcome(), g.review(), b.updated_at),
        (Some(46), Some(GoalOutcome::Delivered), Some(GoalReview::Passed), 46)
    );
    let report = g.report().unwrap();
    assert_eq!(report.summary, "Delivered both repositories");
    assert_eq!(report.commits, ["abc", "def"], "commit then commits, no duplicates");
    assert_eq!(report.pr_urls, ["https://x/pull/1", "https://x/pull/2"]);
    assert_eq!(view(&b, "g").phase, GoalPhase::Closed);
    assert_eq!(b.advance_goals(47), GoalAdvance::default(), "announced once");
}

#[test]
fn a_delivery_closes_the_goal_even_if_the_review_found_issues() {
    let mut b = Board::new("ws-1");
    accept(&mut b, "p-1", new_goal("g", vec![spec("a", &[]), review("rev", &["a"])]), 10).unwrap();
    finish(&mut b, "a", 20, None);
    finish(&mut b, "rev", 21, Some(Verdict::Issues));
    branch(&mut b, ".");
    let id = deliver(&mut b);
    finish_delivery(&mut b, &id, 30, TaskReport { commit: None, pr_url: None, ..delivery_report() });
    b.advance_goals(31);
    let g = b.goal("g").unwrap();
    assert_eq!((g.outcome(), g.review()), (Some(GoalOutcome::Delivered), Some(GoalReview::Issues)));
}

// --- niente task nuovi durante la consegna ---

#[test]
fn a_goal_being_delivered_accepts_no_new_task() {
    let mut b = ready_board();
    let refused = "goal g is being delivered: new tasks cannot join it";
    let id = deliver(&mut b);
    // Proposta dell'Intake con goal_id, task manuale, proposta del revisore.
    assert_eq!(
        b.add_proposal("p".into(), "i", None, in_goal("g", vec![spec("x", &[])]), CTX, 31).unwrap_err(),
        refused
    );
    assert_eq!(b.create_task_in(spec("m", &[]), Some("g"), CTX, 31).unwrap_err(), refused);
    assert_eq!(
        b.add_proposal("p".into(), "t", Some("rev".into()), vec![spec("fix", &["a"])].into(), CTX, 31).unwrap_err(),
        refused
    );
    set_status(&mut b, &id, TaskStatus::Failed);
    assert_eq!(b.create_task_in(spec("m2", &[]), Some("g"), CTX, 31).unwrap_err(), refused, "also while it failed");
    // Un task sciolto resta possibile; le note dell'utente anche.
    b.create_task(spec("loose", &[]), CTX, 32).unwrap();
    b.add_goal_note("g", "keep going", 33).unwrap();
    // Con la consegna `done` il goal si chiude e non riceve piu' nulla.
    finish_delivery(&mut b, &id, 40, delivery_report());
    b.advance_goals(41);
    assert!(b.create_task_in(spec("late", &[]), Some("g"), CTX, 42).unwrap_err().contains("is closed"));
}

#[test]
fn a_goal_deciding_cannot_be_delivered_until_the_proposal_is_settled() {
    let mut b = ready_board();
    b.add_proposal("p-2".into(), "intake", None, in_goal("g", vec![spec("late", &[])]), CTX, 22).unwrap();
    // Con la proposta pendente il goal e' `deciding`: la consegna non parte.
    let err = with_bots(|ctx| b.create_deliver_task("g", ctx, 30)).unwrap_err();
    assert!(err.contains("is not ready to deliver: it is deciding"), "{err}");
    b.discard_proposal("p-2", 23).unwrap();
    deliver(&mut b);
}

// --- tipo riservato ---

#[test]
fn the_deliver_kind_is_rejected_from_every_external_input() {
    let reserved = crate::board::DELIVER_KIND_RESERVED;
    let patch = json!({ "id": "a", "title": "T", "task": "x", "kind": "deliver" });
    assert!(parse_spec(&patch, None).unwrap_err().contains(reserved));
    let proposal = json!({ "tasks": [{ "id": "a", "title": "T", "task": "x", "bot": "general", "kind": "deliver" }] });
    assert!(crate::board::parse_proposal(&proposal).unwrap_err().contains(reserved));
    let mut b = ready_board();
    b.create_task(spec("t", &[]), CTX, 25).unwrap();
    let err = b.update_task("t", &json!({ "kind": "deliver" }), CTX, 26).unwrap_err();
    assert!(err.contains(reserved), "{err}");
    let id = deliver(&mut b);
    set_status(&mut b, &id, TaskStatus::Failed);
    let err = b.update_task(&id, &json!({ "kind": "work" }), CTX, 27).unwrap_err();
    assert!(err.contains("delivery task cannot be changed"), "{err}");
    // ...ma si puo' modificare il resto di un task di consegna fallito.
    b.update_task(&id, &json!({ "title": "Retitled" }), CTX, 28).unwrap();
    assert_eq!(b.task(&id).unwrap().title, "Retitled");
}
