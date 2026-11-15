//! I Bot nel menu di avvio e il lancio di un Bot in un tab. Puro (nessun I/O):
//! l'agente del workspace, il catalogo e l'esistenza delle skill utente arrivano
//! dal chiamante. Il comando lo costruisce il contratto `InteractiveLauncher`
//! dell'agente effettivo; qui non c'e' nulla di proprio di un agente.

use super::agent::{AgentBot, ModelChoice, SkillRef};
use super::catalog::AgentCatalog;
use super::engineering_standard::{BotStandard, StandardFile};
use super::resolve::{resolve_bot, ResolvedBot};
use crate::agents::{
    first_invalid_env_name, AgentIntegration, AgentUnavailable, InteractiveLauncher, InteractiveRequest,
};
use serde::Serialize;
use ts_rs::TS;

/// Prefisso degli id delle voci del menu di avvio che aprono un Bot.
pub const BOT_LAUNCH_PREFIX: &str = "bot:";

/// La voce di lancio del Bot `bot_id`: `bot:<id>`.
pub fn bot_launch_id(bot_id: &str) -> String {
    format!("{BOT_LAUNCH_PREFIX}{bot_id}")
}

/// Le voci dei Bot abilitati: i valori che un avvio predefinito puo' prendere
/// oltre a quelli integrati e del workspace.
pub fn enabled_launch_ids(bots: &[AgentBot]) -> Vec<String> {
    bots.iter().filter(|b| b.enabled).map(|b| bot_launch_id(&b.id)).collect()
}

/// Voce del menu di avvio per un Bot abilitato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BotLaunch {
    /// `bot:<id del Bot>`.
    pub id: String,
    pub bot_id: String,
    pub name: String,
    /// L'agente effettivo; `None` se non si e' potuto risolvere.
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    /// Perche' non si puo' avviare (in italiano); `None` = avviabile.
    pub blocked_reason: Option<String>,
}

/// Lancio pronto di un Bot in un tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BotTabLaunch {
    pub name: String,
    pub agent_id: String,
    pub command: String,
    /// Salvato nella sessione per il ripristino: senza la skill.
    pub restore_command: String,
    /// Cio' che l'agente non sa applicare (in inglese, da `resolve_bot`).
    pub warnings: Vec<String>,
}

/// Perche' l'agente non si puo' usare, per l'utente.
fn unavailable_message(error: &AgentUnavailable) -> String {
    match error {
        AgentUnavailable::Unknown(id) => format!("L'agente «{id}» del Bot non è registrato."),
        AgentUnavailable::Disabled { name, .. } => format!("{name} è disabilitato: abilitalo nelle Integrazioni."),
        AgentUnavailable::MissingContract { name, .. } => unsupported_message(name),
    }
}

fn unsupported_message(agent_name: &str) -> String {
    format!("{agent_name} non supporta l'avvio dei Bot.")
}

/// Il Bot risolto insieme all'integrazione effettiva.
fn resolve<'a>(
    bot: &AgentBot,
    workspace_agent: &str,
    catalog: &AgentCatalog<'a>,
) -> Result<(&'a dyn AgentIntegration, ResolvedBot), AgentUnavailable> {
    let resolved = resolve_bot(bot, workspace_agent, catalog)?;
    Ok((catalog.find(&resolved.agent_id)?, resolved))
}

/// Una voce per ogni Bot abilitato, nell'ordine dell'elenco. La prontezza
/// dell'agente nell'ambiente la valuta il frontend dall'`agent_id`.
pub fn bot_launches(bots: &[AgentBot], workspace_agent: &str, catalog: &AgentCatalog) -> Vec<BotLaunch> {
    bots.iter().filter(|bot| bot.enabled).map(|bot| bot_launch(bot, workspace_agent, catalog)).collect()
}

fn bot_launch(bot: &AgentBot, workspace_agent: &str, catalog: &AgentCatalog) -> BotLaunch {
    let entry = BotLaunch {
        id: bot_launch_id(&bot.id),
        bot_id: bot.id.clone(),
        name: bot.name.clone(),
        agent_id: None,
        agent_name: None,
        blocked_reason: None,
    };
    match resolve(bot, workspace_agent, catalog) {
        Err(error) => BotLaunch { blocked_reason: Some(unavailable_message(&error)), ..entry },
        Ok((agent, resolved)) => BotLaunch {
            agent_id: Some(resolved.agent_id),
            agent_name: Some(agent.display_name().to_string()),
            blocked_reason: agent
                .capabilities()
                .interactive_launch
                .is_none()
                .then(|| unsupported_message(agent.display_name())),
            ..entry
        },
    }
}

