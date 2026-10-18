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
//! - `AutomationState`: `Mutex<Registry>` + `Condvar` (managed state Tauri).
//!   `run` esegue una modifica, preleva gli effetti e li applica FUORI dal
//!   lock; `wait_for_events` e' il long-poll sulla coda.
//! - `Host`: gli effetti verso l'esterno (eventi Tauri, scrittura nel PTY,
//!   timer, workspace). `TauriHost` e' l'implementazione reale; i test usano
//!   un host finto, cosi' anche `dispatch` si prova senza finestra.
//!
//! Il token dell'app (`token()`) autentica le chiamate RPC e gli eventi degli
//! hook: solo i processi figli di un terminale di cli-companion lo conoscono.

use crate::agent_notify::{is_valid_tab_id, RpcRequest, RpcResponse};
use crate::agents::{self, AutomationRequest, PermissionMode};
use crate::workspaces::{Workspace, WorkspaceStore};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

/// Eventi Tauri verso il frontend.
const SPAWN_EVENT: &str = "automation-spawn";
const CLOSE_EVENT: &str = "automation-close";
const UPDATE_EVENT: &str = "automation-update";

/// Tetti e tempi.
pub const MAX_WORKERS_PER_ORCHESTRATOR: usize = 16;
pub const MAX_QUEUE_EVENTS: usize = 1000;
/// Worker (anche chiusi) tenuti in memoria prima di scartare i piu' vecchi.
const MAX_TRACKED_WORKERS: usize = 256;
/// Dopo `complete_task` il tab resta aperto il tempo di vedere l'esito.
pub const CLOSE_DELAY: Duration = Duration::from_millis(1500);
/// Pausa tra l'incolla e l'Invio: la TUI deve aver chiuso il bracketed paste.
pub const ENTER_DELAY: Duration = Duration::from_millis(300);
/// Passo di controllo della vitalita' del client durante `wait_for_events`.
const WAIT_SLICE: Duration = Duration::from_secs(1);
const DEFAULT_WAIT_SECS: u64 = 90;
const MAX_WAIT_SECS: u64 = 600;

const TITLE_MAX: usize = 80;
const TASK_MAX: usize = 100_000;
const MESSAGE_MAX: usize = 20_000;
const SUMMARY_MAX: usize = 50_000;
const LAST_REPORT_MAX: usize = 200;
const CWD_MAX: usize = 1024;
const DEFAULT_AGENT: &str = "claude-code";

/// Prompt passato in riga di comando: ASCII semplice, sicuro in ogni shell.
pub const BOOTSTRAP_PROMPT: &str =
    "Sei una sessione automatica di cli-companion. Chiama subito il tool get_task e segui le istruzioni.";

/// Testo digitato nel prompt dell'orchestratore quando ci sono eventi nuovi.
pub const WAKE_TEXT: &str =
    "[cli-companion] Nuovi eventi dalle sessioni automatiche: chiama wait_for_events.";

/// Istruzioni restituite da `get_task`.
pub const WORKER_INSTRUCTIONS: &str = "\
Sei una sessione automatica di cli-companion, avviata da un orchestratore che ti ha affidato il task qui sopra.\n\
- Lavora in autonomia sul task, senza chiedere conferme all'utente.\n\
- Nessuno legge l'output del tuo terminale: comunica SOLO con i tool MCP di cli-companion.\n\
- report_fact: segnala all'orchestratore i fatti utili che emergono (scoperte, decisioni, avanzamenti importanti).\n\
- request_help: se sei bloccato, fai una domanda all'orchestratore e poi attendi; la risposta arrivera' come messaggio nel tuo prompt.\n\
- complete_task: chiudi SEMPRE il lavoro con questo tool, indicando status (completed, failed o blocked) e un riassunto completo: cosa hai fatto, file toccati, esito dei test.\n\
Se finisci senza chiamare complete_task l'orchestratore non sapra' l'esito.";

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
        return Err("versione RPC non supportata".to_string());
    }
    if req.token.is_empty() || req.token != expected_token {
        return Err("token non valido".to_string());
    }
    if !is_valid_tab_id(&req.session) {
        return Err("sessione non valida".to_string());
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    /// Spawnato, task non ancora letto.
    Starting,
    Running,
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
    /// Quanti `wait_for_events` sono in corso per questo orchestratore.
    waiting: u32,
    /// Messaggio di risveglio gia' digitato e non ancora "consumato".
    wake_pending: bool,
    /// `seq` dell'ultimo evento in coda al momento del risveglio: si
    /// risveglia di nuovo solo per eventi piu' recenti.
    woke_seq: u64,
}

