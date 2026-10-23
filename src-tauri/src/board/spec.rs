//! Campi di un task e loro validazione: `TaskSpec` (propose_tasks, task_create,
//! update_task), id, branch, repository, grafo delle dipendenze, id generati
//! dal titolo. Funzioni pure, nessun I/O.

use super::Task;
use crate::automation::{clip_chars, req_text, valid_model, TASK_MAX, TITLE_MAX};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const ID_MAX: usize = 40;
pub const MAX_TASKS_PER_PROPOSAL: usize = 20;
const MAX_DEPENDS: usize = 50;
const REPO_MAX: usize = 256;
const BRANCH_MAX: usize = 100;

/// Campi di input di un task, normalizzati. `id` vuoto = da generare
/// (`task_create`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskSpec {
    pub id: String,
    pub title: String,
    pub task: String,
    pub depends_on: Vec<String>,
    /// Repository (cartella relativa alla root); `None` = la root.
    pub repo: Option<String>,
    pub model: Option<String>,
}

const SPEC_KEYS: [&str; 6] = ["id", "title", "task", "depends_on", "repo", "model"];

impl TaskSpec {
    pub fn from_task(t: &Task) -> TaskSpec {
        TaskSpec {
            id: t.id.clone(),
            title: t.title.clone(),
            task: t.task.clone(),
            depends_on: t.depends_on.clone(),
            repo: t.repo.clone(),
            model: t.model.clone(),
        }
    }
}

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

