//! Board dei task dentro l'automazione: la parte del `Registry` che tiene le
//! board (stato puro + `Effect`), l'avvio dei task con i worktree git (fuori
//! dal lock, su un thread), i tool RPC dell'orchestratore e i comandi che
//! servono ai comandi Tauri. Il contratto e' `docs/BOARD.MD`; tipi,
//! validazione e scheduler puri sono in `crate::board`.

use super::*;
use crate::board::{
    self, EnvInfo, Isolation, ReportFields, SchedCtx, Task, TaskStatus, TaskView, WorktreeInfo,
};
use super::hooks::{self, HOOK_TIMEOUT};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Esito di un comando git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Avvio di un task scelto dallo scheduler: tutto cio' che serve fuori dal
/// lock (git + spawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartJob {
    pub ws: Workspace,
    pub task: Task,
    pub board_base: Option<String>,
    /// Branch base per repository (impostazione della board).
    pub board_bases: BTreeMap<String, String>,
    /// Modello del branch di integrazione, se la board lo gestisce.
    pub integration_template: Option<String>,
    /// `attempts` al momento della scelta: scarta avvii superati da un
    /// cancel/retry.
    pub attempt: u32,
}

/// Impostazioni multi-repository di `set_board` (`submit_plan` le porta nel
/// `PlanInput`).
#[derive(Debug, Clone, Default)]
pub struct RepoSettings {
    pub base_branches: board::BaseBranchOps,
    /// `None` = non toccare; `Some(None)` = spegni.
    pub integration_branch: Option<Option<String>>,
    /// Chiavi dei repository di cui chiudere l'integrazione registrata.
    pub close_integration: Vec<String>,
}

/// Chiave del repository di `close_integration` ("." = root).
fn close_key(s: &str) -> Result<String, String> {
    Ok(board::repo_key(board::normalize_repo(s)?.as_deref()))
}

impl RepoSettings {
    fn is_empty(&self) -> bool {
        self.base_branches.is_empty() && self.integration_branch.is_none() && self.close_integration.is_empty()
    }

    fn parse(params: &Value) -> Result<RepoSettings, String> {
        let base_branches = match params.get("base_branches") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => board::parse_base_branches(v)?,
        };
        let integration_branch = match params.get("integration_branch") {
            None => None,
            Some(v) => Some(board::parse_integration_template(v)?),
        };
        const BAD: &str = "close_integration must be a repo key or an array of repo keys";
        let close_integration = match params.get("close_integration") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(s)) => vec![close_key(s)?],
            Some(Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().ok_or_else(|| BAD.to_string()).and_then(close_key))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(BAD.to_string()),
        };
        Ok(RepoSettings { base_branches, integration_branch, close_integration })
    }
}

const EXCLUDE_LINE: &str = ".worktrees/";
/// Quanto vale un `git fetch` gia' fatto per (repo, base).
const FETCH_TTL: Duration = Duration::from_secs(60);
/// Tetto di un `git fetch` (il resto dei comandi git ha `GIT_TIMEOUT`).
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Cache dei fetch: (workspace|cartella|base) -> (istante, avviso).
pub(super) type FetchCache = HashMap<String, (std::time::Instant, Option<String>)>;

// ---------------------------------------------------------------------------
// Registry: board
// ---------------------------------------------------------------------------

impl Registry {
    pub fn load_board(&mut self, mut board: Board) {
        self.next_rev += 1;
        board.rev = self.next_rev;
        self.boards.insert(board.workspace_id.clone(), board);
    }

    /// Board del workspace (vuota se non esiste).
    pub fn board_view(&self, ws: &str) -> BoardView {
        let mut v = self.boards.get(ws).map(Board::view).unwrap_or_else(|| Board::new(ws).view());
        v.automation = self.auto.info.get(ws).cloned();
        v
    }

    pub fn task_view(&self, ws: &str, id: &str) -> Option<TaskView> {
        self.boards.get(ws).and_then(|b| b.task_view(id))
    }

    pub(super) fn mark_dirty(&mut self, ws: &str) {
        if !self.dirty.iter().any(|d| d == ws) {
            self.dirty.push(ws.to_string());
        }
    }

    /// Le board di cui `orch` e' proprietario tornano da schedulare: un posto
    /// nel tetto di worker si e' liberato.
    pub(super) fn mark_owner_boards_dirty(&mut self, orch: &str) {
        if orch.is_empty() {
            return;
        }
        let ids: Vec<String> =
            self.boards.iter().filter(|(_, b)| b.owner.as_deref() == Some(orch)).map(|(k, _)| k.clone()).collect();
        for id in ids {
            self.mark_dirty(&id);
        }
    }

    fn board_ref(&self, ws: &str) -> Result<&Board, String> {
        self.boards.get(ws).ok_or_else(|| "task not found: the board is empty".to_string())
    }

    /// Chi chiama un tool di board ne diventa il proprietario: i worker vivi
    /// della board passano a lui e con loro gli eventi in coda (anche quelli
    /// rimasti in attesa senza proprietario).
    pub fn claim_board(&mut self, ws: &str, orch: &str) -> Result<(), String> {
        self.require_orchestrator(orch)?;
        let b = self.boards.entry(ws.to_string()).or_insert_with(|| Board::new(ws));
        let old = b.owner.replace(orch.to_string());
        if old.as_deref() == Some(orch) {
            return Ok(());
        }
        // `owner` e' nella vista.
        self.mark_dirty(ws);
        let sessions: Vec<String> = self
            .workers
            .values()
            .filter(|w| w.board_task.as_ref().is_some_and(|t| t.0 == ws))
            .map(|w| w.session.clone())
            .collect();
        for s in sessions {
            if let Some(w) = self.workers.get_mut(&s) {
                w.orchestrator = orch.to_string();
                if w.tab_open {
                    let info = w.info();
                    self.effects.push(Effect::Update(info));
                }
            }
        }
        let mut moved: Vec<Event> = self.board_queues.remove(ws).map(Vec::from_iter).unwrap_or_default();
        if let Some(q) = old.filter(|o| !o.is_empty()).and_then(|o| self.queues.get_mut(&o)) {
            let mut keep = VecDeque::new();
            for e in q.events.drain(..) {
                if e.board.as_deref() == Some(ws) {
                    moved.push(e);
                } else {
                    keep.push_back(e);
                }
            }
            q.events = keep;
        }
        if !moved.is_empty() {
            let q = self.queues.entry(orch.to_string()).or_default();
            q.events.extend(moved);
            q.events.make_contiguous().sort_by_key(|e| e.seq);
            while q.events.len() > MAX_QUEUE_EVENTS {
                q.events.pop_front();
            }
        }
        self.check_wake(orch);
        Ok(())
    }

    pub fn submit_plan(
        &mut self,
        ws: &str,
        plan: board::PlanInput,
        ceiling: PermissionMode,
        now: i64,
    ) -> Result<Vec<String>, String> {
        let b = self.boards.entry(ws.to_string()).or_insert_with(|| Board::new(ws));
        // Atomico: si lavora su una copia.
        let mut nb = b.clone();
        if let Some(m) = plan.max_parallel {
            nb.max_parallel = m;
        }
        if plan.base_branch.is_some() {
            nb.base_branch = plan.base_branch;
        }
        nb.apply_repo_settings(plan.base_branches, plan.integration_branch);
        let ids = nb.add_plan(plan.specs, ceiling, now)?;
        *b = nb;
        self.mark_dirty(ws);
        Ok(ids)
    }

    pub fn update_task(
        &mut self,
        ws: &str,
        id: &str,
        patch: &Value,
        ceiling: PermissionMode,
        now: i64,
    ) -> Result<(), String> {
        self.board_ref(ws)?;
        let b = self.boards.get_mut(ws).expect("board presente");
        b.update_task(id, patch, ceiling, now)?;
        self.mark_dirty(ws);
        Ok(())
    }

    /// `set_task_status` / azioni dell'utente. Ritorna il worktree del task
    /// se va ripulito (solo per `done`; la pulizia git la fa chi chiama,
    /// fuori dal lock).
    pub fn set_task_status(
        &mut self,
        ws: &str,
        id: &str,
        target: &str,
        note: Option<String>,
        now: i64,
    ) -> Result<Option<WorktreeInfo>, String> {
        let to = match target {
            "done" => TaskStatus::Done,
            "cancelled" => TaskStatus::Cancelled,
            "queued" => TaskStatus::Queued,
            _ => return Err("invalid status (use done, cancelled or queued)".to_string()),
        };
        let b = self.board_ref(ws)?;
        b.check_transition(id, to)?;
        let t = b.task(id).ok_or_else(|| format!("task not found: {id}"))?;
        let (was_running, session, worktree) = (t.status == TaskStatus::Running, t.session.clone(), t.worktree.clone());
        if to == TaskStatus::Cancelled && was_running {
            if let Some(s) = session {
                self.stop_worker(&s, now);
            }
        }
        let b = self.boards.get_mut(ws).expect("board presente");
        b.apply_status(id, to, note, now)?;
        self.mark_dirty(ws);
        Ok(if to == TaskStatus::Done { worktree } else { None })
    }

