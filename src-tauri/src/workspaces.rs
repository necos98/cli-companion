use crate::agents::PermissionMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

mod e2e;
mod settings_patch;
pub use e2e::{E2eConfig, MAX_E2E_TIMEOUT_MIN, MIN_E2E_TIMEOUT_MIN};
pub use settings_patch::{
    apply_settings_patch, validation_error_en, SettingsPatch, MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS,
};

/// Lunghezza massima del nome di un avvio (in caratteri).
pub const MAX_LAUNCH_NAME_LEN: usize = 80;
/// Lunghezza massima del comando di un avvio (in caratteri).
pub const MAX_COMMAND_LEN: usize = 1000;
pub const MAX_RULES_LEN: usize = 4000;
/// Limiti di `RepoConfig`: voci per elenco, lunghezza di un comando e di un percorso.
pub const MAX_REPO_COMMANDS: usize = 30;
pub const MAX_REPO_COMMAND_LEN: usize = 2000;
pub(crate) const MAX_REPO_PATH_LEN: usize = 300;

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
    /// Permessi dei worker automatici: "plan" | "default" | "accept-edits" | "bypass".
    #[serde(default = "default_worker_permission")]
    pub worker_permission: String,
    /// Modello di default dei worker automatici (None = quello dell'agente);
    /// vale quando il task non ne indica uno.
    #[serde(default)]
    pub worker_model: Option<String>,
    /// Regole di permesso Claude Code pre-approvate / vietate per i worker
    /// (es. `Bash(git:*)`), passate con `claude --settings` (vedi `worker_permissions`).
    #[serde(default)]
    pub worker_allow: Vec<String>,
    #[serde(default)]
    pub worker_deny: Vec<String>,
    /// Modalita' di consegna dei task (vedi docs/BOARD.MD).
    #[serde(default)]
    pub delivery: Delivery,
    /// I task di questo workspace partono solo nelle fasce orarie (vedi `schedule`).
    #[serde(default)]
    pub use_schedule: bool,
    /// Avvio predefinito dei nuovi terminali: `None` = eredita il predefinito
    /// globale; "shell" = shell semplice; altrimenti l'id di un avvio del
    /// workspace o di uno integrato (`builtin:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_launch: Option<String>,
    /// Regole per i worker automatici (test vietati, comandi da non lanciare):
    /// `get_task` le accoda alle istruzioni di ogni worker.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub worker_rules: String,
    /// Configurazione per repository (chiave = cartella relativa alla root,
    /// "." = la root): branch base, file ignorati da copiare e comandi di
    /// setup / pulizia dei worktree, eseguiti dall'app.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repos: BTreeMap<String, RepoConfig>,
    /// Archiviato: congelato (nessun task, nessun tab);
    /// i dati restano e si puo' riattivare.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

/// Preparazione e pulizia dei worktree di un repository (vedi docs/BOARD.MD).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    /// Branch base dei task del repository (prima del default di `origin`).
    pub base_branch: Option<String>,
    /// Dove il repository e' montato in un container (es. `/var/www/html`):
    /// ne deriva `$CONTAINER_WORKTREE`.
    pub container_root: Option<String>,
    /// Percorsi (relativi al repository) ignorati da git da copiare nel
    /// worktree appena creato.
    pub copy_ignored: Vec<String>,
    /// Comandi eseguiti nel worktree appena creato, in ordine.
    pub setup: Vec<String>,
    /// Comandi eseguiti nel worktree prima di rimuoverlo.
    pub cleanup: Vec<String>,
    /// `git worktree remove --force`.
    pub remove_force: bool,
    /// Test E2E facoltativi del repository (vedi `E2eConfig`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e2e: Option<E2eConfig>,
}

impl RepoConfig {
    /// Alla creazione del worktree c'e' qualcosa da fare?
    pub fn has_setup(&self) -> bool {
        !self.copy_ignored.is_empty() || !self.setup.is_empty()
    }

