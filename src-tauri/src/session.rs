//! Persistenza della sessione: stato dei tab (blob JSON opaco, lo schema e'
//! del frontend) e cronologia (scrollback) di ogni terminale.
//!
//! File in `<app_data>/`:
//! - `session.json`: l'ultimo stato salvato;
//! - `scrollback/<tab_id>.txt`: output serializzato di un terminale.

use crate::agent_notify::is_valid_tab_id;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const SESSION_FILE: &str = "session.json";
const SCROLLBACK_DIR: &str = "scrollback";
/// Tetto del blob di sessione.
const SESSION_MAX_BYTES: usize = 2 * 1024 * 1024;
/// Tetto dello scrollback di un singolo terminale.
const SCROLLBACK_MAX_BYTES: usize = 4 * 1024 * 1024;

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn scrollback_path(dir: &Path, tab_id: &str) -> Result<PathBuf, String> {
    if !is_valid_tab_id(tab_id) {
        return Err("id tab non valido".to_string());
    }
    Ok(dir.join(SCROLLBACK_DIR).join(format!("{tab_id}.txt")))
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("lettura {}: {e}", path.display())),
    }
}

fn write_capped(path: &Path, data: &str, max: usize, what: &str) -> Result<(), String> {
    if data.len() > max {
        return Err(format!("{what} troppo grande ({} byte, max {max})", data.len()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cartella dati: {e}"))?;
    }
    crate::write_atomic(path, data).map_err(|e| format!("scrittura {}: {e}", path.display()))
}

fn load_session_at(dir: &Path) -> Result<Option<String>, String> {
    read_optional(&dir.join(SESSION_FILE))
}

fn save_session_at(dir: &Path, data: &str) -> Result<(), String> {
    write_capped(&dir.join(SESSION_FILE), data, SESSION_MAX_BYTES, "sessione")
}

fn save_scrollback_at(dir: &Path, tab_id: &str, data: &str) -> Result<(), String> {
    write_capped(&scrollback_path(dir, tab_id)?, data, SCROLLBACK_MAX_BYTES, "scrollback")
}

fn load_scrollback_at(dir: &Path, tab_id: &str) -> Result<Option<String>, String> {
    read_optional(&scrollback_path(dir, tab_id)?)
}

fn delete_scrollback_at(dir: &Path, tab_id: &str) -> Result<(), String> {
    match std::fs::remove_file(scrollback_path(dir, tab_id)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("eliminazione scrollback: {e}")),
    }
}

/// Elimina i file di scrollback il cui tab non e' in `keep` (compresi i
/// temporanei rimasti da un crash). Ritorna quanti file ha rimosso.
fn prune_scrollback_at(dir: &Path, keep: &[String]) -> Result<usize, String> {
    let keep: HashSet<&str> = keep.iter().map(String::as_str).collect();
    let folder = dir.join(SCROLLBACK_DIR);
    let entries = match std::fs::read_dir(&folder) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(format!("lettura scrollback: {e}")),
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let kept = name
            .strip_suffix(".txt")
            .is_some_and(|id| keep.contains(id));
        if !kept && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[tauri::command]
pub async fn load_session(app: AppHandle) -> Result<Option<String>, String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || load_session_at(&dir)).await
}

#[tauri::command]
pub async fn save_session(app: AppHandle, data: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || save_session_at(&dir, &data)).await
}

#[tauri::command]
pub async fn save_scrollback(app: AppHandle, tab_id: String, data: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || save_scrollback_at(&dir, &tab_id, &data)).await
}

#[tauri::command]
pub async fn load_scrollback(app: AppHandle, tab_id: String) -> Result<Option<String>, String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || load_scrollback_at(&dir, &tab_id)).await
}

#[tauri::command]
pub async fn delete_scrollback(app: AppHandle, tab_id: String) -> Result<(), String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || delete_scrollback_at(&dir, &tab_id)).await
}

#[tauri::command]
pub async fn prune_scrollback(app: AppHandle, keep: Vec<String>) -> Result<(), String> {
    let dir = data_dir(&app)?;
    crate::blocking(move || prune_scrollback_at(&dir, &keep).map(|_| ())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn session_roundtrip_and_missing() {
        let d = temp_dir("session");
        assert_eq!(load_session_at(&d).unwrap(), None);
        save_session_at(&d, r#"{"version":1}"#).unwrap();
        assert_eq!(load_session_at(&d).unwrap().as_deref(), Some(r#"{"version":1}"#));
        let big = "x".repeat(SESSION_MAX_BYTES + 1);
        assert!(save_session_at(&d, &big).is_err());
        // Il file buono non e' stato toccato dal rifiuto.
        assert_eq!(load_session_at(&d).unwrap().as_deref(), Some(r#"{"version":1}"#));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scrollback_roundtrip_delete_and_validation() {
        let d = temp_dir("sb");
        assert_eq!(load_scrollback_at(&d, "abc-1").unwrap(), None);
        save_scrollback_at(&d, "abc-1", "ciao\x1b[0m").unwrap();
        assert_eq!(load_scrollback_at(&d, "abc-1").unwrap().as_deref(), Some("ciao\x1b[0m"));
        delete_scrollback_at(&d, "abc-1").unwrap();
        assert_eq!(load_scrollback_at(&d, "abc-1").unwrap(), None);
        // Eliminare un file assente non e' un errore.
        delete_scrollback_at(&d, "abc-1").unwrap();
        for bad in ["", "../x", "a/b", "a\\b", "a.b", &"x".repeat(200)] {
            assert!(save_scrollback_at(&d, bad, "x").is_err(), "{bad}");
            assert!(load_scrollback_at(&d, bad).is_err(), "{bad}");
            assert!(delete_scrollback_at(&d, bad).is_err(), "{bad}");
        }
        let big = "x".repeat(SCROLLBACK_MAX_BYTES + 1);
        assert!(save_scrollback_at(&d, "abc-1", &big).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_keeps_only_listed_tabs() {
        let d = temp_dir("prune");
        assert_eq!(prune_scrollback_at(&d, &[]).unwrap(), 0);
        for id in ["a", "b", "c"] {
            save_scrollback_at(&d, id, "x").unwrap();
        }
        std::fs::write(d.join(SCROLLBACK_DIR).join("z.json.tmp"), "x").unwrap();
        let removed = prune_scrollback_at(&d, &["b".to_string()]).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(load_scrollback_at(&d, "a").unwrap(), None);
        assert!(load_scrollback_at(&d, "b").unwrap().is_some());
        assert_eq!(load_scrollback_at(&d, "c").unwrap(), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