    pub fn set_held(&mut self, ws: &str, id: &str, held: bool, now: i64) -> Result<(), String> {
        self.board_ref(ws)?;
        let b = self.boards.get_mut(ws).expect("board presente");
        b.set_held(id, held, now)?;
        self.mark_dirty(ws);
        Ok(())
    }

    pub fn set_board(&mut self, ws: &str, max_parallel: Option<u8>, paused: Option<bool>, now: i64) -> Result<(), String> {
        self.set_board_ext(ws, max_parallel, paused, RepoSettings::default(), now)
    }

    /// Come `set_board`, con in piu' `base_branches` e `integration_branch`.
    pub fn set_board_ext(
        &mut self,
        ws: &str,
        max_parallel: Option<u8>,
        paused: Option<bool>,
        repo: RepoSettings,
        now: i64,
    ) -> Result<(), String> {
        if max_parallel.is_none() && paused.is_none() && repo.is_empty() {
            return Err("nothing to change: pass max_parallel, paused, base_branches, integration_branch and/or close_integration".to_string());
        }
        if let Some(m) = max_parallel {
            board::parse_max_parallel(&json!(m))?;
        }
        let b = self.boards.entry(ws.to_string()).or_insert_with(|| Board::new(ws));
        // Prima di toccare qualcosa: una chiave sconosciuta annulla l'intera richiesta.
        if let Some(k) = repo.close_integration.iter().find(|k| !b.integrations.contains_key(*k)) {
            return Err(format!("no integration registered for repo '{k}' (see integrations in get_board)"));
        }
        for k in &repo.close_integration {
            b.integrations.remove(k);
        }
        if !repo.is_empty() {
            b.apply_repo_settings(repo.base_branches, repo.integration_branch);
        }
        b.repos = None; // i repository si ricalcolano alla prossima get_board
        if let Some(m) = max_parallel {
            b.max_parallel = m;
        }
        if let Some(p) = paused {
            b.paused = p;
        }
        b.updated_at = now;
        self.mark_dirty(ws);
        Ok(())
    }

    /// Integrazione gia' risolta del repository (chiave "." = root).
    pub fn integration_of(&self, ws: &str, key: &str) -> Option<board::Integration> {
        self.boards.get(ws).and_then(|b| b.integrations.get(key).cloned())
    }

    /// Salva l'integrazione risolta di un repository.
    pub fn set_integration(&mut self, ws: &str, key: &str, integ: board::Integration, now: i64) {
        if let Some(b) = self.boards.get_mut(ws) {
            b.integrations.insert(key.to_string(), integ);
            b.updated_at = now;
            self.mark_dirty(ws);
        }
    }

    /// Repository rilevati in cache (None = da calcolare).
    pub fn repos_cached(&self, ws: &str) -> Option<Vec<String>> {
        self.boards.get(ws).and_then(|b| b.repos.clone())
    }

    /// Mette in cache i repository rilevati. Non e' una modifica della
    /// board: niente salvataggio; li vede la prossima pubblicazione.
    pub fn set_repos(&mut self, ws: &str, repos: Vec<String>) {
        if let Some(b) = self.boards.get_mut(ws) {
            b.repos = Some(repos);
        }
    }

    /// Nota del task (avvisi di pulizia).
    pub fn add_task_note(&mut self, ws: &str, id: &str, note: &str, now: i64) {
        if let Some(t) = self.boards.get_mut(ws).and_then(|b| b.task_mut(id)) {
            t.note = Some(clip_chars(note, 2000));
            t.updated_at = now;
            self.mark_dirty(ws);
        }
    }

    pub fn remove_board(&mut self, ws: &str) {
        if self.boards.remove(ws).is_some() {
            self.board_queues.remove(ws);
            self.dirty.retain(|d| d != ws);
            self.next_rev += 1;
            self.effects.push(Effect::DeleteBoard(ws.to_string(), self.next_rev));
        }
    }

