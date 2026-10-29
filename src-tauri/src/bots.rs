//! Bot: ogni sessione dell'app e' un "bot" con un nome preso da una pool
//! configurabile dall'utente (puramente visivo). Persistenza in `bots.json`.
//! La pool predefinita (`DEFAULT_NAMES`) e' l'unica fonte: il frontend la riceve
//! da `get_bot_settings`.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

const SETTINGS_FILE: &str = "bots.json";
const NAME_MAX_CHARS: usize = 24;
const NAMES_MAX: usize = 200;

const DEFAULT_NAMES: [&str; 40] = [
    "Bolt", "Pixel", "Gizmo", "Sprocket", "Widget", "Byte", "Nova", "Cog", "Rusty", "Chip", "Echo",
    "Flux", "Glitch", "Hex", "Ion", "Jolt", "Kilo", "Loop", "Mochi", "Nano", "Orbit", "Patch",
    "Quark", "Ratchet", "Servo", "Tesla", "Unit", "Volt", "Watt", "Zippy", "Blip", "Cosmo",
    "Dynamo", "Fizz", "Gadget", "Rivet", "Spark", "Tinker", "Turbo", "Zed",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotSettings {
    /// Mostra le sessioni come bot. I file senza il campo valgono `true`.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Pool di nomi. Un file senza il campo vale la pool predefinita; una
    /// lista vuota e' ammessa (i bot si chiamano "Bot 1", "Bot 2"...).
    #[serde(default = "default_names")]
    pub names: Vec<String>,
}

fn default_enabled() -> bool {
    true
}

fn default_names() -> Vec<String> {
    DEFAULT_NAMES.iter().map(|n| n.to_string()).collect()
}

impl Default for BotSettings {
    fn default() -> Self {
        Self { enabled: true, names: default_names() }
    }
}

#[derive(Default)]
pub struct BotSettingsStore {
    inner: Mutex<BotSettings>,
    /// Serializza le modifiche: l'I/O avviene fuori dal lock `inner`.
    save: Mutex<()>,
}

/// Normalizza la pool: trim, 1-24 caratteri, niente caratteri di controllo,
/// niente duplicati (senza distinguere le maiuscole), al massimo 200 nomi.
fn validate_names(names: Vec<String>) -> Result<Vec<String>, String> {
    if names.len() > NAMES_MAX {
        return Err(format!("al massimo {NAMES_MAX} nomi"));
    }
    let mut clean: Vec<String> = Vec::with_capacity(names.len());
    for raw in names {
        let name = raw.trim();
        let chars = name.chars().count();
        if chars == 0 || chars > NAME_MAX_CHARS {
            return Err(format!("il nome deve avere da 1 a {NAME_MAX_CHARS} caratteri"));
        }
        if name.chars().any(char::is_control) {
            return Err(format!("il nome “{name}” contiene caratteri non validi"));
        }
        if clean.iter().any(|n| n.to_lowercase() == name.to_lowercase()) {
            return Err(format!("il nome “{name}” è già nell'elenco"));
        }
        clean.push(name.to_string());
    }
    Ok(clean)
}

pub fn load_from_disk(app: &tauri::App) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let path = dir.join(SETTINGS_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    match serde_json::from_str::<BotSettings>(&raw) {
        Ok(s) => *app.state::<BotSettingsStore>().inner.lock().unwrap() = s,
        Err(_) => {
            // File corrotto: backup e riparto dal default.
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
        }
    }
}

fn save_to_disk(app: &AppHandle, s: &BotSettings) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella dati: {e}"))?;
    let raw = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    crate::write_atomic(&dir.join(SETTINGS_FILE), &raw).map_err(|e| format!("impossibile salvare: {e}"))
}

#[tauri::command]
pub fn get_bot_settings(state: State<BotSettingsStore>) -> BotSettings {
    state.inner.lock().unwrap().clone()
}

#[tauri::command]
pub async fn set_bot_settings(app: AppHandle, enabled: bool, names: Vec<String>) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let names = validate_names(names)?;
        update(&app, |s| *s = BotSettings { enabled, names })
    })
    .await
}

