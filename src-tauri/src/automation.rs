//! Task Manager: coda di task per workspace, scheduler deterministico e worker
//! (tab PTY marcati automatici) che consegnano da soli. I worker parlano con
//! l'app solo tramite tool MCP (`get_task`, `complete_task`) e gli hook del
//! coding agent; i tab normali (Intake compreso) propongono task e consultano
//! la coda. Contratto: `docs/BOARD.MD`.
//!
//! Struttura a strati, pensata per i test:
//! - `Registry`: nucleo PURO (niente `AppHandle`, niente I/O, tempo iniettato).
//!   Tiene worker e board; gli effetti da eseguire li accumula come `Effect`.
//! - `AutomationState` (`state`): `Mutex<Registry>` (managed state Tauri). `run`
//!   esegue una modifica, schedula le board toccate e applica gli effetti
//!   FUORI dal lock (avvio dei task con git, salvataggi, eventi).
//! - `Host` (`host`): gli effetti verso l'esterno (eventi Tauri, timer, git,
//!   workspace). `TauriHost` e' l'implementazione reale; i test usano un host
//!   finto, cosi' anche `dispatch` si prova senza finestra.
//!
//! Il token dell'app (`token()`) autentica le chiamate RPC e gli eventi degli
//! hook: solo i processi figli di un terminale di cli-companion lo conoscono.

use crate::agent_notify::{is_valid_tab_id, RpcRequest, RpcResponse};
use crate::board::{
    self, build_report, Board, BoardView, ReportFields, TaskStatus, WorktreeInfo,
};
use crate::workspaces::{Delivery, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

mod agent_session;
pub mod auto;
mod boards;
mod hooks;
mod host;
mod rpc;
pub(crate) mod settings;
pub mod skills;
mod state;
mod worktrees;

pub use host::{Host, TauriHost};
pub use crate::user_skills::targets::SkillTarget;
pub use state::AutomationState;
pub use worktrees::{CleanupTarget, GitOut, StartJob};

/// Dopo `complete_task` il tab resta aperto il tempo di vedere l'esito.
pub const CLOSE_DELAY: Duration = Duration::from_millis(1500);
/// Dopo la chiusura del tab, il tempo perche' Windows rilasci la cartella
/// del worktree prima di rimuoverla.
pub const CLEANUP_DELAY: Duration = Duration::from_secs(2);
/// Worker (anche chiusi) tenuti in memoria prima di scartare i piu' vecchi.
const MAX_TRACKED_WORKERS: usize = 256;

pub(crate) const TITLE_MAX: usize = 80;
pub(crate) const TASK_MAX: usize = 100_000;
pub(crate) const SUMMARY_MAX: usize = 50_000;
pub(crate) const DEFAULT_AGENT: &str = "claude-code";

/// Prompt passato in riga di comando: ASCII semplice, sicuro in ogni shell
/// (niente virgolette, `$`, backtick, `%`, `!`, `\`). Lo legge il modello:
/// in inglese.
pub const BOOTSTRAP_PROMPT: &str =
    "You are an automated cli-companion session. Call the get_task tool right away and follow its instructions.";

/// Motivo del blocco dello Stop hook: il worker ha chiuso il turno senza
/// riferire (lo legge il modello).
pub const STOP_BLOCK_REASON: &str = "You ended your turn without calling complete_task. If the task is done, deliver it and call complete_task. If you are waiting for the user's decision, end your turn again: the user will answer here.";

// ---------------------------------------------------------------------------
// Token e utilita'
// ---------------------------------------------------------------------------

/// Token casuale dell'app (32 hex), generato al primo uso. Senza crate di
/// random: `RandomState` e' seminato dal sistema operativo.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(generate_token)
}

fn generate_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..2u32)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            h.write_u32(i);
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Il token ricevuto e' quello dell'app?
pub fn token_matches(candidate: Option<&str>) -> bool {
    candidate.is_some_and(|c| !c.is_empty() && c == token())
}