/// Il lancio del Bot e le variabili d'ambiente da preparare per il tab prima
/// che parta. `user_skill_exists` dice se una skill utente c'e' ancora: se manca
/// il lancio fallisce, senza ripiegare su un Bot senza skill. `standard_file` e' il
/// file dello standard di ingegneria interattivo, che riceve un Bot con `work`: se
/// l'agente non lo sa applicare o il file manca, il Bot parte senza e con un avviso.
pub fn build_bot_launch(
    bot: &AgentBot,
    workspace_agent: &str,
    catalog: &AgentCatalog,
    user_skill_exists: impl Fn(&str) -> bool,
    standard_file: &StandardFile,
) -> Result<(BotTabLaunch, Vec<(String, String)>), String> {
    if !bot.enabled {
        return Err(format!("Il Bot «{}» è disabilitato.", bot.name));
    }
    let (agent, resolved) = resolve(bot, workspace_agent, catalog).map_err(|e| unavailable_message(&e))?;
    let launcher: &dyn InteractiveLauncher =
        agent.capabilities().interactive_launch.ok_or_else(|| unsupported_message(agent.display_name()))?;
    if let Some(SkillRef::User(name)) = &resolved.skill {
        if !user_skill_exists(name) {
            return Err(format!(
                "La skill «{name}» del Bot «{}» non esiste più: sceglila di nuovo nelle impostazioni del Bot.",
                bot.name
            ));
        }
    }
    let standard = BotStandard::of(bot, launcher, standard_file);
    let request = InteractiveRequest {
        model: resolved.model.as_deref(),
        effort: resolved.effort,
        skill: resolved.skill.as_ref().map(SkillRef::launch_skill),
        instructions_file: standard.file(),
    };
    let launch = launcher.launch(&request).ok_or_else(|| {
        format!(
            "Il Bot «{}» ha un modello o una skill con caratteri non ammessi nella riga di comando: correggilo nelle impostazioni.",
            bot.name
        )
    })?;
    if let Some(name) = first_invalid_env_name(&launch.env) {
        return Err(format!(
            "{} ha prodotto una variabile d'ambiente non valida ({name}) per il Bot «{}».",
            agent.display_name(),
            bot.name
        ));
    }
    let tab = BotTabLaunch {
        name: bot.name.clone(),
        agent_id: resolved.agent_id,
        command: launch.command,
        restore_command: launch.restore_command,
        warnings: resolved.warnings.into_iter().chain(standard.launch_warning(agent.display_name())).collect(),
    };
    Ok((tab, launch.env))
}

/// Esito del ripristino di un tab Bot dopo un riavvio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct BotTabRestore {
    /// Perche' il tab riparte con il solo comando salvato (in italiano); `None` = ripristinato per intero.
    pub warning: Option<String>,
}

/// Ambiente da rimettere sul tab e avviso per l'utente.
type RestoredEnv = (Vec<(String, String)>, Option<String>);

/// Il tab riparte con il comando salvato, senza ambiente del Bot, ma si avvisa.
fn restore_without_bot(reason: &str) -> RestoredEnv {
    (Vec::new(), Some(format!("{reason} Il terminale riparte con il comando salvato, senza le scelte del Bot.")))
}

/// L'ambiente (`restore_env`) di un tab Bot ripristinato dopo un riavvio, con
/// l'eventuale avviso. La skill non si ricarica e non serve che esista ancora.
/// Se il Bot non c'e' piu', non si risolve, il suo agente non e' quello del tab
/// (`saved_agent`) o l'ambiente non e' valido, il tab riparte comunque, senza
/// ambiente e con un avviso: mai un ripiego silenzioso. Un Bot con `work` riprende
/// lo standard di ingegneria (`standard_file`), se l'agente lo applica; se il file
/// manca riparte senza, con un avviso.
pub fn build_bot_restore(
    bot: Option<&AgentBot>,
    saved_agent: &str,
    workspace_agent: &str,
    catalog: &AgentCatalog,
    standard_file: &StandardFile,
) -> RestoredEnv {
    let Some(bot) = bot else { return restore_without_bot("Il Bot di questo terminale non esiste più.") };
    let Ok((agent, resolved)) = resolve(bot, workspace_agent, catalog) else {
        return restore_without_bot(&format!("L'agente del Bot «{}» non è più disponibile.", bot.name));
    };
    if resolved.agent_id != saved_agent {
        return restore_without_bot(&format!("Il Bot «{}» ora usa un altro agente.", bot.name));
    }
    if resolved.model.is_none() && bot.model != ModelChoice::Default {
        return restore_without_bot(&format!("Il modello del Bot «{}» non è più disponibile.", bot.name));
    }
    let not_applicable =
        || restore_without_bot(&format!("Le impostazioni del Bot «{}» non sono più applicabili.", bot.name));
    let Some(launcher) = agent.capabilities().interactive_launch else { return not_applicable() };
    let standard = BotStandard::of(bot, launcher, standard_file);
    let request = InteractiveRequest {
        model: resolved.model.as_deref(),
        effort: resolved.effort,
        skill: None,
        instructions_file: standard.file(),
    };
    let restore_env =
        launcher.launch(&request).map(|launch| launch.restore_env).filter(|env| first_invalid_env_name(env).is_none());
    match restore_env {
        Some(env) => (env, restore_standard_warning(bot, standard)),
        None => not_applicable(),
    }
}

/// L'avviso di un ripristino senza lo standard che il Bot dovrebbe avere: solo se
/// il file manca (se l'agente non lo applica l'avvio lo ha gia' detto).
fn restore_standard_warning(bot: &AgentBot, standard: BotStandard) -> Option<String> {
    match standard {
        BotStandard::Unavailable(reason) => Some(format!(
            "Lo standard di ingegneria del Bot «{}» non si è potuto preparare ({reason}): il terminale riparte senza.",
            bot.name
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
