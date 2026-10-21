//! Board dei task (v0.7): parte PURA. Ogni workspace ha una board persistente
//! di task con dipendenze; l'orchestratore (LLM) la compila, l'app la
//! esegue con uno scheduler. Qui ci sono solo tipi, validazione, derivazione
//! della colonna, controllo dei conflitti e lo scheduler: niente I/O, niente
//! `AppHandle`, tempo iniettato. L'integrazione con worker, git e disco e' in
//! `automation::boards`. Il contratto condiviso col frontend e' `docs/BOARD.MD`.

use crate::agents::PermissionMode;
use crate::automation::{
    clip_chars, default_title, req_text, resolve_permission, valid_model, WorkerStatus, DEFAULT_AGENT,
    TASK_MAX, TITLE_MAX,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const MAX_TASKS_PER_BOARD: usize = 200;
pub const MAX_TASKS_PER_PLAN: usize = 50;
pub const MAX_PARALLEL_LIMIT: u8 = 16;
pub const DEFAULT_MAX_PARALLEL: u8 = 3;
const ID_MAX: usize = 40;
const MAX_DEPENDS: usize = 50;
const MAX_TOUCHES: usize = 50;
const TOUCH_MAX: usize = 256;
const BRANCH_MAX: usize = 100;
const PRIORITY_LIMIT: i64 = 1_000_000;
const COMMIT_MAX: usize = 100;
const LIST_MAX: usize = 100;
const LIST_ITEM_MAX: usize = 2000;
const FILE_MAX: usize = 512;
const TESTS_MAX: usize = 50;
const TEST_COMMAND_MAX: usize = 1000;
const NOTE_MAX: usize = 2000;
pub const DIFFSTAT_MAX: usize = 4000;
const LAST_REPORT_MAX: usize = 200;
pub const SUMMARY_LIMIT: usize = 50_000;

/// Nota dei task `running` al riavvio dell'app (la leggono anche i modelli).
pub const INTERRUPTED_NOTE: &str = "interrupted: the app was restarted";

// ---------------------------------------------------------------------------
// Tipi
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Queued,
    Running,
    Review,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    #[default]
    Worktree,
    None,
}

/// Colonna derivata mostrata dalla UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    Backlog,
    Ready,
    Running,
    Review,
    Done,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    pub base: String,
    /// Repository del task (cartella relativa alla root); `None` = la root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Setup del repository (file copiati, comandi) completato. `false` solo
    /// per un worktree creato dall'app il cui setup non e' finito: un retry lo
    /// rifa. I file vecchi, senza il campo, valgono come fatti.
    #[serde(default = "yes")]
    pub setup_done: bool,
}

fn yes() -> bool {
    true
}

/// Branch di integrazione di un repository, gestito dalla board: l'app lo
/// crea (con un worktree dedicato) e i task del repo partono da qui.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Integration {
    pub branch: String,
    /// Worktree dove l'orchestratore integra e collauda.
    pub path: String,
    /// Riferimento da cui il branch e' stato creato.
    pub base: String,
    /// Come `WorktreeInfo::setup_done`, per il worktree di integrazione.
    #[serde(default = "yes")]
    pub setup_done: bool,
    /// Vero quando la fase di setup e' girata ma il repository non ha nulla da
    /// preparare (niente copy_ignored, niente comandi): `setup_done` resta vero
    /// per lo scheduling, ma l'orchestratore deve sapere che nulla e' stato preparato.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub setup_skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRun {
    pub command: String,
    /// passed | failed | not_run
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskReport {
    /// completed | failed | blocked
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub tests: Vec<TestRun>,
    #[serde(default)]
    pub follow_ups: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diffstat: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub task: String,
    pub depends_on: Vec<String>,
    pub touches: Vec<String>,
    pub isolation: Isolation,
    pub base_branch: Option<String>,
    /// Repository (cartella relativa alla root del workspace); `None` = la root.
    pub repo: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    pub permission_mode: Option<String>,
    pub priority: i32,
    pub held: bool,
    pub status: TaskStatus,
    pub worker_status: Option<WorkerStatus>,
    pub session: Option<String>,
    pub attempts: u32,
    pub worktree: Option<WorktreeInfo>,
    pub last_report: Option<String>,
    pub note: Option<String>,
    /// Completato fuori dalla board (`done` da `queued`, con nota): nessun
    /// worker, nessun worktree.
    pub external: bool,
    /// Nota dell'orchestratore per se stesso: mai inviata al worker.
    pub orchestrator_note: Option<String>,
    pub report: Option<TaskReport>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub workspace_id: String,
    pub max_parallel: u8,
    pub paused: bool,
    pub base_branch: Option<String>,
    /// Branch base per repository (chiave = repo, "." = root).
    pub base_branches: BTreeMap<String, String>,
    /// Modello del branch di integrazione (`{date}` = data locale); `None` = spento.
    pub integration_branch: Option<String>,
    /// Integrazioni risolte per repository (chiave "." = root).
    pub integrations: BTreeMap<String, Integration>,
    /// Tab dell'orchestratore proprietario: non si ricarica dal disco.
    #[serde(skip_deserializing)]
    pub owner: Option<String>,
    pub updated_at: i64,
    pub tasks: Vec<Task>,
    /// Revisione in memoria (ordina i salvataggi/eventi concorrenti).
    #[serde(skip)]
    pub rev: u64,
    /// `plan_completed` gia' inviato per questa transizione.
    #[serde(skip)]
    pub plan_completed_sent: bool,
    /// Stato dell'esecuzione automatica (solo workspace automatici); lo
    /// imposta il registro a ogni pubblicazione.
    #[serde(skip)]
    pub automation: Option<crate::schedule::AutomationInfo>,
    /// Stato dell'orchestratore del workspace (vedi `BoardView::orchestrator`);
    /// lo imposta il registro a ogni pubblicazione.
    #[serde(skip)]
    pub orchestrator: Option<String>,
    /// Repository rilevati nel workspace (calcolati su richiesta, in cache
    /// fino a submit_plan / set_board).
    #[serde(skip)]
    pub repos: Option<Vec<String>>,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            workspace_id: String::new(),
            max_parallel: DEFAULT_MAX_PARALLEL,
            paused: false,
            base_branch: None,
            base_branches: BTreeMap::new(),
            integration_branch: None,
            integrations: BTreeMap::new(),
            owner: None,
            updated_at: 0,
            tasks: Vec::new(),
            rev: 0,
            plan_completed_sent: false,
            automation: None,
            orchestrator: None,
            repos: None,
        }
    }
}

/// Task esposto: tutti i campi piu' la colonna derivata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskView {
    #[serde(flatten)]
    pub task: Task,
    pub column: Column,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardView {
    pub workspace_id: String,
    /// Revisione monotona (cresce a ogni modifica della vista): il frontend
    /// scarta le risposte piu' vecchie di quella gia' vista.
    pub rev: u64,
    pub max_parallel: u8,
    pub paused: bool,
    pub base_branch: Option<String>,
    pub base_branches: BTreeMap<String, String>,
    pub integration_branch: Option<String>,
    pub integrations: BTreeMap<String, Integration>,
    /// Repository rilevati; null finche' non sono stati calcolati.
    pub repos: Option<Vec<String>>,
    pub owner: Option<String>,
    pub updated_at: i64,
    pub tasks: Vec<TaskView>,
    /// Null per i workspace non automatici.
    pub automation: Option<crate::schedule::AutomationInfo>,
    /// Orchestratore del workspace: `live` (disponibile), `starting` (aperto
    /// dall'app, non ancora vivo), `waiting` (serve e l'app lo apre al
    /// prossimo giro), `stopped` (serve ma l'app non lo riapre: chiuso
    /// dall'utente o troppi tentativi), `window` (serve, fuori dalla finestra
    /// automatica); null se la board non ne ha bisogno e nessuno e' vivo.
    /// Non e' in `to_compact_json`.
    pub orchestrator: Option<String>,
}

/// Lunghezza massima di `note` e `summary` nelle righe compatte.
const COMPACT_NOTE_MAX: usize = 200;
const COMPACT_SUMMARY_MAX: usize = 300;

