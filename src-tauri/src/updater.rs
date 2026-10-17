//! Updater leggero basato su GitHub Releases (zero nuove dipendenze Cargo).
//!
//! - Config: [`../../updater.config.json`](../../../updater.config.json) incluso a
//!   compile-time; override runtime via env `CLI_COMPANION_UPDATE_OWNER`,
//!   `CLI_COMPANION_UPDATE_REPO`, `CLI_COMPANION_UPDATE_API_URL`.
//!   Se il repository non esiste ancora, `owner` resta [`PLACEHOLDER_OWNER`] e ogni
//!   controllo risponde con un messaggio chiaro invece di fallire in modo oscuro.
//! - Check: `GET {api}/repos/{owner}/{repo}/releases/latest` via `curl`
//!   (preinstallato su Windows 10+, macOS e quasi tutte le distro Linux).
//! - Download: `curl --fail` in `%TEMP%/cli-companion-update`; verifica dimensione
//!   + SHA256 (implementazione pura qui sotto, nessuna crate) contro il checksum
//!   pubblicato nella release, se presente.
//! - Install: NON sostituisce file a caldo (su Windows l'exe in esecuzione e' lockato
//!   dal sistema). Scarica l'installer verificato e lo avvia (o ne restituisce il
//!   percorso per l'avvio manuale).
//! - Rollback/failure: su qualsiasi errore il file parziale viene cancellato e
//!   l'installazione corrente resta intatta; il messaggio spiega come riprovare.
//!
//! La logica di confronto versioni e' duplicata in `src/lib/updater.ts` per la GUI:
//! mantenerle allineate (stessi vettori nei test Rust qui sotto e in
//! `tests/updater.test.mjs`).

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
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
    #[serde(default, alias = "assetPatterns")]
    asset_patterns: Vec<String>,
}

const EMBEDDED_CONFIG: &str = include_str!("../../updater.config.json");

/// Config effettiva esposta al frontend.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateConfig {
    pub owner: String,
    pub repo: String,
    pub configured: bool,
    pub current_version: String,
    pub api_url: String,
    pub asset_patterns: Vec<String>,
    pub message: String,
}

pub fn default_asset_patterns() -> Vec<String> {
    vec![
        ".msi".to_string(),
        "setup.exe".to_string(),
        "win-x64.exe".to_string(),
        "windows-x86_64.exe".to_string(),
        ".exe".to_string(),
    ]
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
        asset_patterns: Vec::new(),
    });
    let owner = env_override("CLI_COMPANION_UPDATE_OWNER").unwrap_or(file.owner);
    let repo = env_override("CLI_COMPANION_UPDATE_REPO").unwrap_or(file.repo);
    let api_url = env_override("CLI_COMPANION_UPDATE_API_URL")
        .or(file.api_url)
        .unwrap_or_else(|| "https://api.github.com".to_string());
    let mut asset_patterns = file.asset_patterns;
    if asset_patterns.is_empty() {
        asset_patterns = default_asset_patterns();
    }
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
        asset_patterns,
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
pub struct GithubAsset {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub browser_download_url: String,
}

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
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetInfo {
    pub name: String,
    pub size: u64,
    pub url: String,
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
    pub asset: Option<AssetInfo>,
}

fn curl_bin() -> String {
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
fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn run_curl(args: &[String]) -> Result<Vec<u8>, String> {
    let bin = curl_bin();
    let out = quiet(&mut Command::new(&bin)).args(args).output().map_err(|e| {
        format!("Impossibile eseguire '{bin}' (curl non trovato nel PATH?): {e}. Su Windows 10+ curl.exe è preinstallato.")
    })?;
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

/// Sceglie l'installer: primo pattern (in ordine) che matcha un nome asset
/// (substring, case-insensitive). `None` se nessun pattern matcha.
pub fn pick_installer_asset<'a>(
    assets: &'a [GithubAsset],
    patterns: &[String],
) -> Option<&'a GithubAsset> {
    for pat in patterns {
        let p = pat.to_lowercase();
        if p.is_empty() {
            continue;
        }
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_lowercase().contains(&p))
        {
            return Some(a);
        }
    }
    None
}

