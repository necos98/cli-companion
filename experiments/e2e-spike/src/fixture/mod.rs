//! Sito di prova (fixture) per i test E2E: registrazione, login, impostazioni.
//! Stato in memoria, HTTP/1.1 minimale su `std::net`, asset incorporati.

mod api;
mod http;
mod layout;
mod pages;
mod server;
mod session;
pub mod state;
mod validate;

pub use server::FixtureServer;

pub const DUPLICATE_EMAIL_ERROR: &str = "This email is already registered.";
pub const DISPLAY_NAME_SAVED: &str = "Display name updated.";

pub fn serve_forever(port: u16) -> Result<(), String> {
    server::serve_forever(port)
}

#[cfg(test)]
mod integration_tests;