// ---------------------------------------------------------------------------
// Registry (nucleo puro)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Registry {
    workers: HashMap<String, Worker>,
    queues: HashMap<String, Queue>,
    /// Ultimo evento hook (con token valido) per tab: decide il risveglio.
    last_hook: HashMap<String, String>,
    next_seq: u64,
    next_worker: u64,
    effects: Vec<Effect>,
}

/// Testo obbligatorio: trim, 1..=`max` caratteri.
fn req_text(label: &str, s: &str, max: usize) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{label} mancante"));
    }
    if t.chars().count() > max {
        return Err(format!("{label} troppo lungo (max {max} caratteri)"));
    }
    Ok(t.to_string())
}

fn clip_chars(s: &str, max: usize) -> String {
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
            Err("riservato all'orchestratore".to_string())
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
                "troppe sessioni attive (max {MAX_WORKERS_PER_ORCHESTRATOR}): chiudine qualcuna"
            ));
        }
        Ok(())
    }

    /// Registra un nuovo worker figlio di `orch`.
    pub fn spawn(&mut self, orch: &str, new: NewWorker, now: i64) -> Result<SpawnEvent, String> {
        self.check_spawn(orch)?;
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
            },
        );
        self.prune();
        Ok(ev)
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
            .ok_or_else(|| "riservato alle sessioni automatiche".to_string())
    }

    /// Worker di `orch` (per tutti gli altri: stesso errore, nessuna fuga).
    fn owned_worker(&self, orch: &str, session: &str) -> Result<&Worker, String> {
        self.require_orchestrator(orch)?;
        self.workers
            .get(session)
            .filter(|w| w.orchestrator == orch)
            .ok_or_else(|| "sessione non trovata".to_string())
    }

    fn touch(&mut self, session: &str, now: i64) {
        if let Some(w) = self.workers.get_mut(session) {
            w.updated_at = now;
            let info = w.info();
            self.effects.push(Effect::Update(info));
        }
    }

    fn push_event(&mut self, session: &str, kind: EventKind, text: Option<String>, now: i64) {
        let Some(w) = self.workers.get(session) else {
            return;
        };
        let orch = w.orchestrator.clone();
        self.next_seq += 1;
        let ev = Event {
            seq: self.next_seq,
            session: session.to_string(),
            title: w.title.clone(),
            kind,
            text,
            status: w.status,
            at: now,
        };
        let q = self.queues.entry(orch.clone()).or_default();
        q.events.push_back(ev);
        while q.events.len() > MAX_QUEUE_EVENTS {
            q.events.pop_front();
        }
        self.check_wake(&orch);
    }

    /// Decide se svegliare l'orchestratore `tab`: ci sono eventi in coda,
    /// non sta aspettando, il suo ultimo hook e' `finished` (idle al prompt:
    /// con `needs-input` il testo finirebbe in un dialogo di permesso) e, se
    /// l'abbiamo gia' svegliato, sono arrivati eventi nuovi da allora. Un
    /// orchestratore che ignora il risveglio e chiude il turno viene quindi
    /// risvegliato al prossimo evento, non a ogni turno.
    fn check_wake(&mut self, tab: &str) {
        let idle = self.last_hook.get(tab).map(String::as_str) == Some("finished");
        let Some(q) = self.queues.get_mut(tab) else {
            return;
        };
        let Some(last_seq) = q.events.back().map(|e| e.seq) else {
            return;
        };
        if q.waiting > 0 || !idle || (q.wake_pending && last_seq <= q.woke_seq) {
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
        let w = self.worker_mut(session)?;
        let out = json!({
            "session": w.session,
            "title": w.title,
            "task": w.task,
            "orchestrator": w.orchestrator,
            "instructions": WORKER_INSTRUCTIONS,
        });
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
            return Err("la sessione e' chiusa".to_string());
        }
        // Un'azione del worker prova che sta lavorando.
        if matches!(w.status, WorkerStatus::NeedsInput | WorkerStatus::Idle | WorkerStatus::Starting)
        {
            w.status = WorkerStatus::Running;
        }
        w.help_pending = false;
        w.started_sent = true;
        Ok(w)
    }

    /// `report_fact`.
    pub fn report_fact(&mut self, session: &str, text: &str, now: i64) -> Result<(), String> {
        let text = req_text("testo", text, MESSAGE_MAX)?;
        let w = self.worker_activity(session)?;
        w.last_report = Some(clip_chars(&text, LAST_REPORT_MAX));
        self.push_event(session, EventKind::Fact, Some(text), now);
        self.touch(session, now);
        Ok(())
    }

    /// `request_help`: lo stato diventa `needs_input` finche' non arriva la
    /// risposta (o il worker riprende da solo).
    pub fn request_help(&mut self, session: &str, question: &str, now: i64) -> Result<(), String> {
        let q = req_text("domanda", question, MESSAGE_MAX)?;
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
        let status = WorkerStatus::from_final(status)
            .ok_or_else(|| "status non valido (usa completed, failed o blocked)".to_string())?;
        let summary = req_text("riassunto", summary, SUMMARY_MAX)?;
        let w = self.worker_activity(session)?;
        w.status = status;
        w.last_report = Some(clip_chars(&summary, LAST_REPORT_MAX));
        let closing = !w.keep_open;
        self.push_event(session, EventKind::Completed, Some(summary), now);
        self.touch(session, now);
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
            return Err("la sessione e' chiusa".to_string());
        }
        let resumes = matches!(
            w.status,
            WorkerStatus::NeedsInput
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
            return Err("la sessione e' gia' chiusa".to_string());
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
    pub fn on_hook(&mut self, tab: &str, event: &str, message: Option<&str>, now: i64) {
        if self.last_hook.len() > 512 {
            self.last_hook.clear();
        }
        self.last_hook.insert(tab.to_string(), event.to_string());
        let Some(w) = self.workers.get_mut(tab) else {
            self.check_wake(tab);
            return;
        };
        let (status, help_pending) = (w.status, w.help_pending);
        match event {
            "needs-input"
                if matches!(
                    status,
                    WorkerStatus::Starting | WorkerStatus::Running | WorkerStatus::Idle
                ) =>
            {
                w.status = WorkerStatus::NeedsInput;
                let text = message.map(str::to_string);
                self.push_event(tab, EventKind::NeedsInput, text, now);
                self.touch(tab, now);
            }
            "finished"
                if matches!(
                    status,
                    WorkerStatus::Starting | WorkerStatus::Running | WorkerStatus::NeedsInput
                ) && !help_pending =>
            {
                w.status = WorkerStatus::Idle;
                self.push_event(tab, EventKind::StoppedWithoutReport, None, now);
                self.touch(tab, now);
            }
            "session-end" if !status.is_terminal() => {
                w.status = WorkerStatus::Closed;
                self.push_event(tab, EventKind::Exited, None, now);
                self.touch(tab, now);
            }
            _ => {}
        }
    }

    /// Il tab del worker non esiste piu' (chiuso dall'utente, dall'app o
    /// shell uscita). `exited` solo se non era gia' finito o chiuso.
    pub fn tab_closed(&mut self, tab: &str, now: i64) {
        let Some(w) = self.workers.get_mut(tab) else {
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
    }

    // --- coda ---

    /// Inizio di un `wait_for_events`: azzera il risveglio pendente.
    pub fn begin_wait(&mut self, orch: &str) {
        let q = self.queues.entry(orch.to_string()).or_default();
        q.waiting += 1;
        q.wake_pending = false;
    }

    pub fn end_wait(&mut self, orch: &str) {
        if let Some(q) = self.queues.get_mut(orch) {
            q.waiting = q.waiting.saturating_sub(1);
        }
    }

    /// Svuota la coda dell'orchestratore.
    pub fn drain(&mut self, orch: &str) -> Vec<Event> {
        self.queues
            .get_mut(orch)
            .map(|q| q.events.drain(..).collect())
            .unwrap_or_default()
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
    pub permission_mode: PermissionMode,
    pub model: Option<String>,
    pub keep_open: bool,
}

/// Campo stringa opzionale: assente o `null` = `None`; di altro tipo = errore.
fn opt_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("{key} deve essere una stringa")),
    }
}

