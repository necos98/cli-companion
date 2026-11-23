//! Impostazioni globali di Headroom, persistite in `headroom.json` tramite
//! `store`. Lo stato e' di processo (non un `State` di Tauri): lo leggono i
//! punti di avvio dei terminali, anche senza un `AppHandle`.

use crate::store::{adopt_envelope, data_dir, JsonFile, JsonStore};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::OnceLock;
use ts_rs::TS;

/// Porta predefinita del proxy Headroom.
pub const DEFAULT_PORT: u16 = 8787;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HeadroomSettings {
    /// Instrada le sessioni di Claude Code su Windows dal proxy.
    #[serde(default)]
    pub enabled: bool,
    /// Porta del proxy sull'host locale (1..=65535).
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl Default for HeadroomSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT }
    }
}

impl HeadroomSettings {
    /// Base URL del proxy: l'host e' sempre quello locale.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("La porta di Headroom deve essere fra 1 e 65535.".to_string());
        }
        Ok(())
    }
}

static FILE: JsonFile<HeadroomSettings> = JsonFile::new("headroom.json", &[adopt_envelope]).validated(validate_loaded);

fn validate_loaded(settings: &HeadroomSettings) -> Result<(), String> {
    settings.validate()
}

fn global() -> &'static JsonStore<HeadroomSettings> {
    static STORE: OnceLock<JsonStore<HeadroomSettings>> = OnceLock::new();
    STORE.get_or_init(|| JsonStore::new(&FILE))
}

/// Le impostazioni correnti; prima del caricamento e nei test sono quelle predefinite.
pub fn current() -> HeadroomSettings {
    global().get()
}

/// Valida, salva e pubblica le nuove impostazioni.
pub fn save(dir: &Path, settings: HeadroomSettings) -> Result<HeadroomSettings, String> {
    settings.validate()?;
    global().update(dir, |s| {
        *s = settings;
        Ok(())
    })
}

/// Carica `headroom.json` all'avvio.
pub fn load_from_disk(app: &tauri::App) {
    match data_dir(app.handle()) {
        Ok(dir) => global().load(&dir),
        Err(e) => log::error!("cartella dati non disponibile, impostazioni di Headroom non caricate: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_disabled_on_the_standard_port() {
        let s = HeadroomSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.port, 8787);
        assert_eq!(s.base_url(), "http://127.0.0.1:8787");
        assert_eq!(current(), s);
    }

    #[test]
    fn port_zero_is_rejected() {
        assert!(HeadroomSettings { enabled: true, port: 0 }.validate().is_err());
        assert!(HeadroomSettings { enabled: true, port: 1 }.validate().is_ok());
        assert!(HeadroomSettings { enabled: false, port: 65535 }.validate().is_ok());
    }

    #[test]
    fn file_parses_with_missing_fields_and_rejects_port_zero() {
        assert_eq!(FILE.parse("{}").unwrap(), HeadroomSettings::default());
        assert!(FILE.parse(r#"{"enabled":true,"port":0}"#).is_err());
        let custom = FILE.parse(r#"{"enabled":true,"port":9000}"#).unwrap();
        assert_eq!(custom, HeadroomSettings { enabled: true, port: 9000 });
    }
}
