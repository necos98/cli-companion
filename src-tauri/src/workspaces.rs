use crate::agents::PermissionMode;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

/// Lunghezza massima del nome di un avvio (in caratteri).
pub const MAX_LAUNCH_NAME_LEN: usize = 80;
/// Lunghezza massima del comando di un avvio (in caratteri).
pub const MAX_COMMAND_LEN: usize = 1000;

/// Avvio personalizzato di un workspace: comando eseguito all'apertura di un
/// nuovo terminale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub id: String,
    pub name: String,
    pub command: String,
    /// Solo per i file vecchi: letto e migrato in `Workspace::default_launch`.
    #[serde(default, skip_serializing)]
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// "windows" oppure "wsl"
    pub kind: String,
    /// Percorso Windows (es. C:\proj) oppure percorso Linux assoluto (es. /home/user/proj)
    pub path: String,
    /// Distro WSL, valorizzato solo quando kind == "wsl"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distro: Option<String>,
    /// Shell opzionale (es. pwsh, cmd, bash, zsh). Default: powershell su Windows, shell di default su WSL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    pub created_at: i64,
    /// Avvii personalizzati del workspace (vuoto = solo shell semplice).
    #[serde(default)]
    pub launches: Vec<Launch>,
    /// Tetto dei permessi dei worker automatici: "plan" | "default" |
    /// "accept-edits" | "bypass". L'orchestratore puo' solo restringerlo.
    #[serde(default = "default_worker_permission")]
    pub worker_permission: String,
    /// Comando di setup dei worktree (es. `npm ci`): non lo esegue l'app, lo
    /// legge il worker nell'ambiente del task (`get_task`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_setup: Option<String>,
    /// Modello di default dei worker automatici (None = quello dell'agente);
    /// vale quando ne' il task ne' `spawn_session` ne indicano uno.
    #[serde(default)]
    pub worker_model: Option<String>,
    /// Orchestratore automatico nelle finestre di esecuzione (vedi `schedule`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_orchestrate: Option<AutoOrchestrate>,
    /// Avvio predefinito dei nuovi terminali: `None` = eredita il predefinito
    /// globale; "shell" = shell semplice; altrimenti l'id di un avvio del
    /// workspace o di uno integrato (`builtin:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_launch: Option<String>,
}

/// Migra il vecchio flag `is_default` degli avvii in `default_launch`.
fn migrate_default(ws: &mut Workspace) {
    if ws.default_launch.is_none() {
        ws.default_launch = ws.launches.iter().find(|l| l.is_default).map(|l| l.id.clone());
    }
    for l in ws.launches.iter_mut() {
        l.is_default = false;
    }
}

/// Configurazione dell'orchestratore automatico di un workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoOrchestrate {
    pub enabled: bool,
    #[serde(default = "default_auto_agent")]
    pub agent: String,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_auto_agent() -> String {
    crate::automation::DEFAULT_AGENT.to_string()
}

fn default_worker_permission() -> String {
    PermissionMode::AcceptEdits.as_str().to_string()
}

impl Workspace {
    /// Tetto dei permessi dei worker; un valore illeggibile nel file vale
    /// quanto il default (mai piu' permissivo del solito).
    pub fn worker_permission_mode(&self) -> PermissionMode {
        PermissionMode::parse(&self.worker_permission).unwrap_or(PermissionMode::AcceptEdits)
    }
}

#[derive(Default)]
pub struct WorkspaceStore {
    inner: Mutex<StoreData>,
}

#[derive(Default)]
struct StoreData {
    workspaces: Vec<Workspace>,
    counter: u64,
}

impl WorkspaceStore {
    pub fn all(&self) -> Vec<Workspace> {
        self.inner.lock().unwrap().workspaces.clone()
    }

    pub fn find(&self, id: &str) -> Option<Workspace> {
        self.inner
            .lock()
            .unwrap()
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .cloned()
    }
}

const STORE_FILE: &str = "workspaces.json";

