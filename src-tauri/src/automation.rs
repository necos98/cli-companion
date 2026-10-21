//! Sessioni automatiche: un "orchestratore" (qualunque sessione di coding
//! agent aperta in un tab) avvia "worker" (altri tab PTY, marcati automatici)
//! e li governa via RPC sul socket locale (vedi `agent_notify`); i worker
//! rispondono solo tramite tool MCP (`get_task`, `report_fact`,
//! `request_help`, `complete_task`), mai leggendo il terminale.
//!
//! Struttura a strati, pensata per i test:
//! - `Registry`: nucleo PURO (niente `AppHandle`, niente I/O, tempo iniettato).
//!   Tiene worker, code di eventi per orchestratore e la decisione di
//!   "risveglio"; gli effetti da eseguire li accumula come `Effect`.
//! - `AutomationState`: `Mutex<Registry>` (managed state Tauri). `run`
//!   esegue una modifica, preleva gli effetti e li applica FUORI dal lock.
//!
//! L'orchestratore non resta mai in attesa dentro un tool: dopo gli spawn
//! chiude il turno e, quando arrivano eventi azionabili, cli-companion digita
//! `WAKE_TEXT` nel suo prompt; `get_events` gli consegna la coda.
//! Le board dei task (`board`, `boards`) seguono lo stesso schema: decisioni
//! pure nel `Registry` (scheduler incluso), avvio dei task (git + spawn) e
//! salvataggio come `Effect` eseguiti fuori dal lock.
//! - `Host`: gli effetti verso l'esterno (eventi Tauri, scrittura nel PTY,
//!   timer, workspace). `TauriHost` e' l'implementazione reale; i test usano
//!   un host finto, cosi' anche `dispatch` si prova senza finestra.
//!
//! Il token dell'app (`token()`) autentica le chiamate RPC e gli eventi degli
//! hook: solo i processi figli di un terminale di cli-companion lo conoscono.

use crate::agent_notify::{is_valid_tab_id, RpcRequest, RpcResponse};
use crate::agents::{self, AutomationRequest, PermissionMode};
use crate::board::{Board, BoardView};
use crate::schedule::{Schedule, ScheduleState};
use crate::workspaces::{Workspace, WorkspaceStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// Eventi Tauri verso il frontend.
const SPAWN_EVENT: &str = "automation-spawn";
const CLOSE_EVENT: &str = "automation-close";
const UPDATE_EVENT: &str = "automation-update";
const BOARD_EVENT: &str = "board-update";

mod boards;
pub mod auto;
pub use auto::{ORCHESTRATOR_BOOTSTRAP_PROMPT, WINDOW_CLOSED_TEXT};
pub use boards::{GitOut, StartJob};

/// Tetti e tempi.
pub const MAX_WORKERS_PER_ORCHESTRATOR: usize = 16;
pub const MAX_QUEUE_EVENTS: usize = 1000;
/// Worker (anche chiusi) tenuti in memoria prima di scartare i piu' vecchi.
const MAX_TRACKED_WORKERS: usize = 256;
/// Dopo `complete_task` il tab resta aperto il tempo di vedere l'esito.
pub const CLOSE_DELAY: Duration = Duration::from_millis(1500);
/// Pausa tra l'incolla e l'Invio: la TUI deve aver chiuso il bracketed paste.
pub const ENTER_DELAY: Duration = Duration::from_millis(300);

pub(crate) const TITLE_MAX: usize = 80;
pub(crate) const TASK_MAX: usize = 100_000;
const MESSAGE_MAX: usize = 20_000;
pub(crate) const SUMMARY_MAX: usize = 50_000;
const LAST_REPORT_MAX: usize = 200;
const CWD_MAX: usize = 1024;
pub(crate) const DEFAULT_AGENT: &str = "claude-code";

/// Prompt passato in riga di comando: ASCII semplice, sicuro in ogni shell
/// (niente virgolette, `$`, backtick, `%`, `!`, `\`). Lo legge il modello:
/// in inglese.
pub const BOOTSTRAP_PROMPT: &str =
    "You are an automated cli-companion session. Call the get_task tool right away and follow its instructions.";

/// Testo digitato nel prompt dell'orchestratore quando ci sono eventi nuovi.
pub const WAKE_TEXT: &str =
    "[cli-companion] New events from your automated sessions: call get_events, handle them, then end your turn.";

/// Motivo del blocco dello Stop hook: il worker ha chiuso il turno senza
/// riferire (`reason` e' mostrato al modello).
pub const STOP_BLOCK_REASON: &str = "You are an automated cli-companion worker and your turn ended without reporting. If the task is finished, call the complete_task tool now with a full summary. If you are blocked, call request_help. If you are waiting for a background command, keep waiting.";

/// Istruzioni restituite da `get_task` (le legge il modello: inglese).
pub const WORKER_INSTRUCTIONS: &str = "\
You are an automated cli-companion session, started by an orchestrator that assigned you the task above.\n\
- Work on the task autonomously, without asking the user for confirmation.\n\
- Nobody reads your terminal output: communicate ONLY through the cli-companion MCP tools.\n\
- report_fact: tell the orchestrator about useful facts that emerge (findings, decisions, important progress).\n\
- request_help: if you are blocked, ask the orchestrator a question and then wait; the answer will arrive as a message in your prompt.\n\
- complete_task: ALWAYS end the work with this tool, giving a status (completed, failed or blocked) and a full summary: what you did, files touched, test results. When you can, also fill the optional structured fields: commit, files_changed, tests (command, result, details) and follow_ups.\n\
- If get_task returns an `environment` section, follow it exactly (working folder, setup command, git rules).\n\
- Background commands are fine: if you start one and wait for it, your session resumes automatically when it finishes. The task must still always end with complete_task.\n\
If you finish without calling complete_task the orchestrator will not know the outcome.";

// ---------------------------------------------------------------------------
// Token
// ---------------------------------------------------------------------------

/// Token casuale dell'app (32 hex), generato al primo uso. Senza crate di
/// random: `RandomState` e' seminato dal sistema operativo.
pub fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(generate_token)
}

fn generate_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
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

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tipi
// ---------------------------------------------------------------------------

/// Ruolo di un tab: orchestratore = qualunque tab non registrato come worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Orchestrator,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    /// Spawnato, task non ancora letto.
    Starting,
    Running,
    /// Turno chiuso in attesa di lavoro in background (Claude Code riprende
    /// da solo): non e' un abbandono.
    Background,
    NeedsInput,
    /// L'agente ha finito il turno senza `complete_task`.
    Idle,
    Completed,
    Failed,
    Blocked,
    Closed,
}

impl WorkerStatus {
    /// Stati finali: dopo non ci sono piu' eventi derivati dagli hook.
    fn is_terminal(self) -> bool {
        matches!(
            self,
            WorkerStatus::Completed
                | WorkerStatus::Failed
                | WorkerStatus::Blocked
                | WorkerStatus::Closed
        )
    }

    fn from_final(s: &str) -> Option<WorkerStatus> {
        match s {
            "completed" => Some(WorkerStatus::Completed),
            "failed" => Some(WorkerStatus::Failed),
            "blocked" => Some(WorkerStatus::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Started,
    Fact,
    HelpRequest,
    Completed,
    NeedsInput,
    StoppedWithoutReport,
    Exited,
    /// Board: l'avvio di un task e' fallito (git, comando di lancio).
    StartFailed,
    /// Board: tutti i task sono `done` o `cancelled`.
    PlanCompleted,
    /// Board: l'utente ha agito dalla vista Board.
    UserAction,
}

impl EventKind {
    /// Eventi che richiedono l'attenzione dell'orchestratore e possono
    /// svegliarlo; `started`, `fact` e `user_action` restano in coda fino al
    /// prossimo `get_events`.
    fn is_actionable(self) -> bool {
        !matches!(self, EventKind::Started | EventKind::Fact | EventKind::UserAction)
    }
}

/// Evento nella coda di un orchestratore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub seq: u64,
    pub session: String,
    pub title: String,
    pub kind: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Stato del worker al momento dell'evento.
    pub status: WorkerStatus,
    /// Unix ms.
    pub at: i64,
    /// Id del task, se il worker (o l'evento) e' di una board.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Workspace della board (non esposto): serve a spostare gli eventi al
    /// cambio di proprietario.
    #[serde(skip)]
    pub board: Option<String>,
}

/// Stato di un worker per il frontend (evento `automation-update` e comando
/// `automation_sessions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AutomationTabInfo {
    pub tab_id: String,
    pub status: WorkerStatus,
    pub title: String,
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
    pub parent_tab_id: String,
    pub agent_id: String,
    /// `worker` (task) oppure `orchestrator` (orchestratore automatico di
    /// una finestra di esecuzione: nessun genitore, metadati automatici).
    pub role: Role,
}

/// Voce di `list_sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    pub session: String,
    pub title: String,
    pub workspace: String,
    pub status: WorkerStatus,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_report: Option<String>,
    /// Id del task, per i worker di una board.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

/// Dati di un worker da registrare (gia' validati e con il comando pronto).
#[derive(Debug, Clone)]
pub struct NewWorker {
    pub workspace_id: String,
    pub workspace_name: String,
    pub agent_id: String,
    pub title: String,
    pub task: String,
    pub cwd: Option<String>,
    pub keep_open: bool,
    /// Comando da digitare nella shell del nuovo tab.
    pub command: String,
}

/// Effetto da eseguire fuori dal lock, accumulato dal `Registry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Update(AutomationTabInfo),
    Close { tab_id: String, delay: Duration },
    /// Digitare `WAKE_TEXT` nel prompt di questo tab.
    Wake(String),
    /// Avviare un task di board (git + spawn) su un thread separato.
    StartTask(Box<StartJob>),
    /// Salvare su disco e notificare al frontend la board (`board-update`).
    Publish(Box<Board>),
    /// La board del workspace e' stata rimossa: cancellare il file. `rev` e'
    /// una revisione di rimozione: salvataggi piu' vecchi in ritardo vengono
    /// scartati (niente file resuscitato).
    DeleteBoard(String, u64),
    /// Digitare `text` nel prompt del tab (l'orchestratore e' idle).
    Type { tab: String, text: &'static str },
    /// Aprire il tab dell'orchestratore automatico (`automation-spawn`).
    SpawnTab(Box<SpawnEvent>),
    /// Prendere (`on`) o rilasciare il blocco dello standby. `seq` ordina gli
    /// effetti applicati fuori dal lock da thread diversi: uno vecchio in
    /// ritardo viene scartato.
    KeepAwake { on: bool, seq: u64 },
    /// Emettere `schedule-state`.
    Schedule(Box<ScheduleState>),
}

struct Worker {
    session: String,
    title: String,
    task: String,
    orchestrator: String,
    workspace_name: String,
    keep_open: bool,
    status: WorkerStatus,
    created_at: i64,
    updated_at: i64,
    last_report: Option<String>,
    started_sent: bool,
    /// `request_help` in corso: il Stop che segue non e' un abbandono.
    help_pending: bool,
    /// Il tab esiste ancora.
    tab_open: bool,
    /// (workspace, id task) se il worker lavora per un task di board.
    board_task: Option<(String, String)>,
}

impl Worker {
    fn info(&self) -> AutomationTabInfo {
        AutomationTabInfo {
            tab_id: self.session.clone(),
            status: self.status,
            title: self.title.clone(),
            parent_tab_id: self.orchestrator.clone(),
            last_report: self.last_report.clone(),
        }
    }
}

#[derive(Default)]
struct Queue {
    events: VecDeque<Event>,
    /// Messaggio di risveglio gia' digitato e non ancora "consumato".
    wake_pending: bool,
    /// `seq` dell'ultimo evento azionabile in coda al momento del risveglio:
    /// si risveglia di nuovo solo per eventi azionabili piu' recenti.
    woke_seq: u64,
}

// ---------------------------------------------------------------------------
// Registry (nucleo puro)
// ---------------------------------------------------------------------------

/// Evento di un hook come lo vede il registro.
#[derive(Debug, Clone, Copy, Default)]
pub struct HookEvent<'a> {
    pub event: &'a str,
    pub notification_type: Option<&'a str>,
    pub message: Option<&'a str>,
    pub stop_hook_active: Option<bool>,
    pub background_work: Option<u32>,
}

#[derive(Default)]
pub struct Registry {
    workers: HashMap<String, Worker>,
    queues: HashMap<String, Queue>,
    /// Ultimo evento hook (con token valido) per tab: decide il risveglio.
    last_hook: HashMap<String, String>,
    next_seq: u64,
    next_worker: u64,
    effects: Vec<Effect>,
    /// Board per workspace (vedi `boards`).
    boards: HashMap<String, Board>,
    /// Eventi delle board senza proprietario: passano al prossimo owner.
    board_queues: HashMap<String, VecDeque<Event>>,
    /// Workspace con board modificata da schedulare/salvare/notificare.
    dirty: Vec<String>,
    /// Ultima revisione assegnata alle board (contatore globale monotono:
    /// resta crescente anche se una board sparisce e ricompare).
    next_rev: u64,
    /// Finestre di esecuzione e orchestratori automatici (vedi `auto`).
    auto: auto::AutoCore,
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

impl Registry {
    pub fn role(&self, session: &str) -> Role {
        if self.workers.contains_key(session) {
            Role::Worker
        } else {
            Role::Orchestrator
        }
    }

    /// Effetti accumulati dall'ultima chiamata.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    fn require_orchestrator(&self, session: &str) -> Result<(), String> {
        if self.role(session) == Role::Orchestrator {
            Ok(())
        } else {
            Err("reserved for the orchestrator".to_string())
        }
    }

