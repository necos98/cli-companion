//! Integrazione Muse: segnale acustico a fine lavoro senza euristica busy/idle.
//!
//! Il plugin Muse installato negli ambienti (Windows + distro WSL) dichiara
//! hook `Stop`/`SessionEnd` il cui comando e' questo stesso eseguibile:
//! `cli-companion --notify <evento> <sorgente>`.
//! La modalita' --notify spedisce una riga JSON su TCP 127.0.0.1:NOTIFY_PORT
//! all'istanza GUI, che emette l'evento Tauri `muse-event` verso il frontend.
//!
//! Nota WSL: l'exe gira sempre sull'host Windows (interop), quindi il TCP
//! resta locale e non serve alcun networking cross-VM. Solo std + serde.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::wsl::{installed_distros, windows_to_wsl_path};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Porta del listener locale. Solo 127.0.0.1: nessun prompt del firewall.
pub const NOTIFY_PORT: u16 = 17421;
/// ID del plugin Muse generato da questa app.
pub const PLUGIN_ID: &str = "cli-companion-notify";
/// ID del plugin generato dalla vecchia app (muse-companion), rimosso a ogni
/// installazione: i suoi hook puntano a un exe che non esiste piu'.
const LEGACY_PLUGIN_ID: &str = "muse-companion-notify";
/// Eventi accettati sul socket e in --notify.
const KNOWN_EVENTS: [&str; 3] = ["stop", "session-end", "test"];
/// Nome evento Tauri verso il frontend.
const FRONTEND_EVENT: &str = "muse-event";

/// Payload sul socket e verso il frontend: `{event, source}` + `\n`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotifyPayload {
    pub event: String,
    pub source: String,
}

pub fn is_known_event(event: &str) -> bool {
    KNOWN_EVENTS.contains(&event)
}

fn build_line(event: &str, source: &str) -> Result<String, String> {
    if !is_known_event(event) {
        return Err(format!("evento sconosciuto: {event}"));
    }
    let payload = NotifyPayload {
        event: event.to_string(),
        source: source.to_string(),
    };
    serde_json::to_string(&payload).map_err(|e| e.to_string())
}

fn parse_line(line: &str) -> Result<NotifyPayload, String> {
    let payload: NotifyPayload =
        serde_json::from_str(line.trim()).map_err(|_| "payload non valido".to_string())?;
    if !is_known_event(&payload.event) {
        return Err(format!("evento sconosciuto: {}", payload.event));
    }
    Ok(payload)
}

/// Spedisce un evento al listener locale (usato dalla modalita' --notify).
pub fn send_to(port: u16, event: &str, source: &str) -> Result<(), String> {
    let line = build_line(event, source)?;
    let mut stream = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(1500),
    )
    .map_err(|_| "app non in ascolto (avvia cli-companion)".to_string())?;
    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("invio fallito: {e}"))?;
    stream.flush().map_err(|e| format!("invio fallito: {e}"))?;
    Ok(())
}

pub fn send_notify(event: &str, source: &str) -> Result<(), String> {
    send_to(NOTIFY_PORT, event, source)
}

fn handle_conn(stream: &mut TcpStream, app: &AppHandle) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    let payload = parse_line(&String::from_utf8_lossy(&buf[..n]))?;
    app.emit(FRONTEND_EVENT, &payload).map_err(|e| e.to_string())?;
    Ok(())
}

