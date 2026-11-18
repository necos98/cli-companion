//! Goal: input di `propose_tasks`, appartenenza dei task, nascita e giri,
//! eredita' dalla review, fase e attesa, chiusura, note e archivio.

use super::*;
use crate::board::{
    parse_new_task, parse_proposal, parse_spec, Board, Finished, ProposalInput, Run, RunPhase, SpecContext, Task,
    TaskKind, TaskReport, TaskSpec, TaskState, TaskStatus, Verdict, WorkerActivity, MAX_FINISHED_KEPT,
};
use serde_json::{json, Value};

const CTX: SpecContext<'static> = SpecContext { workspace_agent: None, bots: &[] };

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..TaskSpec::default()
    }
}

fn review(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec { kind: TaskKind::Review, ..spec(id, deps) }
}

fn goal_spec(id: &str) -> GoalSpec {
    GoalSpec {
        id: id.into(),
        title: format!("Goal {id}"),
        request: "the user's words".into(),
        done_when: vec!["it works".into()],
        decisions: vec!["use CSV".into()],
    }
}

fn new_goal(id: &str, tasks: Vec<TaskSpec>) -> ProposalInput {
    ProposalInput { tasks, goal: Some(ProposalGoal::New(goal_spec(id))) }
}

fn in_goal(id: &str, tasks: Vec<TaskSpec>) -> ProposalInput {
    ProposalInput { tasks, goal: Some(ProposalGoal::Existing(id.into())) }
}

/// Propone e accetta subito, dall'Intake.
fn accept(b: &mut Board, pid: &str, input: ProposalInput, now: i64) -> Result<Vec<String>, String> {
    b.add_proposal(pid.into(), "intake", None, input, CTX, now)?;
    b.accept_proposal(pid, CTX, now)
}

/// Board con il goal `g` (task `a`, poi la review `rev` di `a`) accettato al tempo 10.
fn board_with_goal() -> Board {
    let mut b = Board::new("ws-1");
    accept(&mut b, "p-1", new_goal("g", vec![spec("a", &[]), review("rev", &["a"])]), 10).unwrap();
    b
}

fn report(verdict: Option<Verdict>) -> TaskReport {
    TaskReport {
        status: "completed".into(),
        summary: "ok".into(),
        commit: None,
        pr_url: None,
        files_changed: Vec::new(),
        tests: Vec::new(),
        follow_ups: Vec::new(),
        diffstat: None,
        verdict,
        findings: Vec::new(),
        e2e: None,
    }
}

fn finish(b: &mut Board, id: &str, at: i64, verdict: Option<Verdict>) {
    let report = Some(report(verdict));
    b.task_mut(id).unwrap().set_state(TaskState::Done(Finished { started_at: None, finished_at: at, report }));
}

fn set_status(b: &mut Board, id: &str, status: TaskStatus) {
    b.task_mut(id).unwrap().set_status(status);
}

fn live(b: &mut Board, id: &str, worker: WorkerActivity) {
    let phase = RunPhase::Live { session: "tab".into(), worker };
    b.task_mut(id).unwrap().set_state(TaskState::Running(Run { started_at: 1, previous_attempt: None, phase }));
}

fn view(b: &Board, id: &str) -> GoalView {
    b.goal_view(id).unwrap()
}

// --- input di propose_tasks ---

fn proposal_json(extra: Value) -> Value {
    let mut p = json!({ "tasks": [{ "id": "a", "title": "T", "task": "x", "bot": "general" }] });
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    p
}

fn goal_json() -> Value {
    json!({ "id": "export", "title": "Export", "request": "export orders", "done_when": ["a file"] })
}