    fn active_count(&self, orch: &str) -> usize {
        self.workers
            .values()
            .filter(|w| w.orchestrator == orch && w.tab_open)
            .count()
    }

    /// Ruolo e tetto di worker attivi: da controllare prima di lavori costosi.
    pub fn check_spawn(&self, orch: &str) -> Result<(), String> {
        self.require_orchestrator(orch)?;
        if self.active_count(orch) >= MAX_WORKERS_PER_ORCHESTRATOR {
            return Err(format!(
                "too many active sessions (max {MAX_WORKERS_PER_ORCHESTRATOR}): close some"
            ));
        }
        Ok(())
    }

    /// Registra un nuovo worker figlio di `orch`.
    pub fn spawn(&mut self, orch: &str, new: NewWorker, now: i64) -> Result<SpawnEvent, String> {
        self.check_spawn(orch)?;
        Ok(self.spawn_inner(orch, new, now, None))
    }

    /// Registra il worker senza controlli. `orch` vuoto = task di una board
    /// senza proprietario (gli eventi aspettano nella coda della board).
    fn spawn_inner(
        &mut self,
        orch: &str,
        new: NewWorker,
        now: i64,
        board_task: Option<(String, String)>,
    ) -> SpawnEvent {
        self.next_worker += 1;
        let id = format!("auto-{now}-{}", self.next_worker);
        let ev = SpawnEvent {
            tab_id: id.clone(),
            workspace_id: new.workspace_id.clone(),
            command: new.command,
            cwd: new.cwd.clone(),
            title: new.title.clone(),
            parent_tab_id: orch.to_string(),
            agent_id: new.agent_id.clone(),
            role: Role::Worker,
        };
        self.workers.insert(
            id.clone(),
            Worker {
                session: id,
                title: new.title,
                task: new.task,
                orchestrator: orch.to_string(),
                workspace_name: new.workspace_name,
                keep_open: new.keep_open,
                status: WorkerStatus::Starting,
                created_at: now,
                updated_at: now,
                last_report: None,
                started_sent: false,
                help_pending: false,
                tab_open: true,
                board_task,
            },
        );
        self.prune();
        ev
    }

    /// Scarta i worker chiusi piu' vecchi oltre il tetto di memoria.
    fn prune(&mut self) {
        while self.workers.len() > MAX_TRACKED_WORKERS {
            let oldest = self
                .workers
                .values()
                .filter(|w| !w.tab_open)
                .min_by_key(|w| w.created_at)
                .map(|w| w.session.clone());
            match oldest {
                Some(id) => {
                    self.workers.remove(&id);
                }
                None => break,
            }
        }
    }

    fn worker_mut(&mut self, session: &str) -> Result<&mut Worker, String> {
        self.workers
            .get_mut(session)
            .ok_or_else(|| "reserved for automated sessions".to_string())
    }

    /// Worker di `orch` (per tutti gli altri: stesso errore, nessuna fuga).
    fn owned_worker(&self, orch: &str, session: &str) -> Result<&Worker, String> {
        self.require_orchestrator(orch)?;
        self.workers
            .get(session)
            .filter(|w| w.orchestrator == orch)
            .ok_or_else(|| "session not found".to_string())
    }

    fn touch(&mut self, session: &str, now: i64) {
        if let Some(w) = self.workers.get_mut(session) {
            w.updated_at = now;
            let info = w.info();
            self.effects.push(Effect::Update(info));
        }
        self.sync_task(session, now);
    }

    fn push_event(&mut self, session: &str, kind: EventKind, text: Option<String>, now: i64) {
        let Some(w) = self.workers.get(session) else {
            return;
        };
        // Worker di board senza proprietario: l'evento aspetta nella coda
        // della board (orchestratore vuoto).
        let target = Some(w.orchestrator.clone()).filter(|o| !o.is_empty());
        let (board, task) = match &w.board_task {
            Some((ws, t)) => (Some(ws.clone()), Some(t.clone())),
            None => (None, None),
        };
        self.next_seq += 1;
        let ev = Event {
            seq: self.next_seq,
            session: session.to_string(),
            title: w.title.clone(),
            kind,
            text,
            status: w.status,
            at: now,
            task,
            board,
        };
        self.enqueue(target, ev);
    }

    /// Mette l'evento nella coda dell'orchestratore (o, senza, in quella
    /// della board) e valuta il risveglio.
    fn enqueue(&mut self, target: Option<String>, ev: Event) {
        match target {
            Some(orch) => {
                let q = self.queues.entry(orch.clone()).or_default();
                q.events.push_back(ev);
                while q.events.len() > MAX_QUEUE_EVENTS {
                    q.events.pop_front();
                }
                self.check_wake(&orch);
            }
            None => {
                if let Some(ws) = ev.board.clone() {
                    let q = self.board_queues.entry(ws).or_default();
                    q.push_back(ev);
                    while q.len() > MAX_QUEUE_EVENTS {
                        q.pop_front();
                    }
                }
            }
        }
    }

    /// Decide se svegliare l'orchestratore `tab`: ci sono eventi azionabili
    /// in coda (`started` e `fact` no: li consegna il prossimo
    /// `get_events`), il suo ultimo hook e' `finished` (idle al prompt: con
    /// `needs-input` il testo finirebbe in un dialogo di permesso) e, se
    /// l'abbiamo gia' svegliato, sono arrivati
    /// eventi azionabili nuovi da allora. Un orchestratore che ignora il
    /// risveglio e chiude il turno viene quindi risvegliato al prossimo
    /// evento, non a ogni turno.
    fn check_wake(&mut self, tab: &str) {
        let idle = self.last_hook.get(tab).map(String::as_str) == Some("finished");
        let Some(q) = self.queues.get_mut(tab) else {
            return;
        };
        let Some(last_seq) = q.events.iter().rev().find(|e| e.kind.is_actionable()).map(|e| e.seq)
        else {
            return;
        };
        if !idle || (q.wake_pending && last_seq <= q.woke_seq) {
            return;
        }
        q.wake_pending = true;
        q.woke_seq = last_seq;
        // Dopo il risveglio l'orchestratore lavora: niente altri messaggi
        // digitati finche' il suo prossimo `finished` non lo riporta idle.
        self.last_hook.insert(tab.to_string(), "woken".to_string());
        self.effects.push(Effect::Wake(tab.to_string()));
    }

    // --- worker ---

    /// `get_task`: il worker legge il task. La prima volta passa a `running`
    /// e avvisa l'orchestratore.
    pub fn get_task(&mut self, session: &str, now: i64) -> Result<Value, String> {
        self.get_task_in(session, now, None)
    }

    /// Come `get_task`; `ws` (il workspace della board) permette di
    /// descrivere l'ambiente di un task di board.
    pub fn get_task_in(&mut self, session: &str, now: i64, ws: Option<&Workspace>) -> Result<Value, String> {
        let env = self.board_environment(session, ws);
        let w = self.worker_mut(session)?;
        let mut out = json!({
            "session": w.session,
            "title": w.title,
            "task": w.task,
            "orchestrator": w.orchestrator,
            "instructions": WORKER_INSTRUCTIONS,
        });
        if let Some((_, tid)) = &w.board_task {
            out["task_id"] = json!(tid);
        }
        if let Some(env) = env {
            out["environment"] = json!(env);
        }
        let first = !w.started_sent;
        w.started_sent = true;
        if w.status == WorkerStatus::Starting {
            w.status = WorkerStatus::Running;
        }
        if first {
            self.push_event(session, EventKind::Started, None, now);
            self.touch(session, now);
        }
        Ok(out)
    }

    fn worker_activity(&mut self, session: &str) -> Result<&mut Worker, String> {
        let w = self.worker_mut(session)?;
        if w.status == WorkerStatus::Closed || !w.tab_open {
            return Err("the session is closed".to_string());
        }
        // Un'azione del worker prova che sta lavorando.
        if matches!(
            w.status,
            WorkerStatus::NeedsInput
                | WorkerStatus::Idle
                | WorkerStatus::Starting
                | WorkerStatus::Background
        ) {
            w.status = WorkerStatus::Running;
        }
        w.help_pending = false;
        w.started_sent = true;
        Ok(w)
    }

    /// `report_fact`.
    pub fn report_fact(&mut self, session: &str, text: &str, now: i64) -> Result<(), String> {
        let text = req_text("text", text, MESSAGE_MAX)?;
        let w = self.worker_activity(session)?;
        w.last_report = Some(clip_chars(&text, LAST_REPORT_MAX));
        self.push_event(session, EventKind::Fact, Some(text), now);
        self.touch(session, now);
        Ok(())
    }

    /// `request_help`: lo stato diventa `needs_input` finche' non arriva la
    /// risposta (o il worker riprende da solo).
    pub fn request_help(&mut self, session: &str, question: &str, now: i64) -> Result<(), String> {
        let q = req_text("question", question, MESSAGE_MAX)?;
        let w = self.worker_activity(session)?;
        w.status = WorkerStatus::NeedsInput;
        w.help_pending = true;
        w.last_report = Some(clip_chars(&q, LAST_REPORT_MAX));
        self.push_event(session, EventKind::HelpRequest, Some(q), now);
        self.touch(session, now);
        Ok(())
    }

    /// `complete_task`: ritorna se il tab sta per chiudersi.
    pub fn complete_task(
        &mut self,
        session: &str,
        status: &str,
        summary: &str,
        now: i64,
    ) -> Result<bool, String> {
        self.complete_task_report(session, status, summary, crate::board::ReportFields::default(), None, now)
    }

    /// `complete_task` con i campi strutturati e il diffstat calcolato
    /// dall'app (task di board con worktree).
    pub fn complete_task_report(
        &mut self,
        session: &str,
        status: &str,
        summary: &str,
        fields: crate::board::ReportFields,
        diffstat: Option<String>,
        now: i64,
    ) -> Result<bool, String> {
        let status_str = status;
        let status = WorkerStatus::from_final(status)
            .ok_or_else(|| "invalid status (use completed, failed or blocked)".to_string())?;
        let summary = req_text("summary", summary, SUMMARY_MAX)?;
        let w = self.worker_activity(session)?;
        w.status = status;
        w.last_report = Some(clip_chars(&summary, LAST_REPORT_MAX));
        let closing = !w.keep_open;
        self.push_event(session, EventKind::Completed, Some(summary.clone()), now);
        self.touch(session, now);
        self.finish_board_task(session, status_str, &summary, fields, diffstat, now);
        let orch = self.workers.get(session).map(|w| w.orchestrator.clone()).unwrap_or_default();
        self.mark_owner_boards_dirty(&orch);
        if closing {
            self.effects.push(Effect::Close {
                tab_id: session.to_string(),
                delay: CLOSE_DELAY,
            });
        }
        Ok(closing)
    }

    // --- orchestratore ---

    /// Controlla che `orch` possa scrivere a `session` e aggiorna lo stato
    /// (il worker riprende a lavorare). La scrittura nel PTY e' dell'host.
    pub fn send_message(&mut self, orch: &str, session: &str, now: i64) -> Result<(), String> {
        let w = self.owned_worker(orch, session)?;
        if !w.tab_open || w.status == WorkerStatus::Closed {
            return Err("the session is closed".to_string());
        }
        let resumes = matches!(
            w.status,
            WorkerStatus::NeedsInput
                | WorkerStatus::Background
                | WorkerStatus::Idle
                | WorkerStatus::Completed
                | WorkerStatus::Failed
                | WorkerStatus::Blocked
        );
        if let Some(w) = self.workers.get_mut(session) {
            w.help_pending = false;
            if resumes {
                w.status = WorkerStatus::Running;
            }
        }
        if resumes {
            self.touch(session, now);
        }
        Ok(())
    }

    /// `close_session`: chiede la chiusura del tab. Gli stati finali
    /// (completed/failed/blocked) si conservano; gli altri diventano `closed`
    /// subito, cosi' la chiusura non genera un evento `exited` di rimbalzo.
    pub fn close_session(&mut self, orch: &str, session: &str, now: i64) -> Result<(), String> {
        let w = self.owned_worker(orch, session)?;
        if !w.tab_open {
            return Err("the session is already closed".to_string());
        }
        if let Some(w) = self.workers.get_mut(session) {
            if !w.status.is_terminal() {
                w.status = WorkerStatus::Closed;
            }
        }
        self.touch(session, now);
        self.effects.push(Effect::Close {
            tab_id: session.to_string(),
            delay: Duration::ZERO,
        });
        Ok(())
    }

    pub fn list_sessions(&self, orch: &str) -> Vec<SessionInfo> {
        let mut v: Vec<&Worker> = self
            .workers
            .values()
            .filter(|w| w.orchestrator == orch)
            .collect();
        v.sort_by_key(|w| w.created_at);
        v.into_iter()
            .map(|w| SessionInfo {
                session: w.session.clone(),
                title: w.title.clone(),
                workspace: w.workspace_name.clone(),
                status: w.status,
                created_at: w.created_at,
                updated_at: w.updated_at,
                last_report: w.last_report.clone(),
                task: w.board_task.as_ref().map(|t| t.1.clone()),
            })
            .collect()
    }

    /// Worker con tab aperto (per ricostruire lo stato del frontend).
    pub fn open_tabs(&self) -> Vec<AutomationTabInfo> {
        let mut v: Vec<&Worker> = self.workers.values().filter(|w| w.tab_open).collect();
        v.sort_by_key(|w| w.created_at);
        v.into_iter().map(Worker::info).collect()
    }

    // --- hook e chiusura tab ---

