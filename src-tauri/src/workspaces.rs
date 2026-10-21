use crate::agents::PermissionMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, State};

/// Lunghezza massima del nome di un avvio (in caratteri).
pub const MAX_LAUNCH_NAME_LEN: usize = 80;
/// Lunghezza massima del comando di un avvio (in caratteri).
pub const MAX_COMMAND_LEN: usize = 1000;
pub const MAX_RULES_LEN: usize = 4000;
/// Limiti di `RepoConfig`: voci per elenco, lunghezza di un comando e di un percorso.
pub const MAX_REPO_COMMANDS: usize = 30;
pub const MAX_REPO_COMMAND_LEN: usize = 2000;
const MAX_REPO_PATH_LEN: usize = 300;

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
    /// Regole di permesso Claude Code pre-approvate / vietate per i worker
    /// (es. `Bash(git:*)`), passate con `claude --settings` (vedi `worker_permissions`).
    #[serde(default)]
    pub worker_allow: Vec<String>,
    #[serde(default)]
    pub worker_deny: Vec<String>,
    /// Orchestratore automatico nelle finestre di esecuzione (vedi `schedule`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_orchestrate: Option<AutoOrchestrate>,
    /// Avvio predefinito dei nuovi terminali: `None` = eredita il predefinito
    /// globale; "shell" = shell semplice; altrimenti l'id di un avvio del
    /// workspace o di uno integrato (`builtin:...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_launch: Option<String>,
    /// Regole per i worker automatici (test vietati, comandi da non lanciare):
    /// `get_task` le accoda alle istruzioni di ogni worker, di board o ad-hoc.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub worker_rules: String,
    /// Configurazione per repository (chiave = cartella relativa alla root,
    /// "." = la root): branch base, file ignorati da copiare e comandi di
    /// setup / pulizia dei worktree, eseguiti dall'app.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repos: BTreeMap<String, RepoConfig>,
    /// Archiviato: congelato (nessun task, nessun orchestratore, nessun tab);
    /// i dati restano e si puo' riattivare.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

/// Preparazione e pulizia dei worktree di un repository (vedi docs/BOARD.MD).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    /// Branch base dei task del repository (dopo `base_branches` della board).
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
        Ok(self)
    }
}

/// Un campo `Option<Option<T>>` distingue "assente" (nessuna modifica) da
/// `null` (azzera il valore).
fn de_double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Massimo di repository in una proposta di impostazioni.
const MAX_PATCH_REPOS: usize = 20;

/// Modifica parziale della configurazione di un repository: solo i campi
/// presenti cambiano (`null` su `base_branch` / `container_root` li azzera).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoPatch {
    #[serde(default, deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<Option<String>>,
    #[serde(default, deserialize_with = "de_double_option", skip_serializing_if = "Option::is_none")]
    pub container_root: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_ignored: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_force: Option<bool>,
}

impl RepoPatch {
    pub fn is_empty(&self) -> bool {
        *self == RepoPatch::default()
    }

    /// `base` con i campi della patch sovrapposti, poi normalizzato.
    fn merge(&self, base: &RepoConfig) -> Result<RepoConfig, String> {
        let mut c = base.clone();
        if let Some(v) = &self.base_branch {
            c.base_branch = v.clone();
        }
        if let Some(v) = &self.container_root {
            c.container_root = v.clone();
        }
        if let Some(v) = &self.copy_ignored {
            c.copy_ignored = v.clone();
        }
        if let Some(v) = &self.setup {
            c.setup = v.clone();
        }
        if let Some(v) = &self.cleanup {
            c.cleanup = v.clone();
        }
        if let Some(v) = self.remove_force {
            c.remove_force = v;
        }
        c.normalized()
    }

    /// Gli stessi campi della patch, con i valori di `cfg`.
    fn project(&self, cfg: &RepoConfig) -> RepoPatch {
        RepoPatch {
            base_branch: self.base_branch.as_ref().map(|_| cfg.base_branch.clone()),
            container_root: self.container_root.as_ref().map(|_| cfg.container_root.clone()),
            copy_ignored: self.copy_ignored.as_ref().map(|_| cfg.copy_ignored.clone()),
            setup: self.setup.as_ref().map(|_| cfg.setup.clone()),
            cleanup: self.cleanup.as_ref().map(|_| cfg.cleanup.clone()),
            remove_force: self.remove_force.map(|_| cfg.remove_force),
        }
    }
}

