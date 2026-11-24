//! Ciclo di vita dei task nel `Registry`: avvio (preparazione, registrazione
//! del worker, fallimento), guardiano degli avvii bloccati, archiviazione del
//! workspace e pulizia dei worktree a consegna avvenuta.

use crate::automation::hooks::HOOK_TIMEOUT;
use crate::automation::worktrees::{CleanupTarget, StartJob};
use crate::automation::*;
use crate::board::{self, NOTE_MAX};

/// Tempo concesso a un avvio (thread git, poi `get_task` del worker).
pub(in crate::automation) const START_TIMEOUT_MS: i64 = 5 * 60_000;

/// Un passo di preparazione (setup del repository) ha un tetto suo
/// (`HOOK_TIMEOUT`): finche' il thread di avvio ne ha uno in corso il
/// guardiano aspetta quel tempo piu' un margine, contato dall'ultimo battito
/// (`RunPhase::Preparing::heartbeat`, aggiornato a ogni passo). Il tempo dei setup non
/// consuma quindi i 5 minuti dell'avvio; un thread morto a meta' passo viene
/// comunque scoperto.
const PREPARING_TIMEOUT_MS: i64 = HOOK_TIMEOUT.as_millis() as i64 + 2 * 60_000;

/// Task `running` senza sessione da oltre `START_TIMEOUT_MS` (o, con un passo
/// di preparazione in corso, `PREPARING_TIMEOUT_MS`) dall'ultimo segno di
/// vita: (ws, id, attempt).
fn stuck_starts<'a>(boards: impl Iterator<Item = &'a Board>, now: i64) -> Vec<(String, String, u32)> {
    boards
        .flat_map(|b| b.tasks.iter().map(move |t| (b, t)))
        .filter(|(_, t)| {
            t.preparing().is_some_and(|(step, heartbeat)| {
                let limit = if step.is_some() { PREPARING_TIMEOUT_MS } else { START_TIMEOUT_MS };
                now - heartbeat > limit
            })
        })
        .map(|(b, t)| (b.workspace_id.clone(), t.id.clone(), t.attempts()))
        .collect()
}

impl Registry {
    // --- avvio dei task ---

    /// Passo di preparazione in corso di un avvio (vedi `Task::preparing`).
    /// `label` `None` = solo battito per il guardiano. `publish`: pubblica
    /// subito la vista.
    pub fn touch_preparing(&mut self, ws: &str, tid: &str, attempt: u32, label: Option<&str>, now: i64, publish: bool) {
        let ok = self.boards.get_mut(ws).is_some_and(|b| b.touch_preparing(tid, attempt, label, now));
        if ok && publish && label.is_some() {
            self.mark_dirty(ws);
        }
    }

    /// Fine della preparazione: via l'etichetta; il battito resta (il
    /// guardiano ricomincia da qui il suo tempo per il resto dell'avvio).
    pub fn end_preparing(&mut self, ws: &str, tid: &str, attempt: u32, now: i64) {
        if let Some(b) = self.boards.get_mut(ws) {
            b.clear_preparing(tid, attempt, now);
        }
        self.mark_dirty(ws);
    }

    /// Salva nel task il worktree appena creato (prima del setup).
    pub fn record_worktree(&mut self, ws: &str, tid: &str, attempt: u32, wt: WorktreeInfo) {
        if self.boards.get_mut(ws).is_some_and(|b| b.record_worktree(tid, attempt, wt)) {
            self.mark_dirty(ws);
        }
    }

    /// L'avvio `attempt` del task e' ancora quello in corso (running, senza
    /// sessione)?
    pub fn start_is_current(&self, ws: &str, tid: &str, attempt: u32) -> bool {
        self.boards
            .get(ws)
            .and_then(|b| b.task(tid))
            .is_some_and(|t| t.attempts() == attempt && t.preparing().is_some())
    }

    /// Registra il worker di un task appena preparato (git fatto). `None` se
    /// l'avvio e' stato superato (stop, retry).
    pub(in crate::automation) fn register_task_worker(
        &mut self,
        job: &StartJob,
        worktree: Option<WorktreeInfo>,
        warnings: Vec<String>,
        new: NewWorker,
        env: WorkerEnv,
        now: i64,
    ) -> Option<SpawnEvent> {
        let (ws, tid) = (job.ws.id.as_str(), job.task.id.as_str());
        if !self.start_is_current(ws, tid, job.attempt) {
            return None;
        }
        let id = self.next_worker_id(now);
        // Avvisi di preparazione (es. exclude non aggiornato).
        let note =
            (!warnings.is_empty()).then(|| clip_chars(&format!("start warnings: {}", warnings.join("; ")), NOTE_MAX));
        // Il rilevamento dell'avvio e' quello autorevole per `no_git`.
        let free = env.free;
        let spawned = self.boards.get_mut(ws).is_some_and(|b| {
            let ok = b.worker_spawned(tid, job.attempt, id.clone(), worktree, note, now);
            if let Some(t) = b.task_mut(tid).filter(|_| ok) {
                t.no_git = free;
            }
            ok
        });
        if !spawned {
            return None;
        }
        let ev = self.spawn_worker(id, tid, job.attempt, new, env, now);
        self.mark_dirty(ws);
        Some(ev)
    }

