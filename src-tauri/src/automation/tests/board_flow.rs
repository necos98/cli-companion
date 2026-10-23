//! La coda dei task: proposte dell'Intake, scheduler, avvio per modalita' di
//! consegna, retry, stop, eliminazione, comandi della UI.

use super::*;
use crate::board::{Task, TaskSpec};

fn pr_parallel() -> (AutomationState, FakeHost) {
    (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true))
}

fn status_of(st: &AutomationState, h: &FakeHost, id: &str) -> String {
    task(st, h, id)["status"].as_str().unwrap_or("missing").to_string()
}

fn has_git(h: &FakeHost, line: &str) -> bool {
    h.git_log().iter().any(|l| l == line)
}

// --- proposte ---

#[test]
fn propose_creates_a_pending_proposal_and_starts_nothing() {
    let (st, h) = pr_parallel();
    let r = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("a", &[]), spec("b", &["a"])]})).unwrap();
    assert_eq!(r["status"], "pending");
    assert!(r["message"].as_str().unwrap().contains("accept or discard it in the app"));
    let pid = r["proposal"].as_str().unwrap().to_string();
    assert!(pid.starts_with("p-"));
    let v = view(&st, &h);
    assert!(v.tasks.is_empty() && h.spawns().is_empty());
    assert_eq!(v.proposals.len(), 1);
    assert_eq!((v.proposals[0].id.as_str(), v.proposals[0].source_tab.as_str()), (pid.as_str(), INTAKE));
    assert_eq!(v.proposals[0].tasks.len(), 2);
    // La board e' pubblicata e salvata con la proposta.
    assert_eq!(h.board_events.lock().unwrap().last().unwrap().proposals.len(), 1);
    assert_eq!(h.saved.lock().unwrap()["ws-1"].proposals.len(), 1);
    let list = call(&st, &h, INTAKE, "list_tasks", json!({})).unwrap();
    assert_eq!(list["proposals_pending"], 1);
    // Due proposte hanno id progressivi diversi.
    let r2 = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("z", &[])]})).unwrap();
    assert_ne!(r2["proposal"], r["proposal"]);
}

#[test]
fn propose_validates_everything_and_changes_nothing_on_error() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[])]);
    let before = view(&st, &h);
    let err = |p: Value| call(&st, &h, INTAKE, "propose_tasks", p).unwrap_err();
    assert!(err(json!({"tasks": [spec("c", &["d"]), spec("d", &["c"])]})).contains("dependency cycle"));
    assert!(err(json!({"tasks": [spec("c", &["zz"])]})).contains("unknown task"));
    assert!(err(json!({"tasks": [spec("a", &[])]})).contains("duplicate"), "id of an existing task");
    assert!(err(json!({"tasks": [spec("c", &[]), spec("c", &[])]})).contains("duplicate"));
    assert!(err(json!({"tasks": [{"id": "BAD", "title": "T", "task": "x"}]})).contains("invalid id"));
    assert!(err(json!({"tasks": [{"id": "c", "task": "x"}]})).contains("title is missing"));
    assert!(err(json!({"tasks": [{"id": "c", "title": "T", "task": "x", "touches": ["src"]}]})).contains("unknown field"));
    assert!(err(json!({})).contains("tasks is missing"));
    assert!(err(json!({"tasks": [{"id": "c", "title": "T", "task": "x", "model": "sonnet", "repo": "manca"}]})).contains("does not exist"));
    // Gli id delle proposte pendenti sono riservati anche alle altre proposte.
    call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("p1", &[])]})).unwrap();
    assert!(err(json!({"tasks": [spec("p1", &[])]})).contains("pending proposal"));
    // Una dipendenza su un task di un'altra proposta pendente non e' valida.
    assert!(err(json!({"tasks": [spec("q", &["p1"])]})).contains("unknown task"));
    let after = view(&st, &h);
    assert_eq!(after.tasks, before.tasks);
    assert_eq!(after.proposals.len(), 1);
}

#[test]
fn propose_checks_repos_by_delivery_mode() {
    let rooted = |repo: &str| json!({"tasks": [{"id": "c", "title": "T", "task": "x", "model": "sonnet", "repo": repo}]});
    // Modalita' con branch: il repo deve essere git.
    let (st, h) = pr_parallel();
    h.dirs.lock().unwrap().push("C:\\p\\api".to_string());
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    let e = call(&st, &h, INTAKE, "propose_tasks", rooted("api")).unwrap_err();
    assert!(e.contains("not a git repository") && e.contains("branches"), "{e}");
    let e = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("c", &[])]})).unwrap_err();
    assert!(e.contains("not a git repository"), "root checked too: {e}");
    // Senza branch basta che la cartella esista.
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.dirs.lock().unwrap().push("C:\\p\\api".to_string());
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    assert!(call(&st, &h, INTAKE, "propose_tasks", rooted("api")).is_ok());
    assert!(call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("d", &[])]})).is_ok());
    assert!(h.git_log().is_empty(), "no git needed: {:?}", h.git_log());
    assert!(call(&st, &h, INTAKE, "propose_tasks", rooted("manca")).unwrap_err().contains("does not exist"));
}

