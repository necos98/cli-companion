//! Board dei task (v0.12): parte PURA. Ogni workspace ha una coda di task con
//! dipendenze e una lista di proposte pendenti dell'Intake; l'app la esegue con
//! uno scheduler deterministico. Qui ci sono solo tipi, normalizzazione dei file
//! vecchi, validazione, operazioni sulla coda e scheduler: niente I/O, niente
//! `AppHandle`, tempo iniettato. L'integrazione con worker, git e disco e' in
//! `automation::boards`. Il contratto condiviso col frontend e' `docs/BOARD.MD`.

mod report;
mod settings;
mod spec;
pub mod texts;

#[cfg(test)]
mod tests;

pub use report::{build_report, parse_report_fields, ReportFields, TaskReport, TestRun, DIFFSTAT_MAX};
pub use settings::{SettingsProposal, SettingsProposalView};
pub use spec::{
    id_from_title, normalize_repo, parse_max_parallel, parse_proposal, parse_spec, repo_key, valid_branch, valid_id,
    validate_graph, TaskSpec,
};

use crate::automation::{clip_chars, WorkerStatus, DEFAULT_AGENT};
use crate::workspaces::{Delivery, Workspace};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

pub const MAX_TASKS_PER_BOARD: usize = 200;
pub const MAX_PENDING_PROPOSALS: usize = 20;
pub const MAX_PARALLEL_LIMIT: u8 = 16;
pub const DEFAULT_MAX_PARALLEL: u8 = 3;
const LAST_REPORT_MAX: usize = 200;
/// Lunghezza di `note` e `summary` nelle righe compatte di `list_tasks`.
const COMPACT_NOTE_MAX: usize = 200;
const COMPACT_SUMMARY_MAX: usize = 300;

/// Note dell'app sui task (le leggono anche i modelli, via `previous_attempt`).
pub const INTERRUPTED_NOTE: &str = "interrupted: the app was restarted";
pub const ARCHIVED_NOTE: &str = "interrupted: the workspace was archived";
pub const STOPPED_NOTE: &str = "stopped by the user";
const UNINTEGRATED_NOTE: &str = "completed but not integrated (board v0.11): retry to let the worker deliver it";

// ---------------------------------------------------------------------------
// Tipi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Queued,
    Running,
    Done,
    Failed,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 4] = [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Done, TaskStatus::Failed];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<TaskStatus> {
        TaskStatus::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    /// Riferimento da cui il branch e' stato creato (`origin/<base>` o `<base>`).
    pub base: String,
    /// Setup del repository (file copiati, comandi) completato. `false` solo
    /// per un worktree creato dall'app il cui setup non e' finito: un retry lo
    /// rifa. I file vecchi, senza il campo, valgono come fatti.
    #[serde(default = "yes")]
    pub setup_done: bool,
}

fn yes() -> bool {
    true
}

/// Esito del tentativo precedente, mostrato al worker dopo un "Riprova".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousAttempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub task: String,
    pub depends_on: Vec<String>,
    /// Repository (cartella relativa alla root del workspace); `None` = la root.
    pub repo: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    pub status: TaskStatus,
    pub worker_status: Option<WorkerStatus>,
    pub session: Option<String>,
    pub attempts: u32,
    pub worktree: Option<WorktreeInfo>,
    pub last_report: Option<String>,
    pub note: Option<String>,
    pub report: Option<TaskReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_attempt: Option<PreviousAttempt>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// Passo di preparazione in corso (setup del repository) mentre l'app
    /// prepara il worktree; solo in memoria, `None` fuori da `running`.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub preparing: Option<String>,
    /// Ultimo segno di vita della preparazione (ms): il guardiano dell'avvio
    /// lo usa al posto di `started_at` (vedi `stuck_starts`).
    #[serde(skip)]
    pub preparing_since: Option<i64>,
}