    /// Evento di board senza worker (start_failed, plan_completed,
    /// user_action): va al proprietario o aspetta nella coda della board.
    pub fn push_board_event(
        &mut self,
        ws: &str,
        task: Option<&str>,
        kind: EventKind,
        text: String,
        status: WorkerStatus,
        now: i64,
    ) {
        let Some(b) = self.boards.get(ws) else {
            return;
        };
        let owner = b.owner.clone().filter(|o| !o.is_empty());
        let title = task
            .and_then(|id| b.task(id))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "board".to_string());
        self.next_seq += 1;
        let ev = Event {
            seq: self.next_seq,
            session: String::new(),
            title,
            kind,
            text: Some(text),
            status,
            at: now,
            task: task.map(str::to_string),
            board: Some(ws.to_string()),
        };
        self.enqueue(owner, ev);
    }

    /// L'utente ha agito dalla board: evento non azionabile per l'owner.
    pub fn push_user_action(&mut self, ws: &str, task_id: &str, text: String, now: i64) {
        let status = self
            .boards
            .get(ws)
            .and_then(|b| b.task(task_id))
            .and_then(|t| t.worker_status)
            .unwrap_or(WorkerStatus::Closed);
        self.push_board_event(ws, Some(task_id), EventKind::UserAction, text, status, now);
    }

    // --- scheduler ---

    /// Schedula, pubblica e notifica le board modificate. `lookup` risolve i
    /// workspace (senza, nulla parte).
    /// `minute`: ora locale di `now`, per il gate della finestra calcolato
    /// adesso e non al giro del timer (fino a 30 s fa).
    pub fn flush_boards(&mut self, now: i64, minute: u16, lookup: &dyn Fn(&str) -> Option<Workspace>) {
        let dirty = std::mem::take(&mut self.dirty);
        let open = crate::schedule::window_open(&self.auto.sched, minute);
        for ws_id in dirty {
            self.flush_one(&ws_id, now, open, lookup);
        }
    }

    fn flush_one(&mut self, ws_id: &str, now: i64, open: bool, lookup: &dyn Fn(&str) -> Option<Workspace>) {
        if !self.boards.contains_key(ws_id) {
            return;
        }
        if let Some(ws) = lookup(ws_id) {
            let b = &self.boards[ws_id];
            let max_start = match b.owner.as_deref().filter(|o| !o.is_empty()) {
                Some(o) => MAX_WORKERS_PER_ORCHESTRATOR.saturating_sub(self.active_count(o) + b.pending_starts()),
                None => MAX_WORKERS_PER_ORCHESTRATOR,
            };
            let ctx = SchedCtx {
                case_insensitive: ws.kind != "wsl",
                ceiling: ws.worker_permission_mode(),
                max_start,
            };
            // Gate della finestra: separato dalla pausa, che non si tocca.
            let chosen = if self.window_blocks(&ws, open) { Vec::new() } else { board::schedule(b, &ctx) };
            let b = self.boards.get_mut(ws_id).expect("board presente");
            for id in chosen {
                // Subito `running`: un'altra esecuzione non lo sceglie due volte.
                b.start_task(&id, now);
                if let Some(task) = b.task(&id).cloned() {
                    let job = StartJob {
                        ws: ws.clone(),
                        attempt: task.attempts,
                        task,
                        board_base: b.base_branch.clone(),
                        board_bases: b.base_branches.clone(),
                        integration_template: b.integration_branch.clone(),
                    };
                    self.effects.push(Effect::StartTask(Box::new(job)));
                }
            }
        }
        // Piano completato: una volta per transizione.
        let b = self.boards.get_mut(ws_id).expect("board presente");
        let complete = b.is_complete();
        let fire = complete && !b.plan_completed_sent;
        b.plan_completed_sent = complete;
        // Con tutti i task annullati senza essere mai partiti non c'e' nulla da
        // riferire: nessun evento (`completion_text` ritorna `None`).
        let text = if fire { b.completion_text() } else { None };
        if let Some(text) = text {
            self.push_board_event(ws_id, None, EventKind::PlanCompleted, text, WorkerStatus::Completed, now);
        }
        let b = self.boards.get_mut(ws_id).expect("board presente");
        b.automation = self.auto.info.get(ws_id).cloned();
        // Ogni pubblicazione e' una modifica della vista: nuova revisione.
        self.next_rev += 1;
        b.rev = self.next_rev;
        b.updated_at = now;
        let snapshot = b.clone();
        self.effects.push(Effect::Publish(Box::new(snapshot)));
    }

    // --- worker <-> task ---

    /// Riflette lo stato del worker sul suo task (se e' ancora il suo).
    pub(super) fn sync_task(&mut self, session: &str, now: i64) {
        let Some(w) = self.workers.get(session) else {
            return;
        };
        let Some((ws, tid)) = w.board_task.clone() else {
            return;
        };
        let (status, last) = (w.status, w.last_report.clone());
        let changed = self
            .boards
            .get_mut(&ws)
            .and_then(|b| b.task_mut(&tid))
            .filter(|t| t.status == TaskStatus::Running && t.session.as_deref() == Some(session))
            .map(|t| {
                t.worker_status = Some(status);
                if last.is_some() {
                    t.last_report = last;
                }
                t.updated_at = now;
            })
            .is_some();
        if changed {
            self.mark_dirty(&ws);
        }
    }

    /// `complete_task` di un worker di board: il task passa a review /
    /// failed / blocked con il report.
    pub(super) fn finish_board_task(
        &mut self,
        session: &str,
        status: &str,
        summary: &str,
        fields: ReportFields,
        diffstat: Option<String>,
        now: i64,
    ) {
        let Some((ws, tid)) = self.workers.get(session).and_then(|w| w.board_task.clone()) else {
            return;
        };
        let to = match status {
            "completed" => TaskStatus::Review,
            "failed" => TaskStatus::Failed,
            _ => TaskStatus::Blocked,
        };
        let report = board::build_report(status, summary, fields, diffstat);
        let done = self
            .boards
            .get_mut(&ws)
            .is_some_and(|b| b.finish_running(&tid, Some(session), to, Some(report), None, now));
        if done {
            self.mark_dirty(&ws);
        }
    }

    /// Il worker e' finito senza report (tab chiuso, sessione terminata): il
    /// task `running` diventa `failed` con una nota.
    pub(super) fn fail_board_task(&mut self, session: &str, note: &str, now: i64) {
        let Some((ws, tid)) = self.workers.get(session).and_then(|w| w.board_task.clone()) else {
            return;
        };
        let done = self.boards.get_mut(&ws).is_some_and(|b| {
            b.finish_running(&tid, Some(session), TaskStatus::Failed, None, Some(note.to_string()), now)
        });
        if done {
            self.mark_dirty(&ws);
        }
    }

    /// Chiude il tab di un worker (cancel di un task `running`) senza il
    /// rimbalzo di un evento `exited`.
    fn stop_worker(&mut self, session: &str, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        if !w.tab_open {
            return;
        }
        if !w.status.is_terminal() {
            w.status = WorkerStatus::Closed;
        }
        self.touch(session, now);
        self.effects.push(Effect::Close { tab_id: session.to_string(), delay: Duration::ZERO });
    }

    /// Testo `environment` di `get_task` per un worker di board.
    pub(super) fn board_environment(&self, session: &str, ws: Option<&Workspace>) -> Option<String> {
        let w = self.workers.get(session)?;
        let (ws_id, tid) = w.board_task.as_ref()?;
        let ws = ws?;
        let t = self.boards.get(ws_id)?.task(tid)?;
        let repo_dir = repo_dir(ws, t.repo.as_deref());
        let workdir = match (&t.worktree, t.isolation) {
            (Some(wt), Isolation::Worktree) => wt.path.as_str(),
            _ => repo_dir.as_str(),
        };
        // Il setup serve ai worktree (cartella nuova, senza dipendenze).
        let setup = ws.worktree_setup.as_deref().filter(|_| t.isolation == Isolation::Worktree);
        let repo_cfg = ws.repos.get(&board::repo_key(t.repo.as_deref()));
        Some(board::worker_environment(&EnvInfo { root: &ws.path, workdir, task: t, setup, repo_cfg }))
    }

    /// Passo di preparazione in corso di un avvio (vedi `Task::preparing`).
    /// `label` `None` = solo battito per il guardiano. `publish`: pubblica
    /// subito la vista (non sotto il lock dei repository: un host sincrono
    /// ripartirebbe da qui).
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

    /// Registra il worker di un task appena preparato (git fatto). `None` se
    /// l'avvio e' stato superato (cancel, retry) o il tetto e' pieno (il
    /// task torna in coda).
    #[allow(clippy::too_many_arguments)]
    pub fn register_task_worker(
        &mut self,
        ws: &str,
        tid: &str,
        attempt: u32,
        worktree: Option<WorktreeInfo>,
        warnings: Vec<String>,
        new: NewWorker,
        now: i64,
    ) -> Option<SpawnEvent> {
        let b = self.boards.get(ws)?;
        let t = b.task(tid)?;
        if t.status != TaskStatus::Running || t.attempts != attempt || t.session.is_some() {
            return None;
        }
        let owner = b.owner.clone().unwrap_or_default();
        if !owner.is_empty() && self.check_spawn(&owner).is_err() {
            // Tetto di worker pieno nel frattempo (spawn ad-hoc): riprova dopo.
            if let Some(t) = self.boards.get_mut(ws).and_then(|b| b.task_mut(tid)) {
                t.status = TaskStatus::Queued;
                t.worker_status = None;
                t.attempts = t.attempts.saturating_sub(1);
                t.started_at = None;
                t.updated_at = now;
            }
            self.mark_dirty(ws);
            return None;
        }
        let ev = self.spawn_inner(&owner, new, now, Some((ws.to_string(), tid.to_string())));
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
                t.note = Some(clip_chars(&format!("start warnings: {}", warnings.join("; ")), 2000));
            }
            t.updated_at = now;
        }
        self.mark_dirty(ws);
        Some(ev)
    }

    /// Guardiano (giro periodico): un task `running` senza sessione da troppo
    /// (thread di avvio bloccato) diventa `failed` come un avvio fallito; un
    /// worker di board che non ha mai chiamato `get_task` (tab mai
    /// comparso, agente non partito) fallisce il task e chiude il tab.
    pub fn watchdog(&mut self, now: i64) {
        for (ws, tid, attempt) in stuck_starts(self.boards.values(), now) {
            self.start_failed(&ws, &tid, attempt, "the worker did not start in time (start timed out)", now);
        }
        let silent: Vec<String> = self
            .workers
            .values()
            .filter(|w| {
                w.board_task.is_some()
                    && w.tab_open
                    && silent_worker(w.status == WorkerStatus::Starting, w.started_sent, w.created_at, now)
            })
            .map(|w| w.session.clone())
            .collect();
        for s in silent {
            self.abandon_worker(&s, "the worker never called get_task: the agent did not start", now);
        }
    }

    /// Chiude d'ufficio un worker di board: `exited` all'orchestratore, task
    /// `failed` con la nota, tab chiuso.
    fn abandon_worker(&mut self, session: &str, note: &str, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        w.status = WorkerStatus::Closed;
        w.tab_open = false;
        let orch = w.orchestrator.clone();
        self.push_event(session, EventKind::Exited, Some(note.to_string()), now);
        self.touch(session, now);
        self.fail_board_task(session, note, now);
        self.effects.push(Effect::Close { tab_id: session.to_string(), delay: Duration::ZERO });
        self.mark_owner_boards_dirty(&orch);
    }

    /// L'avvio `attempt` del task e' ancora quello in corso (running, senza
    /// sessione)?
    pub fn start_is_current(&self, ws: &str, tid: &str, attempt: u32) -> bool {
        self.boards
            .get(ws)
            .and_then(|b| b.task(tid))
            .is_some_and(|t| t.status == TaskStatus::Running && t.attempts == attempt && t.session.is_none())
    }

    /// L'avvio del task e' fallito: `failed` con la nota e un evento
    /// azionabile `start_failed`.
    pub fn start_failed(&mut self, ws: &str, tid: &str, attempt: u32, msg: &str, now: i64) {
        let note = clip_chars(msg, 2000);
        let done = self.boards.get_mut(ws).is_some_and(|b| {
            b.task(tid).is_some_and(|t| t.attempts == attempt && t.session.is_none())
                && b.finish_running(tid, None, TaskStatus::Failed, None, Some(note.clone()), now)
        });
        if done {
            self.mark_dirty(ws);
            self.push_board_event(
                ws,
                Some(tid),
                EventKind::StartFailed,
                format!("Task '{tid}' could not be started: {note}"),
                WorkerStatus::Failed,
                now,
            );
        }
    }
}

/// Tempo concesso a un avvio (thread git, poi `get_task` del worker).
pub(super) const START_TIMEOUT_MS: i64 = 5 * 60_000;

/// Un passo di preparazione (setup del repository) ha un tetto suo
/// (`HOOK_TIMEOUT`): finche' il thread di avvio ne ha uno in corso il
/// guardiano aspetta quel tempo piu' un margine, contato dall'ultimo battito
/// (`Task::preparing_since`, aggiornato a ogni passo e mentre si aspetta il
/// lock dei repository). Il tempo dei setup non consuma quindi i 5 minuti
/// dell'avvio; un thread morto a meta' passo viene comunque scoperto.
pub(super) const PREPARING_TIMEOUT_MS: i64 = HOOK_TIMEOUT.as_millis() as i64 + 2 * 60_000;

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

