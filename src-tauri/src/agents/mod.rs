//! Plugin dei coding agent: rilevamento per ambiente (Windows + distro WSL),
//! installazione/rimozione del plugin che avvisa cli-companion quando una
//! sessione dell'agente finisce o aspetta una risposta.
//!
//! Aggiungere un agente = un file che implementa `AgentProvider` (vedi
//! `claude_code.rs`) + una riga in `PROVIDERS`: comandi Tauri e UI
//! (Impostazioni -> Coding agent) sono generici e non vanno toccati.
//! Per mostrare i consumi (limiti dell'abbonamento, token di sessione)
//! l'agente implementa anche `usage_limits` / `session_usage` (opzionali,
//! default "non supportato"); `claude_usage.rs` ne e' l'esempio.

mod claude_code;
mod claude_usage;

pub use claude_usage::TranscriptCache;

use crate::launch_settings::LaunchSettingsStore;
use crate::wsl::{
    installed_distros, invalidate_distros_cache, is_windows_drive_path, output_with_timeout,
    windows_to_wsl_path, TtlCache,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Timeout di un comando generico (installazione/rimozione del plugin).
const RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// Timeout della risoluzione del path (shell login/interattiva) e di `--version`.
pub(crate) const DETECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Timeout di `plugin list` (l'agente puo' consultare i marketplace).
pub(crate) const PLUGIN_LIST_TIMEOUT: Duration = Duration::from_secs(20);
/// Validita' della cache dei path risolti.
const RESOLVE_CACHE_TTL: Duration = Duration::from_secs(60);

/// Registro degli agenti supportati.
static PROVIDERS: &[&dyn AgentProvider] = &[&claude_code::ClaudeCode];

/// Ambiente in cui gira un agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Env {
    Windows,
    Wsl(String),
}

impl Env {
    /// Chiave stabile usata dal frontend: "windows" o il nome della distro.
    pub fn key(&self) -> String {
        match self {
            Env::Windows => "windows".to_string(),
            Env::Wsl(d) => d.clone(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Env::Windows => "Windows".to_string(),
            Env::Wsl(d) => d.clone(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Env::Windows => "windows",
            Env::Wsl(_) => "wsl",
        }
    }

    fn from_key(key: &str) -> Result<Env, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("env mancante".to_string());
        }
        Ok(if key == "windows" {
            Env::Windows
        } else {
            Env::Wsl(key.to_string())
        })
    }

    /// Comando che esegue `program` nell'ambiente. Su Windows passa da
    /// `cmd /c`: gli shim npm (.cmd) non si risolvono con uno spawn diretto
    /// (CreateProcess cerca solo .exe), `cmd` applica PATHEXT. Su WSL il
    /// programma e' di norma un path assoluto (`resolve_wsl_command`), eseguito
    /// tramite `bash -lc` (vedi `wsl_login_script`).
    pub fn command(&self, program: &str) -> Command {
        let mut cmd = match self {
            #[cfg(windows)]
            Env::Windows => {
                let mut c = Command::new("cmd");
                c.args(["/c", program]);
                c
            }
            #[cfg(not(windows))]
            Env::Windows => Command::new(program),
            Env::Wsl(d) => {
                let mut c = Command::new("wsl.exe");
                // `-e` (exec diretto): con `--` wsl.exe riunisce gli argomenti e li
                // rilegge la shell di default della distro, perdendo virgolette
                // e argomenti posizionali.
                c.args(["-d", d, "-e"]);
                if program.starts_with('/') {
                    // Path assoluto (risolto con una login shell): va eseguito
                    // dalla stessa login shell, altrimenti un eseguibile con
                    // `#!/usr/bin/env node` (claude installato con nvm) non
                    // trova `node` nel PATH non-login. Gli argomenti seguono
                    // `_` e arrivano come "$@": nessuna interpolazione.
                    c.args(["bash", "-lc", &wsl_login_script(program), "_"]);
                } else {
                    c.arg(program);
                }
                c
            }
        };
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    /// Esegue `program args...`; ritorna lo stdout, o in errore lo stderr
    /// (o lo stdout, o l'exit code).
    pub fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        self.run_timeout(program, args, RUN_TIMEOUT)
    }

    /// Come `run` con un timeout esplicito (il figlio viene ucciso allo scadere).
    pub fn run_timeout(&self, program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
        run_command(self.command(program), args, program, timeout)
    }

    /// Path di una cartella locale come lo vede l'agente nell'ambiente:
    /// slash dritti su Windows, `/mnt/c/...` su WSL.
    pub fn path_arg(&self, win_path: &Path) -> Result<String, String> {
        let s = forward_slashes(&win_path.to_string_lossy());
        match self {
            Env::Windows => Ok(s),
            Env::Wsl(_) => windows_to_wsl_path(&s)
                .ok_or_else(|| format!("percorso non traducibile per WSL: {s}")),
        }
    }
}

/// Script `bash -lc` che esegue `abs_path` con gli argomenti posizionali.
/// Il path e' tra apici singoli (quelli interni sono escapati).
fn wsl_login_script(abs_path: &str) -> String {
    format!("'{}' \"$@\"", abs_path.replace('\'', "'\\''"))
}