#[test]
fn a_proposal_names_a_new_goal_an_open_goal_or_none() {
    let new = parse_proposal(&proposal_json(json!({ "goal": goal_json() }))).unwrap();
    let expected = GoalSpec {
        id: "export".into(),
        title: "Export".into(),
        request: "export orders".into(),
        done_when: vec!["a file".into()],
        decisions: Vec::new(),
    };
    assert_eq!(new.goal, Some(ProposalGoal::New(expected)));
    let existing = parse_proposal(&proposal_json(json!({ "goal_id": " export " }))).unwrap();
    assert_eq!(existing.goal, Some(ProposalGoal::Existing("export".into())));
    assert_eq!(parse_proposal(&proposal_json(json!({}))).unwrap().goal, None);
    assert_eq!(parse_proposal(&proposal_json(json!({ "goal": null, "goal_id": null }))).unwrap().goal, None);
    let both = parse_proposal(&proposal_json(json!({ "goal": goal_json(), "goal_id": "export" }))).unwrap_err();
    assert!(both.contains("not both"), "{both}");
    assert!(parse_proposal(&proposal_json(json!({ "goal_id": "Bad Id" }))).unwrap_err().contains("invalid goal_id"));
    assert!(parse_proposal(&proposal_json(json!({ "goal_id": 3 }))).unwrap_err().contains("must be a string"));
}

#[test]
fn a_new_goal_is_validated_within_its_limits() {
    let with = |k: &str, v: Value| {
        let mut g = goal_json();
        g[k] = v;
        parse_goal_spec(&g)
    };
    let err = |k: &str, v: Value| with(k, v).unwrap_err();
    assert!(err("id", json!("-x")).contains("invalid goal.id"));
    assert!(err("title", json!("")).contains("goal.title is missing"));
    assert!(err("title", json!("t".repeat(GOAL_TITLE_MAX + 1))).contains("too long"));
    assert!(with("title", json!("t".repeat(GOAL_TITLE_MAX))).is_ok());
    assert!(err("request", json!("r".repeat(GOAL_REQUEST_MAX + 1))).contains("too long"));
    assert!(err("done_when", json!([])).contains("empty"));
    assert!(err("done_when", json!(vec!["c"; DONE_WHEN_MAX + 1])).contains("too many"));
    assert!(err("done_when", json!(["d".repeat(DONE_WHEN_ITEM_MAX + 1)])).contains("goal.done_when[0] is too long"));
    assert!(err("done_when", json!(["ok", " "])).contains("goal.done_when[1] is missing"));
    assert!(err("decisions", json!(vec!["d"; DECISIONS_KEPT + 1])).contains("too many"));
    assert!(err("decisions", json!(["d".repeat(DECISION_MAX + 1)])).contains("too long"));
    assert!(err("round", json!(3)).contains("unknown field 'round'"));
    let mut missing = goal_json();
    missing.as_object_mut().unwrap().remove("done_when");
    assert!(parse_goal_spec(&missing).unwrap_err().contains("goal.done_when is missing"));
    assert!(parse_goal_spec(&json!("export")).unwrap_err().contains("must be an object"));
    let full = with("decisions", json!([" keep the old format "])).unwrap();
    assert_eq!(full.decisions, ["keep the old format"]);
}

#[test]
fn the_goal_is_not_a_task_field_and_never_changes() {
    let err = parse_spec(&json!({ "title": "T", "task": "x", "goal": "g" }), None).unwrap_err();
    assert!(err.contains("set goal or goal_id on the proposal"), "{err}");
    let base = spec("a", &[]);
    let err = parse_spec(&json!({ "goal": "g" }), Some(&base)).unwrap_err();
    assert_eq!(err, "the goal of a task cannot be changed");
    let (spec, goal) = parse_new_task(&json!({ "title": "T", "task": "x", "goal": "g" })).unwrap();
    assert_eq!((spec.title.as_str(), goal.as_deref()), ("T", Some("g")));
    assert_eq!(parse_new_task(&json!({ "title": "T", "task": "x", "goal": null })).unwrap().1, None);
    assert!(parse_new_task(&json!({ "title": "T", "task": "x", "goal": "No" })).unwrap_err().contains("invalid"));
}

// --- nascita, giri e appartenenza ---

#[test]
fn accepting_a_new_goal_creates_it_at_the_first_round() {
    let b = board_with_goal();
    let g = b.goal("g").unwrap();
    assert_eq!((g.title.as_str(), g.round(), g.created_at, g.is_open()), ("Goal g", 1, 10, true));
    assert_eq!(g.decisions, [GoalDecision { at: 10, source: DecisionSource::Intake, text: "use CSV".into() }]);
    for id in ["a", "rev"] {
        let t = b.task(id).unwrap();
        assert_eq!((t.goal(), t.round()), (Some("g"), 1), "{id}");
    }
}

