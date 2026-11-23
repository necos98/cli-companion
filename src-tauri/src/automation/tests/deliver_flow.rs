//! Ciclo di vita di una consegna: esito, chiusura del goal con il rapporto e la
//! notifica, pulizia dei worktree e dei branch, fallimento e retry, piu'
//! repository, tipo riservato, goal senza git e chi consegna come ruolo.

use super::deliver::{
    accept_goal, complete_work, deliverer_of, goal_json, goal_view, host_with_api, ready_goal, GOAL_WT,
};
use super::*;
use crate::board::{GoalOutcome, GoalPhase, GoalWaiting, TaskAction, TaskKind, DELIVER_KIND_RESERVED};

const PR_URL: &str = "https://github.com/o/r/pull/7";

fn count(h: &FakeHost, needle: &str) -> usize {
    h.git_log().iter().filter(|l| l.contains(needle)).count()
}

/// Goal `g` pronto e consegna avviata (il worker ha letto il task); ritorna la sessione.
fn started_delivery(mode: DeliveryMode) -> (AutomationState, FakeHost, String) {
    let (st, h) = ready_goal(mode, 2, None);
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    let w = deliverer_of(&st, &h, "g");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    (st, h, w)
}

fn delivered_report() -> Value {
    json!({ "status": "completed", "summary": "Merged the export", "commits": ["abc123"], "pr_urls": [PR_URL] })
}

#[test]
fn a_delivered_goal_closes_with_its_report_a_notice_and_its_branches_go_after_the_tab() {
    let (st, h, w) = started_delivery(DeliveryMode::Pr);
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    call(&st, &h, &w, "complete_task", delivered_report()).unwrap();
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.goal.outcome()), (GoalPhase::Closed, Some(GoalOutcome::Delivered)));
    let report = g.goal.report().cloned().expect("the report of the delivery");
    assert_eq!(report.summary, "Merged the export");
    assert_eq!((report.commits, report.pr_urls), (vec!["abc123".to_string()], vec![PR_URL.to_string()]));
    let notices: Vec<_> = h.notices().into_iter().filter(|n| n.kind == "delivered").collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    let n = &notices[0];
    assert_eq!((n.title.as_str(), n.body.as_str()), ("Export", "Goal consegnato: Export"));
    assert_eq!((n.task_id.as_str(), n.tab_id.as_deref(), n.workspace_id.as_str()), ("deliver-g", None, "ws-1"));
    // Il tab e' ancora aperto: niente e' stato toccato.
    assert_eq!(count(&h, "worktree remove"), 0);
    assert_eq!(count(&h, "branch -D"), 0);
    tab_closed(&st, &h, &w);
    assert_eq!(count(&h, &format!("worktree remove {GOAL_WT}")), 1, "{:?}", h.git_log());
    assert_eq!(count(&h, "branch -D goal/g"), 1);
    assert_eq!(count(&h, "branch -D task/") + count(&h, "branch -d "), 0, "the generic cleanup is not used");
    assert!(goal_view(&st, &h, "g").goal.branches.is_empty(), "the board forgets the deleted branches");
    assert!(task(&st, &h, "deliver-g")["worktree"].is_null());
}

#[test]
fn the_report_fields_of_a_delivery_are_only_for_delivery_tasks() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    queue(&st, &h, vec![spec("loose", &[])]);
    let work = session_of(&st, &h, "loose");
    let err = call(&st, &h, &work, "complete_task", delivered_report()).unwrap_err();
    assert_eq!(err, "commits and pr_urls are only accepted for deliver tasks");
    let verdict = json!({ "status": "completed", "summary": "x", "verdict": "passed" });
    assert!(call(&st, &h, &w, "complete_task", verdict).is_err(), "a delivery carries no verdict");
    call(&st, &h, &w, "complete_task", delivered_report()).unwrap();
}

