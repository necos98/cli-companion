//! Comandi Tauri dei Bot-agente. La validazione (pura) sta fuori dal lock; la
//! modifica passa da `agent_bots` e si salva in `bots.json` prima di essere
//! pubblicata. Ogni comando che cambia l'elenco restituisce le impostazioni salvate.

use super::agent::{AgentBot, AgentBotDraft, SkillRef};
use super::agent_bots;
use super::catalog::AgentCatalog;
use super::engineering_standard::{prepare_standard_file, StandardFile};
use super::launch_refs;
use super::launches::{self, BotLaunch, BotTabLaunch, BotTabRestore};
use super::skills::{selectable_skills, BotSkillOption};
use super::validate::validate_draft;
use super::{update, BotSettings, BotSettingsStore};
use crate::agents::registry;
use crate::hub::settings::HubSettingsStore;
use crate::store::data_dir;
use crate::workspaces::{Workspace, WorkspaceStore};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn list_agent_bots(state: State<BotSettingsStore>) -> Vec<AgentBot> {
    state.read(|s| s.agents.clone())
}

#[tauri::command]
pub async fn create_agent_bot(app: AppHandle, draft: AgentBotDraft) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let draft = validate_draft(draft, &AgentCatalog::current())?;
        let taken = secretary_names(&app);
        update(&app, |s| agent_bots::create(&mut s.agents, draft, &taken).map(drop))
    })
    .await
}

#[tauri::command]
pub async fn update_agent_bot(app: AppHandle, id: String, draft: AgentBotDraft) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let draft = validate_draft(draft, &AgentCatalog::current())?;
        let taken = secretary_names(&app);
        update(&app, |s| agent_bots::update(&mut s.agents, &id, draft, &taken))
    })
    .await
}

/// Rifiuta il Bot `general`. Gli avvii predefiniti (workspace e globale) che
/// puntavano al Bot si azzerano, cosi' non resta una voce appesa.
#[tauri::command]
pub async fn delete_agent_bot(app: AppHandle, id: String) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let saved = update(&app, |s| agent_bots::delete(&mut s.agents, &id))?;
        launch_refs::clear_for_deleted_bot(&app, &id)?;
        Ok(saved)
    })
    .await
}

/// Riporta compito, modello, effort e skill del Bot al suo predefinito.
#[tauri::command]
pub async fn restore_agent_bot_preset(app: AppHandle, id: String) -> Result<BotSettings, String> {
    crate::blocking(move || update(&app, |s| agent_bots::restore_preset(&mut s.agents, &id))).await
}

/// Ricrea i Bot predefiniti che mancano.
#[tauri::command]
pub async fn restore_agent_bot_presets(app: AppHandle) -> Result<BotSettings, String> {
    crate::blocking(move || {
        let taken = secretary_names(&app);
        update(&app, |s| {
            agent_bots::restore_presets(&mut s.agents, &taken);
            Ok(())
        })
    })
    .await
}

/// Le skill che si possono dare a un Bot: del plugin e dell'utente.
#[tauri::command]
pub async fn list_bot_skills(app: AppHandle) -> Result<Vec<BotSkillOption>, String> {
    crate::blocking(move || {
        let store = crate::user_skills::store_dir(&data_dir(&app)?);
        Ok(selectable_skills(crate::user_skills::list(&store)?))
    })
    .await
}

/// I nomi dei segretari dell'HUB, in minuscolo: un Bot non li puo' prendere.
pub(crate) fn secretary_names(app: &AppHandle) -> Vec<String> {
    app.state::<HubSettingsStore>().read(|s| s.secretaries.iter().map(|sec| sec.name.to_lowercase()).collect())
}

/// L'agente del workspace: quello scelto o il predefinito globale. Se e'
/// scelto ma disabilitato non c'e' ripiego: il Bot risulta bloccato.
fn workspace_agent_id(workspace_agent: Option<String>) -> String {
    workspace_agent.unwrap_or_else(|| registry::default_agent().id().to_string())
}

/// Le voci del menu di avvio per i Bot abilitati, per un workspace con quell'agente
/// (`None` = senza un agente proprio). Economico, nessun processo.
#[tauri::command]
pub fn bot_launches(state: State<BotSettingsStore>, workspace_agent: Option<String>) -> Vec<BotLaunch> {
    let agent = workspace_agent_id(workspace_agent);
    state.read(|s| launches::bot_launches(&s.agents, &agent, &AgentCatalog::current()))
}