/// Worker `starting` che non ha mai chiamato `get_task` da oltre il tempo
/// concesso.
pub(super) fn silent_worker(starting: bool, called_get_task: bool, created_at: i64, now: i64) -> bool {
    starting && !called_get_task && now - created_at > START_TIMEOUT_MS
}

impl AutomationState {
    /// Workspace in cui lavora il worker (di board o ad-hoc).
    pub(super) fn worker_workspace_of(&self, session: &str) -> Option<String> {
        let reg = self.inner.lock().unwrap();
        reg.workers.get(session).map(|w| w.workspace_id.clone())
    }

    /// Worktree del task del worker (per il diffstat), se ne ha uno.
    pub(super) fn board_worktree_of(&self, session: &str) -> Option<(String, WorktreeInfo)> {
        let reg = self.inner.lock().unwrap();
        let (ws, tid) = reg.workers.get(session)?.board_task.clone()?;
        let t = reg.boards.get(&ws)?.task(&tid)?;
        t.worktree.clone().filter(|_| t.isolation == Isolation::Worktree).map(|w| (ws, w))
    }
}

// ---------------------------------------------------------------------------
// Git (generico sull'Host: si prova con un host finto)
// ---------------------------------------------------------------------------

/// Esegue git e ritorna lo stdout (trim); fallimento = errore leggibile.
fn git<H: Host>(host: &H, ws: &Workspace, dir: &str, args: &[&str]) -> Result<String, String> {
    let out = host.git(ws, dir, args).map_err(|e| format!("git is not available: {e}"))?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        let why = if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() };
        Err(format!("git {} failed: {}", args.join(" "), clip_chars(why, 500)))
    }
}

/// Unisce un path relativo (con `/`) al path della root nell'ambiente del
/// workspace: `\` su Windows, `/` su WSL.
pub(super) fn join_path(kind: &str, root: &str, rel: &str) -> String {
    let sep = if kind == "wsl" { '/' } else { '\\' };
    let root = root.trim_end_matches(['/', '\\']);
    let rel: String = rel.chars().map(|c| if c == '/' { sep } else { c }).collect();
    format!("{root}{sep}{rel}")
}

fn is_abs(kind: &str, p: &str) -> bool {
    cwd_is_absolute(kind, p)
}

/// Cartella del repository di un task nell'ambiente del workspace: la root
/// se `repo` e' `None`, altrimenti `repo` sotto la root.
pub(super) fn repo_dir(ws: &Workspace, repo: Option<&str>) -> String {
    match repo {
        Some(r) => join_path(&ws.kind, &ws.path, r),
        None => ws.path.clone(),
    }
}

/// La cartella del repository (la root, o `repo` sotto la root) e' dentro un
/// repo git?
pub(super) fn ensure_git_repo<H: Host>(host: &H, ws: &Workspace, repo: Option<&str>) -> Result<(), String> {
    let dir = repo_dir(ws, repo);
    if let Some(r) = repo {
        if !host.dir_exists(ws, &dir) {
            return Err(format!("the repository folder '{r}' ({dir}) does not exist in the workspace"));
        }
    }
    match host.git(ws, &dir, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(o) if o.ok && o.stdout.trim() == "true" => Ok(()),
        Ok(_) => Err(match repo {
            None => format!(
                "the workspace folder ({dir}) is not a git repository: tasks with isolation \"worktree\" need one. \
                 Use isolation \"none\" or ask the user to run git init"
            ),
            Some(r) => format!(
                "the repository folder '{r}' ({dir}) is not a git repository: tasks with isolation \"worktree\" need one. \
                 Fix the repo of the task, use isolation \"none\" or ask the user to run git init there"
            ),
        }),
        Err(e) => Err(format!("git is not available: {e}")),
    }
}

/// `ensure_git_repo` per ogni repository distinto.
pub(super) fn ensure_git_repos<'a, H: Host>(
    host: &H,
    ws: &Workspace,
    repos: impl IntoIterator<Item = Option<&'a str>>,
) -> Result<(), String> {
    let mut seen: Vec<Option<&str>> = Vec::new();
    for r in repos {
        if !seen.contains(&r) {
            seen.push(r);
            ensure_git_repo(host, ws, r)?;
        }
    }
    Ok(())
}

/// Base dichiarata, senza interrogare git: task -> impostazione per
/// repository della board -> impostazione del repository nel workspace ->
/// base della board (solo task della root).
pub(super) fn declared_base(
    task_base: Option<&str>,
    repo: Option<&str>,
    bases: &BTreeMap<String, String>,
    settings_base: Option<&str>,
    board_base: Option<&str>,
) -> Option<String> {
    task_base
        .map(str::to_string)
        .or_else(|| bases.get(&board::repo_key(repo)).cloned())
        .or_else(|| settings_base.map(str::to_string))
        .or_else(|| board_base.filter(|_| repo.is_none()).map(str::to_string))
}

/// Configurazione del repository del task nelle impostazioni del workspace.
fn repo_cfg<'a>(ws: &'a Workspace, repo: Option<&str>) -> Option<&'a crate::workspaces::RepoConfig> {
    ws.repos.get(&board::repo_key(repo))
}