/// Modifica proposta alle impostazioni del workspace: SOLO regole dei worker
/// e configurazione per repository (mai permessi ne' comandi consentiti).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_rules: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repos: Option<BTreeMap<String, RepoPatch>>,
}

impl SettingsPatch {
    pub fn is_empty(&self) -> bool {
        self.workspace_rules.is_none() && self.repos.is_none()
    }

    /// Valida come le impostazioni dell'interfaccia e ritorna la patch con
    /// chiavi e valori normalizzati. Errori in italiano (come l'interfaccia).
    pub fn normalized(&self, ws: &Workspace) -> Result<SettingsPatch, String> {
        let workspace_rules = self.workspace_rules.as_deref().map(normalize_rules).transpose()?;
        let repos = match &self.repos {
            None => None,
            Some(map) => {
                if map.len() > MAX_PATCH_REPOS {
                    return Err(format!("too many repos in the proposal (max {MAX_PATCH_REPOS})"));
                }
                let mut out = BTreeMap::new();
                for (raw, p) in map {
                    let key = repo_config_key(raw)?;
                    if out.contains_key(&key) {
                        return Err(format!("duplicate repo key: {key}"));
                    }
                    if p.is_empty() {
                        return Err(format!("repos['{key}'] has no fields"));
                    }
                    let base = ws.repos.get(&key).cloned().unwrap_or_default();
                    let merged = p.merge(&base)?;
                    out.insert(key, p.project(&merged));
                }
                Some(out)
            }
        };
        Ok(SettingsPatch { workspace_rules, repos })
    }

    /// Valori attuali dei soli campi toccati (per il confronto nell'interfaccia).
    pub fn current(&self, ws: &Workspace) -> SettingsPatch {
        SettingsPatch {
            workspace_rules: self.workspace_rules.as_ref().map(|_| ws.worker_rules.clone()),
            repos: self.repos.as_ref().map(|m| {
                m.iter()
                    .map(|(k, p)| (k.clone(), p.project(&ws.repos.get(k).cloned().unwrap_or_default())))
                    .collect()
            }),
        }
    }

    /// Applica la patch tutto-o-niente: ogni valore si valida su una copia.
    pub fn apply(&self, ws: &mut Workspace) -> Result<(), String> {
        let norm = self.normalized(ws)?;
        let mut next = ws.clone();
        if let Some(r) = norm.workspace_rules {
            next.worker_rules = r;
        }
        for (k, p) in norm.repos.unwrap_or_default() {
            let base = next.repos.get(&k).cloned().unwrap_or_default();
            next.repos.insert(k, p.merge(&base)?);
        }
        *ws = next;
        Ok(())
    }
}

/// Messaggio di validazione (italiano, dell'interfaccia) in inglese per il
/// modello; i messaggi gia' inglesi passano invariati.
pub fn validation_error_en(e: &str) -> String {
    const MAP: [(&str, &str); 13] = [
        ("regole troppo lunghe", "workspace_rules too long (max 4000 characters)"),
        ("le regole non possono", "workspace_rules cannot contain control characters"),
        ("branch base non valido", "invalid base_branch"),
        ("percorso nel container non valido", "invalid container_root"),
        ("il percorso da copiare deve essere relativo", "copy_ignored paths must be relative to the repository"),
        ("percorso da copiare non valido", "invalid copy_ignored path (no '..', not the repository root, max 300 characters)"),
        ("troppi percorsi da copiare", "too many copy_ignored paths (max 30)"),
        ("il comando di setup", "setup commands must be single-line"),
        ("il comando di pulizia", "cleanup commands must be single-line"),
        ("comando di setup troppo lungo", "setup command too long (max 2000 characters)"),
        ("comando di pulizia troppo lungo", "cleanup command too long (max 2000 characters)"),
        ("troppi comandi di setup", "too many setup commands (max 30)"),
        ("troppi comandi di pulizia", "too many cleanup commands (max 30)"),
    ];
    match MAP.iter().find(|(it, _)| e.starts_with(it)) {
        Some((_, en)) => (*en).to_string(),
        None => e.to_string(),
    }
}