#[test]
fn accept_queues_in_order_discard_removes_and_a_failed_accept_keeps_the_proposal() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let p1 = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("a", &[]), spec("b", &["a"])]})).unwrap();
    let p2 = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("c", &[])]})).unwrap();
    let (p1, p2) = (p1["proposal"].as_str().unwrap(), p2["proposal"].as_str().unwrap());
    boards::proposal_decide(&st, &h, "ws-1", p2, false).unwrap();
    let v = boards::proposal_decide(&st, &h, "ws-1", p1, true).unwrap();
    assert!(v.proposals.is_empty());
    let order: Vec<&str> = v.tasks.iter().map(|t| t.task.id.as_str()).collect();
    assert_eq!(order, ["a", "b"]);
    assert_eq!(status_of(&st, &h, "a"), "running", "serial mode starts the first task");
    assert_eq!(task(&st, &h, "b")["waiting_on"], json!(["a"]));
    // Decidere due volte, o su una proposta ignota, e' un errore.
    assert!(boards::proposal_decide(&st, &h, "ws-1", p1, true).unwrap_err().contains("proposal not found"));
    assert!(boards::proposal_decide(&st, &h, "ws-1", "p-nope", false).is_err());
    // Rivalidazione all'accettazione: un id nel frattempo occupato la fa fallire.
    let p3 = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("d", &[])]})).unwrap();
    st.inner.lock().unwrap().boards.get_mut("ws-1").unwrap().tasks.push(Task::from_spec(TaskSpec { id: "d".into(), title: "D".into(), task: "x".into(), ..Default::default() }, 0));
    let e = boards::proposal_decide(&st, &h, "ws-1", p3["proposal"].as_str().unwrap(), true).unwrap_err();
    assert!(e.contains("duplicate"), "{e}");
    assert_eq!(view(&st, &h).proposals.len(), 1);
    // Accettare rilegge anche i repository: la modalita' di consegna puo' essere cambiata.
    let p4 = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [{"id": "e", "title": "E", "task": "x", "model": "sonnet", "repo": "esiste-r"}]})).unwrap();
    h.dirs.lock().unwrap().push("C:\\p\\esiste-r".to_string());
    h.set_delivery(DeliveryMode::Pr, true);
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal");
    let e = boards::proposal_decide(&st, &h, "ws-1", p4["proposal"].as_str().unwrap(), true).unwrap_err();
    assert!(e.contains("not a git repository"), "{e}");
}

// --- avvio per modalita' di consegna ---

#[test]
fn pr_parallel_creates_a_worktree_from_the_base_and_spawns_in_it() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[])]);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|symbolic-ref --short refs/remotes/origin/HEAD".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a main".to_string()), "{git:?}");
    assert_eq!(h.excludes.lock().unwrap().as_slice(), ["C:\\p\\.git\\info\\exclude|.worktrees/"]);
    let sp = h.spawns();
    assert_eq!(sp.len(), 1);
    assert_eq!(sp[0].cwd.as_deref(), Some("C:\\p\\.worktrees\\a"));
    assert!(sp[0].parent_tab_id.is_empty() && sp[0].title == "Task a");
    assert!(sp[0].command.contains("AcceptEdits") && sp[0].command.contains("get_task"));
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["attempts"].as_u64(), a["worker_status"].as_str()), (Some("running"), Some(1), Some("starting")));
    assert_eq!(a["worktree"], json!({"path": "C:\\p\\.worktrees\\a", "branch": "task/a", "base": "main", "setup_done": true}));
    assert_eq!(a["session"], sp[0].tab_id);
}

#[test]
fn pr_base_is_fetched_from_origin_and_follows_the_resolution_order() {
    let (st, h) = pr_parallel();
    h.git_rule("remote get-url origin", true, "git@x:o/r.git", "");
    h.git_rule("symbolic-ref --short refs/remotes/origin/HEAD", true, "origin/develop\n", "");
    h.git_rule("refs/remotes/origin/develop", true, "abc\n", "");
    queue(&st, &h, vec![spec("a", &[])]);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|fetch origin develop".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a origin/develop".to_string()), "{git:?}");
    assert_eq!(task(&st, &h, "a")["worktree"]["base"], "origin/develop");
    // Il fetch ha una cache di 60 s per repository e base.
    queue(&st, &h, vec![spec("b", &[])]);
    assert_eq!(h.git_log().iter().filter(|l| l.contains("fetch origin develop")).count(), 1);
    // L'impostazione del repository vince sul default di origin.
    let (st, h) = pr_parallel();
    h.repo_cfg.lock().unwrap().insert(".".into(), RepoConfig { base_branch: Some("release".into()), ..Default::default() });
    queue(&st, &h, vec![spec("a", &[])]);
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a release"), "{:?}", h.git_log());
    assert!(!h.git_log().iter().any(|l| l.contains("symbolic-ref")));
    // Un fetch fallito non blocca: si usa il locale con un avviso.
    let (st, h) = pr_parallel();
    h.git_rule("remote get-url origin", true, "u", "");
    h.git_rule("fetch origin", false, "", "could not resolve host");
    queue(&st, &h, vec![spec("a", &[])]);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "running");
    assert!(a["note"].as_str().unwrap().contains("start warnings") && a["note"].as_str().unwrap().contains("git fetch origin main failed"), "{a}");
}