/// Il lock dei repository, battendo il cuore del task mentre si aspetta:
/// un altro avvio puo' tenerlo per un setup lungo e il guardiano non deve
/// scambiare l'attesa per un thread bloccato.
fn lock_repo_prep<'a>(st: &'a AutomationState, job: &StartJob) -> std::sync::MutexGuard<'a, FetchCache> {
    use std::sync::TryLockError;
    loop {
        match st.repo_prep.try_lock() {
            Ok(g) => return g,
            Err(TryLockError::Poisoned(e)) => return e.into_inner(),
            Err(TryLockError::WouldBlock) => {
                st.inner.lock().unwrap().touch_preparing(&job.ws.id, &job.task.id, job.attempt, None, now_millis(), false);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// Setup di un worktree appena creato (task o integrazione), se il
/// repository ne ha uno. `publish`: pubblica ogni passo (non sotto il lock
/// dei repository). Il passo in corso e' il battito del guardiano.
#[allow(clippy::too_many_arguments)]
fn setup_worktree<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    worktree: &str,
    task_id: &str,
    branch: &str,
    base: &str,
    subject: &str,
    publish: bool,
) -> Result<Vec<String>, String> {
    let ws = &job.ws;
    let repo = job.task.repo.as_deref();
    let Some(cfg) = repo_cfg(ws, repo).filter(|c| c.has_setup()) else {
        return Ok(Vec::new());
    };
    let key = board::repo_key(repo);
    let ctx = hooks::SetupCtx { ws, cfg, repo: &key, repo_dir: dir, worktree, task_id, branch, base, subject };
    let report = |label: &str| {
        let (ws_id, tid, attempt) = (job.ws.id.as_str(), job.task.id.as_str(), job.attempt);
        if publish {
            st.run(host, |r| r.touch_preparing(ws_id, tid, attempt, Some(label), now_millis(), true));
        } else {
            st.inner.lock().unwrap().touch_preparing(ws_id, tid, attempt, Some(label), now_millis(), false);
        }
    };
    hooks::run_setup(host, &ctx, &report)
}

/// Base di un repository: quella dichiarata, altrimenti (repo) il default del
/// remoto `origin`, altrimenti il branch corrente della cartella.
fn resolve_base<H: Host>(
    host: &H,
    ws: &Workspace,
    dir: &str,
    repo: Option<&str>,
    declared: Option<String>,
) -> Result<String, String> {
    if let Some(b) = declared {
        return Ok(b);
    }
    if repo.is_some() {
        if let Ok(r) = git(host, ws, dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
            let b = r.strip_prefix("origin/").unwrap_or(&r);
            if board::valid_branch(b) {
                return Ok(b.to_string());
            }
        }
    }
    let cur = git(host, ws, dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if cur.is_empty() || cur == "HEAD" {
        return Err(match repo {
            None => "the workspace is in detached HEAD state: check out a branch or set base_branch".to_string(),
            Some(r) => format!(
                "the repository '{r}' is in detached HEAD state: check out a branch or set base_branch / base_branches"
            ),
        });
    }
    Ok(cur)
}

fn ref_exists<H: Host>(host: &H, ws: &Workspace, dir: &str, name: &str) -> bool {
    host.git(ws, dir, &["rev-parse", "--verify", "--quiet", name]).map(|o| o.ok).unwrap_or(false)
}

/// `git fetch origin <base>` (al massimo una volta ogni `FETCH_TTL` per
/// repository e base; salta se manca `origin`) e riferimento da cui creare:
/// `origin/<base>` se esiste, altrimenti il branch locale. Un fetch fallito
/// non blocca: si prosegue con i riferimenti locali e c'e' un avviso.
fn remote_or_local<H: Host>(
    host: &H,
    ws: &Workspace,
    dir: &str,
    base: &str,
    cache: &mut FetchCache,
    warnings: &mut Vec<String>,
) -> String {
    let key = format!("{}|{dir}|{base}", ws.id);
    let warn = match cache.get(&key).filter(|(at, _)| at.elapsed() < FETCH_TTL) {
        Some((_, w)) => w.clone(),
        None => {
            let has_origin = host
                .git(ws, dir, &["remote", "get-url", "origin"])
                .map(|o| o.ok)
                .unwrap_or(false);
            let warn = if !has_origin {
                None
            } else {
                match host.git(ws, dir, &["fetch", "origin", base]) {
                    Ok(o) if o.ok => None,
                    Ok(o) => Some(format!(
                        "git fetch origin {base} failed ({}): using the local refs",
                        clip_chars(o.stderr.trim(), 200)
                    )),
                    Err(e) => Some(format!("git fetch origin {base} failed ({e}): using the local refs")),
                }
            };
            cache.insert(key, (std::time::Instant::now(), warn.clone()));
            warn
        }
    };
    if let Some(w) = warn {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    let remote = format!("origin/{base}");
    if ref_exists(host, ws, dir, &format!("refs/remotes/{remote}")) {
        remote
    } else {
        base.to_string()
    }
}

/// `.worktrees/` fuori dal git status del repository, senza toccare .gitignore.
fn add_worktrees_exclude<H: Host>(host: &H, ws: &Workspace, dir: &str, warnings: &mut Vec<String>) {
    match git(host, ws, dir, &["rev-parse", "--git-path", "info/exclude"]) {
        Ok(rel) => {
            let file = if is_abs(&ws.kind, &rel) { rel } else { join_path(&ws.kind, dir, &rel) };
            if let Err(e) = host.append_exclude(ws, &file, EXCLUDE_LINE) {
                warnings.push(format!("{EXCLUDE_LINE} not added to the git exclude file ({e}): it may show up in git status"));
            }
        }
        Err(e) => warnings.push(format!("git exclude file not found ({e}): {EXCLUDE_LINE} may show up in git status")),
    }
}

/// Data locale (YYYY-MM-DD) per il modello del branch di integrazione.
fn local_date(now: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(now)
        .single()
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Garantisce il branch di integrazione del repository del task con il suo
/// worktree `<repo>/.worktrees/integration` e lo registra nella board. Va
/// chiamata con il lock dei repository preso (un'integrazione per volta).
/// Mai checkout/switch nella cartella principale.
fn ensure_integration<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    template: &str,
    cache: &mut FetchCache,
    warnings: &mut Vec<String>,
) -> Result<board::Integration, String> {
    let (ws, repo) = (&job.ws, job.task.repo.as_deref());
    let key = board::repo_key(repo);
    let now = now_millis();
    let name = board::expand_integration(template, &local_date(now))?;
    // Lock del registro senza `run`: niente flush (un avvio inline sotto il
    // lock dei repository andrebbe in stallo).
    let current = st.inner.lock().unwrap().integration_of(&ws.id, &key);
    // Riferimento di base del repository (dopo il fetch): serve a creare e a
    // capire se un'integrazione sparita e' gia' stata unita.
    let mut base_ref_of = |warnings: &mut Vec<String>| -> Result<String, String> {
        let declared = declared_base(None, repo, &job.board_bases, settings_base(job), job.board_base.as_deref());
        let base = resolve_base(host, ws, dir, repo, declared)?;
        Ok(if repo.is_some() { remote_or_local(host, ws, dir, &base, cache, warnings) } else { base })
    };
    let mut base_ref = None;
    if let Some(mut i) = current.filter(|i| board::integration_matches(&name, &i.branch)) {
        let gone = !host.dir_exists(ws, &i.path);
        let exists = ref_exists(host, ws, dir, &format!("refs/heads/{}", i.branch));
        // Cartella e branch spariti: niente da riprendere, si riparte dalla base attuale.
        let mut merged = gone && !exists;
        if gone && exists {
            if let Ok(b) = base_ref_of(warnings) {
                merged = host
                    .git(ws, dir, &["merge-base", "--is-ancestor", &i.branch, &b])
                    .map(|o| o.ok)
                    .unwrap_or(false);
                base_ref = Some(b);
            }
        }
        if merged {
            // Cartella sparita e branch cancellato o gia' nella base: integrazione finita, se ne crea una nuova.
            warnings.push(format!(
                "integration {} is gone or already merged into the base: starting a new integration",
                i.branch
            ));
        } else {
            if gone {
                // Cartella sparita: si ricrea dal branch, che e' nostro (e va riprepararla).
                warnings.push(format!(
                    "integration folder {} was missing: worktree recreated on branch {}",
                    i.path, i.branch
                ));
                add_worktrees_exclude(host, ws, dir, warnings);
                let _ = git(host, ws, dir, &["worktree", "prune"]);
                git(host, ws, dir, &["worktree", "add", &i.path, &i.branch])?;
                i.setup_done = false;
                st.inner.lock().unwrap().set_integration(&ws.id, &key, i.clone(), now);
            }
            setup_integration(st, host, job, dir, &mut i, warnings)?;
            return Ok(i);
        }
    }
    let base_ref = match base_ref {
        Some(b) => b,
        None => base_ref_of(warnings)?,
    };
    // Un branch con questo nome che non e' nostro (non registrato) non si riusa.
    let branch = board::first_free_branch(&name, |c| ref_exists(host, ws, dir, &format!("refs/heads/{c}")))
        .ok_or_else(|| format!("no free name for the integration branch {name}"))?;
    let path = (1..=100)
        .map(|n| if n == 1 { "integration".to_string() } else { format!("integration-{n}") })
        .map(|f| join_path(&ws.kind, dir, &format!(".worktrees/{f}")))
        .find(|p| !host.dir_exists(ws, p))
        .ok_or_else(|| "no free folder for the integration worktree".to_string())?;
    add_worktrees_exclude(host, ws, dir, warnings);
    let _ = git(host, ws, dir, &["worktree", "prune"]);
    git(host, ws, dir, &["worktree", "add", &path, "-b", &branch, &base_ref])?;
    let mut integ = board::Integration { branch, path, base: base_ref, setup_done: false, setup_skipped: false };
    st.inner.lock().unwrap().set_integration(&ws.id, &key, integ.clone(), now);
    setup_integration(st, host, job, dir, &mut integ, warnings)?;
    Ok(integ)
}

/// Setup del worktree di integrazione, se non e' gia' stato fatto. Se
/// fallisce l'integrazione resta registrata con `setup_done` falso: il
/// prossimo avvio del repository lo rifa.
fn setup_integration<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    dir: &str,
    integ: &mut board::Integration,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if integ.setup_done {
        return Ok(());
    }
    let (path, branch, base) = (integ.path.clone(), integ.branch.clone(), integ.base.clone());
    integ.setup_skipped = !repo_cfg(&job.ws, job.task.repo.as_deref()).is_some_and(|c| c.has_setup());
    // Sotto il lock dei repository: niente pubblicazione dei passi.
    warnings.extend(setup_worktree(st, host, job, dir, &path, "integration", &branch, &base, "integration worktree", false)?);
    integ.setup_done = true;
    let key = board::repo_key(job.task.repo.as_deref());
    st.inner.lock().unwrap().set_integration(&job.ws.id, &key, integ.clone(), now_millis());
    Ok(())
}

/// Branch base dichiarato nelle impostazioni del repository del task.
fn settings_base(job: &StartJob) -> Option<&str> {
    repo_cfg(&job.ws, job.task.repo.as_deref()).and_then(|c| c.base_branch.as_deref())
}

/// Crea (o riusa) il worktree del task nel suo repository. Base: task ->
/// branch di integrazione della board -> impostazione per repository ->
/// base della board (root) -> default di `origin` (repo) -> branch corrente
/// della cartella, risolto adesso. Ritorna anche gli avvisi (non bloccanti)
/// per la nota del task.
pub(super) fn prepare_worktree<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let repo = task.repo.as_deref();
    let dir = repo_dir(ws, repo);
    let mut warnings = Vec::new();
    let base = {
        // Fetch e integrazione di un repository si fanno uno alla volta.
        let needs_prep = repo.is_some() || job.integration_template.is_some();
        let mut guard = needs_prep.then(|| lock_repo_prep(st, job));
        let mut local = FetchCache::new();
        let cache: &mut FetchCache = match guard.as_mut() {
            Some(g) => g,
            None => &mut local,
        };
        let integ = match &job.integration_template {
            Some(tpl) => Some(ensure_integration(st, host, job, &dir, tpl, cache, &mut warnings)?),
            None => None,
        };
        match (&task.base_branch, integ) {
            // Senza base esplicita parte dall'integrazione (riferimento locale).
            (None, Some(i)) => i.branch,
            _ => {
                let declared =
                    declared_base(task.base_branch.as_deref(), repo, &job.board_bases, settings_base(job), job.board_base.as_deref());
                let b = resolve_base(host, ws, &dir, repo, declared)?;
                if repo.is_some() {
                    remote_or_local(host, ws, &dir, &b, cache, &mut warnings)
                } else {
                    b
                }
            }
        }
    };
    let branch = format!("task/{}", task.id);
    // Retry: il worktree gia' creato si riusa cosi' com'e'.
    if let Some(wt) = &task.worktree {
        if host.dir_exists(ws, &wt.path) {
            return Ok((wt.clone(), warnings));
        }
    }
    let path = task
        .worktree
        .as_ref()
        .map(|w| w.path.clone())
        .unwrap_or_else(|| join_path(&ws.kind, &dir, &format!(".worktrees/{}", task.id)));
    let mut info = WorktreeInfo { path, branch, base, repo: task.repo.clone(), setup_done: true };
    // Cartella gia' presente ma non registrata (non e' nostra): niente setup.
    if host.dir_exists(ws, &info.path) {
        return Ok((info, warnings));
    }
    add_worktrees_exclude(host, ws, &dir, &mut warnings);
    let _ = git(host, ws, &dir, &["worktree", "prune"]);
    let branch_ref = format!("refs/heads/{}", info.branch);
    if ref_exists(host, ws, &dir, &branch_ref) {
        git(host, ws, &dir, &["worktree", "add", &info.path, &info.branch])?;
    } else {
        git(host, ws, &dir, &["worktree", "add", &info.path, "-b", &info.branch, &info.base])?;
    }
    // Appena creato: il setup e' da fare. Si registra subito, cosi' un setup
    // fallito (o un'app chiusa a meta') lascia traccia e il retry lo rifa.
    info.setup_done = false;
    st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, info.clone()));
    Ok((info, warnings))
}