/// Backslash -> slash e senza prefisso `\\?\` (i validatori degli agenti
/// rifiutano i backslash nei valori).
pub fn forward_slashes(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches("//?/").to_string()
}

fn run_command(mut cmd: Command, args: &[&str], label: &str, timeout: Duration) -> Result<String, String> {
    let out = output_with_timeout(cmd.args(args), timeout)
        .map_err(|e| format!("{label} non avviabile: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let msg = if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err
        };
        return Err(if msg.is_empty() {
            format!("{label} uscito con codice {:?}", out.status.code())
        } else {
            msg
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Primo token che inizia con una cifra (`2.1.211 (Claude Code)` -> `2.1.211`).
pub fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|t| t.starts_with(|c: char| c.is_ascii_digit()))
        .map(|t| t.trim_matches(|c: char| c == ',' || c == ';').to_string())
}

/// Riga che separa il rumore della shell dall'elenco dei candidati.
const CANDIDATES_MARKER: &str = "__cli_companion_candidates__";

/// Filesystem dei drive Windows montati in WSL (WSL2: 9p, WSL1: drvfs).
const WINDOWS_FS_TYPES: &[&str] = &["9p", "v9fs", "drvfs"];

/// Script che stampa il marcatore e poi, in ordine di PATH, ogni eseguibile
/// `name` come `<tipo fs>\t<path>`.
fn candidates_script(name: &str) -> String {
    format!(
        "printf '%s\\n' {CANDIDATES_MARKER}; type -ap '{}' | while IFS= read -r p; do \
         printf '%s\\t%s\\n' \"$(stat -f -c %T -- \"$p\" 2>/dev/null)\" \"$p\"; done",
        name.replace('\'', "'\\''")
    )
}

/// Primo candidato Linux dall'output di `candidates_script`. Le shell
/// interattive/login stampano rumore (motd, nvm, avvisi) prima del
/// marcatore. Si scartano i programmi sui drive Windows: WSL accoda il PATH
/// di Windows, e lo shim npm `/mnt/c/.../npm/claude` avvierebbe il
/// `claude.exe` di Windows, con la sua config e i path tradotti male.
pub fn parse_command_candidates(output: &str) -> Option<String> {
    let (_, list) = output.rsplit_once(CANDIDATES_MARKER)?;
    list.lines()
        .filter_map(|l| {
            let l = l.trim_end_matches('\r');
            let (fs, path) = l.split_once('\t').unwrap_or(("", l));
            Some((fs.trim(), path.trim())).filter(|(_, p)| p.starts_with('/'))
        })
        .find(|(fs, path)| !WINDOWS_FS_TYPES.contains(fs) && !is_windows_drive_path(path))
        .map(|(_, path)| path.to_string())
}

/// Risolve il path assoluto di `name` in una distro, ignorando i programmi
/// di Windows visibili dal PATH. Il `wsl -d D -e name` diretto usa una
/// shell non-login e spesso non vede `~/.local/bin` o nvm: si chiede a una
/// login shell, con ripiego su quella interattiva.
pub fn resolve_wsl_command(distro: &str, name: &str) -> Option<String> {
    let script = candidates_script(name);
    for flag in ["-lc", "-ic"] {
        let mut cmd = Env::Wsl(distro.to_string()).command("bash");
        // Lo stdout basta (anche con exit != 0 per il rumore); lo stderr si ignora.
        if let Ok(out) = output_with_timeout(cmd.args([flag, &script]), DETECT_TIMEOUT) {
            if let Some(p) = parse_command_candidates(&String::from_utf8_lossy(&out.stdout)) {
                return Some(p);
            }
        }
    }
    None
}

/// `resolve_wsl_command` con cache di breve durata (anche gli esiti `None`).
pub fn resolve_wsl_command_cached(distro: &str, name: &str) -> Option<String> {
    let key = (distro.to_string(), name.to_string());
    if let Some(v) = resolve_cache().get(&key) {
        return v;
    }
    let v = resolve_wsl_command(distro, name);
    resolve_cache().put(key, v.clone());
    v
}

fn resolve_cache() -> &'static TtlCache<(String, String), Option<String>> {
    static C: OnceLock<TtlCache<(String, String), Option<String>>> = OnceLock::new();
    C.get_or_init(|| TtlCache::new(RESOLVE_CACHE_TTL))
}

/// Dimentica distro e path risolti: il rilevamento successivo e' fresco.
fn invalidate_detection_cache() {
    invalidate_distros_cache();
    resolve_cache().clear();
}

/// Esito del rilevamento di un agente in un ambiente.
#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub found: bool,
    pub version: Option<String>,
    /// Programma da usare per i comandi successivi (path assoluto su WSL).
    pub path: Option<String>,
}

/// Contesto per installare/rimuovere un plugin.
pub struct InstallCtx {
    /// Exe corrente (e' il comando degli hook), slash dritti.
    pub exe: String,
    /// Cartella dati dell'app: qui vive il plugin generato.
    pub data_dir: PathBuf,
}

