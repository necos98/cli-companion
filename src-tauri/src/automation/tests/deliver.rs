//! Consegna del goal (docs/GOALS.MD, "Consegna del goal"): quando parte il task di
//! consegna, cosa legge chi lo esegue in ogni modalita', i repository da
//! consegnare, nulla da consegnare e le impostazioni di consegna. Il git finto non
//! tiene branch ne' commit: `goal_branch_exists` fa esistere `goal/<id>` e una
//! regola su `rev-list --count` dice quanti commit ha oltre la base.

use super::*;
use crate::board::{Acceptance, GoalOutcome, GoalPhase, GoalReview, GoalWaiting, TaskKind};

pub(super) const GOAL_WT: &str = "C:\\p\\.worktrees\\deliver-g";

pub(super) fn goal_json(id: &str) -> Value {
    json!({ "id": id, "title": "Export", "request": "export the orders", "done_when": ["a CSV file"],
            "decisions": ["CSV, not Excel"] })
}

pub(super) fn review_spec(id: &str, deps: &[&str]) -> Value {
    let mut s = spec(id, deps);
    s["kind"] = json!("review");
    s
}

/// L'accettazione dell'utente con la sua scelta sulla consegna automatica.
pub(super) fn delivering(auto_deliver: Option<bool>) -> Acceptance {
    Acceptance { auto_deliver, ..Acceptance::default() }
}

/// Propone dall'Intake un goal nuovo con i task `tasks` e lo accetta, con la scelta
/// dell'utente sulla consegna automatica.
pub(super) fn accept_goal(st: &AutomationState, h: &FakeHost, goal: &str, tasks: Vec<Value>, auto: Option<bool>) {
    let r = call(st, h, INTAKE, "propose_tasks", json!({ "tasks": tasks, "goal": goal_json(goal) })).unwrap();
    boards::proposal_decide_with(st, h, "ws-1", r["proposal"].as_str().unwrap(), true, delivering(auto)).unwrap();
}

/// Un host in cui `goal/<goal>` esiste e ha `commits` commit oltre la base.
pub(super) fn host_with_commits(mode: DeliveryMode, goal: &str, commits: u32) -> FakeHost {
    let h = FakeHost::with_delivery(mode, matches!(mode, DeliveryMode::Pr | DeliveryMode::LocalMerge));
    h.goal_branch_exists(goal);
    h.git_rule("rev-list --count", true, &format!("{commits}\n"), "");
    h
}

/// Il worker del task `task` lavora e consegna: il goal ha il suo branch.
pub(super) fn complete_work(st: &AutomationState, h: &FakeHost, task_id: &str) {
    finish(st, h, &session_of(st, h, task_id), "completed");
}

/// Goal `g` pronto (il suo unico task `a` e' concluso), con `auto` come scelta dell'utente.
pub(super) fn ready_goal(mode: DeliveryMode, commits: u32, auto: Option<bool>) -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), host_with_commits(mode, "g", commits));
    accept_goal(&st, &h, "g", vec![spec("a", &[])], auto);
    complete_work(&st, &h, "a");
    (st, h)
}

pub(super) fn goal_view(st: &AutomationState, h: &FakeHost, id: &str) -> crate::board::GoalView {
    view(st, h).goals.into_iter().find(|g| g.goal.id == id).expect("goal")
}

fn count(h: &FakeHost, needle: &str) -> usize {
    h.git_log().iter().filter(|l| l.contains(needle)).count()
}

/// Un goal `dg` che si consegna da solo, sulla board com'e': ritorna la sessione del
/// worker di consegna, partito.
pub(super) fn started_deliverer(st: &AutomationState, h: &FakeHost) -> String {
    h.goal_branch_exists("dg");
    h.git_rule(
        "rev-list --count",
        true,
        "2
",
        "",
    );
    accept_goal(st, h, "dg", vec![spec("da", &[])], Some(true));
    complete_work(st, h, "da");
    deliverer_of(st, h, "dg")
}

