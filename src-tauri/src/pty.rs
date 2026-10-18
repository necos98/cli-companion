use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::agent_notify::{SESSION_ENV, TOKEN_ENV};
use crate::workspaces::WorkspaceStore;

const MAX_SESSIONS: usize = 32;

/// Messaggi sul canale dedicato di una sessione: `{kind:"output",data}` o
/// `{kind:"exit"}`. Un canale per terminale (invece di un evento globale)
/// evita che ogni tab riceva e scarti l'output di tutti gli altri.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PtyEvent {
    Output { data: String },
    Exit,
}

struct PtySession {
    /// Id del tab che ha aperto la sessione (serve a scriverci da fuori).
    tab_id: String,
    /// Workspace in cui e' stato aperto il tab.
    workspace_id: String,
    master: Box<dyn MasterPty + Send>,
    /// Arc: pty_write lo clona e scrive senza tenere il lock della mappa,
    /// cosi' una shell che non legge non blocca le altre sessioni.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

pub struct PtyState {
    sessions: Arc<Mutex<HashMap<u32, PtySession>>>,
    next_id: AtomicU32,
}

impl Default for PtyState {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU32::new(1),
        }
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(2, 300),
        cols: cols.clamp(2, 500),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Decodifica UTF-8 incrementale: i chunk del pty possono spezzare un carattere
/// multibyte (accenti nei path, box drawing). I byte incompleti restano in coda.
fn decode_incremental(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        if pending.is_empty() {
            break;
        }
        match std::str::from_utf8(pending) {
            Ok(valid) => {
                out.push_str(valid);
                pending.clear();
                break;
            }
            Err(e) => {
                let up_to = e.valid_up_to();
                if up_to > 0 {
                    out.push_str(&String::from_utf8_lossy(&pending[..up_to]));
                    pending.drain(..up_to);
                } else if e.error_len().is_none() {
                    break; // sequenza incompleta: aspetto altri byte
                } else {
                    out.push('\u{FFFD}');
                    pending.drain(..1);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decode_ascii_passthrough() {
    let mut pending = b"ciao".to_vec();
    assert_eq!(decode_incremental(&mut pending), "ciao");
    assert!(pending.is_empty());
  }

  #[test]
  fn decode_split_multibyte_waits_for_rest() {
    // "à" = C3 A0 spezzata tra due chunk: il primo non emette nulla.
    let mut pending = vec![0xC3];
    assert_eq!(decode_incremental(&mut pending), "");
    assert_eq!(pending, vec![0xC3]);
    pending.push(0xA0);
    assert_eq!(decode_incremental(&mut pending), "à");
    assert!(pending.is_empty());
  }

  #[test]
  fn decode_invalid_byte_emits_replacement() {
    let mut pending = vec![0xFF];
    assert_eq!(decode_incremental(&mut pending), "�");
    assert!(pending.is_empty());
  }

  #[test]
  fn powershell_detection_matches_names_paths_and_case() {
    for yes in [
      "powershell",
      "powershell.exe",
      "pwsh",
      "pwsh.exe",
      "POWERSHELL.EXE",
      "  pwsh  ",
      r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
      r"C:\Program Files\PowerShell\7\pwsh.exe",
    ] {
      assert!(is_powershell(yes), "{yes}");
    }
    for no in ["", "cmd", "cmd.exe", "bash", "zsh", "nu", "powershell.exe.bak"] {
      assert!(!is_powershell(no), "{no}");
    }
  }

  #[test]
  fn title_hook_emits_osc_and_chains_prompt() {
    assert!(POWERSHELL_TITLE_HOOK.contains("function global:prompt"));
    assert!(POWERSHELL_TITLE_HOOK.contains("[char]27)]0;"));
    assert!(POWERSHELL_TITLE_HOOK.contains("RawUI.WindowTitle"));
    assert!(POWERSHELL_TITLE_HOOK.contains("Get-Command prompt"));
  }

  /// Esercita il vero percorso di consegna dell'hook (argv di CreateProcess
  /// -> powershell.exe) e ne verifica il comportamento end-to-end.
  #[test]
  #[cfg(windows)]
  fn title_hook_roundtrip_through_powershell() {
    fn run(prefix: &str, suffix: &str) -> String {
      let cmd = format!("{prefix}{POWERSHELL_TITLE_HOOK}{suffix}");
      let out = std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-Command", &cmd])
        .output()
        .expect("powershell.exe non avviabile");
      assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
      String::from_utf8_lossy(&out.stdout).into_owned()
    }

    // Default: OSC 0 con fallback + prompt ancora funzionante.
    let out = run("", "; 1+1; prompt");
    assert!(out.contains('2'), "comandi rotti? output: {out:?}");
    assert!(out.contains("\u{1b}]0;"), "OSC 0 mancante: {out:?}");
    assert!(out.contains('\u{7}'), "BEL mancante: {out:?}");

    // Mirror: un titolo impostato via API Win32 (come fanno gli agenti)
    // viene riemesso come OSC al prompt successivo.
    let out = run("", "; $Host.UI.RawUI.WindowTitle = 'AGENTE-123'; prompt");
    assert!(out.contains("]0;AGENTE-123\x07"), "mirror mancante: {out:?}");

    // Chaining: un prompt definito dal profilo viene preservato.
    let out = run("function prompt { 'CUSTOM> ' }; ", "; prompt");
    assert!(out.contains("CUSTOM> "), "prompt profilo perso: {out:?}");
    assert!(out.contains("\u{1b}]0;"), "OSC 0 mancante con profilo: {out:?}");
  }

  #[test]
  fn wslenv_merge_appends_without_duplicates() {
    let v = ["CLI_COMPANION_SESSION"];
    assert_eq!(merge_wslenv(None, &v), "CLI_COMPANION_SESSION/u");
    assert_eq!(merge_wslenv(Some(""), &v), "CLI_COMPANION_SESSION/u");
    assert_eq!(merge_wslenv(Some("PATH/l"), &v), "PATH/l:CLI_COMPANION_SESSION/u");
    assert_eq!(
      merge_wslenv(Some("A:B/p:"), &v),
      "A:B/p:CLI_COMPANION_SESSION/u"
    );
    // Gia' presente (con o senza flag): invariato.
    assert_eq!(
      merge_wslenv(Some("A:CLI_COMPANION_SESSION/u"), &v),
      "A:CLI_COMPANION_SESSION/u"
    );
    assert_eq!(merge_wslenv(Some("CLI_COMPANION_SESSION"), &v), "CLI_COMPANION_SESSION");
    // Nome che contiene solo la variabile come prefisso: non conta.
    assert_eq!(
      merge_wslenv(Some("CLI_COMPANION_SESSION_X"), &v),
      "CLI_COMPANION_SESSION_X:CLI_COMPANION_SESSION/u"
    );
  }

  #[test]
  fn wslenv_merge_handles_several_vars() {
    let v = ["S", "T"];
    assert_eq!(merge_wslenv(None, &v), "S/u:T/u");
    assert_eq!(merge_wslenv(Some("A/p"), &v), "A/p:S/u:T/u");
    // Una gia' presente: si aggiunge solo l'altra.
    assert_eq!(merge_wslenv(Some("T/u"), &v), "T/u:S/u");
    assert_eq!(merge_wslenv(Some("S:T/u"), &v), "S:T/u");
  }

  #[test]
  fn session_env_is_set_and_wsl_adds_wslenv() {
    let mut c = CommandBuilder::new("dummy");
    apply_session_env(&mut c, "tab-1", false);
    assert_eq!(c.get_env("CLI_COMPANION_SESSION"), Some(std::ffi::OsStr::new("tab-1")));
    let token = c.get_env("CLI_COMPANION_TOKEN").unwrap().to_string_lossy().into_owned();
    assert_eq!(token, crate::automation::token());
    assert!(c.get_env("WSLENV").is_none());
    let mut w = CommandBuilder::new("dummy");
    apply_session_env(&mut w, "tab-1", true);
    let wslenv = w.get_env("WSLENV").unwrap().to_string_lossy().into_owned();
    assert!(wslenv.split(':').any(|e| e == "CLI_COMPANION_SESSION/u"), "{wslenv}");
    assert!(wslenv.split(':').any(|e| e == "CLI_COMPANION_TOKEN/u"), "{wslenv}");
  }

  #[test]
  fn size_is_clamped() {
    let s = pty_size(0, 1000);
    assert_eq!((s.cols, s.rows), (2, 300));
    let s = pty_size(80, 24);
    assert_eq!((s.cols, s.rows), (80, 24));
  }

  #[test]
  fn color_env_declares_256_and_truecolor() {
    let mut cmd = CommandBuilder::new("dummy");
    apply_color_env(&mut cmd);
    assert_eq!(
      cmd.get_env("TERM"),
      Some(std::ffi::OsStr::new("xterm-256color"))
    );
    assert_eq!(
      cmd.get_env("COLORTERM"),
      Some(std::ffi::OsStr::new("truecolor"))
    );
  }
}

/// Hint di capacità colore per i figli del PTY (CLI Node/chalk, Python,
/// Rust termcolor...): senza TERM/COLORTERM molti tool disabilitano i colori
/// perché l'environment ereditato dall'app GUI non ne dichiara il supporto.
/// xterm.js gestisce 256 colori + truecolor, quindi li dichiariamo entrambi.
/// Niente FORCE_COLOR: forzerebbe i colori anche contro NO_COLOR/--no-color.
fn apply_color_env(cmd: &mut CommandBuilder) {
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
}

/// Aggiunge le `vars` a un valore di WSLENV (`A/p:B`), separato da `:`, senza
/// duplicarle se ci sono gia' (con o senza flag). Il flag `/u` le fa passare
/// da Windows alla distro; i processi Windows lanciati da li' (l'exe degli
/// hook, via interop) le ricevono ancora dall'ambiente d'origine.
fn merge_wslenv(existing: Option<&str>, vars: &[&str]) -> String {
  let mut entries: Vec<String> = existing
    .unwrap_or("")
    .trim_matches(':')
    .split(':')
    .filter(|e| !e.is_empty())
    .map(str::to_string)
    .collect();
  for var in vars {
    if !entries.iter().any(|e| e.split('/').next() == Some(*var)) {
      entries.push(format!("{var}/u"));
    }
  }
  entries.join(":")
}

/// Imposta `CLI_COMPANION_SESSION` (id del tab) e `CLI_COMPANION_TOKEN` per
/// gli hook degli agenti e il server MCP; su WSL le fa attraversare il
/// confine con WSLENV.
fn apply_session_env(cmd: &mut CommandBuilder, tab_id: &str, wsl: bool) {
  cmd.env(SESSION_ENV, tab_id);
  cmd.env(TOKEN_ENV, crate::automation::token());
  if wsl {
    let parent = std::env::var("WSLENV").ok();
    cmd.env("WSLENV", merge_wslenv(parent.as_deref(), &[SESSION_ENV, TOKEN_ENV]));
  }
}

fn is_powershell(shell: &str) -> bool {
    let file = shell
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(shell)
        .trim()
        .to_lowercase();
    let stem = file.strip_suffix(".exe").unwrap_or(&file);
    matches!(stem, "powershell" | "pwsh")
}

/// Hook iniettato in PowerShell via `-Command`: a ogni prompt riemette il
/// titolo console come sequenza `OSC 0` (`ESC ] 0 ; titolo BEL`).
///
/// Su Windows serve perche' ConPTY non inoltra nello stream VT i titoli
/// impostati via API Win32 (`$Host.UI.RawUI.WindowTitle = ...`, che e' come
/// gli agenti impostano il titolo): senza questo hook il tab non si aggiorna
/// mai. Se il titolo e' ancora quello di default, usa la cartella corrente.
/// Il `prompt` preesistente (profilo utente, oh-my-posh, ...) e' preservato.
const POWERSHELL_TITLE_HOOK: &str = r#"$__mcInitTitle = ''; try { $__mcInitTitle = $Host.UI.RawUI.WindowTitle } catch { }; $__mcPrevPrompt = $null; try { $__mcPrevPrompt = (Get-Command prompt -CommandType Function -ErrorAction Stop).ScriptBlock } catch { }; function global:prompt { $__mcTitle = ''; try { $__mcTitle = $Host.UI.RawUI.WindowTitle; if ([string]::IsNullOrWhiteSpace($__mcTitle) -or $__mcTitle -eq $__mcInitTitle) { $__mcTitle = Split-Path -Leaf -Path (Get-Location).Path }; if ([string]::IsNullOrWhiteSpace($__mcTitle)) { $__mcTitle = 'PS' } } catch { $__mcTitle = 'PS' }; $__mcText = ''; try { if ($__mcPrevPrompt) { $__mcText = (@(& $__mcPrevPrompt) -join '') } else { $__mcText = "PS $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel + 1)) " } } catch { $__mcText = 'PS> ' }; "$([char]27)]0;$__mcTitle`a$__mcText" }"#;

/// Async: lo spawn verifica la cartella WSL con `wsl.exe` (secondi a VM
/// spenta) e avvia la shell; sul main thread congelerebbe la finestra.
#[tauri::command]
pub async fn pty_spawn(
    app: AppHandle,
    workspace_id: String,
    tab_id: String,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    on_event: Channel<PtyEvent>,
) -> Result<u32, String> {
    if !crate::agent_notify::is_valid_tab_id(&tab_id) {
        return Err("id tab non valido".to_string());
    }
    crate::blocking(move || {
        spawn_blocking_session(&app, &workspace_id, &tab_id, cols, rows, cwd, on_event)
    })
    .await
}

fn spawn_blocking_session(
    app: &AppHandle,
    workspace_id: &str,
    tab_id: &str,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    on_event: Channel<PtyEvent>,
) -> Result<u32, String> {
    let pty = app.state::<PtyState>();
    let store = app.state::<WorkspaceStore>();
    let ws = store
        .find(workspace_id)
        .ok_or_else(|| "workspace non trovato".to_string())?;
    // Cartella di partenza: quella richiesta (sessioni automatiche) oppure
    // quella del workspace.
    let dir = cwd
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or(&ws.path)
        .to_string();

    {
        let sessions = pty.sessions.lock().unwrap();
        if sessions.len() >= MAX_SESSIONS {
            return Err("troppe sessioni aperte".to_string());
        }
    }

    let cmd = if ws.kind == "wsl" {
        let distro = ws.distro.clone().unwrap_or_default();
        if distro.is_empty() {
            return Err("distro mancante nel workspace".to_string());
        }
        if !crate::wsl::wsl_dir_exists(&distro, &dir) {
            return Err(format!("cartella non trovata in {distro}: {dir}"));
        }
        let mut c = CommandBuilder::new("wsl.exe");
        c.arg("-d");
        c.arg(&distro);
        c.arg("--cd");
        c.arg(&dir);
        if let Some(shell) = ws.shell.clone().filter(|s| !s.is_empty()) {
            c.arg("-e");
            c.arg(&shell);
        }
        apply_color_env(&mut c);
        apply_session_env(&mut c, tab_id, true);
        c
    } else {
        let meta =
            std::fs::metadata(&dir).map_err(|_| format!("cartella non trovata: {dir}"))?;
        if !meta.is_dir() {
            return Err(format!("non è una cartella: {dir}"));
        }
        let shell = ws
            .shell
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "powershell.exe".to_string());
        let mut c = CommandBuilder::new(&shell);
        if is_powershell(&shell) {
            c.arg("-NoLogo");
            c.arg("-NoExit");
            c.arg("-Command");
            c.arg(POWERSHELL_TITLE_HOOK);
        }
        c.cwd(&dir);
        apply_color_env(&mut c);
        apply_session_env(&mut c, tab_id, false);
        c
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(cols, rows))
        .map_err(|e| format!("pty: {e}"))?;
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn: {e}"))?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("pty reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("pty writer: {e}"))?;

    let session_id = pty.next_id.fetch_add(1, Ordering::SeqCst);
    pty.sessions.lock().unwrap().insert(
        session_id,
        PtySession {
            tab_id: tab_id.to_string(),
            workspace_id: workspace_id.to_string(),
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            child: Mutex::new(child),
        },
    );

    // Thread di lettura: inoltra l'output al frontend sul canale della sessione.
    let sessions = pty.sessions.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    let text = decode_incremental(&mut pending);
                    if !text.is_empty() {
                        let _ = on_event.send(PtyEvent::Output { data: text });
                    }
                }
                Err(_) => break,
            }
        }
        if !pending.is_empty() {
            let text = String::from_utf8_lossy(&pending).into_owned();
            let _ = on_event.send(PtyEvent::Output { data: text });
        }
        let session = sessions.lock().unwrap().remove(&session_id);
        if let Some(sess) = session {
            let mut child = sess.child.lock().unwrap();
            let _ = child.wait();
        }
        let _ = on_event.send(PtyEvent::Exit);
    });

    Ok(session_id)
}

