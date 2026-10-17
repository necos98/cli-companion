use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

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