/// Come `host_with_commits` per il goal `g`, con il repository `api` (che ha `api` commit)
/// oltre alla root. La regola piu' specifica va per prima: vince la prima che combacia.
pub(super) fn host_with_api(mode: DeliveryMode, root: u32, api: u32) -> FakeHost {
    let h = FakeHost::with_delivery(mode, matches!(mode, DeliveryMode::Pr | DeliveryMode::LocalMerge));
    h.dirs.lock().unwrap().push(r"C:\p\api".to_string());
    h.goal_branch_exists("g");
    h.git_rule(r"C:\p\api|rev-list --count", true, &format!("{api}\n"), "");
    h.git_rule("rev-list --count", true, &format!("{root}\n"), "");
    h
}

/// Il worker di consegna del goal `goal`, gia' partito.
pub(super) fn deliverer_of(st: &AutomationState, h: &FakeHost, goal: &str) -> String {
    session_of(st, h, &format!("deliver-{goal}"))
}

#[test]
fn every_mode_gets_its_own_delivery_steps_and_environment() {
    for (mode, parallel, must) in [
        (DeliveryMode::Pr, true, "gh pr create --base main --head goal/g"),
        (DeliveryMode::LocalMerge, true, "merge --ff-only goal/g"),
    ] {
        let (st, h) = (AutomationState::default(), host_with_commits(mode, "g", 3));
        h.set_delivery(mode, parallel);
        accept_goal(&st, &h, "g", vec![spec("a", &[])], None);
        complete_work(&st, &h, "a");
        boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
        let t = call(&st, &h, &deliverer_of(&st, &h, "g"), "get_task", json!({})).unwrap();
        assert_eq!(
            keys(&t),
            ["delivery", "environment", "goal", "goal_tasks", "instructions", "task", "title"],
            "{mode:?}"
        );
        let (steps, env, instr) =
            (t["delivery"].as_str().unwrap(), t["environment"].as_str().unwrap(), t["instructions"].as_str().unwrap());
        assert!(steps.contains(must), "{mode:?}: {steps}");
        assert!(steps.contains("complete_task with status \"completed\""), "{mode:?}: {steps}");
        assert_eq!(steps.contains("gh pr create"), mode == DeliveryMode::Pr, "{mode:?}");
        assert!(
            env.contains(&format!("worktree {GOAL_WT} on goal branch goal/g; base branch main")),
            "{mode:?}: {env}"
        );
        assert!(instr.contains("automated cli-companion deliverer"), "{mode:?}");
        // Il goal e i report dei task del goal.
        assert_eq!(t["goal"]["title"], "Export");
        assert_eq!(t["goal_tasks"][0]["id"], "a");
        assert_eq!(t["title"], "Consegna: Export");
        // Il worktree sta sul branch del goal, senza branch del task, e il worker lavora li'.
        let git = h.git_log();
        let add = format!("C:\\p|worktree add {GOAL_WT} goal/g");
        assert!(git.contains(&add), "{mode:?}: {git:?}");
        assert_eq!(h.spawns().last().unwrap().cwd.as_deref(), Some(GOAL_WT));
        let wt = task(&st, &h, "deliver-g")["worktree"].clone();
        assert_eq!((wt["branch"].as_str(), wt["path"].as_str()), (Some("goal/g"), Some(GOAL_WT)), "{mode:?}");
    }
}

#[test]
fn a_remote_mode_fetches_the_base_and_counts_the_commits_against_origin() {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::Pr, "g", 1));
    h.git_rule("remote get-url origin", true, "git@x:o/r.git", "");
    h.git_rule("rev-parse --verify --quiet refs/remotes/origin/main", true, "abc", "");
    accept_goal(&st, &h, "g", vec![spec("a", &[])], None);
    complete_work(&st, &h, "a");
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    assert!(h.git_log().contains(&"C:\\p|rev-list --count origin/main..goal/g".to_string()), "{:?}", h.git_log());
    assert_eq!(task(&st, &h, "deliver-g")["worktree"]["base"], "origin/main");
}

#[test]
fn an_auto_deliver_goal_without_issues_creates_its_delivery_by_itself() {
    let (st, h) = ready_goal(DeliveryMode::LocalMerge, 2, Some(true));
    let t = task(&st, &h, "deliver-g");
    assert_eq!(
        (t["kind"].as_str(), t["goal"].as_str(), t["status"].as_str()),
        (Some("deliver"), Some("g"), Some("running"))
    );
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.goal.auto_deliver()), (GoalPhase::Delivering, true));
    assert!(g.waiting.is_empty());
}

