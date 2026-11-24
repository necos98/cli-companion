//! Comandi Tauri sugli agenti: avvii integrati, stato per target, profili di
//! input, consumi e ciclo di vita dei pacchetti. Generici: parlano solo con i
//! contratti delle integrazioni.

use super::contracts::{
    AgentIntegration, Capability, InputProfile, InstallCtx, PermissionsGuide, ResumeSpec, SessionUsage,
    UsageLimitsReply,
};
use super::detection::{invalidate_detection_cache, list_targets};
use super::env_status::{collect_status, AgentEnvStatus};
use super::launches::{builtin_launch_list, BuiltinLaunch};
use super::package::{self, OutdatedPlugin, PluginUpdateResult};
use super::readiness::forget_readiness;
use super::registry;
use crate::launch_settings::LaunchSettingsStore;
use crate::platform::{forward_slashes, is_valid_distro_name, Target};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use ts_rs::TS;

/// Comando Tauri `builtin_launches`: gli avvii integrati, uno per agente
/// abilitato. Economico, nessun processo.
#[tauri::command]
pub fn builtin_launches() -> Vec<BuiltinLaunch> {
    builtin_launch_list()
}

#[tauri::command]
pub async fn agent_env_status() -> Result<Vec<AgentEnvStatus>, String> {
    crate::blocking(|| Ok(collect_status(registry::all(), &list_targets()))).await
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AgentProfile {
    pub agent_id: String,
    pub agent_name: String,
    /// Contratti che l'integrazione dichiara.
    pub capabilities: Vec<Capability>,
    /// Vuoto se l'integrazione non dichiara `input_profile`.
    pub input_profile: InputProfile,
    /// `true` per l'agente predefinito (`registry::default_agent`), lo stesso
    /// che usano i worker e i preset.
    pub is_default: bool,
    /// Dati per riprendere le sessioni; `None` se non dichiara `session_resume`.
    pub session_resume: Option<ResumeSpec>,
    /// Avviso sui limiti dell'abbonamento; `None` se non dichiara `usage_limits`.
    pub usage_limits_notice: Option<&'static str>,
    /// Testi sui comandi pre-approvati dei worker; `None` se non dichiara
    /// `worker_permissions`: le regole non si applicano.
    pub worker_permissions: Option<PermissionsGuide>,
}

fn collect_profiles(integrations: &[&dyn AgentIntegration], default_id: &str) -> Vec<AgentProfile> {
    integrations
        .iter()
        .map(|i| {
            let caps = i.capabilities();
            AgentProfile {
                agent_id: i.id().to_string(),
                agent_name: i.display_name().to_string(),
                capabilities: caps.declared(),
                input_profile: caps.input_profile.map(|p| p.input_profile()).unwrap_or_default(),
                is_default: i.id() == default_id,
                session_resume: caps.session_resume.map(|resume| resume.resume_spec()),
                usage_limits_notice: caps.usage_limits.map(|limits| limits.notice()),
                worker_permissions: caps.worker_permissions.map(|permissions| permissions.guide()),
            }
        })
        .collect()
}

/// Profili degli agenti abilitati: economico, nessun processo.
#[tauri::command]
pub fn agent_profiles() -> Vec<AgentProfile> {
    collect_profiles(&registry::enabled(), registry::default_agent().id())
}

/// Target dal frontend per le letture dei consumi. Il nome della distro finisce
/// in un path UNC: niente separatori o caratteri insoliti.
fn usage_env(env_key: &str) -> Result<Target, String> {
    let target = Target::from_key(env_key)?;
    if let Target::Wsl { distro: d } = &target {
        if !is_valid_distro_name(d) {
            return Err(format!("nome distro non valido: {d}"));
        }
    }
    Ok(target)
}

/// Limiti dell'abbonamento; "unsupported" se l'integrazione non li espone.
fn usage_limits_for(integration: &dyn AgentIntegration, target: &Target) -> UsageLimitsReply {
    match integration.capabilities().usage_limits {
        Some(limits) => limits.limits(target),
        None => UsageLimitsReply::status("unsupported", None),
    }
}

/// Consumi di una sessione; `None` se l'integrazione non li espone.
fn session_usage_for(
    integration: &dyn AgentIntegration,
    target: &Target,
    transcript_path: &str,
) -> Result<Option<SessionUsage>, String> {
    match integration.capabilities().session_usage {
        Some(reader) => reader.session_usage(target, transcript_path).map(Some),
        None => Ok(None),
    }
}

/// Limiti dell'abbonamento dell'agente. Stato "disabled" se l'utente ha
/// spento i consumi, "unsupported" se l'agente non li espone.
#[tauri::command]
pub async fn agent_usage_limits(
    settings: State<'_, LaunchSettingsStore>,
    agent_id: String,
    env: String,
) -> Result<UsageLimitsReply, String> {
    let integration = registry::find(&agent_id)?;
    let target = usage_env(&env)?;
    if !settings.get().show_usage {
        return Ok(UsageLimitsReply::status("disabled", None));
    }
    crate::blocking(move || Ok(usage_limits_for(integration, &target))).await
}

/// Token consumati da una sessione (dal transcript). `None` se l'agente non
/// li espone o i consumi sono spenti.
#[tauri::command]
pub async fn agent_session_usage(
    settings: State<'_, LaunchSettingsStore>,
    agent_id: String,
    env: String,
    transcript_path: String,
) -> Result<Option<SessionUsage>, String> {
    let integration = registry::find(&agent_id)?;
    let target = usage_env(&env)?;
    if !settings.get().show_usage {
        return Ok(None);
    }
    crate::blocking(move || session_usage_for(integration, &target, &transcript_path)).await
}

/// Contesto di installazione: exe corrente e cartella dati dell'app.
pub(super) fn install_ctx(app: &AppHandle) -> Result<InstallCtx, String> {
    let exe = std::env::current_exe().map_err(|e| format!("percorso exe: {e}"))?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(InstallCtx { exe: forward_slashes(&exe.to_string_lossy()), data_dir })
}

/// Pacchetti installati da aggiornare, delle sole integrazioni abilitate
/// (nessun processo dell'agente).
#[tauri::command]
pub async fn agent_plugins_outdated(app: AppHandle) -> Result<Vec<OutdatedPlugin>, String> {
    crate::blocking(move || {
        let ctx = install_ctx(&app)?;
        Ok(package::find_outdated(&registry::enabled(), &ctx, &mut list_targets))
    })
    .await
}

/// Esegue un'operazione sui pacchetti: rilevamento e prontezza vanno
/// dimenticati prima (si parte da dati freschi) e la prontezza di nuovo dopo,
/// perche' un sondaggio avviato durante l'operazione non deve sopravviverle.
fn with_fresh_detection<T>(operation: impl FnOnce() -> T) -> T {
    invalidate_detection_cache();
    forget_readiness();
    let result = operation();
    forget_readiness();
    result
}

/// Reinstalla i pacchetti non aggiornati delle integrazioni abilitate, in parallelo.
#[tauri::command]
pub async fn agent_plugins_update(app: AppHandle) -> Result<Vec<PluginUpdateResult>, String> {
    crate::blocking(move || with_fresh_detection(|| Ok(package::update_all(&registry::enabled(), &install_ctx(&app)?))))
        .await
}

/// Installa o rimuove il pacchetto di qualunque integrazione registrata: la
/// pagina delle integrazioni le gestisce tutte, abilitate o no.
#[tauri::command]
pub async fn install_agent_plugin(app: AppHandle, agent_id: String, env_key: String) -> Result<String, String> {
    crate::blocking(move || with_fresh_detection(|| package::install(&install_ctx(&app)?, &agent_id, &env_key))).await
}

#[tauri::command]
pub async fn remove_agent_plugin(app: AppHandle, agent_id: String, env_key: String) -> Result<String, String> {
    crate::blocking(move || with_fresh_detection(|| package::remove(&install_ctx(&app)?, &agent_id, &env_key))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::testing::FakeIntegration;
    use crate::platform::TargetKind;

    #[test]
    fn profiles_cover_all_integrations_and_serialize_kebab_case() {
        let profiles = collect_profiles(registry::all(), "claude-code");
        assert_eq!(profiles.len(), registry::all().len());
        let cc = profiles.iter().find(|p| p.agent_id == "claude-code").unwrap();
        assert_eq!(cc.agent_name, "Claude Code");
        let v = serde_json::to_value(cc).unwrap();
        let rules = v["input_profile"]["rules"].as_array().unwrap();
        for kind in [TargetKind::Windows, TargetKind::Wsl] {
            assert!(rules
                .iter()
                .any(|r| r["event"] == "image-paste" && r["env_kind"] == kind.as_str() && r["send"] == "\u{1b}v"));
        }
        let caps = v["capabilities"].as_array().unwrap();
        assert!(
            caps.contains(&serde_json::json!("input-profile")) && caps.contains(&serde_json::json!("usage-limits"))
        );
        assert_eq!(v["is_default"], true);
        assert_eq!(v["session_resume"]["command_names"], serde_json::json!(["claude", "claude.exe"]));
        assert_eq!(v["session_resume"]["strip_flags"][0]["names"], serde_json::json!(["--resume", "-r"]));
        assert_eq!(v["session_resume"]["strip_flags"][0]["takes_value"], true);
        assert_eq!(v["session_resume"]["resume_template"], "{command} --resume {id}");
        assert!(!v["usage_limits_notice"].as_str().unwrap().is_empty());
        assert!(v["worker_permissions"]["rules_help"].as_str().unwrap().contains("Bash(git:*)"));
        assert!(!v["worker_permissions"]["bypass_note"].as_str().unwrap().is_empty());
    }

    #[test]
    fn profile_without_input_profile_is_empty() {
        let bare = FakeIntegration::bare("bare", "bare-cli");
        let p = &collect_profiles(&[&bare], "other")[0];
        assert!(p.capabilities.is_empty() && p.input_profile.rules.is_empty());
        let v = serde_json::to_value(p).unwrap();
        assert!(
            v["session_resume"].is_null() && v["usage_limits_notice"].is_null() && v["worker_permissions"].is_null()
        );
        assert_eq!(v["is_default"], false);
        let full = FakeIntegration::full("full");
        let p = &collect_profiles(&[&full], "other")[0];
        assert_eq!(p.capabilities.len(), 14);
        assert!(!p.input_profile.rules.is_empty());
        let v = serde_json::to_value(p).unwrap();
        assert_eq!(v["is_default"], false);
        assert_eq!(v["session_resume"]["command_names"], serde_json::json!(["fake"]));
        assert!(!v["usage_limits_notice"].as_str().unwrap().is_empty());
        assert_eq!(v["worker_permissions"]["rules_help"], "Regole finte.");
    }

    #[test]
    fn only_the_given_default_is_marked_default() {
        let a = FakeIntegration::bare("a", "a-cli");
        let b = FakeIntegration::bare("b", "b-cli");
        let flags =
            |default_id| collect_profiles(&[&a, &b], default_id).iter().map(|p| p.is_default).collect::<Vec<_>>();
        assert_eq!(flags("a"), vec![true, false]);
        assert_eq!(flags("b"), vec![false, true]);
        assert_eq!(flags("c"), vec![false, false]);
    }

    #[test]
    fn usage_is_unsupported_without_the_contracts_and_env_is_validated() {
        let bare = FakeIntegration::bare("f", "f-cli");
        assert_eq!(usage_limits_for(&bare, &Target::Windows).state, "unsupported");
        assert_eq!(session_usage_for(&bare, &Target::Windows, "x"), Ok(None));
        let full = FakeIntegration::full("g");
        assert_ne!(usage_limits_for(&full, &Target::Windows).state, "unsupported");
        assert!(session_usage_for(&full, &Target::Windows, "").is_err());
        assert!(session_usage_for(&full, &Target::Windows, "p/projects/s/a.jsonl").unwrap().is_some());
        assert_eq!(usage_env("windows"), Ok(Target::Windows));
        assert_eq!(usage_env("Ubuntu-24.04"), Ok(Target::Wsl { distro: "Ubuntu-24.04".into() }));
        for bad in ["", "a\\b", "a/b", "..", "x:y", "a\nb"] {
            assert!(usage_env(bad).is_err(), "{bad:?}");
        }
        let v = serde_json::to_value(UsageLimitsReply::status("disabled", None)).unwrap();
        assert_eq!(v["state"], "disabled");
        assert!(v["five_hour"].is_null() && v["message"].is_null());
    }
}
