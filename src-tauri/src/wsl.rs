use std::collections::HashMap;
use std::hash::Hash;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

mod home;

pub use home::linux_home;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) fn wsl_command() -> Command {
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// wsl.exe scrive in UTF-16LE con BOM: decodifica tollerante (UTF-16LE/BE o UTF-8).
fn decode_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Come `Command::output` ma con timeout: allo scadere il figlio viene
/// ucciso e si ritorna `TimedOut`. stdout/stderr sono letti in thread
/// dedicati (un figlio loquace non si blocca su una pipe piena); dopo il
/// kill un nipote che tiene aperte le pipe non blocca: si usa quanto letto.
pub(crate) fn output_with_timeout(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        if let Some(mut pipe) = pipe {
            let buf = buf.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = pipe.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    buf.lock().unwrap().extend_from_slice(&chunk[..n]);
                }
                let _ = tx.send(());
            });
        } else {
            let _ = tx.send(());
        }
        (buf, rx)
    };
    let (out_buf, out_done) = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let (err_buf, err_done) = drain(child.stderr.take().map(|p| Box::new(p) as _));

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let grace = Duration::from_secs(1);
    let _ = out_done.recv_timeout(grace);
    let _ = err_done.recv_timeout(grace);
    let take = |b: &Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *b.lock().unwrap());
    match status {
        Some(status) => Ok(Output { status, stdout: take(&out_buf), stderr: take(&err_buf) }),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("nessuna risposta entro {}s", timeout.as_secs()),
        )),
    }
}

/// Cache in-process con scadenza: i risultati del rilevamento WSL (processi
/// lenti) non vanno rifatti a ogni apertura delle impostazioni.
pub(crate) struct TtlCache<K, V> {
    ttl: Duration,
    map: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Eq + Hash, V: Clone> TtlCache<K, V> {
    pub(crate) fn new(ttl: Duration) -> Self {
        Self { ttl, map: Mutex::new(HashMap::new()) }
    }

    pub(crate) fn get(&self, key: &K) -> Option<V> {
        let map = self.map.lock().unwrap();
        map.get(key).filter(|(at, _)| at.elapsed() < self.ttl).map(|(_, v)| v.clone())
    }

    pub(crate) fn put(&self, key: K, value: V) {
        self.map.lock().unwrap().insert(key, (Instant::now(), value));
    }

    pub(crate) fn clear(&self) {
        self.map.lock().unwrap().clear();
    }
}

/// Validita' della cache delle distro installate.
const DISTROS_CACHE_TTL: Duration = Duration::from_secs(60);
/// `wsl -l -q` non deve poter bloccare l'app se WSL e' appeso.
const LIST_DISTROS_TIMEOUT: Duration = Duration::from_secs(10);

fn distros_cache() -> &'static TtlCache<(), Vec<String>> {
    static C: OnceLock<TtlCache<(), Vec<String>>> = OnceLock::new();
    C.get_or_init(|| TtlCache::new(DISTROS_CACHE_TTL))
}

/// Svuota la cache delle distro (dopo install/remove).
pub(crate) fn invalidate_distros_cache() {
    distros_cache().clear();
}

/// Distro installate, con cache di breve durata (solo gli esiti positivi).
pub fn installed_distros() -> Result<Vec<String>, String> {
    if let Some(v) = distros_cache().get(&()) {
        return Ok(v);
    }
    let v = installed_distros_uncached()?;
    distros_cache().put((), v.clone());
    Ok(v)
}