/// Applica una patch alle impostazioni di un workspace e salva su disco
/// (stesso percorso delle impostazioni dell'interfaccia). Ritorna il workspace.
pub fn apply_settings_patch(app: &AppHandle, workspace_id: &str, patch: &SettingsPatch) -> Result<Workspace, String> {
    persist(app, |data| {
        let ws = ws_mut(data, workspace_id)?;
        patch.apply(ws)?;
        Ok(ws.clone())
    })
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
        worktree_setup: None,
        worker_model: None,
        worker_allow: Vec::new(),
        worker_deny: Vec::new(),
        auto_orchestrate: None,
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
    data.workspaces
        .iter_mut()
        .find(|w| w.id == id)
        .ok_or_else(|| "workspace non trovato".to_string())
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
        let mode = PermissionMode::parse(mode.trim())
            .ok_or_else(|| "permesso non valido (usa plan, default, accept-edits o bypass)".to_string())?;
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
        let allow = crate::worker_permissions::validate_rules(&allow).map_err(|e| format!("Consentiti: {e}"))?;
        let deny = crate::worker_permissions::validate_rules(&deny).map_err(|e| format!("Vietati: {e}"))?;
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
        let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        if model.as_deref().is_some_and(|m| !crate::automation::valid_model(m)) {
            return Err("modello non valido (solo lettere, cifre, '.', '_' e '-', max 64)".to_string());
        }
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.worker_model = model;
            Ok(ws.clone())
        })
    })
    .await
}

/// Comando di setup dei worktree del workspace (vuoto = nessuno).
#[tauri::command]
pub async fn set_worktree_setup(app: AppHandle, id: String, command: Option<String>) -> Result<Workspace, String> {
    crate::blocking(move || {
        let command = normalize_setup(command)?;
        persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.worktree_setup = command;
            Ok(ws.clone())
        })
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
pub async fn set_auto_orchestrate(
    app: AppHandle,
    id: String,
    config: Option<AutoOrchestrate>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let known: Vec<(String, String)> = crate::agents::automation_agents()
            .into_iter()
            .map(|(i, n)| (i.to_string(), n.to_string()))
            .collect();
        let config = normalize_auto(config, &known)?;
        let out = persist(&app, |data| {
            let ws = ws_mut(data, &id)?;
            ws.auto_orchestrate = config;
            Ok(ws.clone())
        })?;
        // Fuori dal lock dei workspace: il riallineamento li rilegge.
        let automation = app.state::<crate::automation::AutomationState>();
        automation.reconcile(&crate::automation::TauriHost(app.clone()));
        Ok(out)
    })
    .await
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
/// avvia task, nessun orchestratore si apre; i task in corso tornano in coda
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

