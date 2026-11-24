//! Bot: ogni sessione dell'app e' un "bot" con un nome preso da una pool
//! configurabile dall'utente (puramente visivo). Persistenza in `bots.json`
//! tramite `store` (busta versionata, backup datati se il file e' corrotto).
//! La pool predefinita (`DEFAULT_NAMES`) e' l'unica fonte: il frontend la riceve
//! da `get_bot_settings`. Ogni nome della pool puo' avere un'immagine pixel-art
//! (PNG base64 + colore dominante), validata in `image` e salvata nello stesso file.
//!
//! Nello stesso file stanno i Bot-agente (`agents`): agenti specializzati e
//! globali, ciascuno con agente, modello, effort e skill. La pool dei nomi resta
//! per i terminali generici. Un file per responsabilita':
//! - `agent`: i tipi del Bot-agente;
//! - `presets`: i Bot predefiniti, `migrations`: i passi che li portano nei file esistenti;
//! - `validate`: la validazione dei campi, `catalog`: gli agenti che si possono usare;
//! - `agent_bots`: le operazioni pure sull'elenco, `commands`: i comandi Tauri;
//! - `resolve`: da un Bot ad agente, modello, effort e skill di un lancio;
//! - `launches`: le voci del menu di avvio e il lancio di un Bot in un tab;
//! - `engineering_standard`: lo standard di ingegneria dei Bot `work` avviati in un tab;
//! - `skills`: le skill selezionabili per l'interfaccia;
//! - `derived`: i Bot derivati da un predefinito con un agente fissato;
//! - `launch_refs`: gli avvii predefiniti da azzerare quando un Bot viene eliminato;
//! - `secretary_names`: all'avvio, un nome libero ai Bot che portano il nome di un segretario.

mod agent;
mod agent_bots;
mod catalog;
pub mod commands;
mod derived;
mod engineering_standard;
mod image;
mod launch_refs;
mod launches;
mod migrations;
mod presets;
pub mod resolve;
mod secretary_names;
mod skills;
mod validate;

pub use agent::{AgentBot, SkillRef};
pub use catalog::AgentCatalog;
pub(crate) use derived::derived_bot;
pub(crate) use image::validate_image;
pub use launches::{bot_launch_id, BOT_LAUNCH_PREFIX};
#[cfg(test)]
pub(crate) use presets::default_bots as default_agent_bots;
pub use secretary_names::rename_bots_named_like_secretaries;

use crate::store::{adopt_envelope, data_dir, Decoded, JsonFile, JsonStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;
use validate::normalize_name;

const NAMES_MAX: usize = 200;

const DEFAULT_NAMES: [&str; 40] = [
    "Bolt", "Pixel", "Gizmo", "Sprocket", "Widget", "Byte", "Nova", "Cog", "Rusty", "Chip", "Echo", "Flux", "Glitch",
    "Hex", "Ion", "Jolt", "Kilo", "Loop", "Mochi", "Nano", "Orbit", "Patch", "Quark", "Ratchet", "Servo", "Tesla",
    "Unit", "Volt", "Watt", "Zippy", "Blip", "Cosmo", "Dynamo", "Fizz", "Gadget", "Rivet", "Spark", "Tinker", "Turbo",
    "Zed",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BotSettings {
    /// Mostra le sessioni come bot. I file senza il campo valgono `true`.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Pool di nomi. Un file senza il campo vale la pool predefinita; una
    /// lista vuota e' ammessa (i bot si chiamano "Bot 1", "Bot 2"...).
    #[serde(default = "default_names")]
    pub names: Vec<String>,
    /// Immagini per nome (con la grafia della pool). I file senza il campo non
    /// hanno immagini; le chiavi che non sono nella pool vengono eliminate.
    #[serde(default)]
    pub images: BTreeMap<String, BotImage>,
    /// I Bot-agente: i predefiniti arrivano con le migrazioni; `general` c'e'
    /// sempre (lo rimette `decode_settings`).
    #[serde(default = "presets::default_bots")]
    pub agents: Vec<AgentBot>,
}

/// Immagine di un bot, gia' validata dal backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BotImage {
    /// PNG in base64 standard.
    pub png: String,
    /// Colore dominante, `#rrggbb` minuscolo.
    pub color: String,
}

fn default_enabled() -> bool {
    true
}

fn default_names() -> Vec<String> {
    DEFAULT_NAMES.iter().map(|n| n.to_string()).collect()
}

impl Default for BotSettings {
    fn default() -> Self {
        Self { enabled: true, names: default_names(), images: BTreeMap::new(), agents: presets::default_bots() }
    }
}

/// v1: la busta; v2: i Bot-agente predefiniti; v3: i tipi di task dei Bot, Reviewer e Planner;
/// v4: Quick Answer e Bug Fixer; v5: i `task` esclusivi di General e Planner;
/// v6: Researcher, Test Writer, Docs Writer e Refactorer; v7: Git Operator e Release, spenti.
const MIGRATIONS: [crate::store::Migration; 7] = [
    adopt_envelope,
    migrations::add_agent_bots,
    migrations::add_task_kinds,
    migrations::add_answer_and_fix_bots,
    migrations::exclusive_general_and_planner_tasks,
    migrations::add_research_and_work_bots,
    migrations::add_git_and_release_bots,
];

static SETTINGS_FILE: JsonFile<BotSettings> = JsonFile::new("bots.json", &MIGRATIONS).decoded_with(decode_settings);

/// Lettura intera, poi il Bot `general` rimesso se un file scritto a mano lo ha tolto.
fn decode_settings(data: Value) -> Result<Decoded<BotSettings>, String> {
    let mut value: BotSettings = serde_json::from_value(data).map_err(|e| format!("dati non validi: {e}"))?;
    presets::ensure_general(&mut value.agents);
    Ok(Decoded { value, dropped: Vec::new() })
}

pub type BotSettingsStore = JsonStore<BotSettings>;

pub fn new_store() -> BotSettingsStore {
    JsonStore::new(&SETTINGS_FILE)
}

/// Normalizza la pool: trim, 1-24 caratteri, niente caratteri di controllo,
/// niente duplicati (senza distinguere le maiuscole), al massimo 200 nomi.
fn validate_names(names: Vec<String>) -> Result<Vec<String>, String> {
    if names.len() > NAMES_MAX {
        return Err(format!("al massimo {NAMES_MAX} nomi"));
    }
    let mut clean: Vec<String> = Vec::with_capacity(names.len());
    for raw in names {
        let name = normalize_name(&raw)?;
        if clean.iter().any(|n| n.to_lowercase() == name.to_lowercase()) {
            return Err(format!("il nome “{name}” è già nell'elenco"));
        }
        clean.push(name);
    }
    Ok(clean)
}

pub fn load_from_disk(app: &tauri::App) {
    match data_dir(app.handle()) {
        Ok(dir) => app.state::<BotSettingsStore>().load(&dir),
        Err(e) => log::error!("cartella dati non disponibile, impostazioni dei bot non caricate: {e}"),
    }
}

/// Le voci di lancio dei Bot abilitati salvati (vedi `launches::enabled_launch_ids`).
pub fn enabled_launch_ids(app: &AppHandle) -> Vec<String> {
    app.state::<BotSettingsStore>().read(|s| launches::enabled_launch_ids(&s.agents))
}

/// I Bot-agente salvati, anche i disabilitati: chi li usa sceglie e verifica.
pub fn agent_bots(app: &AppHandle) -> Vec<AgentBot> {
    app.state::<BotSettingsStore>().read(|s| s.agents.clone())
}

#[tauri::command]
pub fn get_bot_settings(state: State<BotSettingsStore>) -> BotSettings {
    state.get()
}

#[tauri::command]
pub async fn set_bot_settings(app: AppHandle, enabled: bool, names: Vec<String>) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let names = validate_names(names)?;
        update(&app, |s| {
            s.enabled = enabled;
            s.names = names;
            prune_images(s);
            Ok(())
        })
    })
    .await
}

