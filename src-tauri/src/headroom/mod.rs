//! Integrazione con Headroom (proxy locale di compressione del contesto):
//! le sessioni di Claude Code su Windows possono passare dal proxy. Con
//! `auto_start` (predefinito) l'app lo avvia da sola e lo ferma all'uscita o alla
//! disattivazione; un proxy avviato da altri non viene mai toccato. Il tramite e'
//! la variabile `ANTHROPIC_BASE_URL`, messa nell'ambiente del terminale al lancio.
//!
//! - `settings`: impostazioni globali (`headroom.json`);
//! - `launch`: la decisione se e come instradare una sessione;
//! - `process`: il processo del proxy avviato dall'app;
//! - `health`: raggiungibilita' del proxy, con cache breve;
//! - `commands`: comandi Tauri.

pub mod commands;
mod health;
mod launch;
mod process;
mod settings;

pub use commands::start_if_auto;
pub use launch::{launch_env, merge_env, Patience};
pub use process::stop_owned;
pub use settings::load_from_disk;
