//! Test E2E eseguiti dall'app (vedi docs/BOARD.MD, "Test E2E"): il comando
//! del repository lo lancia l'app, non la shell del worker, e l'esito (report
//! JUnit + codice di uscita) lo decide l'app, senza modelli. Un task `work`
//! marcato `e2e` si completa solo con un'esecuzione passata sul codice
//! consegnato (`complete_task`, vedi `board::check_gate`).
//!
//! - `verdict`: esito e impronta del codice (pure);
//! - `runs`: esecuzioni in corso, porte riservate, ultimi esiti manuali;
//! - `job`: l'esecuzione in background;
//! - `tools`: i tool MCP `run_e2e` e `get_e2e_result`;
//! - `manual`: l'esecuzione a mano di un repository, dalla UI;
//! - `report`: il file del report da aprire dalla UI.
//!
//! Qui: il `Registry` (esito sul task, blocco della consegna, annullamento) e
//! il calcolo dell'impronta.

use super::worktrees::git;
use super::*;
use crate::board::{check_gate, CodeFingerprint, E2eRun, GateOutcome};

mod job;
mod junit;
pub(super) mod manual;
pub(super) mod report;
pub(super) mod runs;
pub(super) mod tools;
mod verdict;

/// Payload dell'evento `e2e-update`: l'esecuzione manuale di un repository
/// cambia stato (partita o conclusa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct E2eEvent {
    pub workspace_id: String,
    /// Chiave del repository ("." = la root).
    pub repo: String,
    pub run: E2eRun,
}

/// Dati per il blocco della consegna, calcolati fuori dal lock.
#[derive(Debug, Clone)]
pub struct E2eGateInput {
    /// Il repository ha ancora gli E2E configurati.
    pub repo_has_e2e: bool,
    /// Impronta del codice di adesso.
    pub current: Result<CodeFingerprint, String>,
}

/// Il task di un worker visto dagli E2E.
pub(super) struct E2eTarget {
    pub ws_id: String,
    pub task_id: String,
    pub repo: Option<String>,
    /// Cartella del worker (worktree o checkout), nel formato del workspace.
    pub workdir: String,
    pub repo_dir: String,
    pub worktree: Option<WorktreeInfo>,
    /// Task `work` marcato E2E: la consegna e' bloccata dal controllo.
    pub gated: bool,
    pub last_run: Option<E2eRun>,
}

/// Impronta del codice in `dir`: commit, albero e modifiche non committate
/// dei file tracciati.
pub(super) fn fingerprint<H: Host>(host: &H, ws: &Workspace, dir: &str) -> Result<CodeFingerprint, String> {
    let revs = git(host, ws, dir, &["rev-parse", "HEAD", "HEAD^{tree}"])?;
    let changes = git(host, ws, dir, &["status", "--porcelain", "--untracked-files=no"])?;
    verdict::parse_fingerprint(&revs, &changes)
}

/// Dati del blocco per `complete_task`: servono solo a chi completa un task
/// `work` marcato E2E. `None` = non servono (o il workspace non c'e' piu': il
/// registro rifiuta, non potendo verificare).
pub(super) fn gate_input<H: Host>(st: &AutomationState, host: &H, session: &str, status: &str) -> Option<E2eGateInput> {
    if status != "completed" {
        return None;
    }
    let target = st.e2e_target(session).filter(|t| t.gated)?;
    let ws = host.workspace(&target.ws_id)?;
    let repo_has_e2e = ws.repos.get(&board::repo_key(target.repo.as_deref())).is_some_and(|c| c.e2e.is_some());
    Some(E2eGateInput { repo_has_e2e, current: fingerprint(host, &ws, &target.workdir) })
}

fn replace_if_same_run(slot: Option<&mut E2eRun>, run: &E2eRun) -> bool {
    slot.filter(|r| r.run_id == run.run_id).map(|r| *r = run.clone()).is_some()
}

impl Registry {
    /// Il task del worker `session` per gli E2E (a prescindere dallo stato del task).
    fn e2e_target(&self, session: &str) -> Option<E2eTarget> {
        let w = self.workers.get(session)?;
        let t = self.boards.get(&w.workspace_id)?.task(&w.task_id)?;
        Some(E2eTarget {
            ws_id: w.workspace_id.clone(),
            task_id: w.task_id.clone(),
            repo: t.repo.clone(),
            workdir: w.env.workdir.clone(),
            repo_dir: w.env.repo_dir.clone(),
            worktree: t.worktree.clone(),
            gated: t.kind == TaskKind::Work && t.e2e,
            last_run: t.e2e_run.clone(),
        })
    }

    /// Un'esecuzione parte per il task `running` del worker `session`.
    pub fn begin_e2e(&mut self, session: &str, run: E2eRun) -> Result<(), String> {
        let w = self.live_worker(session)?;
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        let task = self
            .boards
            .get_mut(&ws)
            .and_then(|b| b.task_mut(&tid))
            .filter(|t| t.status == TaskStatus::Running && t.session.as_deref() == Some(session))
            .ok_or_else(|| TASK_UNASSIGNED.to_string())?;
        task.updated_at = run.started_at;
        task.e2e_run = Some(run);
        self.mark_dirty(&ws);
        Ok(())
    }

    /// Esito di un'esecuzione di un task: sostituisce `running` solo se e'
    /// ancora l'esecuzione del task (un nuovo tentativo o una nuova esecuzione
    /// l'hanno superata) e aggiorna la copia nel report.
    pub fn finish_e2e(&mut self, ws: &str, tid: &str, run: E2eRun) {
        let Some(task) = self.boards.get_mut(ws).and_then(|b| b.task_mut(tid)) else {
            return;
        };
        let in_task = replace_if_same_run(task.e2e_run.as_mut(), &run);
        let in_report = replace_if_same_run(task.report.as_mut().and_then(|r| r.e2e.as_mut()), &run);
        if in_task || in_report {
            self.mark_dirty(ws);
        }
    }

    /// Blocco della consegna di `complete_task`: il task del worker (a
    /// prescindere dal suo stato) contro `input`. Senza `input` un blocco
    /// richiesto rifiuta, non potendo verificare il codice.
    pub(super) fn e2e_gate(&self, session: &str, completed: bool, input: Option<&E2eGateInput>) -> Result<GateOutcome, String> {
        let task = self.workers.get(session).and_then(|w| self.boards.get(&w.workspace_id)?.task(&w.task_id));
        let Some(task) = task else {
            return Ok(GateOutcome::NotRequired);
        };
        check_gate(task, completed, input.is_none_or(|i| i.repo_has_e2e), input.map(|i| &i.current))
    }

    /// Ultima esecuzione E2E del task, da copiare nel report.
    pub(super) fn last_e2e_run(&self, ws: &str, tid: &str) -> Option<E2eRun> {
        self.boards.get(ws)?.task(tid)?.e2e_run.clone()
    }

    /// Annullare le esecuzioni del task (o, senza `task`, di tutto il workspace).
    pub(super) fn cancel_e2e(&mut self, ws: &str, task: Option<&str>) {
        self.effects.push(Effect::CancelE2e { ws_id: ws.to_string(), task_id: task.map(str::to_string) });
    }
}

impl AutomationState {
    pub(super) fn e2e_target(&self, session: &str) -> Option<E2eTarget> {
        self.inner.lock().unwrap().e2e_target(session)
    }

    /// Applica `Effect::CancelE2e`.
    pub(super) fn cancel_e2e(&self, ws: &str, task: Option<&str>) {
        match task {
            Some(t) => self.e2e.cancel_task(ws, t),
            None => self.e2e.cancel_workspace(ws),
        }
    }
}