fn req_str<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(p, key)?.ok_or_else(|| format!("{key} mancante"))
}

/// Titolo di ripiego: le prime parole del task.
fn default_title(task: &str) -> String {
    let words: Vec<&str> = task.split_whitespace().take(8).collect();
    clip_chars(&words.join(" "), TITLE_MAX).trim().to_string()
}

fn valid_model(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

pub fn parse_spawn_params(p: &Value) -> Result<SpawnParams, String> {
    if p.get("workspace").is_some_and(|v| !v.is_null()) {
        return Err(
            "workspace non ammesso: il worker si apre nel workspace dell'orchestratore".to_string(),
        );
    }
    let task = req_text("task", req_str(p, "task")?, TASK_MAX)?;
    let title = match opt_str(p, "title")?.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if t.chars().count() > TITLE_MAX => {
            return Err(format!("title troppo lungo (max {TITLE_MAX} caratteri)"))
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
            return Err("cwd non valido".to_string())
        }
        Some(c) => Some(c.to_string()),
        None => None,
    };
    let permission_mode = match opt_str(p, "permission_mode")? {
        None => PermissionMode::AcceptEdits,
        Some(m) => PermissionMode::parse(m).ok_or_else(|| {
            "permission_mode non valido (usa default, accept-edits, plan o bypass)".to_string()
        })?,
    };
    let model = match opt_str(p, "model")?.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if !valid_model(m) => {
            return Err("model non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string())
        }
        Some(m) => Some(m.to_string()),
        None => None,
    };
    let keep_open = match p.get("keep_open") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("keep_open deve essere un booleano".to_string()),
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
}

