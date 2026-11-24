//! Registro delle integrazioni: l'unico punto in cui il core nomina i singoli
//! agenti.

use super::claude_code;
use super::contracts::{first_invalid_env_name, AgentIntegration, AutomationRequest, AutomationStart};
use super::enablement;
use super::integrations_store;
use super::models::{effort_is_supported, model_is_valid, unsupported_effort_warning};
use super::opencode;
use super::pi_agent;
use super::resolve::{self, AgentUnavailable, AgentUse};
use crate::platform::TargetKind;

/// Integrazioni supportate. Per aggiungerne una: una riga qui. Quali sono
/// abilitate e quale e' il predefinito lo sceglie l'utente (`integrations.json`);
/// finche' non sceglie, vale la prima.
static INTEGRATIONS: &[&dyn AgentIntegration] = &[&claude_code::ClaudeCode, &opencode::OpenCode, &pi_agent::PiAgent];

/// Tutte le integrazioni registrate, abilitate o no: serve alla pagina delle
/// integrazioni, alla conformita' e al client IPC. Chi mostra o avvia qualcosa
/// all'utente usa `enabled`. Nei test un thread puo' sostituirle con
/// `test_registry`.
pub fn all() -> &'static [&'static dyn AgentIntegration] {
    #[cfg(test)]
    if let Some(registered) = super::test_registry::registered() {
        return registered;
    }
    INTEGRATIONS
}

/// Le integrazioni abilitate, nell'ordine del registro (mai vuoto).
pub fn enabled() -> Vec<&'static dyn AgentIntegration> {
    enablement::enabled_in(all(), &integrations_store::current())
}

/// `id` e' un'integrazione registrata e abilitata.
pub fn is_enabled(id: &str) -> bool {
    integrations_store::current().enabled.iter().any(|e| e == id)
}

/// L'agente predefinito scelto dall'utente: quello dei task senza un agente e
/// dei preset.
pub fn default_agent() -> &'static dyn AgentIntegration {
    enablement::default_in(all(), &integrations_store::current())
}

/// Cerca fra TUTTE le integrazioni registrate, anche disabilitate: gli hook di
/// un'integrazione disabilitata il cui plugin e' ancora installato devono
/// poter essere gestiti senza errori. Per scegliere l'agente di un uso si
/// passa da `resolve_agent`.
pub fn find(id: &str) -> Result<&'static dyn AgentIntegration, String> {
    all().iter().find(|i| i.id() == id).copied().ok_or_else(|| format!("agente sconosciuto: {id}"))
}

/// L'unico punto in cui si sceglie l'agente di un uso: `explicit` (la scelta
/// del chiamante, per esempio l'agente di un task), poi `workspace_agent`,
/// poi il predefinito globale; vince il primo presente e, se non e'
/// utilizzabile, e' un errore: mai un ripiego su un altro agente. `purpose`
/// serve a pretendere il contratto giusto (worker: automazione), cosi' un agente abilitato ma incapace fallisce subito e con un
/// messaggio chiaro.
pub fn resolve_agent(
    explicit: Option<&str>,
    workspace_agent: Option<&str>,
    purpose: AgentUse,
) -> Result<&'static dyn AgentIntegration, AgentUnavailable> {
    resolve::resolve_in(all(), &integrations_store::current(), explicit, workspace_agent, purpose)
}

/// Comando e variabili d'ambiente di avvio di una sessione automatica per
/// l'agente `agent_id`. Le variabili sono validate qui: un'integrazione che ne
/// restituisce di non valide e' difettosa e la sessione non parte. Un `effort`
/// che l'agente non sa applicare non blocca l'avvio: viene tolto dalla
/// richiesta e segnalato in `AutomationStart::warnings`.
pub fn automation_command(
    agent_id: &str,
    env_kind: TargetKind,
    req: &AutomationRequest,
) -> Result<AutomationStart, String> {
    // Errori letti dal modello via RPC: in inglese.
    let integration =
        resolve_agent(Some(agent_id), None, AgentUse::Worker).map_err(|unavailable| unavailable.to_string())?;
    if let Some(model) = req.model.filter(|m| !model_is_valid(integration, m)) {
        return Err(format!("invalid model for {}: {model}", integration.display_name()));
    }
    let (req, warnings) = supported_effort(integration, req);
    let launch = integration
        .capabilities()
        .automation
        .and_then(|automation| automation.command(env_kind, &req))
        .ok_or_else(|| format!("{} does not support automated sessions", integration.display_name()))?;
    if let Some(name) = first_invalid_env_name(&launch.env) {
        return Err(format!(
            "{} returned an invalid environment variable for automated sessions: {name}",
            integration.display_name()
        ));
    }
    Ok(AutomationStart { launch, warnings })
}

