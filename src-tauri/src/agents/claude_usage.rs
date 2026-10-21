//! Consumi di Claude Code: limiti dell'abbonamento (5 ore e settimanale) e
//! token di una sessione letti dal transcript.
//!
//! Limiti: stesso endpoint (non documentato) che usa `/usage`, chiamato con
//! curl e il token OAuth di `~/.claude/.credentials.json`. Il token non va mai
//! sulla riga di comando (l'header passa da stdin) e NON lo rinnoviamo noi:
//! il refresh ruoterebbe il refresh token e romperebbe il login di Claude
//! Code. Token scaduto = stato "expired" finche' Claude Code non lo rinnova.
//!
//! Token: i transcript `projects/<slug>/<id>.jsonl` (e i sub-agent in
//! `<id>/subagents/*.jsonl`) sono append-only; la cache tiene per ogni file
//! l'offset letto, cosi' ad ogni chiamata si leggono solo i byte nuovi.

use super::{Env, LimitWindow, SessionUsage, TokenTotals, UsageLimitsReply};
use crate::wsl::wsl_to_windows_path;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Un risultato valido si riusa per questo tempo.
const LIMITS_FRESH: Duration = Duration::from_secs(3 * 60);
/// Dopo un 429 non si richiama l'endpoint per questo tempo.
const LIMITS_BACKOFF: Duration = Duration::from_secs(10 * 60);
/// Oltre questo numero di file in cache la si svuota (sessioni vecchie).
const CACHE_MAX_FILES: usize = 512;

// ---------------------------------------------------------------------------
// Credenziali
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum Creds {
    Token(String),
    Expired,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Legge `claudeAiOauth` dal JSON delle credenziali. `Err` = messaggio per
/// l'utente (nessun abbonamento / file illeggibile). Il token non finisce
/// mai nei messaggi.
fn parse_credentials(raw: &str, now_ms: u64) -> Result<Creds, String> {
    let v: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|_| "File delle credenziali di Claude Code non leggibile.".to_string())?;
    let oauth = v
        .get("claudeAiOauth")
        .filter(|o| o.is_object())
        .ok_or_else(|| "Nessun abbonamento Claude: i limiti non sono disponibili con una API key.".to_string())?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "Token di accesso di Claude Code assente: rifai il login.".to_string())?;
    let expires = oauth
        .get("expiresAt")
        .and_then(|e| e.as_u64().or_else(|| e.as_f64().map(|f| f.max(0.0) as u64)));
    if expires.is_some_and(|e| e <= now_ms) {
        return Ok(Creds::Expired);
    }
    Ok(Creds::Token(token.to_string()))
}

/// Cartella `.claude` su Windows: `CLAUDE_CONFIG_DIR` se impostata.
fn windows_claude_dir() -> Result<PathBuf, String> {
    let var = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty());
    if let Some(dir) = var("CLAUDE_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    var("USERPROFILE")
        .or_else(|| var("HOME"))
        .map(|h| PathBuf::from(h).join(".claude"))
        .ok_or_else(|| "Cartella utente non trovata.".to_string())
}

/// `$HOME` dell'utente di default della distro, risolto una volta per distro.
fn wsl_home(distro: &str) -> Result<String, String> {
    static HOMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let homes = HOMES.get_or_init(Default::default);
    if let Some(h) = homes.lock().unwrap().get(distro) {
        return Ok(h.clone());
    }
    let out = Env::Wsl(distro.to_string())
        .run("sh", &["-c", "printf %s \"$HOME\""])
        .map_err(|e| format!("Home della distro non risolta: {e}"))?;
    let home = out.trim().to_string();
    if !home.starts_with('/') || home.contains(['\n', '\r']) || home.split('/').any(|c| c == "..") {
        return Err("Home della distro non valida.".to_string());
    }
    homes.lock().unwrap().insert(distro.to_string(), home.clone());
    Ok(home)
}

/// File delle credenziali da provare, in ordine.
fn credentials_candidates(env: &Env) -> Result<Vec<PathBuf>, String> {
    match env {
        Env::Windows => Ok(vec![windows_claude_dir()?.join(".credentials.json")]),
        Env::Wsl(d) => {
            let home = wsl_home(d)?.replace('/', "\\");
            Ok(["wsl.localhost", "wsl$"]
                .iter()
                .map(|host| PathBuf::from(format!("\\\\{host}\\{d}{home}\\.claude\\.credentials.json")))
                .collect())
        }
    }
}