/// Managed state Tauri.
#[derive(Default)]
pub struct AutomationState {
    inner: Mutex<Registry>,
    cv: Condvar,
}

impl AutomationState {
    /// Esegue `f` sul registro, poi applica gli effetti fuori dal lock e
    /// sveglia chi aspetta sulla coda.
    pub fn run<H: Host, T>(&self, host: &H, f: impl FnOnce(&mut Registry) -> T) -> T {
        let (out, effects) = {
            let mut reg = self.inner.lock().unwrap();
            let out = f(&mut reg);
            (out, reg.take_effects())
        };
        self.cv.notify_all();
        apply_effects(host, effects);
        out
    }

    /// Long-poll: ritorna subito se la coda di `orch` non e' vuota, altrimenti
    /// attende un evento fino a `timeout` (lista vuota). Si ferma prima se
    /// `alive` dice che il client non c'e' piu': gli eventi restano in coda.
    pub fn wait_for_events(
        &self,
        orch: &str,
        timeout: Duration,
        alive: &dyn Fn() -> bool,
    ) -> Vec<Event> {
        let deadline = Instant::now() + timeout;
        let mut reg = self.inner.lock().unwrap();
        reg.begin_wait(orch);
        let events = loop {
            let events = reg.drain(orch);
            if !events.is_empty() {
                break events;
            }
            let now = Instant::now();
            if now >= deadline || !alive() {
                break Vec::new();
            }
            let slice = (deadline - now).min(WAIT_SLICE);
            reg = self.cv.wait_timeout(reg, slice).unwrap().0;
        };
        reg.end_wait(orch);
        events
    }
}