impl InstallCtx {
    /// Exe come lo vede l'agente nell'ambiente (`/mnt/c/...` su WSL).
    pub fn exe_for(&self, env: &Env) -> Result<String, String> {
        match env {
            Env::Windows => Ok(self.exe.clone()),
            Env::Wsl(_) => windows_to_wsl_path(&self.exe)
                .ok_or_else(|| format!("percorso exe non traducibile per WSL: {}", self.exe)),
        }
    }

    /// Cartella del plugin generato per (agente, ambiente). Il marketplace
    /// locale e' caricato sul posto: deve restare su disco finche' installato.
    pub fn plugin_root(&self, agent_id: &str, env: &Env) -> PathBuf {
        self.data_dir
            .join("agent-plugins")
            .join(agent_id)
            .join(sanitize_env_key(&env.key()))
    }
}

/// Chiave env sicura per nomi di cartella: solo ASCII semplici, resto `-`.
pub fn sanitize_env_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Evento di input del terminale che un agente puo' voler gestire in modo
/// diverso dal comportamento standard. PUNTO DI ESTENSIONE: per un nuovo
/// evento si aggiunge una variante (serializzata in kebab-case) e il
/// frontend la richiede con lo stesso nome (`resolveInputOverride`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputEvent {
    /// Incolla con appunti senza testo (es. sola immagine): al posto del
    /// Ctrl+V grezzo si invia la sequenza della regola.
    ImagePaste,
}

/// Regola: per `event`, nell'ambiente `env_kind` (`None` = qualunque), il
/// terminale scrive `send` nel pty.
#[derive(Debug, Clone, Serialize)]
pub struct InputRule {
    pub event: InputEvent,
    /// "windows" | "wsl" (vedi `Env::kind`); `None` = tutti gli ambienti.
    pub env_kind: Option<&'static str>,
    pub send: String,
}

/// Profilo di input dichiarativo di un agente.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InputProfile {
    pub rules: Vec<InputRule>,
}

/// Livello di permessi con cui parte una sessione automatica.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    Bypass,
}

impl PermissionMode {
    /// Valore ammesso dall'RPC `spawn_session`: "default" | "accept-edits" |
    /// "plan" | "bypass".
    pub fn parse(s: &str) -> Option<PermissionMode> {
        match s {
            "default" => Some(PermissionMode::Default),
            "accept-edits" => Some(PermissionMode::AcceptEdits),
            "plan" => Some(PermissionMode::Plan),
            "bypass" => Some(PermissionMode::Bypass),
            _ => None,
        }
    }

    /// Inverso di `parse`: stesso vocabolario di RPC, impostazioni e UI.
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Plan => "plan",
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "accept-edits",
            PermissionMode::Bypass => "bypass",
        }
    }

    /// Permissivita' crescente: plan < default < accept-edits < bypass.
    pub fn rank(self) -> u8 {
        match self {
            PermissionMode::Plan => 0,
            PermissionMode::Default => 1,
            PermissionMode::AcceptEdits => 2,
            PermissionMode::Bypass => 3,
        }
    }
}

/// Richiesta di avvio di una sessione automatica.
pub struct AutomationRequest<'a> {
    /// ASCII sicuro (`[A-Za-z0-9 .,:;_()'-]`, garantito dal chiamante):
    /// finisce in una riga di comando digitata nella shell del tab.
    pub bootstrap_prompt: &'a str,
    pub permission_mode: PermissionMode,
    /// Gia' validato (`[A-Za-z0-9._-]{1,64}`).
    pub model: Option<&'a str>,
}

/// Una finestra di limite dell'abbonamento (5 ore o settimanale).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LimitWindow {
    /// Percentuale consumata (0..=100, puo' superare 100 in sforamento).
    pub used_percentage: f64,
    /// Fine della finestra, RFC3339 come ricevuto.
    pub resets_at: Option<String>,
}

/// Limiti dell'abbonamento e loro stato.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UsageLimitsReply {
    /// "ok" | "expired" | "unavailable" | "disabled" | "unsupported".
    pub state: String,
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    /// Spiegazione breve (italiano) quando lo stato non e' "ok".
    pub message: Option<String>,
}

impl UsageLimitsReply {
    /// Risposta senza finestre, con stato e (opzionale) messaggio.
    pub fn status(state: &str, message: Option<&str>) -> Self {
        Self {
            state: state.to_string(),
            five_hour: None,
            seven_day: None,
            message: message.map(str::to_string),
        }
    }
}

/// Token contati in una sessione.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TokenTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// Consumi di una sessione letti dal transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SessionUsage {
    pub main: TokenTotals,
    pub subagents: TokenTotals,
    /// input + cache read + cache creation dell'ultimo messaggio della catena
    /// principale: la dimensione attuale del contesto.
    pub context_tokens: u64,
    /// Modello dell'ultimo messaggio principale.
    pub model: Option<String>,
    /// Messaggi distinti della catena principale.
    pub messages: u32,
}