/// Check completo: config -> latest release -> confronto versioni -> asset.
pub fn check_blocking() -> Result<UpdateStatus, String> {
    let cfg = load_config();
    let release = fetch_latest_release(&cfg)?;
    let latest = normalize_version(&release.tag_name);
    let current = current_version().to_string();
    let asset = pick_installer_asset(&release.assets, &cfg.asset_patterns).map(|a| AssetInfo {
        name: a.name.clone(),
        size: a.size,
        url: a.browser_download_url.clone(),
    });
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
        asset,
    })
}

// ---------------------------------------------------------------------------
// Download + verifica + installazione sicura
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct DownloadResult {
    pub file_path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub verified: bool,
    pub release_version: String,
    pub release_url: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub launched: bool,
    pub file_path: String,
    pub message: String,
}

/// Cartella temporanea dei download (creata se manca).
pub fn update_temp_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("cli-companion-update");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Impossibile creare {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Scarica `url` in `dest` con curl. Su failure rimuove il parziale (rollback).
fn download_file(url: &str, dest: &Path) -> Result<u64, String> {
    let args = vec![
        "-sSL".to_string(),
        "--fail".to_string(),
        "--max-time".to_string(),
        "600".to_string(),
        "-H".to_string(),
        "User-Agent: cli-companion-updater".to_string(),
        "-o".to_string(),
        dest.to_string_lossy().to_string(),
        url.to_string(),
    ];
    if let Err(e) = run_curl(&args) {
        let _ = std::fs::remove_file(dest);
        return Err(format!("Download fallito da {url}: {e}"));
    }
    let meta =
        std::fs::metadata(dest).map_err(|e| format!("File scaricato illeggibile: {e}"))?;
    if meta.len() == 0 {
        let _ = std::fs::remove_file(dest);
        return Err("Download fallito: file vuoto (rimosso).".to_string());
    }
    Ok(meta.len())
}

/// Cerca l'asset checksum: prima `<asset>.sha256`, poi un file `*SHA256SUMS*`.
fn find_checksum_asset<'a>(
    assets: &'a [GithubAsset],
    asset_name: &str,
) -> Option<&'a GithubAsset> {
    let want = format!("{asset_name}.sha256");
    if let Some(a) = assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(&want))
    {
        return Some(a);
    }
    assets.iter().find(|a| {
        let n = a.name.to_lowercase();
        n.contains("sha256") && (n.contains("sums") || n.ends_with(".txt"))
    })
}

/// Interpreta un file checksum: o singolo hash, o formato `SHA256SUMS`
/// (`<hash>  <nomefile>` per riga). Ritorna l'hash atteso per `asset_name`.
fn parse_checksum_file(text: &str, asset_name: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let is_hex64 = |s: &str| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit());
    // Caso 1: il file contiene solo l'hash.
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.len() == 1 && is_hex64(tokens[0]) {
        return Some(tokens[0].to_lowercase());
    }
    // Caso 2: formato SHA256SUMS, una riga per file.
    for line in trimmed.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() >= 2 && is_hex64(cols[0]) {
            let name = cols[1].trim_start_matches('*');
            if name.eq_ignore_ascii_case(asset_name) {
                return Some(cols[0].to_lowercase());
            }
        }
    }
    None
}

