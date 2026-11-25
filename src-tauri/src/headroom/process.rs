//! Il proxy Headroom avviato dall'app: un solo figlio, avviato in modo
//! serializzato, fermato (con tutto l'albero) all'uscita o alla disattivazione.
//! Un proxy avviato da altri non viene mai toccato.

use super::health;
use super::settings::HeadroomSettings;
use crate::platform::hide_window;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Eseguibile cercato nel PATH quando l'utente non ne indica uno.
const DEFAULT_PROGRAM: &str = "headroom";
const LOG_FILE: &str = "headroom-proxy.log";
const POLL_INTERVAL: Duration = Duration::from_millis(300);
/// Il proxy carica un modello ONNX: all'avvio puo' servire oltre un minuto.
const START_TIMEOUT: Duration = Duration::from_secs(120);

struct Owned {
    child: Child,
    port: u16,
    openai_upstream: String,
    started_at: Instant,
}

type State = Mutex<Option<Owned>>;

fn state() -> &'static State {
    static STATE: OnceLock<State> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn lock() -> MutexGuard<'static, Option<Owned>> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Memorizza la cartella dati (per il log del proxy): `launch_env` non ha un `AppHandle`.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// Argomenti del proxy: `--no-rate-limit` perche' il limite predefinito
/// (60 richieste al minuto) rompe i worker in parallelo. Con un upstream OpenAI
/// configurato, `--openai-api-url`: Headroom compone l'URL inoltrato come
/// `<upstream><percorso ricevuto>` (`/v1/chat/completions` diventa
/// `<upstream>/v1/chat/completions`), quindi l'upstream non deve avere `/v1`.
///
/// `--protect-tool-results`: i risultati dei tool MCP di cli-companion (il testo
/// del task di `get_task` prima di tutto) non vengono mai compressi, per
/// qualunque agente. Compressi, il modello li recupera, il proxy li ricomprime
/// alla richiesta dopo e il modello crede di non aver mai letto il task. Il
/// glob copre i nomi con il prefisso del server (`mcp__plugin_cli-companion-notify_cli-companion__get_task`,
/// `cli-companion_get_task`), i nomi nudi gli agenti che li espongono senza
/// prefisso. Effetto collaterale voluto di Headroom: con questa opzione anche i
/// risultati vecchi di Read/Grep/Edit restano protetti.
fn proxy_args(port: u16, openai_upstream: &str) -> Vec<String> {
    let mut args: Vec<String> = ["proxy", "--port", &port.to_string(), "--no-rate-limit"].map(String::from).to_vec();
    args.extend(["--protect-tool-results".to_string(), protected_tools()]);
    if !openai_upstream.is_empty() {
        args.extend(["--openai-api-url".to_string(), openai_upstream.to_string()]);
    }
    args
}

/// Pattern dei tool da non comprimere: il glob del server e i nomi nudi dei suoi tool.
fn protected_tools() -> String {
    let mut patterns = vec![PROTECTED_SERVER_GLOB.to_string()];
    patterns.extend(crate::mcp::all_tool_names());
    patterns.join(",")
}

/// Qualunque tool il cui nome contiene il nome del server MCP (Headroom confronta
/// con `fnmatch`, senza distinguere maiuscole).
const PROTECTED_SERVER_GLOB: &str = "*cli-companion*";

/// Programmi da provare in ordine. Con un eseguibile indicato dall'utente solo quello;
/// altrimenti `headroom` nel PATH e poi la cartella degli strumenti di uv.
fn program_candidates(executable: &str, user_profile: Option<&str>) -> Vec<String> {
    if !executable.is_empty() {
        return vec![executable.to_string()];
    }
    let mut out = vec![DEFAULT_PROGRAM.to_string()];
    if let Some(profile) = user_profile {
        let fallback = Path::new(profile).join(".local").join("bin").join("headroom.exe");
        out.push(fallback.to_string_lossy().into_owned());
    }
    out
}

/// Apre il log in aggiunta; senza cartella dati o in caso di errore, niente log.
fn open_log() -> (Stdio, Stdio) {
    let file = DATA_DIR.get().and_then(|dir| {
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).ok()?;
        OpenOptions::new().create(true).append(true).open(logs.join(LOG_FILE)).ok()
    });
    match file.and_then(|f| f.try_clone().ok().map(|c| (f, c))) {
        Some((out, err)) => (Stdio::from(out), Stdio::from(err)),
        None => (Stdio::null(), Stdio::null()),
    }
}

fn spawn_program(program: &str, settings: &HeadroomSettings) -> std::io::Result<Child> {
    let (out, err) = open_log();
    let mut cmd = Command::new(program);
    cmd.args(proxy_args(settings.port, &settings.openai_upstream)).stdin(Stdio::null()).stdout(out).stderr(err);
    hide_window(&mut cmd);
    cmd.spawn()
}

fn spawn(settings: &HeadroomSettings) -> Result<Child, String> {
    let profile = std::env::var("USERPROFILE").ok();
    for program in program_candidates(&settings.executable, profile.as_deref()) {
        match spawn_program(&program, settings) {
            Ok(child) => return Ok(child),
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("could not start the proxy: {e}")),
        }
    }
    Err("headroom executable not found".to_string())
}

/// Toglie dallo stato un figlio gia' terminato.
fn reap_dead(slot: &mut Option<Owned>) {
    if slot.as_mut().is_some_and(|o| !matches!(o.child.try_wait(), Ok(None))) {
        *slot = None;
    }
}

