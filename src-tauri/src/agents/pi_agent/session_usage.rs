//! Consumi di sessione di Pi: token letti dal file di sessione.
//!
//! I file `<agent dir>/sessions/--<cwd>--/<timestamp>_<id>.jsonl` (versione 3)
//! sono append-only; la cache tiene per ogni file l'offset letto, cosi' ad ogni
//! chiamata si leggono solo i byte nuovi. Pi non ha sub-agent: `subagents`
//! resta a zero.

use super::PiAgent;
use crate::agents::{SessionUsage, SessionUsageReader, TokenTotals};
use crate::platform::{wsl_to_windows_path, Target};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Oltre questo numero di file in cache la si svuota (sessioni vecchie).
const CACHE_MAX_FILES: usize = 512;

/// Stato di lettura di un file `.jsonl`.
#[derive(Default)]
struct FileState {
    /// Byte gia' elaborati (fine dell'ultima riga completa).
    offset: u64,
    /// Lunghezza vista all'ultima lettura: se cala, il file e' stato sostituito.
    len: u64,
    /// Consumi per id di entry (una riga riscritta con lo stesso id vale una volta).
    msgs: HashMap<String, TokenTotals>,
    /// Id e modello dell'ultimo messaggio dell'assistente.
    last: Option<(String, Option<String>)>,
}

/// Cache dei file letti. Un lock per file: quello globale serve solo a
/// prendere/inserire/sfrattare le voci.
#[derive(Default)]
struct SessionCache(Mutex<HashMap<PathBuf, Arc<Mutex<FileState>>>>);

impl SessionCache {
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
    // Scarto veloce: solo i messaggi con usage interessano.
    if !text.contains("\"usage\"") {
        return;
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if v.get("type").and_then(Value::as_str) != Some("message") {
        return;
    }
    let Some(msg) = v.get("message") else {
        return;
    };
    if msg.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else {
        return;
    };
    let id = v.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("anon:{line_offset}"));
    let totals = TokenTotals {
        input_tokens: token_field(usage, "input"),
        output_tokens: token_field(usage, "output"),
        cache_read_tokens: token_field(usage, "cacheRead"),
        cache_creation_tokens: token_field(usage, "cacheWrite"),
    };
    let model = msg.get("model").and_then(Value::as_str).map(str::to_string);
    state.last = Some((id.clone(), model));
    state.msgs.insert(id, totals);
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

/// Consumi di `session_file`, aggiornando `cache` in modo incrementale.
fn read_usage(cache: &SessionCache, session_file: &Path) -> Result<SessionUsage, String> {
    let state = cache.entry(session_file);
    let mut state = state.lock().unwrap();
    refresh(&mut state, session_file).map_err(|e| format!("File di sessione non leggibile: {e}"))?;

    let mut usage = SessionUsage::default();
    for totals in state.msgs.values() {
        add(&mut usage.main, totals);
    }
    usage.messages = state.msgs.len() as u32;
    if let Some((id, model)) = &state.last {
        if let Some(t) = state.msgs.get(id) {
            usage.context_tokens = t.input_tokens + t.cache_read_tokens + t.cache_creation_tokens;
        }
        usage.model = model.clone();
    }
    Ok(usage)
}

/// Controllo sintattico del path del file di sessione, come lo vede l'agente
/// (Windows o Linux): niente `..`, estensione `.jsonl` e forma
/// `.../sessions/<cartella>/<file>.jsonl`.
fn validate_session_path(p: &str) -> Result<(), String> {
    let comps: Vec<&str> = p.trim().split(['/', '\\']).filter(|c| !c.is_empty()).collect();
    let bad = || Err("Percorso del file di sessione non valido.".to_string());
    if p.contains('\0') || comps.contains(&"..") {
        return bad();
    }
    let n = comps.len();
    let file_ok = comps.last().is_some_and(|f| f.len() > ".jsonl".len() && f.ends_with(".jsonl"));
    if !file_ok || n < 3 || comps[n - 3] != "sessions" {
        return bad();
    }
    Ok(())
}

/// Path del file leggibile dall'host: invariato su Windows, UNC (o `C:\` per
/// `/mnt/c`) per una distro WSL.
fn resolve_session_path(env: &Target, p: &str) -> Result<PathBuf, String> {
    validate_session_path(p)?;
    let p = p.trim();
    let bad = || "Percorso del file di sessione non valido.".to_string();
    match env {
        Target::Windows if p.starts_with('/') => Err(bad()),
        Target::Windows => Ok(PathBuf::from(p)),
        Target::Wsl { distro: d } => wsl_to_windows_path(d, p).map(PathBuf::from).ok_or_else(bad),
    }
}

/// Consumi di token della sessione con file `transcript_path`.
fn session_usage(env: &Target, transcript_path: &str, cache: &SessionCache) -> Result<SessionUsage, String> {
    let path = resolve_session_path(env, transcript_path)?;
    read_usage(cache, &path)
}

/// Cache condivisa da tutte le chiamate dell'app: dettaglio interno dell'integrazione.
fn shared_cache() -> &'static SessionCache {
    static CACHE: OnceLock<SessionCache> = OnceLock::new();
    CACHE.get_or_init(SessionCache::default)
}