/// Un coding agent supportato.
pub trait AgentProvider: Sync {
    /// Id stabile (usato in `--notify`, nelle cartelle e nell'API).
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Profilo di input (default: nessuna regola, comportamento standard).
    /// Dichiarativo e senza processi: l'app lo legge una volta all'avvio.
    fn input_profile(&self) -> InputProfile {
        InputProfile::default()
    }
    fn detect(&self, env: &Env) -> Detection;
    /// Solo se `det.found`: il plugin e' gia' installato?
    fn is_installed(&self, env: &Env, det: &Detection) -> bool;
    /// Installa (o reinstalla) il plugin; ritorna il log dei passi.
    fn install(&self, ctx: &InstallCtx, env: &Env, det: &Detection) -> Result<String, String>;
    /// Rimuove il plugin e i file generati; ritorna il log dei passi.
    fn uninstall(&self, ctx: &InstallCtx, env: &Env, det: &Detection) -> Result<String, String>;
    /// Comando da digitare nella shell del tab per avviare una sessione
    /// automatica (`None` = l'agente non supporta l'automazione). `env_kind`
    /// e' "windows" o "wsl": la stringa deve funzionare in PowerShell, cmd e
    /// bash/zsh.
    fn automation_command(&self, _env_kind: &str, _req: &AutomationRequest) -> Option<String> {
        None
    }
    /// Comando che avvia l'agente in modo interattivo (`None` = nessun avvio
    /// integrato). Digitato nella shell del tab: valido in PowerShell, cmd e bash.
    fn interactive_command(&self) -> Option<&'static str> {
        None
    }
    /// Comando che avvia l'agente come orchestratore (`None` = non supportato).
    fn orchestrator_command(&self) -> Option<&'static str> {
        None
    }
    /// Limiti dell'abbonamento nell'ambiente (`None` = non supportato).
    /// Bloccante (rete): da chiamare fuori dal main thread.
    fn usage_limits(&self, _env: &Env) -> Option<UsageLimitsReply> {
        None
    }
    /// Consumi di token di una sessione dal suo transcript, come path visto
    /// dall'agente nell'ambiente (`None` = non supportato). `cache` evita di
    /// rileggere il file ad ogni chiamata.
    fn session_usage(
        &self,
        _env: &Env,
        _transcript_path: &str,
        _cache: &TranscriptCache,
    ) -> Option<Result<SessionUsage, String>> {
        None
    }
}

/// Avvio integrato (disponibile in ogni workspace) derivato dal registro.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BuiltinLaunch {
    /// `builtin:<agent_id>` oppure `builtin:orchestrator`.
    pub id: String,
    pub name: String,
    pub command: String,
    /// Comando salvato nella sessione del tab per il ripristino: per
    /// l'orchestratore e' quello interattivo (riprende la conversazione
    /// invece di rieseguire la skill).
    pub restore_command: String,
    pub agent_id: String,
}

fn collect_builtin_launches(providers: &[&dyn AgentProvider], default_agent: &str) -> Vec<BuiltinLaunch> {
    let mut out: Vec<BuiltinLaunch> = providers
        .iter()
        .filter_map(|p| {
            let cmd = p.interactive_command()?;
            Some(BuiltinLaunch {
                id: format!("builtin:{}", p.id()),
                name: p.display_name().to_string(),
                command: cmd.to_string(),
                restore_command: cmd.to_string(),
                agent_id: p.id().to_string(),
            })
        })
        .collect();
    // Orchestratore: l'agente di default se lo supporta, altrimenti il primo che puo'.
    let orch = providers
        .iter()
        .find(|p| p.id() == default_agent && p.orchestrator_command().is_some())
        .or_else(|| providers.iter().find(|p| p.orchestrator_command().is_some()));
    if let Some(p) = orch {
        if let Some(cmd) = p.orchestrator_command() {
            out.push(BuiltinLaunch {
                id: "builtin:orchestrator".to_string(),
                name: "Orchestratore".to_string(),
                command: cmd.to_string(),
                restore_command: p.interactive_command().unwrap_or(cmd).to_string(),
                agent_id: p.id().to_string(),
            });
        }
    }
    out
}

/// Avvii integrati di tutti gli agenti (usata anche per validare gli id).
pub fn builtin_launch_list() -> Vec<BuiltinLaunch> {
    collect_builtin_launches(PROVIDERS, crate::automation::DEFAULT_AGENT)
}

/// Comando Tauri `builtin_launches`: economico, nessun processo.
#[tauri::command]
pub fn builtin_launches() -> Vec<BuiltinLaunch> {
    builtin_launch_list()
}

/// Comando di avvio di una sessione automatica per l'agente `agent_id`.
pub fn automation_command(
    agent_id: &str,
    env_kind: &str,
    req: &AutomationRequest,
) -> Result<String, String> {
    // Errori letti dal modello via RPC: in inglese.
    let provider = find_provider(agent_id).map_err(|_| format!("unknown agent: {agent_id}"))?;
    provider
        .automation_command(env_kind, req)
        .ok_or_else(|| format!("{} does not support automated sessions", provider.display_name()))
}

/// Agenti che supportano le sessioni automatiche, come (id, nome).
pub fn automation_agents() -> Vec<(&'static str, &'static str)> {
    let probe = AutomationRequest {
        bootstrap_prompt: "probe",
        permission_mode: PermissionMode::Default,
        model: None,
    };
    PROVIDERS
        .iter()
        .filter(|p| p.automation_command("windows", &probe).is_some())
        .map(|p| (p.id(), p.display_name()))
        .collect()
}