/// `git diff --stat <base>...HEAD` piu' i file non committati, troncato.
/// Gira nel worktree del task, che sta nel repository del task.
pub(super) fn compute_diffstat<H: Host>(host: &H, ws: &Workspace, wt: &WorktreeInfo) -> Option<String> {
    let range = format!("{}...HEAD", wt.base);
    let stat = git(host, ws, &wt.path, &["diff", "--stat", &range]).unwrap_or_default();
    let dirty = git(host, ws, &wt.path, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    let mut out = if stat.is_empty() { "no committed changes".to_string() } else { stat };
    if dirty > 0 {
        out.push_str(&format!("\n{dirty} file(s) with uncommitted changes"));
    }
    Some(clip_chars(&out, board::DIFFSTAT_MAX))
}

/// Esegue i comandi di pulizia del repository (se ci sono), poi rimuove il
/// worktree (con `--force` se l'impostazione lo chiede) e il branch del task
/// (senza forzare), nel repository del task. Ritorna gli avvisi: non
/// bloccano mai il passaggio a `done`.
///
/// `integration`: branch di integrazione registrato per il repository; se il
/// branch del task e' gia' dentro (o nella base del worktree) si cancella
/// con `-D`, perche' `-d` confronta solo con l'HEAD della cartella principale.
pub(super) fn cleanup_worktree<H: Host>(
    host: &H,
    ws: &Workspace,
    wt: &WorktreeInfo,
    integration: Option<&str>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let root = repo_dir(ws, wt.repo.as_deref());
    let root = root.as_str();
    let cfg = repo_cfg(ws, wt.repo.as_deref()).cloned().unwrap_or_default();
    if host.dir_exists(ws, &wt.path) {
        if !cfg.cleanup.is_empty() {
            let task_id = wt.branch.strip_prefix("task/").unwrap_or(&wt.branch);
            let vars = hooks::HookVars { repo_dir: root, worktree: &wt.path, task_id, branch: &wt.branch, base: &wt.base };
            warnings.extend(hooks::run_cleanup(host, ws, &cfg, &vars));
        }
        let mut args = vec!["worktree", "remove"];
        if cfg.remove_force {
            args.push("--force");
        }
        args.push(&wt.path);
        if let Err(e) = git(host, ws, root, &args) {
            warnings.push(format!("worktree not removed: {e}"));
            return warnings;
        }
    } else {
        let _ = git(host, ws, root, &["worktree", "prune"]);
    }
    let merged = integration.into_iter().chain([wt.base.as_str()]).any(|r| {
        host.git(ws, root, &["merge-base", "--is-ancestor", &wt.branch, r]).map(|o| o.ok).unwrap_or(false)
    });
    let flag = if merged { "-D" } else { "-d" };
    if let Err(e) = git(host, ws, root, &["branch", flag, &wt.branch]) {
        warnings.push(format!("branch not deleted: {e}"));
    }
    warnings
}

// ---------------------------------------------------------------------------
// Rilevamento dei repository del workspace
// ---------------------------------------------------------------------------

/// Elenco dei repository da un elenco di nomi (uno per riga): normalizzati,
/// senza doppioni, "." per primo.
pub(super) fn parse_repo_list(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| board::normalize_repo(l).ok().map(|r| r.unwrap_or_else(|| ".".to_string())))
        .collect();
    v.sort_by(|a, b| (a != ".", a).cmp(&(b != ".", b)));
    v.dedup();
    v.truncate(200);
    v
}

/// Repository del workspace: "." se la root stessa e' un repo, piu' le
/// sottocartelle dirette (profondita' 1, senza quelle nascoste) con un `.git`
/// (cartella o file). WSL: una sola chiamata `sh`.
pub fn detect_repos(ws: &Workspace) -> Result<Vec<String>, String> {
    if ws.kind == "wsl" {
        let distro = ws
            .distro
            .as_deref()
            .filter(|d| !d.is_empty())
            .ok_or_else(|| "distro WSL mancante".to_string())?;
        let script = "for d in */; do [ -e \"${d}.git\" ] && printf '%s\\n' \"${d%/}\"; done; [ -e .git ] && printf '.\\n'; exit 0";
        let cmd = crate::wsl::wsl_run_command(distro, &ws.path, &["sh", "-c", script]);
        let out = output_with_timeout(cmd, GIT_TIMEOUT)?;
        if !out.status.success() {
            return Err(format!("repository detection failed: {}", clip_chars(String::from_utf8_lossy(&out.stderr).trim(), 200)));
        }
        return Ok(parse_repo_list(&String::from_utf8_lossy(&out.stdout)));
    }
    let root = std::path::Path::new(&ws.path);
    let mut names = Vec::new();
    if root.join(".git").exists() {
        names.push(".".to_string());
    }
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && entry.path().is_dir() && entry.path().join(".git").exists() {
            names.push(name);
        }
    }
    Ok(parse_repo_list(&names.join("\n")))
}

// ---------------------------------------------------------------------------
// Avvio di un task (thread separato)
// ---------------------------------------------------------------------------

struct Prepared {
    worktree: Option<WorktreeInfo>,
    warnings: Vec<String>,
    new: NewWorker,
}

/// Worktree del task pronto: creato (o riusato) e con il setup del
/// repository fatto. Se c'e' qualcosa di lento da fare (setup del task o
/// dell'integrazione) il task risulta "in preparazione" e il guardiano
/// dell'avvio aspetta (vedi `PREPARING_TIMEOUT_MS`). Un setup fallito lascia
/// il worktree com'e' (registrato con `setup_done` falso) e fallisce l'avvio.
fn prepare_and_setup<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let slow = repo_cfg(ws, task.repo.as_deref()).is_some_and(|c| c.has_setup());
    if slow {
        st.run(host, |r| r.touch_preparing(&ws.id, &task.id, job.attempt, Some("worktree"), now_millis(), true));
    }
    let end = |st: &AutomationState| {
        if slow {
            st.run(host, |r| r.end_preparing(&ws.id, &task.id, job.attempt, now_millis()));
        }
    };
    let (mut wt, mut warnings) = match prepare_worktree(st, host, job) {
        Ok(x) => x,
        Err(e) => {
            end(st);
            return Err(e);
        }
    };
    if !wt.setup_done {
        let dir = repo_dir(ws, task.repo.as_deref());
        match setup_worktree(st, host, job, &dir, &wt.path, &task.id, &wt.branch, &wt.base, "worktree setup", true) {
            Ok(w) => warnings.extend(w),
            Err(e) => {
                end(st);
                return Err(e);
            }
        }
        wt.setup_done = true;
        // Registrato fatto subito: se un passo successivo dell'avvio fallisce, il retry non rifa il setup.
        st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, wt.clone()));
    }
    end(st);
    Ok((wt, warnings))
}