#[test]
fn local_merge_parallel_needs_the_checkout_on_the_base() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    h.repo_cfg.lock().unwrap().insert(".".into(), RepoConfig { base_branch: Some("main".into()), ..Default::default() });
    *h.branch.lock().unwrap() = Some("dev".into());
    queue(&st, &h, vec![spec("a", &[])]);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert_eq!(a["note"], "local_merge needs C:\\p checked out on main (now on dev)");
    assert!(h.spawns().is_empty() && !h.git_log().iter().any(|l| l.contains("worktree add")));
    assert_eq!(h.notices().len(), 1);
    // Sul branch giusto parte, dal base locale (nessun fetch).
    *h.branch.lock().unwrap() = Some("main".into());
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    assert_eq!(status_of(&st, &h, "a"), "running");
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a main"), "{:?}", h.git_log());
    assert!(!h.git_log().iter().any(|l| l.contains("fetch") || l.contains("symbolic-ref")));
    // Senza base dichiarata vale il branch corrente: la precondizione e' soddisfatta per definizione.
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, true));
    *h.branch.lock().unwrap() = Some("feature".into());
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status_of(&st, &h, "a"), "running");
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a feature"));
}

#[test]
fn commit_and_push_modes_work_in_place_serially_without_git() {
    for mode in [DeliveryMode::Commit, DeliveryMode::Push] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, true));
        queue(&st, &h, vec![spec("a", &[]), spec("b", &[]), spec("c", &["a"])]);
        // Un solo worker per repository, nel checkout; l'app non fa git.
        assert_eq!((status_of(&st, &h, "a"), status_of(&st, &h, "b")), ("running".into(), "queued".into()));
        assert!(h.git_log().is_empty(), "{mode:?}: {:?}", h.git_log());
        assert_eq!(h.spawns()[0].cwd.as_deref(), Some("C:\\p"));
        assert!(task(&st, &h, "a")["worktree"].is_null());
        let w = session_of(&st, &h, "a");
        let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
        assert!(t["environment"].as_str().unwrap().contains("No dedicated worktree"));
        // Finito a, parte b (il primo in coda), poi c.
        finish(&st, &h, &w, "completed");
        assert_eq!((status_of(&st, &h, "a"), status_of(&st, &h, "b"), status_of(&st, &h, "c")), ("done".into(), "running".into(), "queued".into()));
        finish(&st, &h, &session_of(&st, &h, "b"), "completed");
        assert_eq!(status_of(&st, &h, "c"), "running");
        assert_eq!(h.spawns().len(), 3);
    }
}

#[test]
fn pr_in_place_resolves_the_base_but_creates_nothing() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, false));
    h.git_rule("symbolic-ref --short refs/remotes/origin/HEAD", true, "origin/main\n", "");
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[])]);
    assert_eq!(status_of(&st, &h, "b"), "queued", "one worker per repository");
    assert!(!h.git_log().iter().any(|l| l.contains("worktree") || l.contains("fetch") || l.contains("switch")), "{:?}", h.git_log());
    let w = session_of(&st, &h, "a");
    let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
    let d = t["delivery"].as_str().unwrap();
    assert!(d.contains("git switch -c task/a origin/main") && d.contains("git branch -D task/a"), "{d}");
    assert!(t["instructions"].as_str().unwrap().contains("Work only inside C:\\p;"));
    // Un task done senza worktree non lancia nessuna pulizia.
    finish(&st, &h, &w, "completed");
    st.run(&h, |r| r.tab_closed(&w, 1));
    assert!(!h.git_log().iter().any(|l| l.contains("branch -")), "{:?}", h.git_log());
}

#[test]
fn local_merge_in_place_does_not_need_the_checkout_on_base_up_front() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::LocalMerge, false));
    h.repo_cfg.lock().unwrap().insert(".".into(), RepoConfig { base_branch: Some("main".into()), ..Default::default() });
    *h.branch.lock().unwrap() = Some("dev".into());
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status_of(&st, &h, "a"), "running");
    let t = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    assert!(t["delivery"].as_str().unwrap().contains("git switch -c task/a main"));
}

#[test]
fn multi_repo_tasks_use_their_own_checkout_base_and_slot() {
    let (st, h) = pr_parallel();
    h.dirs.lock().unwrap().push("C:\\p\\api".to_string());
    h.repo_cfg.lock().unwrap().insert("api".into(), RepoConfig { base_branch: Some("dev".into()), ..Default::default() });
    let mut t = spec("a", &[]);
    t["repo"] = json!("api");
    queue(&st, &h, vec![t, spec("b", &[])]);
    assert!(has_git(&h, "C:\\p\\api|worktree add C:\\p\\api\\.worktrees\\a -b task/a dev"), "{:?}", h.git_log());
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\b -b task/b main"), "{:?}", h.git_log());
    assert_eq!(h.spawns().len(), 2);
    assert_eq!(task(&st, &h, "a")["repo"], "api");
    // Serial: ogni repository ha il suo slot.
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.dirs.lock().unwrap().push("C:\\p\\api".to_string());
    let mut t = spec("a", &[]);
    t["repo"] = json!("api");
    queue(&st, &h, vec![t, spec("b", &[]), spec("c", &[])]);
    assert_eq!((status_of(&st, &h, "a"), status_of(&st, &h, "b"), status_of(&st, &h, "c")), ("running".into(), "running".into(), "queued".into()));
    assert_eq!(h.spawns()[0].cwd.as_deref(), Some("C:\\p\\api"));
}

