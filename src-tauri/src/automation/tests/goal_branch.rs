//! Branch del goal (docs/GOALS.MD, "Branch del goal"): `goal/<id>` creato dal
//! primo task e registrato nella board, worktree di ogni task del goal da quel
//! branch in ogni modalita', istruzioni, pulizia, "Elimina branch". Il git finto
//! non tiene i branch: `goal_branch_exists` fa risultare esistente `goal/<id>`.

use super::*;
use crate::board::{Goal, GoalBranch, TaskAction};

const WORKTREE_A: &str = "C:\\p\\.worktrees\\a";

fn goal_json(id: &str) -> Value {
    json!({ "id": id, "title": "Export", "request": "export the orders", "done_when": ["a CSV file"],
            "decisions": ["CSV, not Excel"] })
}

/// Propone dall'Intake un goal nuovo `goal` con i task `tasks` e accetta.
fn queue_goal(st: &AutomationState, h: &FakeHost, goal: &str, tasks: Vec<Value>) {
    let r = call(st, h, INTAKE, "propose_tasks", json!({ "tasks": tasks, "goal": goal_json(goal) })).unwrap();
    boards::proposal_decide(st, h, "ws-1", r["proposal"].as_str().unwrap(), true).unwrap();
}

/// Aggiunge task a un goal esistente.
fn queue_into(st: &AutomationState, h: &FakeHost, goal: &str, tasks: Vec<Value>) {
    let r = call(st, h, INTAKE, "propose_tasks", json!({ "tasks": tasks, "goal_id": goal })).unwrap();
    boards::proposal_decide(st, h, "ws-1", r["proposal"].as_str().unwrap(), true).unwrap();
}

fn review_spec(id: &str, deps: &[&str]) -> Value {
    let mut s = spec(id, deps);
    s["kind"] = json!("review");
    s
}

fn count(h: &FakeHost, needle: &str) -> usize {
    h.git_log().iter().filter(|l| l.contains(needle)).count()
}

fn has_git(h: &FakeHost, line: &str) -> bool {
    h.git_log().iter().any(|l| l == line)
}

fn goal_of(st: &AutomationState, h: &FakeHost, id: &str) -> Goal {
    view(st, h).goals.into_iter().find(|g| g.goal.id == id).expect("goal").goal
}

fn gb(base: &str) -> GoalBranch {
    GoalBranch { branch: "goal/g".into(), base: base.into() }
}

fn set_base_branch(h: &FakeHost, base: &str) {
    let cfg = RepoConfig { base_branch: Some(base.to_string()), ..Default::default() };
    h.repo_cfg.lock().unwrap().insert(".".into(), cfg);
}

fn started(st: &AutomationState, h: &FakeHost, id: &str) -> bool {
    task(st, h, id)["session"].is_string()
}

#[test]
fn the_first_goal_task_creates_the_goal_branch_and_works_in_a_worktree_from_it() {
    for (mode, parallel) in [
        (DeliveryMode::Commit, false),
        (DeliveryMode::Push, false),
        (DeliveryMode::LocalMerge, false),
        (DeliveryMode::LocalMerge, true),
        (DeliveryMode::Pr, true),
    ] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
        queue_goal(&st, &h, "g", vec![spec("a", &[])]);
        let git = h.git_log();
        let find = |needle: &str| git.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle} in {git:?}"));
        assert!(
            find("C:\\p|branch --no-track goal/g main")
                < find(&format!("C:\\p|worktree add {WORKTREE_A} -b task/a goal/g")),
            "{mode:?}: {git:?}"
        );
        assert!(!git.iter().any(|l| l.contains("switch") || l.contains("checkout")), "{mode:?}: {git:?}");
        assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")), "{mode:?}");
        let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
        assert_eq!(saved.goal("g").unwrap().branches.get("."), Some(&gb("main")), "recorded in the saved board");
        let t = task(&st, &h, "a");
        assert_eq!(
            (t["worktree"]["branch"].as_str(), t["worktree"]["base"].as_str()),
            (Some("task/a"), Some("goal/g"))
        );
        assert_eq!(h.spawns()[0].cwd.as_deref(), Some(WORKTREE_A), "{mode:?}");
    }
}

#[test]
fn the_second_task_of_the_goal_reuses_the_branch_and_its_base() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    h.goal_branch_exists("g");
    // Il base del workspace cambia (checkout dell'utente su un altro branch): il goal resta sul suo.
    *h.branch.lock().unwrap() = Some("develop".into());
    queue_into(&st, &h, "g", vec![spec("b", &[])]);
    assert_eq!(count(&h, "branch --no-track goal/g"), 1, "{:?}", h.git_log());
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\b -b task/b goal/g"), "{:?}", h.git_log());
    assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")));
    // Il worker di b legge il base registrato.
    let env = call(&st, &h, &session_of(&st, &h, "b"), "get_task", json!({})).unwrap();
    assert!(env["environment"].as_str().unwrap().contains("goal branch: goal/g; base branch: main"), "{env}");
}

