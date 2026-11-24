//! Riallineamento delle worktree con le board (host finto): task `done` la cui
//! pulizia si e' persa, worktree orfane, pulizia alla rimozione del workspace,
//! cadenza del giro del timer.

use super::*;
use crate::automation::worktrees::{CleanupTarget, ReconcileReport};
use crate::board::WorktreeInfo;

const WORKTREE_A: &str = "C:\\p\\.worktrees\\a";

fn removals(h: &FakeHost) -> Vec<String> {
    h.git_log().into_iter().filter(|l| l.contains("worktree remove")).collect()
}

fn branch_deletions(h: &FakeHost) -> Vec<String> {
    h.git_log().into_iter().filter(|l| l.contains("branch -")).collect()
}

/// Output di `git worktree list --porcelain` per il repository `C:\p`.
fn porcelain(entries: &[(&str, Option<&str>)]) -> String {
    let mut out = "worktree C:/p\nHEAD 1111111\nbranch refs/heads/main\n\n".to_string();
    for (dir, branch) in entries {
        out.push_str(&format!("worktree C:/p/.worktrees/{dir}\nHEAD 2222222\n"));
        match branch {
            Some(b) => out.push_str(&format!("branch refs/heads/{b}\n\n")),
            None => out.push_str("detached\n\n"),
        }
    }
    out
}

/// Host con la root `C:\p` come unico repository e le worktree elencate da git.
fn with_listed(mode: DeliveryMode, entries: &[(&str, Option<&str>)]) -> FakeHost {
    let h = FakeHost::with_delivery(mode, true);
    *h.repos.lock().unwrap() = vec![".".to_string()];
    h.git_rule("worktree list --porcelain", true, &porcelain(entries), "");
    h
}

fn orphan_exists(h: &FakeHost, id: &str) {
    h.dirs.lock().unwrap().push(format!("C:/p/.worktrees/{id}"));
}

/// Il task `a` completato dal worker e l'app "caduta" prima di chiudere il tab:
/// la board salvata, ricaricata in uno stato nuovo senza worker.
fn restarted_after_completion(mode: DeliveryMode) -> (AutomationState, FakeHost) {
    let (st, h, w) = running(mode, true);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    finish(&st, &h, &w, "completed");
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().expect("board salvata");
    let restarted = AutomationState::default();
    restarted.load_board(saved);
    (restarted, h)
}

#[test]
fn a_done_task_whose_cleanup_was_lost_is_cleaned_after_a_restart() {
    let (st, h) = restarted_after_completion(DeliveryMode::Pr);
    assert!(removals(&h).is_empty(), "il tab non e' stato chiuso: nulla ripulito finora");
    let report = st.reconcile_worktrees(&h);
    assert_eq!(report.queued, ["ws-1/a"]);
    assert_eq!(removals(&h), [format!("C:\\p|worktree remove {WORKTREE_A}")]);
    assert_eq!(branch_deletions(&h), ["C:\\p|branch -D task/a"], "consegna confermata: -D come alla chiusura del tab");
    assert!(task(&st, &h, "a")["worktree"].is_null());
}

#[test]
fn a_done_task_with_its_tab_still_open_is_left_to_the_tab_close() {
    let (st, h, w) = running(DeliveryMode::LocalMerge, true);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    finish(&st, &h, &w, "completed");
    assert!(st.reconcile_worktrees(&h).queued.is_empty());
    assert!(removals(&h).is_empty());
    tab_closed(&st, &h, &w);
    assert_eq!(removals(&h).len(), 1);
    // La pulizia e' fatta: il giro dopo non ha piu' nulla da fare.
    assert!(st.reconcile_worktrees(&h).queued.is_empty());
    assert_eq!(removals(&h).len(), 1);
}

