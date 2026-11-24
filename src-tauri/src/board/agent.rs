//! Chi esegue un task: il Bot (se c'e'), l'agente e se il suo modello gli
//! appartiene. E' l'unico posto che trasforma la specifica di un task in quella
//! risolta (agente, modello, effort e skill concreti) che il task si porta
//! dietro. Gli errori sono in inglese: li leggono i modelli (MCP) e l'utente li
//! vede nell'interfaccia.

use super::TaskSpec;
use crate::agents::{model_is_valid, registry, AgentIntegration, AgentUnavailable, AgentUse};
use crate::bots::resolve::resolve_bot;
use crate::bots::{AgentBot, AgentCatalog};

/// Cio' che serve per risolvere una specifica oltre alla specifica stessa.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpecContext<'a> {
    /// Agente del workspace; `None` = il predefinito globale.
    pub workspace_agent: Option<&'a str>,
    /// I Bot-agente tra cui scegliere (tutti, anche i disabilitati).
    pub bots: &'a [AgentBot],
}

impl<'a> SpecContext<'a> {
    /// Un contesto senza Bot, con l'agente del workspace dato.
    pub fn workspace(workspace_agent: Option<&'a str>) -> Self {
        Self { workspace_agent, bots: &[] }
    }
}

/// Come si sceglie l'agente di un uso: `explicit`, agente del workspace,
/// predefinito globale (la firma di `registry::resolve_agent`).
type Resolver<'a> =
    dyn Fn(Option<&str>, Option<&str>, AgentUse) -> Result<&'a dyn AgentIntegration, AgentUnavailable> + 'a;

/// Il registro degli agenti visto dalla risoluzione: iniettato, cosi' i test
/// usano integrazioni finte.
struct AgentEnv<'a, 'r> {
    resolve: &'a Resolver<'r>,
    catalog: &'a AgentCatalog<'r>,
    /// Id del predefinito globale, per un workspace senza agente.
    default_agent: &'a str,
}

/// La specifica risolta: `agent` e' sempre l'agente che eseguira' il task e,
/// con un Bot, `model`, `effort` e `skill` sono la copia di quelli del Bot. Senza
/// Bot, `effort` e `skill` restano vuoti.
pub fn resolve_spec(spec: &TaskSpec, ctx: SpecContext<'_>) -> Result<TaskSpec, String> {
    let Some(bot_id) = spec.bot.as_deref() else {
        let agent = resolve_spec_agent(spec, ctx.workspace_agent)?;
        return Ok(TaskSpec { agent: Some(agent), effort: None, skill: None, ..spec.clone() });
    };
    let catalog = AgentCatalog::current();
    let env = AgentEnv {
        resolve: &registry::resolve_agent,
        catalog: &catalog,
        default_agent: registry::default_agent().id(),
    };
    resolve_with_bot(&env, spec, bot_id, ctx)
}

/// L'id dell'agente che eseguira' `spec` senza Bot: il suo, altrimenti quello
/// del workspace, altrimenti il predefinito globale. L'agente deve poter eseguire
/// i worker e il modello della specifica, se c'e', deve essere uno dei suoi.
fn resolve_spec_agent(spec: &TaskSpec, workspace_agent: Option<&str>) -> Result<String, String> {
    resolve_spec_agent_with(&registry::resolve_agent, spec, workspace_agent)
}

fn resolve_spec_agent_with(
    resolve: &Resolver<'_>,
    spec: &TaskSpec,
    workspace_agent: Option<&str>,
) -> Result<String, String> {
    let integration = resolve(spec.agent.as_deref(), workspace_agent, AgentUse::Worker)
        .map_err(|unavailable| format!("task '{}': {unavailable}", spec.id))?;
    check_model(integration, spec)?;
    Ok(integration.id().to_string())
}