    /// Normalizza e valida: branch valido, percorsi relativi senza `..`,
    /// comandi non vuoti su una riga sola.
    pub fn normalized(mut self) -> Result<RepoConfig, String> {
        self.base_branch = match self.base_branch.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()) {
            Some(b) if crate::board::valid_branch(&b) => Some(b),
            Some(b) => return Err(format!("branch base non valido: {b}")),
            None => None,
        };
        self.container_root = match self.container_root.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) {
            Some(c) if c.chars().count() > MAX_REPO_PATH_LEN || c.chars().any(char::is_control) => {
                return Err("percorso nel container non valido".to_string())
            }
            Some(c) => Some(c.trim_end_matches('/').to_string()),
            None => None,
        };
        let mut copy: Vec<String> = Vec::new();
        for raw in &self.copy_ignored {
            let t = raw.trim().replace('\\', "/");
            if t.is_empty() {
                continue;
            }
            let b = t.as_bytes();
            if t.starts_with('/') || (b.len() >= 2 && b[1] == b':') {
                return Err(format!("il percorso da copiare deve essere relativo al repository: {t}"));
            }
            if t.chars().count() > MAX_REPO_PATH_LEN || t.chars().any(char::is_control) {
                return Err(format!("percorso da copiare non valido: {}", t.chars().take(40).collect::<String>()));
            }
            let segs: Vec<&str> = t.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
            if segs.is_empty() || segs.contains(&"..") {
                return Err(format!("percorso da copiare non valido (niente '..' ne' la radice): {t}"));
            }
            let norm = segs.join("/");
            if !copy.contains(&norm) {
                copy.push(norm);
            }
        }
        if copy.len() > MAX_REPO_COMMANDS {
            return Err(format!("troppi percorsi da copiare (max {MAX_REPO_COMMANDS})"));
        }
        self.copy_ignored = copy;
        self.setup = normalize_commands(&self.setup, "setup")?;
        self.cleanup = normalize_commands(&self.cleanup, "pulizia")?;
        self.e2e = self.e2e.map(E2eConfig::normalized).transpose()?;
        Ok(self)
    }
}

/// Elenco di comandi: trim, righe vuote scartate; ciascuno su una riga, al
/// massimo `MAX_REPO_COMMAND_LEN` caratteri, al massimo `MAX_REPO_COMMANDS`.
fn normalize_commands(list: &[String], what: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in list {
        let c = raw.trim();
        if c.is_empty() {
            continue;
        }
        if c.chars().any(|ch| ch.is_control() && ch != '\t') {
            return Err(format!("il comando di {what} deve stare su una riga: {}", c.chars().take(40).collect::<String>()));
        }
        if c.chars().count() > MAX_REPO_COMMAND_LEN {
            return Err(format!("comando di {what} troppo lungo (max {MAX_REPO_COMMAND_LEN} caratteri)"));
        }
        out.push(c.to_string());
    }
    if out.len() > MAX_REPO_COMMANDS {
        return Err(format!("troppi comandi di {what} (max {MAX_REPO_COMMANDS})"));
    }
    Ok(out)
}

/// Chiave di un repository nelle impostazioni: relativa alla root, `/`, "."
/// per la root.
pub fn repo_config_key(repo: &str) -> Result<String, String> {
    crate::board::normalize_repo(repo).map(|r| r.unwrap_or_else(|| ".".to_string()))
}

/// Migra il vecchio flag `is_default` degli avvii in `default_launch` e la voce
/// rapida `builtin:orchestrator` (v0.11) in `builtin:preset:task-intake`.
fn migrate_default(ws: &mut Workspace) {
    if ws.default_launch.is_none() {
        ws.default_launch = ws.launches.iter().find(|l| l.is_default).map(|l| l.id.clone());
    }
    for l in ws.launches.iter_mut() {
        l.is_default = false;
    }
    crate::agents::presets::migrate_launch_id(&mut ws.default_launch);
}

/// Come un task arriva al repository (vedi docs/BOARD.MD, "Consegna").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMode {
    /// Branch + push + PR + merge (`gh`).
    Pr,
    /// Branch + rebase + fast-forward della base locale.
    LocalMerge,
    /// Commit sul branch corrente + `pull --rebase` + push.
    Push,
    /// Solo commit sul branch corrente.
    #[default]
    Commit,
}

impl DeliveryMode {
    pub const ALL: [DeliveryMode; 4] = [DeliveryMode::Pr, DeliveryMode::LocalMerge, DeliveryMode::Push, DeliveryMode::Commit];

    /// Stesso valore della serializzazione.
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryMode::Pr => "pr",
            DeliveryMode::LocalMerge => "local_merge",
            DeliveryMode::Push => "push",
            DeliveryMode::Commit => "commit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    #[default]
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [MergeMethod; 3] = [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];