/// Imposta l'immagine di un nome della pool (la validazione sta fuori dal lock).
#[tauri::command]
pub async fn set_bot_image(app: AppHandle, name: String, png: String, color: String) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let img = validate_image(&png, &color)?;
        update(&app, |s| {
            let pool =
                pool_name(&s.names, &name).cloned().ok_or_else(|| format!("il nome “{name}” non è nell'elenco"))?;
            s.images.insert(pool, img);
            Ok(())
        })
    })
    .await
}

/// Toglie l'immagine di un nome. Nessun errore se non c'e' o se il nome non e'
/// nella pool: e' un no-op che restituisce le impostazioni correnti.
#[tauri::command]
pub async fn remove_bot_image(app: AppHandle, name: String) -> Result<BotSettings, String> {
    crate::blocking(move || {
        update(&app, |s| {
            if let Some(pool) = pool_name(&s.names, &name).cloned() {
                s.images.remove(&pool);
            }
            Ok(())
        })
    })
    .await
}

#[tauri::command]
pub async fn reset_bot_names(app: AppHandle) -> Result<BotSettings, String> {
    crate::blocking(move || {
        update(&app, |s| {
            restore_default_names(s);
            Ok(())
        })
    })
    .await
}

/// Rimette il pool di nomi predefinito; `enabled` resta com'e'. Restano solo le
/// immagini dei nomi che sono anche nella pool predefinita.
fn restore_default_names(s: &mut BotSettings) {
    s.names = default_names();
    prune_images(s);
}

/// Il nome nella pool che coincide con `name` senza distinguere le maiuscole.
fn pool_name<'a>(names: &'a [String], name: &str) -> Option<&'a String> {
    let wanted = name.trim().to_lowercase();
    names.iter().find(|n| n.to_lowercase() == wanted)
}

/// Tiene solo le immagini dei nomi nella pool, con la grafia della pool.
fn prune_images(s: &mut BotSettings) {
    let images = std::mem::take(&mut s.images);
    s.images = images
        .into_iter()
        .filter_map(|(name, img)| pool_name(&s.names, &name).map(|pool| (pool.clone(), img)))
        .collect();
}

/// Applica `f`, salva e solo poi aggiorna la memoria (se il salvataggio fallisce
/// lo stato resta invariato). Se `f` fallisce non salva nulla.
fn update(app: &AppHandle, f: impl FnOnce(&mut BotSettings) -> Result<(), String>) -> Result<BotSettings, String> {
    let dir = data_dir(app)?;
    app.state::<BotSettingsStore>().update(&dir, f)
}

#[cfg(test)]
mod tests;