fn find_provider(id: &str) -> Result<&'static dyn AgentProvider, String> {
    PROVIDERS
        .iter()
        .find(|p| p.id() == id)
        .copied()
        .ok_or_else(|| format!("agente sconosciuto: {id}"))
}

/// Windows + distro WSL installate (senza quelle interne di Docker, che non
/// hanno una shell utilizzabile).
fn list_envs() -> Vec<Env> {
    let mut envs = vec![Env::Windows];
    envs.extend(
        installed_distros()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| !d.to_lowercase().starts_with("docker-desktop"))
            .map(Env::Wsl),
    );
    envs
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentEnvStatus {
    pub agent_id: String,
    pub agent_name: String,
    pub env_key: String,
    pub env_label: String,
    pub env_kind: String,
    pub found: bool,
    pub version: Option<String>,
    pub installed: bool,
}

fn status_for(provider: &dyn AgentProvider, env: &Env) -> AgentEnvStatus {
    let det = provider.detect(env);
    let installed = det.found && provider.is_installed(env, &det);
    AgentEnvStatus {
        agent_id: provider.id().to_string(),
        agent_name: provider.display_name().to_string(),
        env_key: env.key(),
        env_label: env.label(),
        env_kind: env.kind().to_string(),
        found: det.found,
        version: det.version,
        installed,
    }
}

