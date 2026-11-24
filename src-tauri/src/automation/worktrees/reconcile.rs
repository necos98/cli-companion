//! Riallineamento delle worktree gestite con le board. La pulizia normale di un
//! task `done` scatta alla chiusura del suo tab e dipende da un flag in
//! memoria: un crash, o la chiusura dell'app, la perdono e niente la rifa. Qui
//! il confronto e' a livello, non a fronti: all'avvio e poi a intervalli si
//! guarda cosa c'e' davvero (board e `git worktree list`) e si ripulisce il
//! resto.
//!
//! - Task `done` con un worktree e senza tab aperto: pulizia in coda.
//! - Worktree `task/<id>` che nessun task conosce (task eliminato, board
//!   rimossa): avviso; pulizia in coda solo se non ha modifiche non committate
//!   (e il branch si cancella sempre con `-d`).
//!
//! Git gira su un thread dell'host, mai sotto il lock del registro.

use super::{git, repo_dir, task_branch, CleanupTarget, TASK_BRANCH_PREFIX, WORKTREES_DIR};
use crate::automation::{AutomationState, Effect, Host, Registry, Workspace};
use crate::board::{TaskStatus, WorktreeInfo};
use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Ogni quanto si riallinea (il primo giro e' quello dell'avvio).
const RECONCILE_EVERY_MS: u64 = 5 * 60_000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// `git worktree list --porcelain`
// ---------------------------------------------------------------------------

/// Una voce di `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedWorktree {
    /// Cartella, come la scrive git (`/` anche su Windows).
    pub path: String,
    /// Branch senza `refs/heads/`; `None` se detached o bare.
    pub branch: Option<String>,
    /// La cartella non c'e' piu': git la toglierebbe con `worktree prune`.
    pub prunable: bool,
}

impl ListedWorktree {
    /// Id del task se e' un worktree creato dall'app: branch `task/<id>` in
    /// `<repo>/.worktrees/<id>`. Qualunque altro worktree non e' nostro.
    pub fn task_id(&self) -> Option<&str> {
        let id = self.branch.as_deref()?.strip_prefix(TASK_BRANCH_PREFIX)?;
        let path = self.path.replace('\\', "/");
        let mut parts = path.trim_end_matches('/').rsplit('/');
        (parts.next()? == id && parts.next()? == WORKTREES_DIR).then_some(id)
    }
}

/// Voci dell'output di `git worktree list --porcelain`: blocchi di righe
/// `chiave valore`, ognuno aperto da `worktree <path>`.
pub(super) fn parse_worktree_list(out: &str) -> Vec<ListedWorktree> {
    let mut list: Vec<ListedWorktree> = Vec::new();
    for line in out.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            list.push(ListedWorktree { path: path.to_string(), branch: None, prunable: false });
        } else if let Some(current) = list.last_mut() {
            if let Some(branch) = line.strip_prefix("branch ") {
                current.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_string());
            } else if line == "prunable" || line.starts_with("prunable ") {
                current.prunable = true;
            }
        }
    }
    list
}

// ---------------------------------------------------------------------------
// Stato del riallineamento
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Sweep {
    running: bool,
    last_start: Option<i64>,
}

/// Cadenza del riallineamento e pulizie in corso. Sta in `AutomationState`.
#[derive(Default)]
pub(in crate::automation) struct Reconciler {
    sweep: Mutex<Sweep>,
    /// Pulizie in corso: "<workspace>/<task>".
    cleaning: Mutex<HashSet<String>>,
}

/// Un giro di riallineamento in corso: alla fine (o a un panico) ne libera il posto.
pub(super) struct SweepTurn(Arc<Reconciler>);

impl Drop for SweepTurn {
    fn drop(&mut self) {
        lock(&self.0.sweep).running = false;
    }
}

/// La pulizia di un task e' in corso: alla fine (o a un panico) la libera.
pub(in crate::automation) struct CleanupTurn<'a> {
    owner: &'a Reconciler,
    key: String,
}

impl Drop for CleanupTurn<'_> {
    fn drop(&mut self) {
        lock(&self.owner.cleaning).remove(&self.key);
    }
}

