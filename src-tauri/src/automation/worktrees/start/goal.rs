//! Avvio di un task di un goal in una cartella git (docs/GOALS.MD, "Branch del
//! goal"): il branch `goal/<id>` del repository (creato dal primo task e
//! registrato nella board) e il worktree del task, sempre, da quel branch. La
//! copia di lavoro dell'utente non si tocca e il branch del goal non si mette
//! mai in checkout. Gira fuori dal lock, su un thread dell'host.

use super::{prepare_and_setup, ref_exists, remote_or_local, resolve_base, task_branch, Placement, WorktreeStart};
use crate::automation::worktrees::{git, StartJob};
use crate::automation::*;
use crate::board::{goal_branch_name, repo_key, GoalBranch};
use crate::workspaces::DeliveryMode;

/// Dove lavora un task di un goal: il suo worktree, da `goal/<id>`.
pub(super) fn place<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    goal_id: &str,
) -> Result<Placement, String> {
    let (ws, task) = (&job.ws, &job.task);
    let name = goal_branch_name(goal_id);
    // Retry con il worktree ancora li': si riusa com'e', senza toccare il branch del goal.
    let reusable = task.worktree.as_ref().is_some_and(|wt| host.dir_exists(ws, &wt.path));
    let (base, warnings) = if reusable {
        let base = match &job.goal_branch {
            Some(recorded) => recorded.base.clone(),
            None => resolve_base(host, ws, dir, task.repo.as_deref())?,
        };
        (base, Vec::new())
    } else {
        let (goal_branch, warnings) = ensure_goal_branch(st, host, job, dir, goal_id)?;
        (goal_branch.base, warnings)
    };
    let (wt, more) = prepare_and_setup(st, host, job, WorktreeStart::GoalBranch(&name))?;
    Ok(Placement {
        worktree: Some(wt),
        warnings: [warnings, more].concat(),
        branch: Some(task_branch(&task.id)),
        base: Some(base),
        goal_branch: Some(name),
        ..Placement::default()
    })
}

/// Il branch del goal nel repository del task: quello registrato (che deve
/// esistere ancora: non si ricrea in silenzio) o, al primo avvio, quello creato
/// adesso e registrato nella board. Ritorna anche gli avvisi del fetch.
fn ensure_goal_branch<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    goal_id: &str,
) -> Result<(GoalBranch, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let name = goal_branch_name(goal_id);
    let exists = ref_exists(host, ws, dir, &format!("refs/heads/{name}"));
    if let Some(recorded) = &job.goal_branch {
        return if exists {
            Ok((recorded.clone(), Vec::new()))
        } else {
            Err(format!(
                "the goal branch {name} no longer exists in {dir}: the app does not recreate it silently. \
                 Restore it (git branch {name} <commit>) and retry the task"
            ))
        };
    }
    let base = resolve_base(host, ws, dir, task.repo.as_deref())?;
    let mut warnings = Vec::new();
    if !exists {
        let from = start_ref(st, host, job, dir, &base, &mut warnings);
        // Due task dello stesso goal possono arrivare insieme: se il branch
        // c'e' ora (lo ha creato l'altro), si riusa.
        if let Err(e) = git(host, ws, dir, &["branch", "--no-track", &name, &from]) {
            if !ref_exists(host, ws, dir, &format!("refs/heads/{name}")) {
                return Err(e);
            }
        }
    }
    let key = repo_key(task.repo.as_deref());
    let entry = GoalBranch { branch: name, base };
    // Se un altro avvio ha registrato prima, vale la sua base.
    let stored = st
        .run(host, |r| r.record_goal_branch(&ws.id, goal_id, &key, entry, now_millis()))
        .ok_or_else(|| format!("goal {goal_id} no longer exists"))?;
    Ok((stored, warnings))
}

/// Da dove nasce `goal/<id>`: `origin/<base>` dopo il fetch in `pr`, altrimenti
/// il `<base>` locale.
fn start_ref<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    base: &str,
    warnings: &mut Vec<String>,
) -> String {
    let ws = &job.ws;
    if ws.delivery.mode == DeliveryMode::Pr {
        // Un fetch alla volta: la cache evita di ripeterli per ogni task.
        let mut cache = lock(&st.fetches);
        remote_or_local(host, ws, dir, base, &mut cache, warnings)
    } else {
        base.to_string()
    }
}