/// Avvia il listener in un thread dedicato. Se la porta e' occupata
/// (altra istanza gia' in ascolto) esce in silenzio.
pub fn start_listener(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", NOTIFY_PORT)) {
            Ok(l) => l,
            Err(_) => return,
        };
        for stream in listener.incoming() {
            match stream {
                // Un thread per connessione: un client che non scrive nulla
                // non blocca gli altri per tutto il read timeout.
                Ok(mut s) => {
                    let app = app.clone();
                    std::thread::spawn(move || {
                        let _ = handle_conn(&mut s, &app);
                    });
                }
                Err(_) => continue,
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Generazione del plugin Muse
// ---------------------------------------------------------------------------

/// Chiave env sicura per nomi file: solo ASCII semplici, resto `-`.
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

/// Manifest del plugin con il percorso exe gia' dentro (slash, mai backslash:
/// il validatore `muse plugins validate` rifiuta i backslash nei valori).
pub fn build_manifest_json(exe_path: &str, env_key: &str) -> String {
    let hook = |id: &str, event: &str, notify: &str| {
        serde_json::json!({
            "id": id,
            "event": event,
            "command": [exe_path, "--notify", notify, env_key],
            "timeoutMs": 8000,
        })
    };
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "name": PLUGIN_ID,
        "displayName": "cli-companion: segnale di fine lavoro",
        "version": "0.1.0",
        "description": "Avvisa cli-companion quando una sessione Muse finisce (ding).",
        "compat": { "source": "native", "manifestDir": ".muse-plugin" },
        "capabilities": {
            "skills": [],
            "commands": [],
            "hooks": [
                hook("on-stop", "Stop", "stop"),
                hook("on-session-end", "SessionEnd", "session-end"),
            ],
            "mcpServers": [],
            "reminders": [],
        },
    });
    serde_json::to_string_pretty(&manifest).unwrap_or_default()
}

/// `true` se l'output di `muse plugins list --json` contiene il nostro plugin
/// (match esatto su id: niente sottostringhe).
/// Forma reale osservata: ogni voce annida l'id in `record.id` e `plugin.id`;
/// restano i match diretti per tolleranza a forme future.
pub fn plugin_listed(list_json: &str, id: &str) -> bool {
    let value: serde_json::Value = match serde_json::from_str(list_json) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let plugins = match value.get("plugins").and_then(|p| p.as_array()) {
        Some(a) => a,
        None => return false,
    };
    plugins.iter().any(|p| {
        ["record", "plugin"].iter().any(|nest| {
            p.get(nest).and_then(|n| n.get("id")).and_then(|v| v.as_str()) == Some(id)
        }) || ["id", "name", "plugin_id"]
            .iter()
            .any(|k| p.get(k).and_then(|v| v.as_str()) == Some(id))
    })
}

// ---------------------------------------------------------------------------
// Esecuzione di `muse` (Windows diretto o dentro una distro WSL)
// ---------------------------------------------------------------------------

fn spawn_quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Esegue `muse <args>` su Windows (`distro=None`) o in una distro WSL.
/// Ritorna lo stdout; in caso di errore, lo stderr (o l'exit code).
fn muse_command(distro: Option<&str>) -> Command {
    match distro {
        // Su Windows `muse` e' uno shim .cmd (niente .exe sullo stesso nome):
        // CreateProcess esegue solo .exe, quindi si passa da `cmd /c`, che
        // risolve anche .exe/.bat e gestisce i path con spazi se quotati
        // (Command quota gli argomenti in automatico).
        #[cfg(windows)]
        None => {
            let mut c = Command::new("cmd");
            c.args(["/c", "muse"]);
            c
        }
        #[cfg(not(windows))]
        None => Command::new("muse"),
        Some(d) => {
            let mut c = Command::new("wsl.exe");
            c.args(["-d", d, "--", "muse"]);
            c
        }
    }
}

fn run_command(cmd: &mut Command, args: &[&str], label: &str) -> Result<String, String> {
    cmd.args(args);
    let out = spawn_quiet(cmd)
        .output()
        .map_err(|e| format!("{label} non avviabile: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("{label} uscito con codice {:?}", out.status.code())
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_muse(distro: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut cmd = muse_command(distro);
    run_command(&mut cmd, args, "muse")
}

fn muse_found(distro: Option<&str>) -> bool {
    run_muse(distro, &["--version"]).is_ok()
}

fn current_exe_forward() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("percorso exe: {e}"))?;
    Ok(exe.to_string_lossy().replace('\\', "/"))
}

fn plugin_dir(app: &AppHandle, env_key: &str) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(format!("muse-plugin-{}", sanitize_env_key(env_key)));
    Ok(dir)
}