fn installed_distros_uncached() -> Result<Vec<String>, String> {
    let output = output_with_timeout(wsl_command().args(["-l", "-q"]), LIST_DISTROS_TIMEOUT)
        .map_err(|e| format!("wsl.exe non disponibile: {e}"))?;
    let text = decode_output(&output.stdout);
    let mut names: Vec<String> = text
        .lines()
        .map(|l| l.replace('\0', "").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // Difesa contro output localizzati o senza flag -q (vecchie build).
    names.retain(|n| {
        let lower = n.to_lowercase();
        !(lower.starts_with("windows subsystem")
            || lower.starts_with("per ")
            || lower.contains("installer"))
    });
    Ok(names)
}

pub fn wsl_dir_exists(distro: &str, path: &str) -> bool {
    wsl_command()
        .args(["-d", distro, "-e", "test", "-d", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Comando `argv` nella distro, con `dir` come cartella corrente. `-e`
/// (exec) e' indispensabile: con `--` wsl.exe passa la riga alla shell di
/// default della distro, che rompe spazi, `;` e apici negli argomenti.
pub fn wsl_run_command(distro: &str, dir: &str, argv: &[&str]) -> Command {
    let mut cmd = wsl_command();
    cmd.args(["-d", distro, "--cd", dir, "-e"]).args(argv).stdin(std::process::Stdio::null());
    cmd
}

/// `git <args>` nella distro, in `dir` (vedi `wsl_run_command`).
pub fn wsl_git_command(distro: &str, dir: &str, args: &[&str]) -> Command {
    let mut argv = vec!["git"];
    argv.extend_from_slice(args);
    wsl_run_command(distro, dir, &argv)
}

/// Converte path Windows in path Linux WSL:
/// - `C:\Users\me\proj` -> `/mnt/c/Users/me/proj`
/// - `\\wsl$\Ubuntu\home\me` -> `/home/me`
pub fn windows_to_wsl_path(win: &str) -> Option<String> {
    let t = win.trim();
    let slashed = t.replace('\\', "/");
    let lower = slashed.to_lowercase();
    if lower.starts_with("//wsl$/") {
        let mut parts = slashed.split('/').filter(|p| !p.is_empty());
        let _ = parts.next()?; // wsl$
        let _ = parts.next()?; // distro
        let rest: Vec<&str> = parts.collect();
        return Some(format!("/{}", rest.join("/")));
    }
    let bytes = t.as_bytes();
    if t.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        let drive = bytes[0] as char;
        if drive.is_ascii_alphabetic() {
            let rest = t[2..].replace('\\', "/");
            return Some(format!(
                "/mnt/{}/{}",
                drive.to_ascii_lowercase(),
                rest.trim_start_matches('/')
            ));
        }
    }
    None
}

/// `true` se il path Linux sta su un drive Windows montato (`/mnt/c/...`,
/// mount root predefinita): li' ci sono i programmi di Windows.
pub fn is_windows_drive_path(linux: &str) -> bool {
    let mut comps = linux.trim().split('/').filter(|c| !c.is_empty());
    linux.trim().starts_with('/')
        && comps.next() == Some("mnt")
        && comps
            .next()
            .is_some_and(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_alphabetic())
}

/// Inverso di `windows_to_wsl_path`: path Linux di `distro` -> path Windows
/// leggibile dall'host.
/// - `/mnt/c/Users/me` -> `C:\Users\me`
/// - `/home/me/x` -> `\\wsl.localhost\Ubuntu\home\me\x`
///
/// `None` se il path non e' assoluto.
pub fn wsl_to_windows_path(distro: &str, linux: &str) -> Option<String> {
    let t = linux.trim();
    if !t.starts_with('/') {
        return None;
    }
    let comps: Vec<&str> = t.split('/').filter(|c| !c.is_empty()).collect();
    if let [mnt, drive, rest @ ..] = comps.as_slice() {
        if *mnt == "mnt" && drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
            let d = drive.to_ascii_uppercase();
            return Some(if rest.is_empty() {
                format!("{d}:\\")
            } else {
                format!("{d}:\\{}", rest.join("\\"))
            });
        }
    }
    Some(format!("\\\\wsl.localhost\\{distro}\\{}", comps.join("\\")))
}

/// Nome di distro sicuro da usare in un path UNC: niente separatori o
/// caratteri insoliti.
pub(crate) fn is_valid_distro_name(name: &str) -> bool {
    name.len() <= 64
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

#[tauri::command]
pub async fn list_wsl_distros() -> Result<Vec<String>, String> {
    crate::blocking(installed_distros).await
}

#[tauri::command]
pub async fn check_wsl_path(distro: String, path: String) -> Result<bool, String> {
    if distro.trim().is_empty() || path.trim().is_empty() {
        return Ok(false);
    }
    let path = path.trim();
    let linux_path = if path.starts_with('/') {
        path.to_string()
    } else if let Some(converted) = windows_to_wsl_path(path) {
        converted
    } else {
        return Ok(false);
    };
    crate::blocking(move || Ok(wsl_dir_exists(distro.trim(), &linux_path))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn output_with_timeout_kills_hung_child_and_captures_output() {
        let mut slow = Command::new("ping");
        slow.args(["-n", "30", "127.0.0.1"]);
        let t = Instant::now();
        let err = output_with_timeout(&mut slow, Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(5));

        let mut ok = Command::new("cmd");
        ok.args(["/c", "echo ciao"]);
        let out = output_with_timeout(&mut ok, Duration::from_secs(10)).unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("ciao"));
    }

    #[test]
    fn ttl_cache_expires_and_clears() {
        let c = TtlCache::new(Duration::from_millis(50));
        c.put(1, Some("a".to_string()));
        assert_eq!(c.get(&1), Some(Some("a".to_string())));
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(c.get(&1), None);
        c.put(2, None);
        c.clear();
        assert_eq!(c.get(&2), None);
    }

    #[test]
    fn windows_drive_paths_are_recognized() {
        for p in ["/mnt/c/Users/me/AppData/Roaming/npm/claude", "/mnt/D/x", "/mnt/c", " /mnt/c/x "] {
            assert!(is_windows_drive_path(p), "{p}");
        }
        for p in ["/home/u/.local/bin/claude", "/mnt/cd/x", "/mnt/wsl/x", "/mnt", "/usr/mnt/c/x", "mnt/c/x", ""] {
            assert!(!is_windows_drive_path(p), "{p}");
        }
    }

    #[test]
    fn wsl_to_windows_converts_mnt_and_distro_paths() {
        let w = |p: &str| wsl_to_windows_path("Ubuntu", p);
        assert_eq!(w("/mnt/c/Users/me/x.jsonl").as_deref(), Some(r"C:\Users\me\x.jsonl"));
        assert_eq!(w("/mnt/d").as_deref(), Some(r"D:\"));
        assert_eq!(
            w("/home/u/.claude/projects/p/s.jsonl").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\u\.claude\projects\p\s.jsonl")
        );
        // `/mnt/cd` non e' un drive.
        assert_eq!(w("/mnt/cd/x").as_deref(), Some(r"\\wsl.localhost\Ubuntu\mnt\cd\x"));
        assert_eq!(w("relativo/x"), None);
        assert_eq!(w(r"C:\x"), None);
        // Inverso di windows_to_wsl_path per i drive.
        let back = windows_to_wsl_path(&w("/mnt/c/a/b").unwrap()).unwrap();
        assert_eq!(back, "/mnt/c/a/b");
    }

    #[test]
    fn run_and_git_use_exec_so_argv_skips_the_distro_shell() {
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let c = wsl_run_command("Ubuntu", "/p q", &["sh", "-c", "echo $1", "sh", "a b;c"]);
        assert_eq!(args(&c), ["-d", "Ubuntu", "--cd", "/p q", "-e", "sh", "-c", "echo $1", "sh", "a b;c"]);
        let g = wsl_git_command("Ubuntu", "/p", &["worktree", "add", "/p/.worktrees/a b"]);
        assert_eq!(args(&g), ["-d", "Ubuntu", "--cd", "/p", "-e", "git", "worktree", "add", "/p/.worktrees/a b"]);
    }
}
