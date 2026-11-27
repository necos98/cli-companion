//! Comandi Tauri delle impostazioni dell'HUB. Validano gli argomenti per primi,
//! fanno l'I/O in `crate::blocking` e ritornano le impostazioni salvate con la
//! sola presenza della API key (`HubSettingsView`). Funzionano anche con l'HUB
//! spento: la pagina delle impostazioni le mostra e le modifica.

use super::language::Language;
use super::presets::{self, SecretaryPresetView};
use super::provider::Provider;
use super::secretary::SecretaryDraft;
use super::session::SessionSettings;
use super::{check_request, secretaries, HubSettings, HubSettingsStore, HubSettingsView, HUB_AVAILABLE};
use crate::bots::validate_image;
use crate::hub::api_key::ApiKey;
use crate::hub::engine::SecretaryEngine;
use crate::hub::events::HubFeed;
use crate::hub::lifecycle::HubLifecycleListeners;
use crate::hub::notify::HubNotifier;
use crate::store::data_dir;
use crate::workspaces::WorkspaceStore;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn hub_get_settings(app: AppHandle) -> Result<HubSettingsView, String> {
    crate::blocking(move || {
        let settings = app.state::<HubSettingsStore>().get();
        Ok(view(&app, settings))
    })
    .await
}

/// Spegnere l'HUB butta le istantanee delle board e chiude le sessioni dei
/// segretari, annullando i lavori in corso. Accenderlo (solo il
/// passaggio da spento ad acceso) fissa subito come base la vista corrente di
/// ogni board, senza messaggi ne' scritture: gia' la prima modifica successiva
/// produce i suoi eventi.
#[tauri::command]
pub async fn hub_set_enabled(app: AppHandle, enabled: bool) -> Result<HubSettingsView, String> {
    crate::blocking(move || {
        check_request(HUB_AVAILABLE, enabled)?;
        if !HUB_AVAILABLE {
            // Spegnere un HUB non disponibile e' innocuo: il file salvato non si tocca.
            return Ok(view(&app, app.state::<HubSettingsStore>().get()));
        }
        let mut was_enabled = false;
        let saved = save(&app, |s| {
            was_enabled = s.enabled;
            if enabled && !was_enabled {
                secretaries::seed_presets(&mut s.secretaries);
            }
            s.enabled = enabled;
            Ok(())
        })?;
        let feed = app.state::<HubFeed>();
        match (was_enabled, saved.enabled) {
            (false, true) => feed.baseline(&app),
            (_, false) => {
                feed.reset();
                app.state::<HubNotifier>().discard();
                app.state::<SecretaryEngine>().shutdown(&app);
            }
            (true, true) => {}
        }
        app.state::<HubLifecycleListeners>().enabled_changed(&app, was_enabled, saved.enabled);
        Ok(view(&app, saved))
    })
    .await
}

/// Indirizzo e modello del provider; vuoti = non configurato.
#[tauri::command]
pub async fn hub_set_provider(app: AppHandle, base_url: String, model: String) -> Result<HubSettingsView, String> {
    let provider = Provider::new(&base_url, &model)?;
    save_and_view(app, move |s| {
        s.provider = provider;
        Ok(())
    })
    .await
}

/// Salva la API key nel portachiavi del sistema; non la restituisce mai.
#[tauri::command]
pub async fn hub_set_api_key(app: AppHandle, key: String) -> Result<HubSettingsView, String> {
    crate::blocking(move || {
        app.state::<ApiKey>().set(&key)?;
        Ok(view(&app, app.state::<HubSettingsStore>().get()))
    })
    .await
}

/// Toglie la API key dal portachiavi; nessun errore se non c'era.
#[tauri::command]
pub async fn hub_clear_api_key(app: AppHandle) -> Result<HubSettingsView, String> {
    crate::blocking(move || {
        app.state::<ApiKey>().clear()?;
        Ok(view(&app, app.state::<HubSettingsStore>().get()))
    })
    .await
}

/// Lingua delle risposte dei segretari (`it`, `en`, `es`, `fr`, `de`).
#[tauri::command]
pub async fn hub_set_language(app: AppHandle, language: String) -> Result<HubSettingsView, String> {
    let language = Language::parse(&language)?;
    save_and_view(app, move |s| {
        s.language = language;
        Ok(())
    })
    .await
}