/// Il modello della specifica, se c'e', deve essere uno dell'agente.
fn check_model(integration: &dyn AgentIntegration, spec: &TaskSpec) -> Result<(), String> {
    match spec.model.as_deref().filter(|m| !model_is_valid(integration, m)) {
        Some(model) => Err(format!(
            "task '{}': invalid model for {}: {model} (see get_workspace_settings.agents)",
            spec.id,
            integration.display_name()
        )),
        None => Ok(()),
    }
}

/// La specifica con il Bot `bot_id` risolto e copiato nel task. L'effort e' quello
/// grezzo del Bot: se l'agente non lo supporta lo toglie l'avvio, con un avviso
/// nella nota del task. Gli avvisi di `resolve_bot` (un modello che l'agente non
/// conosce, per esempio) non servono qui: il task parte con il modello della CLI.
/// Una skill che l'agente non sa caricare invece rifiuta il Bot.
fn resolve_with_bot(
    env: &AgentEnv<'_, '_>,
    spec: &TaskSpec,
    bot_id: &str,
    ctx: SpecContext<'_>,
) -> Result<TaskSpec, String> {
    let task = &spec.id;
    let bot = find_bot(spec, bot_id, ctx.bots)?;
    if !bot.enabled {
        return Err(format!("task '{task}': {} is disabled: enable it in the bots settings", bot.name));
    }
    ensure_runs_kind(spec, bot)?;
    let workspace_agent = ctx.workspace_agent.unwrap_or(env.default_agent);
    let resolved = resolve_bot(bot, workspace_agent, env.catalog)
        .map_err(|unavailable| format!("task '{task}': {unavailable}"))?;
    let integration = (env.resolve)(Some(&resolved.agent_id), None, AgentUse::Worker)
        .map_err(|unavailable| format!("task '{task}': {unavailable}"))?;
    if let Some(requested) = spec.agent.as_deref().filter(|a| *a != resolved.agent_id) {
        return Err(format!(
            "task '{task}': agent {requested} does not match the agent of bot {bot_id} ({}): omit agent or choose another bot",
            resolved.agent_id
        ));
    }
    check_model(integration, spec)?;
    if let (Some(skill), None) = (&bot.skill, &resolved.skill) {
        return Err(format!(
            "task '{task}': bot {bot_id} needs the skill {}, which {} cannot load: choose another bot",
            skill.id(),
            integration.display_name()
        ));
    }
    Ok(TaskSpec {
        agent: Some(resolved.agent_id),
        model: spec.model.clone().or(resolved.model),
        effort: bot.effort,
        skill: resolved.skill,
        ..spec.clone()
    })
}

/// Il Bot della specifica, gia' risolta, puo' eseguire il suo tipo di task: per
/// un task che cambia tipo senza rifare la copia del Bot. Senza Bot non c'e'
/// nulla da verificare.
pub fn check_bot_kind(spec: &TaskSpec, bots: &[AgentBot]) -> Result<(), String> {
    match spec.bot.as_deref() {
        Some(bot_id) => ensure_runs_kind(spec, find_bot(spec, bot_id, bots)?),
        None => Ok(()),
    }
}

fn find_bot<'b>(spec: &TaskSpec, bot_id: &str, bots: &'b [AgentBot]) -> Result<&'b AgentBot, String> {
    bots.iter()
        .find(|b| b.id == bot_id)
        .ok_or_else(|| format!("task '{}': unknown bot: {bot_id} (see get_workspace_settings.bots)", spec.id))
}

/// Il Bot esegue i task del tipo della specifica, senza ripieghi su un altro Bot.
fn ensure_runs_kind(spec: &TaskSpec, bot: &AgentBot) -> Result<(), String> {
    if bot.runs(spec.kind) {
        return Ok(());
    }
    let kind = spec.kind.as_str();
    Err(format!(
        "task '{}': bot {} cannot run {kind} tasks: choose a bot whose kinds include {kind} (see \
         get_workspace_settings.bots)",
        spec.id, bot.name
    ))
}

#[cfg(test)]
mod tests;