impl Reconciler {
    /// Il giro e' dovuto e non ce n'e' uno in corso? Allora lo prenota. E'
    /// dovuto se non e' mai partito o se l'ultimo dista `RECONCILE_EVERY_MS`
    /// o piu', anche all'indietro: l'orologio di sistema puo' essere corretto.
    pub(super) fn begin_sweep(self: &Arc<Self>, now: i64) -> Option<SweepTurn> {
        let mut sweep = lock(&self.sweep);
        let due = sweep.last_start.is_none_or(|t| now.abs_diff(t) >= RECONCILE_EVERY_MS);
        if sweep.running || !due {
            return None;
        }
        sweep.running = true;
        sweep.last_start = Some(now);
        Some(SweepTurn(Arc::clone(self)))
    }

    /// Prende il turno di pulizia del task; `None` se un'altra e' in corso.
    pub fn begin_cleanup(&self, ws_id: &str, task_id: &str) -> Option<CleanupTurn<'_>> {
        let key = format!("{ws_id}/{task_id}");
        // Il lock si rilascia prima di costruire il turno: il suo `Drop` lo riprende.
        let taken = lock(&self.cleaning).insert(key.clone());
        taken.then(|| CleanupTurn { owner: self, key })
    }
}

// ---------------------------------------------------------------------------
// Registro: cio' che serve al riallineamento (stato puro)
// ---------------------------------------------------------------------------

impl Registry {
    /// Id dei task della board di `ws`.
    fn task_ids(&self, ws: &str) -> HashSet<String> {
        self.boards.get(ws).into_iter().flat_map(|b| b.tasks.iter().map(|t| t.id.clone())).collect()
    }

    /// Mette in coda la pulizia dei task `done` che hanno ancora un worktree e
    /// il cui tab non e' aperto (con il tab aperto ci pensa la sua chiusura).
    /// Ritorna gli id.
    fn queue_finished_cleanups(&mut self, ws: &str) -> Vec<String> {
        let ids: Vec<String> = self
            .boards
            .get(ws)
            .into_iter()
            .flat_map(|b| &b.tasks)
            .filter(|t| t.status() == TaskStatus::Done && t.worktree.is_some())
            .filter(|t| !self.workers.values().any(|w| w.tab_open && w.workspace_id == ws && w.task_id == t.id))
            .map(|t| t.id.clone())
            .collect();
        for id in &ids {
            self.queue_cleanup(ws, id);
        }
        ids
    }

    /// Mette in coda la pulizia di una worktree `task/<id>` che nessun task
    /// conosce. Il controllo si ripete qui, sotto il lock: un task con quell'id
    /// creato dopo l'elenco di git la rende viva. `false` = non messa in coda.
    pub(in crate::automation) fn queue_orphan_cleanup(&mut self, target: CleanupTarget) -> bool {
        if self.boards.get(&target.ws_id).is_some_and(|b| b.task(&target.task_id).is_some()) {
            return false;
        }
        self.effects.push(Effect::Cleanup(Box::new(target)));
        true
    }