#[test]
fn max_parallel_and_pause_control_the_parallel_starts() {
    let (st, h) = pr_parallel();
    boards::board_set(&st, &h, "ws-1", Some(2), None).unwrap();
    queue(&st, &h, (0..4).map(|i| spec(&format!("t{i}"), &[])).collect());
    assert_eq!(h.spawns().len(), 2);
    assert_eq!((status_of(&st, &h, "t2"), status_of(&st, &h, "t3")), ("queued".into(), "queued".into()));
    // Un posto libero: parte il prossimo in coda.
    finish(&st, &h, &session_of(&st, &h, "t0"), "completed");
    assert_eq!(status_of(&st, &h, "t2"), "running");
    // In pausa nulla di nuovo; i task in corso continuano.
    let v = boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    assert!(v.paused);
    finish(&st, &h, &session_of(&st, &h, "t1"), "completed");
    assert_eq!(status_of(&st, &h, "t3"), "queued");
    assert_eq!(status_of(&st, &h, "t2"), "running");
    let v = boards::board_set(&st, &h, "ws-1", None, Some(false)).unwrap();
    assert!(!v.paused);
    assert_eq!(status_of(&st, &h, "t3"), "running");
    // Parametri non validi.
    for (m, p) in [(Some(0), None), (Some(17), None), (None, None)] {
        assert!(boards::board_set(&st, &h, "ws-1", m, p).is_err());
    }
    assert!(boards::board_set(&st, &h, "ws-9", Some(2), None).is_err());
}

#[test]
fn delivery_mode_change_applies_to_tasks_started_afterwards() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[])]);
    assert_eq!(h.spawns().len(), 2);
    h.set_delivery(DeliveryMode::Commit, false);
    // I due in corso non si toccano; la vista riporta la nuova consegna.
    assert_eq!(status_of(&st, &h, "a"), "running");
    assert_eq!(view(&st, &h).delivery.mode, DeliveryMode::Commit);
    let ta = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    assert!(ta["delivery"].as_str().unwrap().contains("gh pr create"), "il worker in corso mantiene la sua modalita'");
    queue(&st, &h, vec![spec("c", &[])]);
    st.reconcile(&h);
    assert_eq!(status_of(&st, &h, "c"), "queued", "in place the root is busy: one worker per repository");
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    finish(&st, &h, &session_of(&st, &h, "b"), "completed");
    assert_eq!(status_of(&st, &h, "c"), "running");
    assert!(task(&st, &h, "c")["worktree"].is_null());
    assert_eq!(h.spawns().last().unwrap().cwd.as_deref(), Some("C:\\p"));
}

// --- fallimenti, retry, stop ---

#[test]
fn start_failure_fails_the_task_notifies_and_retry_reuses_the_worktree() {
    let (st, h) = pr_parallel();
    h.git_rule("worktree add", false, "", "fatal: '.worktrees/a' already exists");
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[])]);
    for id in ["a", "b"] {
        let t = task(&st, &h, id);
        assert_eq!(t["status"], "failed", "{id}");
        assert!(t["note"].as_str().unwrap().contains("already exists"));
        assert!(t["session"].is_null() && t["worker_status"].is_null());
    }
    assert!(h.spawns().is_empty());
    let n = h.notices();
    assert_eq!(n.len(), 2);
    assert_eq!((n[0].kind, n[0].body.as_str(), n[0].tab_id.clone()), ("failed", "Task a è fallito", None));
    // Dopo aver sistemato git, "Riprova" lo rimette in coda in fondo e riparte.
    h.git_rules.lock().unwrap().clear();
    let v = boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    assert_eq!(v.tasks.iter().map(|t| t.task.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["attempts"].as_u64()), (Some("running"), Some(2)));
    assert_eq!(h.spawns().len(), 1);
    // Il worker fallisce con il worktree: il retry lo riusa senza ricrearlo.
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    let adds = || h.git_log().iter().filter(|l| l.contains("worktree add")).count();
    let before = adds();
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    assert_eq!(adds(), before);
    assert_eq!(h.spawns().last().unwrap().cwd.as_deref(), Some("C:\\p\\.worktrees\\a"));
    assert_eq!(task(&st, &h, "a")["worktree"]["branch"], "task/a");
    // Esiste solo il branch: worktree add senza -b.
    finish(&st, &h, &session_of(&st, &h, "a"), "failed");
    h.dirs.lock().unwrap().clear();
    h.git_rule("rev-parse --verify --quiet refs/heads/task/a", true, "abc\n", "");
    st.inner.lock().unwrap().boards.get_mut("ws-1").unwrap().task_mut("a").unwrap().worktree = None;
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    assert!(has_git(&h, "C:\\p|worktree add C:\\p\\.worktrees\\a task/a"), "{:?}", h.git_log());
}