fn read_credentials(env: &Env) -> Result<String, String> {
    let mut last = None;
    for path in credentials_candidates(env)? {
        match std::fs::read_to_string(&path) {
            Ok(raw) => return Ok(raw),
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) if e.kind() != std::io::ErrorKind::NotFound => format!("Credenziali di Claude Code non leggibili: {e}"),
        _ => "Credenziali di Claude Code non trovate: serve il login con un abbonamento.".to_string(),
    })
}

// ---------------------------------------------------------------------------
// Limiti
// ---------------------------------------------------------------------------

fn parse_window(v: Option<&Value>) -> Option<LimitWindow> {
    let w = v?.as_object()?;
    let used = w.get("utilization")?.as_f64()?;
    Some(LimitWindow {
        used_percentage: used.max(0.0),
        resets_at: w.get("resets_at").and_then(Value::as_str).map(str::to_string),
    })
}

/// Interpreta la risposta dell'endpoint. Tollera campi mancanti o null;
/// `None` se non c'e' nessuna finestra riconoscibile.
fn parse_limits(body: &str) -> Option<UsageLimitsReply> {
    let v: Value = serde_json::from_str(body.trim_start_matches('\u{feff}').trim()).ok()?;
    let five_hour = parse_window(v.get("five_hour"));
    let seven_day = parse_window(v.get("seven_day"));
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(UsageLimitsReply { state: "ok".to_string(), five_hour, seven_day, message: None })
}

/// Esito di una richiesta all'endpoint.
#[derive(Debug)]
enum Fetched {
    Ok(UsageLimitsReply),
    Expired,
    RateLimited,
    Failed(String),
}

/// Corpo e codice HTTP dall'output di curl (`corpo\n<codice>`).
fn split_status(out: &str) -> Option<(&str, u16)> {
    let (body, code) = out.trim_end().rsplit_once('\n')?;
    Some((body, code.trim().parse().ok()?))
}

fn classify_response(out: &str) -> Fetched {
    let Some((body, code)) = split_status(out) else {
        return Fetched::Failed("Risposta non riconosciuta.".to_string());
    };
    match code {
        200 => parse_limits(body)
            .map(Fetched::Ok)
            .unwrap_or_else(|| Fetched::Failed("Risposta non riconosciuta.".to_string())),
        401 | 403 => Fetched::Expired,
        429 => Fetched::RateLimited,
        c => Fetched::Failed(format!("Servizio non disponibile (HTTP {c}).")),
    }
}

fn fetch_limits_http(token: &str) -> Fetched {
    let args: Vec<String> = [
        "-s",
        "-m",
        "15",
        "-H",
        "@-",
        "-H",
        "anthropic-beta: oauth-2025-04-20",
        "-H",
        concat!("User-Agent: cli-companion/", env!("CARGO_PKG_VERSION")),
        "-w",
        "\n%{http_code}",
        USAGE_URL,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // L'header segreto passa da stdin: non compare nella lista dei processi.
    match crate::updater::run_curl_stdin(&args, format!("Authorization: Bearer {token}\n").as_bytes()) {
        Ok(out) => classify_response(&String::from_utf8_lossy(&out)),
        Err(e) => Fetched::Failed(format!("Richiesta non riuscita: {e}")),
    }
}

fn unavailable(message: &str) -> UsageLimitsReply {
    UsageLimitsReply::status("unavailable", Some(message))
}

fn expired() -> UsageLimitsReply {
    UsageLimitsReply::status("expired", Some("Accesso scaduto: usa Claude Code per rinnovarlo."))
}

/// Cache dei limiti di un ambiente.
#[derive(Default)]
struct LimitsEntry {
    good: Option<(Instant, UsageLimitsReply)>,
    backoff_until: Option<Instant>,
}

impl LimitsEntry {
    fn fresh(&self, now: Instant) -> Option<UsageLimitsReply> {
        self.good
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) < LIMITS_FRESH)
            .map(|(_, r)| r.clone())
    }

    fn backing_off(&self, now: Instant) -> bool {
        self.backoff_until.is_some_and(|until| now < until)
    }

    /// Ultimo valore valido (anche vecchio), altrimenti "unavailable".
    fn fallback(&self, message: &str) -> UsageLimitsReply {
        self.good.as_ref().map(|(_, r)| r.clone()).unwrap_or_else(|| unavailable(message))
    }

    fn apply(&mut self, now: Instant, fetched: Fetched) -> UsageLimitsReply {
        match fetched {
            Fetched::Ok(reply) => {
                self.backoff_until = None;
                self.good = Some((now, reply.clone()));
                reply
            }
            Fetched::Expired => {
                self.good = None;
                expired()
            }
            Fetched::RateLimited => {
                self.backoff_until = Some(now + LIMITS_BACKOFF);
                self.fallback("Troppe richieste: riprovo tra qualche minuto.")
            }
            Fetched::Failed(m) => self.fallback(&m),
        }
    }
}