/// La richiesta con il solo effort che l'agente sa applicare, piu' l'avviso
/// (nel log e per il chiamante) se ne ha tolto uno.
fn supported_effort<'a>(
    integration: &dyn AgentIntegration,
    req: &AutomationRequest<'a>,
) -> (AutomationRequest<'a>, Vec<String>) {
    match req.effort {
        Some(effort) if !effort_is_supported(integration, effort) => {
            let warning = unsupported_effort_warning(integration, effort);
            log::warn!("{warning}");
            (AutomationRequest { effort: None, ..*req }, vec![warning])
        }
        _ => (*req, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Capability, PermissionMode};

    #[test]
    fn registry_ids_are_unique_and_findable() {
        let mut ids: Vec<_> = all().iter().map(|i| i.id()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all().len());
        assert!(find("claude-code").is_ok());
        assert!(find("nope").is_err());
    }

    #[test]
    fn automation_command_reports_unknown_and_unsupported_agents() {
        let req = AutomationRequest {
            bootstrap_prompt: "ok",
            permission_mode: PermissionMode::Default,
            model: None,
            permissions_file: None,
            effort: None,
        };
        assert!(automation_command(default_agent().id(), TargetKind::Wsl, &req).is_ok());
        assert_eq!(automation_command("nope", TargetKind::Windows, &req), Err("unknown agent: nope".to_string()));
        // Richiesta non rappresentabile in modo sicuro.
        let bad = AutomationRequest { bootstrap_prompt: "a$b", ..req };
        assert_eq!(
            automation_command(default_agent().id(), TargetKind::Windows, &bad),
            Err("Claude Code does not support automated sessions".to_string())
        );
    }

    #[test]
    fn automation_command_passes_valid_env_and_rejects_an_invalid_one() {
        use crate::agents::test_registry::SecondIntegration;
        use crate::agents::testing::{FakeIntegration, Flaw, FAKE_ENV_NAME};
        let req = AutomationRequest {
            bootstrap_prompt: "ok",
            permission_mode: PermissionMode::Plan,
            model: None,
            permissions_file: None,
            effort: None,
        };
        let _good = SecondIntegration::enabled(FakeIntegration::full("fake-env"));
        let launch = automation_command("fake-env", TargetKind::Windows, &req).unwrap().launch;
        assert_eq!(launch.env, [(FAKE_ENV_NAME.to_string(), "plan".to_string())]);
        // Claude Code non chiede variabili.
        assert!(automation_command(default_agent().id(), TargetKind::Windows, &req).unwrap().launch.env.is_empty());
        drop(_good);

        let _bad = SecondIntegration::enabled(FakeIntegration::full("fake-bad").with_flaw(Flaw::AutomationInvalidEnv));
        assert_eq!(
            automation_command("fake-bad", TargetKind::Windows, &req),
            Err("Fake returned an invalid environment variable for automated sessions: fake_agent_mode".to_string())
        );
    }

    #[test]
    fn automation_command_ignores_an_effort_the_agent_cannot_apply() {
        use crate::agents::test_registry::SecondIntegration;
        use crate::agents::testing::FakeIntegration;
        use crate::agents::Effort;
        let req = AutomationRequest {
            bootstrap_prompt: "ok",
            permission_mode: PermissionMode::Plan,
            model: None,
            permissions_file: None,
            effort: Some(Effort::Max),
        };
        // Un agente senza il contratto: l'avvio riesce, l'effort sparisce dal comando e arriva l'avviso.
        let _bare = SecondIntegration::enabled(FakeIntegration::full("fake-no-effort").without(Capability::Effort));
        let start = automation_command("fake-no-effort", TargetKind::Windows, &req).unwrap();
        assert!(!start.launch.command.contains("--think"), "{}", start.launch.command);
        assert_eq!(start.warnings, ["Fake does not support the effort level \"max\": ignored"]);
        drop(_bare);

        // Un livello fuori da quelli dichiarati dall'agente si comporta allo stesso modo.
        let _partial = SecondIntegration::enabled(FakeIntegration::full("fake-partial"));
        let start = automation_command("fake-partial", TargetKind::Windows, &req).unwrap();
        assert!(!start.launch.command.contains("--think"));
        assert_eq!(start.warnings.len(), 1);

        // Un livello dichiarato passa e non produce avvisi.
        let high = AutomationRequest { effort: Some(Effort::High), ..req };
        let start = automation_command("fake-partial", TargetKind::Windows, &high).unwrap();
        assert!(start.launch.command.ends_with(" --think high"), "{}", start.launch.command);
        assert!(start.warnings.is_empty());
        drop(_partial);

        // Senza effort nessun avviso, e Claude Code lo applica.
        let plain = AutomationRequest { effort: None, ..req };
        assert!(automation_command(default_agent().id(), TargetKind::Windows, &plain).unwrap().warnings.is_empty());
        let claude = AutomationRequest { effort: Some(Effort::XHigh), ..req };
        let start = automation_command(default_agent().id(), TargetKind::Windows, &claude).unwrap();
        assert!(start.launch.command.ends_with(" --effort xhigh"));
        assert!(start.warnings.is_empty());
    }

    #[test]
    fn automation_command_rejects_a_model_the_agent_does_not_accept() {
        let req = AutomationRequest {
            bootstrap_prompt: "ok",
            permission_mode: PermissionMode::Default,
            model: Some("a b"),
            permissions_file: None,
            effort: None,
        };
        assert_eq!(
            automation_command(default_agent().id(), TargetKind::Windows, &req),
            Err("invalid model for Claude Code: a b".to_string())
        );
        let known = AutomationRequest { model: Some("sonnet"), ..req };
        assert!(automation_command(default_agent().id(), TargetKind::Windows, &known).is_ok());
    }
}