#[test]
fn running_failed_and_queued_tasks_keep_their_worktree() {
    let (st, h, w) = running(DeliveryMode::Pr, true);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    assert!(st.reconcile_worktrees(&h).queued.is_empty());
    finish(&st, &h, &w, "failed");
    tab_closed(&st, &h, &w);
    assert!(st.reconcile_worktrees(&h).queued.is_empty());
    assert_eq!(task(&st, &h, "a")["status"], "failed");
    assert!(task(&st, &h, "a")["worktree"].is_object());
    assert!(removals(&h).is_empty() && branch_deletions(&h).is_empty());
}

#[test]
fn two_requests_for_the_same_cleanup_remove_the_worktree_once() {
    let (st, h) = restarted_after_completion(DeliveryMode::LocalMerge);
    h.hold_defers();
    st.reconcile_worktrees(&h);
    st.reconcile_worktrees(&h);
    h.run_held();
    assert_eq!(removals(&h).len(), 1, "{:?}", h.git_log());
    assert_eq!(branch_deletions(&h).len(), 1);
}

#[test]
fn an_orphan_worktree_with_no_changes_is_removed_but_its_branch_is_never_forced() {
    let h = with_listed(DeliveryMode::Pr, &[("ghost", Some("task/ghost"))]);
    orphan_exists(&h, "ghost");
    let st = AutomationState::default();
    let ReconcileReport { queued, warnings } = st.reconcile_worktrees(&h);
    assert_eq!(queued, ["ws-1/ghost"]);
    assert!(warnings.iter().any(|w| w.contains("ghost") && w.contains("belongs to no task")), "{warnings:?}");
    assert_eq!(removals(&h), ["C:\\p|worktree remove C:/p/.worktrees/ghost"]);
    // `pr` cancellerebbe con -D un branch mai consegnato: per le orfane si usa -d.
    assert_eq!(branch_deletions(&h), ["C:\\p|branch -d task/ghost"]);
}

#[test]
fn an_orphan_worktree_with_uncommitted_changes_is_kept_and_reported() {
    let h = with_listed(DeliveryMode::LocalMerge, &[("dirty", Some("task/dirty")), ("clean", Some("task/clean"))]);
    h.git_rule("dirty|status --porcelain", true, " M src/lib.rs\n?? notes.txt\n", "");
    orphan_exists(&h, "dirty");
    orphan_exists(&h, "clean");
    let st = AutomationState::default();
    let report = st.reconcile_worktrees(&h);
    assert_eq!(report.queued, ["ws-1/clean"]);
    assert!(
        report.warnings.iter().any(|w| w.contains("dirty") && w.contains("uncommitted changes") && w.contains("kept")),
        "{:?}",
        report.warnings
    );
    assert_eq!(removals(&h), ["C:\\p|worktree remove C:/p/.worktrees/clean"]);
}

#[test]
fn an_orphan_worktree_is_kept_even_with_remove_force_when_it_has_changes() {
    let h = with_listed(DeliveryMode::LocalMerge, &[("dirty", Some("task/dirty"))]);
    h.repo_cfg.lock().unwrap().insert(".".into(), RepoConfig { remove_force: true, ..Default::default() });
    h.git_rule("dirty|status --porcelain", true, " M a\n", "");
    orphan_exists(&h, "dirty");
    assert!(AutomationState::default().reconcile_worktrees(&h).queued.is_empty());
    assert!(removals(&h).is_empty());
}