pub fn load_from_disk(app: &tauri::App) {
    let dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let path = dir.join(STORE_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    match serde_json::from_str::<Vec<Workspace>>(&raw) {
        Ok(mut workspaces) => {
            workspaces.iter_mut().for_each(migrate_default);
            let store = app.state::<WorkspaceStore>();
            let mut data = store.inner.lock().unwrap();
            data.workspaces = workspaces;
        }
        Err(_) => {
            // File corrotto: backup e riparto vuoto.
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
}

fn save_to_disk(app: &AppHandle, workspaces: &[Workspace]) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella dati: {e}"))?;
    let raw = serde_json::to_string_pretty(workspaces).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join(STORE_FILE), &raw).map_err(|e| format!("impossibile salvare: {e}"))?;
    Ok(())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn default_name_for(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("workspace")
        .to_string()
}

#[tauri::command]
pub fn list_workspaces(state: State<WorkspaceStore>) -> Vec<Workspace> {
    state.inner.lock().unwrap().workspaces.clone()
}

/// Async: la verifica della cartella WSL avvia `wsl.exe`, che a VM spenta
/// puo' impiegare secondi (sul main thread congelerebbe la finestra).
#[tauri::command]
pub async fn add_workspace(
    app: AppHandle,
    name: String,
    kind: String,
    path: String,
    distro: Option<String>,
    shell: Option<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || add_workspace_blocking(&app, name, kind, path, distro, shell)).await
}

fn add_workspace_blocking(
    app: &AppHandle,
    name: String,
    kind: String,
    path: String,
    distro: Option<String>,
    shell: Option<String>,
) -> Result<Workspace, String> {
    let state = app.state::<WorkspaceStore>();
    let kind = kind.trim().to_lowercase();
    if kind != "windows" && kind != "wsl" {
        return Err("kind non valido (usa \"windows\" o \"wsl\")".to_string());
    }
    let mut path = path.trim().to_string();
    if path.is_empty() {
        return Err("percorso mancante".to_string());
    }

    let distro = if kind == "wsl" {
        let distro = distro.unwrap_or_default().trim().to_string();
        if distro.is_empty() {
            return Err("distro WSL mancante".to_string());
        }
        // Accetta anche path incollati in formato Windows o \\wsl$\...
        if !path.starts_with('/') {
            path = crate::wsl::windows_to_wsl_path(&path)
                .ok_or_else(|| "il percorso WSL deve essere assoluto (es. /home/user/proj)".to_string())?;
        }
        if !crate::wsl::wsl_dir_exists(&distro, &path) {
            return Err(format!("cartella non trovata in {distro}: {path}"));
        }
        Some(distro)
    } else {
        let meta = std::fs::metadata(&path).map_err(|_| format!("cartella non trovata: {path}"))?;
        if !meta.is_dir() {
            return Err(format!("non è una cartella: {path}"));
        }
        None
    };

    let mut data = state.inner.lock().unwrap();
    let duplicate = data.workspaces.iter().any(|w| {
        w.kind == kind && w.path == path && w.distro.as_deref().unwrap_or("") == distro.as_deref().unwrap_or("")
    });
    if duplicate {
        return Err("workspace già presente".to_string());
    }

    data.counter += 1;
    let name = if name.trim().is_empty() {
        default_name_for(&path)
    } else {
        name.trim().to_string()
    };
    let ws = Workspace {
        id: format!("ws-{}-{}", now_millis(), data.counter),
        name,
        kind,
        path,
        distro,
        shell: shell
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        created_at: now_millis(),
        launches: Vec::new(),
        worker_permission: default_worker_permission(),
        worktree_setup: None,
        worker_model: None,
        auto_orchestrate: None,
        default_launch: None,
    };
    data.workspaces.push(ws.clone());
    save_to_disk(app, &data.workspaces)?;
    Ok(ws)
}

#[tauri::command]
pub fn rename_workspace(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
    name: String,
) -> Result<Workspace, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("nome mancante".to_string());
    }
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    ws.name = name.to_string();
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