fn limits_cache() -> &'static Mutex<HashMap<String, LimitsEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, LimitsEntry>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Limiti dell'abbonamento per `env`, con cache (3 min) e pausa dopo un 429
/// (10 min). Bloccante: chiamare fuori dal main thread.
pub fn limits(env: &Env) -> UsageLimitsReply {
    let key = env.key();
    {
        let mut cache = limits_cache().lock().unwrap();
        let entry = cache.entry(key.clone()).or_default();
        let now = Instant::now();
        if let Some(r) = entry.fresh(now) {
            return r;
        }
        if entry.backing_off(now) {
            return entry.fallback("Troppe richieste: riprovo tra qualche minuto.");
        }
    }
    // Credenziali e rete fuori dal lock.
    let fetched = match read_credentials(env).and_then(|raw| parse_credentials(&raw, now_ms())) {
        Err(m) => return unavailable(&m),
        Ok(Creds::Expired) => Fetched::Expired,
        Ok(Creds::Token(t)) => fetch_limits_http(&t),
    };
    let mut cache = limits_cache().lock().unwrap();
    cache.entry(key).or_default().apply(Instant::now(), fetched)
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct MsgUsage {
    totals: TokenTotals,
    sidechain: bool,
}

/// Stato di lettura di un file `.jsonl`.
#[derive(Default)]
struct FileState {
    /// Byte gia' elaborati (fine dell'ultima riga completa).
    offset: u64,
    /// Lunghezza vista all'ultima lettura: se cala, il file e' stato sostituito.
    len: u64,
    /// Ultimo uso visto per ogni id messaggio (un messaggio puo' stare su
    /// piu' righe: vale l'ultima).
    msgs: HashMap<String, MsgUsage>,
    /// Id e modello dell'ultimo messaggio della catena principale.
    last_main: Option<(String, Option<String>)>,
}

/// Cache dei transcript letti, condivisa (stato gestito da Tauri). Un lock
/// per file: quello globale serve solo a prendere/inserire/sfrattare le voci,
/// la lettura del disco non serializza le sessioni.
#[derive(Clone, Default)]
pub struct TranscriptCache(Arc<Mutex<HashMap<PathBuf, Arc<Mutex<FileState>>>>>);

impl TranscriptCache {
    /// Stato del file `path` (creato se manca); sfratta tutto oltre il limite.
    fn entry(&self, path: &Path) -> Arc<Mutex<FileState>> {
        let mut files = self.0.lock().unwrap();
        if files.len() > CACHE_MAX_FILES {
            files.clear();
        }
        files.entry(path.to_path_buf()).or_default().clone()
    }
}

fn token_field(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn process_line(state: &mut FileState, line: &[u8], line_offset: u64) {
    let Ok(text) = std::str::from_utf8(line) else {
        return;
    };
    // Scarto veloce: solo le righe assistant con usage interessano.
    if !text.contains("\"usage\"") {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let Some(msg) = v.get("message") else {
        return;
    };
    let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else {
        return;
    };
    let model = msg.get("model").and_then(Value::as_str);
    // Messaggi sintetici (errori locali): niente token, falserebbero il contesto.
    if model == Some("<synthetic>") {
        return;
    }
    let id = msg
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("anon:{line_offset}"));
    let sidechain = v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
    let totals = TokenTotals {
        input_tokens: token_field(usage, "input_tokens"),
        output_tokens: token_field(usage, "output_tokens"),
        cache_read_tokens: token_field(usage, "cache_read_input_tokens"),
        cache_creation_tokens: token_field(usage, "cache_creation_input_tokens"),
    };
    if !sidechain {
        state.last_main = Some((id.clone(), model.map(str::to_string)));
    }
    state.msgs.insert(id, MsgUsage { totals, sidechain });
}

/// Legge i byte nuovi di `path` nello stato. Una riga senza `\n` finale (il
/// file e' in scrittura) non viene consumata: si rilegge alla chiamata dopo.
fn refresh(state: &mut FileState, path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < state.len || len < state.offset {
        *state = FileState::default();
    }
    state.len = len;
    if len == state.offset {
        return Ok(());
    }
    file.seek(SeekFrom::Start(state.offset))?;
    let mut reader = BufReader::new(file.take(len - state.offset));
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        process_line(state, &buf, state.offset);
        state.offset += n as u64;
    }
    Ok(())
}

fn add(a: &mut TokenTotals, b: &TokenTotals) {
    a.input_tokens += b.input_tokens;
    a.output_tokens += b.output_tokens;
    a.cache_read_tokens += b.cache_read_tokens;
    a.cache_creation_tokens += b.cache_creation_tokens;
}

/// Consumi di `transcript` (e dei sub-agent nella cartella accanto),
/// aggiornando `cache` in modo incrementale.
fn read_usage(cache: &TranscriptCache, transcript: &Path) -> Result<SessionUsage, String> {
    let main = cache.entry(transcript);
    let mut main = main.lock().unwrap();
    refresh(&mut main, transcript).map_err(|e| format!("Transcript non leggibile: {e}"))?;

    let mut usage = SessionUsage::default();
    let mut count = 0u32;
    for m in main.msgs.values() {
        if m.sidechain {
            add(&mut usage.subagents, &m.totals);
        } else {
            add(&mut usage.main, &m.totals);
            count += 1;
        }
    }
    usage.messages = count;
    if let Some((id, model)) = &main.last_main {
        if let Some(m) = main.msgs.get(id) {
            usage.context_tokens = m.totals.input_tokens + m.totals.cache_read_tokens + m.totals.cache_creation_tokens;
        }
        usage.model = model.clone();
    }

    let sub_dir = transcript.with_extension("").join("subagents");
    if let Ok(entries) = std::fs::read_dir(&sub_dir) {
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let state = cache.entry(&path);
            let mut state = state.lock().unwrap();
            if refresh(&mut state, &path).is_err() {
                continue;
            }
            for m in state.msgs.values() {
                add(&mut usage.subagents, &m.totals);
            }
        }
    }
    Ok(usage)
}

