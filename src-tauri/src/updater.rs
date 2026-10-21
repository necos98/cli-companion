//! Controllo aggiornamenti leggero via GitHub Releases, usato dalla CLI.
//!
//! L'aggiornamento vero e proprio (download firmato, installer NSIS in modalita'
//! passive, riavvio) lo fa la GUI con `tauri-plugin-updater`: vedi
//! `src/components/UpdateCheck.tsx` e docs/UPDATER.MD. Questo modulo resta solo
//! per `cli-companion --check-update` (src/main.rs), che non apre finestre e non
//! scarica nulla:
//!
//! - Config: [`../../updater.config.json`](../../../updater.config.json) incluso a
//!   compile-time; override runtime via env `CLI_COMPANION_UPDATE_OWNER`,
//!   `CLI_COMPANION_UPDATE_REPO`, `CLI_COMPANION_UPDATE_API_URL`.
//!   Se il repository non esiste ancora, `owner` resta [`PLACEHOLDER_OWNER`] e ogni
//!   controllo risponde con un messaggio chiaro invece di fallire in modo oscuro.
//! - Check: `GET {api}/repos/{owner}/{repo}/releases/latest` via `curl`
//!   (preinstallato su Windows 10+, macOS e quasi tutte le distro Linux).
//!
//! La logica di confronto versioni e' duplicata in `src/lib/updater.ts`:
//! mantenerle allineate (stessi vettori nei test Rust qui sotto e in
//! `tests/updater.test.mjs`).

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::process::Command;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Owner placeholder: indica che il repository GitHub non e' stato ancora creato.
/// Chi legge questo valore negli errori deve impostare `owner`/`repo` in
/// `updater.config.json` (o le env `CLI_COMPANION_UPDATE_OWNER` / `_REPO`).
pub const PLACEHOLDER_OWNER: &str = "REPLACE_ME_OWNER";

/// Config grezza dal JSON (chiavi camelCase come da convenzione JS).
#[derive(Debug, Clone, Deserialize)]
struct FileConfig {
    #[serde(default)]
    owner: String,
    #[serde(default)]
    repo: String,
    #[serde(default, alias = "apiUrl")]
    api_url: Option<String>,
}

const EMBEDDED_CONFIG: &str = include_str!("../../updater.config.json");

/// Config effettiva (solo uso interno: la GUI non la legge piu', usa il plugin).
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub owner: String,
    pub repo: String,
    pub configured: bool,
    pub current_version: String,
    pub api_url: String,
    pub message: String,
}

fn env_override(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Carica la config effettiva: JSON embedded + override da variabili d'ambiente.
pub fn load_config() -> UpdateConfig {
    let file: FileConfig = serde_json::from_str(EMBEDDED_CONFIG).unwrap_or(FileConfig {
        owner: PLACEHOLDER_OWNER.to_string(),
        repo: "cli-companion".to_string(),
        api_url: None,
    });
    let owner = env_override("CLI_COMPANION_UPDATE_OWNER").unwrap_or(file.owner);
    let repo = env_override("CLI_COMPANION_UPDATE_REPO").unwrap_or(file.repo);
    let api_url = env_override("CLI_COMPANION_UPDATE_API_URL")
        .or(file.api_url)
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let configured =
        !owner.trim().is_empty() && owner != PLACEHOLDER_OWNER && !repo.trim().is_empty();
    let message = if configured {
        format!("Pronto: controllerò le release di {owner}/{repo}.")
    } else {
        not_configured_message_raw(&owner, &repo)
    };
    UpdateConfig {
        owner,
        repo,
        configured,
        current_version: current_version().to_string(),
        api_url,
        message,
    }
}

fn not_configured_message_raw(owner: &str, repo: &str) -> String {
    format!(
        "Repository GitHub non ancora configurato (owner='{owner}', repo='{repo}'). \
         L'updater è in attesa: crea il repository e imposta \"owner\"/\"repo\" in \
         updater.config.json (oppure le variabili CLI_COMPANION_UPDATE_OWNER/_REPO). \
         Nessun controllo effettuato."
    )
}

// ---------------------------------------------------------------------------
// Versioni (spec condivisa con src/lib/updater.ts)
// ---------------------------------------------------------------------------

/// Versione locale dal manifest (una delle 3 copie da tenere sincronizzate:
/// package.json, src-tauri/Cargo.toml, src-tauri/tauri.conf.json).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Normalizza: trim, via il prefisso `v`/`V`, stringa vuota -> "0".
pub fn normalize_version(raw: &str) -> String {
    let t = raw.trim();
    let t = t
        .strip_prefix('v')
        .or_else(|| t.strip_prefix('V'))
        .unwrap_or(t);
    if t.is_empty() {
        "0".to_string()
    } else {
        t.to_string()
    }
}

/// Divide una componente ("10rc1" -> (10, "rc1"), "beta" -> (0, "beta")).
fn split_part(part: &str) -> (u64, &str) {
    let digits_len = part
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(|c| c.len_utf8())
        .sum::<usize>();
    let num = part[..digits_len].parse::<u64>().unwrap_or(0);
    (num, &part[digits_len..])
}

/// Confronta due versioni in stile semver tollerante:
/// - prefisso `v` ignorato, componenti mancanti = 0 ("1.2" == "1.2.0"),
/// - parti numeriche confrontate come numeri ("1.10" > "1.9"),
/// - suffisso vuoto (release) > suffisso non vuoto (prerelease).
pub fn compare_versions(local: &str, remote: &str) -> Ordering {
    let a = normalize_version(local);
    let b = normalize_version(remote);
    let pa: Vec<&str> = a.split('.').collect();
    let pb: Vec<&str> = b.split('.').collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or("0");
        let y = pb.get(i).copied().unwrap_or("0");
        let (nx, rx) = split_part(x);
        let (ny, ry) = split_part(y);
        match nx.cmp(&ny) {
            Ordering::Equal => {}
            o => return o,
        }
        match (rx.is_empty(), ry.is_empty()) {
            (true, true) => {}
            (true, false) => return Ordering::Greater, // release > prerelease
            (false, true) => return Ordering::Less,
            (false, false) => match rx.cmp(ry) {
                Ordering::Equal => {}
                o => return o,
            },
        }
    }
    Ordering::Equal
}