#[test]
fn a_failed_delivery_keeps_the_goal_delivering_and_the_retry_reuses_the_worktree() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    call(&st, &h, &w, "complete_task", json!({ "status": "failed", "summary": "rebase conflict in a.txt" })).unwrap();
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.phase, g.goal.is_open(), g.waiting), (GoalPhase::Delivering, true, vec![GoalWaiting::Failed]));
    tab_closed(&st, &h, &w);
    assert_eq!((count(&h, "worktree remove"), count(&h, "branch -D")), (0, 0), "kept for the retry");
    assert!(h.notices().iter().any(|n| n.kind == "failed" && n.task_id == "deliver-g"));
    let adds = count(&h, "worktree add");
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    boards::task_action(&st, &h, "ws-1", "deliver-g", TaskAction::Retry).unwrap();
    assert_eq!(count(&h, "worktree add"), adds, "the same worktree, on the same branch");
    let t = task(&st, &h, "deliver-g");
    assert_eq!((t["status"].as_str(), t["worktree"]["branch"].as_str()), (Some("running"), Some("goal/g")));
    assert_eq!(h.spawns().last().unwrap().cwd.as_deref(), Some(GOAL_WT));
    // La consegna riuscita al secondo tentativo chiude il goal.
    let w2 = deliverer_of(&st, &h, "g");
    call(&st, &h, &w2, "get_task", json!({})).unwrap();
    call(&st, &h, &w2, "complete_task", delivered_report()).unwrap();
    assert_eq!(goal_view(&st, &h, "g").goal.outcome(), Some(GoalOutcome::Delivered));
}

#[test]
fn removing_a_delivery_task_removes_its_worktrees_and_keeps_the_branches() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    call(&st, &h, &w, "complete_task", json!({ "status": "failed", "summary": "no" })).unwrap();
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    boards::task_action(&st, &h, "ws-1", "deliver-g", TaskAction::Delete).unwrap();
    assert_eq!(count(&h, &format!("worktree remove {GOAL_WT}")), 1, "{:?}", h.git_log());
    assert_eq!(count(&h, "branch -D"), 0, "the goal was not delivered");
    assert_eq!(goal_view(&st, &h, "g").goal.branches.len(), 1);
    assert_eq!(goal_view(&st, &h, "g").phase, GoalPhase::Ready);
}

#[test]
fn closing_the_goal_removes_the_failed_delivery_worktrees_and_keeps_the_branches() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    call(&st, &h, &w, "complete_task", json!({ "status": "failed", "summary": "no" })).unwrap();
    tab_closed(&st, &h, &w);
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(count(&h, &format!("worktree remove {GOAL_WT}")), 1, "{:?}", h.git_log());
    assert_eq!(count(&h, "branch -D"), 0);
    let g = goal_view(&st, &h, "g");
    assert_eq!((g.goal.outcome(), g.goal.branches.len()), (Some(GoalOutcome::Abandoned), 1));
}

#[test]
fn a_finished_delivery_whose_tab_is_gone_is_cleaned_by_the_reconciliation() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    call(&st, &h, &w, "complete_task", delivered_report()).unwrap();
    st.registry().workers.get_mut(&w).unwrap().tab_open = false;
    let report = st.reconcile_worktrees(&h);
    assert_eq!(report.queued, ["ws-1/deliver-g"]);
    assert_eq!((count(&h, "worktree remove"), count(&h, "branch -D goal/g")), (1, 1), "{:?}", h.git_log());
}