#[test]
fn discarding_a_new_goal_creates_nothing() {
    let mut b = Board::new("ws-1");
    b.add_proposal("p-1".into(), "intake", None, new_goal("g", vec![spec("a", &[])]), CTX, 1).unwrap();
    assert_eq!(b.proposals[0].goal.as_ref().map(|g| g.id.as_str()), Some("g"));
    b.discard_proposal("p-1", 2).unwrap();
    assert!(b.goals.is_empty() && b.tasks.is_empty());
}

#[test]
fn tasks_join_an_open_goal_at_its_current_round() {
    let mut b = board_with_goal();
    b.goal_mut("g").unwrap().round = 2;
    accept(&mut b, "p-2", in_goal("g", vec![spec("b", &["a"])]), 20).unwrap();
    let t = b.task("b").unwrap();
    assert_eq!((t.goal(), t.round(), b.goal("g").unwrap().round()), (Some("g"), 2, 2));
    // Anche un task creato a mano.
    b.create_task_in(spec("c", &[]), Some("g"), CTX, 21).unwrap();
    assert_eq!((b.task("c").unwrap().goal(), b.task("c").unwrap().round()), (Some("g"), 2));
    b.create_task(spec("loose", &[]), CTX, 22).unwrap();
    assert_eq!((b.task("loose").unwrap().goal(), b.task("loose").unwrap().round()), (None, 0));
}

#[test]
fn unknown_closed_and_taken_goals_are_refused() {
    let mut b = board_with_goal();
    let err = |b: &mut Board, input: ProposalInput| b.add_proposal("p-x".into(), "intake", None, input, CTX, 30);
    assert_eq!(err(&mut b, in_goal("nope", vec![spec("x", &[])])).unwrap_err(), "unknown goal: nope");
    assert_eq!(err(&mut b, new_goal("g", vec![spec("x", &[])])).unwrap_err(), "goal id already in use: g");
    b.add_proposal("p-2".into(), "intake", None, new_goal("h", vec![spec("y", &[])]), CTX, 31).unwrap();
    assert_eq!(err(&mut b, new_goal("h", vec![spec("x", &[])])).unwrap_err(), "goal id already in use: h");
    // Il goal nuovo resta della sua proposta: accettarla non lo trova occupato da se stessa.
    b.accept_proposal("p-2", CTX, 32).unwrap();
    b.goal_mut("h").unwrap().closed_at = Some(33);
    let closed = "goal h is closed: propose a new goal";
    assert_eq!(err(&mut b, in_goal("h", vec![spec("x", &[])])).unwrap_err(), closed);
    assert_eq!(b.create_task_in(spec("x", &[]), Some("h"), CTX, 34).unwrap_err(), closed);
    assert_eq!(b.create_task_in(spec("x", &[]), Some("nope"), CTX, 34).unwrap_err(), "unknown goal: nope");
}

#[test]
fn acceptance_revalidates_the_goal() {
    let mut b = board_with_goal();
    b.add_proposal("p-2".into(), "intake", None, in_goal("g", vec![spec("b", &["a"])]), CTX, 20).unwrap();
    b.goal_mut("g").unwrap().closed_at = Some(21);
    assert!(b.accept_proposal("p-2", CTX, 22).unwrap_err().contains("is closed"));
    assert_eq!(b.proposals.len(), 1, "the proposal stays pending");
    // Un goal con lo stesso id nato nel frattempo per altra via.
    b.add_proposal("p-3".into(), "intake", None, new_goal("k", vec![spec("c", &[])]), CTX, 23).unwrap();
    b.goals.push(Goal { id: "k".into(), round: 1, ..Goal::default() });
    assert_eq!(b.accept_proposal("p-3", CTX, 24).unwrap_err(), "goal id already in use: k");
    assert!(b.task("c").is_none());
}

