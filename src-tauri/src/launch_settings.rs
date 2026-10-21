//! Impostazioni globali degli avvii: l'avvio predefinito dei nuovi terminali
//! quando il workspace non ne ha uno suo. Persistenza in `launch_settings.json`.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

const SETTINGS_FILE: &str = "launch_settings.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSettings {
    /// Id di un avvio integrato (`builtin:...`); `None` = shell semplice.
    #[serde(default)]
    pub default_launch: Option<String>,
}

#[derive(Default)]
pub struct LaunchSettingsStore {
    inner: Mutex<LaunchSettings>,
}

/// Solo `None` o l'id di un avvio integrato (quelli personalizzati sono per workspace).
fn validate(launch_id: &Option<String>, builtin: &[crate::agents::BuiltinLaunch]) -> Result<(), String> {
    match launch_id {
        Some(id) if !builtin.iter().any(|b| b.id == *id) => Err("avvio predefinito non valido".to_string()),
        _ => Ok(()),
    }
}

pub fn load_from_disk(app: &tauri::App) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let path = dir.join(SETTINGS_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    match serde_json::from_str::<LaunchSettings>(&raw) {
        Ok(s) => *app.state::<LaunchSettingsStore>().inner.lock().unwrap() = s,
        Err(_) => {
            // File corrotto: backup e riparto dal default.
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
}

fn save_to_disk(app: &AppHandle, s: &LaunchSettings) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella dati: {e}"))?;
    let raw = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join(SETTINGS_FILE), &raw).map_err(|e| format!("impossibile salvare: {e}"))
}

#[tauri::command]
pub fn get_launch_settings(state: State<LaunchSettingsStore>) -> LaunchSettings {
    state.inner.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_global_default_launch(
    app: AppHandle,
    state: State<LaunchSettingsStore>,
    launch_id: Option<String>,
) -> Result<LaunchSettings, String> {
    validate(&launch_id, &crate::agents::builtin_launch_list())?;
    let mut s = state.inner.lock().unwrap();
    let next = LaunchSettings { default_launch: launch_id };
    save_to_disk(&app, &next)?;
    *s = next.clone();
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_serde() {
        assert_eq!(LaunchSettings::default().default_launch, None);
        let s: LaunchSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, LaunchSettings::default());
        let s = LaunchSettings { default_launch: Some("builtin:claude-code".into()) };
        let back: LaunchSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn only_none_or_builtin_ids_are_valid() {
        let builtin = crate::agents::builtin_launch_list();
        assert!(validate(&None, &builtin).is_ok());
        assert!(validate(&Some("builtin:claude-code".into()), &builtin).is_ok());
        assert!(validate(&Some("builtin:orchestrator".into()), &builtin).is_ok());
        assert!(validate(&Some("shell".into()), &builtin).is_err());
        assert!(validate(&Some("ln-1-1".into()), &builtin).is_err());
    }
}