impl SessionUsageReader for PiAgent {
    fn session_usage(&self, target: &Target, transcript_path: &str) -> Result<SessionUsage, String> {
        session_usage(target, transcript_path, shared_cache())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mc-pi-usage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn line(id: &str, role: &str, usage: (u64, u64, u64, u64), model: &str) -> String {
        format!(
            "{}\n",
            serde_json::json!({
                "type": "message",
                "id": id,
                "parentId": null,
                "message": {"role": role, "model": model, "provider": "p", "usage": {
                    "input": usage.0, "output": usage.1, "cacheRead": usage.2, "cacheWrite": usage.3,
                    "reasoning": 7, "totalTokens": 1, "cost": {"total": 0.5}}, "stopReason": "stop"}
            })
        )
    }

    fn assistant(id: &str, usage: (u64, u64, u64, u64), model: &str) -> String {
        line(id, "assistant", usage, model)
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    fn session_file(dir: &Path) -> PathBuf {
        let p = dir.join("sessions").join("--C--x--").join("2026_abc.jsonl");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        p
    }

    #[test]
    fn sums_assistant_messages_and_takes_context_from_the_last() {
        let dir = tmp("sum");
        let p = session_file(&dir);
        append(&p, "{\"type\":\"session\",\"version\":3,\"id\":\"s\",\"cwd\":\"C:\\\\x\"}\n");
        append(&p, "{\"type\":\"model_change\",\"provider\":\"d\",\"modelId\":\"m0\"}\n");
        append(&p, &line("u1", "user", (999, 999, 999, 999), "ignored"));
        append(&p, &assistant("a", (10, 5, 100, 20), "m1"));
        append(&p, &line("t1", "toolResult", (999, 999, 0, 0), "ignored"));
        append(&p, &assistant("b", (30, 8, 300, 50), "m2"));
        let u = read_usage(&SessionCache::default(), &p).unwrap();
        assert_eq!(u.messages, 2);
        assert_eq!(
            u.main,
            TokenTotals { input_tokens: 40, output_tokens: 13, cache_read_tokens: 400, cache_creation_tokens: 70 }
        );
        assert_eq!(u.context_tokens, 30 + 300 + 50);
        assert_eq!(u.model.as_deref(), Some("m2"));
        assert_eq!(u.subagents, TokenTotals::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_without_assistant_messages_is_empty() {
        let dir = tmp("empty");
        let p = session_file(&dir);
        append(&p, "{\"type\":\"session\",\"version\":3,\"id\":\"s\"}\n");
        let u = read_usage(&SessionCache::default(), &p).unwrap();
        assert_eq!(u, SessionUsage::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incremental_reads_only_add_new_lines() {
        let dir = tmp("incr");
        let p = session_file(&dir);
        let cache = SessionCache::default();
        append(&p, &assistant("a", (1, 5, 0, 0), "m"));
        assert_eq!(read_usage(&cache, &p).unwrap().main.output_tokens, 5);
        append(&p, &assistant("b", (2, 6, 0, 0), "m"));
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.main.output_tokens, u.messages, u.context_tokens), (11, 2, 2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeated_entry_id_counts_once_with_the_last_usage() {
        let dir = tmp("dup");
        let p = session_file(&dir);
        append(&p, &assistant("a", (1, 5, 0, 0), "m"));
        append(&p, &assistant("a", (1, 90, 0, 0), "m"));
        let u = read_usage(&SessionCache::default(), &p).unwrap();
        assert_eq!((u.main.output_tokens, u.messages), (90, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_trailing_line_is_reread_after_append() {
        let dir = tmp("partial");
        let p = session_file(&dir);
        let cache = SessionCache::default();
        let second = assistant("b", (1, 7, 0, 0), "m");
        let (head, tail) = second.split_at(second.len() / 2);
        append(&p, &assistant("a", (1, 3, 0, 0), "m"));
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
        append(&p, "{\"type\":\"custom\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input\":999}}}\n");
        append(&p, "{\"type\":\"message\",\"id\":\"x\",\"message\":{\"role\":\"assistant\",\"usage\":5}}\n");
        append(
            &p,
            "{\"type\":\"message\",\"id\":\"y\",\"message\":{\"role\":\"assistant\",\"usage\":{\"input\":\"7\"}}}\n",
        );
        append(&p, &assistant("ok", (4, 6, 0, 0), "m"));
        let u = read_usage(&SessionCache::default(), &p).unwrap();
        // "y" ha un usage oggetto con campo di tipo sbagliato: conta 0 token ma e' un messaggio.
        assert_eq!((u.main.input_tokens, u.main.output_tokens, u.messages), (4, 6, 2));
        assert_eq!(u.model.as_deref(), Some("m"));
        assert_eq!(u.context_tokens, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shrunk_or_replaced_file_resets_state() {
        let dir = tmp("shrink");
        let p = session_file(&dir);
        let cache = SessionCache::default();
        append(&p, &assistant("a", (1, 100, 0, 0), "m"));
        append(&p, &assistant("b", (1, 100, 0, 0), "m"));
        assert_eq!(read_usage(&cache, &p).unwrap().messages, 2);
        std::fs::write(&p, assistant("c", (9, 1, 0, 0), "m2")).unwrap();
        let u = read_usage(&cache, &p).unwrap();
        assert_eq!((u.messages, u.main.input_tokens, u.main.output_tokens), (1, 9, 1));
        assert_eq!(u.model.as_deref(), Some("m2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tmp("missing");
        let p = session_file(&dir);
        assert!(read_usage(&SessionCache::default(), &p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_trait_reads_through_the_shared_cache() {
        let dir = tmp("trait");
        let p = session_file(&dir);
        append(&p, &assistant("a", (3, 4, 0, 0), "m"));
        let u = PiAgent.session_usage(&Target::Windows, &p.to_string_lossy()).unwrap();
        assert_eq!((u.main.input_tokens, u.messages), (3, 1));
        assert!(PiAgent.session_usage(&Target::Windows, "").is_err());
        assert!(PiAgent.session_usage(&Target::Windows, "../x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- path ---

    #[test]
    fn session_path_validation() {
        for ok in [
            r"C:\Users\me\.pi\agent\sessions\--C--x--\2026-11-24T19-13-56-094Z_abc.jsonl",
            "C:/Users/me/.pi/agent/sessions/--C--x--/abc.jsonl",
            "/home/u/.pi/agent/sessions/--home-u-p--/abc-123.jsonl",
        ] {
            assert!(validate_session_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "../x",
            "abc.jsonl",
            "/home/u/.pi/agent/sessions/abc.jsonl",
            "/home/u/.pi/agent/other/dir/abc.jsonl",
            "/home/u/.pi/agent/sessions/dir/abc.json",
            "/home/u/.pi/agent/sessions/dir/abc.jsonl.txt",
            "/home/u/.pi/agent/sessions/dir/.jsonl",
            "/home/u/.pi/agent/sessions/../../etc/x/abc.jsonl",
            r"C:\Users\me\.pi\agent\sessions\..\dir\abc.jsonl",
            "/home/u/.pi/agent/sessions/dir/\0abc.jsonl",
        ] {
            assert!(validate_session_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn session_path_resolution_per_env() {
        let wsl = Target::Wsl { distro: "Ubuntu".into() };
        assert_eq!(
            resolve_session_path(&wsl, "/home/u/.pi/agent/sessions/d/a.jsonl").unwrap(),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\u\.pi\agent\sessions\d\a.jsonl")
        );
        assert_eq!(
            resolve_session_path(&wsl, "/mnt/c/Users/me/.pi/agent/sessions/d/a.jsonl").unwrap(),
            PathBuf::from(r"C:\Users\me\.pi\agent\sessions\d\a.jsonl")
        );
        assert!(resolve_session_path(&wsl, r"C:\x\sessions\d\a.jsonl").is_err());
        assert_eq!(
            resolve_session_path(&Target::Windows, r"C:\u\.pi\agent\sessions\d\a.jsonl").unwrap(),
            PathBuf::from(r"C:\u\.pi\agent\sessions\d\a.jsonl")
        );
        assert!(resolve_session_path(&Target::Windows, "/home/u/.pi/agent/sessions/d/a.jsonl").is_err());
        assert!(resolve_session_path(&wsl, "/home/u/../x/sessions/d/a.jsonl").is_err());
        assert!(resolve_session_path(&wsl, "relative/sessions/d/a.jsonl").is_err());
    }
}