/// Proposta pendente dell'Intake: l'utente la accetta o la scarta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    /// Tab dell'Intake che l'ha proposta.
    pub source_tab: String,
    pub created_at: i64,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub workspace_id: String,
    pub max_parallel: u8,
    pub paused: bool,
    pub updated_at: i64,
    /// L'ordine dell'array e' l'ordine della coda.
    pub tasks: Vec<Task>,
    pub proposals: Vec<Proposal>,
    /// Proposte pendenti di modifica delle impostazioni del workspace.
    pub settings_proposals: Vec<SettingsProposal>,
    /// Revisione in memoria (ordina i salvataggi/eventi concorrenti).
    #[serde(skip)]
    pub rev: u64,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            workspace_id: String::new(),
            max_parallel: DEFAULT_MAX_PARALLEL,
            paused: false,
            updated_at: 0,
            tasks: Vec::new(),
            proposals: Vec::new(),
            settings_proposals: Vec::new(),
            rev: 0,
        }
    }
}

/// Task esposto: tutti i campi piu' cio' che la UI deriva dalla board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    /// Dipendenze non `done` (solo per `queued`).
    pub waiting_on: Vec<String>,
    /// Il worker aspetta l'utente (`needs_input` o `idle`).
    pub attention: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardView {
    pub workspace_id: String,
    /// Revisione monotona (cresce a ogni modifica della vista): il frontend
    /// scarta le risposte piu' vecchie di quella gia' vista.
    pub rev: u64,
    pub max_parallel: u8,
    pub paused: bool,
    pub updated_at: i64,
    pub tasks: Vec<TaskView>,
    pub proposals: Vec<Proposal>,
    pub settings_proposals: Vec<SettingsProposalView>,
    /// Il workspace usa le fasce orarie e adesso e' fuori fascia.
    pub schedule_blocked: bool,
    /// Consegna del workspace (comodita' per la UI).
    pub delivery: Delivery,
}

impl BoardView {
    /// Righe compatte per `list_tasks`: conteggi per stato e una riga corta
    /// per task (solo gli stati richiesti, o tutti).
    pub fn to_list_json(&self, statuses: Option<&[TaskStatus]>) -> Value {
        let mut counts = serde_json::Map::new();
        for s in TaskStatus::ALL {
            counts.insert(s.as_str().to_string(), Value::from(self.tasks.iter().filter(|t| t.task.status == s).count()));
        }
        let rows: Vec<Value> = self
            .tasks
            .iter()
            .filter(|t| statuses.is_none_or(|l| l.contains(&t.task.status)))
            .map(compact_row)
            .collect();
        let mut out = json!({ "counts": counts, "tasks": rows, "proposals_pending": self.proposals.len() });
        if self.paused {
            out["paused"] = Value::Bool(true);
        }
        out
    }
}

fn compact_row(v: &TaskView) -> Value {
    let t = &v.task;
    let mut o = json!({ "id": t.id, "title": t.title, "status": t.status.as_str() });
    if v.attention {
        o["attention"] = Value::Bool(true);
    }
    if !v.waiting_on.is_empty() {
        o["waiting_on"] = json!(v.waiting_on);
    }
    if let Some(r) = &t.report {
        o["summary"] = Value::from(clip_with_ellipsis(&r.summary, COMPACT_SUMMARY_MAX));
    }
    if let Some(n) = &t.note {
        o["note"] = Value::from(clip_with_ellipsis(n, COMPACT_NOTE_MAX));
    }
    o
}

