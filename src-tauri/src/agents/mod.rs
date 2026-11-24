//! Integrazioni dei coding agent. Il core non sa come funziona un agente: offre
//! dei contratti (`contracts/`, uno per capacita') e ogni integrazione
//! implementa quelli che le servono.
//!
//! Aggiungere un agente:
//! 1. una cartella `agents/<nome>/` con una struct che implementa
//!    `AgentIntegration` (id, nome, rilevamento, comando interattivo) e i
//!    contratti che le servono, dichiarati TUTTI (presenti o `None`) in
//!    `capabilities()` (vedi `claude_code/`);
//! 2. una riga in `INTEGRATIONS` di `registry.rs` e una in quello di
//!    `architecture.rs`.
//!
//! Comandi Tauri e UI (Impostazioni -> Coding agent) sono generici e non vanno
//! toccati; `conformance.rs` verifica da solo ogni integrazione registrata e
//! `architecture.rs` che il nome di un agente non compaia fuori dalla sua
//! cartella. Guida completa: `docs/wiki/integrare-un-coding-agent.md`.
//!
//! Quali integrazioni sono abilitate e quale e' l'agente predefinito lo sceglie
//! l'utente (`integrations.json`, vedi `integrations_store.rs`): aggiungere un
//! agente non cambia nulla in proposito, finche' non lo abilita vale solo la
//! prima del registro. L'agente di un uso si sceglie sempre con
//! `registry::resolve_agent`.

#[cfg(test)]
pub(crate) mod architecture;
mod claude_code;
mod commands;
#[cfg(test)]
mod conformance;
mod contracts;
mod detection;
mod enablement;
mod env_status;
mod integrations_commands;
mod integrations_store;
mod launches;
mod models;
mod opencode;
mod package;
mod pi_agent;
pub mod presets;
mod readiness;
pub mod registry;
mod resolve;
#[cfg(test)]
pub(crate) mod test_registry;
#[cfg(test)]
pub(crate) mod testing;
mod worker_rules;
mod worker_skill;

pub use commands::*;
pub use contracts::*;
pub use integrations_commands::*;
pub use integrations_store::load_integrations;
pub use launches::{builtin_launch_list, BuiltinLaunch};
pub use models::{effort_is_supported, model_is_valid, tier_models, unsupported_effort_warning, TierModels};
pub use readiness::{agent_readiness, Readiness};
pub use registry::automation_command;
pub use resolve::{AgentUnavailable, AgentUse};
pub use worker_rules::{validate_worker_rules, worker_permissions_file, worker_rule_format};
pub use worker_skill::worker_skill_invocation;
/// Per i test delle scelte con piu' agenti: nucleo puro e stato che prende.
#[cfg(test)]
pub(crate) use {enablement::IntegrationsState, resolve::resolve_in};
