//! L'esecuzione in background di un'esecuzione E2E: impronta del codice,
//! cancellazione del vecchio report, comando, lettura del JUnit, verdetto,
//! consegna dell'esito. Gira su un thread dell'host (`Host::defer`), mai sotto
//! il lock del registro.

use super::runs::{scope_key, ClaimError, Owner, RunGuard};
use super::verdict::evaluate;
use super::{fingerprint, E2eEvent};
use crate::automation::hooks::{hook_env, HookVars};
use crate::automation::worktrees::join_path;
use crate::automation::*;
use crate::board::{E2eOutcome, E2eRun};
use crate::workspaces::{E2eConfig, RepoConfig};
use std::time::Instant;

/// Variabile d'ambiente con la porta riservata all'esecuzione: e' un contratto
/// con i preset dei progetti, il nome non cambia.
pub(super) const PORT_VAR: &str = "CLI_COMPANION_E2E_PORT";
/// Dimensione massima del report JUnit letto.
const JUNIT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const SECS_PER_MINUTE: u64 = 60;

/// Dove e per chi gira un'esecuzione.
pub(super) struct RunTarget {
    pub ws: Workspace,
    /// Cartella di lavoro (worktree o checkout), nel formato del workspace.
    pub dir: String,
    pub repo_dir: String,
    pub repo_cfg: RepoConfig,
    /// Vuoto per un'esecuzione manuale.
    pub task_id: String,
    pub worktree: Option<WorktreeInfo>,
    pub owner: Owner,
}

struct E2eJob {
    target: RunTarget,
    e2e: E2eConfig,
    started: E2eRun,
    guard: RunGuard,
}

impl E2eJob {
    fn junit_path(&self) -> String {
        join_path(&self.target.ws.kind, &self.target.dir, &self.e2e.junit)
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(u64::from(self.e2e.timeout_min) * SECS_PER_MINUTE)
    }

    /// Variabili dei comandi del repository (`TASK_ID`, `WORKTREE`, ...) piu' la porta.
    fn env(&self) -> Vec<(String, String)> {
        let t = &self.target;
        let (branch, base) = t.worktree.as_ref().map_or(("", ""), |w| (w.branch.as_str(), w.base.as_str()));
        let vars = HookVars { repo_dir: &t.repo_dir, worktree: &t.dir, task_id: &t.task_id, branch, base };
        let mut env = hook_env(&t.repo_cfg, &vars);
        env.push((PORT_VAR.to_string(), self.guard.port.to_string()));
        env
    }
}

/// Prende la cartella, registra l'esecuzione con `register` (se fallisce la
/// cartella torna libera) e accoda il job. Ritorna l'esecuzione `running`.
pub(super) fn launch<H: Host>(
    st: &AutomationState,
    host: &H,
    target: RunTarget,
    e2e: E2eConfig,
    register: impl FnOnce(&E2eRun) -> Result<(), String>,
    claim_error: impl FnOnce(ClaimError) -> String,
) -> Result<E2eRun, String> {
    let now = now_millis();
    let folder = scope_key(&target.ws.id, &target.dir);
    let guard = st.e2e.claim(folder, target.owner.clone(), now).map_err(claim_error)?;
    let started = E2eRun::started(guard.run_id.clone(), e2e.junit.clone(), now);
    register(&started)?;
    let job = E2eJob { target, e2e, started: started.clone(), guard };
    let (state, h) = (st.clone(), host.clone());
    // Il comando dura minuti: mai sul thread del chiamante.
    host.defer(Duration::ZERO, Box::new(move || run_job(&state, &h, job)));
    Ok(started)
}

fn run_job<H: Host>(st: &AutomationState, host: &H, job: E2eJob) {
    let begun = Instant::now();
    let code = fingerprint(host, &job.target.ws, &job.target.dir).ok();
    let outcome = execute(host, &job);
    let run = job.started.clone().finished(outcome, code, now_millis(), begun.elapsed().as_millis() as u64);
    deliver(st, host, &job.target, run);
    // `job.guard` si rilascia qui: l'esito e' gia' sul task quando chi aspetta si sveglia.
}

fn execute<H: Host>(host: &H, job: &E2eJob) -> E2eOutcome {
    if job.guard.cancel.is_cancelled() {
        return E2eOutcome::error("cancelled", None);
    }
    let (ws, dir) = (&job.target.ws, &job.target.dir);
    let junit = job.junit_path();
    // Un report vecchio non deve passare per quello di questa esecuzione.
    if let Err(e) = host.remove_file(ws, &junit) {
        return E2eOutcome::error(&format!("could not delete the old JUnit report: {e}"), None);
    }
    let run = host.run_hook(ws, dir, &job.e2e.command, &job.env(), job.timeout(), Some(&job.guard.cancel));
    evaluate(run, || host.read_text_file(ws, &junit, JUNIT_MAX_BYTES), &job.e2e.junit)
}

/// L'esito va al task, o in memoria con l'evento per l'esecuzione manuale.
fn deliver<H: Host>(st: &AutomationState, host: &H, target: &RunTarget, run: E2eRun) {
    match &target.owner {
        Owner::Task { ws, task } => st.run(host, |r| r.finish_e2e(ws, task, run)),
        Owner::Repo { ws, repo } => {
            st.e2e.set_last_manual(ws, repo, run.clone());
            host.emit_e2e(&E2eEvent { workspace_id: ws.clone(), repo: repo.clone(), run });
        }
    }
}