#[test]
fn without_auto_deliver_the_goal_waits_and_the_button_creates_the_delivery() {
    let (st, h) = ready_goal(DeliveryMode::LocalMerge, 2, None);
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.waiting.clone()), (GoalPhase::Ready, vec![GoalWaiting::Deliver]));
    assert_eq!(task(&st, &h, "deliver-g"), Value::Null);
    let v = boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    let t = v.tasks.iter().find(|t| t.task.id == "deliver-g").expect("the delivery task");
    assert_eq!((t.task.kind, t.task.goal(), t.task.bot.as_deref()), (TaskKind::Deliver, Some("g"), Some("general")));
    assert_eq!(goal_view(&st, &h, "g").phase, GoalPhase::Delivering);
    let again = boards::goal_deliver(&st, &h, "ws-1", "g").unwrap_err();
    assert_eq!(again, "goal g already has a delivery task: retry it");
    assert_eq!(boards::goal_deliver(&st, &h, "ws-1", "nope").unwrap_err(), "unknown goal: nope");
}

#[test]
fn the_delivery_task_goes_to_the_top_of_the_queue() {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::LocalMerge, "g", 2));
    accept_goal(&st, &h, "g", vec![spec("a", &[])], None);
    complete_work(&st, &h, "a");
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    queue(&st, &h, vec![spec("loose", &[])]);
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    let ids: Vec<String> = view(&st, &h).tasks.iter().map(|t| t.task.id.clone()).collect();
    let (d, l) = (ids.iter().position(|i| i == "deliver-g"), ids.iter().position(|i| i == "loose"));
    assert!(d < l, "{ids:?}");
}

#[test]
fn issues_in_the_last_review_never_start_the_delivery_but_deliver_anyway_works() {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::LocalMerge, "g", 2));
    accept_goal(&st, &h, "g", vec![spec("a", &[]), review_spec("rev", &["a"])], Some(true));
    complete_work(&st, &h, "a");
    let r = session_of(&st, &h, "rev");
    let p = call(&st, &h, &r, "propose_tasks", json!({ "tasks": [spec("fix", &["a"])] })).unwrap();
    let issues = json!({ "status": "completed", "summary": "r", "verdict": "issues", "findings": ["broken"] });
    call(&st, &h, &r, "complete_task", issues).unwrap();
    // La correzione e' scartata: il goal e' pronto con la review che ha trovato problemi.
    boards::proposal_decide(&st, &h, "ws-1", p["proposal"].as_str().unwrap(), false).unwrap();
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.last_review, g.goal.auto_deliver()), (GoalPhase::Ready, GoalReview::Issues, true));
    assert_eq!(g.waiting, [GoalWaiting::Deliver]);
    assert_eq!(task(&st, &h, "deliver-g"), Value::Null, "never by itself");
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(task(&st, &h, "deliver-g")["status"], "running");
}

#[test]
fn a_failed_automatic_creation_leaves_the_goal_ready_and_can_be_done_by_hand() {
    let (st, h) = (AutomationState::default(), host_with_commits(DeliveryMode::LocalMerge, "g", 2));
    accept_goal(&st, &h, "g", vec![spec("a", &[])], Some(true));
    h.edit_bots(|bots| bots.iter_mut().for_each(|b| b.enabled = false));
    complete_work(&st, &h, "a");
    assert_eq!(task(&st, &h, "deliver-g"), Value::Null);
    assert_eq!(goal_view(&st, &h, "g").phase, GoalPhase::Ready);
    assert!(st.registry().auto_delivering.is_empty(), "the next try is possible");
    assert!(boards::goal_deliver(&st, &h, "ws-1", "g").is_err(), "the button shows the reason");
    h.edit_bots(|bots| bots.iter_mut().for_each(|b| b.enabled = true));
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(task(&st, &h, "deliver-g")["status"], "running");
}

