//! Board dentro l'automazione: la parte del `Registry` che tiene le board
//! (stato puro + `Effect`), lo scheduler che le fa avanzare, il guardiano degli
//! avvii e la logica dei comandi (UI) e dei tool di sessione (Intake). Git e
//! worktree sono in `worktrees`; tipi, validazione e scheduler puri in
//! `crate::board`. Il contratto e' `docs/BOARD.MD`.

use super::hooks::HOOK_TIMEOUT;
use super::worktrees::{ensure_repos, CleanupTarget, StartJob};
use super::*;
use crate::board::{self, parse_proposal, parse_spec, SchedCtx, TaskSpec, TaskView};
use serde_json::json;

/// Tempo concesso a un avvio (thread git, poi `get_task` del worker).
pub(super) const START_TIMEOUT_MS: i64 = 5 * 60_000;

/// Un passo di preparazione (setup del repository) ha un tetto suo
/// (`HOOK_TIMEOUT`): finche' il thread di avvio ne ha uno in corso il
/// guardiano aspetta quel tempo piu' un margine, contato dall'ultimo battito
/// (`Task::preparing_since`, aggiornato a ogni passo). Il tempo dei setup non
/// consuma quindi i 5 minuti dell'avvio; un thread morto a meta' passo viene
/// comunque scoperto.
pub(super) const PREPARING_TIMEOUT_MS: i64 = HOOK_TIMEOUT.as_millis() as i64 + 2 * 60_000;

/// Lunghezza massima delle note di avviso scritte dall'app sul task.
pub(super) const NOTE_MAX: usize = 2000;

/// Task `running` senza sessione da oltre `START_TIMEOUT_MS` (o, con un passo
/// di preparazione in corso, `PREPARING_TIMEOUT_MS`) dall'ultimo segno di
/// vita: (ws, id, attempt).
pub(super) fn stuck_starts<'a>(boards: impl Iterator<Item = &'a Board>, now: i64) -> Vec<(String, String, u32)> {
    boards
        .flat_map(|b| b.tasks.iter().map(move |t| (b, t)))
        .filter(|(_, t)| {
            let limit = if t.preparing.is_some() { PREPARING_TIMEOUT_MS } else { START_TIMEOUT_MS };
            let last = t.started_at.map(|s| s.max(t.preparing_since.unwrap_or(0)));
            t.status == TaskStatus::Running && t.session.is_none() && last.is_some_and(|l| now - l > limit)
        })
        .map(|(b, t)| (b.workspace_id.clone(), t.id.clone(), t.attempts))
        .collect()
}

// ---------------------------------------------------------------------------
// Registry: board
// ---------------------------------------------------------------------------

impl Registry {
    pub fn load_board(&mut self, mut board: Board) {
        self.next_rev += 1;
        board.rev = self.next_rev;
        self.boards.insert(board.workspace_id.clone(), board);
    }

    /// Vista della board del workspace (vuota se non esiste).
    pub fn board_view(&self, ws: &Workspace, minute: u16) -> BoardView {
        let blocked = self.schedule_blocked(ws, minute);
        match self.boards.get(&ws.id) {
            Some(b) => b.view_with(Some(ws), blocked),
            None => Board::new(&ws.id).view_with(Some(ws), blocked),
        }
    }

    pub fn task_view(&self, ws: &str, id: &str) -> Option<TaskView> {
        self.boards.get(ws).and_then(|b| b.task_view(id))
    }

    pub(super) fn mark_dirty(&mut self, ws: &str) {
        if !self.dirty.iter().any(|d| d == ws) {
            self.dirty.push(ws.to_string());
        }
    }

    fn board_or_new(&mut self, ws: &str) -> &mut Board {
        self.boards.entry(ws.to_string()).or_insert_with(|| Board::new(ws))
    }

    /// Esegue `f` sulla board e la segna da schedulare/pubblicare se riesce.
    pub(super) fn edit<T>(&mut self, ws: &str, f: impl FnOnce(&mut Board) -> Result<T, String>) -> Result<T, String> {
        let out = f(self.board_or_new(ws))?;
        self.mark_dirty(ws);
        Ok(out)
    }