#[test]
fn repo_setup_runs_after_the_worktree_and_a_failed_setup_is_redone_on_retry() {
    let (st, h) = pr_parallel();
    h.repo_cfg.lock().unwrap().insert(
        ".".into(),
        RepoConfig { copy_ignored: vec![".env".into()], setup: vec!["npm ci".into()], container_root: Some("/srv".into()), ..Default::default() },
    );
    h.git_rule("check-ignore", true, "", "");
    h.files.lock().unwrap().push("C:\\p\\.env".to_string());
    h.hook_rules.lock().unwrap().push(("npm ci".into(), GitOut { ok: false, stdout: String::new(), stderr: "ERR! network".into() }));
    queue(&st, &h, vec![spec("a", &[])]);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("worktree setup of '.' failed at setup 1/1: npm ci") && a["note"].as_str().unwrap().contains("ERR! network"), "{a}");
    assert_eq!(a["worktree"]["setup_done"], false, "il retry lo rifa");
    assert!(a["preparing"].is_null());
    let log = h.git_log();
    let add = log.iter().position(|l| l.contains("worktree add")).unwrap();
    let setup = log.iter().position(|l| l == "hook|npm ci").unwrap();
    assert!(add < setup);
    assert_eq!(h.hook_log.lock().unwrap()[0], "copy C:\\p\\.env->C:\\p\\.worktrees\\a\\.env");
    let env = h.hook_log.lock().unwrap()[1].clone();
    assert!(env.starts_with("C:\\p\\.worktrees\\a|npm ci|") && env.contains("TASK_ID=a") && env.contains("BRANCH=task/a") && env.contains("CONTAINER_WORKTREE=/srv/.worktrees/a"), "{env}");
    // Sistemato il setup, il retry lo rifa nel worktree esistente e il worker parte.
    h.hook_rules.lock().unwrap().clear();
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["worktree"]["setup_done"].as_bool()), (Some("running"), Some(true)));
    assert_eq!(h.git_log().iter().filter(|l| l.contains("worktree add")).count(), 1);
    let t = call(&st, &h, &session_of(&st, &h, "a"), "get_task", json!({})).unwrap();
    let env = t["environment"].as_str().unwrap();
    assert!(env.contains("do NOT redo") && env.contains(".env") && env.contains("npm ci") && env.contains("/srv"), "{env}");
}

#[test]
fn stop_fails_the_task_closes_the_tab_and_is_quiet() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    let w = session_of(&st, &h, "a");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    let v = boards::task_action(&st, &h, "ws-1", "a", "stop").unwrap();
    let a = v.tasks.iter().find(|t| t.task.id == "a").unwrap();
    assert_eq!((a.task.status, a.task.note.as_deref()), (TaskStatus::Failed, Some("stopped by the user")));
    assert!(h.log().contains(&format!("close:{w}")));
    assert!(h.notices().is_empty(), "l'utente lo ha chiesto lui");
    // Il tab si chiude: nessun secondo fallimento. Il dipendente non parte.
    st.run(&h, |r| r.tab_closed(&w, 1));
    assert_eq!(task(&st, &h, "a")["note"], "stopped by the user");
    assert_eq!(status_of(&st, &h, "b"), "queued");
    assert!(call(&st, &h, &w, "complete_task", json!({"status": "completed", "summary": "x"})).is_err());
    // Si ferma solo cio' che e' in corso.
    assert!(boards::task_action(&st, &h, "ws-1", "a", "stop").unwrap_err().contains("only running"));
    assert!(boards::task_action(&st, &h, "ws-1", "b", "stop").is_err());
    // Stop mentre l'avvio e' ancora in corso (nessuna sessione): niente spawn dopo.
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[])]);
    {
        let mut reg = st.inner.lock().unwrap();
        let t = reg.boards.get_mut("ws-1").unwrap().task_mut("a").unwrap();
        t.session = None;
    }
    boards::task_action(&st, &h, "ws-1", "a", "stop").unwrap();
    assert_eq!(task(&st, &h, "a")["note"], "stopped by the user");
}

#[test]
fn a_dependency_must_be_done_not_failed() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    finish(&st, &h, &session_of(&st, &h, "a"), "failed");
    assert_eq!(status_of(&st, &h, "b"), "queued");
    assert_eq!(task(&st, &h, "b")["waiting_on"], json!(["a"]));
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    assert_eq!(status_of(&st, &h, "b"), "running");
}

#[test]
fn mark_done_unblocks_dependents_and_the_worker_picks_them_up() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    finish(&st, &h, &session_of(&st, &h, "a"), "failed");
    assert_eq!(status_of(&st, &h, "b"), "queued");
    assert!(boards::task_action(&st, &h, "ws-1", "b", "mark_done").unwrap_err().contains("only failed tasks can be marked done"));
    let v = boards::task_action(&st, &h, "ws-1", "a", "mark_done").unwrap();
    let a = v.tasks.iter().find(|t| t.task.id == "a").unwrap();
    assert_eq!((a.task.status, a.task.note.as_deref()), (TaskStatus::Done, None));
    // Lo scheduler vede subito il dipendente sbloccato, come dopo le altre azioni.
    assert_eq!(status_of(&st, &h, "b"), "running");
    assert!(boards::task_action(&st, &h, "ws-1", "a", "mark_done").unwrap_err().contains("is done"));
}