/// Controlli comuni a ogni richiesta RPC: versione, token, id tab chiamante.
fn authenticate(req: &RpcRequest, expected_token: &str) -> Result<(), String> {
    if req.rpc != 1 {
        return Err("unsupported RPC version".to_string());
    }
    if req.token.is_empty() || req.token != expected_token {
        return Err("invalid token".to_string());
    }
    if !is_valid_tab_id(&req.session) {
        return Err("invalid session".to_string());
    }
    Ok(())
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Testo obbligatorio: trim, 1..=`max` caratteri.
pub(crate) fn req_text(label: &str, s: &str, max: usize) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{label} is missing"));
    }
    if t.chars().count() > max {
        return Err(format!("{label} is too long (max {max} characters)"));
    }
    Ok(t.to_string())
}

pub(crate) fn clip_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub(crate) fn valid_model(m: &str) -> bool {
    !m.is_empty() && m.len() <= 64 && m.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Campo stringa obbligatorio di un oggetto JSON.
pub(crate) fn req_str<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    match p.get(key) {
        Some(Value::String(s)) => Ok(s.as_str()),
        None | Some(Value::Null) => Err(format!("{key} is missing")),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

// ---------------------------------------------------------------------------
// Tipi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    /// Spawnato, task non ancora letto.
    Starting,
    Running,
    /// Turno chiuso in attesa di lavoro in background (Claude Code riprende
    /// da solo): non e' un abbandono.
    Background,
    /// Permesso in attesa nel terminale.
    NeedsInput,
    /// L'agente ha finito il turno senza `complete_task`.
    Idle,
    Completed,
    Failed,
    Closed,
}

impl WorkerStatus {
    /// Stati finali: dopo non ci sono piu' transizioni dagli hook.
    fn is_terminal(self) -> bool {
        matches!(self, WorkerStatus::Completed | WorkerStatus::Failed | WorkerStatus::Closed)
    }

    /// Il worker aspetta l'utente.
    fn needs_attention(self) -> bool {
        matches!(self, WorkerStatus::NeedsInput | WorkerStatus::Idle)
    }
}

/// Stato di un worker per il frontend (evento `automation-update` e comando
/// `automation_sessions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutomationTabInfo {
    pub tab_id: String,
    pub status: WorkerStatus,
    pub title: String,
    /// Sempre vuoto: i worker non hanno piu' un genitore.
    pub parent_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report: Option<String>,
}

/// Payload di `automation-spawn`: il frontend apre il tab e digita `command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpawnEvent {
    pub tab_id: String,
    pub workspace_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub title: String,
    /// Sempre vuoto (compatibilita' col frontend).
    pub parent_tab_id: String,
    pub agent_id: String,
}

/// Dati di un worker da registrare (gia' validati e con il comando pronto).
#[derive(Debug, Clone)]
pub struct NewWorker {
    pub workspace_id: String,
    pub agent_id: String,
    pub title: String,
    pub cwd: String,
    /// Comando da digitare nella shell del nuovo tab.
    pub command: String,
}

/// Notifica di sistema per l'utente (evento `task-notify`): un task richiede
/// attenzione o e' fallito. Il testo e' in italiano.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub workspace_id: String,
    pub task_id: String,
    /// Tab del worker, se esiste ancora (il toast cliccabile porta li').
    pub tab_id: Option<String>,
    /// `attention` | `failed`
    pub kind: &'static str,
    /// Titolo del task.
    pub title: String,
    pub body: String,
}

/// Effetto da eseguire fuori dal lock, accumulato dal `Registry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Update(AutomationTabInfo),
    Close { tab_id: String, delay: Duration },
    /// Avviare un task di board (git + spawn) su un thread separato.
    StartTask(Box<StartJob>),
    /// Salvare su disco e notificare al frontend la board (`board-update`).
    Publish(Box<Board>, Box<BoardView>),
    /// La board del workspace e' stata rimossa: cancellare il file. `rev` e'
    /// una revisione di rimozione: salvataggi piu' vecchi in ritardo vengono
    /// scartati (niente file resuscitato).
    DeleteBoard(String, u64),
    /// Rimuovere worktree e branch di un task concluso o eliminato.
    Cleanup(Box<CleanupTarget>),
    Notify(Box<Notice>),
    /// Prendere (`on`) o rilasciare il blocco dello standby. `seq` ordina gli
    /// effetti applicati fuori dal lock da thread diversi: uno vecchio in
    /// ritardo viene scartato.
    KeepAwake { on: bool, seq: u64 },
    /// Emettere `schedule-state`.
    Schedule(Box<crate::schedule::ScheduleState>),
}