/// Imposta il tetto dei permessi dei worker automatici del workspace.
#[tauri::command]
pub fn set_worker_permission(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
    mode: String,
) -> Result<Workspace, String> {
    let mode = PermissionMode::parse(mode.trim())
        .ok_or_else(|| "permesso non valido (usa plan, default, accept-edits o bypass)".to_string())?;
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    ws.worker_permission = mode.as_str().to_string();
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

/// Imposta il modello di default dei worker automatici (vuoto = quello dell'agente).
#[tauri::command]
pub fn set_worker_model(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
    model: Option<String>,
) -> Result<Workspace, String> {
    let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    if model.as_deref().is_some_and(|m| !crate::automation::valid_model(m)) {
        return Err("modello non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string());
    }
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    ws.worker_model = model;
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

/// Comando di setup dei worktree del workspace (vuoto = nessuno).
#[tauri::command]
pub fn set_worktree_setup(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
    command: Option<String>,
) -> Result<Workspace, String> {
    let command = normalize_setup(command)?;
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    ws.worktree_setup = command;
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

/// Trim; vuoto = nessun comando; massimo `MAX_COMMAND_LEN` caratteri.
fn normalize_setup(command: Option<String>) -> Result<Option<String>, String> {
    let Some(c) = command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if c.chars().count() > MAX_COMMAND_LEN {
        return Err(format!("comando troppo lungo (max {MAX_COMMAND_LEN} caratteri)"));
    }
    if c.chars().any(|ch| ch.is_control() && ch != '\t') {
        return Err("il comando non puo' contenere caratteri di controllo".to_string());
    }
    Ok(Some(c))
}

/// Valida la configurazione dell'orchestratore automatico: l'agente deve
/// supportare le sessioni automatiche (`known` sono gli id), il modello deve
/// avere il formato dei modelli dei task. Modello vuoto = quello dell'agente.
fn normalize_auto(
    config: Option<AutoOrchestrate>,
    known: &[(String, String)],
) -> Result<Option<AutoOrchestrate>, String> {
    let Some(c) = config else {
        return Ok(None);
    };
    let agent = c.agent.trim().to_string();
    if !known.iter().any(|(id, _)| *id == agent) {
        return Err(format!("agente non valido o senza supporto alle sessioni automatiche: {agent}"));
    }
    let model = match c.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) if crate::automation::valid_model(m) => Some(m.to_string()),
        Some(_) => return Err("modello non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string()),
        None => None,
    };
    Ok(Some(AutoOrchestrate { enabled: c.enabled, agent, model }))
}

/// Imposta l'orchestratore automatico del workspace (None = lo rimuove).
#[tauri::command]
pub fn set_auto_orchestrate(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
    config: Option<AutoOrchestrate>,
) -> Result<Workspace, String> {
    let known: Vec<(String, String)> = crate::agents::automation_agents()
        .into_iter()
        .map(|(i, n)| (i.to_string(), n.to_string()))
        .collect();
    let config = normalize_auto(config, &known)?;
    let out = {
        let mut data = state.inner.lock().unwrap();
        let ws = data
            .workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or_else(|| "workspace non trovato".to_string())?;
        ws.auto_orchestrate = config;
        let out = ws.clone();
        save_to_disk(&app, &data.workspaces)?;
        out
    };
    // Fuori dal lock dei workspace: il riallineamento li rilegge.
    let automation = app.state::<crate::automation::AutomationState>();
    automation.reconcile(&crate::automation::TauriHost(app.clone()));
    Ok(out)
}

#[tauri::command]
pub fn remove_workspace(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
) -> Result<(), String> {
    {
        let mut data = state.inner.lock().unwrap();
        let before = data.workspaces.len();
        data.workspaces.retain(|w| w.id != id);
        if data.workspaces.len() == before {
            return Err("workspace non trovato".to_string());
        }
        save_to_disk(&app, &data.workspaces)?;
    }
    // Fuori dal lock dei workspace: la board lo rilegge per schedulare.
    let automation = app.state::<crate::automation::AutomationState>();
    automation.remove_board(&crate::automation::TauriHost(app.clone()), &id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Avvii personalizzati
// ---------------------------------------------------------------------------

/// Trim del comando, troncato a MAX_COMMAND_LEN caratteri (spacing interno
/// preservato: fa parte del comando).
fn sanitize_command(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.chars().count() > MAX_COMMAND_LEN {
        trimmed.chars().take(MAX_COMMAND_LEN).collect()
    } else {
        trimmed.to_string()
    }
}

/// Valida nome e comando di un avvio: ritorna i valori normalizzati.
fn validate_launch(name: &str, command: &str) -> Result<(String, String), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("nome avvio mancante".to_string());
    }
    if name.chars().count() > MAX_LAUNCH_NAME_LEN {
        return Err(format!(
            "nome avvio troppo lungo (max {MAX_LAUNCH_NAME_LEN} caratteri)"
        ));
    }
    let command = sanitize_command(command);
    if command.is_empty() {
        return Err("comando avvio mancante".to_string());
    }
    Ok((name.to_string(), command))
}

fn push_launch(launches: &mut Vec<Launch>, id: String, name: &str, command: &str) -> Result<(), String> {
    let (name, command) = validate_launch(name, command)?;
    launches.push(Launch { id, name, command, is_default: false });
    Ok(())
}

fn edit_launch(launches: &mut [Launch], launch_id: &str, name: &str, command: &str) -> Result<(), String> {
    let (name, command) = validate_launch(name, command)?;
    let l = launches
        .iter_mut()
        .find(|l| l.id == launch_id)
        .ok_or_else(|| "avvio non trovato".to_string())?;
    l.name = name;
    l.command = command;
    Ok(())
}

/// Id ammesso come predefinito del workspace: "shell", un suo avvio o uno
/// integrato.
fn valid_default_launch(ws: &Workspace, id: &str, builtin: &[crate::agents::BuiltinLaunch]) -> bool {
    id == "shell" || ws.launches.iter().any(|l| l.id == id) || builtin.iter().any(|b| b.id == id)
}

#[tauri::command]
pub fn add_launch(
    app: AppHandle,
    state: State<WorkspaceStore>,
    workspace_id: String,
    name: String,
    command: String,
    is_default: bool,
) -> Result<Workspace, String> {
    let mut data = state.inner.lock().unwrap();
    data.counter += 1;
    let id = format!("ln-{}-{}", now_millis(), data.counter);
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    push_launch(&mut ws.launches, id.clone(), &name, &command)?;
    if is_default {
        ws.default_launch = Some(id);
    }
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

#[tauri::command]
pub fn update_launch(
    app: AppHandle,
    state: State<WorkspaceStore>,
    workspace_id: String,
    launch_id: String,
    name: String,
    command: String,
) -> Result<Workspace, String> {
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    edit_launch(&mut ws.launches, &launch_id, &name, &command)?;
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

#[tauri::command]
pub fn remove_launch(
    app: AppHandle,
    state: State<WorkspaceStore>,
    workspace_id: String,
    launch_id: String,
) -> Result<Workspace, String> {
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    let before = ws.launches.len();
    ws.launches.retain(|l| l.id != launch_id);
    if ws.launches.len() == before {
        return Err("avvio non trovato".to_string());
    }
    if ws.default_launch.as_deref() == Some(launch_id.as_str()) {
        ws.default_launch = None;
    }
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

/// Imposta l'avvio predefinito del workspace (None = eredita il globale).
#[tauri::command]
pub fn set_default_launch(
    app: AppHandle,
    state: State<WorkspaceStore>,
    workspace_id: String,
    launch_id: Option<String>,
) -> Result<Workspace, String> {
    let builtin = crate::agents::builtin_launch_list();
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    if let Some(id) = launch_id.as_deref() {
        if !valid_default_launch(ws, id, &builtin) {
            return Err("avvio predefinito non valido".to_string());
        }
    }
    ws.default_launch = launch_id;
    let out = ws.clone();
    save_to_disk(&app, &data.workspaces)?;
    Ok(out)
}

// ---------------------------------------------------------------------------
// Apri la cartella in Esplora risorse
// ---------------------------------------------------------------------------

/// Percorso UNC di una cartella WSL visibile da Windows:
/// (`Ubuntu`, `/home/me/proj`) -> `\\wsl.localhost\Ubuntu\home\me\proj`.
fn wsl_unc_path(distro: &str, path: &str) -> String {
    let rest = path.trim_matches('/').replace('/', "\\");
    if rest.is_empty() {
        format!("\\\\wsl.localhost\\{distro}")
    } else {
        format!("\\\\wsl.localhost\\{distro}\\{rest}")
    }
}

/// Percorso da passare a explorer.exe per il workspace.
fn explorer_target(ws: &Workspace) -> Result<String, String> {
    if ws.kind == "wsl" {
        let distro = ws
            .distro
            .as_deref()
            .filter(|d| !d.is_empty())
            .ok_or_else(|| "distro WSL mancante".to_string())?;
        Ok(wsl_unc_path(distro, &ws.path))
    } else {
        Ok(ws.path.clone())
    }
}

/// Async per coerenza con gli altri comandi che lanciano processi.
#[tauri::command]
pub async fn open_workspace_folder(app: AppHandle, id: String) -> Result<(), String> {
    crate::blocking(move || {
        let ws = app
            .state::<WorkspaceStore>()
            .find(&id)
            .ok_or_else(|| "workspace non trovato".to_string())?;
        let target = explorer_target(&ws)?;
        let mut cmd = std::process::Command::new("explorer.exe");
        cmd.arg(&target);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        // explorer.exe esce spesso con codice 1 anche se la cartella si apre:
        // conta solo il fallimento dello spawn, l'exit code non si guarda.
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("impossibile aprire Esplora risorse: {e}"))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(id: &str, is_default: bool) -> Launch {
        Launch {
            id: id.to_string(),
            name: id.to_string(),
            command: "ls".to_string(),
            is_default,
        }
    }

    #[test]
    fn validate_trims_and_requires_fields() {
        assert_eq!(
            validate_launch("  Dev  ", "  npm run dev  "),
            Ok(("Dev".to_string(), "npm run dev".to_string()))
        );
        assert!(validate_launch("   ", "ls").is_err());
        assert!(validate_launch("Dev", "   ").is_err());
    }

    #[test]
    fn validate_enforces_length_limits() {
        assert!(validate_launch(&"n".repeat(80), "ls").is_ok());
        assert!(validate_launch(&"n".repeat(81), "ls").is_err());
        let (_, cmd) = validate_launch("Dev", &"x".repeat(1500)).unwrap();
        assert_eq!(cmd, "x".repeat(MAX_COMMAND_LEN));
        assert_eq!(MAX_COMMAND_LEN, 1000);
    }

    fn ws_with(launches: Vec<Launch>) -> Workspace {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        ws.launches = launches;
        ws
    }

    #[test]
    fn migrate_default_moves_flag_to_workspace() {
        let mut ws = ws_with(vec![launch("a", false), launch("b", true)]);
        migrate_default(&mut ws);
        assert_eq!(ws.default_launch.as_deref(), Some("b"));
        assert!(ws.launches.iter().all(|l| !l.is_default));
        // Un valore gia' presente non viene sovrascritto.
        let mut ws = ws_with(vec![launch("a", true)]);
        ws.default_launch = Some("shell".into());
        migrate_default(&mut ws);
        assert_eq!(ws.default_launch.as_deref(), Some("shell"));
        assert!(!ws.launches[0].is_default);
        // Nessun flag: resta None.
        let mut ws = ws_with(vec![launch("a", false)]);
        migrate_default(&mut ws);
        assert_eq!(ws.default_launch, None);
    }

    #[test]
    fn old_file_with_is_default_is_read_but_not_written_back() {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1,
            "launches":[{"id":"l1","name":"n","command":"ls","is_default":true}]}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert!(ws.launches[0].is_default);
        migrate_default(&mut ws);
        let out = serde_json::to_string(&ws).unwrap();
        assert!(!out.contains("is_default"));
        assert!(out.contains(r#""default_launch":"l1""#));
    }

    #[test]
    fn default_launch_validation() {
        let ws = ws_with(vec![launch("l1", false)]);
        let builtin = crate::agents::builtin_launch_list();
        assert!(valid_default_launch(&ws, "shell", &builtin));
        assert!(valid_default_launch(&ws, "l1", &builtin));
        assert!(valid_default_launch(&ws, "builtin:claude-code", &builtin));
        assert!(!valid_default_launch(&ws, "l2", &builtin));
        assert!(!valid_default_launch(&ws, "builtin:nope", &builtin));
    }

    #[test]
    fn push_and_edit_launches() {
        let mut v = Vec::new();
        push_launch(&mut v, "a".into(), "A", "ls").unwrap();
        push_launch(&mut v, "b".into(), "B", "pwd").unwrap();
        edit_launch(&mut v, "a", "A2", "dir").unwrap();
        assert_eq!(v[0].name, "A2");
        assert!(edit_launch(&mut v, "zzz", "X", "ls").is_err());
        // Input non valido: nessuna modifica.
        assert!(push_launch(&mut v, "c".into(), "", "ls").is_err());
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn workspace_without_launches_field_deserializes() {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let ws: Workspace = serde_json::from_str(raw).unwrap();
        assert!(ws.launches.is_empty());
        // Senza il campo: il default storico, mai piu' permissivo.
        assert_eq!(ws.worker_permission, "accept-edits");
        assert_eq!(ws.worker_permission_mode(), PermissionMode::AcceptEdits);
    }

    #[test]
    fn worktree_setup_is_trimmed_and_optional() {
        assert_eq!(normalize_setup(None), Ok(None));
        assert_eq!(normalize_setup(Some("   ".into())), Ok(None));
        assert_eq!(normalize_setup(Some("  npm ci  ".into())), Ok(Some("npm ci".into())));
        assert!(normalize_setup(Some("x".repeat(1000))).is_ok());
        assert!(normalize_setup(Some("x".repeat(1001))).is_err());
        assert!(normalize_setup(Some("a\nb".into())).is_err());
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert_eq!(ws.worktree_setup, None);
        assert!(!serde_json::to_string(&ws).unwrap().contains("worktree_setup"));
        ws.worktree_setup = Some("npm ci".into());
        assert!(serde_json::to_string(&ws).unwrap().contains("\"worktree_setup\":\"npm ci\""));
    }

    #[test]
    fn worker_permission_roundtrips_and_bad_value_falls_back() {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1,"worker_permission":"plan"}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert_eq!(ws.worker_permission_mode(), PermissionMode::Plan);
        ws.worker_permission = "boh".to_string();
        assert_eq!(ws.worker_permission_mode(), PermissionMode::AcceptEdits);
    }


    #[test]
    fn worker_model_defaults_to_none_and_roundtrips() {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert_eq!(ws.worker_model, None);
        ws.worker_model = Some("opus".into());
        let back: Workspace = serde_json::from_str(&serde_json::to_string(&ws).unwrap()).unwrap();
        assert_eq!(back.worker_model.as_deref(), Some("opus"));
    }

    #[test]
    fn auto_orchestrate_is_validated_and_optional() {
        let known = vec![("claude-code".to_string(), "Claude Code".to_string())];
        let cfg = |agent: &str, model: Option<&str>| AutoOrchestrate {
            enabled: true,
            agent: agent.to_string(),
            model: model.map(str::to_string),
        };
        assert_eq!(normalize_auto(None, &known), Ok(None));
        assert_eq!(
            normalize_auto(Some(cfg(" claude-code ", Some("  opus "))), &known),
            Ok(Some(cfg("claude-code", Some("opus"))))
        );
        assert_eq!(normalize_auto(Some(cfg("claude-code", Some("  "))), &known), Ok(Some(cfg("claude-code", None))));
        assert!(normalize_auto(Some(cfg("gemini", None)), &known).is_err());
        assert!(normalize_auto(Some(cfg("claude-code", Some("a b"))), &known).is_err());
        assert!(normalize_auto(Some(cfg("claude-code", Some(&"m".repeat(65)))), &known).is_err());
        // Serde: campo assente = nessuna configurazione; modello null.
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert_eq!(ws.auto_orchestrate, None);
        assert!(!serde_json::to_string(&ws).unwrap().contains("auto_orchestrate"));
        ws.auto_orchestrate = Some(cfg("claude-code", None));
        assert!(serde_json::to_string(&ws)
            .unwrap()
            .contains(r#""auto_orchestrate":{"enabled":true,"agent":"claude-code","model":null}"#));
        let c: AutoOrchestrate = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert_eq!((c.enabled, c.agent.as_str(), c.model), (false, "claude-code", None));
    }

    #[test]
    fn wsl_unc_path_uses_backslashes() {
        assert_eq!(
            wsl_unc_path("Ubuntu", "/home/me/proj"),
            "\\\\wsl.localhost\\Ubuntu\\home\\me\\proj"
        );
        assert_eq!(wsl_unc_path("Ubuntu", "/"), "\\\\wsl.localhost\\Ubuntu");
        assert_eq!(
            wsl_unc_path("Ubuntu-24.04", "/home/me/proj/"),
            "\\\\wsl.localhost\\Ubuntu-24.04\\home\\me\\proj"
        );
    }
}