/// Sotto il lock: riusa il figlio vivo sulla stessa porta o ne avvia uno nuovo
/// (fermando prima quello su un'altra porta). Restituisce pid e istante di avvio.
fn claim_child(settings: &HeadroomSettings) -> Result<(u32, Instant), String> {
    let mut slot = lock();
    reap_dead(&mut slot);
    match slot.take() {
        Some(owned) if owned.port == settings.port && owned.openai_upstream == settings.openai_upstream => {
            *slot = Some(owned)
        }
        Some(other) => kill_tree(other.child),
        None => {}
    }
    if slot.is_none() {
        *slot = Some(Owned {
            child: spawn(settings)?,
            port: settings.port,
            openai_upstream: settings.openai_upstream.clone(),
            started_at: Instant::now(),
        });
    }
    let owned = slot.as_ref().ok_or("the proxy is not running")?;
    Ok((owned.child.id(), owned.started_at))
}

/// Stato del figlio `pid` osservato con un lock breve.
enum Liveness {
    Alive,
    Stopped,
    Exited,
}

fn liveness(pid: u32) -> Liveness {
    let mut slot = lock();
    match slot.as_mut() {
        Some(o) if o.child.id() == pid => {
            if matches!(o.child.try_wait(), Ok(None)) {
                Liveness::Alive
            } else {
                *slot = None;
                Liveness::Exited
            }
        }
        _ => Liveness::Stopped,
    }
}

/// Attende `/health` senza tenere il lock: a ogni giro controlla che il figlio
/// `pid` sia ancora quello dell'app e vivo.
fn wait_healthy(pid: u32, port: u16, started_at: Instant) -> Result<(), String> {
    loop {
        match liveness(pid) {
            Liveness::Stopped => return Err("the proxy was stopped".to_string()),
            Liveness::Exited => return Err(format!("the proxy exited early (see {LOG_FILE})")),
            Liveness::Alive => {}
        }
        if health::probe_fresh(port) {
            return Ok(());
        }
        if started_at.elapsed() >= START_TIMEOUT {
            return Err(format!("the proxy did not answer within {} s", START_TIMEOUT.as_secs()));
        }
        sleep(POLL_INTERVAL);
    }
}

/// Il proxy risponde sulla porta, avviandolo se serve. Bloccante (fino a 120 s);
/// chiamate concorrenti condividono lo stesso figlio (uno solo spawn).
pub fn ensure_running(settings: &HeadroomSettings) -> Result<(), String> {
    if health::is_reachable(settings.port) {
        return Ok(());
    }
    let (pid, started_at) = claim_child(settings)?;
    wait_healthy(pid, settings.port, started_at)
}

/// Se il proxy non risponde e l'app non ne ha uno in avvio, lo avvia in
/// background e torna subito; l'esito finisce nel log.
pub fn start_nonblocking(settings: &HeadroomSettings) {
    if health::is_reachable(settings.port) || owns_running(settings.port) {
        return;
    }
    let settings = settings.clone();
    std::thread::spawn(move || {
        if let Err(e) = ensure_running(&settings) {
            log::warn!("avvio del proxy di Headroom non riuscito: {e}");
        }
    });
}

fn kill_tree(mut child: Child) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("taskkill");
        cmd.args(["/T", "/F", "/PID", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
        hide_window(&mut cmd);
        let _ = cmd.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Ferma subito il proxy avviato dall'app, se c'e'; non attende un avvio in
/// corso (chi lo attende vede il figlio sparito).
pub fn stop_owned() {
    let owned = lock().take();
    if let Some(owned) = owned {
        kill_tree(owned.child);
        health::invalidate();
    }
}

/// L'app ha un proxy vivo (anche se non risponde ancora) su `port`.
pub fn owns_running(port: u16) -> bool {
    let mut slot = lock();
    reap_dead(&mut slot);
    slot.as_ref().is_some_and(|o| o.port == port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_disable_the_rate_limit_and_protect_the_cli_companion_tools() {
        let args = proxy_args(9100, "");
        assert_eq!(args[..4], ["proxy", "--port", "9100", "--no-rate-limit"]);
        assert_eq!(args[4], "--protect-tool-results");
        assert_eq!(args.len(), 6);
    }

    #[test]
    fn protected_tools_cover_the_server_glob_and_every_bare_tool_name() {
        let list = protected_tools();
        let entries: Vec<&str> = list.split(',').collect();
        assert_eq!(entries[0], "*cli-companion*");
        for name in ["get_task", "complete_task", "propose_tasks", "list_tasks", "update_task"] {
            assert!(entries.contains(&name), "{name} missing from {list}");
        }
        assert!(entries.iter().all(|e| !e.is_empty() && !e.contains(' ')));
    }

    #[test]
    fn args_carry_the_openai_upstream_only_when_configured() {
        let args = proxy_args(9100, "https://api.deepseek.com");
        assert_eq!(args[args.len() - 2..], ["--openai-api-url", "https://api.deepseek.com"]);
        assert!(!proxy_args(9100, "").contains(&"--openai-api-url".to_string()));
    }

    #[test]
    fn explicit_executable_is_the_only_candidate() {
        assert_eq!(program_candidates("C:\\x\\h.exe", Some("C:\\u")), ["C:\\x\\h.exe"]);
    }

    #[test]
    fn default_candidates_fall_back_to_the_uv_tool_dir() {
        let list = program_candidates("", Some("C:\\Users\\a"));
        assert_eq!(list[0], "headroom");
        assert!(list[1].ends_with("headroom.exe") && list[1].contains(".local"));
        assert_eq!(program_candidates("", None), ["headroom"]);
    }
}
