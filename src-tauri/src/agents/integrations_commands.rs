//! Comandi Tauri della pagina delle integrazioni: elenco con lo stato di ogni
//! target, abilitazione e scelta dell'agente predefinito. Le regole stanno in
//! `enablement`, il salvataggio in `integrations_store`.

use super::commands::install_ctx;
use super::contracts::{AgentIntegration, Capability, Effort, InstallCtx, SuggestedModel};
use super::detection::list_targets;
use super::enablement::{self, IntegrationsState};
use super::env_status::{probe_pairs, AgentEnvStatus};
use super::integrations_store;
use super::readiness::{cached_probe, readiness, Readiness};
use super::registry;
use crate::platform::Target;
use crate::store::data_dir;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

/// Evento emesso a ogni cambiamento dello stato; il payload e' il nuovo `IntegrationsState`.
pub const INTEGRATIONS_EVENT: &str = "integrations-changed";

/// Stato di un'integrazione in un target.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IntegrationEnv {
    pub status: AgentEnvStatus,
    pub readiness: Readiness,
}

/// Un'integrazione registrata, abilitata o no.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct IntegrationInfo {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub is_default: bool,
    /// Contratti che l'integrazione dichiara.
    pub capabilities: Vec<Capability>,
    /// Modelli consigliati; vuoti se l'integrazione non dichiara `models`.
    pub models: Vec<SuggestedModel>,
    /// Modello usato quando non se ne sceglie uno; `None` senza `models` o se decide l'agente.
    pub default_model: Option<String>,
    /// Livelli di effort che l'integrazione sa applicare; vuoti se non dichiara `effort`.
    pub effort_levels: Vec<Effort>,
    /// Uno per ogni target (Windows + distro WSL).
    pub envs: Vec<IntegrationEnv>,
}

/// Stato di una coppia agente x target. La prontezza passa dalla cache:
/// quella di un'integrazione disabilitata non rileva nulla, ma lo stato
/// dell'ambiente si legge comunque, per mostrare se l'agente c'e'.
fn env_of(integration: &dyn AgentIntegration, target: &Target, ctx: &InstallCtx) -> (String, IntegrationEnv) {
    let readiness = readiness(integration, target, ctx);
    let status = cached_probe(integration, target, ctx).status;
    (integration.id().to_string(), IntegrationEnv { status, readiness })
}

/// Le informazioni di ogni integrazione, con `envs` (coppie id agente x stato)
/// raggruppati per agente nell'ordine in cui arrivano.
fn build_infos(
    integrations: &[&dyn AgentIntegration],
    state: &IntegrationsState,
    envs: Vec<(String, IntegrationEnv)>,
) -> Vec<IntegrationInfo> {
    let state = enablement::view(state, integrations);
    integrations
        .iter()
        .map(|i| {
            let catalog = i.capabilities().models;
            IntegrationInfo {
                id: i.id().to_string(),
                name: i.display_name().to_string(),
                enabled: state.enabled.iter().any(|e| e == i.id()),
                is_default: state.default_agent == i.id(),
                capabilities: i.capabilities().declared(),
                models: catalog.map(|c| c.suggested().to_vec()).unwrap_or_default(),
                default_model: catalog.and_then(|c| c.default_model()).map(str::to_string),
                effort_levels: i.capabilities().effort.map(|e| e.levels().to_vec()).unwrap_or_default(),
                envs: envs.iter().filter(|(id, _)| id == i.id()).map(|(_, env)| env.clone()).collect(),
            }
        })
        .collect()
}

/// Tutte le integrazioni con lo stato di ogni target. Ogni coppia puo' lanciare
/// processi: si sondano in parallelo.
#[tauri::command]
pub async fn integrations_list(app: AppHandle) -> Result<Vec<IntegrationInfo>, String> {
    crate::blocking(move || {
        let ctx = install_ctx(&app)?;
        let envs = probe_pairs(registry::all(), &list_targets(), |i, target| env_of(i, target, &ctx));
        Ok(build_infos(registry::all(), &integrations_store::current(), envs))
    })
    .await
}

/// Applica una regola allo stato, lo salva e lo comunica al frontend. `id`
/// arriva dal frontend: si confronta soltanto con gli id del registro.
fn apply(
    app: &AppHandle,
    rule: impl FnOnce(&IntegrationsState, &[&dyn AgentIntegration]) -> Result<IntegrationsState, String>,
) -> Result<IntegrationsState, String> {
    let next = integrations_store::update(&data_dir(app)?, rule)?;
    if let Err(e) = app.emit(INTEGRATIONS_EVENT, &next) {
        log::warn!("evento {INTEGRATIONS_EVENT} non inviato: {e}");
    }
    crate::automation::integrations_changed(app);
    Ok(next)
}

