//! Avvio di un task di consegna (docs/GOALS.MD, "Consegna del goal"): per ogni
//! repository in cui il goal ha un branch si conta quanti commit ha oltre la base
//! e solo quelli con commit sono da consegnare. Ognuno ha un worktree sul branch
//! `goal/<id>` (`git worktree add <path> goal/<id>`, niente branch del task); il
//! setup del repository gira solo nel principale. Se nessun repository ha commit
//! non c'e' nulla da consegnare e il task si chiude senza worker. Gira fuori dal
//! lock, su un thread dell'host.

use super::setup::{setup_cfg, setup_worktree};
use super::{add_worktrees_exclude, ref_exists, remote_or_local, Placement};
use crate::automation::deliver::DeliverTarget;
use crate::automation::worktrees::{git, join_path, repo_dir, repo_of_key, StartJob, WORKTREES_DIR};
use crate::automation::*;
use crate::board::{goal_branch_name, repo_key};
use crate::workspaces::DeliveryMode;

/// Riepilogo del report di un task di consegna chiuso senza worker.
pub(in crate::automation) const NOTHING_TO_DELIVER: &str =
    "Nothing to deliver: the goal branches have no commits beyond their base.";

/// Un repository con commit da consegnare, prima di avere il worktree.
struct Pending {
    key: String,
    dir: String,
    /// Base come la vede git: `origin/<base>` dopo il fetch (`pr`) o il branch locale.
    base_ref: String,
    /// Base come nome semplice.
    base: String,
}

/// Dove lavora il task di consegna: un worktree sul branch del goal per ogni
/// repository con commit da consegnare. `None` = nessuno ne ha.
pub(super) fn place<H: Host>(st: &AutomationState, host: &H, job: &StartJob) -> Result<Option<Placement>, String> {
    let (ws, task) = (&job.ws, &job.task);
    let goal_id = task.goal().ok_or_else(|| format!("the delivery task '{}' belongs to no goal", task.id))?;
    let name = goal_branch_name(goal_id);
    let mut warnings = Vec::new();
    let mut pending: Vec<Pending> = Vec::new();
    for (key, branch) in &job.goal_branches {
        let dir = repo_dir(ws, repo_of_key(key));
        if !ref_exists(host, ws, &dir, &format!("refs/heads/{name}")) {
            return Err(format!(
                "the goal branch {name} no longer exists in {dir}: the app does not recreate it silently. \
                 Restore it (git branch {name} <commit>) and retry the task"
            ));
        }
        let base_ref = if ws.delivery.mode == DeliveryMode::Pr {
            // Un fetch alla volta: la cache evita di ripeterli per ogni task.
            let mut cache = lock(&st.fetches);
            remote_or_local(host, ws, &dir, &branch.base, &mut cache, &mut warnings)
        } else {
            branch.base.clone()
        };
        if commits_ahead(host, ws, &dir, &base_ref, &name)? > 0 {
            pending.push(Pending { key: key.clone(), dir, base_ref, base: branch.base.clone() });
        }
    }
    if pending.is_empty() {
        return Ok(None);
    }
    // Il principale e' il repository del task, se ha commit: li' gira il setup.
    let task_key = repo_key(task.repo.as_deref());
    let main_at = pending.iter().position(|p| p.key == task_key).unwrap_or(0);
    let main = pending.remove(main_at);
    let (info, setup_warnings) = prepare_main(st, host, job, &main, &name)?;
    warnings.extend(setup_warnings);
    let mut targets = vec![target(&main, &info.path, &name)];
    for p in pending {
        let path = worktree_path(ws, &p.dir, &task.id);
        ensure_worktree(host, ws, &p.dir, &path, &name, &mut warnings)?;
        targets.push(target(&p, &path, &name));
    }
    Ok(Some(Placement {
        worktree: Some(info),
        warnings,
        branch: Some(name),
        base: Some(main.base),
        repo_dir: Some(main.dir),
        deliver: targets,
        ..Placement::default()
    }))
}

fn target(p: &Pending, workdir: &str, branch: &str) -> DeliverTarget {
    DeliverTarget {
        key: p.key.clone(),
        repo_dir: p.dir.clone(),
        workdir: workdir.to_string(),
        branch: branch.to_string(),
        base: p.base.clone(),
    }
}

/// `<repo>/.worktrees/<task-id>`: il worktree del task in un repository.
fn worktree_path(ws: &Workspace, dir: &str, task_id: &str) -> String {
    join_path(ws.kind, dir, &format!("{WORKTREES_DIR}/{task_id}"))
}

/// Commit di `branch` oltre `base_ref` (`git rev-list --count <base>..<branch>`).
fn commits_ahead<H: Host>(host: &H, ws: &Workspace, dir: &str, base_ref: &str, branch: &str) -> Result<u64, String> {
    let range = format!("{base_ref}..{branch}");
    let out = git(host, ws, dir, &["rev-list", "--count", &range])?;
    out.parse().map_err(|_| format!("git rev-list --count {range} gave '{out}' in {dir}: not a number"))
}

/// Crea il worktree sul branch del goal se non c'e' gia' (un retry lo riusa
/// com'e'). `true` se e' stato creato adesso.
fn ensure_worktree<H: Host>(
    host: &H,
    ws: &Workspace,
    dir: &str,
    path: &str,
    branch: &str,
    warnings: &mut Vec<String>,
) -> Result<bool, String> {
    if host.dir_exists(ws, path) {
        return Ok(false);
    }
    add_worktrees_exclude(host, ws, dir, warnings);
    let _ = git(host, ws, dir, &["worktree", "prune"]);
    git(host, ws, dir, &["worktree", "add", path, branch])?;
    Ok(true)
}

/// Il worktree principale: creato (o riusato al retry) e con il setup del
/// repository fatto, come per ogni task: registrato appena creato, cosi' un
/// setup fallito lascia traccia e il retry lo rifa. Ritorna anche gli avvisi.
fn prepare_main<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    main: &Pending,
    branch: &str,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let path = worktree_path(ws, &main.dir, &task.id);
    // Retry: il worktree gia' creato e registrato si riusa com'e'.
    if let Some(wt) = task.worktree.as_ref().filter(|wt| wt.path == path && host.dir_exists(ws, &wt.path)) {
        return Ok((wt.clone(), Vec::new()));
    }
    // Il setup e' quello del repository del worktree principale, qualunque sia il `repo` del task.
    let mut main_job = job.clone();
    main_job.task.repo = repo_of_key(&main.key).map(str::to_string);
    let slow = setup_cfg(&main_job).is_some();
    if slow {
        st.run(host, |r| r.touch_preparing(&ws.id, &task.id, job.attempt, Some("worktree"), now_millis(), true));
    }
    let result = create_main(st, host, &main_job, main, &path, branch);
    if slow {
        st.run(host, |r| r.end_preparing(&ws.id, &task.id, job.attempt, now_millis()));
    }
    result
}

fn create_main<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    main: &Pending,
    path: &str,
    branch: &str,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let mut warnings = Vec::new();
    let created = ensure_worktree(host, ws, &main.dir, path, branch, &mut warnings)?;
    let mut info = WorktreeInfo {
        path: path.to_string(),
        branch: branch.to_string(),
        base: main.base_ref.clone(),
        setup_done: !created,
    };
    if created {
        st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, info.clone()));
        warnings.extend(setup_worktree(st, host, job, &main.dir, &info)?);
        info.setup_done = true;
        st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, info.clone()));
    }
    Ok((info, warnings))
}