/// Controllo sintattico del path del transcript, come lo vede l'agente
/// (Windows o Linux): niente `..`, estensione `.jsonl` e forma
/// `.../projects/<slug>/<id>.jsonl`.
fn validate_transcript_path(p: &str) -> Result<(), String> {
    let comps: Vec<&str> = p.trim().split(['/', '\\']).filter(|c| !c.is_empty()).collect();
    let bad = || Err("Percorso del transcript non valido.".to_string());
    if p.contains('\0') || comps.contains(&"..") {
        return bad();
    }
    let n = comps.len();
    let file_ok = comps.last().is_some_and(|f| f.len() > ".jsonl".len() && f.ends_with(".jsonl"));
    if !file_ok || n < 3 || comps[n - 3] != "projects" {
        return bad();
    }
    Ok(())
}

/// Path del transcript leggibile dall'host: invariato su Windows, UNC (o
/// `C:\` per `/mnt/c`) per una distro WSL.
fn resolve_transcript_path(env: &Env, p: &str) -> Result<PathBuf, String> {
    validate_transcript_path(p)?;
    let p = p.trim();
    match env {
        Env::Windows if p.starts_with('/') => Err("Percorso del transcript non valido.".to_string()),
        Env::Windows => Ok(PathBuf::from(p)),
        Env::Wsl(d) => wsl_to_windows_path(d, p)
            .map(PathBuf::from)
            .ok_or_else(|| "Percorso del transcript non valido.".to_string()),
    }
}