/// Dove e come lavora un worker (calcolato all'avvio, fuori dal lock): serve a
/// generare le istruzioni di `get_task`.
#[derive(Debug, Clone)]
pub(crate) struct WorkerEnv {
    /// Consegna del workspace quando il task e' partito.
    pub delivery: Delivery,
    pub repo_dir: String,
    pub workdir: String,
    pub branch: Option<String>,
    /// Branch base (nome semplice), nelle modalita' con branch.
    pub base: Option<String>,
}

struct Worker {
    session: String,
    workspace_id: String,
    task_id: String,
    /// Tentativo (`attempts` del task) per cui il worker e' partito.
    attempt: u32,
    title: String,
    env: WorkerEnv,
    status: WorkerStatus,
    /// Sessione dell'agente legata al worker (vedi `agent_session`).
    agent: agent_session::AgentSession,
    created_at: i64,
    updated_at: i64,
    last_report: Option<String>,
    /// `get_task` chiamato almeno una volta.
    started_sent: bool,
    /// Il tab esiste ancora.
    tab_open: bool,
    /// Task concluso con un worktree: da ripulire quando il tab si chiude.
    cleanup_on_close: bool,
}

impl Worker {
    fn info(&self) -> AutomationTabInfo {
        AutomationTabInfo {
            tab_id: self.session.clone(),
            status: self.status,
            title: self.title.clone(),
            parent_tab_id: String::new(),
            last_report: self.last_report.clone(),
        }
    }
}

/// Evento di un hook come lo vede il registro.
#[derive(Debug, Clone, Copy, Default)]
pub struct HookEvent<'a> {
    pub event: &'a str,
    pub notification_type: Option<&'a str>,
    pub stop_hook_active: Option<bool>,
    pub background_work: Option<u32>,
    /// `session_id` dello stdin dell'hook.
    pub agent_session_id: Option<&'a str>,
    /// Solo `session-start`: `source` dello stdin.
    pub source: Option<&'a str>,
    /// Solo `session-end`: `reason` dello stdin.
    pub reason: Option<&'a str>,
}

#[derive(Default)]
pub struct Registry {
    workers: HashMap<String, Worker>,
    effects: Vec<Effect>,
    /// Board per workspace (vedi `boards`).
    boards: HashMap<String, Board>,
    /// Workspace con board modificata da schedulare/salvare/notificare.
    dirty: Vec<String>,
    /// Ultima revisione assegnata alle board (contatore globale monotono:
    /// resta crescente anche se una board sparisce e ricompare).
    next_rev: u64,
    next_worker: u64,
    next_proposal: u64,
    /// Fasce orarie e standby (vedi `auto`).
    auto: auto::AutoCore,
}

impl Registry {
    pub fn is_worker(&self, session: &str) -> bool {
        self.workers.contains_key(session)
    }

    /// Effetti accumulati dall'ultima chiamata.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Registra un nuovo worker per un task di board.
    fn spawn_worker(&mut self, ws: &str, tid: &str, attempt: u32, new: NewWorker, env: WorkerEnv, now: i64) -> SpawnEvent {
        self.next_worker += 1;
        let id = format!("auto-{now}-{}", self.next_worker);
        let ev = SpawnEvent {
            tab_id: id.clone(),
            workspace_id: new.workspace_id,
            command: new.command,
            cwd: Some(new.cwd),
            title: new.title.clone(),
            parent_tab_id: String::new(),
            agent_id: new.agent_id,
        };
        self.workers.insert(
            id.clone(),
            Worker {
                session: id,
                workspace_id: ws.to_string(),
                task_id: tid.to_string(),
                attempt,
                title: new.title,
                env,
                status: WorkerStatus::Starting,
                agent: agent_session::AgentSession::default(),
                created_at: now,
                updated_at: now,
                last_report: None,
                started_sent: false,
                tab_open: true,
                cleanup_on_close: false,
            },
        );
        self.prune();
        ev
    }

    /// Scarta i worker chiusi piu' vecchi oltre il tetto di memoria.
    fn prune(&mut self) {
        while self.workers.len() > MAX_TRACKED_WORKERS {
            let oldest = self.workers.values().filter(|w| !w.tab_open).min_by_key(|w| w.created_at).map(|w| w.session.clone());
            match oldest {
                Some(id) => {
                    self.workers.remove(&id);
                }
                None => break,
            }
        }
    }