#[test]
fn every_repository_of_the_goal_gets_a_worktree_and_is_cleaned() {
    let (st, h) = (AutomationState::default(), host_with_api(DeliveryMode::LocalMerge, 2, 1));
    let api = |id: &str| {
        let mut s = spec(id, &[]);
        s["repo"] = json!("api");
        s
    };
    accept_goal(&st, &h, "g", vec![spec("a", &[]), api("b")], Some(true));
    complete_work(&st, &h, "a");
    complete_work(&st, &h, "b");
    let w = deliverer_of(&st, &h, "g");
    let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
    let env = t["environment"].as_str().unwrap();
    assert!(env.contains("Repositories to deliver (2):"), "{env}");
    assert!(env.contains("- api: repository checkout C:\\p\\api"), "{env}");
    let api_wt = "C:\\p\\api\\.worktrees\\deliver-g";
    for add in [format!("C:\\p|worktree add {GOAL_WT} goal/g"), format!("C:\\p\\api|worktree add {api_wt} goal/g")] {
        assert!(h.git_log().contains(&add), "{add} in {:?}", h.git_log());
    }
    assert!(t["delivery"].as_str().unwrap().matches("merge --ff-only goal/g").count() == 2);
    h.dirs.lock().unwrap().extend([GOAL_WT.to_string(), api_wt.to_string()]);
    call(&st, &h, &w, "complete_task", delivered_report()).unwrap();
    tab_closed(&st, &h, &w);
    assert!(h.git_log().contains(&format!("C:\\p\\api|worktree remove {api_wt}")), "{:?}", h.git_log());
    assert_eq!((count(&h, "worktree remove"), count(&h, "branch -D goal/g")), (2, 2), "{:?}", h.git_log());
    assert!(goal_view(&st, &h, "g").goal.branches.is_empty());
}

#[test]
fn only_the_repositories_with_commits_are_delivered() {
    let (st, h) = (AutomationState::default(), host_with_api(DeliveryMode::LocalMerge, 2, 0));
    let mut b = spec("b", &[]);
    b["repo"] = json!("api");
    accept_goal(&st, &h, "g", vec![spec("a", &[]), b], Some(true));
    complete_work(&st, &h, "a");
    complete_work(&st, &h, "b");
    let t = call(&st, &h, &deliverer_of(&st, &h, "g"), "get_task", json!({})).unwrap();
    assert!(t["environment"].as_str().unwrap().contains("Repositories to deliver (1):"));
    assert_eq!(count(&h, "api\\.worktrees\\deliver-g"), 0, "{:?}", h.git_log());
}

#[test]
fn the_delivery_kind_is_reserved_to_the_app() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    queue(&st, &h, vec![spec("a", &[])]);
    let mut deliver = spec("d", &[]);
    deliver["kind"] = json!("deliver");
    let err = call(&st, &h, INTAKE, "propose_tasks", json!({ "tasks": [deliver.clone()] })).unwrap_err();
    assert!(err.contains(DELIVER_KIND_RESERVED), "{err}");
    let manual = json!({ "title": "Deliver", "task": "x", "kind": "deliver" });
    assert_eq!(boards::task_create(&st, &h, "ws-1", &manual).unwrap_err(), DELIVER_KIND_RESERVED);
    let err = call(&st, &h, INTAKE, "update_task", json!({ "id": "a", "kind": "deliver" })).unwrap_err();
    assert_eq!(err, DELIVER_KIND_RESERVED);
    assert_eq!(task(&st, &h, "a")["kind"], "work");
}

#[test]
fn a_goal_in_a_folder_without_git_closes_by_itself_without_a_delivery() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    accept_goal(&st, &h, "g", vec![spec("a", &[])], Some(true));
    complete_work(&st, &h, "a");
    let g = goal_view(&st, &h, "g");
    assert_eq!(
        (g.phase, g.goal.outcome(), g.goal.report().is_none()),
        (GoalPhase::Closed, Some(GoalOutcome::Delivered), true)
    );
    assert!(view(&st, &h).tasks.iter().all(|t| t.task.kind != TaskKind::Deliver));
    assert!(h.notices().iter().all(|n| n.kind != "delivered"), "nothing was delivered");
}

#[test]
fn the_goal_branch_is_never_deleted_by_the_generic_cleanup() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    let wt = crate::board::WorktreeInfo {
        path: GOAL_WT.to_string(),
        branch: "goal/g".to_string(),
        base: "main".to_string(),
        setup_done: true,
    };
    let target = CleanupTarget {
        ws_id: "ws-1".into(),
        task_id: "deliver-g".into(),
        repo: None,
        wt,
        workspace: None,
        keep_unmerged: false,
        goal_branch: None,
    };
    h.dirs.lock().unwrap().push(GOAL_WT.to_string());
    worktrees::run_cleanup(&st, &h, target);
    assert_eq!(count(&h, "worktree remove"), 1);
    assert_eq!(count(&h, "branch -"), 0, "{:?}", h.git_log());
}