/// Scrive la dir del plugin per un env. Ritorna (dir_windows, dir_per_muse):
/// su WSL la seconda e' la traduzione /mnt/... del percorso Windows.
fn write_plugin(app: &AppHandle, env_key: &str, distro: Option<&str>) -> Result<(String, String), String> {
    let dir = plugin_dir(app, env_key)?;
    let exe = current_exe_forward()?;
    let exe_for_manifest = match distro {
        None => exe,
        Some(_) => windows_to_wsl_path(&exe)
            .ok_or_else(|| format!("percorso exe non traducibile per WSL: {exe}"))?,
    };
    let manifest = build_manifest_json(&exe_for_manifest, env_key);
    if manifest.is_empty() {
        return Err("manifest vuoto".to_string());
    }
    let plug_dir = dir.join(".muse-plugin");
    std::fs::create_dir_all(&plug_dir).map_err(|e| format!("scrittura plugin: {e}"))?;
    // String -> file: UTF-8 senza BOM (il validatore rifiuta il BOM).
    std::fs::write(plug_dir.join("plugin.json"), manifest)
        .map_err(|e| format!("scrittura plugin: {e}"))?;
    let win = dir.to_string_lossy().to_string();
    let per_muse = match distro {
        None => win.clone(),
        Some(_) => windows_to_wsl_path(&win)
            .ok_or_else(|| format!("percorso plugin non traducibile per WSL: {win}"))?,
    };
    Ok((win, per_muse))
}

// ---------------------------------------------------------------------------
// Comandi Tauri
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct MuseEnvStatus {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub muse_found: bool,
    pub installed: bool,
}

fn status_for(key: &str, label: &str, kind: &str, distro: Option<&str>) -> MuseEnvStatus {
    let found = muse_found(distro);
    let installed = if found {
        run_muse(distro, &["plugins", "list", "--json"])
            .map(|out| plugin_listed(&out, PLUGIN_ID))
            .unwrap_or(false)
    } else {
        false
    };
    MuseEnvStatus {
        key: key.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        muse_found: found,
        installed,
    }
}

/// Ogni env costa 2 processi (`muse --version` + `plugins list`) e su WSL
/// anche l'avvio della VM: gli env vengono sondati in parallelo.
fn collect_env_status() -> Vec<MuseEnvStatus> {
    let distros = installed_distros().unwrap_or_default();
    std::thread::scope(|s| {
        let win = s.spawn(|| status_for("windows", "Windows", "windows", None));
        let wsl: Vec<_> = distros
            .iter()
            .map(|d| s.spawn(move || status_for(d, d, "wsl", Some(d))))
            .collect();
        std::iter::once(win)
            .chain(wsl)
            .filter_map(|h| h.join().ok())
            .collect()
    })
}

#[tauri::command]
pub async fn muse_env_status() -> Result<Vec<MuseEnvStatus>, String> {
    crate::blocking(|| Ok(collect_env_status())).await
}

fn resolve_env(env_key: &str) -> Result<(String, Option<String>), String> {
    let key = env_key.trim();
    if key.is_empty() {
        return Err("env mancante".to_string());
    }
    if key == "windows" {
        Ok((key.to_string(), None))
    } else {
        Ok((key.to_string(), Some(key.to_string())))
    }
}

#[tauri::command]
pub async fn install_muse_plugin(app: AppHandle, env_key: String) -> Result<String, String> {
    crate::blocking(move || install_plugin_blocking(&app, &env_key)).await
}

fn install_plugin_blocking(app: &AppHandle, env_key: &str) -> Result<String, String> {
    let (key, distro) = resolve_env(env_key)?;
    let distro_ref = distro.as_deref();
    if !muse_found(distro_ref) {
        return Err("muse non trovato in questo env: installalo prima".to_string());
    }
    let (_win_dir, muse_dir) = write_plugin(app, &key, distro_ref)?;
    let mut log = vec![format!("plugin scritto per env '{key}'")];

    // Valida prima di installare: errori nostri, non dell'utente.
    run_muse(distro_ref, &["plugins", "validate", &muse_dir, "--json"]).map_err(|e| {
        format!("validazione plugin fallita: {e}\n{}", log.join("\n"))
    })?;
    log.push("validazione ok".to_string());

    // Reinstalla deterministica: rimuovi (ignora errori) poi installa.
    let _ = run_muse(distro_ref, &["plugins", "remove", LEGACY_PLUGIN_ID]);
    let _ = run_muse(distro_ref, &["plugins", "remove", PLUGIN_ID]);
    run_muse(distro_ref, &["plugins", "install", &muse_dir])
        .map_err(|e| format!("install fallita: {e}\n{}", log.join("\n")))?;
    log.push("install ok".to_string());

    // approve/enable: sintassi best-effort (lo status + il test danno la
    // prova reale); l'esito resta visibile nel log mostrato in UI.
    match run_muse(distro_ref, &["plugins", "approve", PLUGIN_ID]) {
        Ok(_) => log.push("approve ok".to_string()),
        Err(e) => log.push(format!("approve: {e}")),
    }
    match run_muse(distro_ref, &["plugins", "enable", PLUGIN_ID]) {
        Ok(_) => log.push("enable ok".to_string()),
        Err(e) => log.push(format!("enable: {e}")),
    }

    let installed = run_muse(distro_ref, &["plugins", "list", "--json"])
        .map(|out| plugin_listed(&out, PLUGIN_ID))
        .unwrap_or(false);
    log.push(if installed {
        "stato: installato".to_string()
    } else {
        "stato: NON rilevato in 'plugins list'".to_string()
    });
    Ok(log.join("\n"))
}