    /// Id libero per un task creato a mano dal titolo.
    pub fn free_id(&self, ws: &str, title: &str) -> String {
        let board = self.boards.get(ws);
        board::id_from_title(title, &|id| board.is_some_and(|b| b.id_taken(id)))
    }

    pub fn create_task(&mut self, ws: &str, spec: TaskSpec, now: i64) -> Result<String, String> {
        self.edit(ws, |b| b.create_task(spec, now))
    }

    /// Nuova proposta (dell'Intake o del revisore di `source_task`): ritorna l'id.
    pub fn propose(
        &mut self,
        ws: &str,
        source_tab: &str,
        source_task: Option<String>,
        specs: Vec<TaskSpec>,
        now: i64,
    ) -> Result<String, String> {
        self.next_proposal += 1;
        let id = format!("p-{now}-{}", self.next_proposal);
        self.edit(ws, |b| b.add_proposal(id.clone(), source_tab, source_task, specs, now))?;
        Ok(id)
    }

    /// Task di una proposta pendente (per verificarne i repository).
    pub fn proposal_specs(&self, ws: &str, id: &str) -> Result<Vec<TaskSpec>, String> {
        self.boards
            .get(ws)
            .ok_or_else(|| format!("proposal not found: {id}"))?
            .proposal(id)
            .map(|p| p.tasks.clone())
    }

    pub fn decide_proposal(&mut self, ws: &str, id: &str, accept: bool, now: i64) -> Result<(), String> {
        self.edit(ws, |b| if accept { b.accept_proposal(id, now).map(|_| ()) } else { b.discard_proposal(id, now) })
    }