#[test]
fn open_goals_are_capped() {
    let mut b = Board::new("ws-1");
    for i in 0..MAX_OPEN_GOALS {
        accept(&mut b, &format!("p-{i}"), new_goal(&format!("g{i}"), vec![spec(&format!("t{i}"), &[])]), 1).unwrap();
    }
    let err = accept(&mut b, "p-x", new_goal("extra", vec![spec("x", &[])]), 2).unwrap_err();
    assert!(err.contains("too many open goals"), "{err}");
    // Un goal chiuso libera il posto; aggiungere a un goal aperto resta possibile.
    accept(&mut b, "p-y", in_goal("g0", vec![spec("y", &[])]), 2).unwrap();
    b.goal_mut("g1").unwrap().closed_at = Some(3);
    accept(&mut b, "p-x", new_goal("extra", vec![spec("x", &[])]), 4).unwrap();
}

#[test]
fn dependencies_stay_inside_one_goal() {
    let mut b = board_with_goal();
    accept(&mut b, "p-2", new_goal("other", vec![spec("o", &[])]), 11).unwrap();
    b.create_task(spec("loose", &[]), CTX, 12).unwrap();
    let propose = |b: &mut Board, input: ProposalInput| b.add_proposal("p-x".into(), "intake", None, input, CTX, 13);
    let cross = propose(&mut b, in_goal("g", vec![spec("x", &["o"])])).unwrap_err();
    assert_eq!(cross, "task 'x': depends on 'o', which belongs to another goal");
    let on_loose = propose(&mut b, new_goal("n", vec![spec("x", &["loose"])])).unwrap_err();
    assert_eq!(on_loose, "task 'x': depends on 'loose', which is not part of goal 'n'");
    let loose_on_open = propose(&mut b, vec![spec("x", &["a"])].into()).unwrap_err();
    assert_eq!(loose_on_open, "task 'x': depends on 'a', which belongs to the open goal 'g'");
    assert_eq!(b.create_task(spec("x", &["a"]), CTX, 14).unwrap_err(), loose_on_open);
    // Nella stessa proposta e nello stesso goal: lecito.
    propose(&mut b, in_goal("g", vec![spec("x", &["a"]), spec("y", &["x"])])).unwrap();
    // Un goal chiuso non trattiene piu' nessuno.
    b.goal_mut("other").unwrap().closed_at = Some(15);
    b.create_task(spec("after", &["o"]), CTX, 16).unwrap();
    // Una modifica rispetta le stesse regole.
    let err = b.update_task("a", &json!({ "depends_on": ["loose"] }), CTX, 17).unwrap_err();
    assert_eq!(err, "task 'a': depends on 'loose', which is not part of goal 'g'");
    let err = b.update_task("loose", &json!({ "depends_on": ["a"] }), CTX, 17).unwrap_err();
    assert!(err.contains("belongs to the open goal 'g'"), "{err}");
    b.update_task("a", &json!({ "title": "Renamed" }), CTX, 18).unwrap();
    assert_eq!(b.task("a").unwrap().goal(), Some("g"));
}

#[test]
fn a_plan_task_never_belongs_to_a_goal() {
    let mut b = board_with_goal();
    let plan = TaskSpec { kind: TaskKind::Plan, ..spec("plan", &[]) };
    let err = b.create_task_in(plan.clone(), Some("g"), CTX, 20).unwrap_err();
    assert_eq!(err, "task 'plan': a plan task cannot belong to a goal");
    b.create_task(plan, CTX, 21).unwrap();
}

// --- proposta del revisore ---

#[test]
fn the_proposal_of_a_reviewer_inherits_its_goal_and_advances_the_round() {
    let mut b = board_with_goal();
    finish(&mut b, "a", 20, None);
    live(&mut b, "rev", WorkerActivity::Running);
    let fixes = vec![spec("fix", &["a"]), review("rev-2", &["fix"])];
    b.add_proposal("p-2".into(), "rev-tab", Some("rev".into()), fixes.into(), CTX, 21).unwrap();
    let p = &b.proposals[0];
    assert_eq!((p.goal.as_ref(), p.goal_id.as_deref(), p.is_correction()), (None, Some("g"), true));
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal]);
    b.accept_proposal("p-2", CTX, 22).unwrap();
    assert_eq!(b.goal("g").unwrap().round(), 2);
    for id in ["fix", "rev-2"] {
        assert_eq!((b.task(id).unwrap().goal(), b.task(id).unwrap().round()), (Some("g"), 2), "{id}");
    }
}