/// Consumi di token della sessione con transcript `transcript_path`.
pub fn session_usage(env: &Env, transcript_path: &str, cache: &TranscriptCache) -> Result<SessionUsage, String> {
    let path = resolve_transcript_path(env, transcript_path)?;
    read_usage(cache, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // --- credenziali ---

    #[test]
    fn credentials_token_and_expiry() {
        let json = |exp: &str| {
            format!(r#"{{"claudeAiOauth":{{"accessToken":"tok","expiresAt":{exp},"subscriptionType":"max"}}}}"#)
        };
        assert_eq!(parse_credentials(&json("2000"), 1000), Ok(Creds::Token("tok".into())));
        assert_eq!(parse_credentials(&json("1000"), 1000), Ok(Creds::Expired));
        assert_eq!(parse_credentials(&json("500"), 1000), Ok(Creds::Expired));
        assert_eq!(parse_credentials(&json("2000.0"), 1000), Ok(Creds::Token("tok".into())));
        // expiresAt assente: non scaduto.
        let no_exp = r#"{"claudeAiOauth":{"accessToken":"tok"}}"#;
        assert_eq!(parse_credentials(no_exp, 1000), Ok(Creds::Token("tok".into())));
        // BOM tollerato.
        assert!(parse_credentials(&format!("\u{feff}{}", json("2000")), 1000).is_ok());
    }

    #[test]
    fn credentials_errors_never_leak_token() {
        for raw in [
            "",
            "non json",
            "{}",
            r#"{"claudeAiOauth":null}"#,
            r#"{"claudeAiOauth":{"accessToken":""}}"#,
            r#"{"claudeAiOauth":{"expiresAt":5}}"#,
            r#"{"apiKey":"sk-secret"}"#,
        ] {
            let e = parse_credentials(raw, 0).unwrap_err();
            assert!(!e.contains("secret"), "{raw}");
        }
    }

    // --- limiti ---

    #[test]
    fn parse_limits_full_nulls_and_garbage() {
        let full = r#"{"five_hour":{"utilization":45.0,"resets_at":"2026-10-21T12:20:00.494382+00:00"},
            "seven_day":{"utilization":2,"resets_at":"2026-10-28T10:00:00.494406+00:00"},"seven_day_opus":null}"#;
        let r = parse_limits(full).unwrap();
        assert_eq!(r.state, "ok");
        let (f, s) = (r.five_hour.unwrap(), r.seven_day.unwrap());
        assert_eq!(f.used_percentage, 45.0);
        assert_eq!(f.resets_at.as_deref(), Some("2026-10-21T12:20:00.494382+00:00"));
        assert_eq!(s.used_percentage, 2.0);

        // Una finestra null / mancante: l'altra resta.
        let r = parse_limits(r#"{"five_hour":null,"seven_day":{"utilization":10.5,"resets_at":null}}"#).unwrap();
        assert!(r.five_hour.is_none());
        let s = r.seven_day.unwrap();
        assert_eq!((s.used_percentage, s.resets_at), (10.5, None));
        let r = parse_limits(r#"{"five_hour":{"utilization":1}}"#).unwrap();
        assert!(r.five_hour.is_some() && r.seven_day.is_none());

        // Niente di riconoscibile.
        for bad in [
            "",
            "garbage",
            "[]",
            "{}",
            r#"{"five_hour":null,"seven_day":null}"#,
            r#"{"five_hour":{"utilization":"x"}}"#,
            r#"{"five_hour":{"resets_at":"2026"}}"#,
            r#"{"five_hour":5}"#,
        ] {
            assert!(parse_limits(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn http_status_classification() {
        let body = r#"{"five_hour":{"utilization":3.0,"resets_at":null}}"#;
        assert!(matches!(classify_response(&format!("{body}\n200")), Fetched::Ok(_)));
        assert!(matches!(classify_response("{}\n401"), Fetched::Expired));
        assert!(matches!(classify_response("{}\n429"), Fetched::RateLimited));
        assert!(matches!(classify_response("x\n500"), Fetched::Failed(_)));
        assert!(matches!(classify_response("{}\n200"), Fetched::Failed(_)));
        assert!(matches!(classify_response("senza codice"), Fetched::Failed(_)));
        assert!(matches!(classify_response(""), Fetched::Failed(_)));
    }

    fn ok_reply(p: f64) -> UsageLimitsReply {
        UsageLimitsReply {
            state: "ok".into(),
            five_hour: Some(LimitWindow { used_percentage: p, resets_at: None }),
            seven_day: None,
            message: None,
        }
    }

    #[test]
    fn limits_cache_fresh_backoff_and_fallback() {
        let t0 = Instant::now();
        let mut e = LimitsEntry::default();
        assert!(e.fresh(t0).is_none());
        // 429 senza valore precedente: unavailable + pausa.
        let r = e.apply(t0, Fetched::RateLimited);
        assert_eq!(r.state, "unavailable");
        assert!(e.backing_off(t0 + Duration::from_secs(599)));
        assert!(!e.backing_off(t0 + Duration::from_secs(601)));
        // Successo: fresco per 3 minuti, poi no; la pausa si azzera.
        assert_eq!(e.apply(t0, Fetched::Ok(ok_reply(10.0))), ok_reply(10.0));
        assert!(!e.backing_off(t0));
        assert!(e.fresh(t0 + Duration::from_secs(179)).is_some());
        assert!(e.fresh(t0 + Duration::from_secs(181)).is_none());
        // 429 con valore precedente: ritorna l'ultimo buono.
        assert_eq!(e.apply(t0, Fetched::RateLimited), ok_reply(10.0));
        // Errore transitorio: idem. Scaduto: stato expired e valore scartato.
        assert_eq!(e.apply(t0, Fetched::Failed("x".into())), ok_reply(10.0));
        assert_eq!(e.apply(t0, Fetched::Expired).state, "expired");
        assert_eq!(e.apply(t0, Fetched::Failed("x".into())).state, "unavailable");
    }

    // --- transcript ---

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mc-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn line(id: &str, side: bool, usage: (u64, u64, u64, u64), model: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "type": "assistant",
                "isSidechain": side,
                "message": {"id": id, "model": model, "usage": {
                    "input_tokens": usage.0, "output_tokens": usage.1,
                    "cache_read_input_tokens": usage.2, "cache_creation_input_tokens": usage.3}}
            })
        )
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    fn session_file(dir: &Path) -> PathBuf {
        let p = dir.join("projects").join("slug").join("sess.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    }

    #[test]
    fn duplicate_ids_keep_last_usage_and_context_from_last_main() {
        let dir = tmp("dup");
        let p = session_file(&dir);
        append(&p, &line("a", false, (2, 10, 100, 5), "claude-opus-5-5"));
        append(&p, &line("a", false, (2, 182, 100, 5), "claude-opus-5-5"));
        append(&p, &line("b", false, (3, 20, 300, 50), "claude-sonnet-5-5"));
        let cache = TranscriptCache::default();
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!(u.messages, 2);
        assert_eq!(
            u.main,
            TokenTotals { input_tokens: 5, output_tokens: 202, cache_read_tokens: 400, cache_creation_tokens: 55 }
        );
        assert_eq!(u.context_tokens, 3 + 300 + 50);
        assert_eq!(u.model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(u.subagents, TokenTotals::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_id_updated_in_a_later_read() {
        let dir = tmp("dup-later");
        let p = session_file(&dir);
        let cache = TranscriptCache::default();
        append(&p, &line("a", false, (1, 5, 0, 0), "m"));
        assert_eq!(read_usage(&cache, &p).unwrap().main.output_tokens, 5);
        append(&p, &line("a", false, (1, 90, 0, 0), "m"));
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.main.output_tokens, u.messages), (90, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_trailing_line_is_reread_after_append() {
        let dir = tmp("partial");
        let p = session_file(&dir);
        let cache = TranscriptCache::default();
        let second = line("b", false, (1, 7, 0, 0), "m");
        let (head, tail) = second.split_at(second.len() / 2);
        append(&p, &line("a", false, (1, 3, 0, 0), "m"));
        append(&p, head);
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.messages, u.main.output_tokens), (1, 3));
        // Ancora incompleta: invariato.
        assert_eq!(read_usage(&cache, &p).unwrap().messages, 1);
        append(&p, tail);
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.messages, u.main.output_tokens), (2, 10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_and_irrelevant_lines_are_skipped() {
        let dir = tmp("bad");
        let p = session_file(&dir);
        append(&p, "non json {\"usage\"\n");
        append(&p, "\n");
        append(&p, "{\"type\":\"user\",\"message\":{\"usage\":{\"input_tokens\":999}}}\n");
        append(&p, "{\"type\":\"assistant\",\"message\":{\"id\":\"x\",\"usage\":5}}\n");
        append(&p, "{\"type\":\"assistant\",\"message\":{\"id\":\"y\",\"usage\":{\"input_tokens\":\"7\"}}}\n");
        append(&p, &line("<synth>", false, (0, 0, 0, 0), "<synthetic>"));
        append(&p, &line("ok", false, (4, 6, 0, 0), "m"));
        let u = read_usage(&TranscriptCache::default(), &p).unwrap();
        // "y" ha un usage oggetto con campo di tipo sbagliato: conta 0 token ma e' un messaggio.
        assert_eq!(u.main.input_tokens, 4);
        assert_eq!(u.main.output_tokens, 6);
        assert_eq!(u.model.as_deref(), Some("m"));
        assert_eq!(u.context_tokens, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sidechain_and_subagent_files_go_to_subagents() {
        let dir = tmp("sub");
        let p = session_file(&dir);
        append(&p, &line("m1", false, (1, 10, 0, 0), "m"));
        append(&p, &line("s1", true, (2, 20, 0, 0), "m"));
        let subs = p.with_extension("").join("subagents");
        std::fs::create_dir_all(&subs).unwrap();
        append(&subs.join("agent-1.jsonl"), &line("a1", true, (3, 30, 0, 0), "m"));
        append(&subs.join("agent-1.jsonl"), &line("a1", true, (3, 33, 0, 0), "m"));
        append(&subs.join("agent-2.jsonl"), &line("a2", true, (4, 40, 0, 0), "m"));
        append(&subs.join("note.txt"), &line("zz", true, (100, 100, 0, 0), "m"));
        let cache = TranscriptCache::default();
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.main.input_tokens, u.main.output_tokens, u.messages), (1, 10, 1));
        assert_eq!((u.subagents.input_tokens, u.subagents.output_tokens), (2 + 3 + 4, 20 + 33 + 40));
        // Un sub-agent cresce: letto in modo incrementale.
        append(&subs.join("agent-2.jsonl"), &line("a3", true, (1, 1, 0, 0), "m"));
        assert_eq!(read_usage(&cache, &p).unwrap().subagents.output_tokens, 20 + 33 + 40 + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shrunk_or_replaced_file_resets_state() {
        let dir = tmp("shrink");
        let p = session_file(&dir);
        let cache = TranscriptCache::default();
        append(&p, &line("a", false, (1, 100, 0, 0), "m"));
        append(&p, &line("b", false, (1, 100, 0, 0), "m"));
        assert_eq!(read_usage(&cache, &p).unwrap().messages, 2);
        std::fs::write(&p, line("c", false, (9, 1, 0, 0), "m2")).unwrap();
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.messages, u.main.input_tokens, u.main.output_tokens), (1, 9, 1));
        assert_eq!(u.model.as_deref(), Some("m2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_transcript_is_an_error() {
        let dir = tmp("missing");
        let p = session_file(&dir);
        assert!(read_usage(&TranscriptCache::default(), &p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- path ---

    #[test]
    fn transcript_path_validation() {
        for ok in [
            r"C:\Users\me\.claude\projects\C--x\abc.jsonl",
            "C:/Users/me/.claude/projects/slug/abc.jsonl",
            "/home/u/.claude/projects/-home-u-p/abc-123.jsonl",
        ] {
            assert!(validate_transcript_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "abc.jsonl",
            "/home/u/.claude/projects/abc.jsonl",
            "/home/u/.claude/other/slug/abc.jsonl",
            "/home/u/.claude/projects/slug/abc.json",
            "/home/u/.claude/projects/slug/abc.jsonl.txt",
            "/home/u/.claude/projects/slug/.jsonl",
            "/home/u/.claude/projects/../../etc/x/abc.jsonl",
            r"C:\Users\me\.claude\projects\..\slug\abc.jsonl",
            "/home/u/.claude/projects/slug/\0abc.jsonl",
        ] {
            assert!(validate_transcript_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn transcript_path_resolution_per_env() {
        let wsl = Env::Wsl("Ubuntu".into());
        assert_eq!(
            resolve_transcript_path(&wsl, "/home/u/.claude/projects/s/a.jsonl").unwrap(),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\u\.claude\projects\s\a.jsonl")
        );
        assert_eq!(
            resolve_transcript_path(&wsl, "/mnt/c/Users/me/.claude/projects/s/a.jsonl").unwrap(),
            PathBuf::from(r"C:\Users\me\.claude\projects\s\a.jsonl")
        );
        assert!(resolve_transcript_path(&wsl, r"C:\x\projects\s\a.jsonl").is_err());
        assert_eq!(
            resolve_transcript_path(&Env::Windows, r"C:\u\.claude\projects\s\a.jsonl").unwrap(),
            PathBuf::from(r"C:\u\.claude\projects\s\a.jsonl")
        );
        assert!(resolve_transcript_path(&Env::Windows, "/home/u/.claude/projects/s/a.jsonl").is_err());
        assert!(resolve_transcript_path(&wsl, "/home/u/../x/projects/s/a.jsonl").is_err());
    }
}
