//! Riallineamento con git vero in una cartella temporanea: si crea il worktree
//! di un task con il flusso reale (scheduler, `worktree add`), si simula il
//! crash prima della pulizia e si verifica che il riallineamento ripulisca.
//! Senza git sulla macchina i test escono subito (nessun falso rosso).

use super::*;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Repository temporaneo con un commit su `main`; la cartella sparisce a fine test.
pub(super) struct TempRepo {
    pub(super) root: PathBuf,
}

impl TempRepo {
    /// `None` se git non c'e' (il test non ha nulla da provare).
    pub(super) fn create(name: &str) -> Option<TempRepo> {
        if !Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success()) {
            eprintln!("git is not installed: test skipped");
            return None;
        }
        let root = std::env::temp_dir().join(format!("cc-reconcile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("cartella temporanea");
        let repo = TempRepo { root };
        repo.git(&["init", "--quiet", "-b", "main"]);
        std::fs::write(repo.root.join("README.md"), "hello\n").expect("file iniziale");
        repo.git(&["add", "."]);
        commit_in(&repo.root, "initial");
        Some(repo)
    }

    pub(super) fn worktree(&self, id: &str) -> PathBuf {
        self.root.join(".worktrees").join(id)
    }

    pub(super) fn git(&self, args: &[&str]) -> String {
        git_in(&self.root, args)
    }

    fn branches(&self) -> Vec<String> {
        self.git(&["branch", "--list", "task/*", "--format=%(refname:short)"]).lines().map(str::to_string).collect()
    }

    /// Quante worktree conosce git (il checkout principale compreso).
    fn worktree_count(&self) -> usize {
        self.git(&["worktree", "list", "--porcelain"]).lines().filter(|l| l.starts_with("worktree ")).count()
    }

    /// Host con git vero e il workspace "ws-1" sulla cartella temporanea.
    pub(super) fn host(&self, mode: DeliveryMode) -> FakeHost {
        let h = FakeHost::with_delivery(mode, true);
        let mut workspace = ws("ws-1", "Proj", "windows");
        workspace.path = self.root.display().to_string();
        h.store_ws1(workspace);
        h.set_delivery(mode, true);
        *h.repos.lock().unwrap() = vec![".".to_string()];
        h.use_real_git();
        h
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub(super) fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().expect("git parte");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub(super) fn commit_in(dir: &Path, message: &str) {
    let identity = ["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"];
    let args: Vec<&str> = identity.into_iter().chain(["commit", "--quiet", "--allow-empty", "-m", message]).collect();
    git_in(dir, &args);
}

/// Task `a` avviato dallo scheduler con git vero: worktree e branch esistono.
fn started(repo: &TempRepo, mode: DeliveryMode) -> (AutomationState, FakeHost, String) {
    let (st, h) = (AutomationState::default(), repo.host(mode));
    queue(&st, &h, vec![spec("a", &[])]);
    let worker = session_of(&st, &h, "a");
    assert!(repo.worktree("a").is_dir(), "il flusso reale crea il worktree");
    assert_eq!(repo.branches(), ["task/a"]);
    (st, h, worker)
}

/// L'app riparte dalla board salvata, senza worker ne' timer in sospeso.
fn restarted(h: &FakeHost) -> AutomationState {
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().expect("board salvata");
    let st = AutomationState::default();
    st.load_board(saved);
    st
}

/// Il task `a` (fallito) viene eliminato e l'app si chiude (salva la board)
/// prima che la pulizia in coda parta; poi riparte dalla board salvata, che non
/// ha piu' il task.
fn deleted_then_restarted(st: &AutomationState, h: &FakeHost) -> AutomationState {
    h.hold_defers();
    st.run(h, |r| r.delete_task("ws-1", "a", 1)).unwrap();
    st.flush_pending_saves(h);
    *h.held.lock().unwrap() = None;
    let after_restart = restarted(h);
    assert!(after_restart.view(h, "ws-1").unwrap().tasks.is_empty());
    after_restart
}

#[test]
fn a_done_task_left_behind_by_a_crash_is_cleaned_at_the_next_sweep() {
    let Some(repo) = TempRepo::create("done") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    finish(&st, &h, &worker, "completed");
    // Crash: il tab non si chiude mai, la pulizia in memoria si perde.
    let after_restart = restarted(&h);
    assert!(repo.worktree("a").is_dir() && repo.branches() == ["task/a"]);

    let report = after_restart.reconcile_worktrees(&h);

    assert_eq!(report.queued, ["ws-1/a"], "{report:?}");
    assert!(!repo.worktree("a").exists(), "worktree rimosso");
    assert!(repo.branches().is_empty(), "branch cancellato");
    assert_eq!(repo.worktree_count(), 1, "resta solo il checkout principale");
    assert!(task(&after_restart, &h, "a")["worktree"].is_null());
}

#[test]
fn a_deleted_task_whose_cleanup_was_lost_is_found_through_git() {
    let Some(repo) = TempRepo::create("deleted") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    finish(&st, &h, &worker, "failed");
    let after_restart = deleted_then_restarted(&st, &h);
    assert!(repo.worktree("a").is_dir());

    let report = after_restart.reconcile_worktrees(&h);

    assert_eq!(report.queued, ["ws-1/a"], "{report:?}");
    assert!(report.warnings.iter().any(|w| w.contains("belongs to no task")), "{:?}", report.warnings);
    assert!(!repo.worktree("a").exists());
    assert!(repo.branches().is_empty());
}

#[test]
fn an_orphan_with_uncommitted_changes_survives_every_sweep() {
    let Some(repo) = TempRepo::create("dirty") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    finish(&st, &h, &worker, "failed");
    std::fs::write(repo.worktree("a").join("wip.txt"), "lavoro non committato").unwrap();
    let after_restart = deleted_then_restarted(&st, &h);

    for _ in 0..2 {
        let report = after_restart.reconcile_worktrees(&h);
        assert!(report.queued.is_empty(), "{report:?}");
        assert!(report.warnings.iter().any(|w| w.contains("uncommitted changes")), "{:?}", report.warnings);
    }
    assert!(repo.worktree("a").join("wip.txt").is_file());
    assert_eq!(repo.branches(), ["task/a"]);
}

#[test]
fn an_orphan_branch_with_unmerged_commits_is_never_forced_even_in_pr_mode() {
    let Some(repo) = TempRepo::create("unmerged") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::Pr);
    finish(&st, &h, &worker, "failed");
    commit_in(&repo.worktree("a"), "work that was never delivered");
    let after_restart = deleted_then_restarted(&st, &h);

    after_restart.reconcile_worktrees(&h);

    assert!(!repo.worktree("a").exists(), "la cartella pulita si rimuove");
    assert_eq!(repo.branches(), ["task/a"], "il branch con commit non uniti resta");
}

#[test]
fn an_unmerged_branch_of_a_done_task_is_kept_once_and_not_retried_after_a_restart() {
    let Some(repo) = TempRepo::create("unmerged-done") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    commit_in(&repo.worktree("a"), "work that was never merged");
    finish(&st, &h, &worker, "completed");
    let after_crash = restarted(&h);

    let first = after_crash.reconcile_worktrees(&h);
    after_crash.flush_pending_saves(&h);
    let after_restart = restarted(&h);
    let second = after_restart.reconcile_worktrees(&h);

    assert_eq!(first.queued, ["ws-1/a"], "{first:?}");
    assert!(!repo.worktree("a").exists(), "la cartella si rimuove");
    assert_eq!(repo.branches(), ["task/a"], "il branch con commit non uniti resta");
    assert!(task(&after_restart, &h, "a")["worktree"].is_null(), "la pulizia e' salvata");
    assert!(second.queued.is_empty() && second.warnings.is_empty(), "nessun nuovo tentativo: {second:?}");
}

#[test]
fn a_worktree_folder_deleted_by_hand_is_pruned_with_its_branch() {
    let Some(repo) = TempRepo::create("gone") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    finish(&st, &h, &worker, "failed");
    std::fs::remove_dir_all(repo.worktree("a")).unwrap();
    let after_restart = deleted_then_restarted(&st, &h);

    let report = after_restart.reconcile_worktrees(&h);

    assert_eq!(report.queued, ["ws-1/a"], "{report:?}");
    assert!(repo.branches().is_empty());
    assert_eq!(repo.worktree_count(), 1, "i metadati del worktree sono stati potati");
}

#[test]
fn foreign_worktrees_and_live_tasks_are_left_alone_by_a_sweep() {
    let Some(repo) = TempRepo::create("foreign") else { return };
    let (st, h, _worker) = started(&repo, DeliveryMode::LocalMerge);
    let elsewhere = repo.root.join("elsewhere");
    repo.git(&["worktree", "add", "--quiet", "-b", "feature/mine", &elsewhere.display().to_string()]);

    let report = st.reconcile_worktrees(&h);

    assert!(report.queued.is_empty() && report.warnings.is_empty(), "{report:?}");
    assert!(repo.worktree("a").is_dir() && elsewhere.is_dir());
}

#[test]
fn removing_a_workspace_removes_the_worktrees_of_its_tasks() {
    let Some(repo) = TempRepo::create("remove-board") else { return };
    let (st, h, worker) = started(&repo, DeliveryMode::LocalMerge);
    finish(&st, &h, &worker, "failed");
    let workspace = h.ws_for("ws-1").unwrap();

    st.remove_board(&h, &workspace);

    assert!(!repo.worktree("a").exists());
    assert!(repo.branches().is_empty());
}
