//! Stato in memoria della fixture (account, sessioni, login) e sua vista JSON.

use crate::hash::sha256_hex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StateSnapshot {
    pub accounts: Vec<AccountView>,
    pub logins: Vec<String>,
    pub active_sessions: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct AccountView {
    pub name: String,
    pub email: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SeedAccount {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DuplicateEmail;

#[derive(Clone)]
struct Account {
    name: String,
    email: String,
    display_name: String,
    password_hash: String,
}

#[derive(Default)]
pub struct Store {
    accounts: Vec<Account>,
    /// id di sessione -> email dell'account.
    sessions: HashMap<String, String>,
    logins: Vec<String>,
    /// Contatore di processo per gli id di sessione: sopravvive al reset.
    session_counter: u64,
}

impl Store {
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            accounts: self
                .accounts
                .iter()
                .map(|a| AccountView {
                    name: a.name.clone(),
                    email: a.email.clone(),
                    display_name: a.display_name.clone(),
                })
                .collect(),
            logins: self.logins.clone(),
            active_sessions: self.sessions.len(),
        }
    }

    pub fn reset(&mut self) {
        self.accounts.clear();
        self.sessions.clear();
        self.logins.clear();
    }

    /// `email` deve essere gia' normalizzata.
    pub fn add_account(
        &mut self,
        name: &str,
        email: &str,
        password: &str,
    ) -> Result<(), DuplicateEmail> {
        if self.accounts.iter().any(|a| a.email == email) {
            return Err(DuplicateEmail);
        }
        self.accounts.push(Account {
            name: name.to_string(),
            email: email.to_string(),
            display_name: name.to_string(),
            password_hash: sha256_hex(password.as_bytes()),
        });
        Ok(())
    }

    /// Verifica le credenziali e, se corrette, registra il login.
    pub fn login(&mut self, email: &str, password: &str) -> bool {
        let hash = sha256_hex(password.as_bytes());
        let known = self
            .accounts
            .iter()
            .any(|a| a.email == email && a.password_hash == hash);
        if known {
            self.logins.push(email.to_string());
        }
        known
    }

    pub fn open_session(&mut self, email: &str) -> String {
        self.session_counter += 1;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let id = sha256_hex(format!("{}:{nanos}", self.session_counter).as_bytes());
        self.sessions.insert(id.clone(), email.to_string());
        id
    }

    pub fn close_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// (nome, nome visualizzato) dell'account della sessione.
    pub fn session_names(&self, session_id: &str) -> Option<(String, String)> {
        let email = self.sessions.get(session_id)?;
        self.accounts
            .iter()
            .find(|a| &a.email == email)
            .map(|a| (a.name.clone(), a.display_name.clone()))
    }

    /// `false` se la sessione non esiste.
    pub fn set_display_name(&mut self, session_id: &str, display_name: &str) -> bool {
        let Some(email) = self.sessions.get(session_id) else {
            return false;
        };
        match self.accounts.iter_mut().find(|a| &a.email == email) {
            Some(account) => {
                account.display_name = display_name.to_string();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_account() -> Store {
        let mut store = Store::default();
        store
            .add_account("Dana", "dana@example.test", "pw-12345678")
            .unwrap();
        store
    }

    #[test]
    fn duplicate_email_is_rejected() {
        let mut store = store_with_account();
        assert_eq!(
            store.add_account("Altra", "dana@example.test", "x"),
            Err(DuplicateEmail)
        );
        assert_eq!(store.snapshot().accounts.len(), 1);
    }

    #[test]
    fn only_successful_logins_are_recorded() {
        let mut store = store_with_account();
        assert!(!store.login("dana@example.test", "sbagliata"));
        assert!(!store.login("nessuno@example.test", "pw-12345678"));
        assert!(store.login("dana@example.test", "pw-12345678"));
        assert_eq!(store.snapshot().logins, vec!["dana@example.test"]);
    }

    #[test]
    fn sessions_open_close_and_rename() {
        let mut store = store_with_account();
        let sid = store.open_session("dana@example.test");
        assert_eq!(store.snapshot().active_sessions, 1);
        assert!(store.set_display_name(&sid, "Morgan"));
        assert_eq!(
            store.session_names(&sid),
            Some(("Dana".to_string(), "Morgan".to_string()))
        );
        store.close_session(&sid);
        assert_eq!(store.session_names(&sid), None);
        assert!(!store.set_display_name(&sid, "X"));
    }

    #[test]
    fn session_ids_are_unique_and_survive_reset() {
        let mut store = store_with_account();
        let first = store.open_session("dana@example.test");
        store.reset();
        assert_eq!(store.snapshot().active_sessions, 0);
        assert!(store.snapshot().accounts.is_empty());
        assert_ne!(first, store.open_session("x"));
    }

    #[test]
    fn password_is_stored_hashed() {
        let store = store_with_account();
        assert_ne!(store.accounts[0].password_hash, "pw-12345678");
        assert_eq!(store.accounts[0].password_hash.len(), 64);
    }
}