    pub fn as_str(self) -> &'static str {
        match self {
            MergeMethod::Squash => "squash",
            MergeMethod::Merge => "merge",
            MergeMethod::Rebase => "rebase",
        }
    }
}

/// Impostazione di consegna del workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Delivery {
    pub mode: DeliveryMode,
    /// Un worktree per task (solo `pr` e `local_merge`).
    pub parallel: bool,
    /// Solo per `pr`.
    pub merge_method: MergeMethod,
}

impl Delivery {
    /// La modalita' lavora su un branch `task/<id>`.
    pub fn uses_branches(&self) -> bool {
        matches!(self.mode, DeliveryMode::Pr | DeliveryMode::LocalMerge)
    }

    /// Ogni task ha il suo worktree (e i task possono andare in parallelo).
    pub fn worktrees(&self) -> bool {
        self.parallel && self.uses_branches()
    }

    /// `parallel` ha senso solo con i branch.
    pub(crate) fn normalized(mut self) -> Delivery {
        self.parallel &= self.uses_branches();
        self
    }
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
    /// Serializza i salvataggi su disco (vedi `persist`).
    save: Mutex<()>,
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
    match parse_workspaces(&raw) {
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

/// Legge il file dei workspace migrando i campi delle versioni precedenti:
/// `auto_orchestrate.enabled` -> `use_schedule`; `worktree_setup` ->
/// `repos["."].setup` (se vuoto). Entrambi i campi spariscono.
fn parse_workspaces(raw: &str) -> Result<Vec<Workspace>, serde_json::Error> {
    let mut list: Vec<serde_json::Value> = serde_json::from_str(raw)?;
    list.iter_mut().for_each(migrate_legacy_fields);
    serde_json::from_value(serde_json::Value::Array(list))
}

fn migrate_legacy_fields(ws: &mut serde_json::Value) {
    let Some(obj) = ws.as_object_mut() else {
        return;
    };
    let enabled = obj
        .remove("auto_orchestrate")
        .and_then(|a| a.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    if enabled {
        obj.insert("use_schedule".into(), true.into());
    }
    let setup = obj
        .remove("worktree_setup")
        .and_then(|v| v.as_str().map(|c| c.trim().to_string()))
        .filter(|c| !c.is_empty());
    if let Some(cmd) = setup {
        let repos = obj.entry("repos").or_insert_with(|| serde_json::json!({}));
        if let Some(map) = repos.as_object_mut() {
            let root = map.entry(".").or_insert_with(|| serde_json::json!({}));
            if let Some(r) = root.as_object_mut() {
                let empty = r.get("setup").and_then(serde_json::Value::as_array).is_none_or(Vec::is_empty);
                if empty {
                    r.insert("setup".into(), serde_json::json!([cmd]));
                }
            }
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
            return Err("WSL distro missing".to_string());
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

    persist(app, |data| {
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
        worker_model: None,
        worker_allow: Vec::new(),
        worker_deny: Vec::new(),
        delivery: Delivery::default(),
        use_schedule: false,
        default_launch: None,
        worker_rules: String::new(),
        repos: BTreeMap::new(),
        archived: false,
    };
    data.workspaces.push(ws.clone());
    Ok(ws)
    })
}

/// Applica `f` ai dati e salva su disco SENZA tenere il lock dei workspace
/// durante l'I/O: `save` serializza le scritture (snapshot e scrittura sotto lo
/// stesso mutex, cosi' uno snapshot vecchio non sovrascrive uno piu' nuovo),
/// mentre `inner` resta libero per i lettori.
fn persist<T>(
    app: &AppHandle,
    f: impl FnOnce(&mut StoreData) -> Result<T, String>,
) -> Result<T, String> {
    let store = app.state::<WorkspaceStore>();
    let _save = store.save.lock().unwrap();
    let (out, snapshot) = {
        let mut data = store.inner.lock().unwrap();
        let out = f(&mut data)?;
        (out, data.workspaces.clone())
    };
    save_to_disk(app, &snapshot)?;
    Ok(out)
}

fn ws_mut<'a>(data: &'a mut StoreData, id: &str) -> Result<&'a mut Workspace, String> {
    find_ws_mut(&mut data.workspaces, id)
}

fn find_ws_mut<'a>(workspaces: &'a mut [Workspace], id: &str) -> Result<&'a mut Workspace, String> {
    workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())
}