fn apply_effects<H: Host>(host: &H, effects: Vec<Effect>) {
    for e in effects {
        match e {
            Effect::Update(info) => host.emit_update(&info),
            Effect::Close { tab_id, delay } if delay.is_zero() => host.emit_close(&tab_id),
            Effect::Close { tab_id, delay } => {
                let h = host.clone();
                host.defer(delay, Box::new(move || h.emit_close(&tab_id)));
            }
            Effect::Wake(tab) => {
                let _ = type_message(host, &tab, WAKE_TEXT);
            }
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

const ORCHESTRATOR_METHODS: [&str; 6] = [
    "list_agents",
    "spawn_session",
    "list_sessions",
    "send_message",
    "close_session",
    "wait_for_events",
];
const WORKER_METHODS: [&str; 4] = ["get_task", "report_fact", "request_help", "complete_task"];

fn session_param(p: &Value) -> Result<String, String> {
    let s = req_str(p, "session")?.trim();
    if is_valid_tab_id(s) {
        Ok(s.to_string())
    } else {
        Err("session non valido".to_string())
    }
}

fn parse_wait_secs(p: &Value) -> Result<u64, String> {
    match p.get("timeout_secs") {
        None | Some(Value::Null) => Ok(DEFAULT_WAIT_SECS),
        Some(v) => match v.as_u64() {
            Some(n) if (1..=MAX_WAIT_SECS).contains(&n) => Ok(n),
            _ => Err(format!("timeout_secs deve essere tra 1 e {MAX_WAIT_SECS}")),
        },
    }
}

/// Esegue un metodo RPC per `session`. `alive` dice se il client e' ancora
/// connesso (usato dalle attese lunghe).
pub fn dispatch<H: Host>(
    st: &AutomationState,
    host: &H,
    session: &str,
    method: &str,
    params: &Value,
    alive: &dyn Fn() -> bool,
) -> Result<Value, String> {
    if !(params.is_null() || params.is_object()) {
        return Err("params deve essere un oggetto".to_string());
    }
    let role = st.inner.lock().unwrap().role(session);
    if ORCHESTRATOR_METHODS.contains(&method) && role != Role::Orchestrator {
        return Err(format!("{method} e' riservato all'orchestratore"));
    }
    if WORKER_METHODS.contains(&method) && role != Role::Worker {
        return Err(format!("{method} e' riservato alle sessioni automatiche"));
    }
    let now = now_millis();
    match method {
        "whoami" => {
            let mut out = json!({ "role": role, "session": session });
            if role == Role::Orchestrator {
                if let Some(w) = host.tab_workspace(session) {
                    out["workspace"] =
                        json!({ "id": w.id, "name": w.name, "kind": w.kind, "path": w.path });
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
        "wait_for_events" => {
            let secs = parse_wait_secs(params)?;
            let events = st.wait_for_events(session, Duration::from_secs(secs), alive);
            Ok(json!({ "events": events }))
        }
        "get_task" => st.run(host, |r| r.get_task(session, now)),
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
            let closing = st.run(host, |r| r.complete_task(session, status, summary, now))?;
            Ok(json!({ "closing": closing }))
        }
        _ => Err(format!("metodo sconosciuto: {method}")),
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
        .ok_or_else(|| "workspace dell'orchestratore non trovato".to_string())?;
    if let Some(cwd) = &p.cwd {
        if !cwd_is_absolute(&ws.kind, cwd) {
            return Err("cwd deve essere un percorso assoluto nell'ambiente del workspace".to_string());
        }
        if !cwd_inside_workspace(&ws.kind, &ws.path, cwd) {
            return Err(format!(
                "cwd deve stare dentro il workspace ({}); per i git worktree usa una sottocartella, es. .worktrees/",
                ws.path
            ));
        }
        if !host.dir_exists(ws, cwd) {
            return Err(format!("cartella non trovata: {cwd}"));
        }
    }
    let req = AutomationRequest {
        bootstrap_prompt: BOOTSTRAP_PROMPT,
        permission_mode: p.permission_mode,
        model: p.model.as_deref(),
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
}

/// Punto d'ingresso delle RPC dal socket: autentica e smista.
pub fn handle_rpc(app: &AppHandle, req: &RpcRequest, alive: &dyn Fn() -> bool) -> RpcResponse {
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
        alive,
    ))
}

/// Evento di un hook con token valido (vedi `agent_notify`).
pub fn on_hook_event(app: &AppHandle, payload: &crate::agent_notify::NotifyPayload) {
    let Some(tab) = payload.tab_id.as_deref() else {
        return;
    };
    let st = app.state::<AutomationState>();
    st.run(&TauriHost(app.clone()), |r| {
        r.on_hook(tab, &payload.event, payload.message.as_deref(), now_millis())
    });
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
        r.on_hook(&w, "needs-input", Some("serve un permesso"), 2);
        // Ripetuto: nessun duplicato.
        r.on_hook(&w, "needs-input", Some("serve un permesso"), 3);
        r.on_hook(&w, "finished", None, 4);
        let ev = r.drain(ORCH);
        assert_eq!(kinds(&ev), vec![EventKind::NeedsInput, EventKind::StoppedWithoutReport]);
        assert_eq!(ev[0].text.as_deref(), Some("serve un permesso"));
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Idle);
        r.on_hook(&w, "session-end", None, 5);
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::Exited]);
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Closed);
        // Dopo la chiusura nessun altro evento, nemmeno da tab_closed.
        r.on_hook(&w, "finished", None, 6);
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
        r.on_hook(&w, "finished", None, 3);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::NeedsInput);
        r.report_fact(&w, "ok", 4).unwrap();
        r.complete_task(&w, "completed", "fine", 5).unwrap();
        r.drain(ORCH);
        r.on_hook(&w, "finished", None, 6);
        r.on_hook(&w, "needs-input", None, 7);
        r.on_hook(&w, "session-end", None, 8);
        assert!(r.drain(ORCH).is_empty());
        assert_eq!(r.list_sessions(ORCH)[0].status, WorkerStatus::Completed);
    }

    #[test]
    fn stop_before_get_task_is_reported() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.on_hook(&w, "finished", None, 1);
        assert_eq!(kinds(&r.drain(ORCH)), vec![EventKind::StoppedWithoutReport]);
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
        r.on_hook(ORCH, "finished", None, 2);
        // Evento + O idle: un solo risveglio, non ripetuto.
        r.report_fact(&w, "a", 3).unwrap();
        assert_eq!(wakes(&mut r), 1);
        r.report_fact(&w, "b", 4).unwrap();
        assert_eq!(wakes(&mut r), 0);
        // Rientra in wait_for_events: il flag si azzera, e mentre aspetta
        // non si sveglia.
        r.begin_wait(ORCH);
        r.report_fact(&w, "c", 5).unwrap();
        assert_eq!(wakes(&mut r), 0);
        assert_eq!(r.drain(ORCH).len(), 4);
        r.end_wait(ORCH);
        // Sta elaborando gli eventi: niente risveglio finche' non torna idle.
        r.report_fact(&w, "d", 6).unwrap();
        assert_eq!(wakes(&mut r), 0);
        r.on_hook(ORCH, "finished", None, 7);
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn ignored_wake_repeats_only_for_newer_events() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.on_hook(ORCH, "finished", None, 1);
        r.get_task(&w, 2).unwrap();
        assert_eq!(wakes(&mut r), 1);
        // O chiude il turno senza chiamare wait_for_events: nessun evento
        // nuovo, nessun nuovo risveglio (niente spam a ogni turno).
        r.on_hook(ORCH, "finished", None, 3);
        assert_eq!(wakes(&mut r), 0);
        // Un evento nuovo con O idle: si risveglia di nuovo.
        r.report_fact(&w, "a", 4).unwrap();
        assert_eq!(wakes(&mut r), 1);
    }

    #[test]
    fn no_wake_while_orchestrator_waits_on_permission() {
        let mut r = Registry::default();
        let w = spawn(&mut r, "uno");
        r.on_hook(ORCH, "needs-input", Some("permesso"), 1);
        r.get_task(&w, 2).unwrap();
        assert_eq!(wakes(&mut r), 0);
        // Risolto il permesso, O finisce il turno: ora si sveglia.
        r.on_hook(ORCH, "finished", None, 3);
        assert_eq!(wakes(&mut r), 1);
        // Senza eventi in coda, un finished non sveglia nessuno.
        r.drain(ORCH);
        r.begin_wait(ORCH);
        r.end_wait(ORCH);
        r.on_hook(ORCH, "finished", None, 4);
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
        assert_eq!(ok.permission_mode, PermissionMode::AcceptEdits);
        assert!(!ok.keep_open && ok.cwd.is_none() && ok.model.is_none());
        assert_eq!(ok.title, "fai una cosa molto lunga con tante parole");

        let full = parse_spawn_params(&json!({
            "task":"t","title":" Titolo ","agent":"claude-code",
            "cwd":"C:/p","permission_mode":"bypass","model":"claude-opus-4.1_x","keep_open":true
        }))
        .unwrap();
        assert_eq!(full.title, "Titolo");
        assert_eq!(full.permission_mode, PermissionMode::Bypass);
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
        }
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
    }

    impl FakeHost {
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Host for FakeHost {
        fn tab_workspace(&self, tab_id: &str) -> Option<Workspace> {
            match tab_id {
                ORCH => Some(ws("ws-1", "Proj", "windows")),
                "orch-wsl" => Some(ws("ws-2", "Linux", "wsl")),
                _ => None,
            }
        }
        fn dir_exists(&self, _ws: &Workspace, path: &str) -> bool {
            path.contains("esiste")
        }
        fn agents(&self) -> Vec<(String, String)> {
            vec![("claude-code".to_string(), "Claude Code".to_string())]
        }
        fn launch_command(&self, agent: &str, env_kind: &str, req: &AutomationRequest) -> Result<String, String> {
            if agent != "claude-code" {
                return Err(format!("agente sconosciuto: {agent}"));
            }
            Ok(format!("fake {env_kind} {:?} {}", req.permission_mode, req.bootstrap_prompt))
        }
        fn emit_spawn(&self, ev: &SpawnEvent) {
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
    }

    fn call(st: &AutomationState, h: &FakeHost, session: &str, method: &str, params: Value) -> Result<Value, String> {
        dispatch(st, h, session, method, &params, &|| true)
    }

    #[test]
    fn dispatch_full_flow_with_fake_host() {
        let st = AutomationState::default();
        let h = FakeHost::default();

        assert_eq!(call(&st, &h, ORCH, "whoami", Value::Null).unwrap()["role"], "orchestrator");
        let me = call(&st, &h, ORCH, "whoami", Value::Null).unwrap();
        assert_eq!(me["workspace"], json!({"id":"ws-1","name":"Proj","kind":"windows","path":"C:\\p"}));
        // Orchestratore senza workspace noto: whoami funziona, senza il campo.
        assert!(call(&st, &h, "orch-2", "whoami", Value::Null).unwrap().get("workspace").is_none());
        assert!(call(&st, &h, ORCH, "list_workspaces", json!({})).is_err());
        assert_eq!(call(&st, &h, ORCH, "list_agents", json!({})).unwrap()[0]["id"], "claude-code");

        // Validazioni dello spawn.
        let err = call(&st, &h, ORCH, "spawn_session", json!({"workspace":"Proj","task":"t"})).unwrap_err();
        assert!(err.contains("workspace non ammesso"), "{err}");
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","agent":"altro"})).is_err());
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":"rel"})).is_err());
        assert!(call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":"C:\\p\\manca"})).is_err());
        // Fuori dal workspace: anche se la cartella esiste.
        for cwd in ["C:\\esiste", "C:\\p2\\esiste", "C:\\p\\..\\esiste"] {
            let err = call(&st, &h, ORCH, "spawn_session", json!({"task":"t","cwd":cwd})).unwrap_err();
            assert!(err.contains("dentro il workspace"), "{cwd}: {err}");
        }
        let err = call(&st, &h, "orch-wsl", "spawn_session", json!({"task":"t","cwd":"/home/u2/esiste"})).unwrap_err();
        assert!(err.contains("dentro il workspace"), "{err}");
        // Orchestratore senza workspace noto.
        let err = call(&st, &h, "orch-2", "spawn_session", json!({"task":"t"})).unwrap_err();
        assert_eq!(err, "workspace dell'orchestratore non trovato");
        assert!(h.log().is_empty());

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
        assert!(h.log()[0].starts_with(&format!("spawn:{w}:fake windows Plan Sei una sessione")));

        // Il worker e' un worker: whoami lo dice e i metodi orchestratore falliscono.
        assert_eq!(call(&st, &h, &w, "whoami", json!({})).unwrap()["role"], "worker");
        assert!(call(&st, &h, &w, "spawn_session", json!({"task":"t"})).is_err());
        assert!(call(&st, &h, &w, "wait_for_events", json!({})).is_err());
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

        // wait_for_events: eventi gia' in coda -> subito.
        let ev = call(&st, &h, ORCH, "wait_for_events", json!({"timeout_secs": 5})).unwrap();
        let kinds: Vec<_> = ev["events"].as_array().unwrap().iter().map(|e| e["kind"].clone()).collect();
        assert_eq!(kinds, vec![json!("started"), json!("fact")]);
        assert!(call(&st, &h, ORCH, "wait_for_events", json!({"timeout_secs": 0})).is_err());
        assert!(call(&st, &h, ORCH, "wait_for_events", json!({"timeout_secs": 601})).is_err());

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
    fn wait_for_events_blocks_until_event_or_timeout() {
        let st = Arc::new(AutomationState::default());
        let h = FakeHost::default();
        let w = st.run(&h, |r| r.spawn(ORCH, new_worker("w"), 1)).unwrap().tab_id;

        // Timeout: lista vuota dopo l'attesa.
        let t0 = Instant::now();
        assert!(st.wait_for_events(ORCH, Duration::from_millis(150), &|| true).is_empty());
        assert!(t0.elapsed() >= Duration::from_millis(140));

        // Un evento da un altro thread sveglia l'attesa molto prima del timeout.
        let (st2, h2, w2) = (st.clone(), h.clone(), w.clone());
        let pusher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            st2.run(&h2, |r| r.get_task(&w2, 5)).unwrap();
        });
        let t0 = Instant::now();
        let ev = st.wait_for_events(ORCH, Duration::from_secs(30), &|| true);
        pusher.join().unwrap();
        assert_eq!(kinds(&ev), vec![EventKind::Started]);
        assert!(t0.elapsed() < Duration::from_secs(5));

        // Client sparito: l'attesa termina e gli eventi restano in coda.
        st.run(&h, |r| r.report_fact(&w, "x", 6)).unwrap();
        let gone = st.wait_for_events(ORCH, Duration::from_secs(30), &|| false);
        assert_eq!(gone.len(), 1); // gli eventi gia' presenti vengono comunque consegnati
        assert!(st.wait_for_events(ORCH, Duration::from_secs(30), &|| false).is_empty());
        // Dopo l'attesa il contatore e' tornato a zero: il risveglio e' di nuovo possibile.
        st.run(&h, |r| r.on_hook(ORCH, "finished", None, 7));
        st.run(&h, |r| r.report_fact(&w, "y", 8)).unwrap();
        assert!(h.log().iter().any(|l| l.contains("200~") && l.contains("wait_for_events")));
    }

    #[test]
    fn wake_types_message_into_orchestrator() {
        let st = AutomationState::default();
        let h = FakeHost::default();
        let w = st.run(&h, |r| r.spawn(ORCH, new_worker("w"), 1)).unwrap().tab_id;
        st.run(&h, |r| r.on_hook(ORCH, "finished", None, 2));
        st.run(&h, |r| r.get_task(&w, 3)).unwrap();
        let log = h.log();
        let paste = log.iter().position(|l| l.starts_with(&format!("write:{ORCH}:")) && l.contains("200~")).unwrap();
        assert!(log[paste].contains("wait_for_events"));
        assert!(log.contains(&format!("write:{ORCH}:\\r")));
    }
}
