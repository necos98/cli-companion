//! Processo `llama-server`: avvio, attesa della prontezza, arresto.
//! Nessuna logica dell'esperimento: serve anche a un futuro pulsante "Avvia".

use crate::metrics::process_mem;
use crate::proc::output_with_timeout;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const HOST: &str = "127.0.0.1";
const DEFAULT_CTX_SIZE: u32 = 32768;
const DEFAULT_GPU_LAYERS: u32 = 99;
const DEFAULT_PARALLEL: u32 = 1;
const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_POLL: Duration = Duration::from_millis(250);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION_MARKER: &str = "version:";
/// Quanti byte finali del log riportare nell'errore di avvio.
const LOG_TAIL_BYTES: u64 = 1500;

pub struct LlamaServerConfig {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub port: u16,
    pub ctx_size: u32,
    pub gpu_layers: u32,
    pub flash_attn: bool,
    pub parallel: u32,
    pub extra_args: Vec<String>,
    pub log_file: PathBuf,
    pub ready_timeout: Duration,
}

impl LlamaServerConfig {
    pub fn new(exe: PathBuf, model: PathBuf, port: u16, log_file: PathBuf) -> Self {
        LlamaServerConfig {
            exe,
            model,
            port,
            ctx_size: DEFAULT_CTX_SIZE,
            gpu_layers: DEFAULT_GPU_LAYERS,
            flash_attn: true,
            parallel: DEFAULT_PARALLEL,
            extra_args: Vec::new(),
            log_file,
            ready_timeout: DEFAULT_READY_TIMEOUT,
        }
    }

    fn args(&self) -> Vec<String> {
        let mut args = vec![
            "--host".to_string(),
            HOST.to_string(),
            "--port".to_string(),
            self.port.to_string(),
            "--jinja".to_string(),
            "-c".to_string(),
            self.ctx_size.to_string(),
            "-fa".to_string(),
            if self.flash_attn { "on" } else { "off" }.to_string(),
            "-ngl".to_string(),
            self.gpu_layers.to_string(),
            "-np".to_string(),
            self.parallel.to_string(),
            "-m".to_string(),
            self.model.to_string_lossy().into_owned(),
        ];
        args.extend(self.extra_args.iter().cloned());
        args
    }
}

pub struct LlamaServer {
    child: Option<Child>,
    base_url: String,
}

impl LlamaServer {
    /// Avvia il server e attende `/health` ok; fallisce subito se il processo termina.
    pub fn start(cfg: &LlamaServerConfig) -> Result<Self, String> {
        let child = spawn(cfg)?;
        let mut server = LlamaServer {
            child: Some(child),
            base_url: format!("http://{HOST}:{}", cfg.port),
        };
        // In caso di errore `server` viene rilasciato e il Drop termina il processo.
        server.wait_ready(cfg)?;
        Ok(server)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn peak_working_set_bytes(&self) -> Option<u64> {
        self.child
            .as_ref()
            .and_then(process_mem::peak_working_set_bytes)
    }

    /// Arresto: su Windows `llama-server` non ha un canale di uscita pulita
    /// utilizzabile (vedi `spawn`), quindi si termina il processo e si attende.
    pub fn stop(mut self) -> Result<(), String> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        child
            .kill()
            .map_err(|e| format!("llama-server: terminazione fallita: {e}"))?;
        child
            .wait()
            .map_err(|e| format!("llama-server: attesa dopo la terminazione fallita: {e}"))?;
        Ok(())
    }

    fn wait_ready(&mut self, cfg: &LlamaServerConfig) -> Result<(), String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(HEALTH_TIMEOUT))
            .build()
            .into();
        let health_url = format!("{}/health", self.base_url);
        let deadline = Instant::now() + cfg.ready_timeout;
        loop {
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|e| format!("llama-server: controllo del processo fallito: {e}"))?
                {
                    return Err(format!(
                        "llama-server terminato durante l'avvio ({status}). Log {}: {}",
                        cfg.log_file.display(),
                        log_tail(&cfg.log_file)
                    ));
                }
            }
            // 503 (modello in caricamento) e' un errore per ureq: si riprova.
            if agent.get(&health_url).call().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "llama-server non pronto dopo {} s. Log {}: {}",
                    cfg.ready_timeout.as_secs(),
                    cfg.log_file.display(),
                    log_tail(&cfg.log_file)
                ));
            }
            thread::sleep(HEALTH_POLL);
        }
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            // Chiusura di emergenza: se e' gia' terminato l'errore e' irrilevante.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Prima riga di `--version` che contiene "version:".
pub fn llama_server_version(exe: &Path) -> Result<String, String> {
    let out = output_with_timeout(Command::new(exe).arg("--version"), VERSION_TIMEOUT)?;
    // llama-server scrive la versione su stderr: si guardano entrambi i flussi.
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    find_version_line(&text)
        .ok_or_else(|| format!("{}: versione non trovata nell'output", exe.display()))
}

