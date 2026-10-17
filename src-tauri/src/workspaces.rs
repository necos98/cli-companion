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
    /// Al massimo un avvio per workspace e' quello predefinito.
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        Ok(workspaces) => {
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

#[tauri::command]
pub fn remove_workspace(
    app: AppHandle,
    state: State<WorkspaceStore>,
    id: String,
) -> Result<(), String> {
    let mut data = state.inner.lock().unwrap();
    let before = data.workspaces.len();
    data.workspaces.retain(|w| w.id != id);
    if data.workspaces.len() == before {
        return Err("workspace non trovato".to_string());
    }
    save_to_disk(&app, &data.workspaces)?;
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

/// Imposta il flag predefinito di `id`; se `is_default` e' vero toglie il
/// flag a tutti gli altri (al massimo un predefinito per workspace).
fn set_default(launches: &mut [Launch], id: &str, is_default: bool) {
    for l in launches.iter_mut() {
        if l.id == id {
            l.is_default = is_default;
        } else if is_default {
            l.is_default = false;
        }
    }
}

fn push_launch(
    launches: &mut Vec<Launch>,
    id: String,
    name: &str,
    command: &str,
    is_default: bool,
) -> Result<(), String> {
    let (name, command) = validate_launch(name, command)?;
    launches.push(Launch {
        id: id.clone(),
        name,
        command,
        is_default: false,
    });
    set_default(launches, &id, is_default);
    Ok(())
}

fn edit_launch(
    launches: &mut [Launch],
    launch_id: &str,
    name: &str,
    command: &str,
    is_default: bool,
) -> Result<(), String> {
    let (name, command) = validate_launch(name, command)?;
    let l = launches
        .iter_mut()
        .find(|l| l.id == launch_id)
        .ok_or_else(|| "avvio non trovato".to_string())?;
    l.name = name;
    l.command = command;
    set_default(launches, launch_id, is_default);
    Ok(())
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
    push_launch(&mut ws.launches, id, &name, &command, is_default)?;
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
    is_default: bool,
) -> Result<Workspace, String> {
    let mut data = state.inner.lock().unwrap();
    let ws = data
        .workspaces
        .iter_mut()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    edit_launch(&mut ws.launches, &launch_id, &name, &command, is_default)?;
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

    #[test]
    fn setting_default_clears_the_others() {
        let mut v = vec![launch("a", true), launch("b", false), launch("c", false)];
        set_default(&mut v, "b", true);
        let defaults: Vec<_> = v.iter().filter(|l| l.is_default).map(|l| &l.id).collect();
        assert_eq!(defaults, vec!["b"]);
        // Togliere il predefinito non tocca gli altri.
        set_default(&mut v, "b", false);
        assert!(v.iter().all(|l| !l.is_default));
    }

    #[test]
    fn push_and_edit_keep_single_default() {
        let mut v = Vec::new();
        push_launch(&mut v, "a".into(), "A", "ls", true).unwrap();
        push_launch(&mut v, "b".into(), "B", "pwd", true).unwrap();
        assert!(!v[0].is_default && v[1].is_default);
        edit_launch(&mut v, "a", "A2", "dir", true).unwrap();
        assert_eq!(v[0].name, "A2");
        assert!(v[0].is_default && !v[1].is_default);
        assert!(edit_launch(&mut v, "zzz", "X", "ls", false).is_err());
        // Input non valido: nessuna modifica.
        assert!(push_launch(&mut v, "c".into(), "", "ls", false).is_err());
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn workspace_without_launches_field_deserializes() {
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let ws: Workspace = serde_json::from_str(raw).unwrap();
        assert!(ws.launches.is_empty());
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