/// `true` se `remote` e' piu' recente di `local`.
pub fn is_newer_version(local: &str, remote: &str) -> bool {
    compare_versions(local, remote) == Ordering::Less
}

// ---------------------------------------------------------------------------
// GitHub Releases via curl
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    #[serde(default)]
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub configured: bool,
    pub owner: String,
    pub repo: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_name: String,
    pub release_notes: String,
    pub release_url: String,
    pub published_at: String,
}

pub(crate) fn curl_bin() -> String {
    if let Some(custom) = env_override("CLI_COMPANION_UPDATER_CURL") {
        return custom;
    }
    if cfg!(windows) {
        "curl.exe".to_string()
    } else {
        "curl".to_string()
    }
}

fn latest_release_url(cfg: &UpdateConfig) -> String {
    // Hook per test/mock: punta direttamente a un JSON di release (anche file://).
    if let Some(custom) = env_override("CLI_COMPANION_UPDATE_LATEST_URL") {
        return custom;
    }
    format!(
        "{}/repos/{}/{}/releases/latest",
        cfg.api_url.trim_end_matches('/'),
        cfg.owner,
        cfg.repo
    )
}

/// Niente finestra console per i processi figli: la build release e' una app
/// GUI (windows_subsystem) e curl/cmd aprirebbero un terminale lampeggiante.
pub(crate) fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) fn run_curl(args: &[String]) -> Result<Vec<u8>, String> {
    run_curl_with(args, None)
}

/// Come `run_curl` ma scrive `stdin` sullo stdin di curl: serve per passare
/// header segreti con `-H @-` senza metterli nella riga di comando.
pub(crate) fn run_curl_stdin(args: &[String], stdin: &[u8]) -> Result<Vec<u8>, String> {
    run_curl_with(args, Some(stdin))
}

fn run_curl_with(args: &[String], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use std::process::Stdio;
    let bin = curl_bin();
    let spawn_err = |e: std::io::Error| {
        format!("Impossibile eseguire '{bin}' (curl non trovato nel PATH?): {e}. Su Windows 10+ curl.exe è preinstallato.")
    };
    let mut cmd = Command::new(&bin);
    quiet(&mut cmd).args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().map_err(spawn_err)?;
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // Errore ignorato: se curl chiude prima, l'esito si vede dall'exit code.
        let _ = pipe.write_all(data);
    }
    let out = child.wait_with_output().map_err(spawn_err)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "curl fallito (exit {}): {}",
            out.status,
            err.trim()
        ));
    }
    Ok(out.stdout)
}

