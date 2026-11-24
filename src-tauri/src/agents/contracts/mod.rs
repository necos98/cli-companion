//! Contratti (capacita') che il core offre alle integrazioni degli agenti.
//!
//! Il core non sa come funziona un agente: ogni integrazione implementa i
//! contratti che le servono e li dichiara in `AgentIntegration::capabilities`.
//! Un contratto = un file con un trait (piu' i tipi che scambia). Nessun
//! metodo ha un'implementazione di default "non supportato": o l'integrazione
//! dichiara il contratto, o il core non lo usa.

mod automation;
mod capabilities;
mod effort;
mod hook_events;
mod input_profile;
mod integration;
mod interactive_launch;
mod models;
mod package;
mod plugin_skills;
mod proxy_routing;
mod session_resume;
mod session_usage;
mod usage_limits;
mod user_skills;
mod worker_permissions;

pub use automation::{
    first_invalid_env_name, is_shell_safe_path, is_valid_env_name, is_valid_env_value, AutomationLaunch,
    AutomationRequest, AutomationStart, PermissionMode, WorkerAutomation,
};
pub use capabilities::{Capabilities, Capability};
pub use effort::{Effort, EffortLevels};
pub use hook_events::{HookEvent, HookEventKind, HookEvents, WaitKind};
pub use input_profile::{InputEvent, InputProfile, InputProfileSource, InputRule};
pub use integration::{AgentIntegration, Detection};
pub use interactive_launch::{
    is_safe_launch_token, InteractiveLaunch, InteractiveLauncher, InteractiveRequest, LaunchSkill,
};
pub use models::{ModelCatalog, ModelTier, SuggestedModel};
pub use package::{fnv32, plugins_dir, sanitize_env_key, InstallCtx, IntegrationPackage, PackageFile};
pub use plugin_skills::PluginSkills;
pub use proxy_routing::{ProxyRouting, ProxyTarget};
pub use session_resume::{ResumeFlag, ResumeSpec, SessionResume};
#[cfg(test)]
pub use session_resume::{COMMAND_PLACEHOLDER, SESSION_ID_PLACEHOLDER};
pub use session_usage::{SessionUsage, SessionUsageReader, TokenTotals};
pub use usage_limits::{LimitWindow, UsageLimits, UsageLimitsReply};
pub use user_skills::UserSkills;
pub use worker_permissions::{PermissionsGuide, RuleFormat, WorkerPermissions};