/// Notifiche di sistema per i messaggi dei thread; contano con l'HUB acceso.
#[tauri::command]
pub async fn hub_set_notifications(app: AppHandle, enabled: bool) -> Result<HubSettingsView, String> {
    save_and_view(app, move |s| {
        s.notifications = enabled;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn hub_set_session(app: AppHandle, idle_minutes: u32, max_steps: u32) -> Result<HubSettingsView, String> {
    let session = SessionSettings::new(idle_minutes, max_steps)?;
    save_and_view(app, move |s| {
        s.session = session;
        Ok(())
    })
    .await
}

/// I segretari predefiniti, con i testi per l'interfaccia.
#[tauri::command]
pub fn hub_list_secretary_presets() -> Vec<SecretaryPresetView> {
    presets::views()
}

/// Nuovo segretario, senza immagine. `channels` `null` = tutti i canali;
/// `preset` = l'id del preset da cui nasce, se nasce da uno.
#[tauri::command]
pub async fn hub_create_secretary(
    app: AppHandle,
    name: String,
    system_prompt: String,
    tools: Vec<String>,
    channels: Option<Vec<String>>,
    preset: Option<String>,
) -> Result<HubSettingsView, String> {
    let draft = draft(&app, &name, &system_prompt, &tools, channels.as_deref())?;
    save_and_view(app, move |s| match preset {
        Some(id) => secretaries::create_from_preset(&mut s.secretaries, draft, &id),
        None => secretaries::create(&mut s.secretaries, draft),
    })
    .await
}

/// Riporta prompt e tool del segretario `name` a quelli del suo preset; nome,
/// immagine e canali restano.
#[tauri::command]
pub async fn hub_restore_secretary_preset(app: AppHandle, name: String) -> Result<HubSettingsView, String> {
    save_and_view(app, move |s| secretaries::restore_preset(&mut s.secretaries, &name)).await
}

/// Sostituisce i campi del segretario `name` (anche il nome: `new_name`);
/// l'immagine resta. Rifiutato se `new_name` e' di un altro segretario.
#[tauri::command]
pub async fn hub_update_secretary(
    app: AppHandle,
    name: String,
    new_name: String,
    system_prompt: String,
    tools: Vec<String>,
    channels: Option<Vec<String>>,
) -> Result<HubSettingsView, String> {
    let draft = draft(&app, &new_name, &system_prompt, &tools, channels.as_deref())?;
    save_and_view(app, move |s| secretaries::update(&mut s.secretaries, &name, draft)).await
}

#[tauri::command]
pub async fn hub_delete_secretary(app: AppHandle, name: String) -> Result<HubSettingsView, String> {
    save_and_view(app, move |s| secretaries::delete(&mut s.secretaries, &name)).await
}

/// Immagine pixel-art del segretario, validata come quelle dei bot.
#[tauri::command]
pub async fn hub_set_secretary_image(
    app: AppHandle,
    name: String,
    png: String,
    color: String,
) -> Result<HubSettingsView, String> {
    let image = validate_image(&png, &color)?;
    save_and_view(app, move |s| secretaries::set_image(&mut s.secretaries, &name, Some(image))).await
}

#[tauri::command]
pub async fn hub_remove_secretary_image(app: AppHandle, name: String) -> Result<HubSettingsView, String> {
    save_and_view(app, move |s| secretaries::set_image(&mut s.secretaries, &name, None)).await
}

/// Valida i campi di un segretario; i canali dei workspace devono esistere.
fn draft(
    app: &AppHandle,
    name: &str,
    system_prompt: &str,
    tools: &[String],
    channels: Option<&[String]>,
) -> Result<SecretaryDraft, String> {
    let workspaces = app.state::<WorkspaceStore>();
    SecretaryDraft::new(name, system_prompt, tools, channels, |id| workspaces.find(id).is_some())
}

/// Applica `f` fuori dal main thread, salva e ritorna la vista.
async fn save_and_view(
    app: AppHandle,
    f: impl FnOnce(&mut HubSettings) -> Result<(), String> + Send + 'static,
) -> Result<HubSettingsView, String> {
    crate::blocking(move || {
        let saved = save(&app, f)?;
        Ok(view(&app, saved))
    })
    .await
}

/// Applica `f` a una copia e la salva; se `f` o il salvataggio falliscono nulla
/// cambia.
fn save(app: &AppHandle, f: impl FnOnce(&mut HubSettings) -> Result<(), String>) -> Result<HubSettings, String> {
    let dir = data_dir(app)?;
    app.state::<HubSettingsStore>().update(&dir, f)
}

/// Le impostazioni con la presenza della API key. Se il portachiavi non
/// risponde la chiave risulta assente (l'errore e' nel log): le impostazioni
/// restano leggibili e la si puo' salvare di nuovo.
fn view(app: &AppHandle, settings: HubSettings) -> HubSettingsView {
    let api_key_set = app.state::<ApiKey>().is_set().unwrap_or_else(|e| {
        log::error!("hub: presenza della API key non verificabile: {e}");
        false
    });
    HubSettingsView::new(settings, api_key_set, HUB_AVAILABLE)
}