#[test]
fn an_automatic_delivery_is_asked_once_while_it_is_in_flight() {
    let (st, _h) = ready_goal(DeliveryMode::LocalMerge, 2, None);
    let advance = || crate::board::GoalAdvance { auto_deliver: vec!["g".to_string()], ..Default::default() };
    let mut r = st.registry();
    let auto =
        |r: &mut Registry| r.take_effects().into_iter().filter(|e| matches!(e, Effect::AutoDeliver { .. })).count();
    r.apply_advance("ws-1", advance());
    r.apply_advance("ws-1", advance());
    assert_eq!(auto(&mut r), 1);
    r.auto_deliver_done("ws-1", "g");
    r.apply_advance("ws-1", advance());
    assert_eq!(auto(&mut r), 1);
}

#[test]
fn nothing_to_deliver_closes_the_goal_without_a_worker_and_deletes_its_branches() {
    let (st, h) = ready_goal(DeliveryMode::LocalMerge, 0, None);
    let spawned = h.spawns().len();
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(h.spawns().len(), spawned, "no worker");
    let t = task(&st, &h, "deliver-g");
    assert_eq!(t["status"], "done");
    assert_eq!(t["report"]["summary"], "Nothing to deliver: the goal branches have no commits beyond their base.");
    assert_eq!(count(&h, "worktree add C:\\p\\.worktrees\\deliver-g"), 0);
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.goal.outcome()), (GoalPhase::Closed, Some(GoalOutcome::Delivered)));
    assert_eq!(g.goal.report().map(|r| r.commits.len() + r.pr_urls.len()), Some(0));
    // Subito: i branch del goal vanno via e la board li dimentica; la notifica c'e'.
    assert_eq!(count(&h, "branch -D goal/g"), 1, "{:?}", h.git_log());
    assert!(g.goal.branches.is_empty());
    let notices = h.notices();
    assert_eq!(notices.iter().filter(|n| n.kind == "delivered").count(), 1, "{notices:?}");
}

#[test]
fn a_missing_goal_branch_fails_the_start_with_a_clear_reason() {
    let (st, h) = ready_goal(DeliveryMode::LocalMerge, 2, None);
    h.git_rules.lock().unwrap().retain(|(needle, _)| !needle.contains("refs/heads/goal/g"));
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    let t = task(&st, &h, "deliver-g");
    assert_eq!(t["status"], "failed");
    assert!(t["note"].as_str().unwrap().contains("goal branch goal/g no longer exists"), "{t}");
}

#[test]
fn delivery_settings_are_set_by_the_user_and_validated() {
    let (st, h) = ready_goal(DeliveryMode::LocalMerge, 2, None);
    let v = boards::goal_set_delivery(&st, &h, "ws-1", "g", Some(true), None).unwrap();
    assert!(v.goals[0].goal.auto_deliver());
    let err = boards::goal_set_delivery(&st, &h, "ws-1", "g", None, None).unwrap_err();
    assert_eq!(err, "nothing to change: give auto_deliver or bot");
    let err = boards::goal_set_delivery(&st, &h, "ws-1", "g", None, Some("nope".into())).unwrap_err();
    assert!(err.contains("nope"), "{err}");
    h.edit_bots(|bots| {
        let mut other = bots[0].clone();
        other.id = "reviewer-only".into();
        other.kinds = vec![TaskKind::Review];
        bots.push(other);
    });
    assert!(boards::goal_set_delivery(&st, &h, "ws-1", "g", None, Some("reviewer-only".into())).is_err());
    let v = boards::goal_set_delivery(&st, &h, "ws-1", "g", Some(false), Some("general".into())).unwrap();
    assert_eq!((v.goals[0].goal.auto_deliver(), v.goals[0].goal.deliver_bot()), (false, "general"));
    assert!(boards::goal_set_delivery(&st, &h, "ws-1", "nope", Some(true), None).is_err());
}

#[test]
fn auto_deliver_in_a_decision_needs_a_proposal_with_a_new_goal() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    let r = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": [spec("loose", &[])] })).unwrap();
    let id = r["proposal"].as_str().unwrap();
    let err = boards::proposal_decide_with(&st, &h, "ws-1", id, true, delivering(Some(true))).unwrap_err();
    assert!(err.contains("new goal"), "{err}");
    assert!(boards::proposal_decide_with(&st, &h, "ws-1", id, true, delivering(Some(false))).is_ok());
}
