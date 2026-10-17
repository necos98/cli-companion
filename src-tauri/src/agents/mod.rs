//! Plugin dei coding agent: rilevamento per ambiente (Windows + distro WSL),
//! installazione/rimozione del plugin che avvisa cli-companion quando una
//! sessione dell'agente finisce o aspetta una risposta.
//!
//! Aggiungere un agente = un file che implementa `AgentProvider` (vedi
//! `claude_code.rs`) + una riga in `PROVIDERS`: comandi Tauri e UI
//! (Impostazioni -> Coding agent) sono generici e non vanno toccati.

mod claude_code;

use crate::wsl::{installed_distros, windows_to_wsl_path};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Manager};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
        run_command(self.command(program), args, program)
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

fn run_command(mut cmd: Command, args: &[&str], label: &str) -> Result<String, String> {
    let out = cmd
        .args(args)
        .output()
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

/// Path assoluto dall'output di `command -v`: ultima riga non vuota che
/// inizia con '/'. Le shell interattive/login stampano rumore (motd, nvm,
/// avvisi sul terminale) prima del risultato.
pub fn parse_command_path(output: &str) -> Option<String> {
    output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .rev()
        .find(|l| l.starts_with('/'))
        .map(str::to_string)
}

/// Risolve il path assoluto di `name` in una distro. Il `wsl -d D -e name`
/// diretto usa una shell non-login e spesso non vede `~/.local/bin` o nvm:
/// si chiede a una login shell, con ripiego su quella interattiva.
pub fn resolve_wsl_command(distro: &str, name: &str) -> Option<String> {
    let script = format!("command -v {name}");
    for flag in ["-lc", "-ic"] {
        let mut cmd = Env::Wsl(distro.to_string()).command("bash");
        // Lo stdout basta (anche con exit != 0 per il rumore); lo stderr si ignora.
        if let Ok(out) = cmd.args([flag, &script]).output() {
            if let Some(p) = parse_command_path(&String::from_utf8_lossy(&out.stdout)) {
                return Some(p);
            }
        }
    }
    None
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
    fn command_path_skips_noise() {
        assert_eq!(
            parse_command_path("/home/u/.local/bin/claude\n").as_deref(),
            Some("/home/u/.local/bin/claude")
        );
        let noisy = "bash: cannot set terminal process group (1): Inappropriate ioctl\r\n\
                     Welcome!\r\n/usr/bin/other-noise\r\n/home/u/.nvm/bin/claude\r\n\r\n";
        assert_eq!(
            parse_command_path(noisy).as_deref(),
            Some("/home/u/.nvm/bin/claude")
        );
        assert_eq!(parse_command_path("alias claude='x'\n"), None);
        assert_eq!(parse_command_path(""), None);
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
        let direct = run_command(Command::new("mc-fake-shim"), &[], "shim");
        let via_cmd = Env::Windows.run("mc-fake-shim", &[]);

        std::env::set_var("PATH", &prev_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(direct.is_err(), "lo spawn diretto non deve risolvere gli shim .cmd");
        assert!(via_cmd.expect("cmd /c deve risolvere gli shim .cmd").contains("fake-shim-ok"));
    }
}