/// Id ammesso come predefinito del workspace: "shell", un suo avvio o uno
/// integrato.
fn valid_default_launch(ws: &Workspace, id: &str, builtin: &[crate::agents::BuiltinLaunch]) -> bool {
    id == "shell" || ws.launches.iter().any(|l| l.id == id) || builtin.iter().any(|b| b.id == id)
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
            data.counter += 1;
            let id = format!("ln-{}-{}", now_millis(), data.counter);
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
            if ws.default_launch.as_deref() == Some(launch_id.as_str()) {
                ws.default_launch = None;
            }
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
                if !valid_default_launch(ws, id, &builtin) {
                    return Err("avvio predefinito non valido".to_string());
                }
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

    fn patch(v: serde_json::Value) -> SettingsPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn settings_patch_parsing_is_strict_and_tells_null_from_absent() {
        let p = patch(serde_json::json!({"repos": {"a": {"base_branch": null, "setup": []}}}));
        let r = &p.repos.as_ref().unwrap()["a"];
        assert_eq!(r.base_branch, Some(None));
        assert_eq!(r.container_root, None);
        assert_eq!(r.setup, Some(vec![]));
        assert_eq!(serde_json::to_value(&p).unwrap(), serde_json::json!({"repos": {"a": {"base_branch": null, "setup": []}}}));
        for bad in [
            serde_json::json!({"worker_allow": []}),
            serde_json::json!({"repos": {"a": {"allow": []}}}),
            serde_json::json!({"workspace_rules": 1}),
        ] {
            assert!(serde_json::from_value::<SettingsPatch>(bad).is_err());
        }
    }

    #[test]
    fn settings_patch_applies_all_or_nothing_and_snapshots_current() {
        let mut ws = ws_with(vec![]);
        ws.worker_rules = "old".into();
        ws.repos.insert("api".into(), RepoConfig { base_branch: Some("main".into()), setup: vec!["a".into()], ..Default::default() });
        let p = patch(serde_json::json!({
            "workspace_rules": " new ",
            "repos": {"api/": {"setup": ["b"], "container_root": " /srv/ "}, ".": {"remove_force": true}}
        }));
        let norm = p.normalized(&ws).unwrap();
        assert_eq!(norm.workspace_rules.as_deref(), Some("new"));
        assert_eq!(norm.repos.as_ref().unwrap()["api"].container_root, Some(Some("/srv".to_string())));
        let cur = norm.current(&ws);
        assert_eq!(cur.workspace_rules.as_deref(), Some("old"));
        assert_eq!(cur.repos.as_ref().unwrap()["api"].setup, Some(vec!["a".to_string()]));
        assert_eq!(cur.repos.as_ref().unwrap()["."].remove_force, Some(false));
        // Un valore non valido in un repository non applica nulla del resto.
        let bad = patch(serde_json::json!({
            "workspace_rules": "new",
            "repos": {"api": {"setup": ["x"]}, "web": {"base_branch": "no good"}}
        }));
        let mut copy = ws.clone();
        assert!(bad.apply(&mut copy).is_err());
        assert_eq!(copy, ws);
        p.apply(&mut ws).unwrap();
        assert_eq!(ws.worker_rules, "new");
        let api = &ws.repos["api"];
        assert_eq!((api.base_branch.as_deref(), api.setup.as_slice()), (Some("main"), &["b".to_string()][..]));
        assert_eq!(api.container_root.as_deref(), Some("/srv"));
        assert!(ws.repos["."].remove_force);
        // null azzera il branch base.
        patch(serde_json::json!({"repos": {"api": {"base_branch": null}}})).apply(&mut ws).unwrap();
        assert_eq!(ws.repos["api"].base_branch, None);
        // Mai permessi o comandi consentiti: i campi non esistono nella patch.
        assert_eq!(ws.worker_allow, Vec::<String>::new());
    }

    #[test]
    fn settings_patch_errors_map_to_english() {
        let ws = ws_with(vec![]);
        let err = |v: serde_json::Value| validation_error_en(&patch(v).normalized(&ws).unwrap_err());
        for (v, expect) in [
            (serde_json::json!({"workspace_rules": "x".repeat(MAX_RULES_LEN + 1)}), "too long"),
            (serde_json::json!({"workspace_rules": "a\u{1}b"}), "control characters"),
            (serde_json::json!({"repos": {"a": {"base_branch": "no good"}}}), "invalid base_branch"),
            (serde_json::json!({"repos": {"a": {"copy_ignored": ["/abs"]}}}), "relative"),
            (serde_json::json!({"repos": {"a": {"copy_ignored": [".."]}}}), "copy_ignored"),
            (serde_json::json!({"repos": {"a": {"setup": ["a\nb"]}}}), "single-line"),
            (serde_json::json!({"repos": {"a": {"cleanup": ["a\nb"]}}}), "cleanup"),
            (serde_json::json!({"repos": {"a": {"setup": vec!["x"; MAX_REPO_COMMANDS + 1]}}}), "too many setup"),
            (serde_json::json!({"repos": {"a": {}}}), "no fields"),
        ] {
            let e = err(v);
            assert!(e.contains(expect) && e.is_ascii(), "{e}");
        }
    }
}
