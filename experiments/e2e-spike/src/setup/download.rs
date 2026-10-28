//! Download con verifica SHA-256 in streaming e file temporaneo `.part`.

use crate::hash::hex;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const API_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// ureq non espone un timeout per singola lettura: `timeout_recv_body` e' un
/// budget totale, quindi si usa un tetto ampio che intercetta comunque i blocchi.
const DOWNLOAD_BODY_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const USER_AGENT: &str = "e2e-spike";
const BUFFER_BYTES: usize = 1024 * 1024;
const PROGRESS_STEP_PERCENT: u64 = 10;
const PART_SUFFIX: &str = "part";

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Il file era gia' presente e valido.
    Present,
    Downloaded,
}

/// Agent per le chiamate API: timeout globale di 60 s.
pub fn api_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(API_TIMEOUT))
        .build()
        .into()
}

/// Agent per i download grandi: nessun timeout globale.
pub fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(API_TIMEOUT))
        .timeout_recv_body(Some(DOWNLOAD_BODY_TIMEOUT))
        .build()
        .into()
}

/// Garantisce che `dest` esista con l'hash atteso, scaricandolo se serve.
/// Senza hash atteso un file esistente viene accettato (con avviso).
pub fn ensure_file(
    agent: &ureq::Agent,
    url: &str,
    dest: &Path,
    expected: Option<&str>,
) -> Result<Outcome, String> {
    if dest.exists() && existing_is_valid(dest, expected)? {
        return Ok(Outcome::Present);
    }
    let part = part_path(dest);
    let actual = download_to(agent, url, &part)?;
    if let Some(expected) = expected {
        if actual != expected {
            // Il parziale corrotto non va riusato; se la rimozione fallisce l'errore sotto resta.
            let _ = fs::remove_file(&part);
            return Err(format!(
                "{}: SHA-256 diverso (atteso {expected}, ottenuto {actual})",
                dest.display()
            ));
        }
    } else {
        println!("  avviso: nessun hash pubblicato per {url}, verifica saltata");
    }
    fs::rename(&part, dest).map_err(|e| format!("rinomina di {} fallita: {e}", part.display()))?;
    Ok(Outcome::Downloaded)
}

fn existing_is_valid(dest: &Path, expected: Option<&str>) -> Result<bool, String> {
    let Some(expected) = expected else {
        println!(
            "  avviso: {} presente, hash atteso sconosciuto",
            dest.display()
        );
        return Ok(true);
    };
    let actual = hash_file(dest)?;
    if actual == expected {
        return Ok(true);
    }
    println!("  {}: hash diverso, lo riscarico", dest.display());
    Ok(false)
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".");
    name.push(PART_SUFFIX);
    PathBuf::from(name)
}

pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|e| format!("apertura di {} fallita: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUFFER_BYTES];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("lettura di {} fallita: {e}", path.display()))?;
        if n == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

/// Scarica in `part` calcolando l'hash mentre scrive; ritorna lo SHA-256.
fn download_to(agent: &ureq::Agent, url: &str, part: &Path) -> Result<String, String> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|e| format!("download di {url}: {e}"))?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();
    let file =
        File::create(part).map_err(|e| format!("creazione di {} fallita: {e}", part.display()))?;
    let mut writer = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut progress = Progress::new(total);
    let mut buf = vec![0u8; BUFFER_BYTES];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download di {url} interrotto: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer
            .write_all(&buf[..n])
            .map_err(|e| format!("scrittura di {} fallita: {e}", part.display()))?;
        progress.advance(n as u64);
    }
    writer
        .flush()
        .map_err(|e| format!("scrittura di {} fallita: {e}", part.display()))?;
    Ok(hex(&hasher.finalize()))
}

/// Stampa l'avanzamento a scatti del 10%.
struct Progress {
    total: Option<u64>,
    done: u64,
    last_step: u64,
}

impl Progress {
    fn new(total: Option<u64>) -> Self {
        Progress {
            total,
            done: 0,
            last_step: 0,
        }
    }

    fn advance(&mut self, bytes: u64) {
        self.done += bytes;
        let Some(total) = self.total.filter(|t| *t > 0) else {
            return;
        };
        let step = self.done * 100 / total / PROGRESS_STEP_PERCENT;
        if step > self.last_step {
            self.last_step = step;
            println!("  {}%", step * PROGRESS_STEP_PERCENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_hex;

    fn temp_file(name: &str, content: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("e2e-spike-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(
            part_path(Path::new("a/b.zip")),
            PathBuf::from("a/b.zip.part")
        );
    }

    #[test]
    fn hash_file_matches_in_memory_hash() {
        let path = temp_file("hash.bin", b"contenuto");
        assert_eq!(hash_file(&path).unwrap(), sha256_hex(b"contenuto"));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn present_file_with_matching_hash_is_not_downloaded() {
        let path = temp_file("present.bin", b"ok");
        let expected = sha256_hex(b"ok");
        // L'URL non e' raggiungibile: se tentasse il download fallirebbe.
        let outcome = ensure_file(&api_agent(), "http://127.0.0.1:1/x", &path, Some(&expected));
        assert_eq!(outcome, Ok(Outcome::Present));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn progress_prints_only_on_new_steps() {
        let mut progress = Progress::new(Some(100));
        progress.advance(5);
        assert_eq!(progress.last_step, 0);
        progress.advance(10);
        assert_eq!(progress.last_step, 1);
        let mut unknown = Progress::new(None);
        unknown.advance(1000);
        assert_eq!(unknown.last_step, 0);
    }
}
