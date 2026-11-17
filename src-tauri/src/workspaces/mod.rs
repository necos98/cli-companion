//! I workspace: cartelle di progetto (Windows o WSL) con le loro impostazioni.
//!
//! - `model`: il workspace e i suoi avvii; `worker_settings`: le impostazioni dei
//!   worker, per agente dove dipendono dall'agente;
//! - `delivery`, `repo_config`, `e2e`: configurazione di consegna e dei repository;
//! - `validate`: validazione dei valori (errori tipizzati, vedi `crate::validate`);
//! - `store`, `migrations`: stato in memoria e persistenza su `workspaces.json`;
//! - `commands`, `launches`, `explorer`: comandi Tauri;
//! - `location`: posizione del workspace (verifica, duplicati, cambio di cartella);
//! - `settings_patch`: modifiche proposte dagli agenti;
//! - `target`: dove girano i comandi del workspace.

mod commands;
mod delivery;
mod e2e;
mod explorer;
mod launches;
mod location;
mod migrations;
mod model;
mod repo_config;
mod settings_patch;
mod store;
mod target;
mod validate;
mod worker_settings;

pub use commands::*;
pub use delivery::{Delivery, DeliveryMode, MergeMethod};
pub use e2e::{E2eConfig, MAX_E2E_TIMEOUT_MIN, MIN_E2E_TIMEOUT_MIN};
pub use explorer::*;
pub use launches::*;
pub use location::*;
pub(crate) use migrations::take_legacy_worker_fields;
#[cfg(test)]
pub(crate) use model::Launch;
pub use model::Workspace;
pub use repo_config::RepoConfig;
pub use settings_patch::{apply_settings_patch, SettingsPatch, MAX_PATCH_AGENTS, MAX_PATCH_LAUNCHES, MAX_PATCH_REPOS};
pub use store::{load_from_disk, WorkspaceStore};
pub use validate::{
    MAX_COMMAND_LEN, MAX_LAUNCH_NAME_LEN, MAX_REPO_COMMANDS, MAX_REPO_COMMAND_LEN, MAX_REPO_PATH_LEN, MAX_RULES_LEN,
};
#[cfg(test)]
pub(crate) use worker_settings::AgentWorkerSettings;