/// Tronca a `max` caratteri (mai in mezzo a un carattere), con "…" se tagliato.
fn clip_with_ellipsis(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

impl Column {
    pub const ALL: [Column; 8] = [
        Column::Backlog,
        Column::Ready,
        Column::Running,
        Column::Review,
        Column::Done,
        Column::Failed,
        Column::Blocked,
        Column::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Column::Backlog => "backlog",
            Column::Ready => "ready",
            Column::Running => "running",
            Column::Review => "review",
            Column::Done => "done",
            Column::Failed => "failed",
            Column::Blocked => "blocked",
            Column::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

impl BoardView {
    /// JSON compatto per l'orchestratore: impostazioni, conteggi per colonna
    /// e una riga corta per task. Senza `columns` mostra i task non
    /// `done`/`cancelled`; con `columns` solo quelli nelle colonne indicate.
    pub fn to_compact_json(&self, columns: Option<&[Column]>) -> Value {
        let mut counts = Map::new();
        for c in Column::ALL {
            let n = self.tasks.iter().filter(|t| t.column == c).count();
            if n > 0 {
                counts.insert(c.as_str().to_string(), Value::from(n));
            }
        }
        let shown = |c: Column| match columns {
            Some(cols) => cols.contains(&c),
            None => !matches!(c, Column::Done | Column::Cancelled),
        };
        let rows: Vec<Value> = self.tasks.iter().filter(|t| shown(t.column)).map(|t| self.compact_row(t)).collect();
        // I task trattenuti stanno nella colonna `ready` ma non partono: conteggio a parte.
        let held = self.tasks.iter().filter(|t| t.task.held && t.task.status == TaskStatus::Queued).count();
        let mut out = serde_json::json!({
            "workspace_id": self.workspace_id,
            "rev": self.rev,
            "max_parallel": self.max_parallel,
            "paused": self.paused,
            "base_branch": self.base_branch,
            "owner": self.owner,
            "updated_at": self.updated_at,
            "automation": self.automation,
            "counts": counts,
            "tasks": rows,
        });
        if held > 0 {
            out["held"] = Value::from(held);
        }
        // Multi-repository: solo se servono, per non appesantire il caso comune.
        if !self.base_branches.is_empty() {
            out["base_branches"] = serde_json::json!(self.base_branches);
        }
        if self.integration_branch.is_some() {
            out["integration_branch"] = serde_json::json!(self.integration_branch);
        }
        if !self.integrations.is_empty() {
            // Solo cio' che serve all'orchestratore; `setup` riassume setup_done/setup_skipped.
            let ints: Map<String, Value> = self
                .integrations
                .iter()
                .map(|(k, i)| {
                    let setup = if !i.setup_done {
                        "pending"
                    } else if i.setup_skipped {
                        "none"
                    } else {
                        "done"
                    };
                    (
                        k.clone(),
                        serde_json::json!({"branch": i.branch, "path": i.path, "base": i.base, "setup": setup}),
                    )
                })
                .collect();
            out["integrations"] = Value::Object(ints);
        }
        if let Some(r) = &self.repos {
            out["repos"] = serde_json::json!(r);
        }
        out
    }

    fn compact_row(&self, v: &TaskView) -> Value {
        let t = &v.task;
        let mut o = Map::new();
        o.insert("id".into(), Value::from(t.id.clone()));
        o.insert("title".into(), Value::from(t.title.clone()));
        o.insert("column".into(), Value::from(v.column.as_str()));
        o.insert("attempts".into(), Value::from(t.attempts));
        if let Some(r) = &t.repo {
            o.insert("repo".into(), Value::from(r.clone()));
        }
        if t.held && t.status == TaskStatus::Queued {
            o.insert("held".into(), Value::Bool(true));
        }
        if let Some(w) = &t.worker_status {
            o.insert("worker_status".into(), serde_json::to_value(w).unwrap_or(Value::Null));
        }
        if let Some(p) = &t.preparing {
            o.insert("preparing".into(), Value::from(p.clone()));
        }
        if v.column == Column::Backlog {
            let waiting: Vec<&String> = t
                .depends_on
                .iter()
                .filter(|d| !self.tasks.iter().any(|x| &x.task.id == *d && x.column == Column::Done))
                .collect();
            if !waiting.is_empty() {
                o.insert("waiting_on".into(), serde_json::json!(waiting));
            }
        }
        if t.priority != 0 {
            o.insert("priority".into(), Value::from(t.priority));
        }
        if let Some(m) = &t.model {
            o.insert("model".into(), Value::from(m.clone()));
        }
        if t.external {
            o.insert("external".into(), Value::Bool(true));
        }
        if let Some(n) = &t.note {
            o.insert("note".into(), Value::from(clip_with_ellipsis(n, COMPACT_NOTE_MAX)));
        }
        if let Some(n) = &t.orchestrator_note {
            o.insert("orchestrator_note".into(), Value::from(clip_with_ellipsis(n, COMPACT_NOTE_MAX)));
        }
        if let Some(r) = &t.report {
            o.insert("summary".into(), Value::from(clip_with_ellipsis(&r.summary, COMPACT_SUMMARY_MAX)));
        }
        Value::Object(o)
    }
}

impl TaskView {
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
// Validazione dei campi
// ---------------------------------------------------------------------------

/// `^[a-z0-9][a-z0-9-]{0,39}$`
pub fn valid_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= ID_MAX
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Nome di branch sicuro da passare come argv a git: niente `-` iniziale,
/// niente `..`, solo caratteri semplici.
pub fn valid_branch(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= BRANCH_MAX
        && !s.starts_with('-')
        && !s.starts_with('/')
        && !s.ends_with('/')
        && !s.ends_with('.')
        && !s.contains("..")
        && !s.contains("//")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// Normalizza un'area: trim, `\` -> `/`, niente `./` iniziale ne' `/` finale,
/// relativa alla root e senza `..`. L'area che coincide con la root vale ".".
pub fn normalize_area(s: &str) -> Result<String, String> {
    let t = s.trim().replace('\\', "/");
    if t.is_empty() {
        return Err("empty area in touches".to_string());
    }
    if t.chars().count() > TOUCH_MAX || t.chars().any(char::is_control) {
        return Err(format!("invalid area in touches: {}", clip_chars(&t, 40)));
    }
    let b = t.as_bytes();
    if t.starts_with('/') || (b.len() >= 2 && b[1] == b':') {
        return Err(format!("touches must be relative to the workspace root: {t}"));
    }
    let segs: Vec<&str> = t.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.contains(&"..") {
        return Err(format!("touches cannot contain '..': {t}"));
    }
    Ok(if segs.is_empty() { ".".to_string() } else { segs.join("/") })
}

fn area_segments(area: &str, ci: bool) -> Vec<String> {
    area.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(|s| if ci { s.to_lowercase() } else { s.to_string() })
        .collect()
}

/// Due aree si sovrappongono se una e' uguale o prefisso per segmenti
/// dell'altra. `ci`: confronto senza maiuscole (workspace Windows).
pub fn areas_overlap(a: &str, b: &str, ci: bool) -> bool {
    let (x, y) = (area_segments(a, ci), area_segments(b, ci));
    let n = x.len().min(y.len());
    x[..n] == y[..n]
}

/// Due task confliggono se dichiarano aree sovrapposte.
pub fn touches_conflict(a: &[String], b: &[String], ci: bool) -> bool {
    a.iter().any(|x| b.iter().any(|y| areas_overlap(x, y, ci)))
}

fn str_list(v: &Value, label: &str, max_items: usize, max_len: usize) -> Result<Vec<String>, String> {
    let arr = v.as_array().ok_or_else(|| format!("{label} must be an array of strings"))?;
    if arr.len() > max_items {
        return Err(format!("{label} has too many entries (max {max_items})"));
    }
    arr.iter()
        .map(|e| {
            let s = e.as_str().ok_or_else(|| format!("{label} must be an array of strings"))?.trim();
            if s.is_empty() || s.chars().count() > max_len {
                return Err(format!("{label}: entries must be 1..={max_len} characters"));
            }
            Ok(s.to_string())
        })
        .collect()
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

fn parse_branch(v: &Value, label: &str) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().is_empty() => Ok(None),
        Value::String(s) if valid_branch(s.trim()) => Ok(Some(s.trim().to_string())),
        Value::String(_) => Err(format!(
            "invalid {label} (letters, digits, '.', '_', '-' and '/', max {BRANCH_MAX}, no leading '-')"
        )),
        _ => Err(format!("{label} must be a string")),
    }
}

/// Repository di un task: cartella relativa alla root. Normalizza i
/// separatori (`/`), toglie `./` iniziale e `/` finale; niente path assoluti
/// ne' `..`. `"."` (la root) vale `None`.
pub fn normalize_repo(s: &str) -> Result<Option<String>, String> {
    let t = s.trim().replace('\\', "/");
    if t.is_empty() {
        return Err("repo is empty (use \".\" for the workspace root, or null)".to_string());
    }
    if t.chars().count() > TOUCH_MAX || t.chars().any(char::is_control) {
        return Err(format!("invalid repo: {}", clip_chars(&t, 40)));
    }
    let b = t.as_bytes();
    if t.starts_with('/') || (b.len() >= 2 && b[1] == b':') {
        return Err(format!("repo must be relative to the workspace root: {t}"));
    }
    let segs: Vec<&str> = t.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.contains(&"..") {
        return Err(format!("repo cannot contain '..': {t}"));
    }
    Ok(if segs.is_empty() { None } else { Some(segs.join("/")) })
}

fn parse_repo(v: &Value) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => normalize_repo(s),
        _ => Err("repo must be a string".to_string()),
    }
}

/// Chiave di un repository nelle mappe della board: "." = la root.
pub fn repo_key(repo: Option<&str>) -> String {
    repo.unwrap_or(".").to_string()
}

/// `base_branches` di `submit_plan` / `set_board`: (repo, branch) da
/// impostare; branch `None` (stringa vuota o null) = toglie la voce.
pub type BaseBranchOps = Vec<(String, Option<String>)>;

pub fn parse_base_branches(v: &Value) -> Result<BaseBranchOps, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "base_branches must be an object (repo -> branch; an empty string removes the entry)".to_string())?;
    if obj.len() > MAX_TOUCHES {
        return Err(format!("base_branches has too many entries (max {MAX_TOUCHES})"));
    }
    let mut out = Vec::new();
    for (k, val) in obj {
        let key = repo_key(normalize_repo(k)?.as_deref());
        out.push((key, parse_branch(val, &format!("base_branches[{k}]"))?));
    }
    Ok(out)
}

/// Data di esempio per validare il modello del branch di integrazione.
const SAMPLE_DATE: &str = "2000-01-01";

/// Espande il modello del branch di integrazione (`{date}` = `date`).
pub fn expand_integration(template: &str, date: &str) -> Result<String, String> {
    let name = template.replace("{date}", date);
    if valid_branch(&name) {
        Ok(name)
    } else {
        Err(format!("invalid integration branch name: {}", clip_chars(&name, 60)))
    }
}

/// `integration_branch` di `submit_plan` / `set_board`: `None` spegne.
pub fn parse_integration_template(v: &Value) -> Result<Option<String>, String> {
    let t = match v {
        Value::Null => None,
        Value::String(s) if s.trim().is_empty() => None,
        Value::String(s) => Some(s.trim().to_string()),
        _ => return Err("integration_branch must be a string".to_string()),
    };
    if let Some(t) = &t {
        expand_integration(t, SAMPLE_DATE).map_err(|e| format!("integration_branch: {e}"))?;
    }
    Ok(t)
}

/// `branch` e' `name` o un suo derivato con suffisso numerico (`name-2`)?
pub fn integration_matches(name: &str, branch: &str) -> bool {
    branch == name
        || branch
            .strip_prefix(name)
            .and_then(|r| r.strip_prefix('-'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// Primo nome libero: `name`, poi `name-2`, `name-3`, ...
pub fn first_free_branch(name: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    (1..=100)
        .map(|n| if n == 1 { name.to_string() } else { format!("{name}-{n}") })
        .find(|c| valid_branch(c) && !exists(c))
}

pub fn parse_max_parallel(v: &Value) -> Result<u8, String> {
    match v.as_u64() {
        Some(n) if (1..=MAX_PARALLEL_LIMIT as u64).contains(&n) => Ok(n as u8),
        _ => Err(format!("max_parallel must be an integer between 1 and {MAX_PARALLEL_LIMIT}")),
    }
}

// ---------------------------------------------------------------------------
// TaskSpec
// ---------------------------------------------------------------------------

/// Campi di input di un task (`submit_plan` / `update_task`), normalizzati.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: String,
    pub title: String,
    pub task: String,
    pub depends_on: Vec<String>,
    pub touches: Vec<String>,
    pub isolation: Isolation,
    pub base_branch: Option<String>,
    pub repo: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    pub permission_mode: Option<PermissionMode>,
    pub priority: i32,
    pub held: bool,
    pub orchestrator_note: Option<String>,
}

pub const SPEC_KEYS: [&str; 14] = [
    "id",
    "title",
    "task",
    "depends_on",
    "touches",
    "isolation",
    "base_branch",
    "repo",
    "agent",
    "model",
    "permission_mode",
    "priority",
    "held",
    "orchestrator_note",
];

impl TaskSpec {
    pub fn from_task(t: &Task) -> TaskSpec {
        TaskSpec {
            id: t.id.clone(),
            title: t.title.clone(),
            task: t.task.clone(),
            depends_on: t.depends_on.clone(),
            touches: t.touches.clone(),
            isolation: t.isolation,
            base_branch: t.base_branch.clone(),
            repo: t.repo.clone(),
            agent: t.agent.clone(),
            model: t.model.clone(),
            permission_mode: t.permission_mode.as_deref().and_then(PermissionMode::parse),
            priority: t.priority,
            held: t.held,
            orchestrator_note: t.orchestrator_note.clone(),
        }
    }
}

/// Legge un task da JSON. Senza `base` e' una creazione (`id` e `task`
/// obbligatori); con `base` e' una modifica parziale: i campi assenti
/// restano quelli di `base`. Campi sconosciuti = errore (refusi).
pub fn parse_spec(v: &Value, base: Option<&TaskSpec>) -> Result<TaskSpec, String> {
    let obj = v.as_object().ok_or_else(|| "each task must be an object".to_string())?;
    for k in obj.keys() {
        if !SPEC_KEYS.contains(&k.as_str()) {
            return Err(format!("unknown field '{k}' (allowed: {})", SPEC_KEYS.join(", ")));
        }
    }
    let nonnull = |k: &str| obj.get(k).filter(|x| !x.is_null());
    let text = |k: &str| -> Result<Option<&str>, String> {
        match nonnull(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(format!("{k} must be a string")),
        }
    };

    let id = match (text("id")?, base) {
        (Some(s), Some(b)) if s.trim() != b.id => return Err("the id of a task cannot be changed".to_string()),
        (_, Some(b)) => b.id.clone(),
        (Some(s), None) => {
            let s = s.trim();
            if !valid_id(s) {
                return Err(format!(
                    "invalid id '{}' (lowercase letters, digits and '-', 1..={ID_MAX} characters, not starting with '-')",
                    clip_chars(s, 50)
                ));
            }
            s.to_string()
        }
        (None, None) => return Err("id is missing".to_string()),
    };
    let task = match (text("task")?, base) {
        (Some(s), _) => req_text("task", s, TASK_MAX)?,
        (None, Some(b)) => b.task.clone(),
        (None, None) => return Err("task is missing".to_string()),
    };
    let title = match (text("title")?.map(str::trim).filter(|t| !t.is_empty()), base) {
        (Some(t), _) if t.chars().count() > TITLE_MAX => {
            return Err(format!("title is too long (max {TITLE_MAX} characters)"))
        }
        (Some(t), _) => t.to_string(),
        (None, Some(b)) => b.title.clone(),
        (None, None) => default_title(&task),
    };
    let depends_on = match nonnull("depends_on") {
        Some(v) => {
            let list = dedup(str_list(v, "depends_on", MAX_DEPENDS, ID_MAX)?);
            if let Some(bad) = list.iter().find(|d| !valid_id(d)) {
                return Err(format!("invalid task id in depends_on: {}", clip_chars(bad, 50)));
            }
            list
        }
        None if obj.contains_key("depends_on") => Vec::new(),
        None => base.map(|b| b.depends_on.clone()).unwrap_or_default(),
    };
    let touches = match nonnull("touches") {
        Some(v) => {
            let raw = str_list(v, "touches", MAX_TOUCHES, TOUCH_MAX)?;
            let norm: Result<Vec<String>, String> = raw.iter().map(|a| normalize_area(a)).collect();
            dedup(norm?)
        }
        None if obj.contains_key("touches") => Vec::new(),
        None => base.map(|b| b.touches.clone()).unwrap_or_default(),
    };
    let isolation = match text("isolation")? {
        Some("worktree") => Isolation::Worktree,
        Some("none") => Isolation::None,
        Some(_) => return Err("invalid isolation (use worktree or none)".to_string()),
        None => base.map(|b| b.isolation).unwrap_or_default(),
    };
    let base_branch = match obj.get("base_branch") {
        Some(v) => parse_branch(v, "base_branch")?,
        None => base.and_then(|b| b.base_branch.clone()),
    };
    let repo = match obj.get("repo") {
        Some(v) => parse_repo(v)?,
        None => base.and_then(|b| b.repo.clone()),
    };
    let agent = match text("agent")?.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => a.to_string(),
        None => base.map(|b| b.agent.clone()).unwrap_or_else(|| DEFAULT_AGENT.to_string()),
    };
    let model = match obj.get("model") {
        None => base.and_then(|b| b.model.clone()),
        Some(Value::Null) => None,
        Some(Value::String(m)) if m.trim().is_empty() => None,
        Some(Value::String(m)) if valid_model(m.trim()) => Some(m.trim().to_string()),
        Some(Value::String(_)) => {
            return Err("invalid model (only letters, digits, '.', '_' and '-', max 64)".to_string())
        }
        Some(_) => return Err("model must be a string".to_string()),
    };
    let permission_mode = match obj.get("permission_mode") {
        None => base.and_then(|b| b.permission_mode),
        Some(Value::Null) => None,
        Some(Value::String(m)) => Some(PermissionMode::parse(m).ok_or_else(|| {
            "invalid permission_mode (use default, accept-edits, plan or bypass)".to_string()
        })?),
        Some(_) => return Err("permission_mode must be a string".to_string()),
    };
    let priority = match nonnull("priority") {
        None => base.map(|b| b.priority).unwrap_or(0),
        Some(v) => match v.as_i64() {
            Some(n) if n.abs() <= PRIORITY_LIMIT => n as i32,
            _ => return Err(format!("priority must be an integer between -{PRIORITY_LIMIT} and {PRIORITY_LIMIT}")),
        },
    };
    let held = match nonnull("held") {
        None => base.map(|b| b.held).unwrap_or(false),
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("held must be a boolean".to_string()),
    };
    let orchestrator_note = match obj.get("orchestrator_note") {
        None => base.and_then(|b| b.orchestrator_note.clone()),
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(clip_chars(s.trim(), NOTE_MAX)).filter(|n| !n.is_empty()),
        Some(_) => return Err("orchestrator_note must be a string".to_string()),
    };
    Ok(TaskSpec {
        id,
        title,
        task,
        depends_on,
        touches,
        isolation,
        base_branch,
        repo,
        agent,
        model,
        permission_mode,
        priority,
        held,
        orchestrator_note,
    })
}

/// Parametri di `submit_plan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    pub specs: Vec<TaskSpec>,
    pub max_parallel: Option<u8>,
    pub base_branch: Option<String>,
    pub base_branches: BaseBranchOps,
    /// `None` = non toccare; `Some(None)` = spegni.
    pub integration_branch: Option<Option<String>>,
}