#[tauri::command]
pub async fn reset_bot_names(app: AppHandle) -> Result<BotSettings, String> {
    crate::blocking(move || update(&app, restore_default_names)).await
}

/// Rimette il pool di nomi predefinito; `enabled` resta com'e'.
fn restore_default_names(s: &mut BotSettings) {
    s.names = default_names();
}

/// Applica `f`, salva e solo poi aggiorna la memoria (se il salvataggio fallisce
/// lo stato resta invariato). Il lock `inner` non copre l'I/O.
fn update(app: &AppHandle, f: impl FnOnce(&mut BotSettings)) -> Result<BotSettings, String> {
    let store = app.state::<BotSettingsStore>();
    let _save = store.save.lock().unwrap();
    let mut next = store.inner.lock().unwrap().clone();
    f(&mut next);
    save_to_disk(app, &next)?;
    *store.inner.lock().unwrap() = next.clone();
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn default_is_enabled_with_the_builtin_pool() {
        let s = BotSettings::default();
        assert!(s.enabled);
        assert_eq!(s.names.len(), DEFAULT_NAMES.len());
        assert_eq!(s.names[0], "Bolt");
        assert_eq!(s.names.last().map(String::as_str), Some("Zed"));
        assert_eq!(BotSettingsStore::default().inner.lock().unwrap().clone(), s);
    }

    #[test]
    fn restoring_names_keeps_enabled() {
        for enabled in [true, false] {
            let mut s = BotSettings { enabled, names: names(&["Ada"]) };
            restore_default_names(&mut s);
            assert_eq!(s, BotSettings { enabled, names: default_names() });
        }
    }

    #[test]
    fn default_pool_passes_its_own_validation() {
        assert_eq!(validate_names(default_names()).unwrap(), default_names());
    }

    #[test]
    fn old_and_partial_files_fall_back_to_defaults() {
        let s: BotSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, BotSettings::default());
        let off: BotSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!off.enabled);
        assert_eq!(off.names, default_names());
        let custom: BotSettings = serde_json::from_str(r#"{"names":["Ada"]}"#).unwrap();
        assert!(custom.enabled);
        assert_eq!(custom.names, names(&["Ada"]));
        assert!(serde_json::from_str::<BotSettings>("non json").is_err());
    }

    #[test]
    fn serde_roundtrip_keeps_an_empty_pool() {
        let s = BotSettings { enabled: false, names: vec![] };
        let back: BotSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn validation_trims_and_accepts_an_empty_list() {
        assert_eq!(validate_names(names(&["  Ada ", "Bob"])).unwrap(), names(&["Ada", "Bob"]));
        assert_eq!(validate_names(vec![]).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn validation_rejects_bad_lengths() {
        assert!(validate_names(names(&[""])).is_err());
        assert!(validate_names(names(&["   "])).is_err());
        assert!(validate_names(vec!["a".repeat(NAME_MAX_CHARS)]).is_ok());
        assert!(validate_names(vec!["a".repeat(NAME_MAX_CHARS + 1)]).is_err());
        // Il limite conta i caratteri, non i byte.
        assert!(validate_names(vec!["é".repeat(NAME_MAX_CHARS)]).is_ok());
    }

    #[test]
    fn validation_rejects_control_characters() {
        assert!(validate_names(names(&["a\nb"])).is_err());
        assert!(validate_names(names(&["a\u{0}b"])).is_err());
        assert!(validate_names(names(&["a\tb"])).is_err());
    }

    #[test]
    fn validation_rejects_duplicates_ignoring_case() {
        assert!(validate_names(names(&["Bolt", "bolt"])).is_err());
        assert!(validate_names(names(&["Bolt", " BOLT "])).is_err());
        assert!(validate_names(names(&["Bolt", "Bolt 2"])).is_ok());
    }

    #[test]
    fn validation_caps_the_pool_size() {
        let at_cap: Vec<String> = (0..NAMES_MAX).map(|i| format!("Bot{i}")).collect();
        assert!(validate_names(at_cap.clone()).is_ok());
        let mut over = at_cap;
        over.push("Extra".into());
        assert!(validate_names(over).is_err());
    }
}