    pub fn update_task(&mut self, ws: &str, id: &str, patch: &Value, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.update_task(id, patch, now))
    }

    /// Toglie un task; se aveva un worktree lo fa ripulire.
    pub fn delete_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let removed = self.edit(ws, |b| b.delete_task(id, now))?;
        if let Some(wt) = removed.worktree {
            self.effects.push(Effect::Cleanup(Box::new(CleanupTarget { ws_id: ws.to_string(), task_id: id.to_string(), repo: removed.repo, wt })));
        }
        Ok(())
    }

    pub fn retry_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.retry_task(id, now))
    }

    pub fn mark_task_done(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.mark_done(id, now))
    }

    pub fn move_task(&mut self, ws: &str, id: &str, before: Option<&str>, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.move_task(id, before, now))
    }

    pub fn task_to_top(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.task_to_top(id, now))
    }

    /// "Ferma": il task `running` fallisce e il tab del worker si chiude.
    pub fn stop_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let b = self.boards.get(ws).ok_or_else(|| "task not found: the board is empty".to_string())?;
        b.check_stop(id)?;
        let session = b.task(id).and_then(|t| t.session.clone());
        if let Some(w) = session.as_deref().and_then(|s| self.workers.get_mut(s)).filter(|w| w.tab_open) {
            if !w.status.is_terminal() {
                w.status = WorkerStatus::Closed;
            }
            let tab = w.session.clone();
            self.touch(&tab, now);
            self.effects.push(Effect::Close { tab_id: tab, delay: Duration::ZERO });
        }
        self.fail_task(ws, id, None, board::STOPPED_NOTE, now);
        Ok(())
    }

    pub fn set_board(&mut self, ws: &str, max_parallel: Option<u8>, paused: Option<bool>, now: i64) -> Result<(), String> {
        if max_parallel.is_none() && paused.is_none() {
            return Err("nothing to change: pass max_parallel and/or paused".to_string());
        }
        if let Some(m) = max_parallel {
            board::parse_max_parallel(&json!(m))?;
        }
        self.edit(ws, |b| {
            if let Some(m) = max_parallel {
                b.max_parallel = m;
            }
            if let Some(p) = paused {
                b.paused = p;
            }
            b.updated_at = now;
            Ok(())
        })
    }

    pub fn remove_board(&mut self, ws: &str) {
        if self.boards.remove(ws).is_some() {
            self.dirty.retain(|d| d != ws);
            self.next_rev += 1;
            self.effects.push(Effect::DeleteBoard(ws.to_string(), self.next_rev));
        }
    }

    // --- scheduler ---

    /// Schedula e pubblica le board modificate. `lookup` risolve i workspace
    /// (senza, nulla parte); `minute`: ora locale di `now`, per il gate delle
    /// fasce calcolato adesso e non al giro del timer (fino a 30 s fa).
    pub fn flush_boards(&mut self, now: i64, minute: u16, lookup: &dyn Fn(&str) -> Option<Workspace>) {
        for ws_id in std::mem::take(&mut self.dirty) {
            self.flush_one(&ws_id, now, minute, lookup);
        }
        self.update_awake();
    }

    fn flush_one(&mut self, ws_id: &str, now: i64, minute: u16, lookup: &dyn Fn(&str) -> Option<Workspace>) {
        let Some(ws) = lookup(ws_id) else {
            return self.publish_board(ws_id, None, now, minute);
        };
        // Archiviato: congelato. Fuori fascia (se il workspace le usa): nessun avvio.
        let ctx = SchedCtx { delivery: ws.delivery, gate_open: !ws.archived && !self.schedule_blocked(&ws, minute) };
        let Some(b) = self.boards.get_mut(ws_id) else {
            return;
        };
        for id in board::schedule(b, &ctx) {
            // Subito `running`: un'altra esecuzione non lo sceglie due volte.
            b.start_task(&id, now);
            if let Some(task) = b.task(&id).cloned() {
                let job = StartJob { ws: ws.clone(), attempt: task.attempts, task };
                self.effects.push(Effect::StartTask(Box::new(job)));
            }
        }
        self.publish_board(ws_id, Some(&ws), now, minute);
    }

    /// Ogni pubblicazione e' una modifica della vista: nuova revisione.
    fn publish_board(&mut self, ws_id: &str, ws: Option<&Workspace>, now: i64, minute: u16) {
        let blocked = ws.is_some_and(|w| self.schedule_blocked(w, minute));
        self.next_rev += 1;
        let Some(b) = self.boards.get_mut(ws_id) else {
            return;
        };
        b.rev = self.next_rev;
        b.updated_at = now;
        let view = b.view_with(ws, blocked);
        self.effects.push(Effect::Publish(Box::new(b.clone()), Box::new(view)));
    }

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
            .is_some_and(|t| t.status == TaskStatus::Running && t.attempts == attempt && t.session.is_none())
    }

    /// Registra il worker di un task appena preparato (git fatto). `None` se
    /// l'avvio e' stato superato (stop, retry).
    pub(super) fn register_task_worker(
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
        let ev = self.spawn_worker(ws, tid, job.attempt, new, env, now);
        if let Some(t) = self.boards.get_mut(ws).and_then(|b| b.task_mut(tid)) {
            t.session = Some(ev.tab_id.clone());
            t.worker_status = Some(WorkerStatus::Starting);
            t.preparing = None;
            t.preparing_since = None;
            if worktree.is_some() {
                t.worktree = worktree;
            }
            // Avvisi di preparazione (es. exclude non aggiornato).
            if !warnings.is_empty() {
                t.note = Some(clip_chars(&format!("start warnings: {}", warnings.join("; ")), NOTE_MAX));
            }
            t.updated_at = now;
        }
        self.mark_dirty(ws);
        Some(ev)
    }

    /// L'avvio del task e' fallito: `failed` con la nota.
    pub fn start_failed(&mut self, ws: &str, tid: &str, attempt: u32, msg: &str, now: i64) {
        let current = self.boards.get(ws).and_then(|b| b.task(tid)).is_some_and(|t| t.attempts == attempt && t.session.is_none());
        if current && self.fail_task(ws, tid, None, &clip_chars(msg, NOTE_MAX), now) {
            self.notify(ws, tid, None, "failed");
        }
    }

    /// Workspace archiviato: i task in corso tornano in coda e i worker si
    /// staccano (la chiusura dei loro tab non genera fallimenti).
    pub fn archive_workspace(&mut self, ws: &str, now: i64) {
        let Some(b) = self.boards.get_mut(ws) else {
            return;
        };
        b.requeue_running(board::ARCHIVED_NOTE, now);
        for w in self.workers.values_mut().filter(|w| w.workspace_id == ws) {
            w.tab_open = false;
            if !w.status.is_terminal() {
                w.status = WorkerStatus::Closed;
            }
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
            .filter(|w| w.tab_open && w.status == WorkerStatus::Starting && !w.started_sent && now - w.created_at > START_TIMEOUT_MS)
            .map(|w| w.session.clone())
            .collect();
        for s in silent {
            if let Some(w) = self.workers.get_mut(&s) {
                w.tab_open = false;
            }
            self.close_worker(&s, "the worker never called get_task: the agent did not start", now);
            self.effects.push(Effect::Close { tab_id: s, delay: Duration::ZERO });
        }
    }

    // --- pulizia ---

    /// Task `done` con worktree: lo ripulisce l'applicazione degli effetti.
    pub(super) fn queue_cleanup(&mut self, ws: &str, tid: &str) {
        let target = self
            .boards
            .get(ws)
            .and_then(|b| b.task(tid))
            .filter(|t| t.status == TaskStatus::Done)
            .and_then(|t| t.worktree.clone().map(|wt| CleanupTarget { ws_id: ws.to_string(), task_id: tid.to_string(), repo: t.repo.clone(), wt }));
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
            t.note = Some(clip_chars(&format!("cleanup warnings: {}", warnings.join("; ")), NOTE_MAX));
        }
        t.updated_at = now;
        self.mark_dirty(ws);
    }
}