/// Prepara il lancio di un Bot nel tab `tab_id` che il frontend sta per aprire:
/// le variabili d'ambiente dell'agente vengono messe da parte per `pty_spawn`. Un Bot
/// con `work` riceve lo standard di ingegneria interattivo (vedi `engineering_standard`).
#[tauri::command]
pub async fn bot_interactive_launch(
    app: AppHandle,
    workspace_id: String,
    bot_id: String,
    tab_id: String,
) -> Result<BotTabLaunch, String> {
    crate::blocking(move || {
        if !crate::ipc::is_valid_tab_id(&tab_id) {
            return Err("Identificativo del tab non valido.".to_string());
        }
        let ws = app.state::<WorkspaceStore>().find(&workspace_id).ok_or("Workspace non trovato.")?;
        let bot = app
            .state::<BotSettingsStore>()
            .read(|s| s.agents.iter().find(|b| b.id == bot_id).cloned())
            .ok_or("Bot non trovato.")?;
        // Il disco serve solo se il Bot ha una skill dell'utente.
        let user_skills = match &bot.skill {
            Some(SkillRef::User(_)) => crate::user_skills::list(&crate::user_skills::store_dir(&data_dir(&app)?))?,
            _ => Vec::new(),
        };
        let standard = standard_file(&app, &ws);
        let (mut launch, mut env) = launches::build_bot_launch(
            &bot,
            &workspace_agent_id(ws.agent),
            &AgentCatalog::current(),
            |name| user_skills.iter().any(|s| s.name == name),
            &standard,
        )?;
        let headroom = crate::headroom::launch_env(&launch.agent_id, ws.kind, crate::headroom::Patience::NoWait);
        crate::headroom::merge_env(&mut env, &headroom);
        launch.warnings.extend(headroom.warning);
        if !env.is_empty() {
            crate::pty::stage_env(&app, &tab_id, env);
        }
        Ok(launch)
    })
    .await
}

/// Prepara l'ambiente del tab Bot `tab_id` ripristinato dopo un riavvio (per
/// `pty_spawn`, come all'avvio) senza ricaricare la skill. Se il Bot e' sparito
/// o cambiato il tab riparte comunque e l'esito porta l'avviso per l'utente.
#[tauri::command]
pub async fn bot_restore_env(
    app: AppHandle,
    workspace_id: String,
    bot_id: String,
    agent_id: String,
    tab_id: String,
) -> Result<BotTabRestore, String> {
    crate::blocking(move || {
        if !crate::ipc::is_valid_tab_id(&tab_id) {
            return Err("Identificativo del tab non valido.".to_string());
        }
        let ws = app.state::<WorkspaceStore>().find(&workspace_id).ok_or("Workspace non trovato.")?;
        let bot = app.state::<BotSettingsStore>().read(|s| s.agents.iter().find(|b| b.id == bot_id).cloned());
        // Sempre, anche se il Bot e' cambiato: la riga salvata di un agente che porta
        // il file nel comando lo nomina e la CLI rifiuta un file che non c'e'.
        let standard = standard_file(&app, &ws);
        let (mut env, mut warning) = launches::build_bot_restore(
            bot.as_ref(),
            &agent_id,
            &workspace_agent_id(ws.agent),
            &AgentCatalog::current(),
            &standard,
        );
        let headroom = crate::headroom::launch_env(&agent_id, ws.kind, crate::headroom::Patience::NoWait);
        crate::headroom::merge_env(&mut env, &headroom);
        if let Some(text) = headroom.warning {
            warning = Some(warning.map_or_else(|| text.clone(), |w| format!("{w} {text}")));
        }
        if !env.is_empty() {
            crate::pty::stage_env(&app, &tab_id, env);
        }
        Ok(BotTabRestore { warning })
    })
    .await
}

/// Il file dello standard interattivo per i tab del workspace `ws`, scritto
/// nella cartella dati dell'app; un errore diventa l'avviso del lancio.
fn standard_file(app: &AppHandle, ws: &Workspace) -> StandardFile {
    prepare_standard_file(&data_dir(app)?, ws.kind)
}