#[test]
fn a_worker_never_names_the_goal_of_its_proposal() {
    let mut b = board_with_goal();
    let err = b
        .add_proposal("p-2".into(), "rev-tab", Some("rev".into()), new_goal("n", vec![spec("x", &[])]), CTX, 20)
        .unwrap_err();
    assert!(err.contains("cannot be set by an automated session"), "{err}");
    // La review di un task sciolto propone task sciolti, senza giro.
    b.create_task(spec("solo", &[]), CTX, 21).unwrap();
    b.create_task(review("solo-rev", &["solo"]), CTX, 22).unwrap();
    b.add_proposal("p-3".into(), "t", Some("solo-rev".into()), vec![spec("y", &[])].into(), CTX, 23).unwrap();
    assert!(!b.proposals[0].is_correction() && b.proposals[0].goal_id.is_none());
    b.accept_proposal("p-3", CTX, 24).unwrap();
    assert_eq!(b.task("y").unwrap().goal(), None);
}

#[test]
fn an_intake_proposal_for_an_open_goal_keeps_the_round() {
    let mut b = board_with_goal();
    accept(&mut b, "p-2", in_goal("g", vec![spec("b", &[])]), 20).unwrap();
    assert_eq!((b.goal("g").unwrap().round(), b.task("b").unwrap().round()), (1, 1));
}

// --- fase e attesa ---

#[test]
fn the_phase_follows_work_review_and_proposals() {
    let mut b = board_with_goal();
    assert_eq!(view(&b, "g").phase, GoalPhase::Executing);
    set_status(&mut b, "a", TaskStatus::Failed);
    assert_eq!(view(&b, "g").phase, GoalPhase::Executing);
    finish(&mut b, "a", 20, None);
    for status in [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Failed] {
        set_status(&mut b, "rev", status);
        assert_eq!(view(&b, "g").phase, GoalPhase::Verifying, "{status:?}");
    }
    finish(&mut b, "rev", 21, Some(Verdict::Issues));
    assert_eq!(view(&b, "g").phase, GoalPhase::Ready);
    b.add_proposal("p-2".into(), "rev-tab", Some("rev".into()), vec![spec("fix", &["a"])].into(), CTX, 22).unwrap();
    assert_eq!(view(&b, "g").phase, GoalPhase::Deciding);
    b.discard_proposal("p-2", 23).unwrap();
    b.close_finished_goals(24);
    let closed = view(&b, "g");
    assert_eq!((closed.phase, closed.waiting.as_slice()), (GoalPhase::Closed, &[][..]));
}

#[test]
fn waiting_lists_what_needs_the_user_in_order() {
    let mut b = board_with_goal();
    assert!(view(&b, "g").waiting.is_empty(), "the goal proceeds by itself");
    live(&mut b, "a", WorkerActivity::NeedsInput);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Attention]);
    live(&mut b, "a", WorkerActivity::Running);
    assert!(view(&b, "g").waiting.is_empty());
    set_status(&mut b, "rev", TaskStatus::Failed);
    b.add_proposal("p-2".into(), "intake", None, in_goal("g", vec![spec("b", &[])]), CTX, 20).unwrap();
    live(&mut b, "a", WorkerActivity::Idle);
    assert_eq!(view(&b, "g").waiting, [GoalWaiting::Proposal, GoalWaiting::Failed, GoalWaiting::Attention]);
}