fn asset_names(release: &GithubRelease) -> String {
    if release.assets.is_empty() {
        return "(nessun asset)".to_string();
    }
    release
        .assets
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Scarica l'installer della latest release e lo verifica.
/// `asset_name`: nome esatto da preferire; `None`/vuoto = scelta automatica.
pub fn download_update_blocking(asset_name: Option<String>) -> Result<DownloadResult, String> {
    let cfg = load_config();
    let release = fetch_latest_release(&cfg)?;
    let wanted = asset_name.unwrap_or_default();
    let asset: &GithubAsset = if !wanted.trim().is_empty() {
        release
            .assets
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(wanted.trim()))
            .ok_or_else(|| {
                format!(
                    "Asset '{}' non trovato nella release {}. Disponibili: {}",
                    wanted.trim(),
                    release.tag_name,
                    asset_names(&release)
                )
            })?
    } else {
        pick_installer_asset(&release.assets, &cfg.asset_patterns)
            .or_else(|| release.assets.first())
            .ok_or_else(|| {
                format!(
                    "La release {} non contiene file scaricabili.",
                    release.tag_name
                )
            })?
    };

    let dir = update_temp_dir()?;
    let dest = dir.join(&asset.name);
    let size = download_file(&asset.browser_download_url, &dest)?;
    let digest = sha256_file(&dest)?;

    // Verifica checksum se la release la pubblica.
    let mut verified = false;
    let mut note =
        "Checksum non pubblicata nella release: verificata solo la dimensione del file."
            .to_string();
    if let Some(sum_asset) = find_checksum_asset(&release.assets, &asset.name) {
        let sum_path = dir.join(&sum_asset.name);
        match download_file(&sum_asset.browser_download_url, &sum_path) {
            Ok(_) => {
                let text = std::fs::read_to_string(&sum_path).unwrap_or_default();
                match parse_checksum_file(&text, &asset.name) {
                    Some(expected) if expected.eq_ignore_ascii_case(&digest) => {
                        verified = true;
                        note = "Checksum SHA256 verificata con successo.".to_string();
                    }
                    Some(expected) => {
                        let _ = std::fs::remove_file(&dest); // rollback
                        return Err(format!(
                            "Checksum NON valida per {}: atteso {expected}, calcolato {digest}. \
                             File eliminato, installazione corrente intatta.",
                            asset.name
                        ));
                    }
                    None => {
                        note = format!(
                            "File checksum '{}' non interpretabile: confronto saltato.",
                            sum_asset.name
                        );
                    }
                }
            }
            Err(e) => {
                note = format!("Checksum non scaricabile ({e}): confronto saltato.");
            }
        }
    }

    let short = digest[..16.min(digest.len())].to_string();
    Ok(DownloadResult {
        file_path: dest.to_string_lossy().to_string(),
        file_name: asset.name.clone(),
        size_bytes: size,
        sha256: digest,
        verified,
        release_version: normalize_version(&release.tag_name),
        release_url: release.html_url.clone(),
        message: format!("Scaricato {} ({} byte, sha256 {short}…). {note}", asset.name, size),
    })
}

/// Prepara/avvia l'installazione. Non tocca mai l'app in esecuzione:
/// con `launch=true` avvia l'installer e l'utente completa la procedura,
/// con `launch=false` restituisce solo il percorso per l'avvio manuale.
pub fn install_update_blocking(file_path: String, launch: bool) -> Result<InstallResult, String> {
    let path = PathBuf::from(file_path.trim());
    let meta = std::fs::metadata(&path).map_err(|_| {
        format!(
            "File '{}' non trovato: riesegui il download prima di installare.",
            path.display()
        )
    })?;
    if meta.len() == 0 {
        return Err("File vuoto: riesegui il download prima di installare.".to_string());
    }
    if !launch {
        return Ok(InstallResult {
            launched: false,
            file_path: path.to_string_lossy().to_string(),
            message: format!(
                "Pronto da installare: {}. Chiudi l'app ed esegui il file per aggiornare. \
                 Finché non completi l'installer, l'installazione corrente resta invariata.",
                path.display()
            ),
        });
    }
    launch_detached(&path)?;
    Ok(InstallResult {
        launched: true,
        file_path: path.to_string_lossy().to_string(),
        message: "Installer avviato: completa la procedura guidata, poi riavvia l'app. \
                  Se l'installer fallisce, l'installazione corrente resta invariata."
            .to_string(),
    })
}