    /// Evento di un hook con token valido. Per i worker genera gli eventi
    /// derivati; per ogni tab ricorda l'ultimo hook (decide il risveglio).
    /// Ritorna `Some(motivo)` se lo Stop hook va bloccato: il worker ha
    /// chiuso il turno senza riferire e Claude Code deve proseguire.
    pub fn on_hook(&mut self, tab: &str, hook: &HookEvent, now: i64) -> Option<String> {
        if self.last_hook.len() > 512 {
            self.last_hook.clear();
        }
        // `idle_prompt` (~60 s dopo la fine del turno) conferma che il tab e'
        // fermo al prompt: per il risveglio vale come `finished`, non come
        // una richiesta di permesso.
        let last = if hook.notification_type == Some("idle_prompt") { "finished" } else { hook.event };
        self.last_hook.insert(tab.to_string(), last.to_string());
        let Some(w) = self.workers.get_mut(tab) else {
            // Orchestratore automatico: testo di chiusura / drain prima del
            // risveglio (il testo di chiusura chiede gia' get_events).
            self.auto_hook(tab, hook.event);
            self.check_wake(tab);
            return None;
        };
        let (status, help_pending) = (w.status, w.help_pending);
        match hook.event {
            // `idle_prompt` (~60 s dopo la fine del turno) non e' una
            // richiesta di permesso: il worker non e' fermo su un dialogo.
            "needs-input" if hook.notification_type == Some("idle_prompt") => {}
            "needs-input"
                if matches!(
                    status,
                    WorkerStatus::Starting
                        | WorkerStatus::Running
                        | WorkerStatus::Background
                        | WorkerStatus::Idle
                ) =>
            {
                w.status = WorkerStatus::NeedsInput;
                let text = hook.message.map(str::to_string);
                self.push_event(tab, EventKind::NeedsInput, text, now);
                self.touch(tab, now);
            }
            "finished"
                if matches!(
                    status,
                    WorkerStatus::Starting
                        | WorkerStatus::Running
                        | WorkerStatus::Background
                        | WorkerStatus::NeedsInput
                ) && !help_pending =>
            {
                // Lavoro in background attivo: Claude Code riprende da solo.
                if hook.background_work.is_some_and(|n| n > 0) {
                    if status != WorkerStatus::Background {
                        w.status = WorkerStatus::Background;
                        self.touch(tab, now);
                    }
                } else if hook.stop_hook_active != Some(true) {
                    // Prima volta senza report: si fa proseguire il turno.
                    return Some(STOP_BLOCK_REASON.to_string());
                } else {
                    // Gia' sollecitato e ancora nessun report: abbandono.
                    w.status = WorkerStatus::Idle;
                    self.push_event(tab, EventKind::StoppedWithoutReport, None, now);
                    self.touch(tab, now);
                }
            }
            "session-end" if !status.is_terminal() => {
                w.status = WorkerStatus::Closed;
                self.push_event(tab, EventKind::Exited, None, now);
                self.touch(tab, now);
                self.fail_board_task(tab, "the session ended without a complete_task report", now);
                let orch = self.workers.get(tab).map(|w| w.orchestrator.clone()).unwrap_or_default();
                self.mark_owner_boards_dirty(&orch);
            }
            _ => {}
        }
        None
    }

    /// Il tab del worker non esiste piu' (chiuso dall'utente, dall'app o
    /// shell uscita). `exited` solo se non era gia' finito o chiuso.
    pub fn tab_closed(&mut self, tab: &str, now: i64) {
        let Some(w) = self.workers.get_mut(tab) else {
            self.auto_tab_closed(tab);
            return;
        };
        if !w.tab_open {
            return;
        }
        w.tab_open = false;
        let unfinished = !w.status.is_terminal();
        if unfinished {
            w.status = WorkerStatus::Closed;
            self.push_event(tab, EventKind::Exited, None, now);
        }
        self.touch(tab, now);
        self.fail_board_task(tab, "the tab was closed before complete_task", now);
        // Il tab libera un posto nel tetto di worker: i task in coda
        // tornati indietro per il tetto pieno possono ripartire.
        let orch = self.workers.get(tab).map(|w| w.orchestrator.clone()).unwrap_or_default();
        self.mark_owner_boards_dirty(&orch);
    }

    // --- coda ---

    /// `get_events`: svuota la coda dell'orchestratore; il risveglio digitato
    /// e' consumato.
    pub fn drain(&mut self, orch: &str) -> Vec<Event> {
        let Some(q) = self.queues.get_mut(orch) else {
            return Vec::new();
        };
        q.wake_pending = false;
        q.events.drain(..).collect()
    }
}

// ---------------------------------------------------------------------------
// Validazione dei parametri di spawn_session
// ---------------------------------------------------------------------------

/// Parametri di `spawn_session` dopo la validazione sintattica.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnParams {
    pub task: String,
    pub title: String,
    pub agent: String,
    pub cwd: Option<String>,
    /// `None` = non richiesto: vale il tetto del workspace.
    pub permission_mode: Option<PermissionMode>,
    pub model: Option<String>,
    pub keep_open: bool,
}

/// Permesso effettivo di un worker: assente = il tetto del workspace;
/// richiesto = ammesso solo se non piu' permissivo del tetto.
pub fn resolve_permission(
    requested: Option<PermissionMode>,
    max: PermissionMode,
) -> Result<PermissionMode, String> {
    match requested {
        None => Ok(max),
        Some(m) if m.rank() <= max.rank() => Ok(m),
        Some(m) => Err(format!(
            "permission_mode '{}' is not allowed: the maximum for this workspace is '{}' (the user changes it in the workspace settings in cli-companion)",
            m.as_str(),
            max.as_str()
        )),
    }
}

/// Campo stringa opzionale: assente o `null` = `None`; di altro tipo = errore.
pub(crate) fn opt_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

pub(crate) fn req_str<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(p, key)?.ok_or_else(|| format!("{key} is missing"))
}

/// Titolo di ripiego: le prime parole del task.
pub(crate) fn default_title(task: &str) -> String {
    let words: Vec<&str> = task.split_whitespace().take(8).collect();
    clip_chars(&words.join(" "), TITLE_MAX).trim().to_string()
}

pub(crate) fn valid_model(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

pub fn parse_spawn_params(p: &Value) -> Result<SpawnParams, String> {
    if p.get("workspace").is_some_and(|v| !v.is_null()) {
        return Err(
            "workspace is not allowed: the worker opens in the orchestrator's workspace".to_string(),
        );
    }
    let task = req_text("task", req_str(p, "task")?, TASK_MAX)?;
    let title = match opt_str(p, "title")?.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.chars().count() > TITLE_MAX => {
            return Err(format!("title is too long (max {TITLE_MAX} characters)"))
        }
        Some(t) => t.to_string(),
        None => default_title(&task),
    };
    let agent = opt_str(p, "agent")?
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .unwrap_or(DEFAULT_AGENT)
        .to_string();
    let cwd = match opt_str(p, "cwd")?.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) if c.chars().count() > CWD_MAX || c.chars().any(char::is_control) => {
            return Err("invalid cwd".to_string())
        }
        Some(c) => Some(c.to_string()),
        None => None,
    };
    let permission_mode = match opt_str(p, "permission_mode")? {
        None => None,
        Some(m) => Some(PermissionMode::parse(m).ok_or_else(|| {
            "invalid permission_mode (use default, accept-edits, plan or bypass)".to_string()
        })?),
    };
    let model = match opt_str(p, "model")?.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if !valid_model(m) => {
            return Err("invalid model (only letters, digits, '.', '_' and '-', max 64)".to_string())
        }
        Some(m) => Some(m.to_string()),
        None => None,
    };
    let keep_open = match p.get("keep_open") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("keep_open must be a boolean".to_string()),
    };
    Ok(SpawnParams {
        task,
        title,
        agent,
        cwd,
        permission_mode,
        model,
        keep_open,
    })
}

/// Il `cwd` e' un path assoluto nell'ambiente del workspace?
fn cwd_is_absolute(kind: &str, path: &str) -> bool {
    if kind == "wsl" {
        return path.starts_with('/');
    }
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\');
    drive || path.starts_with("\\\\") || path.starts_with("//")
}

/// Il `cwd` (gia' assoluto) e' la cartella del workspace o una sua
/// sottocartella? Niente segmenti `..`. Windows: senza distinguere maiuscole
/// e con `\` e `/` equivalenti; WSL: confronto per segmenti su `/`.
fn cwd_inside_workspace(kind: &str, root: &str, cwd: &str) -> bool {
    let wsl = kind == "wsl";
    let split = |s: &str| -> Vec<String> {
        s.split(|c| c == '/' || (!wsl && c == '\\'))
            .filter(|seg| !seg.is_empty())
            .map(|seg| if wsl { seg.to_string() } else { seg.to_lowercase() })
            .collect()
    };
    let (root, cwd) = (split(root), split(cwd));
    !cwd.iter().any(|seg| seg == "..") && cwd.len() >= root.len() && cwd[..root.len()] == root[..]
}

/// Testo sicuro per il bracketed paste: niente caratteri di controllo (in
/// particolare ESC, che potrebbe chiudere l'incolla in anticipo); restano
/// a capo e tab.
fn sanitize_paste(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

// ---------------------------------------------------------------------------
// Stato Tauri + effetti
// ---------------------------------------------------------------------------

/// Effetti verso l'esterno. `Clone + Send + 'static`: i timer li portano in
/// un thread.
pub trait Host: Clone + Send + 'static {
    /// Workspace in cui e' aperto il tab (None se il tab non ha un pty vivo).
    fn tab_workspace(&self, tab_id: &str) -> Option<Workspace>;
    /// La cartella esiste nell'ambiente del workspace?
    fn dir_exists(&self, ws: &Workspace, path: &str) -> bool;
    fn agents(&self) -> Vec<(String, String)>;
    fn launch_command(
        &self,
        agent: &str,
        env_kind: &str,
        req: &AutomationRequest,
    ) -> Result<String, String>;
    fn emit_spawn(&self, ev: &SpawnEvent);
    fn emit_close(&self, tab_id: &str);
    fn emit_update(&self, info: &AutomationTabInfo);
    fn write_to_tab(&self, tab_id: &str, data: &str) -> Result<(), String>;
    /// Esegue `job` dopo `delay` su un altro thread.
    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>);
    /// Workspace per id.
    fn workspace(&self, id: &str) -> Option<Workspace>;
    /// Esegue `git <args>` in `dir` nell'ambiente del workspace (argv, mai
    /// una shell). `Err` solo se git non parte.
    fn git(&self, ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String>;
    /// Aggiunge `line` al file `file` (info/exclude) se manca.
    fn append_exclude(&self, ws: &Workspace, file: &str, line: &str) -> Result<(), String>;
    fn save_board(&self, board: &Board) -> Result<(), String>;
    fn delete_board(&self, ws_id: &str);
    fn emit_board(&self, view: &BoardView);
    /// Tutti i workspace.
    fn workspaces(&self) -> Vec<Workspace>;
    /// Minuti locali del giorno all'istante `now` (unix ms).
    fn local_minute(&self, now: i64) -> u16;
    fn emit_schedule(&self, state: &ScheduleState);
    fn save_schedule(&self, s: &Schedule) -> Result<(), String>;
    /// Prende o rilascia il blocco dello standby.
    fn set_keep_awake(&self, on: bool);
    fn workflow_read(&self, ws_id: &str) -> Result<Option<String>, String>;
    /// `None` elimina il workflow.
    fn workflow_write(&self, ws_id: &str, content: Option<&str>) -> Result<(), String>;
}

/// Salvataggi coalescenti: al massimo uno per board ogni `SAVE_EVERY_MS`,
/// sempre con l'ultimo stato (un report_fact dopo l'altro non riscrive il
/// JSON intero ogni volta). L'evento `board-update` non e' coalescente.
pub(super) const SAVE_EVERY_MS: i64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SaveDecision {
    /// Salvare subito.
    Now,
    /// Un salvataggio e' in coda: armare un timer fra `Duration`.
    Arm(Duration),
    /// Gia' in coda con un timer armato: lo stato nuovo lo sostituisce.
    Queued,
}

#[derive(Default)]
struct SaveSlot {
    last_at: Option<i64>,
    pending: Option<Board>,
    armed: bool,
}

/// Parte pura della coalescenza (tempo iniettato).
#[derive(Default)]
pub(super) struct SaveGate {
    slots: HashMap<String, SaveSlot>,
}

impl SaveGate {
    pub(super) fn offer(&mut self, board: &Board, now: i64) -> SaveDecision {
        let s = self.slots.entry(board.workspace_id.clone()).or_default();
        let since = s.last_at.map_or(i64::MAX, |t| now - t);
        if s.pending.is_none() && !s.armed && since >= SAVE_EVERY_MS {
            s.last_at = Some(now);
            return SaveDecision::Now;
        }
        s.pending = Some(board.clone());
        if s.armed {
            SaveDecision::Queued
        } else {
            s.armed = true;
            SaveDecision::Arm(Duration::from_millis((SAVE_EVERY_MS - since).clamp(0, SAVE_EVERY_MS) as u64))
        }
    }

    /// Il timer e' scattato: l'ultimo stato da salvare, se c'e'.
    pub(super) fn take(&mut self, ws: &str, now: i64) -> Option<Board> {
        let s = self.slots.get_mut(ws)?;
        s.armed = false;
        s.last_at = Some(now);
        s.pending.take()
    }

    pub(super) fn take_all(&mut self) -> Vec<Board> {
        self.slots.values_mut().filter_map(|s| s.pending.take()).collect()
    }

    pub(super) fn forget(&mut self, ws: &str) {
        self.slots.remove(ws);
    }
}