    fn worker_mut(&mut self, session: &str) -> Result<&mut Worker, String> {
        self.workers.get_mut(session).ok_or_else(|| "reserved for automated sessions".to_string())
    }

    /// Worker vivo (tab aperto, non chiuso) del chiamante.
    fn live_worker(&mut self, session: &str) -> Result<&mut Worker, String> {
        let w = self.worker_mut(session)?;
        if w.status == WorkerStatus::Closed || !w.tab_open {
            return Err("the session is closed".to_string());
        }
        Ok(w)
    }

    /// Pubblica lo stato del worker al frontend e lo riflette sul suo task.
    fn touch(&mut self, session: &str, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        w.updated_at = now;
        let (info, status, last, ws, tid) = (w.info(), w.status, w.last_report.clone(), w.workspace_id.clone(), w.task_id.clone());
        self.effects.push(Effect::Update(info));
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

    /// Notifica di sistema per il task del worker (`attention` | `failed`).
    fn notify(&mut self, ws: &str, tid: &str, tab: Option<&str>, kind: &'static str) {
        let Some(t) = self.boards.get(ws).and_then(|b| b.task(tid)) else {
            return;
        };
        let body = match kind {
            "attention" => format!("{} richiede attenzione", t.title),
            _ => format!("{} è fallito", t.title),
        };
        let notice = Notice {
            workspace_id: ws.to_string(),
            task_id: tid.to_string(),
            tab_id: tab.map(str::to_string),
            kind,
            title: t.title.clone(),
            body,
        };
        self.effects.push(Effect::Notify(Box::new(notice)));
    }

    /// Imposta lo stato di un worker; l'ingresso in attenzione notifica l'utente.
    fn set_status(&mut self, session: &str, status: WorkerStatus, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        let was_attention = w.status.needs_attention();
        w.status = status;
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        self.touch(session, now);
        if status.needs_attention() && !was_attention {
            self.notify(&ws, &tid, Some(session), "attention");
        }
    }

    // --- worker ---

    /// `get_task`: istruzioni, ambiente e passi di consegna del task. La prima
    /// volta il worker passa a `running`.
    pub fn get_task(&mut self, session: &str, ws: &Workspace, now: i64) -> Result<Value, String> {
        let w = self.live_worker(session)?;
        let (ws_id, tid, env) = (w.workspace_id.clone(), w.task_id.clone(), w.env.clone());
        let t = self
            .boards
            .get(&ws_id)
            .and_then(|b| b.task(&tid))
            .filter(|t| t.status == TaskStatus::Running && t.session.as_deref() == Some(session))
            .ok_or_else(|| "this task is no longer assigned to this session".to_string())?;
        let info = board::texts::EnvInfo {
            delivery: &env.delivery,
            root: &ws.path,
            repo_dir: &env.repo_dir,
            workdir: &env.workdir,
            task: t,
            branch: env.branch.as_deref(),
            base: env.base.as_deref(),
            repo_cfg: ws.repos.get(&board::repo_key(t.repo.as_deref())),
        };
        let mut out = serde_json::json!({
            "task": t.task,
            "title": t.title,
            "instructions": board::texts::instructions(&info, &ws.worker_rules),
            "environment": board::texts::environment(&info),
            "delivery": board::texts::delivery_steps(&info),
        });
        if let Some(p) = &t.previous_attempt {
            out["previous_attempt"] = serde_json::to_value(p).unwrap_or(Value::Null);
        }
        let w = self.worker_mut(session)?;
        let first = !w.started_sent;
        w.started_sent = true;
        if w.status == WorkerStatus::Starting {
            w.status = WorkerStatus::Running;
        }
        if first {
            self.touch(session, now);
        }
        Ok(out)
    }

    /// `complete_task`: il task diventa `done` (consegnato) o `failed` con il
    /// report; il tab si chiude dopo `CLOSE_DELAY`. Un task fallito per un
    /// `session-end` mentre il tab era aperto viene prima riaperto.
    pub fn complete_task(
        &mut self,
        session: &str,
        status: &str,
        summary: &str,
        fields: ReportFields,
        diffstat: Option<String>,
        now: i64,
    ) -> Result<(), String> {
        // `blocked` e' un sinonimo di `failed`.
        let completed = match status {
            "completed" => true,
            "failed" | "blocked" => false,
            _ => return Err("invalid status (use completed or failed)".to_string()),
        };
        let summary = req_text("summary", summary, SUMMARY_MAX)?;
        self.reopen_after_session_end(session, now);
        let w = self.live_worker(session)?;
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        w.status = if completed { WorkerStatus::Completed } else { WorkerStatus::Failed };
        w.last_report = Some(clip_chars(&summary, 200));
        let to = if completed { TaskStatus::Done } else { TaskStatus::Failed };
        let report = build_report(if completed { "completed" } else { "failed" }, &summary, fields, diffstat);
        let finished = self
            .boards
            .get_mut(&ws)
            .is_some_and(|b| b.finish_running(&tid, Some(session), to, Some(report), None, now));
        if finished {
            let has_worktree = self.boards.get(&ws).and_then(|b| b.task(&tid)).is_some_and(|t| t.worktree.is_some());
            if let Some(w) = self.workers.get_mut(session) {
                w.cleanup_on_close = completed && has_worktree;
            }
            self.mark_dirty(&ws);
            if !completed {
                self.notify(&ws, &tid, Some(session), "failed");
            }
        }
        self.touch(session, now);
        self.effects.push(Effect::Close { tab_id: session.to_string(), delay: CLOSE_DELAY });
        Ok(())
    }

    // --- hook e chiusura tab ---

    /// Evento di un hook con token valido. Per i worker aggiorna lo stato;
    /// ritorna `Some(motivo)` se lo Stop hook va bloccato: il worker ha chiuso
    /// il turno senza riferire e Claude Code deve proseguire. Gli eventi di
    /// un'altra sessione dell'agente (processi secondari) sono ignorati.
    pub fn on_hook(&mut self, tab: &str, hook: &HookEvent, now: i64) -> Option<String> {
        let w = self.workers.get_mut(tab).filter(|w| w.tab_open)?;
        if !w.agent.admit(hook) {
            if hook.event == "session-end" {
                let why = format!("it came from another agent session ({})", hook.agent_session_id.unwrap_or_default());
                self.note_ignored_session_end(tab, &why, now);
            }
            return None;
        }
        let status = w.status;
        match hook.event {
            // `idle_prompt` (~60 s dopo la fine del turno) non e' una
            // richiesta di permesso: il worker non e' fermo su un dialogo.
            "needs-input" if hook.notification_type == Some("idle_prompt") => {}
            "needs-input"
                if matches!(
                    status,
                    WorkerStatus::Starting | WorkerStatus::Running | WorkerStatus::Background | WorkerStatus::Idle
                ) =>
            {
                self.set_status(tab, WorkerStatus::NeedsInput, now);
            }
            "finished"
                if matches!(
                    status,
                    WorkerStatus::Starting
                        | WorkerStatus::Running
                        | WorkerStatus::Background
                        | WorkerStatus::NeedsInput
                        | WorkerStatus::Idle
                ) =>
            {
                if hook.background_work.is_some_and(|n| n > 0) {
                    // Lavoro in background attivo: Claude Code riprende da solo.
                    if status != WorkerStatus::Background {
                        self.set_status(tab, WorkerStatus::Background, now);
                    }
                } else if hook.stop_hook_active != Some(true) {
                    // Prima volta senza report: si fa proseguire il turno. Un
                    // nuovo turno dopo la risposta dell'utente non e' piu' attesa.
                    if status.needs_attention() {
                        self.set_status(tab, WorkerStatus::Running, now);
                    }
                    return Some(STOP_BLOCK_REASON.to_string());
                } else if status != WorkerStatus::Idle {
                    // Gia' sollecitato e ancora nessun report: aspetta l'utente.
                    self.set_status(tab, WorkerStatus::Idle, now);
                }
            }
            "session-end" if !status.is_terminal() => {
                if agent_session::switches_session(hook) {
                    let why = format!("the agent continues in a new session ({})", hook.reason.unwrap_or_default());
                    self.note_ignored_session_end(tab, &why, now);
                } else {
                    self.close_worker(tab, agent_session::SESSION_END_NOTE, now);
                }
            }
            _ => {}
        }
        None
    }

    /// L'utente ha premuto Invio nel terminale del worker: se aspettava lui,
    /// riprende a lavorare (nessun hook segnala la risposta).
    pub fn user_input(&mut self, tab: &str, now: i64) {
        if self.workers.get(tab).is_some_and(|w| w.tab_open && w.status.needs_attention()) {
            self.set_status(tab, WorkerStatus::Running, now);
        }
    }

    /// Il worker e' finito senza report: stato `closed`, task `failed` con la nota.
    fn close_worker(&mut self, session: &str, note: &str, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        w.status = WorkerStatus::Closed;
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        self.touch(session, now);
        if self.fail_task(&ws, &tid, Some(session), note, now) {
            self.notify(&ws, &tid, Some(session), "failed");
        }
    }

    /// Task `running` -> `failed` con una nota; `true` se era ancora in corso.
    fn fail_task(&mut self, ws: &str, tid: &str, session: Option<&str>, note: &str, now: i64) -> bool {
        let done = self
            .boards
            .get_mut(ws)
            .is_some_and(|b| b.finish_running(tid, session, TaskStatus::Failed, None, Some(note.to_string()), now));
        if done {
            self.mark_dirty(ws);
        }
        done
    }

    /// Il tab del worker non esiste piu' (chiuso dall'utente, dall'app o
    /// shell uscita). Un task ancora in corso fallisce; uno concluso con un
    /// worktree viene ripulito.
    pub fn tab_closed(&mut self, tab: &str, now: i64) {
        let Some(w) = self.workers.get_mut(tab) else {
            return;
        };
        if !w.tab_open {
            return;
        }
        w.tab_open = false;
        let (ws, tid, cleanup) = (w.workspace_id.clone(), w.task_id.clone(), std::mem::take(&mut w.cleanup_on_close));
        if !w.status.is_terminal() {
            self.close_worker(tab, "the tab was closed before complete_task", now);
        } else {
            self.touch(tab, now);
        }
        if cleanup {
            self.queue_cleanup(&ws, &tid);
        }
    }

    /// Worker con tab aperto (per ricostruire lo stato del frontend).
    pub fn open_tabs(&self) -> Vec<AutomationTabInfo> {
        let mut v: Vec<&Worker> = self.workers.values().filter(|w| w.tab_open).collect();
        v.sort_by_key(|w| w.created_at);
        v.into_iter().map(Worker::info).collect()
    }
}

// ---------------------------------------------------------------------------
// Punti d'ingresso con l'app reale
// ---------------------------------------------------------------------------

/// Punto d'ingresso delle RPC dal socket: autentica e smista.
pub fn handle_rpc(app: &AppHandle, req: &RpcRequest) -> RpcResponse {
    if let Err(e) = authenticate(req, token()) {
        return RpcResponse::failure(e);
    }
    let st = app.state::<AutomationState>();
    RpcResponse::from_result(rpc::dispatch(&st, &TauriHost(app.clone()), &req.session, &req.method, &req.params))
}

/// Evento di un hook con token valido (vedi `agent_notify`). Ritorna il
/// motivo di blocco se lo Stop hook di un worker va fatto proseguire.
pub fn on_hook_event(app: &AppHandle, payload: &crate::agent_notify::NotifyPayload) -> Option<String> {
    let tab = payload.tab_id.as_deref()?;
    let hook = HookEvent {
        event: &payload.event,
        notification_type: payload.notification_type.as_deref(),
        stop_hook_active: payload.stop_hook_active,
        background_work: payload.background_work,
        agent_session_id: payload.agent_session_id.as_deref(),
        source: payload.source.as_deref(),
        reason: payload.reason.as_deref(),
    };
    let st = app.state::<AutomationState>();
    st.run(&TauriHost(app.clone()), |r| r.on_hook(tab, &hook, now_millis()))
}

/// L'utente ha premuto Invio nel terminale del tab (chiamato da `pty_write`).
pub fn user_input(app: &AppHandle, tab: &str) {
    let st = app.state::<AutomationState>();
    st.run(&TauriHost(app.clone()), |r| r.user_input(tab, now_millis()));
}

/// Chiusura dell'app: salva le board con un salvataggio ancora in coda.
pub fn flush_on_exit(app: &AppHandle) {
    app.state::<AutomationState>().flush_pending_saves(&TauriHost(app.clone()));
}

/// Carica le board salvate (all'avvio dell'app), normalizzando quelle della
/// v0.11: i task `running` diventano `failed`; un file corrotto va in `.json.bak`.
pub fn load_boards(app: &tauri::App) {
    let Ok(dir) = worktrees::boards_dir(app.handle()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let st = app.state::<AutomationState>();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|raw| Board::from_json(&raw, now_millis()));
        match loaded {
            Ok(board) => st.load_board(board),
            Err(_) => {
                let _ = std::fs::rename(&path, path.with_extension("json.bak"));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Comandi Tauri
// ---------------------------------------------------------------------------

/// Il frontend ha chiuso un tab automatico (utente, app o shell uscita).
#[tauri::command]
pub async fn automation_tab_closed(app: AppHandle, tab_id: String) -> Result<(), String> {
    if !is_valid_tab_id(&tab_id) {
        return Err("id tab non valido".to_string());
    }
    // Il registro puo' essere occupato (callback dell'host): fuori dal main thread.
    crate::blocking(move || {
        let st = app.state::<AutomationState>();
        st.run(&TauriHost(app.clone()), |r| r.tab_closed(&tab_id, now_millis()));
        Ok(())
    })
    .await
}

/// Worker con tab aperto: il frontend ricostruisce lo stato (hot reload).
#[tauri::command]
pub async fn automation_sessions(app: AppHandle) -> Result<Vec<AutomationTabInfo>, String> {
    crate::blocking(move || Ok(app.state::<AutomationState>().open_tabs())).await
}

/// Board del workspace (vuota se non esiste).
#[tauri::command]
pub async fn board_get(app: AppHandle, workspace_id: String) -> Result<BoardView, String> {
    crate::blocking(move || app.state::<AutomationState>().view(&TauriHost(app.clone()), &workspace_id)).await
}

/// Imposta `max_parallel` e/o la pausa della coda.
#[tauri::command]
pub async fn board_set(
    app: AppHandle,
    workspace_id: String,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    crate::blocking(move || boards::board_set(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, max_parallel, paused))
        .await
}

/// Nuovo task creato a mano (senza proposta): `{ title, task, depends_on?, repo?, model? }`.
#[tauri::command]
pub async fn task_create(app: AppHandle, workspace_id: String, spec: Value) -> Result<BoardView, String> {
    crate::blocking(move || boards::task_create(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &spec)).await
}

/// Modifica di un task `queued` o `failed` (campi di `spec`, gli assenti restano).
#[tauri::command]
pub async fn task_update(app: AppHandle, workspace_id: String, task_id: String, patch: Value) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_update(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &task_id, &patch)
    })
    .await
}

/// Azione dell'utente su un task: `retry`, `mark_done`, `stop`, `delete` o `to_top`.
#[tauri::command]
pub async fn task_action(app: AppHandle, workspace_id: String, task_id: String, action: String) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_action(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &task_id, &action)
    })
    .await
}