#[test]
fn delete_removes_failed_tasks_and_cleans_their_worktree() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"]), spec("c", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    // b dipende da a (non done): non si elimina a.
    let e = boards::task_action(&st, &h, "ws-1", "a", "delete").unwrap_err();
    assert!(e.contains("b depend on it"), "{e}");
    // Running non si elimina.
    assert!(boards::task_action(&st, &h, "ws-1", "c", "delete").unwrap_err().contains("only queued or failed"));
    // Eliminato b (queued), poi a (failed con worktree): pulizia senza forzare.
    boards::task_action(&st, &h, "ws-1", "b", "delete").unwrap();
    let v = boards::task_action(&st, &h, "ws-1", "a", "delete").unwrap();
    assert_eq!(v.tasks.iter().map(|t| t.task.id.as_str()).collect::<Vec<_>>(), ["c"]);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|worktree remove C:\\p\\.worktrees\\a".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|branch -D task/a".to_string()), "{git:?}");
    assert!(!git.iter().any(|l| l.contains("--force")));
    assert!(boards::task_action(&st, &h, "ws-1", "a", "delete").is_err());
    assert!(boards::task_action(&st, &h, "ws-1", "c", "boh").unwrap_err().contains("azione non valida"));
}

// --- comandi della UI ---

#[test]
fn task_create_generates_ids_and_validates() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let mk = |title: &str| boards::task_create(&st, &h, "ws-1", &json!({"title": title, "task": "do it"}));
    let ids = |v: &BoardView| v.tasks.iter().map(|t| t.task.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&mk("Modalità di consegna").unwrap()), ["modalit-di-consegna"]);
    assert_eq!(ids(&mk("Modalità di consegna").unwrap()).last().unwrap(), "modalit-di-consegna-2");
    assert_eq!(ids(&mk("Modalità di consegna").unwrap()).last().unwrap(), "modalit-di-consegna-3");
    assert_eq!(ids(&mk("日本語").unwrap()).last().unwrap(), "task-1");
    // Un id riservato da una proposta pendente non viene riusato.
    call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("riservato", &[])]})).unwrap();
    assert_eq!(ids(&mk("Riservato").unwrap()).last().unwrap(), "riservato-2");
    let v = view(&st, &h);
    assert!(v.tasks.iter().all(|t| t.task.status == TaskStatus::Queued && t.task.attempts == 0));
    // Con dipendenze e modello; errori di validazione.
    let v = boards::task_create(&st, &h, "ws-1", &json!({"title": "Dopo", "task": "x", "depends_on": ["task-1"], "model": "opus"})).unwrap();
    let t = v.tasks.last().unwrap();
    assert_eq!((t.task.id.as_str(), t.task.model.as_deref(), t.waiting_on.as_slice()), ("dopo", Some("opus"), &["task-1".to_string()][..]));
    for bad in [json!({"task": "x"}), json!({"title": "T"}), json!({"title": "T", "task": "x", "depends_on": ["zz"]}), json!({"title": "T", "task": "x", "repo": "manca"})] {
        assert!(boards::task_create(&st, &h, "ws-1", &bad).is_err(), "{bad}");
    }
    assert!(boards::task_create(&st, &h, "ws-9", &json!({"title": "T", "task": "x"})).is_err());
    // L'archiviato e' congelato.
    *h.archived.lock().unwrap() = true;
    assert!(boards::task_create(&st, &h, "ws-1", &json!({"title": "T", "task": "x"})).unwrap_err().contains("archiviato"));
}

#[test]
fn update_move_and_to_top_through_the_ui_commands() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[]), spec("c", &[])]);
    let order = |v: &BoardView| v.tasks.iter().map(|t| t.task.id.clone()).collect::<Vec<_>>().join(",");
    assert_eq!(order(&boards::task_move(&st, &h, "ws-1", "c", Some("a")).unwrap()), "c,a,b");
    assert_eq!(order(&boards::task_move(&st, &h, "ws-1", "c", None).unwrap()), "a,b,c");
    assert_eq!(order(&boards::task_action(&st, &h, "ws-1", "c", "to_top").unwrap()), "c,a,b");
    assert!(boards::task_move(&st, &h, "ws-1", "zz", None).is_err());
    let v = boards::task_update(&st, &h, "ws-1", "b", &json!({"title": "Nuovo", "task": "altro", "depends_on": ["a"]})).unwrap();
    let b = v.tasks.iter().find(|t| t.task.id == "b").unwrap();
    assert_eq!((b.task.title.as_str(), b.task.task.as_str(), b.waiting_on.as_slice()), ("Nuovo", "altro", &["a".to_string()][..]));
    assert!(boards::task_update(&st, &h, "ws-1", "a", &json!({"depends_on": ["b"]})).unwrap_err().contains("cycle"));
    assert!(boards::task_update(&st, &h, "ws-1", "a", &json!({"repo": "manca"})).unwrap_err().contains("does not exist"));
    assert!(boards::task_update(&st, &h, "ws-1", "a", &json!({"unknown": 1})).is_err());
    // Un task in corso non si modifica ne' si riordina.
    boards::board_set(&st, &h, "ws-1", None, Some(false)).unwrap();
    let running = view(&st, &h).tasks.iter().find(|t| t.task.status == TaskStatus::Running).unwrap().task.id.clone();
    assert!(boards::task_update(&st, &h, "ws-1", &running, &json!({"title": "x"})).unwrap_err().contains("only queued or failed"));
    assert!(boards::task_move(&st, &h, "ws-1", &running, None).is_err());
    assert!(boards::task_action(&st, &h, "ws-1", &running, "to_top").is_err());
    assert!(boards::task_action(&st, &h, "ws-1", &running, "retry").unwrap_err().contains("only failed"));
}