/// Permesso dei worker da testo (trim): "plan", "default", "accept-edits", "bypass".
fn parse_worker_permission(mode: &str) -> Result<PermissionMode, String> {
    PermissionMode::parse(mode.trim())
        .ok_or_else(|| "permesso non valido (usa plan, default, accept-edits o bypass)".to_string())
}

/// Modello dei worker: trim, vuoto = quello dell'agente.
fn normalize_worker_model(model: Option<String>) -> Result<Option<String>, String> {
    let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    if model.as_deref().is_some_and(|m| !crate::automation::valid_model(m)) {
        return Err("modello non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string());
    }
    Ok(model)
}

/// Etichette (nei messaggi) delle due liste di regole di permesso.
const ALLOW_RULES_LABEL: &str = "Consentiti";
const DENY_RULES_LABEL: &str = "Vietati";

/// Valida una lista di regole di permesso; l'errore porta l'etichetta della lista.
fn normalize_permission_rules(rules: &[String], label: &str) -> Result<Vec<String>, String> {
    crate::worker_permissions::validate_rules(rules).map_err(|e| format!("{label}: {e}"))
}

/// Dopo il salvataggio di consegna / fasce orarie lo scheduler va riallineato.
/// Da chiamare fuori dal lock dei workspace: lo scheduler li rilegge.
pub(crate) fn reconcile_automation(app: &AppHandle) {
    let automation = app.state::<crate::automation::AutomationState>();
    automation.reconcile(&crate::automation::TauriHost(app.clone()));
}

#[tauri::command]
pub async fn rename_workspace(app: AppHandle, id: String, name: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("nome mancante".to_string());
        }
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.name = name;
            Ok(ws.clone())
        })
    })
    .await
}

/// Imposta il tetto dei permessi dei worker automatici del workspace.
#[tauri::command]
pub async fn set_worker_permission(app: AppHandle, id: String, mode: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let mode = parse_worker_permission(&mode)?;
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.worker_permission = mode.as_str().to_string();
            Ok(ws.clone())
        })
    })
    .await
}

/// Imposta le regole di permesso pre-approvate (`allow`) e vietate (`deny`)
/// dei worker Claude Code del workspace (sintassi dei permessi di Claude Code).
#[tauri::command]
pub async fn set_worker_permission_rules(
    app: AppHandle,
    id: String,
    allow: Vec<String>,
    deny: Vec<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let allow = normalize_permission_rules(&allow, ALLOW_RULES_LABEL)?;
        let deny = normalize_permission_rules(&deny, DENY_RULES_LABEL)?;
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.worker_allow = allow;
            ws.worker_deny = deny;
            Ok(ws.clone())
        })
    })
    .await
}

/// Imposta il modello di default dei worker automatici (vuoto = quello dell'agente).
#[tauri::command]
pub async fn set_worker_model(app: AppHandle, id: String, model: Option<String>) -> Result<Workspace, String> {
    crate::blocking(move || {
        let model = normalize_worker_model(model)?;
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.worker_model = model;
            Ok(ws.clone())
        })
    })
    .await
}

/// Imposta la modalita' di consegna (vale per i task che partono da ora).
#[tauri::command]
pub async fn set_delivery(app: AppHandle, id: String, delivery: Delivery) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.delivery = delivery.normalized();
            Ok(ws.clone())
        })?;
        reconcile_automation(&app);
        Ok(out)
    })
    .await
}

/// I task del workspace partono solo nelle fasce orarie.
#[tauri::command]
pub async fn set_use_schedule(app: AppHandle, id: String, value: bool) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.use_schedule = value;
            Ok(ws.clone())
        })?;
        reconcile_automation(&app);
        Ok(out)
    })
    .await
}

/// Regole per i worker del workspace (vuoto = nessuna).
#[tauri::command]
pub async fn set_worker_rules(app: AppHandle, workspace_id: String, rules: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let rules = normalize_rules(&rules)?;
        persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            ws.worker_rules = rules;
            Ok(ws.clone())
        })
    })
    .await
}

