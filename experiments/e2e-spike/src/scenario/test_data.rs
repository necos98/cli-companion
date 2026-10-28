//! Dati di test generati dall'harness per ogni esecuzione.

use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const NAMES: [&str; 6] = [
    "Dana Rivera",
    "Luca Ferri",
    "Mira Okafor",
    "Tomas Berg",
    "Ines Duarte",
    "Kofi Mensah",
];
const EMAIL_DOMAIN: &str = "example.test";
const PASSWORD_HEX_BYTES: usize = 4;

pub const KEY_NAME: &str = "name";
pub const KEY_EMAIL: &str = "email";
pub const KEY_PASSWORD: &str = "password";

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct TestData {
    entries: Vec<(String, String)>,
}

impl TestData {
    /// `run` parte da 1. L'email e' unica (millisecondi) e sempre minuscola.
    pub fn generate(config: &str, scenario: &str, run: u32, extras: &[(&str, &str)]) -> Self {
        let name = NAMES[(run.max(1) as usize - 1) % NAMES.len()];
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let email = format!(
            "{}.{}.{}.{run}.{}@{EMAIL_DOMAIN}",
            slug(name, '.'),
            slug(config, '-'),
            slug(scenario, '-'),
            now.as_millis()
        );
        let mut entries = vec![
            (KEY_NAME.to_string(), name.to_string()),
            (KEY_EMAIL.to_string(), email),
            (
                KEY_PASSWORD.to_string(),
                password(config, scenario, run, now.as_nanos()),
            ),
        ];
        entries.extend(extras.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        TestData { entries }
    }

    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    pub fn get(&self, key: &str) -> &str {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    }

    /// Sostituisce `{chiave}` con il valore corrispondente.
    pub fn render(&self, template: &str) -> String {
        self.entries
            .iter()
            .fold(template.to_string(), |text, (k, v)| {
                text.replace(&format!("{{{k}}}"), v)
            })
    }
}

/// Minuscolo, solo `[a-z0-9]` e `separator`.
pub fn slug(text: &str, separator: char) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(separator) {
            out.push(separator);
        }
    }
    out.trim_matches(separator).to_string()
}

fn password(config: &str, scenario: &str, run: u32, nanos: u128) -> String {
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = Sha256::digest(format!("{config}/{scenario}/{run}/{nanos}/{counter}"));
    let hex: String = digest
        .iter()
        .take(PASSWORD_HEX_BYTES)
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("Spike-{hex}-pw")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_lowercase_valid_and_encodes_context() {
        let data = TestData::generate("q4_k_m", "S1-it", 3, &[]);
        let email = data.get(KEY_EMAIL);
        assert!(email.ends_with("@example.test"));
        assert_eq!(email, email.to_lowercase());
        assert!(email.starts_with("mira.okafor.q4-k-m.s1-it.3."), "{email}");
        assert!(email
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-@".contains(c)));
    }

    #[test]
    fn password_has_expected_shape_and_differs_per_call() {
        let a = TestData::generate("base", "S1", 1, &[]);
        let b = TestData::generate("base", "S1", 1, &[]);
        let password = a.get(KEY_PASSWORD);
        assert!(password.len() >= 8);
        assert!(password.starts_with("Spike-") && password.ends_with("-pw"));
        assert_ne!(password, b.get(KEY_PASSWORD));
    }

    #[test]
    fn extras_are_appended_and_rendered() {
        let data = TestData::generate("base", "S3", 1, &[("new_display_name", "Morgan Lee")]);
        assert_eq!(data.entries().len(), 4);
        assert_eq!(
            data.render("Hi {new_display_name} / {name}"),
            "Hi Morgan Lee / Dana Rivera"
        );
    }

    #[test]
    fn names_rotate_with_the_run_number() {
        let first = TestData::generate("base", "S1", 1, &[]);
        let wrapped = TestData::generate("base", "S1", 7, &[]);
        assert_eq!(first.get(KEY_NAME), wrapped.get(KEY_NAME));
    }

    #[test]
    fn slug_collapses_separators() {
        assert_eq!(slug("Q4_K M", '-'), "q4-k-m");
        assert_eq!(slug("--x--", '-'), "x");
    }
}