#[test]
fn in_pr_mode_the_goal_branch_starts_from_origin_after_the_fetch() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    h.git_rule("remote get-url origin", true, "git@x:o/r.git", "");
    h.git_rule("rev-parse --verify --quiet refs/remotes/origin/main", true, "abc", "");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    let git = h.git_log();
    let find = |needle: &str| git.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle} in {git:?}"));
    assert!(find("C:\\p|fetch origin main") < find("C:\\p|branch --no-track goal/g origin/main"), "{git:?}");
    assert!(has_git(&h, &format!("C:\\p|worktree add {WORKTREE_A} -b task/a goal/g")), "{git:?}");
    assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")), "the base is the plain name");
    // Il worktree nasce dal branch del goal: niente fetch per il task.
    assert_eq!(count(&h, "fetch"), 1, "{git:?}");
}

#[test]
fn an_existing_goal_branch_that_is_not_recorded_is_reused() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.goal_branch_exists("g");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    assert_eq!(count(&h, "branch --no-track goal/g"), 0, "{:?}", h.git_log());
    assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")));
}

#[test]
fn a_git_branch_failure_is_fatal_unless_the_branch_exists_now() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.git_rule("branch --no-track goal/g main", false, "", "fatal: cannot lock ref");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("cannot lock ref"), "{a}");
    assert!(goal_of(&st, &h, "g").branches.is_empty());
}

#[test]
fn retry_reuses_the_worktree_and_never_touches_the_goal_branch() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    h.git_rule("remote get-url origin", true, "git@x:o/r.git", "");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    finish(&st, &h, &session_of(&st, &h, "a"), "failed");
    let before = h.git_log().len();
    boards::task_action(&st, &h, "ws-1", "a", TaskAction::Retry).unwrap();
    let again: Vec<String> = h.git_log().split_off(before);
    assert!(
        again.iter().all(|l| !l.contains("fetch") && !l.contains("branch") && !l.contains("worktree")),
        "{again:?}"
    );
    assert_eq!(count(&h, "worktree add"), 1);
    assert_eq!(h.spawns().len(), 2);
    assert_eq!(h.spawns()[1].cwd.as_deref(), Some(WORKTREE_A));
    let env = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    assert!(env["environment"]
        .as_str()
        .unwrap()
        .contains("Task branch: task/a; goal branch: goal/g; base branch: main"));
}

#[test]
fn two_goals_in_the_same_repository_get_their_own_branch_and_run_together() {
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::Pr, true)] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
        queue_goal(&st, &h, "g1", vec![spec("a", &[])]);
        queue_goal(&st, &h, "g2", vec![spec("b", &[])]);
        assert!(started(&st, &h, "a") && started(&st, &h, "b"), "{mode:?}");
        assert!(
            has_git(&h, "C:\\p|branch --no-track goal/g1 main") && has_git(&h, "C:\\p|branch --no-track goal/g2 main"),
            "{mode:?}"
        );
        assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a goal/g1"));
        assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\b -b task/b goal/g2"));
    }
}

#[test]
fn goal_tasks_run_in_parallel_in_place_modes_without_a_checkout_on_the_base() {
    for mode in [DeliveryMode::Commit, DeliveryMode::LocalMerge] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, false));
        queue_goal(&st, &h, "g", vec![spec("a", &[]), spec("b", &[]), spec("c", &[])]);
        assert!(["a", "b", "c"].iter().all(|id| started(&st, &h, id)), "{mode:?}");
    }
    // Con i worktree in `local_merge` un task sciolto vuole il checkout sul base; uno di un goal no.
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    set_base_branch(&h, "main");
    *h.branch.lock().unwrap() = Some("dev".into());
    queue(&st, &h, vec![spec("loose", &[])]);
    assert_eq!(task(&st, &h, "loose")["status"], "failed");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    assert!(started(&st, &h, "a"), "{:?}", task(&st, &h, "a"));
}

#[test]
fn a_loose_task_is_still_exclusive_while_a_goal_runs_in_the_same_repository() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    queue(&st, &h, vec![spec("l1", &[]), spec("l2", &[])]);
    assert!(started(&st, &h, "a") && started(&st, &h, "l1") && !started(&st, &h, "l2"));
}