/// Ogni coppia agente x env costa piu' processi e su WSL anche l'avvio della
/// VM: si sondano tutte in parallelo.
fn collect_status() -> Vec<AgentEnvStatus> {
    let envs = list_envs();
    std::thread::scope(|s| {
        let handles: Vec<_> = envs
            .iter()
            .flat_map(|env| PROVIDERS.iter().map(move |p| (*p, env)))
            .map(|(p, env)| s.spawn(move || status_for(p, env)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

#[tauri::command]
pub async fn agent_env_status() -> Result<Vec<AgentEnvStatus>, String> {
    crate::blocking(|| Ok(collect_status())).await
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentProfile {
    pub agent_id: String,
    pub agent_name: String,
    pub input_profile: InputProfile,
}

fn collect_profiles() -> Vec<AgentProfile> {
    PROVIDERS
        .iter()
        .map(|p| AgentProfile {
            agent_id: p.id().to_string(),
            agent_name: p.display_name().to_string(),
            input_profile: p.input_profile(),
        })
        .collect()
}

/// Profili di input di tutti gli agenti: economico, nessun processo.
#[tauri::command]
pub fn agent_profiles() -> Vec<AgentProfile> {
    collect_profiles()
}

/// Env dal frontend per le letture dei consumi. Il nome della distro finisce
/// in un path UNC: niente separatori o caratteri insoliti.
fn usage_env(env_key: &str) -> Result<Env, String> {
    let env = Env::from_key(env_key)?;
    if let Env::Wsl(d) = &env {
        let ok = d.len() <= 64
            && d != ".."
            && d.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '));
        if !ok {
            return Err(format!("nome distro non valido: {d}"));
        }
    }
    Ok(env)
}

/// Limiti dell'abbonamento dell'agente. Stato "disabled" se l'utente ha
/// spento i consumi, "unsupported" se l'agente non li espone.
#[tauri::command]
pub async fn agent_usage_limits(
    settings: State<'_, LaunchSettingsStore>,
    agent_id: String,
    env: String,
) -> Result<UsageLimitsReply, String> {
    let provider = find_provider(&agent_id)?;
    let env = usage_env(&env)?;
    if !settings.show_usage() {
        return Ok(UsageLimitsReply::status("disabled", None));
    }
    crate::blocking(move || {
        Ok(provider
            .usage_limits(&env)
            .unwrap_or_else(|| UsageLimitsReply::status("unsupported", None)))
    })
    .await
}

/// Token consumati da una sessione (dal transcript). `None` se l'agente non
/// li espone o i consumi sono spenti.
#[tauri::command]
pub async fn agent_session_usage(
    settings: State<'_, LaunchSettingsStore>,
    cache: State<'_, TranscriptCache>,
    agent_id: String,
    env: String,
    transcript_path: String,
) -> Result<Option<SessionUsage>, String> {
    let provider = find_provider(&agent_id)?;
    let env = usage_env(&env)?;
    if !settings.show_usage() {
        return Ok(None);
    }
    let cache = cache.inner().clone();
    crate::blocking(move || match provider.session_usage(&env, &transcript_path, &cache) {
        Some(r) => r.map(Some),
        None => Ok(None),
    })
    .await
}

/// Risolve agente + ambiente (la distro deve esistere) e rileva l'agente.
fn prepare(
    app: &AppHandle,
    agent_id: &str,
    env_key: &str,
) -> Result<(&'static dyn AgentProvider, Env, Detection, InstallCtx), String> {
    let provider = find_provider(agent_id)?;
    let env = Env::from_key(env_key)?;
    if let Env::Wsl(d) = &env {
        if !list_envs().contains(&Env::Wsl(d.clone())) {
            return Err(format!("distro non trovata: {d}"));
        }
    }
    let exe = std::env::current_exe().map_err(|e| format!("percorso exe: {e}"))?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let ctx = InstallCtx {
        exe: forward_slashes(&exe.to_string_lossy()),
        data_dir,
    };
    let det = provider.detect(&env);
    Ok((provider, env, det, ctx))
}

#[tauri::command]
pub async fn install_agent_plugin(
    app: AppHandle,
    agent_id: String,
    env_key: String,
) -> Result<String, String> {
    crate::blocking(move || {
        invalidate_detection_cache();
        let (provider, env, det, ctx) = prepare(&app, &agent_id, &env_key)?;
        if !det.found {
            return Err(format!(
                "{} non trovato in questo env: installalo prima",
                provider.display_name()
            ));
        }
        provider.install(&ctx, &env, &det)
    })
    .await
}

#[tauri::command]
pub async fn remove_agent_plugin(
    app: AppHandle,
    agent_id: String,
    env_key: String,
) -> Result<String, String> {
    crate::blocking(move || {
        invalidate_detection_cache();
        let (provider, env, det, ctx) = prepare(&app, &agent_id, &env_key)?;
        provider.uninstall(&ctx, &env, &det)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_first_numeric_token() {
        assert_eq!(parse_version("2.1.211 (Claude Code)\n").as_deref(), Some("2.1.211"));
        assert_eq!(parse_version("Claude Code 1.2.3, build").as_deref(), Some("1.2.3"));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("   \n"), None);
    }

    #[test]
    fn command_candidates_skip_noise_and_keep_path_order() {
        let p = |s: &str| parse_command_candidates(s);
        let m = CANDIDATES_MARKER;
        assert_eq!(
            p(&format!("{m}\next2/ext3\t/home/u/.local/bin/claude\n")).as_deref(),
            Some("/home/u/.local/bin/claude")
        );
        // Rumore prima del marcatore (anche righe che sembrano path).
        let noisy = format!(
            "bash: cannot set terminal process group (1): Inappropriate ioctl\r\n\
             Welcome!\r\n/usr/bin/other-noise\r\n{m}\r\next2/ext3\t/home/u/.nvm/bin/claude\r\n\
             ext2/ext3\t/usr/bin/claude\r\n\r\n"
        );
        assert_eq!(p(&noisy).as_deref(), Some("/home/u/.nvm/bin/claude"));
        // Path con spazi; tipo fs assente (stat non disponibile).
        assert_eq!(p(&format!("{m}\n\t/opt/my tools/claude\n")).as_deref(), Some("/opt/my tools/claude"));
        assert_eq!(p(&format!("{m}\n/usr/local/bin/claude\n")).as_deref(), Some("/usr/local/bin/claude"));
        // Niente marcatore (script non eseguito) o nessun candidato.
        assert_eq!(p("/home/u/.local/bin/claude\n"), None);
        assert_eq!(p(&format!("{m}\n")), None);
        assert_eq!(p(""), None);
        assert_eq!(p(&format!("{m}\nclaude is aliased to x\n")), None);
    }

    /// Regressione: in una distro senza claude, `command -v` trovava lo shim
    /// npm di Windows (PATH di Windows accodato da WSL), che avvia il
    /// `claude.exe` di Windows: plugin "installato" letto dalla config di
    /// Windows e `plugin validate /mnt/c/...` letto come `C:\mnt\c\...`.
    #[test]
    fn command_candidates_skip_windows_programs() {
        let p = |s: &str| parse_command_candidates(s);
        let m = CANDIDATES_MARKER;
        let shim = "/mnt/c/Users/j.magarelli/AppData/Roaming/npm/claude";
        // Solo il claude di Windows: nessun claude nella distro.
        assert_eq!(p(&format!("{m}\nv9fs\t{shim}\n")), None);
        // Per path anche senza tipo fs, e per tipo fs anche con mount root diversa.
        assert_eq!(p(&format!("{m}\n\t{shim}\n")), None);
        assert_eq!(p(&format!("{m}\n9p\t/c/Users/u/npm/claude\ndrvfs\t/win/claude\n")), None);
        // Windows prima nel PATH: vince comunque quello Linux.
        assert_eq!(
            p(&format!("{m}\nv9fs\t{shim}\next2/ext3\t/home/u/.local/bin/claude\n")).as_deref(),
            Some("/home/u/.local/bin/claude")
        );
        // Un drive con nome lungo non e' un drive Windows.
        assert_eq!(p(&format!("{m}\next2/ext3\t/mnt/cd/claude\n")).as_deref(), Some("/mnt/cd/claude"));
    }

    /// Risoluzione reale in una distro: i programmi di Windows (nel PATH via
    /// interop) non vengono mai scelti, quelli Linux si'.
    /// `cargo test -- --ignored resolve_wsl_real --nocapture`
    #[test]
    #[ignore]
    fn resolve_wsl_real_skips_windows_programs() {
        let distro = std::env::var("CC_TEST_DISTRO").unwrap_or_else(|_| "Ubuntu-24.04".to_string());
        let bash = resolve_wsl_command(&distro, "bash").expect("bash nella distro");
        assert!(bash.ends_with("/bash") && !is_windows_drive_path(&bash), "{bash}");
        // Solo nel PATH di Windows.
        assert_eq!(resolve_wsl_command(&distro, "notepad.exe"), None);
        assert_eq!(resolve_wsl_command(&distro, "cmd.exe"), None);
        let claude = resolve_wsl_command(&distro, "claude");
        println!("claude: {claude:?}");
        assert!(claude.as_deref().is_none_or(|p| !is_windows_drive_path(p)));
    }

    #[test]
    fn candidates_script_quotes_name() {
        let s = candidates_script("claude");
        assert!(s.starts_with(&format!("printf '%s\\n' {CANDIDATES_MARKER};")));
        assert!(s.contains("type -ap 'claude'"));
        assert!(candidates_script("it's").contains("type -ap 'it'\\''s'"));
    }

    #[test]
    fn sanitize_env_key_keeps_simple_names() {
        assert_eq!(sanitize_env_key("windows"), "windows");
        assert_eq!(sanitize_env_key("Ubuntu-24.04"), "Ubuntu-24.04");
        assert_eq!(sanitize_env_key("a/b\\c d"), "a-b-c-d");
    }

    #[test]
    fn env_keys_and_paths() {
        assert_eq!(Env::from_key("windows"), Ok(Env::Windows));
        assert_eq!(
            Env::from_key(" Ubuntu "),
            Ok(Env::Wsl("Ubuntu".to_string()))
        );
        assert!(Env::from_key("  ").is_err());
        assert_eq!(Env::Windows.kind(), "windows");
        assert_eq!(Env::Wsl("U".into()).kind(), "wsl");

        let p = Path::new(r"C:\Users\me\AppData\Roaming\app");
        assert_eq!(Env::Windows.path_arg(p).unwrap(), "C:/Users/me/AppData/Roaming/app");
        assert_eq!(
            Env::Wsl("U".into()).path_arg(p).unwrap(),
            "/mnt/c/Users/me/AppData/Roaming/app"
        );
        assert_eq!(forward_slashes(r"\\?\C:\a\b.exe"), "C:/a/b.exe");

        let ctx = InstallCtx {
            exe: "C:/Program Files/cc/cli-companion.exe".to_string(),
            data_dir: PathBuf::from(r"C:\data"),
        };
        assert_eq!(
            ctx.exe_for(&Env::Wsl("U".into())).unwrap(),
            "/mnt/c/Program Files/cc/cli-companion.exe"
        );
        let root = ctx.plugin_root("claude-code", &Env::Wsl("Ubuntu 24".into()));
        assert!(root.ends_with(Path::new("agent-plugins/claude-code/Ubuntu-24")));
    }

    #[test]
    fn registry_ids_are_unique_and_findable() {
        let mut ids: Vec<_> = PROVIDERS.iter().map(|p| p.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), PROVIDERS.len());
        assert!(find_provider("claude-code").is_ok());
        assert!(find_provider("nope").is_err());
    }

    #[test]
    fn profiles_cover_all_providers_and_serialize_kebab_case() {
        let profiles = collect_profiles();
        assert_eq!(profiles.len(), PROVIDERS.len());
        let cc = profiles.iter().find(|p| p.agent_id == "claude-code").unwrap();
        assert_eq!(cc.agent_name, "Claude Code");
        let v = serde_json::to_value(cc).unwrap();
        let rules = v["input_profile"]["rules"].as_array().unwrap();
        for kind in ["windows", "wsl"] {
            assert!(rules.iter().any(|r| r["event"] == "image-paste"
                && r["env_kind"] == kind
                && r["send"] == "\u{1b}v"));
        }
    }

    struct Fake {
        id: &'static str,
        interactive: Option<&'static str>,
        orchestrator: Option<&'static str>,
    }

    impl AgentProvider for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn display_name(&self) -> &'static str {
            "Fake"
        }
        fn detect(&self, _: &Env) -> Detection {
            Detection::default()
        }
        fn is_installed(&self, _: &Env, _: &Detection) -> bool {
            false
        }
        fn install(&self, _: &InstallCtx, _: &Env, _: &Detection) -> Result<String, String> {
            Ok(String::new())
        }
        fn uninstall(&self, _: &InstallCtx, _: &Env, _: &Detection) -> Result<String, String> {
            Ok(String::new())
        }
        fn interactive_command(&self) -> Option<&'static str> {
            self.interactive
        }
        fn orchestrator_command(&self) -> Option<&'static str> {
            self.orchestrator
        }
    }

    #[test]
    fn builtin_launches_from_registry() {
        let l = builtin_launch_list();
        assert_eq!(l[0].id, "builtin:claude-code");
        assert_eq!(l[0].name, "Claude Code");
        assert_eq!(l[0].command, l[0].restore_command);
        let o = l.iter().find(|b| b.id == "builtin:orchestrator").unwrap();
        assert_eq!(o.name, "Orchestratore");
        assert_eq!(o.agent_id, "claude-code");
        assert_eq!(o.restore_command, l[0].command);
        assert!(o.command.contains("cli-companion-notify:orchestrate"));
    }

    #[test]
    fn builtin_orchestrator_prefers_default_agent_and_may_be_absent() {
        let a = Fake { id: "a", interactive: Some("a-cli"), orchestrator: Some("a-orch") };
        let b = Fake { id: "b", interactive: Some("b-cli"), orchestrator: Some("b-orch") };
        let c = Fake { id: "c", interactive: None, orchestrator: None };
        let l = collect_builtin_launches(&[&a, &b, &c], "b");
        let ids: Vec<_> = l.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["builtin:a", "builtin:b", "builtin:orchestrator"]);
        assert_eq!(l[2].command, "b-orch");
        assert_eq!(l[2].restore_command, "b-cli");
        // Default senza orchestratore: il primo che ce l'ha.
        let l = collect_builtin_launches(&[&c, &a], "c");
        assert_eq!(l.last().unwrap().agent_id, "a");
        // Nessun orchestratore: nessuna voce.
        let n = Fake { id: "n", interactive: Some("n-cli"), orchestrator: None };
        assert_eq!(collect_builtin_launches(&[&n], "n").len(), 1);
        assert!(collect_builtin_launches(&[&c], "c").is_empty());
    }

    #[test]
    fn usage_is_unsupported_by_default_and_env_is_validated() {
        let f = Fake { id: "f", interactive: None, orchestrator: None };
        assert!(f.usage_limits(&Env::Windows).is_none());
        assert!(f.session_usage(&Env::Windows, "x", &TranscriptCache::default()).is_none());
        assert_eq!(usage_env("windows"), Ok(Env::Windows));
        assert_eq!(usage_env("Ubuntu-24.04"), Ok(Env::Wsl("Ubuntu-24.04".into())));
        for bad in ["", "a\\b", "a/b", "..", "x:y", "a\nb"] {
            assert!(usage_env(bad).is_err(), "{bad:?}");
        }
        let v = serde_json::to_value(UsageLimitsReply::status("disabled", None)).unwrap();
        assert_eq!(v["state"], "disabled");
        assert!(v["five_hour"].is_null() && v["message"].is_null());
    }

    #[test]
    fn default_input_profile_is_empty() {
        assert!(InputProfile::default().rules.is_empty());
    }

    #[test]
    fn command_wraps_windows_cmd_shim_and_wsl() {
        let c = Env::Windows.command("claude");
        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            assert_eq!(c.get_program(), "cmd");
            let args: Vec<_> = c.get_args().collect();
            assert_eq!(args, vec![OsStr::new("/c"), OsStr::new("claude")]);
        }
        #[cfg(not(windows))]
        assert_eq!(c.get_program(), "claude");
        let w = Env::Wsl("Ubuntu-24.04".into()).command("/home/u/.local/bin/claude");
        assert_eq!(w.get_program(), "wsl.exe");
        let wargs: Vec<_> = w.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            wargs,
            vec![
                "-d",
                "Ubuntu-24.04",
                "-e",
                "bash",
                "-lc",
                "'/home/u/.local/bin/claude' \"$@\"",
                "_"
            ]
        );
        // Programma non assoluto (es. `bash` per la risoluzione): invariato.
        let b = Env::Wsl("U".into()).command("bash");
        let bargs: Vec<_> = b.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(bargs, vec!["-d", "U", "-e", "bash"]);
    }

    #[test]
    fn wsl_login_script_quotes_path_and_passes_args_positionally() {
        assert_eq!(wsl_login_script("/a b/claude"), "'/a b/claude' \"$@\"");
        assert_eq!(wsl_login_script("/it's/claude"), "'/it'\\''s/claude' \"$@\"");
        // Gli argomenti di run() vengono dopo `_` e non toccano lo script.
        let mut c = Env::Wsl("U".into()).command("/x/claude");
        c.args(["plugin", "list", "--json"]);
        let a: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&a[a.len() - 4..], ["_", "plugin", "list", "--json"]);
    }

    /// Regressione "agente non trovato" su Windows: `claude`/`npm` si risolvono
    /// a shim .cmd e CreateProcess cerca solo .exe per nome, quindi lo spawn
    /// diretto fallisce mentre `cmd /c` (PATHEXT) lo trova. Prova con uno
    /// shim finto in temp (niente dipendenza da un agente installato).
    #[test]
    #[cfg(windows)]
    fn cmd_shim_found_only_through_cmd_c() {
        let dir = std::env::temp_dir().join(format!("mc-cmd-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mc-fake-shim.cmd"), "@echo fake-shim-ok\r\n").unwrap();

        // Solo lo shim in testa al PATH (ripristinato subito dopo gli spawn).
        let prev_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(&prev_path));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

        // Spawn diretto per nome: non risolve. Via Env::run (cmd /c): si'.
        let direct = run_command(Command::new("mc-fake-shim"), &[], "shim", RUN_TIMEOUT);
        let via_cmd = Env::Windows.run("mc-fake-shim", &[]);

        std::env::set_var("PATH", &prev_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(direct.is_err(), "lo spawn diretto non deve risolvere gli shim .cmd");
        assert!(via_cmd.expect("cmd /c deve risolvere gli shim .cmd").contains("fake-shim-ok"));
    }
}