impl AutomationState {
    /// Workspace e task del worker (per `get_task` e il diffstat).
    pub(super) fn worker_task_of(&self, session: &str) -> Option<(String, String)> {
        let reg = self.inner.lock().unwrap();
        reg.workers.get(session).map(|w| (w.workspace_id.clone(), w.task_id.clone()))
    }

    /// Worktree del task del worker (per il diffstat), se ne ha uno.
    pub(super) fn worktree_of(&self, session: &str) -> Option<(String, WorktreeInfo)> {
        let (ws, tid) = self.worker_task_of(session)?;
        let reg = self.inner.lock().unwrap();
        let wt = reg.boards.get(&ws)?.task(&tid)?.worktree.clone()?;
        Some((ws, wt))
    }
}

// ---------------------------------------------------------------------------
// Logica dei comandi Tauri e dei tool di sessione
// ---------------------------------------------------------------------------

pub(super) fn workspace_of<H: Host>(host: &H, ws_id: &str) -> Result<Workspace, String> {
    let ws = host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
    if ws.archived {
        return Err("workspace archiviato".to_string());
    }
    Ok(ws)
}

/// Workspace del tab che chiama un tool di sessione.
pub(super) fn session_workspace<H: Host>(host: &H, session: &str) -> Result<Workspace, String> {
    host.tab_workspace(session).ok_or_else(|| "the workspace of this tab was not found".to_string())
}

pub(super) fn board_set<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    if max_parallel.is_none() && paused.is_none() {
        return Err("nessuna modifica richiesta".to_string());
    }
    if max_parallel.is_some_and(|m| !(1..=board::MAX_PARALLEL_LIMIT).contains(&m)) {
        return Err(format!("max_parallel deve essere tra 1 e {}", board::MAX_PARALLEL_LIMIT));
    }
    let now = now_millis();
    st.run(host, |r| r.set_board(ws_id, max_parallel, paused, now))?;
    st.view(host, ws_id)
}

pub(super) fn task_create<H: Host>(st: &AutomationState, host: &H, ws_id: &str, spec: &Value) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    let mut spec = parse_spec(spec, None)?;
    ensure_repos(host, &ws, [spec.repo.as_deref()])?;
    let now = now_millis();
    st.run(host, |r| {
        if spec.id.is_empty() {
            spec.id = r.free_id(ws_id, &spec.title);
        }
        r.create_task(ws_id, spec, now)
    })?;
    st.view(host, ws_id)
}

/// Modifica di un task: il repository nuovo si verifica fuori dal lock.
pub(super) fn update_checked<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    id: &str,
    patch: &Value,
) -> Result<(), String> {
    let current = st.run(host, |r| r.task_view(&ws.id, id)).ok_or_else(|| format!("task not found: {id}"))?;
    let spec = parse_spec(patch, Some(&TaskSpec::from_task(&current.task)))?;
    if spec.repo != current.task.repo {
        ensure_repos(host, ws, [spec.repo.as_deref()])?;
    }
    let now = now_millis();
    st.run(host, |r| r.update_task(&ws.id, id, patch, now))
}