#[cfg(target_os = "windows")]
fn launch_detached(path: &Path) -> Result<(), String> {
    quiet(&mut Command::new("cmd"))
        .args(["/C", "start", "", &path.to_string_lossy()])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Avvio installer fallito: {e}"))
}

#[cfg(target_os = "macos")]
fn launch_detached(path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Avvio installer fallito: {e}"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn launch_detached(path: &Path) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Avvio installer fallito: {e}"))
}

// ---------------------------------------------------------------------------
// SHA256 puro (nessuna dipendenza) — FIPS 180-4
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    sha256_bytes(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

/// SHA256 esadecimale di un file.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let data =
        std::fs::read(path).map_err(|e| format!("Lettura file per SHA256 fallita: {e}"))?;
    Ok(sha256_hex(&data))
}

// ---------------------------------------------------------------------------
// Comandi Tauri (ogni comando deve avere il corrispondente invoke in
// src/lib/tauri.ts: vedi tests/tauri-args.test.mjs)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_update_config() -> UpdateConfig {
    load_config()
}

// Async + spawn_blocking: curl puo' durare fino a 25 s (check) o 600 s
// (download); sul main thread congelava la finestra, anche all'avvio.
#[tauri::command]
pub async fn check_update() -> Result<UpdateStatus, String> {
    crate::blocking(check_blocking).await
}

#[tauri::command]
pub async fn download_update(asset_name: Option<String>) -> Result<DownloadResult, String> {
    crate::blocking(move || download_update_blocking(asset_name)).await
}