#[test]
fn a_recorded_goal_branch_that_vanished_fails_the_start() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    // `goal/g` non esiste piu' (il git finto lo dice): b non lo ricrea.
    queue_into(&st, &h, "g", vec![spec("b", &[])]);
    let b = task(&st, &h, "b");
    assert_eq!(b["status"], "failed");
    let note = b["note"].as_str().unwrap();
    assert!(note.contains("the goal branch goal/g no longer exists in C:\\p"), "{note}");
    assert_eq!(count(&h, "branch --no-track goal/g"), 1);
    assert!(!has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\b -b task/b goal/g"));
}

#[test]
fn a_goal_in_a_folder_without_git_works_in_free_mode_with_no_branch() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    assert!(started(&st, &h, "a"), "{:?}", task(&st, &h, "a"));
    assert!(goal_of(&st, &h, "g").branches.is_empty());
    assert_eq!(count(&h, "branch --no-track goal/"), 0);
    assert_eq!(count(&h, "worktree add"), 0);
    assert_eq!(h.spawns()[0].cwd.as_deref(), Some("C:\\p"));
    let t = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    assert!(!t["environment"].as_str().unwrap().contains("goal/"), "{t}");
    assert!(!t["delivery"].as_str().unwrap().contains("goal/"), "{t}");
}

#[test]
fn get_task_of_a_goal_task_has_the_goal_environment_and_delivery() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    let t = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    let (env, delivery) = (t["environment"].as_str().unwrap(), t["delivery"].as_str().unwrap());
    assert!(
        env.contains("- Delivery mode: goal branch goal/g (the goal is delivered to main as a whole later)"),
        "{env}"
    );
    assert!(env.contains("- Git worktree: branch task/a (created from goal/g)"), "{env}");
    assert!(env.contains("- Task branch: task/a; goal branch: goal/g; base branch: main"), "{env}");
    assert!(delivery.contains("git fetch . task/a:goal/g"), "{delivery}");
    assert!(delivery.contains("git rebase goal/g"), "{delivery}");
    assert!(!delivery.contains("git push") && !delivery.contains("merge --ff-only"), "{delivery}");
    assert!(!delivery.contains("gh pr"), "{delivery}");
}

#[test]
fn get_task_of_a_loose_task_is_unchanged() {
    let (st, h, w) = running(DeliveryMode::Pr, true);
    let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
    for key in ["environment", "delivery", "instructions"] {
        assert!(!t[key].as_str().unwrap().contains("goal/"), "{key}: {t}");
    }
    assert!(t["delivery"].as_str().unwrap().contains("gh pr create"));
}

#[test]
fn the_goal_review_works_in_a_worktree_from_the_goal_branch() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    queue_goal(&st, &h, "g", vec![spec("a", &[]), review_spec("rev", &["a"])]);
    h.goal_branch_exists("g");
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\rev -b task/rev goal/g"), "{:?}", h.git_log());
    let t = call(&st, &h, &session_of(&st, &h, "rev"), "get_task", json!({})).unwrap();
    let (env, instructions) = (t["environment"].as_str().unwrap(), t["instructions"].as_str().unwrap());
    assert!(env.contains("- Goal branch: goal/g (the integrated state to verify); base branch: main"), "{env}");
    assert!(instructions.contains("it is the goal branch goal/g"), "{instructions}");
    assert!(instructions.contains("git diff main...goal/g"), "{instructions}");
    assert!(!instructions.contains("check `origin/<base>`"), "{instructions}");
}

#[test]
fn a_goal_task_branch_is_deleted_at_done_only_when_the_goal_contains_it() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    tab_closed(&st, &h, &w);
    let git = h.git_log();
    let find = |needle: &str| git.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle} in {git:?}"));
    assert!(find("C:\\p|merge-base --is-ancestor task/a goal/g") < find("C:\\p|branch -D task/a"), "{git:?}");
    assert!(find("C:\\p|worktree remove C:\\p\\.worktrees\\a") < find("C:\\p|merge-base --is-ancestor task/a goal/g"));
    assert!(task(&st, &h, "a")["worktree"].is_null());
}

#[test]
fn a_goal_task_branch_not_contained_in_the_goal_is_kept_with_a_warning() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    h.git_rule("merge-base --is-ancestor", false, "", "");
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    tab_closed(&st, &h, &w);
    assert_eq!(count(&h, "branch -D"), 0, "{:?}", h.git_log());
    let a = task(&st, &h, "a");
    assert!(a["note"].as_str().unwrap().contains("branch task/a kept: not contained in goal/g"), "{a}");
    assert!(a["worktree"].is_null(), "the worktree is gone anyway");
}

#[test]
fn a_failed_goal_task_keeps_its_worktree_and_branch() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    tab_closed(&st, &h, &w);
    assert_eq!(count(&h, "worktree remove") + count(&h, "branch -D") + count(&h, "merge-base"), 0, "{:?}", h.git_log());
    assert!(task(&st, &h, "a")["worktree"].is_object());
}