#[test]
fn a_delivery_task_has_no_generic_cleanup_target() {
    let (st, _h, _) = started_delivery(DeliveryMode::LocalMerge);
    let delivery = st.registry().boards.get("ws-1").unwrap().task("deliver-g").unwrap().clone();
    assert!(delivery.worktree.is_some());
    assert!(CleanupTarget::for_task("ws-1", delivery).is_none(), "a delivery has its own cleanup");
}

#[test]
fn the_deliverer_reads_its_goal_and_its_tasks_but_nothing_else() {
    let (st, h, w) = started_delivery(DeliveryMode::LocalMerge);
    assert_eq!(call(&st, &h, &w, "whoami", json!({})).unwrap()["role"], "deliverer");
    let goal = call(&st, &h, &w, "get_goal", json!({ "id": "g" })).unwrap();
    assert_eq!(goal["goal"]["request"], goal_json("g")["request"]);
    let detail = call(&st, &h, &w, "get_task_details", json!({ "id": "a" })).unwrap();
    assert_eq!(detail["task"]["id"], "a");
    for method in ["propose_tasks", "list_tasks", "list_goals", "update_task", "delete_task", "propose_settings"] {
        let err = call(&st, &h, &w, method, json!({})).unwrap_err();
        assert_eq!(err, format!("{method} is not available to automated sessions"), "{method}");
    }
    assert!(call(&st, &h, &w, "run_e2e", json!({})).is_err_and(|e| !e.contains("not available")), "allowed by role");
}

#[test]
fn the_delivery_flow_works_with_real_git() {
    use super::reconcile_git::{commit_in, git_in, TempRepo};
    let Some(repo) = TempRepo::create("deliver-flow") else { return };
    let (st, h) = (AutomationState::default(), repo.host(DeliveryMode::LocalMerge));
    accept_goal(&st, &h, "g", vec![spec("a", &[])], None);
    // Il worker del task integra nel goal come dicono le istruzioni.
    let wt = repo.worktree("a");
    std::fs::write(wt.join("feature.txt"), "x\n").unwrap();
    git_in(&wt, &["add", "."]);
    commit_in(&wt, "feature");
    git_in(&wt, &["fetch", ".", "task/a:goal/g"]);
    let a = session_of(&st, &h, "a");
    finish(&st, &h, &a, "completed");
    tab_closed(&st, &h, &a);
    boards::goal_deliver(&st, &h, "ws-1", "g").unwrap();
    // Il worktree di consegna e' sul branch del goal, senza un branch del task.
    let delivery_wt = repo.worktree("deliver-g");
    assert_eq!(git_in(&delivery_wt, &["rev-parse", "--abbrev-ref", "HEAD"]), "goal/g");
    assert!(delivery_wt.join("feature.txt").is_file());
    assert_eq!(repo.git(&["branch", "--list", "task/deliver-g"]), "");
    // Il worker porta il goal sulla base (fast-forward senza checkout) e riferisce.
    let w = deliverer_of(&st, &h, "g");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    git_in(&repo.root, &["merge", "--ff-only", "goal/g"]);
    call(&st, &h, &w, "complete_task", json!({ "status": "completed", "summary": "done" })).unwrap();
    assert!(repo.git(&["branch", "--list", "goal/g"]).contains("goal/g"), "kept until the tab closes");
    tab_closed(&st, &h, &w);
    assert!(!delivery_wt.exists());
    assert_eq!(repo.git(&["branch", "--list", "goal/g"]), "");
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main", "the user's checkout is never touched");
    assert!(goal_view(&st, &h, "g").goal.branches.is_empty());
}