#[test]
fn the_view_counts_done_tasks_and_lists_every_goal() {
    let mut b = board_with_goal();
    finish(&mut b, "a", 20, None);
    accept(&mut b, "p-2", new_goal("h", vec![spec("h1", &[])]), 21).unwrap();
    b.create_task(spec("loose", &[]), CTX, 22).unwrap();
    let views = b.goal_views();
    let summary: Vec<(&str, GoalProgress)> = views.iter().map(|v| (v.goal.id.as_str(), v.tasks)).collect();
    assert_eq!(summary, [("g", GoalProgress { done: 1, total: 2 }), ("h", GoalProgress { done: 0, total: 1 })]);
    assert_eq!(views[0], view(&b, "g"));
    let row = views[0].to_row_json();
    assert_eq!(
        row,
        json!({ "id": "g", "title": "Goal g", "phase": "verifying", "round": 1, "waiting": [],
                "tasks": { "done": 1, "total": 2 } })
    );
    let detail = b.goal_detail_json("g").unwrap();
    assert_eq!((detail["request"].clone(), detail["phase"].clone()), (json!("the user's words"), json!("verifying")));
    assert_eq!(detail["progress"], json!({ "done": 1, "total": 2 }));
    assert_eq!(
        detail["tasks"],
        json!([
            { "id": "a", "title": "Task a", "status": "done", "kind": "work", "round": 1 },
            { "id": "rev", "title": "Task rev", "status": "queued", "kind": "review", "round": 1 }
        ])
    );
    assert!(b.goal_detail_json("nope").is_none());
    // Nella vista della board: i goal e il goal di ogni task.
    let board = b.view_with(None, false, &crate::board::AgentGate::default());
    assert_eq!(board.goals, views);
    let task = serde_json::to_value(&board.tasks[0]).unwrap();
    assert_eq!((task["goal"].clone(), task["round"].clone()), (json!("g"), json!(1)));
    let loose = serde_json::to_value(board.tasks.iter().find(|t| t.task.id == "loose").unwrap()).unwrap();
    assert!(loose.get("goal").is_none() && loose.get("round").is_none());
    assert_eq!(crate::board::compact_row(&board.tasks[0])["goal"], "g");
}

// --- chiusura ---

#[test]
fn a_goal_with_every_task_done_closes_as_delivered_with_the_last_review() {
    let mut b = board_with_goal();
    finish(&mut b, "a", 20, None);
    assert!(b.close_finished_goals(21).is_empty(), "the review is still open");
    finish(&mut b, "rev", 22, Some(Verdict::Issues));
    b.add_proposal("p-2".into(), "rev-tab", Some("rev".into()), vec![spec("fix", &["a"])].into(), CTX, 23).unwrap();
    assert!(b.close_finished_goals(24).is_empty(), "a proposal is pending");
    b.accept_proposal("p-2", CTX, 25).unwrap();
    b.create_task_in(review("rev-2", &["fix"]), Some("g"), CTX, 25).unwrap();
    finish(&mut b, "fix", 26, None);
    finish(&mut b, "rev-2", 27, Some(Verdict::Passed));
    assert_eq!(b.close_finished_goals(28), ["g"]);
    let g = b.goal("g").unwrap();
    assert_eq!(
        (g.closed_at(), g.outcome(), g.review()),
        (Some(28), Some(GoalOutcome::Delivered), Some(GoalReview::Passed))
    );
    assert_eq!(b.updated_at, 28);
    assert!(b.close_finished_goals(29).is_empty(), "closed once");
}

#[test]
fn the_review_of_a_closed_goal_is_the_last_one_or_none() {
    let mut b = Board::new("ws-1");
    accept(&mut b, "p-1", new_goal("plain", vec![spec("p", &[])]), 1).unwrap();
    finish(&mut b, "p", 2, None);
    accept(&mut b, "p-2", new_goal("checked", vec![spec("c", &[]), review("r1", &["c"]), review("r2", &["c"])]), 1)
        .unwrap();
    finish(&mut b, "c", 2, None);
    finish(&mut b, "r2", 3, Some(Verdict::Passed));
    finish(&mut b, "r1", 4, Some(Verdict::Issues));
    b.close_finished_goals(5);
    assert_eq!(b.goal("plain").unwrap().review(), Some(GoalReview::NotReviewed));
    assert_eq!(b.goal("checked").unwrap().review(), Some(GoalReview::Issues), "r1 finished last");
}

#[test]
fn a_goal_left_without_tasks_closes_as_abandoned() {
    let mut b = Board::new("ws-1");
    accept(&mut b, "p-1", new_goal("g", vec![spec("a", &[])]), 1).unwrap();
    b.delete_task("a", 2).unwrap();
    assert_eq!(b.close_finished_goals(3), ["g"]);
    assert_eq!(b.goal("g").unwrap().outcome(), Some(GoalOutcome::Abandoned));
}