/// Un goal "g" abbandonato (il suo task `a` fallito e tolto) con il branch registrato.
fn abandoned_goal() -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "g", vec![spec("a", &[])]);
    finish(&st, &h, &session_of(&st, &h, "a"), "failed");
    boards::goal_close(&st, &h, "ws-1", "g").unwrap();
    (st, h)
}

#[test]
fn closing_a_goal_keeps_its_branches_and_delete_branches_removes_them() {
    let (st, h) = abandoned_goal();
    assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")), "closing keeps the branches");
    h.goal_branch_exists("g");
    let v = boards::goal_delete_branches(&st, &h, "ws-1", "g").unwrap();
    assert!(has_git(&h, "C:\\p|branch -D goal/g"), "{:?}", h.git_log());
    assert!(v.goals[0].goal.branches.is_empty());
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert!(saved.goal("g").unwrap().branches.is_empty());
    assert_eq!(boards::goal_delete_branches(&st, &h, "ws-1", "g").unwrap_err(), "goal g has no branches");
}

#[test]
fn a_branch_that_is_already_gone_counts_as_deleted() {
    let (st, h) = abandoned_goal();
    let v = boards::goal_delete_branches(&st, &h, "ws-1", "g").unwrap();
    assert_eq!(count(&h, "branch -D goal/g"), 0);
    assert!(v.goals[0].goal.branches.is_empty());
}

#[test]
fn delete_branches_reports_what_git_refuses_and_keeps_the_entry() {
    let (st, h) = abandoned_goal();
    h.goal_branch_exists("g");
    h.git_rule("branch -D goal/g", false, "", "error: Cannot delete branch 'goal/g' checked out at 'C:/p'");
    let err = boards::goal_delete_branches(&st, &h, "ws-1", "g").unwrap_err();
    assert!(err.contains("branch goal/g not deleted") && err.contains("checked out"), "{err}");
    assert_eq!(goal_of(&st, &h, "g").branches.len(), 1, "kept: the user can retry");
}

#[test]
fn delete_branches_refuses_open_and_delivered_goals_and_unknown_workspaces() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue_goal(&st, &h, "open", vec![spec("a", &[])]);
    let err = boards::goal_delete_branches(&st, &h, "ws-1", "open").unwrap_err();
    assert_eq!(err, "goal open is open: close it first");
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    let err = boards::goal_delete_branches(&st, &h, "ws-1", "open").unwrap_err();
    assert_eq!(err, "goal open was delivered: its branches are kept");
    assert_eq!(boards::goal_delete_branches(&st, &h, "ws-1", "nope").unwrap_err(), "unknown goal: nope");
    assert_eq!(boards::goal_delete_branches(&st, &h, "ws-9", "open").unwrap_err(), "workspace non trovato");
    assert_eq!(count(&h, "branch -D goal/"), 0);
}

#[test]
fn the_goal_branch_flow_works_with_real_git() {
    use super::reconcile_git::{commit_in, git_in, TempRepo};
    let Some(repo) = TempRepo::create("goal-branch") else { return };
    let (st, h) = (AutomationState::default(), repo.host(DeliveryMode::Commit));
    queue_goal(&st, &h, "g", vec![spec("a", &[]), spec("b", &["a"])]);
    let main = repo.git(&["rev-parse", "main"]);
    assert_eq!((repo.git(&["rev-parse", "goal/g"]), repo.git(&["rev-parse", "task/a"])), (main.clone(), main));
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main", "the user's checkout is never touched");
    // Il worker fa quello che dice la consegna: commit e avanzamento del goal senza checkout.
    let wt = repo.worktree("a");
    std::fs::write(wt.join("feature.txt"), "x\n").unwrap();
    git_in(&wt, &["add", "."]);
    commit_in(&wt, "feature");
    git_in(&wt, &["fetch", ".", "task/a:goal/g"]);
    assert_eq!(repo.git(&["rev-parse", "goal/g"]), repo.git(&["rev-parse", "task/a"]));
    assert_ne!(repo.git(&["rev-parse", "goal/g"]), repo.git(&["rev-parse", "main"]));
    // A done il branch del task e' contenuto nel goal: via worktree e branch.
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    tab_closed(&st, &h, &w);
    assert!(!wt.exists());
    assert_eq!(repo.git(&["branch", "--list", "task/a"]), "");
    // Il task successivo parte dal goal e vede il lavoro del primo.
    assert!(repo.worktree("b").join("feature.txt").is_file());
    assert_eq!(goal_of(&st, &h, "g").branches.get("."), Some(&gb("main")));
}
