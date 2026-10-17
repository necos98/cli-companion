use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// Lunghezza massima del comando di default (in caratteri).
pub const MAX_COMMAND_LEN: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Comando eseguito automaticamente a ogni nuovo terminale.
    /// Stringa vuota = nessun comando.
    #[serde(default)]
    pub default_command: String,
}

#[derive(Default)]
pub struct SettingsStore {
    inner: Mutex<Settings>,
}

const STORE_FILE: &str = "settings.json";

pub fn load_from_disk(app: &tauri::App) {
    let dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let path = dir.join(STORE_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(settings) => {
            let store = app.state::<SettingsStore>();
            *store.inner.lock().unwrap() = settings;
        }
        Err(_) => {
            // File corrotto: backup e riparto vuoto.
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
}

fn save_to_disk(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella dati: {e}"))?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join(STORE_FILE), &raw).map_err(|e| format!("impossibile salvare: {e}"))?;
    Ok(())
}

fn sanitize(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.chars().count() > MAX_COMMAND_LEN {
        trimmed.chars().take(MAX_COMMAND_LEN).collect()
    } else {
        trimmed.to_string()
    }
}

#[tauri::command]
pub fn get_settings(state: State<SettingsStore>) -> Settings {
    state.inner.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_default_command(
    app: AppHandle,
    state: State<SettingsStore>,
    command: String,
) -> Result<Settings, String> {
    let cleaned = sanitize(&command);
    let mut guard = state.inner.lock().unwrap();
    guard.default_command = cleaned;
    let out = guard.clone();
    save_to_disk(&app, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_trims_and_keeps_inner_spacing() {
        assert_eq!(sanitize("  npm run dev  "), "npm run dev");
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("   "), "");
    }

    #[test]
    fn sanitize_truncates_to_max_chars() {
        assert_eq!(sanitize(&"x".repeat(1500)), "x".repeat(MAX_COMMAND_LEN));
        assert_eq!(MAX_COMMAND_LEN, 1000);
    }

    #[test]
    fn settings_default_is_empty_command() {
        assert_eq!(Settings::default().default_command, "");
    }
}