/// Managed state Tauri. `Clone` (Arc): i thread di avvio dei task lo portano
/// con se' per rientrare dopo il lavoro git.
#[derive(Default, Clone)]
pub struct AutomationState {
    inner: Arc<Mutex<Registry>>,
    /// Ultima revisione pubblicata (emessa) per board: una pubblicazione in
    /// ritardo non ne sovrascrive una piu' recente. Alla rimozione di una
    /// board ci resta la revisione di rimozione (lapide). Ordine dei lock:
    /// `published` poi `saves`; mai con `inner` in mano.
    published: Arc<Mutex<HashMap<String, u64>>>,
    saves: Arc<Mutex<SaveGate>>,
    /// Ultimo `seq` di `KeepAwake` applicato.
    awake_applied: Arc<Mutex<u64>>,
    /// Un avvio alla volta per task (git worktree sullo stesso path).
    starts: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl AutomationState {
    /// Esegue `f` sul registro, poi schedula le board modificate e applica
    /// gli effetti fuori dal lock.
    pub fn run<H: Host, T>(&self, host: &H, f: impl FnOnce(&mut Registry) -> T) -> T {
        let (out, effects) = {
            let mut reg = self.inner.lock().unwrap();
            let out = f(&mut reg);
            let now = now_millis();
            reg.flush_boards(now, host.local_minute(now), &|id| host.workspace(id));
            (out, reg.take_effects())
        };
        apply_effects(self, host, effects);
        out
    }

    fn save<H: Host>(host: &H, board: &Board) {
        if let Err(e) = host.save_board(board) {
            eprintln!("board {}: {e}", board.workspace_id);
        }
    }

    /// Emette la board se e' piu' recente dell'ultima pubblicata e la salva
    /// (coalescendo i salvataggi ravvicinati).
    fn publish<H: Host>(&self, host: &H, board: &Board) {
        let decision = {
            let mut map = self.published.lock().unwrap();
            if board.rev <= map.get(&board.workspace_id).copied().unwrap_or(0) {
                return;
            }
            map.insert(board.workspace_id.clone(), board.rev);
            host.emit_board(&board.view());
            let d = self.saves.lock().unwrap().offer(board, now_millis());
            if d == SaveDecision::Now {
                Self::save(host, board);
            }
            d
        };
        // Il timer fuori dal lock: un host sincrono lo eseguirebbe subito.
        if let SaveDecision::Arm(delay) = decision {
            let (st, h, ws) = (self.clone(), host.clone(), board.workspace_id.clone());
            host.defer(delay, Box::new(move || st.flush_save(&h, &ws)));
        }
    }

    /// Timer di salvataggio scattato.
    fn flush_save<H: Host>(&self, host: &H, ws: &str) {
        let map = self.published.lock().unwrap();
        let Some(b) = self.saves.lock().unwrap().take(ws, now_millis()) else {
            return;
        };
        // Board rimossa nel frattempo (lapide): niente file resuscitato.
        if map.get(ws) == Some(&b.rev) {
            Self::save(host, &b);
        }
    }

    /// Chiusura dell'app: salva i salvataggi ancora in coda.
    pub fn flush_pending_saves<H: Host>(&self, host: &H) {
        let map = self.published.lock().unwrap();
        for b in self.saves.lock().unwrap().take_all() {
            if map.get(&b.workspace_id) == Some(&b.rev) {
                Self::save(host, &b);
            }
        }
    }

    /// Il workspace e' stato rimosso: via la sua board e il file.
    pub fn remove_board<H: Host>(&self, host: &H, ws_id: &str) {
        self.run(host, |r| {
            r.remove_board(ws_id);
            r.auto_forget(ws_id);
        });
        let _ = host.workflow_write(ws_id, None);
    }
}

fn apply_effects<H: Host>(st: &AutomationState, host: &H, effects: Vec<Effect>) {
    for e in effects {
        match e {
            Effect::StartTask(job) => {
                // Git e' lento: mai sul thread corrente (puo' essere il main).
                let (st, h) = (st.clone(), host.clone());
                host.defer(Duration::ZERO, Box::new(move || boards::run_start_job(&st, &h, *job)));
            }
            Effect::Publish(board) => st.publish(host, &board),
            Effect::DeleteBoard(ws_id, rev) => {
                {
                    let mut map = st.published.lock().unwrap();
                    st.saves.lock().unwrap().forget(&ws_id);
                    map.insert(ws_id.clone(), rev);
                    host.delete_board(&ws_id);
                }
                let mut view = Board::new(&ws_id).view();
                view.rev = rev;
                host.emit_board(&view);
            }
            Effect::Update(info) => host.emit_update(&info),
            Effect::Close { tab_id, delay } if delay.is_zero() => host.emit_close(&tab_id),
            Effect::Close { tab_id, delay } => {
                let h = host.clone();
                host.defer(delay, Box::new(move || h.emit_close(&tab_id)));
            }
            Effect::Wake(tab) => {
                let _ = type_message(host, &tab, WAKE_TEXT);
            }
            Effect::Type { tab, text } => {
                let _ = type_message(host, &tab, text);
            }
            Effect::SpawnTab(ev) => host.emit_spawn(&ev),
            Effect::KeepAwake { on, seq } => {
                // Un effetto vecchio, arrivato dopo uno piu' nuovo, si scarta.
                let mut last = st.awake_applied.lock().unwrap();
                if seq > *last {
                    *last = seq;
                    host.set_keep_awake(on);
                }
            }
            Effect::Schedule(state) => host.emit_schedule(&state),
        }
    }
}

/// Digita `text` nel prompt del tab: bracketed paste, poi Invio dopo una
/// breve pausa su un altro thread (la TUI deve prima elaborare l'incolla).
fn type_message<H: Host>(host: &H, tab: &str, text: &str) -> Result<(), String> {
    host.write_to_tab(tab, &format!("\x1b[200~{}\x1b[201~", sanitize_paste(text)))?;
    let (h, tab) = (host.clone(), tab.to_string());
    host.defer(
        ENTER_DELAY,
        Box::new(move || {
            let _ = h.write_to_tab(&tab, "\r");
        }),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch RPC
// ---------------------------------------------------------------------------

const ORCHESTRATOR_METHODS: [&str; 14] = [
    "list_agents",
    "spawn_session",
    "list_sessions",
    "send_message",
    "close_session",
    "get_events",
    "submit_plan",
    "get_board",
    "get_task_details",
    "update_task",
    "set_task_status",
    "set_board",
    "get_workflow",
    "save_workflow",
];
const WORKER_METHODS: [&str; 4] = ["get_task", "report_fact", "request_help", "complete_task"];

fn session_param(p: &Value) -> Result<String, String> {
    let s = req_str(p, "session")?.trim();
    if is_valid_tab_id(s) {
        Ok(s.to_string())
    } else {
        Err("invalid session".to_string())
    }
}

/// Esegue un metodo RPC per `session`.
pub fn dispatch<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    if !(params.is_null() || params.is_object()) {
        return Err("params must be an object".to_string());
    }
    let role = st.inner.lock().unwrap().role(session);
    if ORCHESTRATOR_METHODS.contains(&method) && role != Role::Orchestrator {
        return Err(format!("{method} is reserved for the orchestrator"));
    }
    if WORKER_METHODS.contains(&method) && role != Role::Worker {
        return Err(format!("{method} is reserved for automated sessions"));
    }
    let now = now_millis();
    match method {
        "whoami" => {
            let mut out = json!({ "role": role, "session": session });
            if role == Role::Orchestrator {
                if let Some(w) = host.tab_workspace(session) {
                    out["workspace"] = json!({
                        "id": w.id, "name": w.name, "kind": w.kind, "path": w.path,
                        "worker_permission": w.worker_permission_mode().as_str(),
                        "worker_model": w.worker_model,
                    });
                }
            }
            Ok(out)
        }
        "list_agents" => Ok(Value::Array(
            host.agents()
                .into_iter()
                .map(|(id, name)| json!({ "id": id, "name": name }))
                .collect(),
        )),
        "spawn_session" => rpc_spawn(st, host, session, params, now),
        "list_sessions" => {
            let list = st.inner.lock().unwrap().list_sessions(session);
            serde_json::to_value(list).map_err(|e| e.to_string())
        }
        "send_message" => {
            let target = session_param(params)?;
            let text = req_text("text", req_str(params, "text")?, MESSAGE_MAX)?;
            st.run(host, |r| r.send_message(session, &target, now))?;
            type_message(host, &target, &text)?;
            Ok(json!({ "sent": true }))
        }
        "close_session" => {
            let target = session_param(params)?;
            st.run(host, |r| r.close_session(session, &target, now))?;
            Ok(json!({ "closed": true }))
        }
        "get_events" => {
            let events = st.run(host, |r| r.drain(session));
            Ok(json!({ "events": events }))
        }
        "submit_plan" | "get_board" | "get_task_details" | "update_task" | "set_task_status" | "set_board" => {
            boards::rpc_board(st, host, session, method, params, now)
        }
        "get_workflow" | "save_workflow" => auto::rpc_workflow(host, session, method, params),
        "get_task" => {
            let ws = st.board_workspace_of(session).and_then(|id| host.workspace(&id));
            st.run(host, |r| r.get_task_in(session, now, ws.as_ref()))
        }
        "report_fact" => {
            let text = req_str(params, "text")?;
            st.run(host, |r| r.report_fact(session, text, now))?;
            Ok(json!({ "delivered": true }))
        }
        "request_help" => {
            let question = req_str(params, "question")?;
            st.run(host, |r| r.request_help(session, question, now))?;
            Ok(json!({ "delivered": true }))
        }
        "complete_task" => {
            let status = req_str(params, "status")?;
            let summary = req_str(params, "summary")?;
            let fields = crate::board::parse_report_fields(params)?;
            // Diffstat dei task con worktree: git fuori dal lock.
            let diffstat = st.board_worktree_of(session).and_then(|(ws_id, wt)| {
                let ws = host.workspace(&ws_id)?;
                boards::compute_diffstat(host, &ws, &wt)
            });
            let closing =
                st.run(host, |r| r.complete_task_report(session, status, summary, fields, diffstat, now))?;
            Ok(json!({ "closing": closing }))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}

fn rpc_spawn<H: Host>(
    st: &AutomationState,
    host: &H,
    orch: &str,
    params: &Value,
    now: i64,
) -> Result<Value, String> {
    let p = parse_spawn_params(params)?;
    st.inner.lock().unwrap().check_spawn(orch)?;
    // Il worker si apre sempre nel workspace dell'orchestratore.
    let ws = &host
        .tab_workspace(orch)
        .ok_or_else(|| "the orchestrator's workspace was not found".to_string())?;
    if let Some(cwd) = &p.cwd {
        if !cwd_is_absolute(&ws.kind, cwd) {
            return Err("cwd must be an absolute path in the workspace's environment".to_string());
        }
        if !cwd_inside_workspace(&ws.kind, &ws.path, cwd) {
            return Err(format!(
                "cwd must be inside the workspace ({}); for git worktrees use a subfolder, e.g. .worktrees/",
                ws.path
            ));
        }
        if !host.dir_exists(ws, cwd) {
            return Err(format!("folder not found: {cwd}"));
        }
    }
    let permission_mode = resolve_permission(p.permission_mode, ws.worker_permission_mode())?;
    let req = AutomationRequest {
        bootstrap_prompt: BOOTSTRAP_PROMPT,
        permission_mode,
        model: p.model.as_deref().or(ws.worker_model.as_deref()),
    };
    let command = host.launch_command(&p.agent, &ws.kind, &req)?;
    let new = NewWorker {
        workspace_id: ws.id.clone(),
        workspace_name: ws.name.clone(),
        agent_id: p.agent,
        title: p.title,
        task: p.task,
        cwd: p.cwd,
        keep_open: p.keep_open,
        command,
    };
    let ev = st.run(host, |r| r.spawn(orch, new, now))?;
    host.emit_spawn(&ev);
    Ok(json!({ "session": ev.tab_id, "title": ev.title }))
}

// ---------------------------------------------------------------------------
// Host reale + comandi Tauri
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct TauriHost(pub AppHandle);

impl Host for TauriHost {
    fn tab_workspace(&self, tab_id: &str) -> Option<Workspace> {
        let id = crate::pty::workspace_of_tab(&self.0, tab_id)?;
        self.0.state::<WorkspaceStore>().find(&id)
    }

    fn dir_exists(&self, ws: &Workspace, path: &str) -> bool {
        if ws.kind == "wsl" {
            crate::wsl::wsl_dir_exists(ws.distro.as_deref().unwrap_or(""), path)
        } else {
            std::path::Path::new(path).is_dir()
        }
    }

    fn agents(&self) -> Vec<(String, String)> {
        agents::automation_agents()
            .into_iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect()
    }

    fn launch_command(
        &self,
        agent: &str,
        env_kind: &str,
        req: &AutomationRequest,
    ) -> Result<String, String> {
        agents::automation_command(agent, env_kind, req)
    }

    fn emit_spawn(&self, ev: &SpawnEvent) {
        let _ = self.0.emit(SPAWN_EVENT, ev);
    }

    fn emit_close(&self, tab_id: &str) {
        let _ = self.0.emit(CLOSE_EVENT, json!({ "tab_id": tab_id }));
    }

    fn emit_update(&self, info: &AutomationTabInfo) {
        let _ = self.0.emit(UPDATE_EVENT, info);
    }

    fn write_to_tab(&self, tab_id: &str, data: &str) -> Result<(), String> {
        crate::pty::write_to_tab(&self.0, tab_id, data)
    }

    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            job();
        });
    }

    fn workspace(&self, id: &str) -> Option<Workspace> {
        self.0.state::<WorkspaceStore>().find(id)
    }

    fn git(&self, ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
        boards::run_git(ws, dir, args)
    }

    fn append_exclude(&self, ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
        boards::append_exclude_line(ws, file, line)
    }

    fn save_board(&self, board: &Board) -> Result<(), String> {
        let dir = boards::boards_dir(&self.0)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella board: {e}"))?;
        let raw = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;
        let file = boards::board_file(&dir, &board.workspace_id)?;
        crate::write_atomic(&file, &raw).map_err(|e| format!("impossibile salvare la board: {e}"))
    }

    fn delete_board(&self, ws_id: &str) {
        if let Ok(dir) = boards::boards_dir(&self.0) {
            if let Ok(file) = boards::board_file(&dir, ws_id) {
                let _ = std::fs::remove_file(file);
            }
        }
    }

    fn emit_board(&self, view: &BoardView) {
        let _ = self.0.emit(BOARD_EVENT, view);
    }

    fn workspaces(&self) -> Vec<Workspace> {
        self.0.state::<WorkspaceStore>().all()
    }

    fn local_minute(&self, now: i64) -> u16 {
        use chrono::{Local, TimeZone, Timelike};
        Local
            .timestamp_millis_opt(now)
            .single()
            .map(|t| (t.hour() * 60 + t.minute()) as u16)
            .unwrap_or(0)
    }

    fn emit_schedule(&self, state: &ScheduleState) {
        auto::emit_schedule(&self.0, state);
    }

    fn save_schedule(&self, s: &Schedule) -> Result<(), String> {
        crate::schedule::save_schedule(&auto::data_dir(&self.0)?, s)
    }

    fn set_keep_awake(&self, on: bool) {
        crate::awake::set(on);
    }

    fn workflow_read(&self, ws_id: &str) -> Result<Option<String>, String> {
        crate::schedule::read_workflow(&auto::workflows_dir(&self.0)?, ws_id)
    }

    fn workflow_write(&self, ws_id: &str, content: Option<&str>) -> Result<(), String> {
        crate::schedule::write_workflow(&auto::workflows_dir(&self.0)?, ws_id, content)
    }
}

/// Punto d'ingresso delle RPC dal socket: autentica e smista.
pub fn handle_rpc(app: &AppHandle, req: &RpcRequest) -> RpcResponse {
    if let Err(e) = authenticate(req, token()) {
        return RpcResponse::failure(e);
    }
    let st = app.state::<AutomationState>();
    RpcResponse::from_result(dispatch(
        &st,
        &TauriHost(app.clone()),
        &req.session,
        &req.method,
        &req.params,
    ))
}

/// Evento di un hook con token valido (vedi `agent_notify`). Ritorna il
/// motivo di blocco se lo Stop hook di un worker va fatto proseguire.
pub fn on_hook_event(
    app: &AppHandle,
    payload: &crate::agent_notify::NotifyPayload,
) -> Option<String> {
    let tab = payload.tab_id.as_deref()?;
    let hook = HookEvent {
        event: &payload.event,
        notification_type: payload.notification_type.as_deref(),
        message: payload.message.as_deref(),
        stop_hook_active: payload.stop_hook_active,
        background_work: payload.background_work,
    };
    let st = app.state::<AutomationState>();
    st.run(&TauriHost(app.clone()), |r| r.on_hook(tab, &hook, now_millis()))
}

/// Il frontend ha chiuso un tab automatico (utente, app o shell uscita).
#[tauri::command]
pub fn automation_tab_closed(
    app: AppHandle,
    state: State<AutomationState>,
    tab_id: String,
) -> Result<(), String> {
    if !is_valid_tab_id(&tab_id) {
        return Err("id tab non valido".to_string());
    }
    state.run(&TauriHost(app), |r| r.tab_closed(&tab_id, now_millis()));
    Ok(())
}

/// Il pty di un tab e' terminato da solo (chiamato dal thread di lettura
/// solo se nessuno aveva chiamato `pty_close`: non e' una chiusura
/// dell'utente). Per l'orchestratore automatico significa crash/uscita.
pub fn pty_exited(app: &AppHandle, tab_id: &str) {
    let st = app.state::<AutomationState>();
    st.run(&TauriHost(app.clone()), |r| r.auto_tab_exited(tab_id, now_millis()));
}

/// Chiusura dell'app: salva le board con un salvataggio ancora in coda.
pub fn flush_on_exit(app: &AppHandle) {
    app.state::<AutomationState>().flush_pending_saves(&TauriHost(app.clone()));
}

/// Carica le board salvate (all'avvio dell'app). I task `running` diventano
/// `failed`; un file corrotto va in `.json.bak`.
pub fn load_boards(app: &tauri::App) {
    let Ok(dir) = boards::boards_dir(app.handle()) else {
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
            Ok(board) => st.inner.lock().unwrap().load_board(board),
            Err(_) => {
                let _ = std::fs::rename(&path, path.with_extension("json.bak"));
            }
        }
    }
}