pub fn parse_plan(p: &Value) -> Result<PlanInput, String> {
    let obj = p.as_object().ok_or_else(|| "params must be an object".to_string())?;
    for k in obj.keys() {
        if !["tasks", "max_parallel", "base_branch", "base_branches", "integration_branch"].contains(&k.as_str()) {
            return Err(format!(
                "unknown field '{k}' (allowed: tasks, max_parallel, base_branch, base_branches, integration_branch)"
            ));
        }
    }
    let arr = obj
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| "tasks is missing (an array of task objects)".to_string())?;
    if arr.is_empty() {
        return Err("tasks is empty".to_string());
    }
    if arr.len() > MAX_TASKS_PER_PLAN {
        return Err(format!("too many tasks in one plan (max {MAX_TASKS_PER_PLAN})"));
    }
    let mut specs = Vec::with_capacity(arr.len());
    for (i, t) in arr.iter().enumerate() {
        specs.push(parse_spec(t, None).map_err(|e| format!("tasks[{i}]: {e}"))?);
    }
    let max_parallel = match obj.get("max_parallel") {
        None | Some(Value::Null) => None,
        Some(v) => Some(parse_max_parallel(v)?),
    };
    let base_branch = match obj.get("base_branch") {
        None => None,
        Some(v) => parse_branch(v, "base_branch")?,
    };
    let base_branches = match obj.get("base_branches") {
        None | Some(Value::Null) => Vec::new(),
        Some(v) => parse_base_branches(v)?,
    };
    let integration_branch = match obj.get("integration_branch") {
        None => None,
        Some(v) => Some(parse_integration_template(v)?),
    };
    Ok(PlanInput { specs, max_parallel, base_branch, base_branches, integration_branch })
}