#[tauri::command]
pub async fn set_integration_enabled(app: AppHandle, id: String, enabled: bool) -> Result<IntegrationsState, String> {
    crate::blocking(move || apply(&app, |state, registered| enablement::set_enabled(state, registered, &id, enabled)))
        .await
}

#[tauri::command]
pub async fn set_default_integration(app: AppHandle, id: String) -> Result<IntegrationsState, String> {
    crate::blocking(move || apply(&app, |state, registered| enablement::set_default(state, registered, &id))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::env_status::status_for;
    use crate::agents::testing::{FakeIntegration, FAKE_EFFORT_LEVELS, FAKE_MODEL_LARGE, FAKE_MODEL_SMALL};

    fn env(agent: &'static str, key: &str, readiness: Readiness) -> (String, IntegrationEnv) {
        let fake = FakeIntegration::bare(agent, "fake-cli");
        let target = if key == "windows" { Target::Windows } else { Target::Wsl { distro: key.into() } };
        (agent.to_string(), IntegrationEnv { status: status_for(&fake, &target), readiness })
    }

    #[test]
    fn infos_carry_flags_models_and_envs_per_integration() {
        let bare = FakeIntegration::bare("bare", "bare-cli");
        let full = FakeIntegration::full("full");
        let state = IntegrationsState { enabled: vec!["full".into()], default_agent: "full".into() };
        let envs = vec![
            env("bare", "windows", Readiness::Disabled),
            env("full", "windows", Readiness::NotFound),
            env("bare", "Ubuntu", Readiness::Disabled),
            env("full", "Ubuntu", Readiness::Ready),
        ];
        let infos = build_infos(&[&bare, &full], &state, envs);
        assert_eq!(infos.len(), 2);

        let (bare_info, full_info) = (&infos[0], &infos[1]);
        assert_eq!((bare_info.id.as_str(), bare_info.name.as_str()), ("bare", "Fake"));
        assert!(!bare_info.enabled && !bare_info.is_default);
        assert!(bare_info.capabilities.is_empty() && bare_info.models.is_empty());
        assert_eq!(bare_info.default_model, None);
        assert!(bare_info.effort_levels.is_empty());
        let readiness_of = |info: &IntegrationInfo| info.envs.iter().map(|e| e.readiness).collect::<Vec<_>>();
        assert_eq!(readiness_of(bare_info), vec![Readiness::Disabled, Readiness::Disabled]);

        assert!(full_info.enabled && full_info.is_default);
        assert_eq!(full_info.capabilities.len(), 14);
        let models: Vec<_> = full_info.models.iter().map(|m| m.id).collect();
        assert_eq!(models, vec![FAKE_MODEL_SMALL, FAKE_MODEL_LARGE]);
        assert_eq!(full_info.default_model.as_deref(), Some(FAKE_MODEL_SMALL));
        assert_eq!(full_info.effort_levels, FAKE_EFFORT_LEVELS);
        let keys: Vec<_> = full_info.envs.iter().map(|e| e.status.env_key.as_str()).collect();
        assert_eq!(keys, vec!["windows", "Ubuntu"]);
        assert_eq!(readiness_of(full_info), vec![Readiness::NotFound, Readiness::Ready]);
    }

    #[test]
    fn infos_read_the_state_through_the_invariants() {
        let first = FakeIntegration::bare("first", "first-cli");
        let second = FakeIntegration::bare("second", "second-cli");
        // Stato non configurato: la prima abilitata e predefinita.
        let infos = build_infos(&[&first, &second], &IntegrationsState::default(), Vec::new());
        let flags: Vec<_> = infos.iter().map(|i| (i.enabled, i.is_default)).collect();
        assert_eq!(flags, vec![(true, true), (false, false)]);
        assert!(infos.iter().all(|i| i.envs.is_empty()));
    }

    #[test]
    fn the_dto_serializes_for_the_frontend() {
        let full = FakeIntegration::full("full");
        let info = &build_infos(&[&full], &IntegrationsState::default(), Vec::new())[0];
        let v = serde_json::to_value(info).unwrap();
        assert_eq!(v["is_default"], true);
        assert_eq!(v["models"][0]["tier"], "standard");
        assert_eq!(v["effort_levels"], serde_json::json!(["low", "high"]));
        assert!(v["capabilities"].as_array().unwrap().contains(&serde_json::json!("models")));
        let env = serde_json::to_value(env("full", "windows", Readiness::NotInstalled).1).unwrap();
        assert_eq!(env["readiness"], "not-installed");
        assert_eq!(env["status"]["env_key"], "windows");
    }
}