#[tauri::command]
pub async fn install_update(file_path: String, launch: bool) -> Result<InstallResult, String> {
    crate::blocking(move || install_update_blocking(file_path, launch)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // I test che leggono/scrivono le env CLI_COMPANION_UPDATE_* devono girare
    // in mutua esclusione (cargo test è parallelo per default).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            size: 1,
            browser_download_url: "https://example.invalid/x".to_string(),
        }
    }

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
    fn sha256_vettori_noti() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn checksum_parse_singolo_e_sums() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(parse_checksum_file(h, "app.exe"), Some(h.to_string()));
        let sums = format!("{h}  app.exe\n{h} *altro.msi\n");
        assert_eq!(
            parse_checksum_file(&sums, "app.exe"),
            Some(h.to_string())
        );
        assert_eq!(parse_checksum_file(&sums, "manca.exe"), None);
        assert_eq!(parse_checksum_file("spazzatura", "app.exe"), None);
        assert_eq!(parse_checksum_file("", "app.exe"), None);
    }

    #[test]
    fn picker_installer_rispetta_pattern() {
        let assets = vec![
            asset("app-0.2.0.exe"),
            asset("app-0.2.0.msi"),
            asset("app.AppImage"),
        ];
        let pats = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pick_installer_asset(&assets, &pats(&[".msi", ".exe"])).unwrap().name,
            "app-0.2.0.msi"
        );
        assert_eq!(
            pick_installer_asset(&assets, &pats(&[".exe"])).unwrap().name,
            "app-0.2.0.exe"
        );
        assert!(pick_installer_asset(&assets, &pats(&[".dmg"])).is_none());
        // Case-insensitive.
        assert_eq!(
            pick_installer_asset(&assets, &pats(&["MSI"])).unwrap().name,
            "app-0.2.0.msi"
        );
    }

    #[test]
    fn config_embedded_si_legge_e_placeholder_rilevato() {
        let _guard = ENV_LOCK.lock().unwrap();
        // Assicura che nessun override d'ambiente sporchi questo test.
        std::env::remove_var("CLI_COMPANION_UPDATE_OWNER");
        std::env::remove_var("CLI_COMPANION_UPDATE_REPO");
        let cfg = load_config();
        assert_eq!(cfg.repo, "cli-companion");
        assert!(!cfg.asset_patterns.is_empty());
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
        raw.extend_from_slice(br#"{"tag_name":"v0.2.0","assets":[]}"#);
        let r = parse_release(&raw, "mock").unwrap();
        assert_eq!(r.tag_name, "v0.2.0");
        assert!(r.assets.is_empty());
        assert!(parse_release(b"non-json", "mock").is_err());
    }

    #[test]
    fn download_verifica_checksum_e_rollback_su_file_locali() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join("cli-companion-updater-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file_url = |p: &std::path::Path| {
            format!(
                "file:///{}",
                p.to_string_lossy().replace('\\', "/").replace(' ', "%20")
            )
        };
        let write_release = |tag: &str, entries: &[(&std::path::Path, &str)]| {
            let assets: Vec<serde_json::Value> = entries
                .iter()
                .map(|(p, name)| {
                    serde_json::json!({
                        "name": name,
                        "size": 1,
                        "browser_download_url": file_url(p),
                    })
                })
                .collect();
            let rel = dir.join(format!("release-{tag}.json"));
            std::fs::write(
                &rel,
                serde_json::json!({
                    "tag_name": tag,
                    "html_url": "https://example.invalid/r",
                    "assets": assets,
                })
                .to_string(),
            )
            .unwrap();
            rel
        };

        std::env::set_var("CLI_COMPANION_UPDATE_OWNER", "acme-test");

        // Caso OK: checksum valida -> verified=true, file presente.
        let payload = dir.join("ok-payload-t1.msi");
        std::fs::write(&payload, b"fake-installer-bytes-t1").unwrap();
        let digest = sha256_file(&payload).unwrap();
        let sum = dir.join("ok-payload-t1.msi.sha256");
        std::fs::write(&sum, &digest).unwrap();
        let rel_ok = write_release(
            "v9.9.9-ok",
            &[(&payload, "ok-payload-t1.msi"), (&sum, "ok-payload-t1.msi.sha256")],
        );
        std::env::set_var("CLI_COMPANION_UPDATE_LATEST_URL", file_url(&rel_ok));
        let res = download_update_blocking(None).expect("download ok");
        assert!(res.verified, "checksum valida non riconosciuta");
        assert_eq!(res.sha256, digest);
        assert!(Path::new(&res.file_path).exists());
        let _ = std::fs::remove_file(&res.file_path);

        // Caso mismatch: errore chiaro + rollback (file rimosso).
        let payload2 = dir.join("bad-payload-t1.exe");
        std::fs::write(&payload2, b"other-bytes").unwrap();
        let sum2 = dir.join("bad-payload-t1.exe.sha256");
        std::fs::write(&sum2, "0".repeat(64)).unwrap();
        let rel_bad = write_release(
            "v9.9.9-bad",
            &[
                (&payload2, "bad-payload-t1.exe"),
                (&sum2, "bad-payload-t1.exe.sha256"),
            ],
        );
        std::env::set_var("CLI_COMPANION_UPDATE_LATEST_URL", file_url(&rel_bad));
        let err = download_update_blocking(None).unwrap_err();
        assert!(err.contains("NON valida"), "{err}");
        let dest = update_temp_dir().unwrap().join("bad-payload-t1.exe");
        assert!(!dest.exists(), "rollback mancato: {dest:?} ancora presente");

        std::env::remove_var("CLI_COMPANION_UPDATE_OWNER");
        std::env::remove_var("CLI_COMPANION_UPDATE_LATEST_URL");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_su_file_mancante_messaggio_chiaro() {
        let err =
            install_update_blocking("Z:/non/esiste/app-9.9.9.exe".to_string(), false).unwrap_err();
        assert!(err.contains("riesegui il download"), "{err}");
    }
}
