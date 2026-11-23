//! Integrazione con Headroom (proxy locale di compressione del contesto):
//! le sessioni di Claude Code su Windows possono passare dal proxy, che l'utente
//! avvia per conto suo (l'app non lo avvia ne' lo ferma). Il tramite e' la
//! variabile `ANTHROPIC_BASE_URL`, messa nell'ambiente del terminale al lancio.
//!
//! - `settings`: impostazioni globali (`headroom.json`);
//! - `launch`: la decisione se e come instradare una sessione;
//! - `health`: raggiungibilita' del proxy, con cache breve;
//! - `commands`: comandi Tauri.

pub mod commands;
mod health;
mod launch;
mod settings;

pub use launch::{launch_env, merge_env};
pub use settings::load_from_disk;