/// Tronca a `max` caratteri (mai in mezzo a un carattere), con "…" se tagliato.
fn clip_with_ellipsis(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

impl TaskView {
    /// JSON del task; senza `include_task` manca il testo della specifica.
    pub fn to_json(&self, include_task: bool) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if !include_task {
            if let Some(o) = v.as_object_mut() {
                o.remove("task");
            }
        }
        v
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

impl Task {
    pub fn from_spec(s: TaskSpec, now: i64) -> Task {
        Task {
            id: s.id,
            title: s.title,
            task: s.task,
            depends_on: s.depends_on,
            repo: s.repo,
            agent: DEFAULT_AGENT.to_string(),
            model: s.model,
            created_at: now,
            updated_at: now,
            ..Task::default()
        }
    }

    fn apply_spec(&mut self, s: TaskSpec, now: i64) {
        self.title = s.title;
        self.task = s.task;
        self.depends_on = s.depends_on;
        self.repo = s.repo;
        self.model = s.model;
        self.updated_at = now;
    }

    /// Il worker aspetta l'utente.
    pub fn needs_attention(&self) -> bool {
        self.status == TaskStatus::Running
            && matches!(self.worker_status, Some(WorkerStatus::NeedsInput | WorkerStatus::Idle))
    }
}

// ---------------------------------------------------------------------------
// Caricamento: normalizzazione dei file della v0.11
// ---------------------------------------------------------------------------

/// Porta un file della v0.11 al modello attuale (sul JSON, cosi' i tipi non
/// portano stati che non esistono piu'): vedi docs/BOARD.MD, "Normalizzazione".
fn normalize_legacy(v: &mut Value, now: i64) {
    let Some(tasks) = v.get_mut("tasks").and_then(Value::as_array_mut) else {
        return;
    };
    let status_of = |t: &Value| t.get("status").and_then(Value::as_str).unwrap_or("queued").to_string();
    tasks.retain(|t| status_of(t) != "cancelled");
    let alive: HashSet<String> = tasks.iter().filter_map(|t| t.get("id")?.as_str().map(str::to_string)).collect();
    for t in tasks.iter_mut() {
        let status = status_of(t);
        let Some(o) = t.as_object_mut() else {
            continue;
        };
        let (new, note) = match status.as_str() {
            "queued" => ("queued", None),
            "done" => ("done", None),
            "running" => ("failed", Some(INTERRUPTED_NOTE)),
            "review" => ("failed", Some(UNINTEGRATED_NOTE)),
            _ => ("failed", None), // blocked, failed
        };
        o.insert("status".into(), new.into());
        if let Some(n) = note {
            o.insert("note".into(), n.into());
            o.insert("finished_at".into(), now.into());
            o.insert("updated_at".into(), now.into());
        }
        // I tab dei worker non si ripristinano.
        o.insert("session".into(), Value::Null);
        o.insert("worker_status".into(), Value::Null);
        if let Some(deps) = o.get_mut("depends_on").and_then(Value::as_array_mut) {
            deps.retain(|d| d.as_str().is_some_and(|d| alive.contains(d)));
        }
        if let Some(r) = o.get_mut("report").and_then(Value::as_object_mut) {
            if r.get("status").and_then(Value::as_str) == Some("blocked") {
                r.insert("status".into(), "failed".into());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

impl Board {
    pub fn new(workspace_id: &str) -> Board {
        Board { workspace_id: workspace_id.to_string(), ..Board::default() }
    }

    /// Carica una board dal file, normalizzando quelle della v0.11.
    pub fn from_json(raw: &str, now: i64) -> Result<Board, String> {
        let mut v: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        normalize_legacy(&mut v, now);
        let mut b: Board = serde_json::from_value(v).map_err(|e| e.to_string())?;
        if b.workspace_id.is_empty() {
            return Err("workspace_id missing".to_string());
        }
        b.max_parallel = b.max_parallel.clamp(1, MAX_PARALLEL_LIMIT);
        for t in b.tasks.iter_mut().filter(|t| t.agent.is_empty()) {
            t.agent = DEFAULT_AGENT.to_string();
        }
        Ok(b)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Tutte le dipendenze esistono e sono `done`.
    pub fn deps_done(&self, t: &Task) -> bool {
        t.depends_on.iter().all(|d| self.task(d).is_some_and(|x| x.status == TaskStatus::Done))
    }

    pub fn running_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Running).count()
    }

    fn view_of(&self, t: &Task) -> TaskView {
        let waiting_on = if t.status == TaskStatus::Queued {
            t.depends_on.iter().filter(|d| !self.task(d).is_some_and(|x| x.status == TaskStatus::Done)).cloned().collect()
        } else {
            Vec::new()
        };
        TaskView { task: t.clone(), waiting_on, attention: t.needs_attention() }
    }

    pub fn task_view(&self, id: &str) -> Option<TaskView> {
        self.task(id).map(|t| self.view_of(t))
    }

    /// Vista per il frontend; consegna e valori attuali delle impostazioni
    /// proposte vengono dal workspace `ws` (la board non lo conosce; `None` =
    /// consegna di default e nessun valore attuale), `schedule_blocked` dal gate.
    pub fn view_with(&self, ws: Option<&Workspace>, schedule_blocked: bool) -> BoardView {
        BoardView {
            workspace_id: self.workspace_id.clone(),
            rev: self.rev,
            max_parallel: self.max_parallel,
            paused: self.paused,
            updated_at: self.updated_at,
            tasks: self.tasks.iter().map(|t| self.view_of(t)).collect(),
            proposals: self.proposals.clone(),
            settings_proposals: self.settings_proposals.iter().map(|p| p.view(ws)).collect(),
            schedule_blocked,
            delivery: ws.map(|w| w.delivery).unwrap_or_default(),
        }
    }

    /// Archiviazione del workspace: i task `running` (in avvio o col worker
    /// vivo) tornano in coda, staccati da sessione e preparazione. Ritorna
    /// quanti.
    pub fn requeue_running(&mut self, note: &str, now: i64) -> usize {
        let mut n = 0;
        for t in self.tasks.iter_mut().filter(|t| t.status == TaskStatus::Running) {
            t.status = TaskStatus::Queued;
            t.session = None;
            t.worker_status = None;
            t.preparing = None;
            t.preparing_since = None;
            t.started_at = None;
            t.note = Some(note.to_string());
            t.updated_at = now;
            n += 1;
        }
        if n > 0 {
            self.updated_at = now;
        }
        n
    }

    // --- task nuovi e proposte ---

    /// Verifica i task nuovi contro la board: id validi e unici (anche rispetto
    /// a `reserved`, gli id delle altre proposte pendenti), dipendenze verso la
    /// proposta o task esistenti, niente cicli, limiti.
    pub fn check_specs(&self, specs: &[TaskSpec], reserved: &HashSet<&str>) -> Result<(), String> {
        if specs.is_empty() {
            return Err("tasks is empty".to_string());
        }
        if self.tasks.len() + specs.len() > MAX_TASKS_PER_BOARD {
            return Err(format!("too many tasks on the board (max {MAX_TASKS_PER_BOARD})"));
        }
        let mut seen: HashSet<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        for s in specs {
            if !valid_id(&s.id) {
                return Err(format!("invalid task id: {}", clip_chars(&s.id, 50)));
            }
            if reserved.contains(s.id.as_str()) {
                return Err(format!("duplicate task id: {} (already used by a pending proposal)", s.id));
            }
            if !seen.insert(s.id.as_str()) {
                return Err(format!("duplicate task id: {}", s.id));
            }
        }
        let mut all = self.tasks.clone();
        all.extend(specs.iter().cloned().map(|s| Task::from_spec(s, 0)));
        validate_graph(&all)
    }

    fn pending_ids(&self, except: Option<&str>) -> HashSet<&str> {
        self.proposals
            .iter()
            .filter(|p| Some(p.id.as_str()) != except)
            .flat_map(|p| p.tasks.iter().map(|t| t.id.as_str()))
            .collect()
    }

    /// Id non ancora usati da task o proposte (per i task creati a mano).
    pub fn id_taken(&self, id: &str) -> bool {
        self.task(id).is_some() || self.pending_ids(None).contains(id)
    }

    /// Aggiunge in fondo alla coda task gia' verificati con `check_specs`.
    fn push_specs(&mut self, specs: Vec<TaskSpec>, now: i64) -> Vec<String> {
        let ids = specs.iter().map(|s| s.id.clone()).collect();
        self.tasks.extend(specs.into_iter().map(|s| Task::from_spec(s, now)));
        self.updated_at = now;
        ids
    }

    /// Task creato a mano, senza proposta.
    pub fn create_task(&mut self, spec: TaskSpec, now: i64) -> Result<String, String> {
        self.check_specs(std::slice::from_ref(&spec), &self.pending_ids(None))?;
        Ok(self.push_specs(vec![spec], now).remove(0))
    }

    /// Nuova proposta pendente dell'Intake.
    pub fn add_proposal(&mut self, id: String, source_tab: &str, specs: Vec<TaskSpec>, now: i64) -> Result<(), String> {
        if self.proposals.len() >= MAX_PENDING_PROPOSALS {
            return Err(format!(
                "too many pending proposals (max {MAX_PENDING_PROPOSALS}): wait for the user to accept or discard some"
            ));
        }
        self.check_specs(&specs, &self.pending_ids(None))?;
        self.proposals.push(Proposal { id, source_tab: source_tab.to_string(), created_at: now, tasks: specs });
        self.updated_at = now;
        Ok(())
    }

    pub fn proposal(&self, id: &str) -> Result<&Proposal, String> {
        self.proposals.iter().find(|p| p.id == id).ok_or_else(|| format!("proposal not found: {id}"))
    }

    /// Accetta una proposta: dopo una nuova validazione i task entrano in
    /// coda, nell'ordine della proposta. Se la validazione fallisce la
    /// proposta resta pendente.
    pub fn accept_proposal(&mut self, id: &str, now: i64) -> Result<Vec<String>, String> {
        let specs = self.proposal(id)?.tasks.clone();
        self.check_specs(&specs, &HashSet::new())?;
        self.proposals.retain(|p| p.id != id);
        Ok(self.push_specs(specs, now))
    }

    pub fn discard_proposal(&mut self, id: &str, now: i64) -> Result<(), String> {
        self.proposal(id)?;
        self.proposals.retain(|p| p.id != id);
        self.updated_at = now;
        Ok(())
    }

    // --- modifiche dei task ---

    fn known(&self, id: &str) -> Result<&Task, String> {
        self.task(id).ok_or_else(|| format!("task not found: {id}"))
    }

    /// Modifica parziale di un task `queued` o `failed` (`patch` = campi di
    /// `TaskSpec`); `repo` solo senza worktree.
    pub fn update_task(&mut self, id: &str, patch: &Value, now: i64) -> Result<(), String> {
        let t = self.known(id)?;
        if !matches!(t.status, TaskStatus::Queued | TaskStatus::Failed) {
            return Err(format!("task '{id}' is {}: only queued or failed tasks can be updated", t.status.as_str()));
        }
        let spec = parse_spec(patch, Some(&TaskSpec::from_task(t)))?;
        if patch.as_object().is_some_and(|o| o.keys().all(|k| k == "id")) {
            return Err("no fields to update".to_string());
        }
        if spec.repo != t.repo && t.worktree.is_some() {
            return Err(format!("task '{id}' already has a worktree: its repo cannot be changed"));
        }
        let mut all = self.tasks.clone();
        if let Some(slot) = all.iter_mut().find(|x| x.id == id) {
            slot.apply_spec(spec, now);
        }
        validate_graph(&all)?;
        self.tasks = all;
        self.updated_at = now;
        Ok(())
    }

    /// Toglie un task `queued` o `failed` che nessun task non `done` aspetta.
    /// Ritorna il task rimosso (il suo worktree va ripulito da chi chiama).
    pub fn delete_task(&mut self, id: &str, now: i64) -> Result<Task, String> {
        let t = self.known(id)?;
        if !matches!(t.status, TaskStatus::Queued | TaskStatus::Failed) {
            return Err(format!("task '{id}' is {}: only queued or failed tasks can be deleted", t.status.as_str()));
        }
        let waiting: Vec<&str> = self
            .tasks
            .iter()
            .filter(|x| x.status != TaskStatus::Done && x.depends_on.iter().any(|d| d == id))
            .map(|x| x.id.as_str())
            .collect();
        if !waiting.is_empty() {
            return Err(format!(
                "task '{id}' cannot be deleted: {} depend on it. Remove it from their depends_on (update_task) or delete them first",
                waiting.join(", ")
            ));
        }
        let pos = self.tasks.iter().position(|x| x.id == id).expect("task presente");
        let removed = self.tasks.remove(pos);
        // I task gia' `done` non devono tenere un riferimento a un task sparito.
        for x in &mut self.tasks {
            x.depends_on.retain(|d| d != id);
        }
        self.updated_at = now;
        Ok(removed)
    }

    /// "Riprova": `failed` -> `queued` in fondo alla coda. Worktree e branch
    /// restano; l'esito precedente passa al prossimo worker (`previous_attempt`).
    pub fn retry_task(&mut self, id: &str, now: i64) -> Result<(), String> {
        let t = self.known(id)?;
        if t.status != TaskStatus::Failed {
            return Err(format!("task '{id}' is {}: only failed tasks can be retried", t.status.as_str()));
        }
        let pos = self.tasks.iter().position(|x| x.id == id).expect("task presente");
        let mut t = self.tasks.remove(pos);
        let previous = PreviousAttempt { summary: t.report.take().map(|r| r.summary), note: t.note.take() };
        t.previous_attempt = (previous != PreviousAttempt::default()).then_some(previous);
        t.status = TaskStatus::Queued;
        t.last_report = None;
        t.started_at = None;
        t.finished_at = None;
        t.updated_at = now;
        self.tasks.push(t);
        self.updated_at = now;
        Ok(())
    }

    /// "Segna come fatto": `failed` -> `done` a mano, per un lavoro consegnato
    /// ma finito fallito. Il report resta; la nota del fallimento e il tentativo
    /// precedente no. Worktree e branch non si toccano.
    pub fn mark_done(&mut self, id: &str, now: i64) -> Result<(), String> {
        let t = self.known(id)?;
        if t.status != TaskStatus::Failed {
            return Err(format!("task '{id}' is {}: only failed tasks can be marked done", t.status.as_str()));
        }
        let t = self.task_mut(id).expect("task presente");
        t.status = TaskStatus::Done;
        t.note = None;
        t.previous_attempt = None;
        t.finished_at = Some(now);
        t.updated_at = now;
        self.updated_at = now;
        Ok(())
    }

    /// Sposta un `queued` prima di `before` (un altro `queued`) o in fondo.
    pub fn move_task(&mut self, id: &str, before: Option<&str>, now: i64) -> Result<(), String> {
        self.require_queued(id, "reordered")?;
        if let Some(b) = before {
            if b != id && self.known(b)?.status != TaskStatus::Queued {
                return Err(format!("task '{b}' is not queued: a task can only be placed before a queued task"));
            }
        }
        if before == Some(id) {
            return Ok(());
        }
        let pos = self.tasks.iter().position(|x| x.id == id).expect("task presente");
        let t = self.tasks.remove(pos);
        let at = before.and_then(|b| self.tasks.iter().position(|x| x.id == b)).unwrap_or(self.tasks.len());
        self.tasks.insert(at, t);
        self.updated_at = now;
        Ok(())
    }

    /// Porta un `queued` in testa alla coda.
    pub fn task_to_top(&mut self, id: &str, now: i64) -> Result<(), String> {
        self.require_queued(id, "moved to the top")?;
        let pos = self.tasks.iter().position(|x| x.id == id).expect("task presente");
        let t = self.tasks.remove(pos);
        let at = self.tasks.iter().position(|x| x.status == TaskStatus::Queued).unwrap_or(self.tasks.len());
        self.tasks.insert(at, t);
        self.updated_at = now;
        Ok(())
    }

    fn require_queued(&self, id: &str, what: &str) -> Result<(), String> {
        let t = self.known(id)?;
        if t.status == TaskStatus::Queued {
            Ok(())
        } else {
            Err(format!("task '{id}' is {}: only queued tasks can be {what}", t.status.as_str()))
        }
    }

    pub fn check_stop(&self, id: &str) -> Result<(), String> {
        let t = self.known(id)?;
        if t.status == TaskStatus::Running {
            Ok(())
        } else {
            Err(format!("task '{id}' is {}: only running tasks can be stopped", t.status.as_str()))
        }
    }

    // --- ciclo di vita ---

    /// Segna `running` un task scelto dallo scheduler. Mantiene
    /// `previous_attempt` (lo legge `get_task`).
    pub fn start_task(&mut self, id: &str, now: i64) {
        if let Some(t) = self.task_mut(id) {
            t.status = TaskStatus::Running;
            t.worker_status = Some(WorkerStatus::Starting);
            t.session = None;
            t.attempts += 1;
            t.started_at = Some(now);
            t.preparing = None;
            t.preparing_since = None;
            t.finished_at = None;
            t.note = None;
            t.report = None;
            t.last_report = None;
            t.updated_at = now;
        }
        self.updated_at = now;
    }

    /// Segno di vita della preparazione di un avvio ancora in corso (task
    /// `running` senza sessione, stesso tentativo). `label` = passo da mostrare
    /// (`None` = solo battito: il guardiano non scatta). `false` se l'avvio non
    /// e' piu' quello.
    pub fn touch_preparing(&mut self, id: &str, attempt: u32, label: Option<&str>, now: i64) -> bool {
        let Some(t) = self.task_mut(id) else {
            return false;
        };
        if t.status != TaskStatus::Running || t.attempts != attempt || t.session.is_some() {
            return false;
        }
        t.preparing_since = Some(now);
        if let Some(l) = label {
            t.preparing = Some(l.to_string());
        }
        true
    }

    /// Fine della preparazione (setup finito o fallito): via l'etichetta, il
    /// battito resta a `now`.
    pub fn clear_preparing(&mut self, id: &str, attempt: u32, now: i64) {
        if let Some(t) = self.task_mut(id).filter(|t| t.attempts == attempt && t.status == TaskStatus::Running) {
            t.preparing = None;
            t.preparing_since = Some(now);
        }
    }

    /// Registra il worktree appena creato, prima del setup: se il setup
    /// fallisce, il retry lo ritrova (con `setup_done` falso) e lo rifa.
    pub fn record_worktree(&mut self, id: &str, attempt: u32, wt: WorktreeInfo) -> bool {
        let Some(t) = self.task_mut(id) else {
            return false;
        };
        if t.status != TaskStatus::Running || t.attempts != attempt || t.session.is_some() {
            return false;
        }
        t.worktree = Some(wt);
        true
    }

    /// Fine di un task `running` (report del worker, avvio fallito, tab
    /// chiuso, stop). Se `session` e' dato deve essere quella del task.
    /// `false` se il task non era piu' `running` per quel worker.
    pub fn finish_running(
        &mut self,
        id: &str,
        session: Option<&str>,
        to: TaskStatus,
        report: Option<TaskReport>,
        note: Option<String>,
        now: i64,
    ) -> bool {
        let Some(t) = self.task_mut(id) else {
            return false;
        };
        if t.status != TaskStatus::Running || session.is_some_and(|s| t.session.as_deref() != Some(s)) {
            return false;
        }
        t.status = to;
        t.session = None;
        t.worker_status = None;
        t.preparing = None;
        t.preparing_since = None;
        t.previous_attempt = None;
        if let Some(r) = &report {
            t.last_report = Some(clip_chars(&r.summary, LAST_REPORT_MAX));
        }
        if report.is_some() {
            t.report = report;
        }
        if note.is_some() {
            t.note = note;
        }
        t.finished_at = Some(now);
        t.updated_at = now;
        self.updated_at = now;
        true
    }

    /// Riapre per `session` un task fallito con `failed_note` al tentativo
    /// `attempt`: il worker che lo aveva era ancora vivo e riferisce. Solo se
    /// nessuno l'ha ripreso nel frattempo (ancora `failed`, stesso tentativo,
    /// stessa nota). Il task torna `running` con `note` come traccia; la
    /// conclusione resta a `finish_running`.
    pub fn reopen_failed(&mut self, id: &str, session: &str, attempt: u32, failed_note: &str, note: &str, now: i64) -> bool {
        let Some(t) = self.task_mut(id) else {
            return false;
        };
        if t.status != TaskStatus::Failed || t.attempts != attempt || t.note.as_deref() != Some(failed_note) {
            return false;
        }
        t.status = TaskStatus::Running;
        t.session = Some(session.to_string());
        t.worker_status = Some(WorkerStatus::Running);
        t.note = Some(note.to_string());
        t.finished_at = None;
        t.updated_at = now;
        self.updated_at = now;
        true
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Contesto dello scheduler (lo fornisce chi chiama: il nucleo non conosce
/// i workspace).
#[derive(Debug, Clone, Copy)]
pub struct SchedCtx {
    pub delivery: Delivery,
    /// Falso quando il workspace e' archiviato o usa le fasce ed e' fuori fascia.
    pub gate_open: bool,
}

/// Id dei task da avviare ora, in ordine di coda. Funzione pura.
///
/// In modalita' parallela (worktree) fino a `max_parallel` running nella
/// board; altrimenti un solo running per repository, comunque entro
/// `max_parallel`.
pub fn schedule(board: &Board, ctx: &SchedCtx) -> Vec<String> {
    if board.paused || !ctx.gate_open {
        return Vec::new();
    }
    let mut busy: HashSet<String> = HashSet::new();
    let mut running = 0usize;
    for t in board.tasks.iter().filter(|t| t.status == TaskStatus::Running) {
        running += 1;
        busy.insert(repo_key(t.repo.as_deref()));
    }
    let parallel = ctx.delivery.worktrees();
    let mut chosen = Vec::new();
    for t in board.tasks.iter().filter(|t| t.status == TaskStatus::Queued && board.deps_done(t)) {
        if running + chosen.len() >= board.max_parallel as usize {
            break;
        }
        if !parallel && !busy.insert(repo_key(t.repo.as_deref())) {
            continue;
        }
        chosen.push(t.id.clone());
    }
    chosen
}