#[test]
fn an_orphan_whose_state_cannot_be_read_is_kept() {
    let h = with_listed(DeliveryMode::LocalMerge, &[("odd", Some("task/odd"))]);
    h.git_rule("odd|status --porcelain", false, "", "fatal: not a git repository");
    orphan_exists(&h, "odd");
    let report = AutomationState::default().reconcile_worktrees(&h);
    assert!(report.queued.is_empty() && removals(&h).is_empty());
    assert!(
        report.warnings.iter().any(|w| w.contains("odd") && w.contains("state is unknown")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn an_orphan_whose_folder_is_gone_is_pruned_and_its_branch_deleted() {
    let h = FakeHost::with_delivery(DeliveryMode::LocalMerge, true);
    *h.repos.lock().unwrap() = vec![".".to_string()];
    let prunable = "task/gone\nprunable gitdir file points to non-existent location\n";
    let listed = porcelain(&[("gone", Some("task/gone"))]).replace("task/gone\n", prunable);
    h.git_rule("worktree list --porcelain", true, &listed, "");
    let report = AutomationState::default().reconcile_worktrees(&h);
    assert_eq!(report.queued, ["ws-1/gone"]);
    let git = h.git_log();
    assert!(!git.iter().any(|l| l.contains("status --porcelain")), "niente da controllare: {git:?}");
    assert!(git.contains(&"C:\\p|worktree prune".to_string()), "{git:?}");
    assert!(removals(&h).is_empty());
    assert_eq!(branch_deletions(&h), ["C:\\p|branch -d task/gone"]);
}

#[test]
fn worktrees_of_known_tasks_and_foreign_worktrees_are_not_orphans() {
    let h = with_listed(
        DeliveryMode::LocalMerge,
        &[("a", Some("task/a")), ("mine", Some("feature/x")), ("detached", None), ("other", Some("task/a"))],
    );
    let st = AutomationState::default();
    queue(&st, &h, vec![spec("a", &[])]);
    let report = st.reconcile_worktrees(&h);
    assert!(report.queued.is_empty() && report.warnings.is_empty(), "{report:?}");
    assert!(!h.git_log().iter().any(|l| l.contains("status --porcelain")), "{:?}", h.git_log());
}

#[test]
fn an_orphan_is_not_queued_if_a_task_with_its_id_appears_in_the_meantime() {
    let mut reg = Registry::default();
    let target = CleanupTarget {
        ws_id: "ws-1".into(),
        task_id: "x".into(),
        repo: None,
        wt: WorktreeInfo {
            path: "C:/p/.worktrees/x".into(),
            branch: "task/x".into(),
            base: String::new(),
            setup_done: true,
        },
        workspace: None,
        keep_unmerged: true,
    };
    assert!(reg.queue_orphan_cleanup(target.clone()));
    assert_eq!(reg.take_effects().len(), 1);
    reg.create_task(
        "ws-1",
        crate::board::TaskSpec { id: "x".into(), title: "X".into(), task: "t".into(), ..Default::default() },
        crate::board::SpecContext::default(),
        1,
    )
    .unwrap();
    assert!(!reg.queue_orphan_cleanup(target));
    assert!(reg.take_effects().is_empty());
}

#[test]
fn archived_workspaces_and_modes_without_worktrees_are_not_scanned() {
    let h = with_listed(DeliveryMode::LocalMerge, &[("ghost", Some("task/ghost"))]);
    *h.archived.lock().unwrap() = true;
    let st = AutomationState::default();
    assert_eq!(st.reconcile_worktrees(&h), ReconcileReport::default());
    assert!(h.git_log().is_empty());

    let h = with_listed(DeliveryMode::Commit, &[("ghost", Some("task/ghost"))]);
    assert_eq!(st.reconcile_worktrees(&h), ReconcileReport::default());
    assert!(h.git_log().is_empty());
}

#[test]
fn git_and_repository_listing_failures_are_reported_and_clean_nothing() {
    let h = with_listed(DeliveryMode::LocalMerge, &[("ghost", Some("task/ghost"))]);
    h.git_rules.lock().unwrap().clear();
    h.git_rule("worktree list --porcelain", false, "", "fatal: not a git repository");
    let report = AutomationState::default().reconcile_worktrees(&h);
    assert!(report.queued.is_empty() && removals(&h).is_empty());
    assert!(report.warnings.iter().any(|w| w.contains("worktrees not listed")), "{:?}", report.warnings);

    *h.repos_err.lock().unwrap() = Some("wsl down".to_string());
    let report = AutomationState::default().reconcile_worktrees(&h);
    assert!(
        report.warnings.iter().any(|w| w.contains("repositories not listed") && w.contains("wsl down")),
        "{report:?}"
    );
}

#[test]
fn repositories_other_than_the_root_are_scanned_in_their_own_folder() {
    let h = with_listed(DeliveryMode::LocalMerge, &[]);
    *h.repos.lock().unwrap() = vec![".".to_string(), "api".to_string()];
    AutomationState::default().reconcile_worktrees(&h);
    let listed: Vec<String> = h.git_log().into_iter().filter(|l| l.contains("worktree list")).collect();
    assert_eq!(listed, ["C:\\p|worktree list --porcelain", "C:\\p\\api|worktree list --porcelain"]);
}

#[test]
fn the_timer_reconciles_at_startup_then_every_five_minutes() {
    let h = with_listed(DeliveryMode::LocalMerge, &[]);
    let st = AutomationState::default();
    let sweeps = |h: &FakeHost| h.git_log().iter().filter(|l| l.contains("worktree list")).count();
    st.tick(&h, 1_000);
    assert_eq!(sweeps(&h), 1, "primo giro dopo l'avvio");
    st.tick(&h, 31_000);
    st.tick(&h, 4 * 60_000);
    assert_eq!(sweeps(&h), 1);
    st.tick(&h, 1_000 + 5 * 60_000);
    assert_eq!(sweeps(&h), 2);
}

#[test]
fn the_startup_sweep_cleans_what_the_crash_left_behind() {
    let (st, h) = restarted_after_completion(DeliveryMode::LocalMerge);
    st.tick(&h, 1_000);
    assert_eq!(removals(&h), [format!("C:\\p|worktree remove {WORKTREE_A}")]);
}

#[test]
fn removing_a_workspace_cleans_the_worktrees_of_its_tasks() {
    let (st, h, _w) = running(DeliveryMode::Pr, true);
    h.dirs.lock().unwrap().push(WORKTREE_A.to_string());
    // Il workspace non e' piu' nello store: la pulizia usa la copia che riceve.
    let mut gone = h.ws_for("ws-1").unwrap();
    gone.path = "C:\\snapshot".to_string();
    h.hold_defers();
    st.remove_board(&h, &gone);
    assert!(removals(&h).is_empty(), "la pulizia aspetta il rilascio delle cartelle");
    h.run_held();
    assert_eq!(removals(&h), [format!("C:\\snapshot|worktree remove {WORKTREE_A}")]);
    assert_eq!(branch_deletions(&h), ["C:\\snapshot|branch -D task/a"]);
    assert!(h.deleted.lock().unwrap().contains(&"ws-1".to_string()));
}

#[test]
fn removing_a_workspace_without_worktrees_cleans_nothing() {
    let h = FakeHost::with_delivery(DeliveryMode::Commit, false);
    let st = AutomationState::default();
    queue(&st, &h, vec![spec("a", &[])]);
    st.remove_board(&h, &h.ws_for("ws-1").unwrap());
    assert!(h.git_log().iter().all(|l| !l.contains("worktree remove") && !l.contains("branch -")), "{:?}", h.git_log());
}

#[test]
fn a_leftover_worktree_on_a_goal_branch_is_removed_but_its_branch_is_never_deleted() {
    let h = FakeHost::with_delivery(DeliveryMode::Pr, true);
    *h.repos.lock().unwrap() = vec![".".to_string()];
    h.dirs.lock().unwrap().push("C:/p/.worktrees/g".to_string());
    let target = CleanupTarget {
        ws_id: "ws-1".into(),
        task_id: "g".into(),
        repo: None,
        wt: WorktreeInfo {
            path: "C:/p/.worktrees/g".into(),
            branch: "goal/export".into(),
            base: "main".into(),
            setup_done: true,
        },
        workspace: None,
        keep_unmerged: false,
    };
    let st = AutomationState::default();
    assert!(st.run(&h, |r| r.queue_orphan_cleanup(target)));
    assert_eq!(removals(&h), ["C:\\p|worktree remove C:/p/.worktrees/g"], "{:?}", h.git_log());
    assert!(branch_deletions(&h).is_empty(), "the goal branch may hold undelivered work: {:?}", h.git_log());
}