/// Scarica e interpreta la latest release. Errore chiaro se repo non configurato.
pub fn fetch_latest_release(cfg: &UpdateConfig) -> Result<GithubRelease, String> {
    if !cfg.configured {
        return Err(not_configured_message_raw(&cfg.owner, &cfg.repo));
    }
    let url = latest_release_url(cfg);
    let args = vec![
        "-sSL".to_string(),
        "--fail".to_string(),
        "--max-time".to_string(),
        "25".to_string(),
        "-H".to_string(),
        "Accept: application/vnd.github+json".to_string(),
        "-H".to_string(),
        "User-Agent: cli-companion-updater".to_string(),
        url.clone(),
    ];
    let body = run_curl(&args).map_err(|e| {
        format!(
            "Controllo aggiornamenti fallito per {}/{}: {e}. Verifica la connessione e che il repository esista e abbia almeno una release.",
            cfg.owner, cfg.repo
        )
    })?;
    let release = parse_release(&body, &url)?;
    if release.tag_name.trim().is_empty() {
        return Err("La release GitHub non ha tag_name: impossibile confrontare le versioni."
            .to_string());
    }
    Ok(release)
}

/// Interpreta il JSON di una release (tollera il BOM UTF-8).
fn parse_release(body: &[u8], url: &str) -> Result<GithubRelease, String> {
    let body = body
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(body);
    let release: GithubRelease = serde_json::from_slice(body)
        .map_err(|e| format!("Risposta GitHub non valida da {url}: {e}"))?;
    Ok(release)
}

/// Check completo: config -> latest release -> confronto versioni.
pub fn check_blocking() -> Result<UpdateStatus, String> {
    let cfg = load_config();
    let release = fetch_latest_release(&cfg)?;
    let latest = normalize_version(&release.tag_name);
    let current = current_version().to_string();
    Ok(UpdateStatus {
        configured: true,
        owner: cfg.owner,
        repo: cfg.repo,
        current_version: current.clone(),
        latest_version: latest.clone(),
        update_available: is_newer_version(&current, &latest),
        release_name: release.name.clone().unwrap_or_default(),
        release_notes: release.body.clone().unwrap_or_default(),
        release_url: release.html_url.clone(),
        published_at: release.published_at.clone().unwrap_or_default(),
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // I test che leggono/scrivono le env CLI_COMPANION_UPDATE_* devono girare
    // in mutua esclusione (cargo test è parallelo per default).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn normalize_strip_v_e_vuoti() {
        assert_eq!(normalize_version("  v1.2.3 "), "1.2.3");
        assert_eq!(normalize_version("V0.1.0"), "0.1.0");
        assert_eq!(normalize_version(""), "0");
        assert_eq!(normalize_version("2.0"), "2.0");
    }

    #[test]
    fn compare_versioni_vettori() {
        // Stessi vettori di tests/updater.test.mjs (parità TS/Rust).
        let cases = [
            ("0.1.0", "0.1.0", Ordering::Equal),
            ("0.1.0", "0.2.0", Ordering::Less),
            ("v0.1.0", "0.1.1", Ordering::Less),
            ("1.10.0", "1.9.0", Ordering::Greater),
            ("1.2", "1.2.0", Ordering::Equal),
            ("2.0.0", "10.0.0", Ordering::Less),
            ("1.0.0-beta", "1.0.0", Ordering::Less),
            ("1.0.0", "1.0.0-beta", Ordering::Greater),
        ];
        for (a, b, want) in cases {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
        }
        assert!(is_newer_version("0.1.0", "0.2.0"));
        assert!(!is_newer_version("0.2.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
    }

    #[test]
    fn config_embedded_si_legge_e_placeholder_rilevato() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Assicura che nessun override d'ambiente sporchi questo test.
        std::env::remove_var("CLI_COMPANION_UPDATE_OWNER");
        std::env::remove_var("CLI_COMPANION_UPDATE_REPO");
        let cfg = load_config();
        assert_eq!(cfg.repo, "cli-companion");
        assert_eq!(cfg.configured, cfg.owner != PLACEHOLDER_OWNER);
        if !cfg.configured {
            assert!(cfg.message.contains("non ancora configurato"));
        }
        // Il check senza repo configurato fallisce con messaggio chiaro.
        if !cfg.configured {
            let err = fetch_latest_release(&cfg).unwrap_err();
            assert!(err.contains("non ancora configurato"), "{err}");
        }
    }

    #[test]
    fn config_override_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("CLI_COMPANION_UPDATE_OWNER", "acme-test");
        let cfg = load_config();
        std::env::remove_var("CLI_COMPANION_UPDATE_OWNER");
        assert_eq!(cfg.owner, "acme-test");
        assert!(cfg.configured);
    }

    #[test]
    fn parse_release_tollera_bom_e_campi_mancanti() {
        let mut raw = b"\xef\xbb\xbf".to_vec();
        raw.extend_from_slice(br#"{"tag_name":"v0.2.0"}"#);
        let r = parse_release(&raw, "mock").unwrap();
        assert_eq!(r.tag_name, "v0.2.0");
        assert!(r.published_at.is_none());
        assert!(parse_release(b"non-json", "mock").is_err());
    }
}