#[test]
fn closing_a_goal_by_hand_removes_its_open_work_and_proposals() {
    let mut b = board_with_goal();
    accept(&mut b, "p-2", in_goal("g", vec![spec("b", &[]), spec("c", &["b"])]), 11).unwrap();
    finish(&mut b, "a", 20, None);
    set_status(&mut b, "b", TaskStatus::Failed);
    b.add_proposal("p-3".into(), "intake", None, in_goal("g", vec![spec("d", &[])]), CTX, 21).unwrap();
    b.add_proposal("p-4".into(), "intake", None, vec![spec("e", &[])].into(), CTX, 21).unwrap();
    let removed = b.close_goal("g", 22).unwrap();
    let mut ids: Vec<&str> = removed.iter().map(|t| t.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["b", "c", "rev"]);
    assert_eq!(b.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a"], "done work stays");
    assert_eq!(b.proposals.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["p-4"]);
    let g = b.goal("g").unwrap();
    assert_eq!(
        (g.outcome(), g.review(), g.closed_at()),
        (Some(GoalOutcome::Abandoned), Some(GoalReview::NotReviewed), Some(22))
    );
    assert_eq!(b.close_goal("g", 23).unwrap_err(), "goal g is already closed");
    assert_eq!(b.close_goal("nope", 23).unwrap_err(), "unknown goal: nope");
    // Un goal chiuso non riceve piu' task.
    let err = accept(&mut b, "p-5", in_goal("g", vec![spec("x", &[])]), 24).unwrap_err();
    assert_eq!(err, "goal g is closed: propose a new goal");
}

#[test]
fn closing_a_goal_with_a_running_task_changes_nothing() {
    let mut b = board_with_goal();
    live(&mut b, "a", WorkerActivity::Running);
    let before = b.clone();
    let err = b.close_goal("g", 20).unwrap_err();
    assert_eq!(err, "cannot close goal 'g': a are running, stop them first");
    assert_eq!(b, before);
}

#[test]
fn user_notes_join_the_decision_log_and_the_oldest_fall_off() {
    let mut b = board_with_goal();
    b.add_goal_note("g", "  keep it simple  ", 20).unwrap();
    let last = b.goal("g").unwrap().decisions.last().unwrap().clone();
    assert_eq!(last, GoalDecision { at: 20, source: DecisionSource::User, text: "keep it simple".into() });
    for i in 0..DECISIONS_KEPT {
        b.add_goal_note("g", &format!("note {i}"), 30 + i as i64).unwrap();
    }
    let decisions = &b.goal("g").unwrap().decisions;
    assert_eq!(decisions.len(), DECISIONS_KEPT);
    assert_eq!((decisions[0].text.as_str(), decisions[DECISIONS_KEPT - 1].text.as_str()), ("note 0", "note 49"));
    assert_eq!(b.add_goal_note("g", " ", 90).unwrap_err(), "text is missing");
    assert!(b.add_goal_note("g", &"n".repeat(DECISION_MAX + 1), 90).unwrap_err().contains("too long"));
    assert_eq!(b.add_goal_note("nope", "x", 90).unwrap_err(), "unknown goal: nope");
    b.close_goal("g", 91).unwrap();
    assert!(b.add_goal_note("g", "late", 92).unwrap_err().contains("is closed"));
}

// --- archivio e lettura ---

/// Board con `n` task conclusi sciolti "t0".."t<n-1>" (il numero e' il tempo di fine).
fn board_with_finished(n: usize) -> Board {
    let mut b = Board::new("ws-1");
    for i in 0..n {
        b.tasks.push(Task::from_spec(spec(&format!("t{i}"), &[]), 1));
        finish(&mut b, &format!("t{i}"), 100 + i as i64, None);
    }
    b
}

#[test]
fn tasks_of_an_open_goal_are_never_archived() {
    let mut b = board_with_finished(MAX_FINISHED_KEPT);
    accept(&mut b, "p-1", new_goal("g", vec![spec("old", &[])]), 1).unwrap();
    finish(&mut b, "old", 0, None);
    b.tasks.push(Task::from_spec(spec("extra", &[]), 1));
    finish(&mut b, "extra", 1, None);
    let gone: Vec<String> = b.take_archivable().into_iter().map(|t| t.id).collect();
    assert_eq!(gone, ["extra"], "the oldest belongs to an open goal");
    b.close_finished_goals(2);
    b.tasks.push(Task::from_spec(spec("extra-2", &[]), 1));
    finish(&mut b, "extra-2", 999, None);
    let gone: Vec<String> = b.take_archivable().into_iter().map(|t| t.id).collect();
    assert_eq!(gone, ["old", "t0"], "once the goal is closed its tasks go, oldest first");
}

#[test]
fn closed_goals_beyond_the_limit_leave_the_board_oldest_first() {
    let mut b = Board::new("ws-1");
    let closed = |i: usize, at: i64| Goal { id: format!("c{i}"), round: 1, closed_at: Some(at), ..Goal::default() };
    for i in 0..MAX_CLOSED_GOALS_KEPT + 2 {
        b.goals.push(closed(i, 1000 - i as i64));
    }
    b.goals.push(Goal { id: "open".into(), round: 1, ..Goal::default() });
    let gone: Vec<String> = b.take_archivable_goals().into_iter().map(|g| g.id).collect();
    let (first, second) = (format!("c{}", MAX_CLOSED_GOALS_KEPT + 1), format!("c{MAX_CLOSED_GOALS_KEPT}"));
    assert_eq!(gone, [first.clone(), second.clone()], "closed earliest = the last pushed");
    assert_eq!(b.goals.len(), MAX_CLOSED_GOALS_KEPT + 1);
    assert!(b.goal("open").is_some() && b.goal(&first).is_none());
    assert!(b.take_archivable_goals().is_empty());
    // Rifiutati dall'archivio: tornano senza doppioni.
    b.restore_archived(Vec::new(), vec![closed(MAX_CLOSED_GOALS_KEPT, 1), closed(0, 1)]);
    assert_eq!(b.goals.len(), MAX_CLOSED_GOALS_KEPT + 2);
    assert_eq!(b.goals.last().unwrap().id, second);
}

#[test]
fn goals_and_task_membership_survive_a_save_and_a_load() {
    let mut b = board_with_goal();
    b.add_proposal("p-2".into(), "intake", None, new_goal("h", vec![spec("b", &[])]), CTX, 20).unwrap();
    b.close_goal("g", 21).unwrap();
    accept(&mut b, "p-3", new_goal("k", vec![spec("c", &[])]), 22).unwrap();
    let back = Board::from_json(&serde_json::to_string(&b).unwrap(), 30).unwrap();
    assert_eq!((back.goals.clone(), back.tasks.clone(), back.proposals.clone()), (b.goals, b.tasks, b.proposals));
    assert_eq!(back.task("c").unwrap().round(), 1);
}

#[test]
fn reading_restores_the_goal_invariants() {
    let raw = json!({
        "schema_version": 2,
        "data": {
            "workspace_id": "ws-1",
            "tasks": [
                { "id": "a", "title": "A", "task": "x", "goal": "g" },
                { "id": "b", "title": "B", "task": "x", "round": 4 }
            ]
        }
    });
    let b = Board::from_json(&raw.to_string(), 1).unwrap();
    assert!(b.goals.is_empty(), "a v2 file gains no goal");
    assert_eq!((b.task("a").unwrap().round(), b.task("b").unwrap().round()), (1, 0));
    let raw = json!({
        "schema_version": 3,
        "data": {
            "workspace_id": "ws-1",
            "goals": [
                { "id": "open", "title": "O", "request": "r", "done_when": ["d"], "outcome": "delivered" },
                { "id": "shut", "title": "S", "request": "r", "done_when": ["d"], "round": 2, "closed_at": 5 }
            ]
        }
    });
    let b = Board::from_json(&raw.to_string(), 1).unwrap();
    let (open, shut) = (b.goal("open").unwrap(), b.goal("shut").unwrap());
    assert_eq!((open.round(), open.is_open(), open.outcome(), open.review()), (1, true, None, None));
    assert_eq!(
        (shut.round(), shut.outcome(), shut.review()),
        (2, Some(GoalOutcome::Abandoned), Some(GoalReview::NotReviewed))
    );
}

#[test]
fn the_archive_line_of_a_goal_is_versioned() {
    let b = board_with_goal();
    let line = crate::board::archive_goal_line(b.goal("g").unwrap()).unwrap();
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!((v["schema_version"].clone(), v["data"]["id"].clone()), (json!(1), json!("g")));
    assert!(!line.contains('\n'));
}