/// Configurazione di un repository del workspace (`None` la rimuove).
#[tauri::command]
pub async fn set_repo_config(
    app: AppHandle,
    workspace_id: String,
    repo: String,
    config: Option<RepoConfig>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let key = repo_config_key(&repo)?;
        let config = config.map(RepoConfig::normalized).transpose()?;
        persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            match config {
                Some(c) => ws.repos.insert(key, c),
                None => ws.repos.remove(&key),
            };
            Ok(ws.clone())
        })
    })
    .await
}

/// Trim; massimo `MAX_RULES_LEN` caratteri; niente caratteri di controllo
/// a parte a capo e tabulazione.
pub(crate) fn normalize_rules(rules: &str) -> Result<String, String> {
    let r = rules.trim();
    if r.chars().count() > MAX_RULES_LEN {
        return Err(format!("regole troppo lunghe (max {MAX_RULES_LEN} caratteri)"));
    }
    if r.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Err("le regole non possono contenere caratteri di controllo".to_string());
    }
    Ok(r.to_string())
}

/// Nuovo ordine dei workspace: `ids` elenca tutti gli id o solo un
/// sottoinsieme (i non elencati seguono, nel loro ordine relativo).
fn apply_order(list: &mut Vec<Workspace>, ids: &[String]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !list.iter().any(|w| &w.id == id) {
            return Err("workspace non trovato".to_string());
        }
        if !seen.insert(id.as_str()) {
            return Err("id duplicato".to_string());
        }
    }
    let mut rest = std::mem::take(list);
    for id in ids {
        if let Some(i) = rest.iter().position(|w| &w.id == id) {
            list.push(rest.remove(i));
        }
    }
    list.extend(rest);
    Ok(())
}

/// Riordina i workspace e salva; ritorna l'elenco completo.
#[tauri::command]
pub async fn reorder_workspaces(app: AppHandle, ids: Vec<String>) -> Result<Vec<Workspace>, String> {
    crate::blocking(move || {
        persist(&app, |data| {
            apply_order(&mut data.workspaces, &ids)?;
            Ok(data.workspaces.clone())
        })
    })
    .await
}

/// Archivia / riattiva un workspace. Archiviato = congelato: la board non
/// avvia task; i task in corso tornano in coda
/// (i tab li chiude il frontend).
#[tauri::command]
pub async fn set_workspace_archived(app: AppHandle, workspace_id: String, archived: bool) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            ws.archived = archived;
            Ok(ws.clone())
        })?;
        // Fuori dal lock dei workspace: il registro li rilegge.
        let automation = app.state::<crate::automation::AutomationState>();
        automation.set_archived(&crate::automation::TauriHost(app.clone()), &workspace_id, archived);
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn remove_workspace(app: AppHandle, id: String) -> Result<(), String> {
    crate::blocking(move || {
        persist(&app, |data| {
            let before = data.workspaces.len();
            data.workspaces.retain(|w| w.id != id);
            if data.workspaces.len() == before {
                return Err("workspace non trovato".to_string());
            }
            Ok(())
        })?;
        // Fuori dal lock dei workspace: la board lo rilegge per schedulare.
        let automation = app.state::<crate::automation::AutomationState>();
        automation.remove_board(&crate::automation::TauriHost(app.clone()), &id);
        Ok(())
    })
    .await
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

/// Id ammesso come predefinito del workspace: "shell", uno degli avvii dati o
/// uno integrato.
fn valid_default_launch(launches: &[Launch], id: &str, builtin: &[crate::agents::BuiltinLaunch]) -> bool {
    id == "shell" || launches.iter().any(|l| l.id == id) || builtin.iter().any(|b| b.id == id)
}

/// Come `valid_default_launch`, con il messaggio dell'interfaccia.
fn check_default_launch(
    launches: &[Launch],
    id: &str,
    builtin: &[crate::agents::BuiltinLaunch],
) -> Result<(), String> {
    if valid_default_launch(launches, id, builtin) {
        Ok(())
    } else {
        Err("avvio predefinito non valido".to_string())
    }
}

/// Nuovo id di un avvio; il contatore garantisce l'unicita' nello stesso millisecondo.
fn next_launch_id(counter: &mut u64) -> String {
    *counter += 1;
    format!("ln-{}-{}", now_millis(), counter)
}

/// L'avvio `removed` non c'e' piu': se era il predefinito, si torna a ereditare.
fn clear_default_if_removed(ws: &mut Workspace, removed: &str) {
    if ws.default_launch.as_deref() == Some(removed) {
        ws.default_launch = None;
    }
}

#[tauri::command]
pub async fn add_launch(
    app: AppHandle,
    workspace_id: String,
    name: String,
    command: String,
    is_default: bool,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        persist(&app, |data| {
            let id = next_launch_id(&mut data.counter);
            let ws = ws_mut(data, &workspace_id)?;
            push_launch(&mut ws.launches, id.clone(), &name, &command)?;
            if is_default {
                ws.default_launch = Some(id);
            }
            Ok(ws.clone())
        })
    })
    .await
}