#[test]
fn intake_tools_inspect_and_fix_the_queue() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"]), spec("c", &["a"])]);
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    let list = call(&st, &h, INTAKE, "list_tasks", json!({})).unwrap();
    assert_eq!(list["counts"], json!({"queued": 1, "running": 1, "done": 1, "failed": 0}));
    assert_eq!(list["tasks"].as_array().unwrap().len(), 3);
    let only = call(&st, &h, INTAKE, "list_tasks", json!({"status": ["done"]})).unwrap();
    assert_eq!(only["tasks"][0]["id"], "a");
    assert_eq!(only["tasks"][0]["summary"], "fatto");
    assert!(call(&st, &h, INTAKE, "list_tasks", json!({"status": ["bogus"]})).unwrap_err().contains("invalid status"));
    let d = call(&st, &h, INTAKE, "get_task_details", json!({"id": "a"})).unwrap();
    assert_eq!(d["task"]["task"], "do a");
    assert_eq!(d["task"]["report"]["summary"], "fatto");
    assert!(call(&st, &h, INTAKE, "get_task_details", json!({"id": "zz"})).unwrap_err().contains("task not found"));
    // update_task: senza testo nella risposta; i running non si modificano.
    let u = call(&st, &h, INTAKE, "update_task", json!({"id": "c", "title": "Nuovo", "model": "opus"})).unwrap();
    assert_eq!((u["task"]["title"].as_str(), u["task"]["model"].as_str()), (Some("Nuovo"), Some("opus")));
    assert!(u["task"].get("task").is_none());
    assert!(call(&st, &h, INTAKE, "update_task", json!({"id": "b", "title": "x"})).unwrap_err().contains("only queued or failed"));
    assert!(call(&st, &h, INTAKE, "update_task", json!({"title": "x"})).unwrap_err().contains("id is missing"));
    // delete_task: queued senza dipendenti.
    let del = call(&st, &h, INTAKE, "delete_task", json!({"id": "c"})).unwrap();
    assert_eq!(del, json!({"deleted": "c"}));
    assert!(call(&st, &h, INTAKE, "delete_task", json!({"id": "b"})).unwrap_err().contains("only queued or failed"));
    assert!(call(&st, &h, INTAKE, "delete_task", json!({"id": "c"})).is_err());
}

// --- persistenza e pubblicazione ---

#[test]
fn board_is_published_in_order_and_saved_with_the_latest_state() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[])]);
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.task("a").unwrap().status, TaskStatus::Done);
    let ev = h.board_events.lock().unwrap().clone();
    assert!(ev.len() >= 3);
    assert!(ev.windows(2).all(|p| p[0].rev < p[1].rev), "rev crescente");
    assert_eq!(ev.last().unwrap().tasks[0].task.status, TaskStatus::Done);
    assert_eq!(ev.last().unwrap().workspace_id, "ws-1");
    assert_eq!(saved.rev, st.inner.lock().unwrap().boards["ws-1"].rev);
    // Ogni vista porta la consegna del workspace.
    assert_eq!(ev.last().unwrap().delivery.mode, DeliveryMode::Commit);
    // La board si ricarica dal file com'e' (i running diventano failed, qui non ce ne sono).
    let back = Board::from_json(&serde_json::to_string(&saved).unwrap(), 1).unwrap();
    assert_eq!(back.tasks, saved.tasks);
}

#[test]
fn removing_a_workspace_deletes_its_board_with_a_tombstone() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![spec("a", &[])]);
    st.remove_board(&h, "ws-1");
    assert_eq!(h.deleted.lock().unwrap().as_slice(), ["ws-1"]);
    assert!(h.saved.lock().unwrap().get("ws-1").is_none());
    let last = h.board_events.lock().unwrap().last().unwrap().clone();
    assert!(last.tasks.is_empty());
    assert!(last.rev > view(&st, &h).rev || view(&st, &h).tasks.is_empty());
    // Un salvataggio in ritardo non resuscita il file.
    st.flush_pending_saves(&h);
    assert!(h.saved.lock().unwrap().get("ws-1").is_none());
}