    /// L'avvio del task e' fallito: `failed` con la nota.
    pub fn start_failed(&mut self, ws: &str, tid: &str, attempt: u32, msg: &str, now: i64) {
        let current = self
            .boards
            .get(ws)
            .and_then(|b| b.task(tid))
            .is_some_and(|t| t.attempts() == attempt && t.session().is_none());
        let note = clip_chars(msg, NOTE_MAX);
        if current
            && self.fail_task(
                ws,
                tid,
                None,
                &note,
                Failure { cause: FailCause::Other, reason: FailReason::StartFailed },
                now,
            )
        {
            self.notify(ws, tid, None, "failed");
        }
    }

    /// Workspace archiviato: i task in corso tornano in coda e i worker si
    /// staccano (la chiusura dei loro tab non genera fallimenti).
    pub fn archive_workspace(&mut self, ws: &str, now: i64) {
        self.cancel_e2e(ws, None);
        let Some(b) = self.boards.get_mut(ws) else {
            return;
        };
        b.requeue_running(board::ARCHIVED_NOTE, now);
        for w in self.workers.values_mut().filter(|w| w.workspace_id == ws) {
            w.tab_open = false;
            w.ended.get_or_insert(WorkerEnd::Closed);
        }
        self.mark_dirty(ws);
    }

    /// Guardiano (giro periodico): un task `running` senza sessione da troppo
    /// (thread di avvio bloccato) fallisce come un avvio fallito; un worker
    /// che non ha mai chiamato `get_task` (tab mai comparso, agente non
    /// partito) fallisce il task e chiude il tab.
    pub fn watchdog(&mut self, now: i64) {
        for (ws, tid, attempt) in stuck_starts(self.boards.values(), now) {
            self.start_failed(&ws, &tid, attempt, "the worker did not start in time (start timed out)", now);
        }
        let silent: Vec<String> = self
            .workers
            .values()
            .filter(|w| {
                w.tab_open
                    && self.worker_status(w) == WorkerStatus::Starting
                    && !w.started_sent
                    && now - w.created_at > START_TIMEOUT_MS
            })
            .map(|w| w.session.clone())
            .collect();
        for s in silent {
            if let Some(w) = self.workers.get_mut(&s) {
                w.tab_open = false;
            }
            let note = "the worker never called get_task: the agent did not start";
            self.close_worker(&s, note, FailCause::Other, now);
            self.queue_read_only_check(&s);
            self.effects.push(Effect::Close { tab_id: s, delay: Duration::ZERO });
        }
    }

    // --- pulizia ---

    /// Task `done` con worktree: lo ripulisce l'applicazione degli effetti.
    pub(in crate::automation) fn queue_cleanup(&mut self, ws: &str, tid: &str) {
        let target = self
            .boards
            .get(ws)
            .and_then(|b| b.task(tid))
            .filter(|t| t.status() == TaskStatus::Done)
            .and_then(|t| CleanupTarget::for_task(ws, t.clone()));
        if let Some(t) = target {
            self.effects.push(Effect::Cleanup(Box::new(t)));
        }
    }

    /// Esito della pulizia: worktree sparito dal task; gli avvisi diventano
    /// la nota (mai bloccanti).
    pub fn cleanup_done(&mut self, ws: &str, tid: &str, removed: bool, warnings: &[String], now: i64) {
        let Some(t) = self.boards.get_mut(ws).and_then(|b| b.task_mut(tid)) else {
            return;
        };
        if removed {
            t.worktree = None;
        }
        if !warnings.is_empty() {
            t.replace_note(&format!("cleanup warnings: {}", warnings.join("; ")), now);
        }
        t.updated_at = now;
        self.mark_dirty(ws);
    }
}

impl AutomationState {
    /// Workspace e task del worker (per `get_task` e il diffstat).
    pub(in crate::automation) fn worker_task_of(&self, session: &str) -> Option<(String, String)> {
        let reg = self.registry();
        reg.workers.get(session).map(|w| (w.workspace_id.clone(), w.task_id.clone()))
    }

    /// Worktree del task del worker (per il diffstat), se ne ha uno.
    pub(in crate::automation) fn worktree_of(&self, session: &str) -> Option<(String, WorktreeInfo)> {
        let (ws, tid) = self.worker_task_of(session)?;
        let reg = self.registry();
        let wt = reg.boards.get(&ws)?.task(&tid)?.worktree.clone()?;
        Some((ws, wt))
    }
}