fn prepare_start<H: Host>(st: &AutomationState, host: &H, job: &StartJob) -> Result<Prepared, String> {
    let (ws, task) = (&job.ws, &job.task);
    let (worktree, warnings) = match task.isolation {
        Isolation::Worktree => {
            let (wt, w) = prepare_and_setup(st, host, job)?;
            (Some(wt), w)
        }
        Isolation::None => (None, Vec::new()),
    };
    let workdir = worktree.as_ref().map(|w| w.path.clone()).unwrap_or_else(|| repo_dir(ws, task.repo.as_deref()));
    if let (Some(r), None) = (&task.repo, &worktree) {
        if !host.dir_exists(ws, &workdir) {
            return Err(format!("the repository folder '{r}' ({workdir}) does not exist in the workspace"));
        }
    }
    let requested = task.permission_mode.as_deref().and_then(PermissionMode::parse);
    let permission_mode = resolve_permission(requested, ws.worker_permission_mode())?;
    let settings_path = host.worker_settings_path(ws, &task.agent)?;
    let req = AutomationRequest {
        bootstrap_prompt: BOOTSTRAP_PROMPT,
        permission_mode,
        model: task.model.as_deref().or(ws.worker_model.as_deref()),
        settings_path: settings_path.as_deref(),
    };
    let command = host.launch_command(&task.agent, &ws.kind, &req)?;
    Ok(Prepared {
        worktree,
        warnings,
        new: NewWorker {
            workspace_id: ws.id.clone(),
            workspace_name: ws.name.clone(),
            agent_id: task.agent.clone(),
            title: task.title.clone(),
            task: task.task.clone(),
            cwd: Some(workdir),
            keep_open: false,
            command,
        },
    })
}

/// Lavoro dell'avvio: git, comando di lancio, registrazione del worker e
/// spawn del tab. Gira su un thread dell'host; rientra nello stato solo per
/// registrare il risultato.
pub(super) fn run_start_job<H: Host>(st: &AutomationState, host: &H, job: StartJob) {
    let (ws_id, tid, attempt) = (job.ws.id.clone(), job.task.id.clone(), job.attempt);
    // Un solo avvio per volta per task: cancel + retry mentre il primo e'
    // ancora in git non devono sovrapporre due `worktree add` sullo stesso path.
    let key = format!("{ws_id}/{tid}");
    let lock = st.starts.lock().unwrap().entry(key.clone()).or_default().clone();
    {
        let _turn = lock.lock().unwrap_or_else(|e| e.into_inner());
        // Superato mentre aspettava il turno: niente git.
        if st.run(host, |r| r.start_is_current(&ws_id, &tid, attempt)) {
            start_inner(st, host, &job);
        }
    }
    let mut starts = st.starts.lock().unwrap();
    if starts.get(&key).is_some_and(|l| Arc::strong_count(l) <= 2) {
        starts.remove(&key);
    }
}

fn start_inner<H: Host>(st: &AutomationState, host: &H, job: &StartJob) {
    let (ws_id, tid, attempt) = (job.ws.id.clone(), job.task.id.clone(), job.attempt);
    match prepare_start(st, host, job) {
        Ok(p) => {
            let ev = st.run(host, |r| {
                r.register_task_worker(&ws_id, &tid, attempt, p.worktree, p.warnings, p.new, now_millis())
            });
            if let Some(ev) = ev {
                host.emit_spawn(&ev);
            }
        }
        Err(e) => st.run(host, |r| r.start_failed(&ws_id, &tid, attempt, &e, now_millis())),
    }
}

// ---------------------------------------------------------------------------
// RPC dell'orchestratore
// ---------------------------------------------------------------------------

/// Esegue `f` sul registro e schedula subito, cosi' il risultato riflette
/// i task appena avviati.
fn board_op<H: Host, T>(
    st: &AutomationState,
    host: &H,
    now: i64,
    f: impl FnOnce(&mut Registry) -> Result<T, String>,
) -> Result<T, String> {
    st.run(host, |r| {
        let out = f(r);
        r.flush_boards(now, host.local_minute(now), &|id| host.workspace(id));
        out
    })
}

const PAUSED_WARNING: &str = "the board is paused: the scheduler starts no task until set_board paused: false";

fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

fn check_agents<H: Host>(host: &H, specs: &[board::TaskSpec]) -> Result<(), String> {
    let known = host.agents();
    for s in specs {
        if !known.iter().any(|(id, _)| *id == s.agent) {
            return Err(format!("task '{}': unknown agent: {} (see list_agents)", s.id, s.agent));
        }
    }
    Ok(())
}

/// `columns` di `get_board`: elenco opzionale di nomi di colonna validi.
fn parse_columns(params: &Value) -> Result<Option<Vec<board::Column>>, String> {
    let valid = || board::Column::ALL.map(board::Column::as_str).join(", ");
    let list = match params.get("columns") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(format!("columns must be an array of column names ({})", valid())),
    };
    let mut out = Vec::new();
    for v in list {
        let col = v.as_str().and_then(board::Column::parse);
        out.push(col.ok_or_else(|| format!("invalid column {v} (valid: {})", valid()))?);
    }
    Ok(Some(out))
}