pub(super) fn task_update<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    patch: &Value,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    update_checked(st, host, &ws, task_id, patch)?;
    st.view(host, ws_id)
}

pub(super) fn task_action<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    action: &str,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    let now = now_millis();
    st.run(host, |r| match action {
        "retry" => r.retry_task(ws_id, task_id, now),
        "mark_done" => r.mark_task_done(ws_id, task_id, now),
        "stop" => r.stop_task(ws_id, task_id, now),
        "delete" => r.delete_task(ws_id, task_id, now),
        "to_top" => r.task_to_top(ws_id, task_id, now),
        _ => Err("azione non valida (usa retry, mark_done, stop, delete o to_top)".to_string()),
    })?;
    st.view(host, ws_id)
}

pub(super) fn task_move<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    before: Option<&str>,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    let now = now_millis();
    st.run(host, |r| r.move_task(ws_id, task_id, before, now))?;
    st.view(host, ws_id)
}

pub(super) fn proposal_decide<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    proposal_id: &str,
    accept: bool,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    if accept {
        // Nuova validazione: la modalita' di consegna o i repository possono essere cambiati.
        let specs = st.run(host, |r| r.proposal_specs(ws_id, proposal_id))?;
        ensure_repos(host, &ws, specs.iter().map(|s| s.repo.as_deref()))?;
    }
    let now = now_millis();
    st.run(host, |r| r.decide_proposal(ws_id, proposal_id, accept, now))?;
    st.view(host, ws_id)
}

/// Tool MCP delle sessioni normali (Intake compreso): la coda del workspace
/// del tab chiamante.
pub(super) fn rpc_session<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    let ws = session_workspace(host, session)?;
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    let now = now_millis();
    match method {
        "propose_tasks" => propose_tasks(st, host, &ws, session, None, params),
        "list_tasks" => {
            let statuses = parse_statuses(params)?;
            let minute = host.local_minute(now);
            let view = st.run(host, |r| r.board_view(&ws, minute));
            Ok(view.to_list_json(statuses.as_deref()))
        }
        "get_task_details" => task_details(st, host, &ws, params),
        "update_task" => {
            let id = req_str(params, "id")?.trim().to_string();
            update_checked(st, host, &ws, &id, params)?;
            let view = st.run(host, |r| r.task_view(&ws.id, &id));
            Ok(json!({ "task": view.map(|v| v.to_json(false)) }))
        }
        "delete_task" => {
            let id = req_str(params, "id")?.trim().to_string();
            st.run(host, |r| r.delete_task(&ws.id, &id, now))?;
            Ok(json!({ "deleted": id }))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}

/// `propose_tasks`: proposta di task per la coda di `ws`. `tab` e' il tab che la fa;
/// `source_task` il task di revisione, se a farla e' il suo revisore.
pub(super) fn propose_tasks<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    tab: &str,
    source_task: Option<String>,
    params: &Value,
) -> Result<Value, String> {
    let specs = parse_proposal(params)?;
    ensure_repos(host, ws, specs.iter().map(|s| s.repo.as_deref()))?;
    let now = now_millis();
    let id = st.run(host, |r| r.propose(&ws.id, tab, source_task, specs, now))?;
    Ok(json!({
        "proposal": id,
        "status": "pending",
        "message": "The user will accept or discard it in the app. Do not wait for the decision."
    }))
}

/// `get_task_details`: il task di `ws` con il testo della specifica.
pub(super) fn task_details<H: Host>(st: &AutomationState, host: &H, ws: &Workspace, params: &Value) -> Result<Value, String> {
    let id = req_str(params, "id")?.trim();
    let view = st.run(host, |r| r.task_view(&ws.id, id)).ok_or_else(|| format!("task not found: {id}"))?;
    Ok(json!({ "task": view.to_json(true) }))
}

/// `status` di `list_tasks`: elenco opzionale di stati validi.
fn parse_statuses(params: &Value) -> Result<Option<Vec<TaskStatus>>, String> {
    let valid = || TaskStatus::ALL.map(TaskStatus::as_str).join(", ");
    let list = match params.get("status") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(format!("status must be an array of statuses ({})", valid())),
    };
    list.iter()
        .map(|v| v.as_str().and_then(TaskStatus::parse).ok_or_else(|| format!("invalid status {v} (valid: {})", valid())))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}