fn find_version_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| l.contains(VERSION_MARKER))
        .map(str::to_string)
}

fn spawn(cfg: &LlamaServerConfig) -> Result<Child, String> {
    if let Some(dir) = cfg.log_file.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("creazione di {} fallita: {e}", dir.display()))?;
    }
    let log = File::create(&cfg.log_file)
        .map_err(|e| format!("creazione del log {} fallita: {e}", cfg.log_file.display()))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("duplicazione dell'handle del log fallita: {e}"))?;
    let mut cmd = Command::new(&cfg.exe);
    cmd.args(cfg.args())
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    // Console nascosta e separata (CREATE_NO_WINDOW): non condivisa con il
    // chiamante, quindi CTRL_BREAK via GenerateConsoleCtrlEvent non arriva
    // (provato: nessun effetto, il processo restava vivo fino al kill).
    #[cfg(windows)]
    cmd.creation_flags(crate::proc::CREATE_NO_WINDOW);
    cmd.spawn()
        .map_err(|e| format!("avvio di {} fallito: {e}", cfg.exe.display()))
}

/// Ultimi byte del log, per diagnosticare un avvio fallito.
fn log_tail(path: &Path) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES)))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).trim().to_string())
    };
    read().unwrap_or_else(|e| format!("(log non leggibile: {e})"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::Quant;
    use crate::paths::Paths;

    fn config() -> LlamaServerConfig {
        LlamaServerConfig::new(
            PathBuf::from("llama-server"),
            PathBuf::from("model.gguf"),
            18181,
            PathBuf::from("llama.log"),
        )
    }

    #[test]
    fn default_args_follow_the_contract() {
        let args = config().args();
        assert_eq!(
            args.join(" "),
            "--host 127.0.0.1 --port 18181 --jinja -c 32768 -fa on -ngl 99 -np 1 -m model.gguf"
        );
    }

    #[test]
    fn custom_values_and_extra_args_are_applied() {
        let mut cfg = config();
        cfg.ctx_size = 4096;
        cfg.flash_attn = false;
        cfg.parallel = 2;
        cfg.extra_args = vec!["--no-webui".into()];
        let args = cfg.args().join(" ");
        assert!(args.contains("-c 4096 -fa off -ngl 99 -np 2"), "{args}");
        assert!(args.ends_with("--no-webui"), "{args}");
    }

    #[test]
    fn version_line_is_found_among_noise() {
        let text = "ggml_cuda_init: found 1 CUDA devices\nversion: 0.8.0-dev (build 11960)\nbuilt with MSVC";
        assert_eq!(
            find_version_line(text).as_deref(),
            Some("version: 0.8.0-dev (build 11960)")
        );
        assert_eq!(find_version_line("niente"), None);
    }

    #[test]
    fn log_tail_reports_missing_file() {
        assert!(log_tail(Path::new("non-esiste.log")).starts_with("(log non leggibile"));
    }

    /// Prova manuale con exe e modello reali (dopo `setup`):
    /// `cargo test -- --ignored llama_server_real`.
    #[test]
    #[ignore = "richiede llama-server e il modello Q8_0 scaricati da `setup`"]
    fn llama_server_real_start_and_stop() {
        let paths = Paths::from_manifest();
        let log = paths.results.join("llama-server-test.log");
        let cfg = LlamaServerConfig::new(
            paths.llama_server_exe(),
            paths.model(Quant::Q8_0),
            18181,
            log,
        );
        let server = LlamaServer::start(&cfg).unwrap();
        assert_eq!(server.base_url(), "http://127.0.0.1:18181");
        assert!(server.peak_working_set_bytes().unwrap_or(0) > 0);
        let started = Instant::now();
        server.stop().unwrap();
        println!("stop in {:?}", started.elapsed());
        assert!(ureq::get("http://127.0.0.1:18181/health").call().is_err());
    }
}