/// I permessi richiesti dai task devono stare entro il tetto del workspace.
pub fn check_permissions(specs: &[TaskSpec], ceiling: PermissionMode) -> Result<(), String> {
    for s in specs {
        resolve_permission(s.permission_mode, ceiling).map_err(|e| format!("task '{}': {e}", s.id))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Grafo
// ---------------------------------------------------------------------------

/// Dipendenze esistenti, niente auto-dipendenze, niente cicli.
pub fn validate_graph(tasks: &[Task]) -> Result<(), String> {
    let idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    for t in tasks {
        for d in &t.depends_on {
            if *d == t.id {
                return Err(format!("task '{}' cannot depend on itself", t.id));
            }
            if !idx.contains_key(d.as_str()) {
                return Err(format!("task '{}' depends on unknown task '{d}'", t.id));
            }
        }
    }
    // DFS con colori: 0 = nuovo, 1 = nel percorso, 2 = chiuso.
    fn visit(i: usize, tasks: &[Task], idx: &HashMap<&str, usize>, color: &mut [u8], path: &mut Vec<usize>) -> Option<Vec<usize>> {
        color[i] = 1;
        path.push(i);
        for d in &tasks[i].depends_on {
            let j = idx[d.as_str()];
            if color[j] == 1 {
                let start = path.iter().position(|&p| p == j).unwrap_or(0);
                let mut cycle = path[start..].to_vec();
                cycle.push(j);
                return Some(cycle);
            }
            if color[j] == 0 {
                if let Some(c) = visit(j, tasks, idx, color, path) {
                    return Some(c);
                }
            }
        }
        path.pop();
        color[i] = 2;
        None
    }
    let mut color = vec![0u8; tasks.len()];
    for i in 0..tasks.len() {
        if color[i] == 0 {
            if let Some(c) = visit(i, tasks, &idx, &mut color, &mut Vec::new()) {
                let names: Vec<&str> = c.iter().map(|&k| tasks[k].id.as_str()).collect();
                return Err(format!("dependency cycle: {}", names.join(" -> ")));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Report strutturato di complete_task
// ---------------------------------------------------------------------------

/// Campi strutturati opzionali di `complete_task`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportFields {
    pub commit: Option<String>,
    pub files_changed: Vec<String>,
    pub tests: Vec<TestRun>,
    pub follow_ups: Vec<String>,
}

pub fn parse_report_fields(p: &Value) -> Result<ReportFields, String> {
    let nonnull = |k: &str| p.get(k).filter(|v| !v.is_null());
    let commit = match nonnull("commit") {
        None => None,
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else if s.chars().count() > COMMIT_MAX || s.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("invalid commit (a hash or ref, no spaces)".to_string());
            } else {
                Some(s.to_string())
            }
        }
        Some(_) => return Err("commit must be a string".to_string()),
    };
    let files_changed = match nonnull("files_changed") {
        Some(v) => str_list(v, "files_changed", 500, FILE_MAX)?,
        None => Vec::new(),
    };
    let follow_ups = match nonnull("follow_ups") {
        Some(v) => str_list(v, "follow_ups", LIST_MAX, LIST_ITEM_MAX)?,
        None => Vec::new(),
    };
    let tests = match nonnull("tests") {
        None => Vec::new(),
        Some(Value::Array(arr)) => {
            if arr.len() > TESTS_MAX {
                return Err(format!("tests has too many entries (max {TESTS_MAX})"));
            }
            let mut out = Vec::new();
            for t in arr {
                let o: &Map<String, Value> = t.as_object().ok_or_else(|| "tests entries must be objects".to_string())?;
                let command = req_text(
                    "tests[].command",
                    o.get("command").and_then(Value::as_str).unwrap_or(""),
                    TEST_COMMAND_MAX,
                )?;
                let result = o.get("result").and_then(Value::as_str).unwrap_or("");
                if !["passed", "failed", "not_run"].contains(&result) {
                    return Err("tests[].result must be passed, failed or not_run".to_string());
                }
                let details = match o.get("details") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) if s.trim().is_empty() => None,
                    Some(Value::String(s)) => Some(clip_chars(s.trim(), LIST_ITEM_MAX)),
                    Some(_) => return Err("tests[].details must be a string".to_string()),
                };
                out.push(TestRun { command, result: result.to_string(), details });
            }
            out
        }
        Some(_) => return Err("tests must be an array".to_string()),
    };
    Ok(ReportFields { commit, files_changed, tests, follow_ups })
}

pub fn build_report(status: &str, summary: &str, f: ReportFields, diffstat: Option<String>) -> TaskReport {
    TaskReport {
        status: status.to_string(),
        summary: summary.to_string(),
        commit: f.commit,
        files_changed: f.files_changed,
        tests: f.tests,
        follow_ups: f.follow_ups,
        diffstat: diffstat.map(|d| clip_chars(&d, DIFFSTAT_MAX)),
    }
}

// ---------------------------------------------------------------------------
// Board
// ---------------------------------------------------------------------------

impl Task {
    pub fn from_spec(s: TaskSpec, now: i64) -> Task {
        Task {
            id: s.id,
            title: s.title,
            task: s.task,
            depends_on: s.depends_on,
            touches: s.touches,
            isolation: s.isolation,
            base_branch: s.base_branch,
            repo: s.repo,
            agent: s.agent,
            model: s.model,
            permission_mode: s.permission_mode.map(|m| m.as_str().to_string()),
            priority: s.priority,
            held: s.held,
            orchestrator_note: s.orchestrator_note,
            status: TaskStatus::Queued,
            created_at: now,
            updated_at: now,
            ..Task::default()
        }
    }

    fn apply_spec(&mut self, s: TaskSpec, now: i64) {
        self.title = s.title;
        self.task = s.task;
        self.depends_on = s.depends_on;
        self.touches = s.touches;
        self.isolation = s.isolation;
        self.base_branch = s.base_branch;
        self.repo = s.repo;
        self.agent = s.agent;
        self.model = s.model;
        self.permission_mode = s.permission_mode.map(|m| m.as_str().to_string());
        self.priority = s.priority;
        self.held = s.held;
        self.orchestrator_note = s.orchestrator_note;
        self.updated_at = now;
    }

    /// Permesso effettivo (tetto se non richiesto).
    pub fn effective_permission(&self, ceiling: PermissionMode) -> PermissionMode {
        self.permission_mode
            .as_deref()
            .and_then(PermissionMode::parse)
            .filter(|m| m.rank() <= ceiling.rank())
            .unwrap_or(ceiling)
    }
}

impl Board {
    pub fn new(workspace_id: &str) -> Board {
        Board { workspace_id: workspace_id.to_string(), ..Board::default() }
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Tutte le dipendenze esistono e sono `done` (una `cancelled` non basta).
    pub fn deps_done(&self, t: &Task) -> bool {
        t.depends_on
            .iter()
            .all(|d| self.task(d).is_some_and(|x| x.status == TaskStatus::Done))
    }

    pub fn column(&self, t: &Task) -> Column {
        match t.status {
            TaskStatus::Queued if self.deps_done(t) => Column::Ready,
            TaskStatus::Queued => Column::Backlog,
            TaskStatus::Running => Column::Running,
            TaskStatus::Review => Column::Review,
            TaskStatus::Done => Column::Done,
            TaskStatus::Failed => Column::Failed,
            TaskStatus::Blocked => Column::Blocked,
            TaskStatus::Cancelled => Column::Cancelled,
        }
    }

    pub fn running_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Running).count()
    }

    /// Task `running` il cui worker non e' ancora stato registrato.
    pub fn pending_starts(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Running && t.session.is_none())
            .count()
    }

    /// Almeno un task e tutti `done` o `cancelled`.
    pub fn is_complete(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .iter()
                .all(|t| matches!(t.status, TaskStatus::Done | TaskStatus::Cancelled))
    }

    /// Testo di `plan_completed` con i conteggi, o `None` se non c'e' nulla
    /// da riferire: piano non completo, oppure tutti i task annullati senza
    /// mai essere partiti (l'orchestratore li ha annullati lui stesso).
    pub fn completion_text(&self) -> Option<String> {
        if !self.is_complete() {
            return None;
        }
        let count = |f: &dyn Fn(&Task) -> bool| self.tasks.iter().filter(|t| f(t)).count();
        let cancelled = count(&|t| t.status == TaskStatus::Cancelled);
        if cancelled == self.tasks.len() && self.tasks.iter().all(|t| t.attempts == 0) {
            return None;
        }
        let done = count(&|t| t.status == TaskStatus::Done);
        let external = count(&|t| t.status == TaskStatus::Done && t.external);
        let failed = count(&|t| t.status == TaskStatus::Failed);
        Some(format!(
            "Plan completed: {done} done (of which {external} completed outside the board), {failed} failed, {cancelled} cancelled."
        ))
    }

    pub fn view(&self) -> BoardView {
        BoardView {
            workspace_id: self.workspace_id.clone(),
            rev: self.rev,
            max_parallel: self.max_parallel,
            paused: self.paused,
            base_branch: self.base_branch.clone(),
            base_branches: self.base_branches.clone(),
            integration_branch: self.integration_branch.clone(),
            integrations: self.integrations.clone(),
            repos: self.repos.clone(),
            owner: self.owner.clone(),
            updated_at: self.updated_at,
            tasks: self
                .tasks
                .iter()
                .map(|t| TaskView { task: t.clone(), column: self.column(t) })
                .collect(),
            automation: self.automation.clone(),
            orchestrator: self.orchestrator.clone(),
        }
    }

    pub fn task_view(&self, id: &str) -> Option<TaskView> {
        self.task(id).map(|t| TaskView { task: t.clone(), column: self.column(t) })
    }

    /// Carica una board dal file: i task `running` diventano `failed` (i tab
    /// automatici non si ripristinano) e il proprietario si azzera.
    pub fn from_json(raw: &str, now: i64) -> Result<Board, String> {
        let mut b: Board = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if b.workspace_id.is_empty() {
            return Err("workspace_id missing".to_string());
        }
        b.max_parallel = b.max_parallel.clamp(1, MAX_PARALLEL_LIMIT);
        b.owner = None;
        for t in &mut b.tasks {
            if t.status == TaskStatus::Running {
                t.status = TaskStatus::Failed;
                t.note = Some(INTERRUPTED_NOTE.to_string());
                t.finished_at = Some(now);
                t.updated_at = now;
            }
            t.session = None;
            t.worker_status = None;
            if t.status == TaskStatus::Done {
                t.held = false;
            }
        }
        b.plan_completed_sent = b.is_complete();
        Ok(b)
    }

    /// Aggiunge i task di un piano in modo atomico: se qualcosa non va la
    /// board resta com'era.
    pub fn add_plan(&mut self, specs: Vec<TaskSpec>, ceiling: PermissionMode, now: i64) -> Result<Vec<String>, String> {
        if self.tasks.len() + specs.len() > MAX_TASKS_PER_BOARD {
            return Err(format!("too many tasks on the board (max {MAX_TASKS_PER_BOARD})"));
        }
        check_permissions(&specs, ceiling)?;
        let mut seen: HashSet<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        for s in &specs {
            if !seen.insert(s.id.as_str()) {
                return Err(format!("duplicate task id: {}", s.id));
            }
        }
        let ids: Vec<String> = specs.iter().map(|s| s.id.clone()).collect();
        let mut all = self.tasks.clone();
        all.extend(specs.into_iter().map(|s| Task::from_spec(s, now)));
        validate_graph(&all)?;
        self.tasks = all;
        self.updated_at = now;
        Ok(ids)
    }

    /// Applica `base_branches` e `integration_branch`. Cambiare il modello
    /// del branch di integrazione scarta le integrazioni gia' risolte (se ne
    /// crea una nuova al prossimo avvio); la cache dei repo si invalida.
    pub fn apply_repo_settings(&mut self, base_branches: BaseBranchOps, integration: Option<Option<String>>) {
        for (k, v) in base_branches {
            match v {
                Some(b) => {
                    self.base_branches.insert(k, b);
                }
                None => {
                    self.base_branches.remove(&k);
                }
            }
        }
        if let Some(tpl) = integration {
            if tpl != self.integration_branch {
                self.integrations.clear();
            }
            self.integration_branch = tpl;
        }
        self.repos = None;
    }

    /// Modifica parziale di un task (`patch` = campi di TaskSpec).
    pub fn update_task(&mut self, id: &str, patch: &Value, ceiling: PermissionMode, now: i64) -> Result<(), String> {
        let t = self.task(id).ok_or_else(|| format!("task not found: {id}"))?;
        if !matches!(
            t.status,
            TaskStatus::Queued | TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled
        ) {
            return Err(format!(
                "task '{id}' is {}: only queued, failed, blocked or cancelled tasks can be updated",
                t.status.as_str()
            ));
        }
        let spec = parse_spec(patch, Some(&TaskSpec::from_task(t)))?;
        if patch.as_object().is_some_and(|o| o.keys().all(|k| k == "id")) {
            return Err("no fields to update".to_string());
        }
        if spec.isolation != t.isolation && t.worktree.is_some() {
            return Err(format!(
                "task '{id}' already has a worktree: its isolation cannot be changed"
            ));
        }
        if spec.repo != t.repo && t.worktree.is_some() {
            return Err(format!("task '{id}' already has a worktree: its repo cannot be changed"));
        }
        check_permissions(std::slice::from_ref(&spec), ceiling)?;
        let mut all = self.tasks.clone();
        if let Some(slot) = all.iter_mut().find(|x| x.id == id) {
            slot.apply_spec(spec, now);
        }
        validate_graph(&all)?;
        self.tasks = all;
        self.updated_at = now;
        Ok(())
    }

    /// Cambio di stato deciso dall'orchestratore o dall'utente: verifica e
    /// applica i campi (la chiusura del tab e la pulizia sono di chi chiama).
    pub fn check_transition(&self, id: &str, to: TaskStatus) -> Result<(), String> {
        let t = self.task(id).ok_or_else(|| format!("task not found: {id}"))?;
        let from = t.status;
        let ok = match to {
            TaskStatus::Done => matches!(
                from,
                TaskStatus::Queued | TaskStatus::Review | TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled
            ),
            TaskStatus::Cancelled => !matches!(from, TaskStatus::Done | TaskStatus::Cancelled),
            TaskStatus::Queued => matches!(
                from,
                TaskStatus::Failed | TaskStatus::Blocked | TaskStatus::Cancelled | TaskStatus::Review
            ),
            _ => false,
        };
        if ok {
            return Ok(());
        }
        Err(match (to, from) {
            (TaskStatus::Done, TaskStatus::Running) => {
                format!("task '{id}' is running: wait for it to finish (or cancel it) before marking it done")
            }
            (TaskStatus::Done, _) => format!(
                "task '{id}' is {}: only queued, review, failed, blocked or cancelled tasks can be marked done",
                from.as_str()
            ),
            (TaskStatus::Cancelled, _) => format!("task '{id}' is already {}", from.as_str()),
            (TaskStatus::Queued, _) => format!(
                "task '{id}' is {}: only failed, blocked, cancelled or review tasks can be retried",
                from.as_str()
            ),
            _ => "unsupported status change".to_string(),
        })
    }

    /// `deleted`: solo un task in coda o annullato che non e' mai partito, senza
    /// worktree ne' sessione e che nessun altro task elenca in `depends_on`.
    pub fn check_delete(&self, id: &str) -> Result<(), String> {
        let t = self.task(id).ok_or_else(|| format!("task not found: {id}"))?;
        if !matches!(t.status, TaskStatus::Queued | TaskStatus::Cancelled) {
            return Err(format!(
                "task '{id}' is {}: only queued or cancelled tasks that never started can be deleted",
                t.status.as_str()
            ));
        }
        if t.attempts > 0 || t.worktree.is_some() || t.session.is_some() {
            return Err(format!(
                "task '{id}' was already started (attempts: {}{}): only tasks that never started can be deleted; cancel it instead",
                t.attempts,
                if t.worktree.is_some() { ", has a worktree" } else { "" }
            ));
        }
        let dependents: Vec<&str> =
            self.tasks.iter().filter(|x| x.depends_on.iter().any(|d| d == id)).map(|x| x.id.as_str()).collect();
        if !dependents.is_empty() {
            return Err(format!(
                "task '{id}' cannot be deleted: it is a dependency of {}. Remove it from their depends_on (update_task) or delete them first",
                dependents.join(", ")
            ));
        }
        Ok(())
    }

    /// Toglie il task dalla board (dopo `check_delete`).
    pub fn delete_task(&mut self, id: &str, now: i64) -> Result<(), String> {
        self.check_delete(id)?;
        self.tasks.retain(|t| t.id != id);
        self.updated_at = now;
        Ok(())
    }

    pub fn apply_status(&mut self, id: &str, to: TaskStatus, note: Option<String>, now: i64) -> Result<(), String> {
        self.check_transition(id, to)?;
        let note = note.map(|n| clip_chars(n.trim(), NOTE_MAX)).filter(|n| !n.is_empty());
        let t = self.task_mut(id).ok_or_else(|| format!("task not found: {id}"))?;
        // Completato fuori dalla board: serve una nota che dica come.
        let external = to == TaskStatus::Done && matches!(t.status, TaskStatus::Queued | TaskStatus::Cancelled);
        if external && note.is_none() {
            return Err(format!(
                "task '{id}' is {}: to mark it done pass a note saying how it was completed outside the board",
                t.status.as_str()
            ));
        }
        t.external = external;
        t.status = to;
        t.session = None;
        t.worker_status = None;
        t.preparing = None;
        t.preparing_since = None;
        t.updated_at = now;
        match to {
            TaskStatus::Queued => {
                t.report = None;
                // La nota precedente resta finche' non ne arriva una nuova.
                if note.is_some() {
                    t.note = note;
                }
                t.finished_at = None;
                t.last_report = None;
            }
            _ => {
                t.finished_at = Some(now);
                // Un task concluso non e' piu' trattenuto (un annullato si': il retry lo ritrova).
                if to == TaskStatus::Done {
                    t.held = false;
                }
                if note.is_some() {
                    t.note = note;
                }
            }
        }
        self.updated_at = now;
        Ok(())
    }

    /// `hold` / `unhold`: solo i task in coda si trattengono.
    pub fn set_held(&mut self, id: &str, held: bool, now: i64) -> Result<(), String> {
        let t = self.task_mut(id).ok_or_else(|| format!("task not found: {id}"))?;
        if held && t.status != TaskStatus::Queued {
            return Err(format!(
                "task '{id}' is {}: only queued tasks can be held",
                t.status.as_str()
            ));
        }
        t.held = held;
        t.updated_at = now;
        self.updated_at = now;
        Ok(())
    }

    /// Segna `running` un task scelto dallo scheduler.
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
    /// chiuso). Se `session` e' dato deve essere quella del task. `false` se
    /// il task non era piu' `running` per quel worker.
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
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Contesto dello scheduler (lo fornisce chi chiama: il nucleo non conosce
/// i workspace).
#[derive(Debug, Clone, Copy)]
pub struct SchedCtx {
    /// Confronto delle aree senza maiuscole (workspace Windows).
    pub case_insensitive: bool,
    /// Tetto dei permessi del workspace (decide cosa e' read-only).
    pub ceiling: PermissionMode,
    /// Quanti task si possono avviare al massimo (tetto di 16 worker attivi
    /// per orchestratore, meno quelli gia' attivi).
    pub max_start: usize,
}

/// Task `none` che puo' scrivere nella root condivisa.
fn writes_shared_root(t: &Task, ceiling: PermissionMode) -> bool {
    t.isolation == Isolation::None && t.effective_permission(ceiling) != PermissionMode::Plan
}

/// Id dei task da avviare ora, in ordine di scelta. Funzione pura.
pub fn schedule(board: &Board, ctx: &SchedCtx) -> Vec<String> {
    if board.paused {
        return Vec::new();
    }
    let running: Vec<&Task> = board.tasks.iter().filter(|t| t.status == TaskStatus::Running).collect();
    let slots = (board.max_parallel as usize).saturating_sub(running.len()).min(ctx.max_start);
    if slots == 0 {
        return Vec::new();
    }
    let mut cands: Vec<(usize, &Task)> = board
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.status == TaskStatus::Queued && !t.held && board.deps_done(t))
        .collect();
    cands.sort_by(|(ia, a), (ib, b)| {
        b.priority
            .cmp(&a.priority)
            .then(a.created_at.cmp(&b.created_at))
            .then(ia.cmp(ib))
    });
    let mut chosen: Vec<&Task> = Vec::new();
    // Un solo scrittore condiviso per repository (None = la root).
    let mut shared_writers: Vec<Option<&str>> = running
        .iter()
        .filter(|t| writes_shared_root(t, ctx.ceiling))
        .map(|t| t.repo.as_deref())
        .collect();
    for (_, c) in cands {
        if chosen.len() >= slots {
            break;
        }
        let clash = running
            .iter()
            .chain(chosen.iter())
            .any(|o| o.repo == c.repo && touches_conflict(&c.touches, &o.touches, ctx.case_insensitive));
        if clash {
            continue;
        }
        if writes_shared_root(c, ctx.ceiling) {
            if shared_writers.contains(&c.repo.as_deref()) {
                continue;
            }
            shared_writers.push(c.repo.as_deref());
        }
        chosen.push(c);
    }
    chosen.into_iter().map(|t| t.id.clone()).collect()
}

// ---------------------------------------------------------------------------
// Ambiente del worker
// ---------------------------------------------------------------------------

pub struct EnvInfo<'a> {
    pub root: &'a str,
    pub workdir: &'a str,
    pub task: &'a Task,
    /// Comando di setup del workspace (`worktree_setup`).
    pub setup: Option<&'a str>,
    /// Configurazione del repository del task (setup eseguito dall'app).
    pub repo_cfg: Option<&'a crate::workspaces::RepoConfig>,
}

