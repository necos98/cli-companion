//! Accesso allo stato condiviso e cookie di sessione.

use super::http::Request;
use super::state::Store;
use std::sync::{Mutex, MutexGuard, PoisonError};

const SESSION_COOKIE: &str = "session";

/// Lock dello stato; un lock avvelenato (panic in un altro thread) si recupera.
pub fn lock(store: &Mutex<Store>) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn session_id(req: &Request) -> Option<&str> {
    req.cookie(SESSION_COOKIE).filter(|id| !id.is_empty())
}

/// (nome, nome visualizzato) dell'utente collegato, se c'e' una sessione valida.
pub fn current_user(store: &Mutex<Store>, req: &Request) -> Option<(String, String)> {
    lock(store).session_names(session_id(req)?)
}

pub fn set_cookie_value(session_id: &str) -> String {
    format!("{SESSION_COOKIE}={session_id}; HttpOnly; SameSite=Strict; Path=/")
}

pub fn clear_cookie_value() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_http_only_and_strict() {
        let value = set_cookie_value("abc");
        assert!(value.starts_with("session=abc;"));
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("SameSite=Strict"));
        assert!(value.contains("Path=/"));
        assert!(clear_cookie_value().contains("Max-Age=0"));
    }
}