#[tauri::command]
pub async fn update_launch(
    app: AppHandle,
    workspace_id: String,
    launch_id: String,
    name: String,
    command: String,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            edit_launch(&mut ws.launches, &launch_id, &name, &command)?;
            Ok(ws.clone())
        })
    })
    .await
}

#[tauri::command]
pub async fn remove_launch(app: AppHandle, workspace_id: String, launch_id: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            let before = ws.launches.len();
            ws.launches.retain(|l| l.id != launch_id);
            if ws.launches.len() == before {
                return Err("avvio non trovato".to_string());
            }
            clear_default_if_removed(ws, &launch_id);
            Ok(ws.clone())
        })
    })
    .await
}

/// Imposta l'avvio predefinito del workspace (None = eredita il globale).
#[tauri::command]
pub async fn set_default_launch(
    app: AppHandle,
    workspace_id: String,
    launch_id: Option<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let builtin = crate::agents::builtin_launch_list();
        persist(&app, |data| {
            let ws = ws_mut(data, &workspace_id)?;
            if let Some(id) = launch_id.as_deref() {
                check_default_launch(&ws.launches, id, &builtin)?;
            }
            ws.default_launch = launch_id;
            Ok(ws.clone())
        })
    })
    .await
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
            .ok_or_else(|| "WSL distro missing".to_string())?;
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
    #[test]
    fn worker_rules_are_trimmed_and_limited() {
        assert_eq!(super::normalize_rules("  no tests\nno npm  ").unwrap(), "no tests\nno npm");
        assert_eq!(super::normalize_rules("   ").unwrap(), "");
        assert!(super::normalize_rules(&"a".repeat(4000)).is_ok());
        assert!(super::normalize_rules(&"a".repeat(4001)).is_err());
        assert!(super::normalize_rules("a\u{0}b").is_err());
    }

    use super::*;

    fn rc(f: impl FnOnce(&mut RepoConfig)) -> Result<RepoConfig, String> {
        let mut c = RepoConfig::default();
        f(&mut c);
        c.normalized()
    }

    #[test]
    fn repo_config_is_normalized() {
        let c = rc(|c| {
            c.base_branch = Some("  develop ".into());
            c.container_root = Some(" /var/www/html/ ".into());
            c.copy_ignored = vec![" configuration.php ".into(), "".into(), ".\\public\\api\\.env".into(), "configuration.php".into()];
            c.setup = vec!["  composer dump-autoload ".into(), "   ".into()];
            c.cleanup = vec!["rm -rf x\ty".into()];
        })
        .unwrap();
        assert_eq!(c.base_branch.as_deref(), Some("develop"));
        assert_eq!(c.container_root.as_deref(), Some("/var/www/html"));
        assert_eq!(c.copy_ignored, ["configuration.php", "public/api/.env"]);
        assert_eq!(c.setup, ["composer dump-autoload"]);
        assert_eq!(c.cleanup, ["rm -rf x\ty"]);
        // Vuoti diventano None.
        let c = rc(|c| {
            c.base_branch = Some("  ".into());
            c.container_root = Some("".into());
        })
        .unwrap();
        assert!(c.base_branch.is_none() && c.container_root.is_none() && !c.has_setup());
    }

    #[test]
    fn repo_config_rejects_bad_input() {
        for b in ["-x", "a b", "a..b", "x/", "/x"] {
            assert!(rc(|c| c.base_branch = Some(b.into())).is_err(), "{b}");
        }
        for p in ["../x", "a/../../b", "/etc/passwd", "C:\\x", "C:/x", "..", ".", "a\0b"] {
            assert!(rc(|c| c.copy_ignored = vec![p.into()]).is_err(), "{p}");
        }
        assert!(rc(|c| c.setup = vec!["a\nb".into()]).is_err());
        assert!(rc(|c| c.cleanup = vec!["a\rb".into()]).is_err());
        assert!(rc(|c| c.setup = vec!["x".repeat(MAX_REPO_COMMAND_LEN + 1)]).is_err());
        assert!(rc(|c| c.setup = vec!["x".repeat(MAX_REPO_COMMAND_LEN)]).is_ok());
        assert!(rc(|c| c.setup = vec!["x".into(); MAX_REPO_COMMANDS + 1]).is_err());
        assert!(rc(|c| c.cleanup = vec!["x".into(); MAX_REPO_COMMANDS]).is_ok());
        assert!(rc(|c| c.container_root = Some("/a\nb".into())).is_err());
    }

    #[test]
    fn repo_config_keys_and_persistence() {
        assert_eq!(repo_config_key(" DDB/ ").unwrap(), "DDB");
        assert_eq!(repo_config_key(".").unwrap(), ".");
        assert_eq!(repo_config_key("a\\b").unwrap(), "a/b");
        assert!(repo_config_key("../x").is_err() && repo_config_key("/x").is_err() && repo_config_key("").is_err());
        // Un workspace vecchio, senza il campo, si legge; quello vuoto non lo scrive.
        let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1}"#;
        let mut ws: Workspace = serde_json::from_str(raw).unwrap();
        assert!(ws.repos.is_empty());
        assert!(!serde_json::to_string(&ws).unwrap().contains("repos"));
        ws.repos.insert("DDB".into(), RepoConfig { setup: vec!["x".into()], remove_force: true, ..Default::default() });
        let back: Workspace = serde_json::from_str(&serde_json::to_string(&ws).unwrap()).unwrap();
        assert_eq!(back.repos["DDB"].setup, ["x"]);
        assert!(back.repos["DDB"].remove_force);
    }

    #[test]
    fn repo_config_e2e_is_validated_optional_and_persisted() {
        let e2e = |command: &str, junit: &str| E2eConfig { command: command.into(), junit: junit.into(), timeout_min: 20 };
        let c = rc(|c| c.e2e = Some(e2e(" npm run e2e ", "out\\j.xml"))).unwrap();
        assert_eq!(c.e2e, Some(e2e("npm run e2e", "out/j.xml")));
        assert!(rc(|c| c.e2e = Some(e2e("", "j.xml"))).is_err());
        assert!(rc(|c| c.e2e = Some(e2e("x", "../j.xml"))).is_err());
        // Una configurazione senza e2e, o con e2e senza timeout, si legge; senza e2e non lo scrive.
        let old: RepoConfig = serde_json::from_str(r#"{"setup":["s"]}"#).unwrap();
        assert!(old.e2e.is_none() && !serde_json::to_string(&old).unwrap().contains("e2e"));
        let new: RepoConfig = serde_json::from_str(r#"{"e2e":{"command":"c","junit":"j.xml"}}"#).unwrap();
        assert_eq!(new.e2e.as_ref().map(|e| e.timeout_min), Some(20));
        let back: RepoConfig = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
        assert_eq!(back, new);
    }

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
    fn orchestrator_default_launch_becomes_task_intake() {
        let raw = r#"[{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1,
            "default_launch":"builtin:orchestrator"},
            {"id":"ws-2","name":"q","kind":"windows","path":"C:\\q","created_at":1,
            "default_launch":"builtin:claude-code"}]"#;
        let mut list = parse_workspaces(raw).unwrap();
        list.iter_mut().for_each(migrate_default);
        assert_eq!(list[0].default_launch.as_deref(), Some("builtin:preset:task-intake"));
        assert_eq!(list[1].default_launch.as_deref(), Some("builtin:claude-code"));
        let builtin = crate::agents::builtin_launch_list();
        assert!(valid_default_launch(&list[0].launches, "builtin:preset:task-intake", &builtin));
        assert!(!valid_default_launch(&list[0].launches, "builtin:orchestrator", &builtin));
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
        assert!(valid_default_launch(&ws.launches, "shell", &builtin));
        assert!(valid_default_launch(&ws.launches, "l1", &builtin));
        assert!(valid_default_launch(&ws.launches, "builtin:claude-code", &builtin));
        assert!(!valid_default_launch(&ws.launches, "l2", &builtin));
        assert!(!valid_default_launch(&ws.launches, "builtin:nope", &builtin));
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

    fn ws_id(id: &str) -> Workspace {
        let mut w = ws_with(Vec::new());
        w.id = id.to_string();
        w
    }

    fn order(list: &[Workspace]) -> Vec<&str> {
        list.iter().map(|w| w.id.as_str()).collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reorder_applies_a_permutation_or_a_visible_subset() {
        let mut list = vec![ws_id("a"), ws_id("b"), ws_id("c"), ws_id("d")];
        apply_order(&mut list, &ids(&["d", "c", "b", "a"])).unwrap();
        assert_eq!(order(&list), ["d", "c", "b", "a"]);
        // Sottoinsieme: i non elencati seguono nel loro ordine relativo.
        apply_order(&mut list, &ids(&["a", "c"])).unwrap();
        assert_eq!(order(&list), ["a", "c", "d", "b"]);
        apply_order(&mut list, &[]).unwrap();
        assert_eq!(order(&list), ["a", "c", "d", "b"]);
    }

    #[test]
    fn reorder_rejects_unknown_and_duplicate_ids_without_changes() {
        let mut list = vec![ws_id("a"), ws_id("b")];
        assert!(apply_order(&mut list, &ids(&["b", "x"])).is_err());
        assert!(apply_order(&mut list, &ids(&["b", "b"])).is_err());
        assert_eq!(order(&list), ["a", "b"]);
    }

    #[test]
    fn archived_defaults_to_false_and_is_written_only_when_set() {
        let mut ws = ws_with(Vec::new());
        assert!(!ws.archived);
        assert!(!serde_json::to_string(&ws).unwrap().contains("archived"));
        ws.archived = true;
        let raw = serde_json::to_string(&ws).unwrap();
        assert!(raw.contains("\"archived\":true"));
        assert!(serde_json::from_str::<Workspace>(&raw).unwrap().archived);
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

    #[test]
    fn delivery_defaults_and_normalization() {
        let ws = ws_with(Vec::new());
        assert_eq!(ws.delivery, Delivery { mode: DeliveryMode::Commit, parallel: false, merge_method: MergeMethod::Squash });
        assert!(!ws.use_schedule);
        let d: Delivery = serde_json::from_str(r#"{"mode":"local_merge","parallel":true}"#).unwrap();
        assert!(d.worktrees() && d.uses_branches());
        assert_eq!(d.merge_method, MergeMethod::Squash);
        // `parallel` non ha senso senza branch.
        let push = Delivery { mode: DeliveryMode::Push, parallel: true, ..Default::default() };
        assert!(!push.worktrees() && !push.normalized().parallel);
        let pr: Delivery = serde_json::from_str(r#"{"mode":"pr","merge_method":"rebase"}"#).unwrap();
        assert!(pr.uses_branches() && !pr.worktrees());
        assert_eq!(serde_json::to_value(pr).unwrap()["mode"], "pr");
        assert!(serde_json::from_str::<Delivery>(r#"{"mode":"yolo"}"#).is_err());
    }

    #[test]
    fn legacy_fields_are_migrated_and_dropped() {
        let raw = r#"[
            {"id":"a","name":"a","kind":"windows","path":"C:/a","created_at":1,
             "auto_orchestrate":{"enabled":true,"agent":"claude-code"},"worktree_setup":"  npm ci "},
            {"id":"b","name":"b","kind":"windows","path":"C:/b","created_at":1,
             "auto_orchestrate":{"enabled":false},"worktree_setup":"npm ci",
             "repos":{".":{"setup":["make"]}}},
            {"id":"c","name":"c","kind":"windows","path":"C:/c","created_at":1}
        ]"#;
        let list = parse_workspaces(raw).unwrap();
        assert!(list[0].use_schedule && !list[1].use_schedule && !list[2].use_schedule);
        // worktree_setup diventa il setup della root solo se questo e' vuoto.
        assert_eq!(list[0].repos["."].setup, ["npm ci"]);
        assert_eq!(list[1].repos["."].setup, ["make"]);
        assert!(list[2].repos.is_empty());
        let out = serde_json::to_string(&list).unwrap();
        assert!(!out.contains("worktree_setup") && !out.contains("auto_orchestrate"));
        assert!(parse_workspaces("{").is_err());
    }
}