/// Testo `environment` di `get_task` per un task di board (lo legge il
/// modello: inglese).
pub fn worker_environment(e: &EnvInfo) -> String {
    let t = e.task;
    let mut s = String::new();
    s.push_str("Environment of this task\n");
    s.push_str(&format!("- Workspace root: {}\n", e.root));
    s.push_str(&format!("- Your working folder: {}\n", e.workdir));
    match (&t.worktree, t.isolation) {
        (Some(w), Isolation::Worktree) => s.push_str(&format!(
            "- Git worktree: branch {} (created from {}). Work and commit here, on this branch.\n",
            w.branch, w.base
        )),
        _ if t.repo.is_some() => s.push_str(
            "- No dedicated worktree: you work directly in the repository folder, on the branch that is checked out there.\n",
        ),
        _ => s.push_str("- No dedicated worktree: you work directly in the workspace root, on the branch that is checked out there.\n"),
    }
    if let Some(r) = &t.repo {
        s.push_str(&format!(
            "- Repository: {r} (a folder of the workspace root; your working folder belongs to it and the declared areas are relative to it)\n"
        ));
    }
    s.push_str("\nFirst step\n");
    if let (Some(cfg), Some(w), Isolation::Worktree) = (e.repo_cfg.filter(|c| c.has_setup()), &t.worktree, t.isolation) {
        if w.setup_done {
            s.push_str("- The app already prepared this worktree: do NOT redo it.");
            if !cfg.copy_ignored.is_empty() {
                s.push_str(&format!(" Git-ignored files copied from the repository: {}.", cfg.copy_ignored.join(", ")));
            }
            if !cfg.setup.is_empty() {
                s.push_str(&format!(" Setup commands already run: {}.", cfg.setup.join("; ")));
            }
            s.push('\n');
            if let Some(c) = cfg.container_root.as_deref().filter(|c| !c.is_empty()) {
                s.push_str(&format!(
                    "- The repository is mounted in its container at {c}: this worktree is visible there at the same relative path.\n"
                ));
            }
        }
    }
    match e.setup.map(str::trim).filter(|c| !c.is_empty()) {
        Some(cmd) => s.push_str(&format!(
            "- Before anything else, run this setup command in your working folder and wait for it to finish: {cmd}\n"
        )),
        None => s.push_str("- No setup command is configured for this workspace: start working right away.\n"),
    }
    if !t.touches.is_empty() {
        let rel = if t.repo.is_some() { "the repository" } else { "the workspace root" };
        s.push_str(&format!("\nDeclared areas of this task (paths relative to {rel}): {}\n", t.touches.join(", ")));
    }
    s.push_str("\nRules\n");
    s.push_str("- Work only inside your working folder; do not modify files outside it.\n");
    match (&t.worktree, t.isolation) {
        (Some(w), Isolation::Worktree) => s.push_str(&format!(
            "- Commit your work on the branch {}. Do NOT merge, push, rebase or switch branches: the orchestrator integrates your branch.\n",
            w.branch
        )),
        _ => {
            s.push_str("- Do not create branches, and do NOT merge, push or switch branches.\n");
            if t.touches.is_empty() {
                s.push_str("- Keep your changes minimal and limited to what the task needs.\n");
            } else {
                s.push_str("- Do not touch areas outside the declared ones: other tasks may be working on them at the same time.\n");
            }
        }
    }
    s.push_str(
        "- Finish with complete_task and fill the structured fields: status, summary, commit (hash of your last commit, if any), \
         files_changed, tests (command, result passed/failed/not_run, details) and follow_ups (anything left to do).\n",
    );
    s
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(id: &str) -> TaskSpec {
        parse_spec(&json!({"id": id, "task": "do it"}), None).unwrap()
    }

    fn spec_with(v: Value) -> TaskSpec {
        parse_spec(&v, None).unwrap()
    }

    fn board_with(specs: Vec<TaskSpec>) -> Board {
        let mut b = Board::new("ws-1");
        b.max_parallel = 16;
        b.add_plan(specs, PermissionMode::Bypass, 10).unwrap();
        b
    }

    fn ctx() -> SchedCtx {
        SchedCtx { case_insensitive: false, ceiling: PermissionMode::AcceptEdits, max_start: 16 }
    }

    fn set_status(b: &mut Board, id: &str, s: TaskStatus) {
        b.task_mut(id).unwrap().status = s;
    }

    #[test]
    fn id_rules() {
        for ok in ["a", "0", "auth-api", "a-", &"a".repeat(40)] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "-a", "A", "a_b", "a b", "a.b", &"a".repeat(41), "è"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn spec_defaults_and_normalization() {
        let s = spec_with(json!({"id": "a", "task": "  fai una cosa  ", "touches": ["./src\\auth/", "src/auth", " lib "]}));
        assert_eq!(s.task, "fai una cosa");
        assert_eq!(s.title, "fai una cosa");
        assert_eq!(s.isolation, Isolation::Worktree);
        assert_eq!(s.agent, "claude-code");
        assert_eq!(s.priority, 0);
        assert!(!s.held && s.depends_on.is_empty());
        // Normalizzate e senza doppioni.
        assert_eq!(s.touches, vec!["src/auth", "lib"]);
        let full = spec_with(json!({
            "id": "b", "title": " T ", "task": "x", "depends_on": ["a", "a"], "isolation": "none",
            "base_branch": "release/1.0", "agent": "claude-code", "model": "sonnet",
            "permission_mode": "plan", "priority": -3, "held": true
        }));
        assert_eq!(full.depends_on, vec!["a"]);
        assert_eq!(full.title, "T");
        assert_eq!(full.isolation, Isolation::None);
        assert_eq!(full.permission_mode, Some(PermissionMode::Plan));
        assert_eq!(full.priority, -3);
        assert!(full.held);
        assert_eq!(full.base_branch.as_deref(), Some("release/1.0"));
    }

    #[test]
    fn spec_validation_errors() {
        let bad = |v: Value| parse_spec(&v, None).unwrap_err();
        assert!(bad(json!({"task": "x"})).contains("id is missing"));
        assert!(bad(json!({"id": "A", "task": "x"})).contains("invalid id"));
        assert!(bad(json!({"id": "a"})).contains("task is missing"));
        assert!(bad(json!({"id": "a", "task": "  "})).contains("task is missing"));
        assert!(bad(json!({"id": "a", "task": "x".repeat(TASK_MAX + 1)})).contains("too long"));
        assert!(bad(json!({"id": "a", "task": "x", "title": "t".repeat(81)})).contains("title is too long"));
        assert!(bad(json!({"id": "a", "task": "x", "dependencies": []})).contains("unknown field 'dependencies'"));
        assert!(bad(json!({"id": "a", "task": "x", "depends_on": ["B"]})).contains("invalid task id"));
        assert!(bad(json!({"id": "a", "task": "x", "depends_on": "b"})).contains("array"));
        assert!(bad(json!({"id": "a", "task": "x", "isolation": "docker"})).contains("isolation"));
        assert!(bad(json!({"id": "a", "task": "x", "permission_mode": "yolo"})).contains("permission_mode"));
        assert!(bad(json!({"id": "a", "task": "x", "model": "a b"})).contains("model"));
        assert!(bad(json!({"id": "a", "task": "x", "priority": 1.5})).contains("priority"));
        assert!(bad(json!({"id": "a", "task": "x", "held": "yes"})).contains("held"));
        assert!(bad(json!({"id": "a", "task": "x", "base_branch": "--upload-pack=x"})).contains("base_branch"));
        assert!(bad(json!({"id": "a", "task": "x", "base_branch": "a..b"})).contains("base_branch"));
        assert!(bad(json!("nope")).contains("object"));
        let many: Vec<String> = (0..=MAX_TOUCHES).map(|i| format!("d{i}")).collect();
        assert!(bad(json!({"id": "a", "task": "x", "touches": many})).contains("too many"));
        let deps: Vec<String> = (0..=MAX_DEPENDS).map(|i| format!("d{i}")).collect();
        assert!(bad(json!({"id": "a", "task": "x", "depends_on": deps})).contains("too many"));
    }

    #[test]
    fn touches_normalization_and_validation() {
        assert_eq!(normalize_area(" ./src\\auth/ ").unwrap(), "src/auth");
        assert_eq!(normalize_area("src//a/./b/").unwrap(), "src/a/b");
        assert_eq!(normalize_area("./").unwrap(), ".");
        assert_eq!(normalize_area(".").unwrap(), ".");
        for bad in ["", "  ", "../x", "a/../b", "/abs", "C:\\x", "c:/x", "a\nb", &"a".repeat(257)] {
            assert!(normalize_area(bad).is_err(), "{bad:?}");
        }
        assert!(normalize_area(&"a".repeat(256)).is_ok());
    }

    #[test]
    fn overlap_is_by_whole_segments() {
        assert!(areas_overlap("src", "src", false));
        assert!(areas_overlap("src", "src/auth", false));
        assert!(areas_overlap("src/auth/login", "src", false));
        assert!(!areas_overlap("src", "src2", false));
        assert!(!areas_overlap("src/auth", "src/authz", false));
        assert!(!areas_overlap("src/a", "src/b", false));
        // La root contiene tutto.
        assert!(areas_overlap(".", "anything/deep", false));
        // Maiuscole e separatori.
        assert!(!areas_overlap("Src/Auth", "src/auth", false));
        assert!(areas_overlap("Src/Auth", "src/auth/x", true));
        assert!(areas_overlap("src\\Auth", "SRC/auth", true));
        assert!(!areas_overlap("src", "srcx", true));
        // Nessuna area dichiarata = nessun conflitto.
        assert!(!touches_conflict(&[], &["src".to_string()], false));
        assert!(!touches_conflict(&[], &[], false));
        assert!(touches_conflict(&["a".to_string(), "src".to_string()], &["src/x".to_string()], false));
    }

    #[test]
    fn plan_parsing_limits() {
        assert!(parse_plan(&json!({})).unwrap_err().contains("tasks is missing"));
        assert!(parse_plan(&json!({"tasks": []})).unwrap_err().contains("empty"));
        assert!(parse_plan(&json!({"tasks": [{"id": "a", "task": "x"}], "extra": 1})).is_err());
        let err = parse_plan(&json!({"tasks": [{"id": "a", "task": "x"}, {"id": "B", "task": "x"}]})).unwrap_err();
        assert!(err.starts_with("tasks[1]:"), "{err}");
        let many: Vec<Value> = (0..=MAX_TASKS_PER_PLAN).map(|i| json!({"id": format!("t{i}"), "task": "x"})).collect();
        assert!(parse_plan(&json!({"tasks": many})).unwrap_err().contains("too many"));
        let ok = parse_plan(&json!({"tasks": [{"id": "a", "task": "x"}], "max_parallel": 5, "base_branch": "develop"})).unwrap();
        assert_eq!((ok.max_parallel, ok.base_branch.as_deref()), (Some(5), Some("develop")));
        for bad in [json!(0), json!(17), json!("3"), json!(2.5), json!(-1)] {
            assert!(parse_plan(&json!({"tasks": [{"id": "a", "task": "x"}], "max_parallel": bad})).is_err(), "{bad}");
        }
        assert_eq!(parse_max_parallel(&json!(16)), Ok(16));
        assert_eq!(parse_max_parallel(&json!(1)), Ok(1));
    }

    #[test]
    fn add_plan_validates_atomically() {
        let mut b = Board::new("ws-1");
        let a = spec("a");
        let mut bb = spec("b");
        bb.depends_on = vec!["a".to_string()];
        assert_eq!(b.add_plan(vec![a.clone(), bb.clone()], PermissionMode::Bypass, 5).unwrap(), vec!["a", "b"]);
        assert_eq!(b.tasks.len(), 2);
        assert_eq!(b.task("a").unwrap().created_at, 5);

        // Duplicato (anche con un task gia' presente): nessuna modifica.
        let err = b.add_plan(vec![spec("c"), a.clone()], PermissionMode::Bypass, 6).unwrap_err();
        assert!(err.contains("duplicate task id: a"), "{err}");
        let err = b.add_plan(vec![spec("c"), spec("c")], PermissionMode::Bypass, 6).unwrap_err();
        assert!(err.contains("duplicate"), "{err}");
        // Dipendenza inesistente.
        let mut d = spec("d");
        d.depends_on = vec!["zzz".to_string()];
        let err = b.add_plan(vec![d], PermissionMode::Bypass, 6).unwrap_err();
        assert!(err.contains("unknown task 'zzz'"), "{err}");
        // Dipendenza su un task della stessa chiamata o gia' presente.
        let mut e = spec("e");
        e.depends_on = vec!["f".to_string(), "a".to_string()];
        assert!(b.add_plan(vec![e, spec("f")], PermissionMode::Bypass, 6).is_ok());
        assert_eq!(b.tasks.len(), 4);
        assert_eq!(b.tasks.len(), 4);
    }

    #[test]
    fn cycles_and_self_dependencies_are_rejected() {
        let mut b = Board::new("ws-1");
        let dep = |id: &str, on: &[&str]| {
            let mut s = spec(id);
            s.depends_on = on.iter().map(|x| x.to_string()).collect();
            s
        };
        let err = b.add_plan(vec![dep("a", &["a"])], PermissionMode::Bypass, 1).unwrap_err();
        assert!(err.contains("cannot depend on itself"), "{err}");
        let err = b.add_plan(vec![dep("a", &["b"]), dep("b", &["a"])], PermissionMode::Bypass, 1).unwrap_err();
        assert!(err.contains("dependency cycle") && err.contains("a -> b -> a"), "{err}");
        let err = b
            .add_plan(vec![dep("x", &[]), dep("a", &["b"]), dep("b", &["c"]), dep("c", &["a", "x"])], PermissionMode::Bypass, 1)
            .unwrap_err();
        assert!(err.contains("dependency cycle"), "{err}");
        assert!(b.tasks.is_empty());
        // Un diamante non e' un ciclo.
        assert!(b
            .add_plan(
                vec![dep("a", &[]), dep("b", &["a"]), dep("c", &["a"]), dep("d", &["b", "c"])],
                PermissionMode::Bypass,
                1
            )
            .is_ok());
    }

    #[test]
    fn plan_limits_on_the_board() {
        let mut b = Board::new("ws-1");
        for chunk in 0..4 {
            let specs: Vec<TaskSpec> = (0..50).map(|i| spec(&format!("t{chunk}-{i}"))).collect();
            b.add_plan(specs, PermissionMode::Bypass, 1).unwrap();
        }
        assert_eq!(b.tasks.len(), MAX_TASKS_PER_BOARD);
        assert!(b.add_plan(vec![spec("one-more")], PermissionMode::Bypass, 1).unwrap_err().contains("too many tasks"));
    }

    #[test]
    fn permissions_must_stay_under_the_ceiling() {
        let mut b = Board::new("ws-1");
        let mut s = spec("a");
        s.permission_mode = Some(PermissionMode::Bypass);
        let err = b.add_plan(vec![s.clone()], PermissionMode::AcceptEdits, 1).unwrap_err();
        assert!(err.contains("task 'a'") && err.contains("is not allowed"), "{err}");
        s.permission_mode = Some(PermissionMode::Plan);
        assert!(b.add_plan(vec![s], PermissionMode::AcceptEdits, 1).is_ok());
    }

    #[test]
    fn column_derivation() {
        let mut dep_b = spec("b");
        dep_b.depends_on = vec!["a".to_string()];
        let mut b = board_with(vec![spec("a"), dep_b]);
        let col = |b: &Board, id: &str| b.column(b.task(id).unwrap());
        assert_eq!(col(&b, "a"), Column::Ready);
        assert_eq!(col(&b, "b"), Column::Backlog);
        for (s, c) in [
            (TaskStatus::Running, Column::Running),
            (TaskStatus::Review, Column::Review),
            (TaskStatus::Failed, Column::Failed),
            (TaskStatus::Blocked, Column::Blocked),
            (TaskStatus::Cancelled, Column::Cancelled),
        ] {
            set_status(&mut b, "a", s);
            assert_eq!(col(&b, "a"), c);
            // Solo `done` sblocca il dipendente.
            assert_eq!(col(&b, "b"), Column::Backlog, "{s:?}");
        }
        set_status(&mut b, "a", TaskStatus::Done);
        assert_eq!(col(&b, "a"), Column::Done);
        assert_eq!(col(&b, "b"), Column::Ready);
        // Held non e' una colonna.
        b.task_mut("b").unwrap().held = true;
        assert_eq!(col(&b, "b"), Column::Ready);
    }

    #[test]
    fn board_view_serializes_with_column_and_optional_text() {
        let mut b = board_with(vec![spec("a")]);
        b.owner = Some("tab-1".to_string());
        let v = b.view();
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["workspace_id"], "ws-1");
        assert_eq!(j["max_parallel"], 16);
        assert_eq!(j["paused"], false);
        assert_eq!(j["owner"], "tab-1");
        let t = &j["tasks"][0];
        assert_eq!(t["id"], "a");
        assert_eq!(t["column"], "ready");
        assert_eq!(t["status"], "queued");
        assert_eq!(t["isolation"], "worktree");
        assert_eq!(t["task"], "do it");
        assert!(t["worker_status"].is_null() && t["session"].is_null() && t["report"].is_null());
        assert_eq!(t["attempts"], 0);
        assert!(v.tasks[0].to_json(false).get("task").is_none());
        assert_eq!(v.tasks[0].to_json(true)["column"], "ready");
    }

    #[test]
    fn compact_json_rows_counts_and_columns() {
        let mut dep = spec("b");
        dep.depends_on = vec!["a".to_string()];
        let mut m = spec("m");
        m.priority = 3;
        m.model = Some("sonnet".to_string());
        let mut b = board_with(vec![spec("a"), dep, m, spec("d"), spec("c")]);
        set_status(&mut b, "d", TaskStatus::Done);
        set_status(&mut b, "c", TaskStatus::Cancelled);
        b.task_mut("a").unwrap().held = true;
        let j = b.view().to_compact_json(None);
        assert_eq!(j["counts"], json!({"backlog": 1, "ready": 2, "done": 1, "cancelled": 1}));
        let ids: Vec<&str> = j["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["a", "b", "m"]);
        let row = |id: &str| j["tasks"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap().clone();
        assert_eq!(row("a")["held"], true);
        assert!(row("a").get("waiting_on").is_none() && row("a").get("priority").is_none());
        assert_eq!(row("b")["waiting_on"], json!(["a"]));
        assert_eq!((row("m")["priority"].clone(), row("m")["model"].clone()), (json!(3), json!("sonnet")));
        for k in ["task", "worktree", "touches", "created_at", "report", "session", "status"] {
            assert!(row("m").get(k).is_none(), "{k}");
        }
        // Con `columns` anche done, in ordine di board.
        let j = b.view().to_compact_json(Some(&[Column::Done, Column::Backlog]));
        let ids: Vec<&str> = j["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["b", "d"]);
    }

    #[test]
    fn compact_json_truncates_note_and_summary_on_char_boundary() {
        let mut b = board_with(vec![spec("a")]);
        let t = b.task_mut("a").unwrap();
        t.note = Some("è".repeat(250));
        t.report = Some(TaskReport {
            status: "completed".to_string(),
            summary: "世".repeat(301),
            commit: None,
            files_changed: Vec::new(),
            tests: Vec::new(),
            follow_ups: Vec::new(),
            diffstat: None,
        });
        let j = b.view().to_compact_json(None);
        let note = j["tasks"][0]["note"].as_str().unwrap();
        assert_eq!(note.chars().count(), 201);
        assert!(note.ends_with('…') && note.starts_with("èè"));
        let sum = j["tasks"][0]["summary"].as_str().unwrap();
        assert_eq!(sum.chars().count(), 301);
        assert!(sum.ends_with('…'));
        // Sotto il limite resta intatto.
        b.task_mut("a").unwrap().note = Some("breve".to_string());
        assert_eq!(b.view().to_compact_json(None)["tasks"][0]["note"], "breve");
    }

    #[test]
    fn board_roundtrips_and_restart_normalizes_running_tasks() {
        let mut b = board_with(vec![spec("a"), spec("b"), spec("c")]);
        b.owner = Some("tab-9".to_string());
        b.paused = true;
        b.start_task("a", 20);
        b.task_mut("a").unwrap().session = Some("auto-1".to_string());
        set_status(&mut b, "b", TaskStatus::Review);
        let raw = serde_json::to_string(&b).unwrap();
        let back = Board::from_json(&raw, 99).unwrap();
        assert_eq!(back.owner, None);
        assert!(back.paused);
        let a = back.task("a").unwrap();
        assert_eq!(a.status, TaskStatus::Failed);
        assert_eq!(a.note.as_deref(), Some("interrupted: the app was restarted"));
        assert!(a.session.is_none() && a.worker_status.is_none());
        assert_eq!((a.finished_at, a.attempts), (Some(99), 1));
        assert_eq!(back.task("b").unwrap().status, TaskStatus::Review);
        assert_eq!(back.task("c").unwrap().status, TaskStatus::Queued);
        // File corrotto o senza workspace.
        assert!(Board::from_json("{non json", 1).is_err());
        assert!(Board::from_json("{}", 1).is_err());
        // Campi mancanti: default.
        let min = Board::from_json(r#"{"workspace_id":"w","tasks":[{"id":"x","task":"t"}]}"#, 1).unwrap();
        assert_eq!(min.max_parallel, DEFAULT_MAX_PARALLEL);
        assert_eq!(min.tasks[0].status, TaskStatus::Queued);
        // Una board gia' completa non rilancia plan_completed.
        let mut done = Board::new("w");
        done.add_plan(vec![spec("a")], PermissionMode::Bypass, 1).unwrap();
        set_status(&mut done, "a", TaskStatus::Done);
        let loaded = Board::from_json(&serde_json::to_string(&done).unwrap(), 2).unwrap();
        assert!(loaded.plan_completed_sent);
    }

    #[test]
    fn update_task_rules() {
        let mut b = board_with(vec![spec("a"), spec("b")]);
        b.update_task("a", &json!({"title": "Nuovo", "priority": 5, "touches": ["src/x/"]}), PermissionMode::Bypass, 30)
            .unwrap();
        let a = b.task("a").unwrap();
        assert_eq!((a.title.as_str(), a.priority, a.touches.clone()), ("Nuovo", 5, vec!["src/x".to_string()]));
        assert_eq!(a.updated_at, 30);
        assert_eq!(a.task, "do it", "i campi assenti non cambiano");
        // Dipendenze rivalidate.
        b.update_task("b", &json!({"depends_on": ["a"]}), PermissionMode::Bypass, 31).unwrap();
        assert!(b.update_task("a", &json!({"depends_on": ["b"]}), PermissionMode::Bypass, 32).unwrap_err().contains("cycle"));
        assert!(b.update_task("a", &json!({"depends_on": ["zz"]}), PermissionMode::Bypass, 32).unwrap_err().contains("unknown"));
        assert!(b.task("a").unwrap().depends_on.is_empty(), "nessuna modifica parziale");
        // null azzera i campi opzionali.
        b.update_task("a", &json!({"model": "opus"}), PermissionMode::Bypass, 33).unwrap();
        b.update_task("a", &json!({"model": null, "depends_on": null}), PermissionMode::Bypass, 34).unwrap();
        assert!(b.task("a").unwrap().model.is_none());
        // id non modificabile, campi vuoti, task inesistente, permessi.
        assert!(b.update_task("a", &json!({"id": "z"}), PermissionMode::Bypass, 1).unwrap_err().contains("cannot be changed"));
        assert!(b.update_task("a", &json!({"id": "a"}), PermissionMode::Bypass, 1).unwrap_err().contains("no fields"));
        assert!(b.update_task("nope", &json!({"title": "x"}), PermissionMode::Bypass, 1).unwrap_err().contains("not found"));
        assert!(b
            .update_task("a", &json!({"permission_mode": "bypass"}), PermissionMode::AcceptEdits, 1)
            .unwrap_err()
            .contains("not allowed"));
        // Solo da queued/failed/blocked/cancelled.
        for (s, ok) in [
            (TaskStatus::Queued, true),
            (TaskStatus::Failed, true),
            (TaskStatus::Blocked, true),
            (TaskStatus::Cancelled, true),
            (TaskStatus::Running, false),
            (TaskStatus::Review, false),
            (TaskStatus::Done, false),
        ] {
            set_status(&mut b, "b", s);
            let r = b.update_task("b", &json!({"title": "t"}), PermissionMode::Bypass, 1);
            assert_eq!(r.is_ok(), ok, "{s:?}");
        }
        // L'isolamento non cambia dopo la creazione del worktree.
        set_status(&mut b, "b", TaskStatus::Failed);
        b.task_mut("b").unwrap().worktree =
            Some(WorktreeInfo { path: "p".into(), branch: "task/b".into(), base: "main".into(), repo: None, setup_done: true });
        let err = b.update_task("b", &json!({"isolation": "none"}), PermissionMode::Bypass, 1).unwrap_err();
        assert!(err.contains("already has a worktree"), "{err}");
        assert!(b.update_task("b", &json!({"isolation": "worktree"}), PermissionMode::Bypass, 1).is_ok());
    }

    #[test]
    fn status_transitions() {
        let mut b = board_with(vec![spec("a")]);
        let t = |b: &mut Board, to, from| {
            set_status(b, "a", from);
            b.check_transition("a", to).is_ok()
        };
        use TaskStatus::*;
        for from in [Queued, Review, Failed, Blocked, Cancelled] {
            assert!(t(&mut b, Done, from), "done from {from:?}");
        }
        for from in [Running, Done] {
            assert!(!t(&mut b, Done, from), "done from {from:?}");
        }
        for from in [Queued, Running, Review, Failed, Blocked] {
            assert!(t(&mut b, Cancelled, from), "cancel from {from:?}");
        }
        for from in [Done, Cancelled] {
            assert!(!t(&mut b, Cancelled, from), "cancel from {from:?}");
        }
        for from in [Failed, Blocked, Cancelled, Review] {
            assert!(t(&mut b, Queued, from), "retry from {from:?}");
        }
        for from in [Queued, Running, Done] {
            assert!(!t(&mut b, Queued, from), "retry from {from:?}");
        }
        assert!(b.check_transition("a", Running).is_err());
        assert!(b.check_transition("zz", Done).is_err());
        set_status(&mut b, "a", Running);
        assert!(b.check_transition("a", Done).unwrap_err().contains("running"));
    }

    #[test]
    fn cancelled_to_done_is_external_and_needs_note() {
        let mut b = board_with(vec![spec("a")]);
        set_status(&mut b, "a", TaskStatus::Cancelled);
        let err = b.apply_status("a", TaskStatus::Done, None, 5).unwrap_err();
        assert!(err.contains("cancelled") && err.contains("note"), "{err}");
        assert_eq!(b.task("a").unwrap().status, TaskStatus::Cancelled);
        b.apply_status("a", TaskStatus::Done, Some("fatto a mano".into()), 6).unwrap();
        let t = b.task("a").unwrap();
        assert!(t.external && t.status == TaskStatus::Done);
        assert_eq!(t.note.as_deref(), Some("fatto a mano"));
    }

    #[test]
    fn held_is_cleared_on_done_only() {
        let mut b = board_with(vec![spec("a"), spec("c")]);
        for id in ["a", "c"] {
            b.task_mut(id).unwrap().held = true;
        }
        b.apply_status("a", TaskStatus::Done, Some("fatto".into()), 5).unwrap();
        assert!(!b.task("a").unwrap().held);
        // Annullato: resta trattenuto, anche dopo il retry.
        b.apply_status("c", TaskStatus::Cancelled, None, 5).unwrap();
        assert!(b.task("c").unwrap().held);
        b.apply_status("c", TaskStatus::Queued, None, 6).unwrap();
        assert!(b.task("c").unwrap().held);
        // La riga compatta mostra held solo per i task in coda.
        b.task_mut("a").unwrap().held = true;
        b.apply_status("c", TaskStatus::Cancelled, None, 7).unwrap();
        let j = b.view().to_compact_json(Some(&[Column::Done, Column::Cancelled, Column::Ready]));
        for r in j["tasks"].as_array().unwrap() {
            assert!(r.get("held").is_none(), "{r}");
        }
        // Caricando da disco, un done torna non trattenuto.
        let raw = serde_json::to_string(&b).unwrap();
        assert!(b.task("a").unwrap().held);
        let loaded = Board::from_json(&raw, 10).unwrap();
        assert!(!loaded.task("a").unwrap().held);
        assert!(loaded.task("c").unwrap().held);
    }

    #[test]
    fn compact_json_summarizes_integrations() {
        let mut b = Board::new("ws");
        let int = |done, skipped| Integration { branch: "i".into(), path: "p".into(), base: "main".into(), setup_done: done, setup_skipped: skipped };
        b.integrations.insert("a".into(), int(true, false));
        b.integrations.insert("b".into(), int(true, true));
        b.integrations.insert("c".into(), int(false, false));
        let j = b.view().to_compact_json(None);
        assert_eq!(j["integrations"]["a"], json!({"branch": "i", "path": "p", "base": "main", "setup": "done"}));
        assert_eq!(j["integrations"]["b"]["setup"], "none");
        assert_eq!(j["integrations"]["c"]["setup"], "pending");
        assert!(j["integrations"]["a"].get("setup_done").is_none());
        // La serializzazione completa resta invariata (usata dal frontend).
        let full = serde_json::to_value(&b.integrations["b"]).unwrap();
        assert_eq!(full["setup_skipped"], true);
        assert!(serde_json::to_value(&b.integrations["a"]).unwrap().get("setup_skipped").is_none());
    }

    #[test]
    fn apply_status_resets_on_retry() {
        let mut b = board_with(vec![spec("a")]);
        b.start_task("a", 20);
        assert!(b.finish_running("a", None, TaskStatus::Failed, None, Some("boom".into()), 25));
        {
            let t = b.task_mut("a").unwrap();
            t.report = Some(build_report("failed", "s", ReportFields::default(), None));
            t.worktree = Some(WorktreeInfo { path: "p".into(), branch: "task/a".into(), base: "main".into(), repo: None, setup_done: true });
        }
        b.apply_status("a", TaskStatus::Queued, None, 30).unwrap();
        let t = b.task("a").unwrap();
        assert_eq!((t.status, t.attempts), (TaskStatus::Queued, 1));
        assert!(t.report.is_none() && t.finished_at.is_none());
        assert_eq!(t.note.as_deref(), Some("boom"), "la nota precedente resta");
        assert!(t.worktree.is_some(), "il worktree si riusa");
        b.apply_status("a", TaskStatus::Cancelled, None, 31).unwrap();
        b.apply_status("a", TaskStatus::Queued, Some("riprova".into()), 32).unwrap();
        assert_eq!(b.task("a").unwrap().note.as_deref(), Some("riprova"));
        b.apply_status("a", TaskStatus::Cancelled, Some("  non serve piu'  ".into()), 40).unwrap();
        let t = b.task("a").unwrap();
        assert_eq!(t.note.as_deref(), Some("non serve piu'"));
        assert_eq!(t.finished_at, Some(40));
        // hold solo per i task in coda.
        assert!(b.set_held("a", true, 41).is_err());
        b.apply_status("a", TaskStatus::Queued, None, 42).unwrap();
        b.set_held("a", true, 43).unwrap();
        assert!(b.task("a").unwrap().held);
        b.set_held("a", false, 44).unwrap();
        assert!(!b.task("a").unwrap().held);
    }

    #[test]
    fn finish_running_checks_session() {
        let mut b = board_with(vec![spec("a")]);
        b.start_task("a", 20);
        b.task_mut("a").unwrap().session = Some("s1".into());
        assert!(!b.finish_running("a", Some("s2"), TaskStatus::Review, None, None, 30));
        assert!(!b.finish_running("zz", None, TaskStatus::Review, None, None, 30));
        let rep = build_report("completed", &"x".repeat(500), ReportFields::default(), Some("d".repeat(5000)));
        assert_eq!(rep.diffstat.as_ref().unwrap().len(), DIFFSTAT_MAX);
        assert!(b.finish_running("a", Some("s1"), TaskStatus::Review, Some(rep), None, 30));
        let t = b.task("a").unwrap();
        assert_eq!(t.status, TaskStatus::Review);
        assert_eq!(t.last_report.as_ref().unwrap().chars().count(), 200);
        assert!(t.session.is_none());
        // Gia' concluso: nessun secondo effetto.
        assert!(!b.finish_running("a", Some("s1"), TaskStatus::Failed, None, None, 31));
    }

    #[test]
    fn report_fields_parsing() {
        let f = parse_report_fields(&json!({
            "status": "completed", "summary": "s", "commit": " abc1234 ",
            "files_changed": ["a.rs", "b.rs"],
            "tests": [{"command": "cargo test", "result": "passed"}, {"command": "npm t", "result": "not_run", "details": "no node"}],
            "follow_ups": ["x"]
        }))
        .unwrap();
        assert_eq!(f.commit.as_deref(), Some("abc1234"));
        assert_eq!(f.files_changed.len(), 2);
        assert_eq!(f.tests[0], TestRun { command: "cargo test".into(), result: "passed".into(), details: None });
        assert_eq!(f.tests[1].details.as_deref(), Some("no node"));
        assert_eq!(parse_report_fields(&json!({"status": "x"})).unwrap(), ReportFields::default());
        for bad in [
            json!({"commit": "a b"}),
            json!({"commit": 5}),
            json!({"files_changed": "a"}),
            json!({"files_changed": [1]}),
            json!({"tests": {"command": "x"}}),
            json!({"tests": [{"command": "x", "result": "ok"}]}),
            json!({"tests": [{"result": "passed"}]}),
            json!({"tests": [{"command": "x", "result": "passed", "details": 3}]}),
            json!({"follow_ups": [""]}),
        ] {
            assert!(parse_report_fields(&bad).is_err(), "{bad}");
        }
        let r = build_report("blocked", "sum", f, None);
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["status"], "blocked");
        assert_eq!(j["tests"][0]["result"], "passed");
        assert!(j.get("diffstat").is_none());
    }

    // --- scheduler ---

    fn ids(v: Vec<String>) -> Vec<String> {
        v
    }

    #[test]
    fn scheduler_orders_by_priority_then_creation_then_position() {
        let mut hi = spec("hi");
        hi.priority = 10;
        let mut b = Board::new("ws-1");
        b.max_parallel = 16;
        b.add_plan(vec![spec("a"), spec("b")], PermissionMode::Bypass, 5).unwrap();
        b.add_plan(vec![spec("c"), hi], PermissionMode::Bypass, 4).unwrap();
        // priority desc, poi created_at (c,hi sono piu' vecchi), poi posizione.
        assert_eq!(ids(schedule(&b, &ctx())), vec!["hi", "c", "a", "b"]);
    }

    #[test]
    fn scheduler_respects_max_parallel_and_running() {
        let mut b = board_with((0..6).map(|i| spec(&format!("t{i}"))).collect());
        b.max_parallel = 2;
        assert_eq!(schedule(&b, &ctx()), vec!["t0", "t1"]);
        set_status(&mut b, "t0", TaskStatus::Running);
        assert_eq!(schedule(&b, &ctx()), vec!["t1"]);
        set_status(&mut b, "t1", TaskStatus::Running);
        assert!(schedule(&b, &ctx()).is_empty());
        // Review non occupa slot.
        set_status(&mut b, "t0", TaskStatus::Review);
        assert_eq!(schedule(&b, &ctx()), vec!["t2"]);
        // Tetto per orchestratore.
        b.max_parallel = 16;
        set_status(&mut b, "t1", TaskStatus::Queued);
        let c = SchedCtx { max_start: 2, ..ctx() };
        assert_eq!(schedule(&b, &c).len(), 2);
        let c = SchedCtx { max_start: 0, ..ctx() };
        assert!(schedule(&b, &c).is_empty());
    }

    #[test]
    fn scheduler_skips_held_paused_and_unmet_dependencies() {
        let mut dep = spec("dep");
        dep.depends_on = vec!["base".to_string()];
        let mut held = spec("held");
        held.held = true;
        let mut b = board_with(vec![spec("base"), dep, held, spec("free")]);
        assert_eq!(schedule(&b, &ctx()), vec!["base", "free"]);
        b.paused = true;
        assert!(schedule(&b, &ctx()).is_empty());
        b.paused = false;
        // review non soddisfa la dipendenza, done si'.
        set_status(&mut b, "base", TaskStatus::Review);
        assert_eq!(schedule(&b, &ctx()), vec!["free"]);
        set_status(&mut b, "base", TaskStatus::Done);
        assert_eq!(schedule(&b, &ctx()), vec!["dep", "free"]);
    }

    #[test]
    fn cancelled_dependencies_do_not_satisfy() {
        let mut dep = spec("dep");
        dep.depends_on = vec!["base".to_string()];
        let mut b = board_with(vec![spec("base"), dep]);
        set_status(&mut b, "base", TaskStatus::Cancelled);
        assert!(schedule(&b, &ctx()).is_empty());
        assert_eq!(b.column(b.task("dep").unwrap()), Column::Backlog);
        for s in [TaskStatus::Failed, TaskStatus::Blocked] {
            set_status(&mut b, "base", s);
            assert!(schedule(&b, &ctx()).is_empty(), "{s:?}");
        }
    }

    fn touching(id: &str, areas: &[&str]) -> TaskSpec {
        spec_with(json!({"id": id, "task": "x", "touches": areas}))
    }

    #[test]
    fn scheduler_avoids_touch_conflicts() {
        let mut b = board_with(vec![
            touching("a", &["src/auth"]),
            touching("b", &["src/auth/login"]),
            touching("c", &["src/db"]),
            touching("d", &["src/authz"]),
            touching("e", &[]),
            touching("f", &["src"]),
        ]);
        // b confligge con a (prefisso), f con tutti quelli in src; d no (segmento diverso).
        assert_eq!(schedule(&b, &ctx()), vec!["a", "c", "d", "e"]);
        // Un task in esecuzione blocca i conflittuali.
        set_status(&mut b, "a", TaskStatus::Running);
        assert_eq!(schedule(&b, &ctx()), vec!["c", "d", "e"]);
        set_status(&mut b, "a", TaskStatus::Review);
        assert_eq!(schedule(&b, &ctx()), vec!["b", "c", "d", "e"]);
    }

    #[test]
    fn scheduler_touch_matching_is_case_insensitive_on_windows() {
        let b = board_with(vec![touching("a", &["Src/Auth"]), touching("b", &["src/auth/x"])]);
        let unix = SchedCtx { case_insensitive: false, ..ctx() };
        let win = SchedCtx { case_insensitive: true, ..ctx() };
        assert_eq!(schedule(&b, &unix), vec!["a", "b"]);
        assert_eq!(schedule(&b, &win), vec!["a"]);
    }

    #[test]
    fn scheduler_allows_one_writing_task_without_isolation() {
        let none = |id: &str, mode: Option<&str>| {
            let mut v = json!({"id": id, "task": "x", "isolation": "none"});
            if let Some(m) = mode {
                v["permission_mode"] = json!(m);
            }
            spec_with(v)
        };
        let mut b = board_with(vec![
            none("w1", None),
            none("w2", None),
            none("r1", Some("plan")),
            none("r2", Some("plan")),
            spec("iso"),
        ]);
        // Un solo scrittore nella root condivisa; i read-only (plan) e i worktree passano.
        assert_eq!(schedule(&b, &ctx()), vec!["w1", "r1", "r2", "iso"]);
        // Con uno scrittore gia' in esecuzione nessun altro scrittore parte.
        set_status(&mut b, "w1", TaskStatus::Running);
        assert_eq!(schedule(&b, &ctx()), vec!["r1", "r2", "iso"]);
        // Un read-only in esecuzione non blocca lo scrittore.
        set_status(&mut b, "w1", TaskStatus::Queued);
        set_status(&mut b, "r1", TaskStatus::Running);
        assert_eq!(schedule(&b, &ctx()), vec!["w1", "r2", "iso"]);
        // Con tetto `plan` ogni task `none` e' read-only.
        let ro = SchedCtx { ceiling: PermissionMode::Plan, ..ctx() };
        assert_eq!(schedule(&b, &ro), vec!["w1", "w2", "r2", "iso"]);
        // Con un tetto piu' alto del richiesto: il permesso effettivo e' quello richiesto.
        let t = b.task("r1").unwrap();
        assert_eq!(t.effective_permission(PermissionMode::Bypass), PermissionMode::Plan);
        assert_eq!(b.task("w1").unwrap().effective_permission(PermissionMode::Bypass), PermissionMode::Bypass);
    }

    #[test]
    fn done_from_queued_requires_a_note_and_marks_external() {
        let mut b = board_with(vec![spec("a"), spec_with(json!({"id": "b", "task": "x", "depends_on": ["a"], "held": true}))]);
        assert!(b.check_transition("a", TaskStatus::Done).is_ok());
        let e = b.apply_status("a", TaskStatus::Done, None, 20).unwrap_err();
        assert!(e.contains("note"), "{e}");
        let e = b.apply_status("a", TaskStatus::Done, Some("   ".into()), 20).unwrap_err();
        assert!(e.contains("note"), "{e}");
        assert_eq!(b.task("a").unwrap().status, TaskStatus::Queued);
        // Anche un task trattenuto.
        b.apply_status("a", TaskStatus::Done, Some("merged by hand".into()), 21).unwrap();
        let a = b.task("a").unwrap();
        assert!(a.external && a.status == TaskStatus::Done);
        assert_eq!(a.note.as_deref(), Some("merged by hand"));
        // Sblocca i dipendenti come un done normale.
        assert_eq!(b.view().tasks[1].column, Column::Ready);
        let j = b.view().to_compact_json(Some(&[Column::Done]));
        assert_eq!(j["tasks"][0]["external"], true);
        // Un done normale (da review) non e' esterno.
        b.add_plan(vec![spec("c")], PermissionMode::Bypass, 30).unwrap();
        b.start_task("c", 31);
        b.finish_running("c", None, TaskStatus::Review, None, None, 32);
        b.apply_status("c", TaskStatus::Done, None, 33).unwrap();
        assert!(!b.task("c").unwrap().external);
    }

    #[test]
    fn completion_text_has_counts_and_is_suppressed_when_nothing_ever_started() {
        let mut b = board_with(vec![spec("a"), spec("b"), spec("c")]);
        assert!(b.completion_text().is_none(), "piano non completo");
        b.apply_status("a", TaskStatus::Done, Some("by hand".into()), 20).unwrap();
        b.start_task("b", 21);
        b.finish_running("b", None, TaskStatus::Review, None, None, 22);
        b.apply_status("b", TaskStatus::Done, None, 23).unwrap();
        b.apply_status("c", TaskStatus::Cancelled, None, 24).unwrap();
        let t = b.completion_text().unwrap();
        assert!(t.contains("2 done (of which 1 completed outside the board)"), "{t}");
        assert!(t.contains("0 failed") && t.contains("1 cancelled"), "{t}");
        // Tutti annullati e mai partiti: niente evento.
        let mut c = board_with(vec![spec("a"), spec("b")]);
        c.apply_status("a", TaskStatus::Cancelled, None, 20).unwrap();
        c.apply_status("b", TaskStatus::Cancelled, None, 21).unwrap();
        assert!(c.is_complete() && c.completion_text().is_none());
        // Annullato dopo essere partito: l'evento c'e'.
        let mut d = board_with(vec![spec("a")]);
        d.start_task("a", 20);
        d.finish_running("a", None, TaskStatus::Failed, None, None, 21);
        d.apply_status("a", TaskStatus::Cancelled, None, 22).unwrap();
        assert!(d.completion_text().unwrap().contains("1 cancelled"));
    }

    #[test]
    fn orchestrator_note_is_a_spec_field_and_held_is_counted() {
        let mut b = board_with(vec![spec_with(json!({"id": "a", "task": "x", "orchestrator_note": "  merge me last  ", "held": true})), spec("b")]);
        assert_eq!(b.task("a").unwrap().orchestrator_note.as_deref(), Some("merge me last"));
        let j = b.view().to_compact_json(None);
        assert_eq!(j["tasks"][0]["held"], true);
        assert_eq!(j["tasks"][0]["orchestrator_note"], "merge me last");
        assert_eq!(j["held"], 1);
        assert_eq!(j["counts"]["ready"], 2);
        assert!(j["tasks"][1].get("held").is_none());
        // update_task: assente = invariato, null = azzera, stringa = sostituisce.
        b.update_task("a", &json!({"title": "t"}), PermissionMode::Bypass, 20).unwrap();
        assert_eq!(b.task("a").unwrap().orchestrator_note.as_deref(), Some("merge me last"));
        b.update_task("a", &json!({"orchestrator_note": "new"}), PermissionMode::Bypass, 21).unwrap();
        assert_eq!(b.task("a").unwrap().orchestrator_note.as_deref(), Some("new"));
        b.update_task("a", &json!({"orchestrator_note": null}), PermissionMode::Bypass, 22).unwrap();
        assert!(b.task("a").unwrap().orchestrator_note.is_none());
        assert!(parse_spec(&json!({"id": "a", "task": "x", "orchestrator_note": 3}), None).is_err());
        let long = parse_spec(&json!({"id": "a", "task": "x", "orchestrator_note": "é".repeat(2500)}), None).unwrap();
        assert_eq!(long.orchestrator_note.unwrap().chars().count(), 2000);
        b.set_held("a", false, 23).unwrap();
        assert!(b.view().to_compact_json(None).get("held").is_none());
    }

    #[test]
    fn worker_environment_mentions_setup_worktree_and_rules() {
        let mut t = Task::from_spec(touching("a", &["src/auth", "docs"]), 1);
        t.worktree = Some(WorktreeInfo { path: "C:\\p\\.worktrees\\a".into(), branch: "task/a".into(), base: "main".into(), repo: None, setup_done: true });
        let env = worker_environment(&EnvInfo { root: "C:\\p", workdir: "C:\\p\\.worktrees\\a", task: &t, setup: Some("npm ci"), repo_cfg: None });
        assert!(env.contains("run this setup command") && env.contains("npm ci"));
        assert!(env.contains("branch task/a (created from main)"));
        assert!(env.contains("src/auth, docs"));
        assert!(env.contains("Do NOT merge, push, rebase or switch branches"));
        assert!(env.contains("complete_task") && env.contains("files_changed"));
        assert!(env.find("setup command").unwrap() < env.find("Rules").unwrap());
        let env = worker_environment(&EnvInfo { root: "C:\\p", workdir: "C:\\p\\.worktrees\\a", task: &t, setup: None, repo_cfg: None });
        assert!(env.contains("No setup command"));
        // Senza isolamento: root, niente branch, aree dichiarate.
        let mut n = Task::from_spec(touching("n", &["lib"]), 1);
        n.isolation = Isolation::None;
        let env = worker_environment(&EnvInfo { root: "/r", workdir: "/r", task: &n, setup: Some("  "), repo_cfg: None });
        assert!(env.contains("No dedicated worktree") && env.contains("No setup command"));
        assert!(env.contains("Do not create branches") && env.contains("Do not touch areas outside the declared ones"));
        assert!(env.is_ascii());
    }

    #[test]
    fn repo_is_normalized_and_validated() {
        let repo = |v: Value| parse_spec(&json!({"id": "a", "task": "x", "repo": v}), None);
        assert_eq!(repo(json!("freud-manager")).unwrap().repo.as_deref(), Some("freud-manager"));
        assert_eq!(repo(json!(" ./apps\\web/ ")).unwrap().repo.as_deref(), Some("apps/web"));
        // La root vale None, come null o l'assenza del campo.
        for root in [json!("."), json!("./"), json!(null)] {
            assert_eq!(repo(root).unwrap().repo, None);
        }
        assert_eq!(spec("a").repo, None);
        for bad in ["", "  ", "/abs", "C:\\x", "c:/x", "..", "a/../b", "../x", "a\nb", &"r".repeat(257)] {
            assert!(repo(json!(bad)).is_err(), "{bad:?}");
        }
        assert!(repo(json!(3)).unwrap_err().contains("string"));
        // Parziale: il campo assente resta, null lo toglie.
        let base = spec_with(json!({"id": "a", "task": "x", "repo": "r1"}));
        let kept = parse_spec(&json!({"id": "a", "title": "t"}), Some(&base)).unwrap();
        assert_eq!(kept.repo.as_deref(), Some("r1"));
        let cleared = parse_spec(&json!({"id": "a", "repo": null}), Some(&base)).unwrap();
        assert_eq!(cleared.repo, None);
    }

    #[test]
    fn repo_cannot_change_once_the_task_has_a_worktree() {
        let mut b = board_with(vec![spec_with(json!({"id": "a", "task": "x", "repo": "r1"}))]);
        b.update_task("a", &json!({"repo": "r2"}), PermissionMode::Bypass, 20).unwrap();
        assert_eq!(b.task("a").unwrap().repo.as_deref(), Some("r2"));
        b.task_mut("a").unwrap().worktree =
            Some(WorktreeInfo { path: "p".into(), branch: "task/a".into(), base: "main".into(), repo: Some("r2".into()), setup_done: true });
        let e = b.update_task("a", &json!({"repo": "r3"}), PermissionMode::Bypass, 21).unwrap_err();
        assert!(e.contains("repo cannot be changed"), "{e}");
        // Lo stesso valore e altri campi si possono ancora cambiare.
        b.update_task("a", &json!({"repo": "r2", "priority": 2}), PermissionMode::Bypass, 22).unwrap();
    }

    #[test]
    fn scheduler_conflicts_only_inside_the_same_repo() {
        let t = |id: &str, repo: Option<&str>, iso: &str| {
            let mut v = json!({"id": id, "task": "x", "touches": ["src"], "isolation": iso});
            if let Some(r) = repo {
                v["repo"] = json!(r);
            }
            spec_with(v)
        };
        // Stessa area `src` in repo diversi e nella root: nessun conflitto.
        let b = board_with(vec![t("a", Some("r1"), "worktree"), t("b", Some("r2"), "worktree"), t("c", None, "worktree")]);
        assert_eq!(schedule(&b, &ctx()), ["a", "b", "c"]);
        // Nello stesso repo si accodano.
        let b = board_with(vec![t("a", Some("r1"), "worktree"), t("b", Some("r1"), "worktree")]);
        assert_eq!(schedule(&b, &ctx()), ["a"]);
        // Anche contro un task gia' in esecuzione.
        let mut b = board_with(vec![t("a", Some("r1"), "worktree"), t("b", Some("r1"), "worktree"), t("c", Some("r2"), "worktree")]);
        set_status(&mut b, "a", TaskStatus::Running);
        assert_eq!(schedule(&b, &ctx()), ["c"]);
        // Un solo task `none` scrivente per repository.
        let b = board_with(vec![
            spec_with(json!({"id": "a", "task": "x", "isolation": "none", "repo": "r1"})),
            spec_with(json!({"id": "b", "task": "x", "isolation": "none", "repo": "r2"})),
            spec_with(json!({"id": "c", "task": "x", "isolation": "none", "repo": "r1"})),
            spec_with(json!({"id": "d", "task": "x", "isolation": "none"})),
        ]);
        assert_eq!(schedule(&b, &ctx()), ["a", "b", "d"]);
    }

    #[test]
    fn repo_settings_merge_and_integration_reset() {
        let p = parse_plan(&json!({
            "tasks": [{"id": "a", "task": "x"}],
            "base_branches": {"freud-manager": "master", "./DDB/": "main", ".": "dev"},
            "integration_branch": "integration/{date}"
        }))
        .unwrap();
        let mut b = Board::new("ws");
        b.apply_repo_settings(p.base_branches, p.integration_branch);
        assert_eq!(b.base_branches.get("freud-manager").map(String::as_str), Some("master"));
        assert_eq!(b.base_branches.get("DDB").map(String::as_str), Some("main"));
        assert_eq!(b.base_branches.get(".").map(String::as_str), Some("dev"));
        assert_eq!(b.integration_branch.as_deref(), Some("integration/{date}"));
        b.integrations.insert(".".into(), Integration { branch: "x".into(), path: "p".into(), base: "main".into(), setup_done: true, setup_skipped: false });
        b.repos = Some(vec![".".into()]);
        // Stringa vuota o null tolgono una voce; le altre restano.
        let ops = parse_base_branches(&json!({"DDB": "", "freud-manager": null})).unwrap();
        b.apply_repo_settings(ops, None);
        assert_eq!(b.base_branches.keys().collect::<Vec<_>>(), ["."]);
        // Stesso modello: le integrazioni restano; cambiato o spento: si scartano.
        assert_eq!(b.repos, None);
        b.apply_repo_settings(Vec::new(), Some(Some("integration/{date}".into())));
        assert_eq!(b.integrations.len(), 1);
        b.apply_repo_settings(Vec::new(), Some(None));
        assert!(b.integrations.is_empty() && b.integration_branch.is_none());
        // Validazione.
        assert!(parse_base_branches(&json!({"x": "a..b"})).is_err());
        assert!(parse_base_branches(&json!({"../x": "main"})).is_err());
        assert!(parse_base_branches(&json!(["x"])).is_err());
        assert!(parse_integration_template(&json!("--bad")).is_err());
        assert!(parse_integration_template(&json!("a b/{date}")).is_err());
        assert!(parse_integration_template(&json!("{other}")).is_err());
        assert_eq!(parse_integration_template(&json!("")).unwrap(), None);
        assert_eq!(parse_integration_template(&json!(null)).unwrap(), None);
    }

    #[test]
    fn integration_names_get_a_numeric_suffix() {
        assert_eq!(expand_integration("integration/{date}", "2026-10-21").unwrap(), "integration/2026-10-21");
        let taken = ["int", "int-2"];
        assert_eq!(first_free_branch("int", |c| taken.contains(&c)).as_deref(), Some("int-3"));
        assert_eq!(first_free_branch("int", |_| false).as_deref(), Some("int"));
        assert_eq!(first_free_branch("int", |_| true), None);
        assert!(integration_matches("int", "int"));
        assert!(integration_matches("int", "int-2"));
        assert!(!integration_matches("int", "int-x"));
        assert!(!integration_matches("int", "int-"));
        assert!(!integration_matches("int", "integration"));
        assert_eq!(repo_key(None), ".");
        assert_eq!(repo_key(Some("a/b")), "a/b");
    }
}