#[tauri::command]
pub async fn remove_muse_plugin(app: AppHandle, env_key: String) -> Result<String, String> {
    crate::blocking(move || {
        let (key, distro) = resolve_env(&env_key)?;
        let out = run_muse(distro.as_deref(), &["plugins", "remove", PLUGIN_ID])?;
        if let Ok(dir) = plugin_dir(&app, &key) {
            let _ = std::fs::remove_dir_all(dir);
        }
        Ok(out.trim().to_string())
    })
    .await
}

/// Self-test del canale: lancia questo stesso exe in modalita' --notify
/// (stesso percorso degli hook) e verifica l'exit code. Il ding arriva
/// poi via evento al frontend come prova udibile.
#[tauri::command]
pub async fn test_muse_notify() -> Result<String, String> {
    crate::blocking(self_test_blocking).await
}

fn self_test_blocking() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("percorso exe: {e}"))?;
    let mut cmd = Command::new(&exe);
    let status = spawn_quiet(&mut cmd)
        .args(["--notify", "test", "self-test"])
        .status()
        .map_err(|e| format!("self-test non avviabile: {e}"))?;
    if status.success() {
        Ok("segnale inviato: se senti il ding, il canale funziona.".to_string())
    } else {
        Err(format!(
            "self-test fallito (exit {:?}): l'app e' in ascolto?",
            status.code()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json_with_forward_slashes() {
        let raw = build_manifest_json("C:/app/cli-companion.exe", "windows");
        assert!(!raw.is_empty());
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["name"], PLUGIN_ID);
        let hooks = v["capabilities"]["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
        assert_eq!(hooks[0]["event"], "Stop");
        assert_eq!(hooks[1]["event"], "SessionEnd");
        for h in hooks {
            let cmd = h["command"].as_array().unwrap();
            assert_eq!(cmd[0], "C:/app/cli-companion.exe");
            assert!(!cmd[0].as_str().unwrap().contains('\\'));
            assert!(h["timeoutMs"].as_u64().unwrap() > 0);
        }
        // Nessun BOM: il primo byte e' '{'.
        assert_eq!(raw.as_bytes()[0], b'{');
    }

    #[test]
    fn manifest_supports_wsl_paths() {
        let raw = build_manifest_json("/mnt/c/app/cli-companion.exe", "Ubuntu-24.04");
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let cmd = v["capabilities"]["hooks"][0]["command"].as_array().unwrap();
        assert_eq!(cmd[0], "/mnt/c/app/cli-companion.exe");
        assert_eq!(cmd[3], "Ubuntu-24.04");
    }

    #[test]
    fn plugin_listed_matches_exact_id_only() {
        assert!(!plugin_listed(r#"{"plugins":[]}"#, PLUGIN_ID));
        assert!(!plugin_listed("non json", PLUGIN_ID));
        assert!(!plugin_listed(r#"{}"#, PLUGIN_ID));
        assert!(plugin_listed(
            r#"{"plugins":[{"id":"cli-companion-notify"}]}"#,
            PLUGIN_ID
        ));
        assert!(plugin_listed(
            r#"{"plugins":[{"name":"cli-companion-notify"}]}"#,
            PLUGIN_ID
        ));
        // Sottostringa di un altro id: non deve matchare.
        assert!(!plugin_listed(
            r#"{"plugins":[{"id":"cli-companion-notify-extra"}]}"#,
            PLUGIN_ID
        ));
    }

    #[test]
    fn plugin_listed_matches_real_list_shape() {
        // Forma reale di `muse plugins list --json` (ridotta): id annidato.
        let real = r#"{
            "plugins": [{
                "record": {"id": "cli-companion-notify", "enabled": true},
                "warning": "third-party plugin: hooks require review before activation",
                "plugin": {"id": "cli-companion-notify"},
                "valid": true, "active": true, "diagnostics": []
            }]
        }"#;
        assert!(plugin_listed(real, PLUGIN_ID));
        let other = real.replace("cli-companion-notify", "altro-plugin");
        assert!(!plugin_listed(&other, PLUGIN_ID));
    }

    #[test]
    fn sanitize_env_key_keeps_simple_names() {
        assert_eq!(sanitize_env_key("windows"), "windows");
        assert_eq!(sanitize_env_key("Ubuntu-24.04"), "Ubuntu-24.04");
        assert_eq!(sanitize_env_key("a/b\\c d"), "a-b-c-d");
    }

    #[test]
    fn wire_roundtrip_over_tcp() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        send_to(port, "stop", "windows").unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).unwrap();
        let payload = parse_line(&String::from_utf8_lossy(&buf[..n])).unwrap();
        assert_eq!(
            payload,
            NotifyPayload {
                event: "stop".to_string(),
                source: "windows".to_string(),
            }
        );
    }

    #[test]
    fn muse_command_wraps_windows_cmd_shim() {
        let c = muse_command(None);
        #[cfg(windows)]
        {
            use std::ffi::OsStr;
            assert_eq!(c.get_program(), "cmd");
            let args: Vec<_> = c.get_args().collect();
            assert_eq!(args, vec![OsStr::new("/c"), OsStr::new("muse")]);
        }
        #[cfg(not(windows))]
        assert_eq!(c.get_program(), "muse");
        let w = muse_command(Some("Ubuntu-24.04"));
        assert_eq!(w.get_program(), "wsl.exe");
        let wargs: Vec<_> = w.get_args().collect();
        assert!(wargs.iter().any(|a| a.to_string_lossy() == "Ubuntu-24.04"));
    }

    /// Regressione "muse non trovato" su Windows: `muse` si risolve a uno
    /// shim .cmd e CreateProcess cerca solo .exe per nome, quindi lo spawn
    /// diretto fallisce mentre `cmd /c` (PATHEXT) lo trova. Prova con uno
    /// shim finto in temp (niente dipendenza da muse installato).
    #[test]
    #[cfg(windows)]
    fn cmd_shim_found_only_through_cmd_c() {
        let dir =
            std::env::temp_dir().join(format!("mc-cmd-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mc-fake-shim.cmd"), "@echo fake-shim-ok\r\n").unwrap();

        // Solo lo shim in testa al PATH (ripristinato subito dopo gli spawn).
        let prev_path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.clone()];
        paths.extend(std::env::split_paths(&prev_path));
        let joined = std::env::join_paths(paths).unwrap();
        std::env::set_var("PATH", &joined);

        // Spawn diretto per nome come faceva il vecchio codice: non risolve.
        let direct = run_command(&mut Command::new("mc-fake-shim"), &[], "shim");
        // Via `cmd /c` come il nuovo codice: risolve ed esegue.
        let mut wrapped = Command::new("cmd");
        wrapped.args(["/c", "mc-fake-shim"]);
        let via_cmd = run_command(&mut wrapped, &[], "shim");

        std::env::set_var("PATH", &prev_path);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            direct.is_err(),
            "lo spawn diretto non deve risolvere gli shim .cmd"
        );
        let out = via_cmd.expect("cmd /c deve risolvere gli shim .cmd");
        assert!(out.contains("fake-shim-ok"));
    }

    #[test]
    fn wire_rejects_unknown_event_and_garbage() {
        assert!(build_line("nope", "x").is_err());
        assert!(parse_line("non json").is_err());
        assert!(parse_line(r#"{"event":"nope","source":"x"}"#).is_err());
        assert!(send_to(1, "stop", "x").is_err()); // porta chiusa
    }
}