/// Riordina un task `queued`: prima di `before_id`, o in fondo.
#[tauri::command]
pub async fn task_move(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    before_id: Option<String>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_move(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &task_id, before_id.as_deref())
    })
    .await
}

/// Accetta o scarta una proposta dell'Intake.
#[tauri::command]
pub async fn proposal_decide(
    app: AppHandle,
    workspace_id: String,
    proposal_id: String,
    accept: bool,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::proposal_decide(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &proposal_id, accept)
    })
    .await
}

/// Approva o scarta una proposta di impostazioni dell'agente.
#[tauri::command]
pub async fn settings_proposal_decide(
    app: AppHandle,
    workspace_id: String,
    proposal_id: String,
    accept: bool,
) -> Result<settings::SettingsDecision, String> {
    crate::blocking(move || {
        settings::settings_proposal_decide(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &proposal_id,
            accept,
        )
    })
    .await
}

/// Repository del workspace (root se e' un repo, sottocartelle con `.git`):
/// per la scelta del repository dei task. Git/disco fuori dal thread principale.
#[tauri::command]
pub async fn workspace_detect_repos(app: AppHandle, workspace_id: String) -> Result<Vec<String>, String> {
    crate::blocking(move || {
        let host = TauriHost(app);
        let ws = host.workspace(&workspace_id).ok_or_else(|| "workspace non trovato".to_string())?;
        host.list_repos(&ws)
    })
    .await
}

#[cfg(test)]
mod tests;