/// Scrive nel terminale del tab `tab_id` (usato dall'automazione per
/// digitare messaggi nei prompt degli agenti). Come `pty_write`, non tiene il
/// lock della mappa durante la scrittura.
pub fn write_to_tab(app: &AppHandle, tab_id: &str, data: &str) -> Result<(), String> {
    let pty = app.state::<PtyState>();
    let writer = pty
        .sessions
        .lock()
        .unwrap()
        .values()
        .find(|sess| sess.tab_id == tab_id)
        .map(|sess| sess.writer.clone())
        .ok_or_else(|| "terminale del tab non trovato".to_string())?;
    let mut writer = writer.lock().unwrap();
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

/// Id del workspace in cui e' aperto il tab `tab_id` (None se non ha un pty vivo).
pub fn workspace_of_tab(app: &AppHandle, tab_id: &str) -> Option<String> {
    let pty = app.state::<PtyState>();
    let sessions = pty.sessions.lock().unwrap();
    sessions
        .values()
        .find(|sess| sess.tab_id == tab_id)
        .map(|sess| sess.workspace_id.clone())
}

#[tauri::command]
pub fn pty_write(pty: State<PtyState>, session_id: u32, data: String) -> Result<(), String> {
    let writer = pty
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .map(|sess| sess.writer.clone())
        .ok_or_else(|| "sessione non trovata".to_string())?;
    let mut writer = writer.lock().unwrap();
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn pty_resize(
    pty: State<PtyState>,
    session_id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let sessions = pty.sessions.lock().unwrap();
    let sess = sessions
        .get(&session_id)
        .ok_or_else(|| "sessione non trovata".to_string())?;
    sess.master
        .resize(pty_size(cols, rows))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn pty_close(pty: State<PtyState>, session_id: u32) -> Result<(), String> {
    let session = pty.sessions.lock().unwrap().remove(&session_id);
    if let Some(sess) = session {
        let mut child = sess.child.lock().unwrap();
        let _ = child.kill();
    }
    Ok(())
}
