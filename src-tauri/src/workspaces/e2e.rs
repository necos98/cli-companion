//! Configurazione E2E di un repository: comando, report JUnit, timeout
//! (vedi docs/BOARD.MD, "Test E2E").

use super::{MAX_REPO_COMMAND_LEN, MAX_REPO_PATH_LEN};
use serde::{Deserialize, Serialize};

/// Timeout predefinito e limiti, in minuti.
pub const DEFAULT_E2E_TIMEOUT_MIN: u32 = 20;
pub const MIN_E2E_TIMEOUT_MIN: u32 = 1;
pub const MAX_E2E_TIMEOUT_MIN: u32 = 60;

fn default_timeout_min() -> u32 {
    DEFAULT_E2E_TIMEOUT_MIN
}

/// Come l'app esegue gli E2E di un repository: `command` nella cartella del
/// task, poi legge il report JUnit in `junit` (relativo alla cartella).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E2eConfig {
    pub command: String,
    pub junit: String,
    #[serde(default = "default_timeout_min")]
    pub timeout_min: u32,
}

impl E2eConfig {
    /// Normalizza e valida: comando su una riga non vuoto, percorso JUnit
    /// relativo senza `..`, timeout nei limiti.
    pub fn normalized(self) -> Result<E2eConfig, String> {
        let command = self.command.trim();
        if command.is_empty() {
            return Err("comando E2E mancante".to_string());
        }
        if command.chars().any(|c| c.is_control() && c != '\t') {
            return Err("il comando E2E deve stare su una riga".to_string());
        }
        if command.chars().count() > MAX_REPO_COMMAND_LEN {
            return Err(format!("comando E2E troppo lungo (max {MAX_REPO_COMMAND_LEN} caratteri)"));
        }
        if !(MIN_E2E_TIMEOUT_MIN..=MAX_E2E_TIMEOUT_MIN).contains(&self.timeout_min) {
            return Err(format!(
                "timeout E2E non valido (da {MIN_E2E_TIMEOUT_MIN} a {MAX_E2E_TIMEOUT_MIN} minuti)"
            ));
        }
        Ok(E2eConfig { command: command.to_string(), junit: normalize_junit(&self.junit)?, timeout_min: self.timeout_min })
    }
}

/// Percorso del report: relativo, `/` come separatore, senza `..`.
fn normalize_junit(raw: &str) -> Result<String, String> {
    let t = raw.trim().replace('\\', "/");
    if t.is_empty() {
        return Err("percorso JUnit E2E mancante".to_string());
    }
    let b = t.as_bytes();
    if t.starts_with('/') || (b.len() >= 2 && b[1] == b':') {
        return Err("il percorso JUnit E2E deve essere relativo al repository".to_string());
    }
    if t.chars().count() > MAX_REPO_PATH_LEN || t.chars().any(char::is_control) {
        return Err(format!("percorso JUnit E2E non valido (max {MAX_REPO_PATH_LEN} caratteri)"));
    }
    let segs: Vec<&str> = t.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segs.is_empty() || segs.contains(&"..") {
        return Err("percorso JUnit E2E non valido (niente '..')".to_string());
    }
    Ok(segs.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(command: &str, junit: &str, timeout_min: u32) -> E2eConfig {
        E2eConfig { command: command.into(), junit: junit.into(), timeout_min }
    }

    #[test]
    fn is_normalized() {
        let c = cfg("  npm run e2e ", " .\\reports\\junit.xml ", 5).normalized().unwrap();
        assert_eq!(c, cfg("npm run e2e", "reports/junit.xml", 5));
    }

    #[test]
    fn timeout_defaults_when_missing() {
        let c: E2eConfig = serde_json::from_str(r#"{"command":"x","junit":"j.xml"}"#).unwrap();
        assert_eq!(c.timeout_min, DEFAULT_E2E_TIMEOUT_MIN);
        assert!(serde_json::from_str::<E2eConfig>(r#"{"command":"x","junit":"j","extra":1}"#).is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(cfg("  ", "j.xml", 5).normalized().is_err());
        assert!(cfg("a\nb", "j.xml", 5).normalized().is_err());
        assert!(cfg(&"x".repeat(MAX_REPO_COMMAND_LEN + 1), "j.xml", 5).normalized().is_err());
        assert!(cfg(&"x".repeat(MAX_REPO_COMMAND_LEN), "j.xml", 5).normalized().is_ok());
        for j in ["", "  ", "../j.xml", "a/../../j", "/etc/j", "C:\\j.xml", "C:/j", "..", ".", "a\0b"] {
            assert!(cfg("x", j, 5).normalized().is_err(), "{j}");
        }
        assert!(cfg("x", &"a".repeat(MAX_REPO_PATH_LEN + 1), 5).normalized().is_err());
        assert!(cfg("x", &"a".repeat(MAX_REPO_PATH_LEN), 5).normalized().is_ok());
        for t in [0, MAX_E2E_TIMEOUT_MIN + 1] {
            assert!(cfg("x", "j.xml", t).normalized().is_err(), "{t}");
        }
        for t in [MIN_E2E_TIMEOUT_MIN, MAX_E2E_TIMEOUT_MIN] {
            assert!(cfg("x", "j.xml", t).normalized().is_ok(), "{t}");
        }
    }
}