/// Repository di un task: cartella relativa alla root. Normalizza i
/// separatori (`/`), toglie `./` iniziale e `/` finale; niente path assoluti
/// ne' `..`. `"."` (la root) vale `None`.
pub fn normalize_repo(s: &str) -> Result<Option<String>, String> {
    let t = s.trim().replace('\\', "/");
    if t.is_empty() {
        return Err("repo is empty (use \".\" for the workspace root, or null)".to_string());
    }
    if t.chars().count() > REPO_MAX || t.chars().any(char::is_control) {
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

/// Chiave di un repository nelle mappe: "." = la root.
pub fn repo_key(repo: Option<&str>) -> String {
    repo.unwrap_or(".").to_string()
}

pub fn parse_max_parallel(v: &Value) -> Result<u8, String> {
    match v.as_u64() {
        Some(n) if (1..=u64::from(super::MAX_PARALLEL_LIMIT)).contains(&n) => Ok(n as u8),
        _ => Err(format!("max_parallel must be an integer between 1 and {}", super::MAX_PARALLEL_LIMIT)),
    }
}

fn dedup(v: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    v.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

fn parse_depends(v: &Value) -> Result<Vec<String>, String> {
    let arr = v.as_array().ok_or_else(|| "depends_on must be an array of task ids".to_string())?;
    if arr.len() > MAX_DEPENDS {
        return Err(format!("depends_on has too many entries (max {MAX_DEPENDS})"));
    }
    let mut out = Vec::new();
    for e in arr {
        let s = e.as_str().ok_or_else(|| "depends_on must be an array of task ids".to_string())?.trim();
        if !valid_id(s) {
            return Err(format!("invalid task id in depends_on: {}", clip_chars(s, 50)));
        }
        out.push(s.to_string());
    }
    Ok(dedup(out))
}

/// Legge un task da JSON. Senza `base` e' una creazione (`title` e `task`
/// obbligatori, `id` facoltativo); con `base` e' una modifica parziale: i
/// campi assenti restano quelli di `base`. Campi sconosciuti = errore (refusi).
pub fn parse_spec(v: &Value, base: Option<&TaskSpec>) -> Result<TaskSpec, String> {
    let obj = v.as_object().ok_or_else(|| "each task must be an object".to_string())?;
    for k in obj.keys() {
        if !SPEC_KEYS.contains(&k.as_str()) {
            return Err(format!("unknown field '{k}' (allowed: {})", SPEC_KEYS.join(", ")));
        }
    }
    let text = |k: &str| -> Result<Option<&str>, String> {
        match obj.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(format!("{k} must be a string")),
        }
    };
    let id = match (text("id")?.map(str::trim), base) {
        (Some(s), Some(b)) if s != b.id => return Err("the id of a task cannot be changed".to_string()),
        (_, Some(b)) => b.id.clone(),
        (Some(s), None) if valid_id(s) => s.to_string(),
        (Some(s), None) => {
            return Err(format!(
                "invalid id '{}' (lowercase letters, digits and '-', 1..={ID_MAX} characters, not starting with '-')",
                clip_chars(s, 50)
            ))
        }
        (None, None) => String::new(),
    };
    let title = match (text("title")?.map(str::trim).filter(|t| !t.is_empty()), base) {
        (Some(t), _) if t.chars().count() > TITLE_MAX => {
            return Err(format!("title is too long (max {TITLE_MAX} characters)"))
        }
        (Some(t), _) => t.to_string(),
        (None, Some(b)) => b.title.clone(),
        (None, None) => return Err("title is missing".to_string()),
    };
    let task = match (text("task")?, base) {
        (Some(s), _) => req_text("task", s, TASK_MAX)?,
        (None, Some(b)) => b.task.clone(),
        (None, None) => return Err("task is missing".to_string()),
    };
    let depends_on = match obj.get("depends_on") {
        None => base.map(|b| b.depends_on.clone()).unwrap_or_default(),
        Some(Value::Null) => Vec::new(),
        Some(v) => parse_depends(v)?,
    };
    let repo = match obj.get("repo") {
        None => base.and_then(|b| b.repo.clone()),
        Some(Value::Null) => None,
        Some(Value::String(s)) => normalize_repo(s)?,
        Some(_) => return Err("repo must be a string".to_string()),
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
    Ok(TaskSpec { id, title, task, depends_on, repo, model })
}

/// Parametri di `propose_tasks`: `{ tasks: [TaskSpec] }`, ogni task con id.
pub fn parse_proposal(p: &Value) -> Result<Vec<TaskSpec>, String> {
    let obj = p.as_object().ok_or_else(|| "params must be an object".to_string())?;
    if let Some(k) = obj.keys().find(|k| k.as_str() != "tasks") {
        return Err(format!("unknown field '{k}' (allowed: tasks)"));
    }
    let arr = obj
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| "tasks is missing (an array of task objects)".to_string())?;
    if arr.is_empty() {
        return Err("tasks is empty".to_string());
    }
    if arr.len() > MAX_TASKS_PER_PROPOSAL {
        return Err(format!("too many tasks in one proposal (max {MAX_TASKS_PER_PROPOSAL})"));
    }
    let mut specs = Vec::with_capacity(arr.len());
    for (i, t) in arr.iter().enumerate() {
        let spec = parse_spec(t, None).map_err(|e| format!("tasks[{i}]: {e}"))?;
        if spec.id.is_empty() {
            return Err(format!("tasks[{i}]: id is missing"));
        }
        if spec.model.is_none() {
            return Err(format!("tasks[{i}]: task {}: model is required", spec.id));
        }
        specs.push(spec);
    }
    Ok(specs)
}

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

/// Id per un task creato a mano: slug ASCII del titolo (minuscole, il resto
/// diventa `-`, al massimo `ID_MAX` caratteri); `-2`, `-3`... se occupato;
/// `task-<n>` se il titolo non dà nulla di utilizzabile.
pub fn id_from_title(title: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let base = slug.trim_matches('-');
    let base = base.get(..base.len().min(ID_MAX)).unwrap_or(base).trim_end_matches('-');
    if !base.is_empty() {
        for n in 1u32.. {
            let suffix = if n == 1 { String::new() } else { format!("-{n}") };
            let stem = &base[..base.len().min(ID_MAX - suffix.len())];
            let id = format!("{}{suffix}", stem.trim_end_matches('-'));
            if !taken(&id) {
                return id;
            }
            if n >= 1000 {
                break;
            }
        }
    }
    (1u32..).map(|n| format!("task-{n}")).find(|id| !taken(id)).unwrap_or_default()
}