    /// La pulizia e' ancora utile? Un task della board senza piu' il worktree
    /// e' gia' stato ripulito da un'altra richiesta; un task sparito (eliminato,
    /// board rimossa) o una worktree orfana si ripuliscono.
    pub(in crate::automation) fn cleanup_needed(&self, target: &CleanupTarget) -> bool {
        match self.boards.get(&target.ws_id).and_then(|b| b.task(&target.task_id)) {
            Some(t) => t.worktree.is_some(),
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Il giro
// ---------------------------------------------------------------------------

/// Esito di un giro.
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::automation) struct ReconcileReport {
    /// Pulizie messe in coda: "<workspace>/<task>".
    pub queued: Vec<String>,
    /// Worktree lasciate dove sono, e perche'.
    pub warnings: Vec<String>,
}

impl AutomationState {
    /// Giro del timer: se e' dovuto riallinea, su un thread dell'host (git e'
    /// lento e il timer non deve aspettarlo). Uno alla volta.
    pub(in crate::automation) fn reconcile_worktrees_if_due<H: Host>(&self, host: &H, now: i64) {
        let Some(turn) = self.reconciler.begin_sweep(now) else {
            return;
        };
        let (st, h) = (self.clone(), host.clone());
        host.defer(
            Duration::ZERO,
            Box::new(move || {
                let report = st.reconcile_worktrees(&h);
                drop(turn);
                for w in &report.warnings {
                    log::warn!("worktrees: {w}");
                }
                if !report.queued.is_empty() {
                    log::info!("worktrees: cleaning {}", report.queued.join(", "));
                }
            }),
        );
    }

    /// Confronta board e worktree di git in ogni workspace attivo e mette in
    /// coda le pulizie dovute.
    pub(in crate::automation) fn reconcile_worktrees<H: Host>(&self, host: &H) -> ReconcileReport {
        let mut report = ReconcileReport::default();
        // Un workspace archiviato e' congelato: non si tocca.
        for ws in host.workspaces().into_iter().filter(|w| !w.archived) {
            let finished = self.run(host, |r| r.queue_finished_cleanups(&ws.id));
            report.queued.extend(finished.into_iter().map(|id| format!("{}/{id}", ws.id)));
            if ws.delivery.worktrees() {
                self.reconcile_orphans(host, &ws, &mut report);
            }
        }
        report
    }

    /// Le worktree `task/<id>` dei repository di `ws` che nessun task conosce.
    fn reconcile_orphans<H: Host>(&self, host: &H, ws: &Workspace, report: &mut ReconcileReport) {
        let repos = match host.list_repos(ws) {
            Ok(repos) => repos,
            Err(e) => {
                report.warnings.push(format!("workspace {}: repositories not listed: {e}", ws.id));
                return;
            }
        };
        let known = self.run(host, |r| r.task_ids(&ws.id));
        for name in &repos {
            let repo = (name != ".").then_some(name.as_str());
            let dir = repo_dir(ws, repo);
            let listed = match git(host, ws, &dir, &["worktree", "list", "--porcelain"]) {
                Ok(out) => parse_worktree_list(&out),
                Err(e) => {
                    report.warnings.push(format!("{dir}: worktrees not listed: {e}"));
                    continue;
                }
            };
            for wt in &listed {
                if let Some(id) = wt.task_id().filter(|id| !known.contains(*id)) {
                    self.reclaim_orphan(host, ws, repo, wt, id, report);
                }
            }
        }
    }

    /// Una worktree senza task: avviso, e pulizia se non rischia lavoro non committato.
    fn reclaim_orphan<H: Host>(
        &self,
        host: &H,
        ws: &Workspace,
        repo: Option<&str>,
        wt: &ListedWorktree,
        id: &str,
        report: &mut ReconcileReport,
    ) {
        let label = format!("{}/{id}", ws.id);
        // Cartella sparita: niente da perdere, resta il branch. Altrimenti si
        // guarda lo stato; senza risposta si lascia stare.
        let uncommitted = if wt.prunable {
            Ok(false)
        } else {
            git(host, ws, &wt.path, &["status", "--porcelain"]).map(|status| !status.is_empty())
        };
        let kept = match uncommitted {
            Ok(false) => None,
            Ok(true) => Some("it has uncommitted changes".to_string()),
            Err(e) => Some(format!("its state is unknown ({e})")),
        };
        if let Some(why) = kept {
            report.warnings.push(format!("{label}: {} belongs to no task and was kept: {why}", wt.path));
            return;
        }
        let target = CleanupTarget {
            ws_id: ws.id.clone(),
            task_id: id.to_string(),
            repo: repo.map(str::to_string),
            wt: WorktreeInfo { path: wt.path.clone(), branch: task_branch(id), base: String::new(), setup_done: true },
            workspace: None,
            keep_unmerged: true,
        };
        if self.run(host, |r| r.queue_orphan_cleanup(target)) {
            report.warnings.push(format!("{label}: {} belongs to no task, removing it", wt.path));
            report.queued.push(label);
        }
    }
}