pub(super) fn rpc_board<H: Host>(
    st: &AutomationState,
    host: &H,
    orch: &str,
    method: &str,
    params: &Value,
    now: i64,
) -> Result<Value, String> {
    if !(params.is_null() || params.is_object()) {
        return Err("params must be an object".to_string());
    }
    let empty = json!({});
    let params = if params.is_null() { &empty } else { params };
    // La board e' quella del workspace del tab dell'orchestratore.
    let ws = host
        .tab_workspace(orch)
        .ok_or_else(|| "the orchestrator's workspace was not found".to_string())?;
    let ceiling = ws.worker_permission_mode();
    st.run(host, |r| r.claim_board(&ws.id, orch))?;
    match method {
        "submit_plan" => {
            let plan = board::parse_plan(params)?;
            board::check_permissions(&plan.specs, ceiling)?;
            check_agents(host, &plan.specs)?;
            // Un repo git per ogni repository dei task con worktree (git fuori dal lock).
            ensure_git_repos(
                host,
                &ws,
                plan.specs.iter().filter(|s| s.isolation == Isolation::Worktree).map(|s| s.repo.as_deref()),
            )?;
            let ids = board_op(st, host, now, |r| r.submit_plan(&ws.id, plan, ceiling, now))?;
            let view = st.run(host, |r| r.board_view(&ws.id));
            let mut out = view.to_compact_json(None);
            out["added"] = json!(ids);
            if view.paused {
                out["warnings"] = json!([PAUSED_WARNING]);
            }
            Ok(out)
        }
        "get_board" => {
            let columns = parse_columns(params)?;
            // Repository rilevati: una volta per board (cache), mai a ogni chiamata.
            if st.run(host, |r| r.repos_cached(&ws.id)).is_none() {
                if let Ok(repos) = host.list_repos(&ws) {
                    st.run(host, |r| r.set_repos(&ws.id, repos));
                }
            }
            let mut out = st.run(host, |r| r.board_view(&ws.id)).to_compact_json(columns.as_deref());
            // Integrazione registrata con la cartella sparita: l'orchestratore decide.
            if let Some(ints) = out.get_mut("integrations").and_then(Value::as_object_mut) {
                for i in ints.values_mut() {
                    let gone = i.get("path").and_then(Value::as_str).is_some_and(|p| !host.dir_exists(&ws, p));
                    if gone {
                        i["missing"] = json!(true);
                    }
                }
            }
            Ok(out)
        }
        "get_task_details" => {
            let id = req_str(params, "id")?.trim().to_string();
            let view = st.run(host, |r| r.task_view(&ws.id, &id)).ok_or_else(|| format!("unknown task: {id}"))?;
            Ok(json!({ "task": view.to_json(true) }))
        }
        "update_task" => {
            let id = req_str(params, "id")?.trim().to_string();
            let spec_agent = opt_str(params, "agent")?.map(str::trim).filter(|a| !a.is_empty());
            if let Some(a) = spec_agent {
                if !host.agents().iter().any(|(k, _)| k == a) {
                    return Err(format!("unknown agent: {a} (see list_agents)"));
                }
            }
            // Il task risultante usa un worktree in un repository nuovo? Va verificato.
            if params.get("isolation").is_some() || params.get("repo").is_some() {
                let current = st.run(host, |r| r.task_view(&ws.id, &id));
                if let Some(cur) = current {
                    let spec = board::parse_spec(params, Some(&board::TaskSpec::from_task(&cur.task)))?;
                    if spec.isolation == Isolation::Worktree {
                        ensure_git_repo(host, &ws, spec.repo.as_deref())?;
                    }
                }
            }
            board_op(st, host, now, |r| r.update_task(&ws.id, &id, params, ceiling, now))?;
            let view = st.run(host, |r| r.task_view(&ws.id, &id));
            // Con `task` nel patch si rilegge il testo salvato.
            let with_text = params.get("task").is_some();
            let paused = st.run(host, |r| r.board_view(&ws.id)).paused;
            let mut out = json!({ "task": view.map(|v| v.to_json(with_text)) });
            if paused {
                out["warnings"] = json!([PAUSED_WARNING]);
            }
            Ok(out)
        }
        "set_task_status" => {
            let id = req_str(params, "id")?.trim().to_string();
            let status = req_str(params, "status")?.trim().to_string();
            let note = opt_str(params, "note")?.map(str::to_string);
            let cleanup = opt_bool(params, "cleanup")?.unwrap_or(true);
            let wt = board_op(st, host, now, |r| r.set_task_status(&ws.id, &id, &status, note, now))?;
            let warnings = cleanup_after_done(st, host, &ws, &id, wt.filter(|_| cleanup));
            let view = st.run(host, |r| r.task_view(&ws.id, &id));
            let mut out = json!({ "task": view.map(|v| v.to_json(false)) });
            let mut warnings = warnings;
            if st.run(host, |r| r.board_view(&ws.id)).paused {
                warnings.push(PAUSED_WARNING.to_string());
            }
            if !warnings.is_empty() {
                out["warnings"] = json!(warnings);
            }
            Ok(out)
        }
        "set_board" => {
            let max_parallel = match params.get("max_parallel") {
                None | Some(Value::Null) => None,
                Some(v) => Some(board::parse_max_parallel(v)?),
            };
            let paused = opt_bool(params, "paused")?;
            let repo = RepoSettings::parse(params)?;
            board_op(st, host, now, |r| r.set_board_ext(&ws.id, max_parallel, paused, repo, now))?;
            Ok(st.run(host, |r| r.board_view(&ws.id)).to_compact_json(None))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}

/// Pulizia git del worktree dopo `done` (fuori dal lock); gli avvisi
/// finiscono anche nella nota del task.
fn cleanup_after_done<H: Host>(
    st: &AutomationState,
    host: &H,
    ws: &Workspace,
    id: &str,
    wt: Option<WorktreeInfo>,
) -> Vec<String> {
    let Some(wt) = wt else {
        return Vec::new();
    };
    let key = board::repo_key(wt.repo.as_deref());
    let integ = st.run(host, |r| r.integration_of(&ws.id, &key)).map(|i| i.branch);
    let warnings = cleanup_worktree(host, ws, &wt, integ.as_deref());
    if !warnings.is_empty() {
        let note = format!("cleanup warnings: {}", warnings.join("; "));
        st.run(host, |r| r.add_task_note(&ws.id, id, &note, now_millis()));
    }
    warnings
}

// ---------------------------------------------------------------------------
// Comandi Tauri (logica)
// ---------------------------------------------------------------------------

pub(super) fn board_set_cmd<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    if host.workspace(ws_id).is_none() {
        return Err("workspace non trovato".to_string());
    }
    let now = now_millis();
    board_op(st, host, now, |r| r.set_board(ws_id, max_parallel, paused, now))
        .map_err(|e| {
            e.replace(
                "nothing to change: pass max_parallel, paused, base_branches, integration_branch and/or close_integration",
                "nessuna modifica richiesta",
            )
        })?;
    Ok(st.run(host, |r| r.board_view(ws_id)))
}

/// Azione dell'utente dalla vista Board; genera un evento `user_action` per
/// l'orchestratore proprietario.
pub(super) fn board_action<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    action: &str,
) -> Result<BoardView, String> {
    let ws = host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
    let now = now_millis();
    let (verb, target): (&str, Option<&str>) = match action {
        "hold" => ("put on hold", None),
        "unhold" => ("released from hold", None),
        "retry" => ("retried (back to the queue)", Some("queued")),
        "cancel" => ("cancelled", Some("cancelled")),
        "done" => ("marked as done", Some("done")),
        _ => return Err("azione non valida (usa hold, unhold, retry, cancel o done)".to_string()),
    };
    let wt = board_op(st, host, now, |r| {
        let wt = match target {
            Some(t) => r.set_task_status(ws_id, task_id, t, None, now)?,
            None => {
                r.set_held(ws_id, task_id, action == "hold", now)?;
                None
            }
        };
        r.push_user_action(ws_id, task_id, format!("The user {verb} task '{task_id}' from the board."), now);
        Ok(wt)
    })?;
    cleanup_after_done(st, host, &ws, task_id, wt);
    Ok(st.run(host, |r| r.board_view(ws_id)))
}

/// Cambia il modello di un task dalla vista Board (`None` = default del
/// workspace); genera un evento `user_action` per l'orchestratore.
pub(super) fn board_set_model<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    task_id: &str,
    model: Option<&str>,
) -> Result<BoardView, String> {
    let ws = host.workspace(ws_id).ok_or_else(|| "workspace non trovato".to_string())?;
    let model = model.map(str::trim).filter(|m| !m.is_empty());
    if model.is_some_and(|m| !super::valid_model(m)) {
        return Err("modello non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string());
    }
    let now = now_millis();
    board_op(st, host, now, |r| {
        r.update_task(ws_id, task_id, &json!({ "model": model }), ws.worker_permission_mode(), now)?;
        let text = match model {
            Some(m) => format!("The user set the model of task '{task_id}' to '{m}' from the board."),
            None => format!(
                "The user cleared the model of task '{task_id}' from the board (it will use the workspace default)."
            ),
        };
        r.push_user_action(ws_id, task_id, text, now);
        Ok(())
    })?;
    Ok(st.run(host, |r| r.board_view(ws_id)))
}

// ---------------------------------------------------------------------------
// Host reale: git, file
// ---------------------------------------------------------------------------

/// Tetto di un comando git (o wsl.exe): oltre, il processo si uccide e
/// l'avvio fallisce invece di bloccare per sempre il thread.
pub(super) const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Esegue `cmd` con un tempo massimo. Allo scadere uccide il figlio e ritorna
/// errore; non aspetta i thread di lettura (un nipote potrebbe tenere le
/// pipe aperte).
pub(super) fn output_with_timeout(
    mut cmd: std::process::Command,
    limit: Duration,
) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::sync::mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let (out_rx, err_rx) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    };
    // Processo chiuso: le pipe si chiudono, breve attesa per l output.
    let grab = |rx: std::sync::mpsc::Receiver<Vec<u8>>| rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    Ok(std::process::Output { status, stdout: grab(out_rx), stderr: grab(err_rx) })
}

pub(super) fn run_git(ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
    let cmd = if ws.kind == "wsl" {
        let distro = ws
            .distro
            .as_deref()
            .filter(|d| !d.is_empty())
            .ok_or_else(|| "distro WSL mancante".to_string())?;
        crate::wsl::wsl_git_command(distro, dir, args)
    } else {
        let mut cmd = std::process::Command::new("git");
        cmd.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };
    let limit = if args.first() == Some(&"fetch") { FETCH_TIMEOUT } else { GIT_TIMEOUT };
    let output = output_with_timeout(cmd, limit).map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    Ok(GitOut {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Aggiunge `line` al file di esclusione (se manca). WSL: uno script `sh`
/// con file e riga passati come argv (mai interpolati; `-e`, senza la shell
/// di default della distro di mezzo).
pub(super) fn append_exclude_line(ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
    if ws.kind == "wsl" {
        let distro = ws
            .distro
            .as_deref()
            .filter(|d| !d.is_empty())
            .ok_or_else(|| "distro WSL mancante".to_string())?;
        let script = "mkdir -p \"$(dirname \"$1\")\" && { grep -qxF -e \"$2\" \"$1\" 2>/dev/null || printf '%s\n' \"$2\" >> \"$1\"; }";
        let cmd = crate::wsl::wsl_run_command(distro, &ws.path, &["sh", "-c", script, "sh", file, line]);
        let out = output_with_timeout(cmd, GIT_TIMEOUT)?;
        return if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!("exclude not updated: {}", clip_chars(err.trim(), 200)))
        };
    }
    let path = std::path::Path::new(file);
    let current = std::fs::read_to_string(path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    std::fs::write(path, next).map_err(|e| e.to_string())
}

pub(super) fn boards_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|d| d.join("boards"))
        .map_err(|e| e.to_string())
}

/// File della board; l'id del workspace non deve poter uscire dalla cartella.
pub(super) fn board_file(dir: &std::path::Path, ws_id: &str) -> Result<PathBuf, String> {
    if ws_id.is_empty() || !ws_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("id workspace non valido".to_string());
    }
    Ok(dir.join(format!("{ws_id}.json")))
}
