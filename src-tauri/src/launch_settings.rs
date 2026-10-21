//! Impostazioni globali degli avvii: l'avvio predefinito dei nuovi terminali
//! quando il workspace non ne ha uno suo, e il flag globale che abilita la
//! lettura dei consumi dell'agente (`show_usage`). Persistenza in
//! `launch_settings.json`.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

const SETTINGS_FILE: &str = "launch_settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchSettings {
    /// Id di un avvio integrato (`builtin:...`); `None` = shell semplice.
    #[serde(default)]
    pub default_launch: Option<String>,
    /// Mostra i consumi dell'agente (limiti e token). I file vecchi senza il
    /// campo valgono `true`.
    #[serde(default = "default_show_usage")]
    pub show_usage: bool,
}

fn default_show_usage() -> bool {
    true
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self { default_launch: None, show_usage: true }
    }
}

#[derive(Default)]
pub struct LaunchSettingsStore {
    inner: Mutex<LaunchSettings>,
    /// Serializza le modifiche: l'I/O avviene fuori dal lock `inner` (i lettori girano sul main thread).
    save: Mutex<()>,
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
pub async fn set_global_default_launch(app: AppHandle, launch_id: Option<String>) -> Result<LaunchSettings, String> {
    crate::blocking(move || {
        validate(&launch_id, &crate::agents::builtin_launch_list())?;
        update(&app, |s| s.default_launch = launch_id)
    })
    .await
}

#[tauri::command]
pub async fn set_show_usage(app: AppHandle, enabled: bool) -> Result<LaunchSettings, String> {
    crate::blocking(move || update(&app, |s| s.show_usage = enabled)).await
}

/// Applica `f`, salva e solo poi aggiorna la memoria (se il salvataggio fallisce
/// lo stato resta invariato). Il lock `inner` non copre l'I/O.
fn update(app: &AppHandle, f: impl FnOnce(&mut LaunchSettings)) -> Result<LaunchSettings, String> {
    let store = app.state::<LaunchSettingsStore>();
    let _save = store.save.lock().unwrap();
    let mut next = store.inner.lock().unwrap().clone();
    f(&mut next);
    save_to_disk(app, &next)?;
    *store.inner.lock().unwrap() = next.clone();
    Ok(next)
}

impl LaunchSettingsStore {
    /// Flag `show_usage` corrente.
    pub fn show_usage(&self) -> bool {
        self.inner.lock().unwrap().show_usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_serde() {
        assert_eq!(LaunchSettings::default().default_launch, None);
        let s: LaunchSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, LaunchSettings::default());
        let s = LaunchSettings { default_launch: Some("builtin:claude-code".into()), show_usage: false };
        let back: LaunchSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn show_usage_defaults_to_true_on_old_files() {
        assert!(LaunchSettings::default().show_usage);
        let old: LaunchSettings = serde_json::from_str(r#"{"default_launch":"builtin:claude-code"}"#).unwrap();
        assert!(old.show_usage);
        assert_eq!(old.default_launch.as_deref(), Some("builtin:claude-code"));
        let off: LaunchSettings = serde_json::from_str(r#"{"show_usage":false}"#).unwrap();
        assert!(!off.show_usage);
        assert!(LaunchSettingsStore::default().show_usage());
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