/// Board del workspace (vuota se non esiste).
#[tauri::command]
pub fn board_get(state: State<AutomationState>, workspace_id: String) -> BoardView {
    state.inner.lock().unwrap().board_view(&workspace_id)
}

/// Imposta `max_parallel` e/o la pausa della board.
#[tauri::command]
pub async fn board_set(
    app: AppHandle,
    workspace_id: String,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        let st = app.state::<AutomationState>();
        boards::board_set_cmd(&st, &TauriHost(app.clone()), &workspace_id, max_parallel, paused)
    })
    .await
}

/// Imposta (o azzera) il modello di un task della board dalla vista Board.
#[tauri::command]
pub async fn board_task_set_model(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    model: Option<String>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        let st = app.state::<AutomationState>();
        boards::board_set_model(&st, &TauriHost(app.clone()), &workspace_id, &task_id, model.as_deref())
    })
    .await
}

/// Azione dell'utente dalla vista Board: hold, unhold, retry, cancel, done.
#[tauri::command]
pub async fn board_task_action(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    action: String,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        let st = app.state::<AutomationState>();
        boards::board_action(&st, &TauriHost(app.clone()), &workspace_id, &task_id, &action)
    })
    .await
}

/// Worker con tab aperto: il frontend ricostruisce lo stato (hot reload).
#[tauri::command]
pub fn automation_sessions(state: State<AutomationState>) -> Vec<AutomationTabInfo> {
    state.inner.lock().unwrap().open_tabs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const ORCH: &str = "orch-1";

    fn new_worker(title: &str) -> NewWorker {
        NewWorker {
            workspace_id: "ws-1".to_string(),
            workspace_name: "Proj".to_string(),
            agent_id: "claude-code".to_string(),
            title: title.to_string(),
            task: "fai cose".to_string(),
            cwd: None,
            keep_open: false,
            command: "fake-agent".to_string(),
        }
    }

    fn spawn(r: &mut Registry, title: &str) -> String {
        r.spawn(ORCH, new_worker(title), 1000).unwrap().tab_id
    }

    fn kinds(events: &[Event]) -> Vec<EventKind> {
        events.iter().map(|e| e.kind).collect()
    }

    /// Hook semplice (senza dati dello Stop hook).
    fn hook(r: &mut Registry, tab: &str, event: &str, message: Option<&str>, now: i64) -> Option<String> {
        r.on_hook(tab, &HookEvent { event, message, ..Default::default() }, now)
    }

    /// Stop hook di un worker con i dati di Claude Code.
    fn stop(r: &mut Registry, tab: &str, active: Option<bool>, background: Option<u32>, now: i64) -> Option<String> {
        let ev = HookEvent {
            event: "finished",
            stop_hook_active: active,
            background_work: background,
            ..Default::default()
        };
        r.on_hook(tab, &ev, now)
    }

    fn status_of(r: &Registry) -> WorkerStatus {
        r.list_sessions(ORCH)[0].status
    }

    #[test]
    fn token_has_expected_format_and_is_stable() {
        let t = token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(t, token());
        assert_ne!(generate_token(), generate_token());
        assert!(token_matches(Some(t)));
        assert!(!token_matches(Some("")));
        assert!(!token_matches(Some("zzz")));
        assert!(!token_matches(None));
    }

    #[test]
    fn authenticate_checks_version_token_and_session() {
        let req = |rpc, token: &str, session: &str| RpcRequest {
            rpc,
            token: token.to_string(),
            session: session.to_string(),
            method: "whoami".to_string(),
            params: Value::Null,
        };
        assert!(authenticate(&req(1, "abc", "tab-1"), "abc").is_ok());
        assert!(authenticate(&req(2, "abc", "tab-1"), "abc").is_err());
        assert!(authenticate(&req(1, "", "tab-1"), "").is_err());
        assert!(authenticate(&req(1, "x", "tab-1"), "abc").is_err());
        assert!(authenticate(&req(1, "abc", "a b"), "abc").is_err());
        assert!(authenticate(&req(1, "abc", ""), "abc").is_err());
    }

    #[test]
    fn roles_and_spawn_permissions() {
        let mut r = Registry::default();
        assert_eq!(r.role(ORCH), Role::Orchestrator);
        let w = spawn(&mut r, "uno");
        assert!(w.starts_with("auto-") && is_valid_tab_id(&w));
        assert_eq!(r.role(&w), Role::Worker);
        // Un worker non puo' avviare altri worker.
        assert!(r.spawn(&w, new_worker("figlio"), 1).is_err());
        // Id univoci anche nello stesso millisecondo.
        let w2 = spawn(&mut r, "due");
        assert_ne!(w, w2);
    }

    #[test]
    fn spawn_event_carries_command_and_parent() {
        let mut r = Registry::default();
        let mut nw = new_worker("t");
        nw.cwd = Some("C:/x".to_string());
        let ev = r.spawn(ORCH, nw, 5).unwrap();
        assert_eq!(ev.command, "fake-agent");
        assert_eq!(ev.parent_tab_id, ORCH);
        assert_eq!(ev.workspace_id, "ws-1");
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["cwd"], "C:/x");
        assert_eq!(v["agent_id"], "claude-code");
        let list = r.list_sessions(ORCH);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].status, WorkerStatus::Starting);
        assert_eq!(list[0].workspace, "Proj");
        assert!(r.take_effects().is_empty());
    }

    #[test]
    fn active_workers_are_capped_per_orchestrator() {
        let mut r = Registry::default();
        let mut ids = Vec::new();
        for i in 0..MAX_WORKERS_PER_ORCHESTRATOR {
            ids.push(spawn(&mut r, &format!("w{i}")));
        }
        assert!(r.spawn(ORCH, new_worker("extra"), 1).is_err());
        // Un altro orchestratore ha il suo tetto.
        assert!(r.spawn("orch-2", new_worker("altro"), 1).is_ok());
        // Chiudere un tab libera un posto.
        r.tab_closed(&ids[0], 2000);
        assert!(r.spawn(ORCH, new_worker("extra"), 1).is_ok());
    }

    #[test]
    fn only_the_parent_can_manage_a_worker() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        assert!(r.send_message("orch-2", &w, 1).is_err());
        assert!(r.close_session("orch-2", &w, 1).is_err());
        assert!(r.list_sessions("orch-2").is_empty());
        // Un worker non gestisce nessuno.
        assert!(r.send_message(&w, &w, 1).is_err());
        assert!(r.send_message(ORCH, "nope", 1).is_err());
        assert!(r.send_message(ORCH, &w, 1).is_ok());
        // I metodi worker sono solo dei worker.
        assert!(r.get_task(ORCH, 1).is_err());
        assert!(r.report_fact(ORCH, "x", 1).is_err());
        assert!(r.complete_task(ORCH, "completed", "x", 1).is_err());
    }

    #[test]
    fn worker_lifecycle_and_events() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        let task = r.get_task(&w, 2000).unwrap();
        assert_eq!(task["task"], "fai cose");
        assert_eq!(task["orchestrator"], ORCH);
        assert!(task["instructions"].as_str().unwrap().contains("complete_task"));
        // Seconda lettura: nessun secondo `started`.
        r.get_task(&w, 2001).unwrap();
        r.report_fact(&w, "  trovato X  ", 2002).unwrap();
        r.request_help(&w, "che faccio?", 2003).unwrap();
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::NeedsInput);
        r.report_fact(&w, "ripreso", 2004).unwrap();
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Running);
        assert!(r.complete_task(&w, "completed", "fatto", 2005).unwrap());

        let events = r.drain(ORCH);
        assert_eq!(
            kinds(&events),
            vec![
                EventKind::Started,
                EventKind::Fact,
                EventKind::HelpRequest,
                EventKind::Fact,
                EventKind::Completed
            ]
        );
        assert_eq!(events[1].text.as_deref(), Some("trovato X"));
        assert_eq!(events[4].status, WorkerStatus::Completed);
        assert!(events.windows(2).all(|p| p[0].seq < p[1].seq));
        assert!(r.drain(ORCH).is_empty());

        let s = &r.list_sessions(ORCH)[0];
        assert_eq!(s.status, WorkerStatus::Completed);
        assert_eq!(s.last_report.as_deref(), Some("fatto"));
        let effects = r.take_effects();
        assert!(effects.contains(&Effect::Close { tab_id: w.clone(), delay: CLOSE_DELAY }));
        assert!(effects.iter().any(|e| matches!(e, Effect::Update(i) if i.status == WorkerStatus::Completed)));
        // Il tab c'e' ancora
        // finche' non si chiude: tab_closed non genera `exited`.
        r.tab_closed(&w, 3000);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Completed);
        assert!(r.open_tabs().is_empty());
        // Chiuso: i tool non rispondono piu'.
        assert!(r.report_fact(&w, "tardi", 3001).is_err());
    }

    #[test]
    fn complete_keep_open_does_not_close_and_validates() {
        let mut r = Registry::default();
        let mut nw = new_worker("k");
        nw.keep_open = true;
        let w = r.spawn(ORCH, nw, 1).unwrap().tab_id;
        assert!(r.complete_task(&w, "bogus", "x", 2).is_err());
        assert!(r.complete_task(&w, "failed", "   ", 2).is_err());
        assert!(r.complete_task(&w, "failed", &"x".repeat(SUMMARY_MAX + 1), 2).is_err());
        assert!(r.report_fact(&w, "", 2).is_err());
        assert!(r.report_fact(&w, &"x".repeat(MESSAGE_MAX + 1), 2).is_err());
        assert!(r.request_help(&w, " ", 2).is_err());
        assert!(!r.complete_task(&w, "blocked", "bloccato", 3).unwrap());
        assert!(!r.take_effects().iter().any(|e| matches!(e, Effect::Close { .. })));
        // L'orchestratore scrive al worker: riprende a lavorare.
        r.send_message(ORCH, &w, 4).unwrap();
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Running);
    }

    #[test]
    fn hook_events_derive_worker_events() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.drain(ORCH);
        hook(&mut r, &w, "needs-input", Some("serve un permesso"), 2);
        // Ripetuto: nessun duplicato.
        hook(&mut r, &w, "needs-input", Some("serve un permesso"), 3);
        // Primo Stop senza report: si blocca (nessun evento); il secondo,
        // con `stop_hook_active`, e' un abbandono.
        assert_eq!(stop(&mut r, &w, None, None, 4).as_deref(), Some(STOP_BLOCK_REASON));
        assert_eq!(stop(&mut r, &w, Some(false), Some(0), 4).as_deref(), Some(STOP_BLOCK_REASON));
        assert_eq!(stop(&mut r, &w, Some(true), None, 4), None);
        let ev = r.drain(ORCH);
        assert_eq!(kinds(&ev), vec![EventKind::NeedsInput, EventKind::StoppedWithoutReport]);
        assert_eq!(ev[0].text.as_deref(), Some("serve un permesso"));
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Idle);
        // Idle: altri Stop non fanno nulla.
        assert_eq!(stop(&mut r, &w, None, None, 4), None);
        assert!(r.drain(ORCH).is_empty());
        hook(&mut r, &w, "session-end", None, 5);
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::Exited]);
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Closed);
        // Dopo la chiusura nessun altro evento, nemmeno da tab_closed.
        hook(&mut r, &w, "finished", None, 6);
        r.tab_closed(&w, 7);
        assert!(r.drain(ORCH).is_empty());
    }

    #[test]
    fn hooks_after_completion_and_help_are_quiet() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.request_help(&w, "aiuto", 2).unwrap();
        r.drain(ORCH);
        // Lo Stop che segue request_help non e' un abbandono.
        hook(&mut r, &w, "finished", None, 3);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::NeedsInput);
        r.report_fact(&w, "ok", 4).unwrap();
        r.complete_task(&w, "completed", "fine", 5).unwrap();
        r.drain(ORCH);
        hook(&mut r, &w, "finished", None, 6);
        hook(&mut r, &w, "needs-input", None, 7);
        hook(&mut r, &w, "session-end", None, 8);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Completed);
    }

    #[test]
    fn stop_before_get_task_is_nudged_then_reported() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        assert!(stop(&mut r, &w, None, None, 1).is_some());
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(status_of(&r), WorkerStatus::Starting);
        assert!(stop(&mut r, &w, Some(true), None, 2).is_none());
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::StoppedWithoutReport]);
    }

    #[test]
    fn stop_with_background_work_moves_to_background_quietly() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.drain(ORCH);
        r.take_effects();
        assert_eq!(stop(&mut r, &w, None, Some(2), 2), None);
        assert_eq!(status_of(&r), WorkerStatus::Background);
        assert!(r.drain(ORCH).is_empty());
        // Update emesso per la UI, con lo stato serializzato "background".
        let fx = r.take_effects();
        let info = fx.iter().find_map(|e| match e {
            Effect::Update(i) => Some(i.clone()),
            _ => None,
        });
        let info = info.unwrap();
        assert_eq!(info.status, WorkerStatus::Background);
        assert_eq!(serde_json::to_value(&info).unwrap()["status"], "background");
        // Un secondo Stop ancora in background: nessun nuovo update.
        stop(&mut r, &w, Some(true), Some(1), 3);
        assert!(r.take_effects().is_empty());
        // Finito il background senza report: sollecito, poi abbandono.
        assert!(stop(&mut r, &w, None, Some(0), 4).is_some());
        assert_eq!(status_of(&r), WorkerStatus::Background);
        assert!(stop(&mut r, &w, Some(true), Some(0), 5).is_none());
        assert_eq!(status_of(&r), WorkerStatus::Idle);
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::StoppedWithoutReport]);
    }

    #[test]
    fn background_resumes_on_activity_message_and_close() {
        // Attivita' del worker: torna running.
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        stop(&mut r, &w, None, Some(1), 2);
        r.report_fact(&w, "ancora vivo", 3).unwrap();
        assert_eq!(status_of(&r), WorkerStatus::Running);
        // Messaggio dell'orchestratore: torna running.
        stop(&mut r, &w, None, Some(1), 4);
        assert_eq!(status_of(&r), WorkerStatus::Background);
        r.send_message(ORCH, &w, 5).unwrap();
        assert_eq!(status_of(&r), WorkerStatus::Running);
        // Permesso richiesto mentre e' in background.
        stop(&mut r, &w, None, Some(1), 6);
        hook(&mut r, &w, "needs-input", Some("permesso"), 7);
        assert_eq!(status_of(&r), WorkerStatus::NeedsInput);
        // complete_task da background.
        stop(&mut r, &w, None, Some(1), 8);
        r.complete_task(&w, "completed", "fatto", 9).unwrap();
        assert_eq!(status_of(&r), WorkerStatus::Completed);
        // Chiusura da background: closed senza evento exited di rimbalzo.
        let w2 = spawn(&mut r, "due");
        r.get_task(&w2, 10).unwrap();
        r.drain(ORCH);
        stop(&mut r, &w2, None, Some(1), 11);
        r.close_session(ORCH, &w2, 12).unwrap();
        r.tab_closed(&w2, 13);
        assert!(r.drain(ORCH).is_empty());
        let st2 = r.list_sessions(ORCH).into_iter().find(|x| x.session == w2).unwrap().status;
        assert_eq!(st2, WorkerStatus::Closed);
        // Il tab si chiude con il worker in background: exited.
        let w3 = spawn(&mut r, "tre");
        r.get_task(&w3, 14).unwrap();
        r.drain(ORCH);
        stop(&mut r, &w3, None, Some(1), 15);
        r.tab_closed(&w3, 16);
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::Exited]);
    }

    #[test]
    fn idle_prompt_notification_is_ignored_for_workers() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.drain(ORCH);
        let idle = HookEvent {
            event: "needs-input",
            notification_type: Some("idle_prompt"),
            message: Some("Claude is waiting for your input"),
            ..Default::default()
        };
        assert_eq!(r.on_hook(&w, &idle, 2), None);
        assert_eq!(status_of(&r), WorkerStatus::Running);
        assert!(r.drain(ORCH).is_empty());
        // permission_prompt / elicitation_dialog: come prima.
        for (i, nt) in ["permission_prompt", "elicitation_dialog"].into_iter().enumerate() {
            let mut r = Registry::default();
            let w = spawn(&mut r, "x");
            r.get_task(&w, 1).unwrap();
            r.drain(ORCH);
            let ev = HookEvent { event: "needs-input", notification_type: Some(nt), ..Default::default() };
            r.on_hook(&w, &ev, 2 + i as i64);
            assert_eq!(status_of(&r), WorkerStatus::NeedsInput, "{nt}");
            assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::NeedsInput]);
        }
    }

    #[test]
    fn model_facing_texts_are_english_and_prompt_is_shell_safe() {
        assert!(BOOTSTRAP_PROMPT.is_ascii());
        let unsafe_char = |c: char| matches!(c, '"' | '$' | '`' | '%' | '!' | '\\') || c.is_control();
        assert!(!BOOTSTRAP_PROMPT.chars().any(unsafe_char));
        assert!(BOOTSTRAP_PROMPT.contains("get_task"));
        assert!(WAKE_TEXT.is_ascii() && WAKE_TEXT.contains("get_events"));
        assert!(WORKER_INSTRUCTIONS.contains("complete_task"));
        assert!(WORKER_INSTRUCTIONS.contains("Background commands are fine"));
    }

    #[test]
    fn nudge_reason_is_english_and_stop_with_help_is_quiet() {
        assert!(STOP_BLOCK_REASON.contains("complete_task") && STOP_BLOCK_REASON.contains("request_help"));
        assert!(STOP_BLOCK_REASON.contains("background"));
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.request_help(&w, "aiuto", 2).unwrap();
        // Con una richiesta di aiuto pendente non si sollecita ne' si riferisce.
        assert_eq!(stop(&mut r, &w, None, None, 3), None);
        assert_eq!(stop(&mut r, &w, Some(true), None, 4), None);
        assert_eq!(status_of(&r), WorkerStatus::NeedsInput);
        // Un orchestratore (non worker) non riceve mai un blocco.
        assert_eq!(stop(&mut r, ORCH, None, None, 5), None);
    }

    #[test]
    fn tab_closed_reports_exited_once() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.drain(ORCH);
        r.tab_closed(&w, 2);
        r.tab_closed(&w, 3);
        let ev = r.drain(ORCH);
        assert_eq!(kinds(&ev), vec![EventKind::Exited]);
        assert_eq!(ev[0].status, WorkerStatus::Closed);
        r.tab_closed("sconosciuto", 4); // nessun effetto
    }

    #[test]
    fn close_session_does_not_echo_exited() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        r.drain(ORCH);
        r.take_effects();
        r.close_session(ORCH, &w, 2).unwrap();
        assert!(r.take_effects().contains(&Effect::Close { tab_id: w.clone(), delay: Duration::ZERO }));
        r.tab_closed(&w, 3);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Closed);
        assert!(r.close_session(ORCH, &w, 4).is_err());
        assert!(r.send_message(ORCH, &w, 4).is_err());
    }

    fn wakes(r: &mut Registry) -> usize {
        r.take_effects()
            .iter()
            .filter(|e| matches!(e, Effect::Wake(t) if t == ORCH))
            .count()
    }

    #[test]
    fn wake_only_when_orchestrator_is_idle_at_prompt() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap(); // nessun hook di ORCH: niente risveglio
        assert_eq!(wakes(&mut r), 0);
        hook(&mut r, ORCH, "finished", None, 2);
        // `started` e `fact` si accodano ma non svegliano.
        r.report_fact(&w, "f", 2).unwrap();
        assert_eq!(wakes(&mut r), 0);
        // Evento azionabile + O idle: un solo risveglio, non ripetuto.
        r.request_help(&w, "a", 3).unwrap();
        assert_eq!(wakes(&mut r), 1);
        r.request_help(&w, "b", 4).unwrap();
        assert_eq!(wakes(&mut r), 0);
        // get_events: started + fact + 2 help_request, anche i non azionabili.
        assert_eq!(r.drain(ORCH).len(), 4);
        // Sta elaborando gli eventi: niente risveglio finche' non torna idle.
        r.request_help(&w, "c", 5).unwrap();
        assert_eq!(wakes(&mut r), 0);
        hook(&mut r, ORCH, "finished", None, 6);
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn idle_prompt_counts_as_idle_for_the_orchestrator() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        hook(&mut r, ORCH, "finished", None, 2);
        // ~60 s dopo la fine del turno Claude Code manda `idle_prompt`: O e'
        // ancora al prompt e un evento azionabile deve svegliarlo.
        let idle = HookEvent {
            event: "needs-input",
            notification_type: Some("idle_prompt"),
            ..Default::default()
        };
        r.on_hook(ORCH, &idle, 3);
        r.request_help(&w, "a", 4).unwrap();
        assert_eq!(wakes(&mut r), 1);
        // Risveglio ignorato: un idle_prompt successivo non lo ripete.
        r.on_hook(ORCH, &idle, 5);
        assert_eq!(wakes(&mut r), 0);
        // Un evento azionabile in coda mentre O lavorava: idle_prompt lo sveglia.
        r.drain(ORCH);
        hook(&mut r, ORCH, "prompt", None, 6);
        r.complete_task(&w, "completed", "fatto", 7).unwrap();
        assert_eq!(wakes(&mut r), 0);
        r.on_hook(ORCH, &idle, 8);
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn non_actionable_events_never_wake_nor_block_a_later_wake() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        hook(&mut r, ORCH, "finished", None, 1);
        r.get_task(&w, 2).unwrap();
        r.report_fact(&w, "a", 3).unwrap();
        // Anche un nuovo `finished` di O con soli started/fact in coda: nulla.
        hook(&mut r, ORCH, "finished", None, 4);
        assert_eq!(wakes(&mut r), 0);
        // Un evento azionabile sveglia, e i fact accodati dopo non ne
        // provocano un altro.
        r.complete_task(&w, "completed", "fatto", 5).unwrap();
        assert_eq!(wakes(&mut r), 1);
        hook(&mut r, ORCH, "finished", None, 6);
        assert_eq!(wakes(&mut r), 0);
        // Ogni tipo azionabile sveglia.
        for (i, (kind, expect)) in [
            (EventKind::Started, false),
            (EventKind::Fact, false),
            (EventKind::HelpRequest, true),
            (EventKind::Completed, true),
            (EventKind::NeedsInput, true),
            (EventKind::StoppedWithoutReport, true),
            (EventKind::Exited, true),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(kind.is_actionable(), expect, "{kind:?} ({i})");
        }
    }

    #[test]
    fn stopped_without_report_and_exited_wake_the_orchestrator() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        hook(&mut r, ORCH, "finished", None, 2);
        assert_eq!(wakes(&mut r), 0);
        stop(&mut r, &w, None, None, 3); // sollecito: nessun evento
        assert_eq!(wakes(&mut r), 0);
        stop(&mut r, &w, Some(true), None, 4);
        assert_eq!(wakes(&mut r), 1);
        // Stop con lavoro in background: nessun evento, nessun risveglio.
        let w2 = spawn(&mut r, "due");
        r.get_task(&w2, 5).unwrap();
        r.drain(ORCH);
        hook(&mut r, ORCH, "finished", None, 6);
        stop(&mut r, &w2, None, Some(1), 7);
        assert_eq!(wakes(&mut r), 0);
        r.tab_closed(&w2, 8);
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn ignored_wake_repeats_only_for_newer_events() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        hook(&mut r, ORCH, "finished", None, 1);
        r.get_task(&w, 2).unwrap();
        r.request_help(&w, "a", 3).unwrap();
        assert_eq!(wakes(&mut r), 1);
        // O chiude il turno senza chiamare get_events: nessun evento
        // nuovo, nessun nuovo risveglio (niente spam a ogni turno).
        hook(&mut r, ORCH, "finished", None, 4);
        assert_eq!(wakes(&mut r), 0);
        // Un fact nuovo non conta; un evento azionabile nuovo con O idle si'.
        r.report_fact(&w, "x", 5).unwrap();
        hook(&mut r, ORCH, "finished", None, 6);
        assert_eq!(wakes(&mut r), 0);
        r.request_help(&w, "b", 7).unwrap();
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn no_wake_while_orchestrator_waits_on_permission() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        hook(&mut r, ORCH, "needs-input", Some("permesso"), 1);
        r.get_task(&w, 2).unwrap();
        r.request_help(&w, "a", 2).unwrap();
        assert_eq!(wakes(&mut r), 0);
        // Risolto il permesso, O finisce il turno: ora si sveglia.
        hook(&mut r, ORCH, "finished", None, 3);
        assert_eq!(wakes(&mut r), 1);
        // Senza eventi in coda, un finished non sveglia nessuno.
        r.drain(ORCH);
        hook(&mut r, ORCH, "finished", None, 4);
        assert_eq!(wakes(&mut r), 0);
    }

    #[test]
    fn queue_is_capped_and_seq_increases() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.get_task(&w, 1).unwrap();
        for i in 0..(MAX_QUEUE_EVENTS + 50) {
            r.report_fact(&w, &format!("f{i}"), 2).unwrap();
        }
        let ev = r.drain(ORCH);
        assert_eq!(ev.len(), MAX_QUEUE_EVENTS);
        // Scartati i piu' vecchi: resta il fondo, sempre ordinato per seq.
        assert_eq!(ev.last().unwrap().text.as_deref(), Some(&*format!("f{}", MAX_QUEUE_EVENTS + 49)));
        assert!(ev.windows(2).all(|p| p[0].seq < p[1].seq));
        assert!(ev[0].text.as_deref() != Some("f0"));
    }

    #[test]
    fn closed_workers_are_pruned_beyond_the_cap() {
        let mut r = Registry::default();
        for i in 0..(MAX_TRACKED_WORKERS + 10) {
            let id = r.spawn(ORCH, new_worker(&format!("w{i}")), i as i64).unwrap().tab_id;
            r.tab_closed(&id, 1);
        }
        assert!(r.workers.len() <= MAX_TRACKED_WORKERS + 1);
    }

    #[test]
    fn spawn_params_validation() {
        let ok = parse_spawn_params(&json!({"task":"  fai   una cosa  molto lunga con tante parole davvero tante "})).unwrap();
        assert_eq!(ok.agent, "claude-code");
        assert_eq!(ok.permission_mode, None);
        assert!(!ok.keep_open && ok.cwd.is_none() && ok.model.is_none());
        assert_eq!(ok.title, "fai una cosa molto lunga con tante parole");

        let full = parse_spawn_params(&json!({
            "task":"t","title":" Titolo ","agent":"claude-code",
            "cwd":"C:/p","permission_mode":"bypass","model":"claude-opus-4.1_x","keep_open":true
        }))
        .unwrap();
        assert_eq!(full.title, "Titolo");
        assert_eq!(full.permission_mode, Some(PermissionMode::Bypass));
        assert_eq!(full.model.as_deref(), Some("claude-opus-4.1_x"));
        assert!(full.keep_open);

        for bad in [
            json!({}),
            json!({"task":"  "}),
            json!({"task":"x".repeat(TASK_MAX + 1)}),
            json!({"task":"t","title":"x".repeat(81)}),
            json!({"task":"t","permission_mode":"yolo"}),
            json!({"task":"t","model":"a b"}),
            json!({"task":"t","model":"a;b"}),
            json!({"task":"t","model":"x".repeat(65)}),
            json!({"task":"t","keep_open":"si"}),
            json!({"task":"t","cwd":"a\nb"}),
            json!({"workspace":"w","task":"t"}),
            json!({"workspace":5,"task":"t"}),
        ] {
            assert!(parse_spawn_params(&bad).is_err(), "{bad}");
        }
        assert!(parse_spawn_params(&json!({"task":"x".repeat(TASK_MAX)})).is_ok());
        // Null equivale ad assente.
        assert!(parse_spawn_params(&json!({"task":"t","model":null,"cwd":null})).is_ok());
    }

    fn ws(id: &str, name: &str, kind: &str) -> Workspace {
        Workspace {
            id: id.to_string(),
            name: name.to_string(),
            kind: kind.to_string(),
            path: if kind == "wsl" { "/home/u".to_string() } else { "C:\\p".to_string() },
            distro: (kind == "wsl").then(|| "Ubuntu".to_string()),
            shell: None,
            created_at: 0,
            launches: Vec::new(),
            worker_permission: "accept-edits".to_string(),
            worktree_setup: None,
            worker_model: None,
            auto_orchestrate: None,
            default_launch: None,
        }
    }

    #[test]
    fn resolve_permission_covers_every_combination() {
        use PermissionMode::*;
        let all = [Plan, Default, AcceptEdits, Bypass];
        for max in all {
            // Assente: vale il tetto.
            assert_eq!(resolve_permission(None, max), Ok(max));
            for req in all {
                let r = resolve_permission(Some(req), max);
                if req.rank() <= max.rank() {
                    assert_eq!(r, Ok(req), "{req:?} <= {max:?}");
                } else {
                    let e = r.unwrap_err();
                    assert!(e.contains(&format!("'{}' is not allowed", req.as_str())), "{e}");
                    assert!(e.contains(&format!("maximum for this workspace is '{}'", max.as_str())), "{e}");
                }
            }
        }
        assert!(resolve_permission(Some(Bypass), AcceptEdits).is_err());
        assert_eq!(resolve_permission(Some(Plan), Bypass), Ok(Plan));
    }

    #[test]
    fn cwd_inside_workspace_rules() {
        // Windows: maiuscole e separatori indifferenti, slash finali ignorati.
        assert!(cwd_inside_workspace("windows", "C:\\p", "C:\\p"));
        assert!(cwd_inside_workspace("windows", "C:\\p\\", "c:/P/"));
        assert!(cwd_inside_workspace("windows", "C:\\p", "C:/p/.worktrees/x"));
        assert!(!cwd_inside_workspace("windows", "C:\\p", "C:\\p2"));
        assert!(!cwd_inside_workspace("windows", "C:\\p", "C:\\"));
        assert!(!cwd_inside_workspace("windows", "C:\\p", "D:\\p"));
        assert!(!cwd_inside_workspace("windows", "C:\\p", "C:\\p\\..\\q"));
        assert!(!cwd_inside_workspace("windows", "C:\\p", "C:\\p\\a\\..\\.."));
        // WSL: confronto esatto su segmenti.
        assert!(cwd_inside_workspace("wsl", "/home/u/app", "/home/u/app"));
        assert!(cwd_inside_workspace("wsl", "/home/u/app/", "/home/u/app/.worktrees/x/"));
        assert!(!cwd_inside_workspace("wsl", "/home/u/app", "/home/u/app2"));
        assert!(!cwd_inside_workspace("wsl", "/home/u/app", "/home/u"));
        assert!(!cwd_inside_workspace("wsl", "/home/u/app", "/home/u/APP"));
        assert!(!cwd_inside_workspace("wsl", "/home/u/app", "/home/u/app/../x"));
    }

    #[test]
    fn cwd_and_paste_helpers() {
        assert!(cwd_is_absolute("windows", "C:\\a"));
        assert!(cwd_is_absolute("windows", "d:/a"));
        assert!(cwd_is_absolute("windows", "\\\\srv\\share"));
        assert!(!cwd_is_absolute("windows", "rel/dir"));
        assert!(!cwd_is_absolute("windows", "C:"));
        assert!(cwd_is_absolute("wsl", "/home/u"));
        assert!(!cwd_is_absolute("wsl", "home/u"));
        assert!(!cwd_is_absolute("wsl", "C:\\a"));
        assert_eq!(sanitize_paste("a\x1b[201~b\r\nc\td\x07"), "a[201~b\nc\td");
    }

    // --- strato con Host finto ---

    #[derive(Clone, Default)]
    struct FakeHost {
        log: Arc<Mutex<Vec<String>>>,
        /// "<dir>|<args>" di ogni git eseguito.
        git_log: Arc<Mutex<Vec<String>>>,
        /// Risposte git forzate: la prima il cui testo e' contenuto in "<dir>|<args>".
        git_rules: Arc<Mutex<Vec<(String, GitOut)>>>,
        /// Cartelle che "esistono" oltre a quelle con "esiste" nel nome.
        dirs: Arc<Mutex<Vec<String>>>,
        spawns: Arc<Mutex<Vec<SpawnEvent>>>,
        /// Board salvate (ultima per workspace) e viste emesse.
        saved: Arc<Mutex<HashMap<String, Board>>>,
        board_events: Arc<Mutex<Vec<BoardView>>>,
        deleted: Arc<Mutex<Vec<String>>>,
        excludes: Arc<Mutex<Vec<String>>>,
        setup: Arc<Mutex<Option<String>>>,
        /// Minuti locali "correnti" dell'host finto.
        minute: Arc<Mutex<u16>>,
        /// Configurazione dell'orchestratore automatico per workspace.
        auto: Arc<Mutex<HashMap<String, crate::workspaces::AutoOrchestrate>>>,
        /// Tab (oltre agli orchestratori fissi) con un pty "vivo".
        live: Arc<Mutex<Vec<String>>>,
        schedule_events: Arc<Mutex<Vec<ScheduleState>>>,
        saved_schedule: Arc<Mutex<Option<Schedule>>>,
        awake: Arc<Mutex<Vec<bool>>>,
        workflows: Arc<Mutex<HashMap<String, String>>>,
        /// Modello di default dei worker del workspace "ws-1".
        worker_model: Arc<Mutex<Option<String>>>,
        /// Errore forzato di `append_exclude`.
        exclude_err: Arc<Mutex<Option<String>>>,
        /// `true` = i `git worktree add` restano bloccati (prova della serializzazione).
        add_hold: Arc<(Mutex<bool>, std::sync::Condvar)>,
        add_in_flight: Arc<std::sync::atomic::AtomicUsize>,
        add_max_in_flight: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FakeHost {
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
        fn git_log(&self) -> Vec<String> {
            self.git_log.lock().unwrap().clone()
        }
        fn git_rule(&self, needle: &str, ok: bool, stdout: &str, stderr: &str) {
            let out = GitOut { ok, stdout: stdout.to_string(), stderr: stderr.to_string() };
            self.git_rules.lock().unwrap().push((needle.to_string(), out));
        }
        fn spawns(&self) -> Vec<SpawnEvent> {
            self.spawns.lock().unwrap().clone()
        }
        fn ws_for(&self, id: &str) -> Option<Workspace> {
            match id {
                "ws-1" => {
                    let mut w = ws("ws-1", "Proj", "windows");
                    w.worktree_setup = self.setup.lock().unwrap().clone();
                    w.worker_model = self.worker_model.lock().unwrap().clone();
                    w.auto_orchestrate = self.auto.lock().unwrap().get(id).cloned();
                    Some(w)
                }
                "ws-2" => Some(ws("ws-2", "Linux", "wsl")),
                _ => None,
            }
        }
    }

    impl Host for FakeHost {
        fn tab_workspace(&self, tab_id: &str) -> Option<Workspace> {
            match tab_id {
                ORCH | "orch-b" => self.ws_for("ws-1"),
                "orch-wsl" => self.ws_for("ws-2"),
                t if self.live.lock().unwrap().iter().any(|l| l == t) => self.ws_for("ws-1"),
                _ => None,
            }
        }
        fn dir_exists(&self, _ws: &Workspace, path: &str) -> bool {
            path.contains("esiste") || self.dirs.lock().unwrap().iter().any(|d| d == path)
        }
        fn agents(&self) -> Vec<(String, String)> {
            vec![("claude-code".to_string(), "Claude Code".to_string())]
        }
        fn launch_command(&self, agent: &str, env_kind: &str, req: &AutomationRequest) -> Result<String, String> {
            if agent != "claude-code" {
                return Err(format!("unknown agent: {agent}"));
            }
            Ok(format!("fake {env_kind} {:?} {} model={}", req.permission_mode, req.bootstrap_prompt, req.model.unwrap_or("-")))
        }
        fn emit_spawn(&self, ev: &SpawnEvent) {
            self.spawns.lock().unwrap().push(ev.clone());
            self.log.lock().unwrap().push(format!("spawn:{}:{}", ev.tab_id, ev.command));
        }
        fn emit_close(&self, tab_id: &str) {
            self.log.lock().unwrap().push(format!("close:{tab_id}"));
        }
        fn emit_update(&self, info: &AutomationTabInfo) {
            self.log.lock().unwrap().push(format!("update:{}:{:?}", info.tab_id, info.status));
        }
        fn write_to_tab(&self, tab_id: &str, data: &str) -> Result<(), String> {
            self.log.lock().unwrap().push(format!("write:{tab_id}:{}", data.escape_debug()));
            Ok(())
        }
        fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
            self.log.lock().unwrap().push(format!("defer:{}", delay.as_millis()));
            job();
        }
        fn workspace(&self, id: &str) -> Option<Workspace> {
            self.ws_for(id)
        }
        fn git(&self, _ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
            let line = format!("{dir}|{}", args.join(" "));
            self.git_log.lock().unwrap().push(line.clone());
            if args.starts_with(&["worktree", "add"]) {
                use std::sync::atomic::Ordering::SeqCst;
                let n = self.add_in_flight.fetch_add(1, SeqCst) + 1;
                self.add_max_in_flight.fetch_max(n, SeqCst);
                let (m, c) = &*self.add_hold;
                let mut hold = m.lock().unwrap();
                while *hold {
                    hold = c.wait(hold).unwrap();
                }
                drop(hold);
                self.add_in_flight.fetch_sub(1, SeqCst);
            }
            if let Some((_, out)) = self.git_rules.lock().unwrap().iter().find(|(n, _)| line.contains(n.as_str())) {
                return Ok(out.clone());
            }
            let ok = |s: &str| GitOut { ok: true, stdout: s.to_string(), stderr: String::new() };
            let a = args.join(" ");
            Ok(match a.as_str() {
                "rev-parse --is-inside-work-tree" => ok("true\n"),
                "rev-parse --abbrev-ref HEAD" => ok("main\n"),
                "rev-parse --git-path info/exclude" => ok(".git/info/exclude\n"),
                s if s.starts_with("rev-parse --verify") => GitOut { ok: false, stdout: String::new(), stderr: String::new() },
                s if s.starts_with("diff --stat") => ok(" 2 files changed, 10 insertions(+)\n"),
                _ => ok(""),
            })
        }
        fn append_exclude(&self, _ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
            if let Some(e) = self.exclude_err.lock().unwrap().clone() {
                return Err(e);
            }
            self.excludes.lock().unwrap().push(format!("{file}|{line}"));
            Ok(())
        }
        fn save_board(&self, board: &Board) -> Result<(), String> {
            self.saved.lock().unwrap().insert(board.workspace_id.clone(), board.clone());
            Ok(())
        }
        fn delete_board(&self, ws_id: &str) {
            self.deleted.lock().unwrap().push(ws_id.to_string());
            self.saved.lock().unwrap().remove(ws_id);
        }
        fn emit_board(&self, view: &BoardView) {
            self.board_events.lock().unwrap().push(view.clone());
        }
        fn workspaces(&self) -> Vec<Workspace> {
            ["ws-1", "ws-2"].iter().filter_map(|id| self.ws_for(id)).collect()
        }
        fn local_minute(&self, _now: i64) -> u16 {
            *self.minute.lock().unwrap()
        }
        fn emit_schedule(&self, state: &ScheduleState) {
            self.schedule_events.lock().unwrap().push(state.clone());
        }
        fn save_schedule(&self, s: &Schedule) -> Result<(), String> {
            *self.saved_schedule.lock().unwrap() = Some(s.clone());
            Ok(())
        }
        fn set_keep_awake(&self, on: bool) {
            self.awake.lock().unwrap().push(on);
        }
        fn workflow_read(&self, ws_id: &str) -> Result<Option<String>, String> {
            Ok(self.workflows.lock().unwrap().get(ws_id).cloned())
        }
        fn workflow_write(&self, ws_id: &str, content: Option<&str>) -> Result<(), String> {
            let mut m = self.workflows.lock().unwrap();
            match content {
                Some(c) => m.insert(ws_id.to_string(), c.to_string()),
                None => m.remove(ws_id),
            };
            Ok(())
        }
    }

    fn call(st: &AutomationState, h: &FakeHost, session: &str, method: &str, params: Value) -> Result<Value, String> {
        dispatch(st, h, session, method, &params)
    }

    mod auto_flow;
    mod board_flow;

    #[test]
    fn spawn_session_model_falls_back_to_workspace_worker_model() {
        let st = AutomationState::default();
        let h = FakeHost::default();
        // Nessun modello ovunque: default dell'agente.
        call(&st, &h, ORCH, "spawn_session", json!({"task":"t"})).unwrap();
        assert!(h.spawns()[0].command.contains("model=-"), "{}", h.spawns()[0].command);
        // Default del workspace.
        *h.worker_model.lock().unwrap() = Some("sonnet".to_string());
        let me = call(&st, &h, ORCH, "whoami", Value::Null).unwrap();
        assert_eq!(me["workspace"]["worker_model"], "sonnet");
        call(&st, &h, ORCH, "spawn_session", json!({"task":"t"})).unwrap();
        assert!(h.spawns()[1].command.contains("model=sonnet"), "{}", h.spawns()[1].command);
        // Il modello esplicito vince.
        call(&st, &h, ORCH, "spawn_session", json!({"task":"t","model":"opus"})).unwrap();
        assert!(h.spawns()[2].command.contains("model=opus"), "{}", h.spawns()[2].command);
    }

    #[test]
    fn dispatch_full_flow_with_fake_host() {
        let st = AutomationState::default();
        let h = FakeHost::default();

        assert_eq!(call(&st, &h, ORCH, "whoami", Value::Null).unwrap()["role"], "orchestrator");
        let me = call(&st, &h, ORCH, "whoami", Value::Null).unwrap();
        assert_eq!(
            me["workspace"],
            json!({"id":"ws-1","name":"Proj","kind":"windows","path":"C:\\p","worker_permission":"accept-edits","worker_model":null})
        );
        // Orchestratore senza workspace noto: whoami funziona, senza il campo.
        assert!(call(&st, &h, "orch-2", "whoami", Value::Null).unwrap().get("workspace").is_none());
        assert!(call(&st, &h, ORCH, "list_workspaces", json!({})).is_err());
        assert_eq!(call(&st, &h, ORCH, "list_agents", json!({})).unwrap()[0]["id"], "claude-code");

        // Validazioni dello spawn.
        let err = call(&st, &h, ORCH, "spawn_session", json!({"workspace":"Proj","task":"t"})).unwrap_err();
        assert!(err.contains("workspace is not allowed"), "{err}");
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","agent":"altro"})).is_err());
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":"rel"})).is_err());
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":"C:\\p\\manca"})).is_err());
        // Fuori dal workspace: anche se la cartella esiste.
        for cwd in ["C:\\esiste", "C:\\p2\\esiste", "C:\\p\\..\\esiste"] {
            let err = call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":cwd})).unwrap_err();
            assert!(err.contains("inside the workspace"), "{cwd}: {err}");
        }
        let err = call(&st, &h, "orch-wsl", "spawn_session", json!({"task":"t","cwd":"/home/u2/esiste"})).unwrap_err();
        assert!(err.contains("inside the workspace"), "{err}");
        // Orchestratore senza workspace noto.
        let err = call(&st, &h, "orch-2", "spawn_session", json!({"task":"t"})).unwrap_err();
        assert_eq!(err, "the orchestrator's workspace was not found");
        // Piu' permissivo del tetto del workspace (accept-edits): rifiutato.
        let err = call(&st, &h, ORCH, "spawn_session", json!({"task":"t","permission_mode":"bypass"})).unwrap_err();
        assert!(err.contains("is not allowed"), "{err}");
        assert!(h.log().is_empty());

        // Senza permission_mode: il tetto del workspace.
        {
            let (st, h) = (AutomationState::default(), FakeHost::default());
            call(&st, &h, "orch-wsl", "spawn_session", json!({"task":"l"})).unwrap();
            assert!(h.log()[0].contains("fake wsl AcceptEdits"), "{:?}", h.log());
        }

        // Un orchestratore WSL: il worker eredita il suo workspace (ambiente wsl).
        {
            let (st, h) = (AutomationState::default(), FakeHost::default());
            let r = call(&st, &h, "orch-wsl", "spawn_session", json!({"task":"l","cwd":"/home/u/.worktrees/esiste"})).unwrap();
            assert!(h.log()[0].starts_with(&format!("spawn:{}:fake wsl", r["session"].as_str().unwrap())));
            assert_eq!(st.inner.lock().unwrap().list_sessions("orch-wsl")[0].workspace, "Linux");
        }

        // Il worker eredita il workspace dell'orchestratore (windows), cwd con maiuscole e slash diversi.
        let r = call(
            &st,
            &h,
            ORCH,
            "spawn_session",
            json!({"task":"fai X","cwd":"c:/P/esiste","permission_mode":"plan"}),
        )
        .unwrap();
        let w = r["session"].as_str().unwrap().to_string();
        assert_eq!(r["title"], "fai X");
        assert!(h.log()[0].starts_with(&format!("spawn:{w}:fake windows Plan You are an automated")));

        // Il worker e' un worker: whoami lo dice e i metodi orchestratore falliscono.
        assert_eq!(call(&st, &h, &w, "whoami", json!({})).unwrap()["role"], "worker");
        assert!(call(&st, &h, &w, "spawn_session", json!({"task":"t"})).is_err());
        assert!(call(&st, &h, &w, "get_events", json!({})).is_err());
        assert!(call(&st, &h, ORCH, "get_task", json!({})).is_err());

        let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
        assert_eq!(t["task"], "fai X");
        call(&st, &h, &w, "report_fact", json!({"text":"ciao"})).unwrap();
        assert!(call(&st, &h, &w, "report_fact", json!({})).is_err());
        let ls = call(&st, &h, ORCH, "list_sessions", json!({})).unwrap();
        assert_eq!(ls[0]["status"], "running");
        assert_eq!(ls[0]["last_report"], "ciao");

        // send_message: bracketed paste, poi Invio dopo 300 ms.
        let before = h.log().len();
        call(&st, &h, ORCH, "send_message", json!({"session": w, "text": "fai Y\x1b"})).unwrap();
        let log = h.log();
        assert!(log[before..].contains(&format!("write:{w}:\\u{{1b}}[200~fai Y\\u{{1b}}[201~")));
        assert!(log[before..].contains(&"defer:300".to_string()));
        assert_eq!(log.last().unwrap(), &format!("write:{w}:\\r"));
        assert!(call(&st, &h, ORCH, "send_message", json!({"session": "altro", "text": "x"})).is_err());
        assert!(call(&st, &h, "orch-2", "send_message", json!({"session": w, "text": "x"})).is_err());

        // get_events: consegna la coda e la svuota, senza mai attendere.
        let ev = call(&st, &h, ORCH, "get_events", json!({})).unwrap();
        let kinds: Vec<_> = ev["events"].as_array().unwrap().iter().map(|e| e["kind"].clone()).collect();
        assert_eq!(kinds, vec![json!("started"), json!("fact")]);
        assert_eq!(call(&st, &h, ORCH, "get_events", Value::Null).unwrap(), json!({"events": []}));

        // complete_task con chiusura ritardata di 1500 ms.
        let c = call(&st, &h, &w, "complete_task", json!({"status":"completed","summary":"fatto"})).unwrap();
        assert_eq!(c["closing"], true);
        let log = h.log();
        assert!(log.contains(&"defer:1500".to_string()));
        assert_eq!(log.last().unwrap(), &format!("close:{w}"));

        // close_session su un worker gia' concluso: tab ancora aperto -> emit immediato.
        call(&st, &h, ORCH, "close_session", json!({"session": w})).unwrap();
        assert!(call(&st, &h, ORCH, "bogus", json!({})).is_err());
        assert!(call(&st, &h, ORCH, "whoami", json!([1])).is_err());
    }

    #[test]
    fn get_events_rearms_an_ignored_wake() {
        let (st, h) = (AutomationState::default(), FakeHost::default());
        let w = st.run(&h, |r| r.spawn(ORCH, new_worker("w"), 1)).unwrap().tab_id;
        st.run(&h, |r| r.get_task(&w, 2)).unwrap();
        st.run(&h, |r| hook(r, ORCH, "finished", None, 3));
        st.run(&h, |r| r.request_help(&w, "a", 4)).unwrap();
        let pastes = || h.log().iter().filter(|l| l.starts_with(&format!("write:{ORCH}:")) && l.contains("200~")).count();
        assert_eq!(pastes(), 1);
        // O legge la coda e chiude il turno: il prossimo evento azionabile lo
        // sveglia di nuovo.
        call(&st, &h, ORCH, "get_events", json!({})).unwrap();
        st.run(&h, |r| hook(r, ORCH, "finished", None, 5));
        st.run(&h, |r| r.complete_task(&w, "completed", "fatto", 6)).unwrap();
        assert_eq!(pastes(), 2);
    }

    #[test]
    fn wake_types_message_into_orchestrator() {
        let st = AutomationState::default();
        let h = FakeHost::default();
        let w = st.run(&h, |r| r.spawn(ORCH, new_worker("w"), 1)).unwrap().tab_id;
        st.run(&h, |r| hook(r, ORCH, "finished", None, 2));
        st.run(&h, |r| r.request_help(&w, "q", 3)).unwrap();
        let log = h.log();
        let paste = log.iter().position(|l| l.starts_with(&format!("write:{ORCH}:")) && l.contains("200~")).unwrap();
        assert!(log[paste].contains("get_events"));
        assert!(log.contains(&format!("write:{ORCH}:\\r")));
    }
}
