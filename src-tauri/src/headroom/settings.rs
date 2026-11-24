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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HeadroomSettings {
    /// Instrada dal proxy le sessioni degli agenti che sanno farsi instradare, su Windows.
    #[serde(default)]
    pub enabled: bool,
    /// Porta del proxy sull'host locale (1..=65535).
    #[serde(default = "default_port")]
    pub port: u16,
    /// L'app avvia il proxy da sola (e lo ferma all'uscita o alla disattivazione).
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
    /// Eseguibile di `headroom`; vuoto = risolto in automatico.
    #[serde(default)]
    pub executable: String,
    /// Upstream compatibile OpenAI a cui il proxy inoltra le richieste in
    /// formato OpenAI (senza `/v1`); vuoto = quello predefinito di Headroom.
    #[serde(default)]
    pub openai_upstream: String,
}

/// Upstream OpenAI predefinito di Headroom.
pub const DEFAULT_OPENAI_UPSTREAM: &str = "https://api.openai.com";
/// Lunghezza massima dell'upstream OpenAI.
const OPENAI_UPSTREAM_MAX: usize = 300;

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_auto_start() -> bool {
    true
}

impl Default for HeadroomSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            auto_start: true,
            executable: String::new(),
            openai_upstream: String::new(),
        }
    }
}

impl HeadroomSettings {
    /// Base URL del proxy: l'host e' sempre quello locale.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Upstream OpenAI effettivo (senza `/` finale): quello configurato o il predefinito.
    pub fn effective_openai_upstream(&self) -> String {
        match self.openai_upstream.trim_end_matches('/') {
            "" => DEFAULT_OPENAI_UPSTREAM.to_string(),
            configured => configured.to_string(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.port == 0 {
            return Err("La porta di Headroom deve essere fra 1 e 65535.".to_string());
        }
        if self.executable.trim() != self.executable || self.executable.contains(['\n', '\r', '\0']) {
            return Err("L'eseguibile di Headroom non e' valido: niente spazi ai bordi o a capo.".to_string());
        }
        validate_openai_upstream(&self.openai_upstream)
    }
}

/// Vuoto, oppure un URL `http://` o `https://` senza spazi o caratteri di
/// controllo, senza `/v1` finale (lo aggiunge Headroom alla rotta).
fn validate_openai_upstream(upstream: &str) -> Result<(), String> {
    if upstream.is_empty() {
        return Ok(());
    }
    let bad = |why: &str| Err(format!("L'upstream OpenAI di Headroom non e' valido: {why}."));
    if upstream.chars().count() > OPENAI_UPSTREAM_MAX {
        return bad("al massimo 300 caratteri");
    }
    if upstream.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return bad("niente spazi o a capo");
    }
    let rest = upstream.strip_prefix("https://").or_else(|| upstream.strip_prefix("http://"));
    match rest.map(|r| r.trim_end_matches('/')) {
        None | Some("") => bad("deve iniziare con http:// o https://"),
        Some(r) if r.to_ascii_lowercase().ends_with("/v1") => bad("senza /v1 finale, lo aggiunge Headroom"),
        Some(_) => Ok(()),
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
pub fn save(dir: &Path, mut settings: HeadroomSettings) -> Result<HeadroomSettings, String> {
    settings.executable = settings.executable.trim().to_string();
    settings.openai_upstream = settings.openai_upstream.trim().trim_end_matches('/').to_string();
    settings.validate()?;
    global().update(dir, |s| {
        *s = settings;
        Ok(())
    })
}

/// Carica `headroom.json` all'avvio.
pub fn load_from_disk(app: &tauri::App) {
    match data_dir(app.handle()) {
        Ok(dir) => {
            global().load(&dir);
            super::process::set_data_dir(dir);
        }
        Err(e) => log::error!("cartella dati non disponibile, impostazioni di Headroom non caricate: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(enabled: bool, port: u16) -> HeadroomSettings {
        HeadroomSettings { enabled, port, ..HeadroomSettings::default() }
    }

    #[test]
    fn defaults_are_disabled_on_the_standard_port() {
        let s = HeadroomSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.port, 8787);
        assert_eq!(s.base_url(), "http://127.0.0.1:8787");
        assert!(s.auto_start);
        assert!(s.executable.is_empty());
        assert!(s.openai_upstream.is_empty());
        assert_eq!(current(), s);
    }

    #[test]
    fn executable_must_be_trimmed_and_single_line() {
        let with_exe = |e: &str| HeadroomSettings { executable: e.to_string(), ..HeadroomSettings::default() };
        assert!(with_exe("C:\\tools\\headroom.exe").validate().is_ok());
        assert!(with_exe(" headroom").validate().is_err());
        assert!(with_exe("head\nroom").validate().is_err());
        assert!(with_exe("head\0room").validate().is_err());
    }

    #[test]
    fn openai_upstream_is_empty_or_a_clean_http_url() {
        let with_up = |u: &str| HeadroomSettings { openai_upstream: u.to_string(), ..HeadroomSettings::default() };
        assert!(with_up("").validate().is_ok());
        assert!(with_up("https://api.deepseek.com").validate().is_ok());
        assert!(with_up("http://localhost:11434/").validate().is_ok());
        assert!(with_up("https://openrouter.ai/api").validate().is_ok());
        for bad in
            ["api.deepseek.com", "ftp://x", "https://", "https://a b", "https://a\nb", "https://x/v1", "https://x/V1/"]
        {
            assert!(with_up(bad).validate().is_err(), "{bad:?}");
        }
        assert!(with_up(&format!("https://{}", "a".repeat(300))).validate().is_err());
    }

    #[test]
    fn effective_openai_upstream_falls_back_to_the_default() {
        let with_up = |u: &str| HeadroomSettings { openai_upstream: u.to_string(), ..HeadroomSettings::default() };
        assert_eq!(with_up("").effective_openai_upstream(), "https://api.openai.com");
        assert_eq!(with_up("https://api.deepseek.com/").effective_openai_upstream(), "https://api.deepseek.com");
    }

    #[test]
    fn old_files_get_auto_start_on() {
        let s = FILE.parse(r#"{"enabled":true,"port":9000}"#).unwrap();
        assert!(s.auto_start);
        assert!(s.openai_upstream.is_empty());
    }

    #[test]
    fn port_zero_is_rejected() {
        assert!(with(true, 0).validate().is_err());
        assert!(with(true, 1).validate().is_ok());
        assert!(with(false, 65535).validate().is_ok());
    }

    #[test]
    fn file_parses_with_missing_fields_and_rejects_port_zero() {
        assert_eq!(FILE.parse("{}").unwrap(), HeadroomSettings::default());
        assert!(FILE.parse(r#"{"enabled":true,"port":0}"#).is_err());
        let custom = FILE.parse(r#"{"enabled":true,"port":9000}"#).unwrap();
        assert_eq!(custom, with(true, 9000));
    }
}