#[test]
fn saves_are_coalesced_to_one_per_second() {
    let mut gate = state::SaveGate::default();
    let b = Board::new("ws-1");
    assert_eq!(gate.offer(&b, 10_000), state::SaveDecision::Now);
    assert_eq!(gate.offer(&b, 10_100), state::SaveDecision::Arm(Duration::from_millis(900)));
    assert_eq!(gate.offer(&b, 10_200), state::SaveDecision::Queued);
    assert!(gate.take("ws-1", 11_000).is_some());
    assert!(gate.take("ws-1", 11_001).is_none());
    assert_eq!(gate.offer(&b, 12_500), state::SaveDecision::Now);
    gate.offer(&b, 12_600);
    assert_eq!(gate.take_all().len(), 1);
    gate.forget("ws-1");
    assert_eq!(gate.offer(&b, 12_700), state::SaveDecision::Now);
}

// --- guardiano, archiviazione, riavvio ---

#[test]
fn watchdog_fails_a_start_that_never_finished() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[])]);
    {
        // Simula il thread di avvio bloccato: running, nessuna sessione.
        let mut reg = st.inner.lock().unwrap();
        let t = reg.boards.get_mut("ws-1").unwrap().task_mut("a").unwrap();
        t.session = None;
        t.started_at = Some(1_000);
    }
    st.run(&h, |r| r.watchdog(1_000 + 4 * 60_000));
    assert_eq!(status_of(&st, &h, "a"), "running", "entro i 5 minuti");
    st.run(&h, |r| r.watchdog(1_000 + 5 * 60_000 + 1));
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("did not start in time"));
    assert_eq!(h.notices().len(), 1);
    // Un passo di setup in corso allunga il tempo: conta dall'ultimo battito.
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[])]);
    {
        let mut reg = st.inner.lock().unwrap();
        let t = reg.boards.get_mut("ws-1").unwrap().task_mut("a").unwrap();
        t.session = None;
        t.started_at = Some(1_000);
        t.preparing = Some("setup 1/1: npm ci".into());
        t.preparing_since = Some(1_000);
    }
    st.run(&h, |r| r.watchdog(1_000 + 6 * 60_000));
    assert_eq!(status_of(&st, &h, "a"), "running");
    st.run(&h, |r| r.watchdog(1_000 + 13 * 60_000));
    assert_eq!(status_of(&st, &h, "a"), "failed");
}

#[test]
fn archiving_requeues_running_tasks_and_freezes_the_scheduler() {
    let (st, h) = pr_parallel();
    queue(&st, &h, vec![spec("a", &[]), spec("b", &["a"])]);
    let w = session_of(&st, &h, "a");
    *h.archived.lock().unwrap() = true;
    st.set_archived(&h, "ws-1", true);
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["note"].as_str()), (Some("queued"), Some("interrupted: the workspace was archived")));
    assert!(a["session"].is_null());
    // La chiusura del suo tab non lo fa fallire.
    st.run(&h, |r| r.tab_closed(&w, 1));
    assert_eq!(status_of(&st, &h, "a"), "queued");
    assert_eq!(h.spawns().len(), 1, "congelato: nessun nuovo avvio");
    // Riattivato: riparte.
    *h.archived.lock().unwrap() = false;
    st.set_archived(&h, "ws-1", false);
    assert_eq!(status_of(&st, &h, "a"), "running");
    assert_eq!(h.spawns().len(), 2);
}

#[test]
fn loaded_boards_resume_on_the_first_tick_and_running_tasks_are_failed() {
    let (st, h) = pr_parallel();
    let raw = json!({"workspace_id": "ws-1", "tasks": [
        {"id": "old", "title": "Old", "task": "x", "status": "running", "session": "auto-1"},
        {"id": "next", "title": "Next", "task": "x", "status": "queued"},
    ]});
    st.load_board(Board::from_json(&raw.to_string(), 5).unwrap());
    let a = task(&st, &h, "old");
    assert_eq!((a["status"].as_str(), a["note"].as_str()), (Some("failed"), Some("interrupted: the app was restarted")));
    assert_eq!(status_of(&st, &h, "next"), "queued", "nothing starts before the first tick");
    assert!(h.spawns().is_empty());
    st.tick(&h, now_millis());
    assert_eq!(status_of(&st, &h, "next"), "running");
}

#[test]
fn git_commands_have_the_expected_timeouts() {
    assert_eq!(worktrees::GIT_TIMEOUT.as_secs(), 120);
    assert_eq!(boards::START_TIMEOUT_MS, 5 * 60_000);
}

#[test]
fn repo_detection_list_parsing() {
    assert_eq!(worktrees::parse_repo_list("api\n.\n\nweb\napi\n../x\n"), [".", "api", "web"]);
    assert!(worktrees::parse_repo_list("").is_empty());
    assert_eq!(worktrees::join_path("windows", "C:\\p\\", "a/b"), "C:\\p\\a\\b");
    assert_eq!(worktrees::join_path("wsl", "/home/u/", "a/b"), "/home/u/a/b");
    assert_eq!(worktrees::repo_dir(&ws("w", "w", "wsl"), Some("api")), "/home/u/api");
}
