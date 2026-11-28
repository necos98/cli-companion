//! Integrazione finta configurabile, condivisa dai test (avvii, pacchetti,
//! comandi, conformita'). Implementa tutti i contratti con logica semplice e
//! corretta in vocabolario del core; `declares` decide quali dichiarare.

use super::contracts::*;
use super::presets::{Preset, PRESETS};
use crate::platform::{Target, TargetKind};
use crate::validate::{ErrorCode, ValidationError};
use std::path::{Path, PathBuf};

/// Difetto voluto, per provare che la conformita' lo segnala.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flaw {
    /// Il pacchetto non spedisce la prima skill del plugin.
    PluginSkillMissing,
    /// Il comando di automazione lascia passare `$` nel prompt.
    AutomationLetsDollarThrough,
    /// Il modello del comando di ripresa non contiene il segnaposto dell'id.
    ResumeTemplateWithoutId,
    /// La regola dei modelli accetta nomi con spazi.
    ModelValidationAcceptsSpaces,
    /// Un modello consigliato esce dalla forma generica (`valid_model_shape`).
    SuggestedModelOutsideShape,
    /// L'automazione restituisce una variabile d'ambiente con nome non valido.
    AutomationInvalidEnv,
    /// Il comando dell'effort contiene un carattere non sicuro per la shell.
    EffortArgsUnsafe,
    /// L'invocazione dell'esploratore va su piu' righe.
    ExplorerInvocationMultiline,
    /// Il comando di ripristino riporta anche la skill.
    RestoreKeepsSkill,
    /// L'avvio interattivo accetta un nome di skill con `$`.
    LaunchAcceptsDollarSkill,
    /// L'avvio interattivo dichiara le istruzioni di sistema ma le ignora.
    InstructionsIgnored,
    /// L'instradamento sul proxy restituisce una variabile d'ambiente con nome non valido.
    ProxyInvalidEnv,
}

#[derive(Debug, Clone)]
pub struct FakeIntegration {
    pub id: &'static str,
    pub interactive: &'static str,
    /// Versione del pacchetto.
    pub version: &'static str,
    pub declares: Vec<Capability>,
    pub flaw: Option<Flaw>,
    /// L'agente risulta presente nel target.
    pub found: bool,
    /// Il pacchetto risulta installato (se l'agente e' presente).
    pub installed: bool,
    /// L'avvio interattivo applica il file di istruzioni di sistema.
    pub applies_instructions: bool,
}

impl FakeIntegration {
    /// Nessun contratto dichiarato.
    pub fn bare(id: &'static str, interactive: &'static str) -> Self {
        Self {
            id,
            interactive,
            version: "1.0-hfake",
            declares: Vec::new(),
            flaw: None,
            found: false,
            installed: false,
            applies_instructions: true,
        }
    }

    /// Dichiara solo il pacchetto, con la versione data.
    pub fn versioned(id: &'static str, version: &'static str) -> Self {
        Self { version, declares: vec![Capability::Package], ..Self::bare(id, "fake-cli") }
    }

    /// Dichiara solo le skill del plugin.
    pub fn plugin_skills_only(id: &'static str, interactive: &'static str) -> Self {
        Self { declares: vec![Capability::PluginSkills], ..Self::bare(id, interactive) }
    }

    /// Dichiara tutti i contratti.
    pub fn full(id: &'static str) -> Self {
        let declares = vec![
            Capability::Package,
            Capability::HookEvents,
            Capability::SessionResume,
            Capability::Automation,
            Capability::WorkerPermissions,
            Capability::PluginSkills,
            Capability::UserSkills,
            Capability::UsageLimits,
            Capability::SessionUsage,
            Capability::InputProfile,
            Capability::Models,
            Capability::Effort,
            Capability::InteractiveLaunch,
            Capability::ProxyRouting,
            Capability::ExplorationDelegation,
        ];
        Self { declares, ..Self::bare(id, "fake --yes") }
    }

    /// Risultati del rilevamento: se l'agente e' presente e se il pacchetto e' installato.
    pub fn detected(mut self, found: bool, installed: bool) -> Self {
        self.found = found;
        self.installed = installed;
        self
    }

    /// Toglie un contratto fra quelli dichiarati.
    pub fn without(mut self, capability: Capability) -> Self {
        self.declares.retain(|c| *c != capability);
        self
    }

    /// L'avvio interattivo ignora il file di istruzioni di sistema (e lo dichiara).
    pub fn without_instructions(mut self) -> Self {
        self.applies_instructions = false;
        self
    }

    pub fn with_flaw(mut self, flaw: Flaw) -> Self {
        self.flaw = Some(flaw);
        self
    }
}

impl AgentIntegration for FakeIntegration {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        "Fake"
    }

    fn detect(&self, _: &Target) -> Detection {
        Detection { found: self.found, ..Detection::default() }
    }

    fn interactive_command(&self) -> &'static str {
        self.interactive
    }

    fn capabilities(&self) -> Capabilities<'_> {
        let on = |c| self.declares.contains(&c);
        Capabilities {
            package: on(Capability::Package).then_some(self as &dyn IntegrationPackage),
            hook_events: on(Capability::HookEvents).then_some(self as &dyn HookEvents),
            session_resume: on(Capability::SessionResume).then_some(self as &dyn SessionResume),
            automation: on(Capability::Automation).then_some(self as &dyn WorkerAutomation),
            worker_permissions: on(Capability::WorkerPermissions).then_some(self as &dyn WorkerPermissions),
            plugin_skills: on(Capability::PluginSkills).then_some(self as &dyn PluginSkills),
            user_skills: on(Capability::UserSkills).then_some(self as &dyn UserSkills),
            usage_limits: on(Capability::UsageLimits).then_some(self as &dyn UsageLimits),
            session_usage: on(Capability::SessionUsage).then_some(self as &dyn SessionUsageReader),
            input_profile: on(Capability::InputProfile).then_some(self as &dyn InputProfileSource),
            models: on(Capability::Models).then_some(self as &dyn ModelCatalog),
            effort: on(Capability::Effort).then_some(self as &dyn EffortLevels),
            interactive_launch: on(Capability::InteractiveLaunch).then_some(self as &dyn InteractiveLauncher),
            proxy_routing: on(Capability::ProxyRouting).then_some(self as &dyn ProxyRouting),
            exploration_delegation: on(Capability::ExplorationDelegation).then_some(self as &dyn ExplorationDelegation),
        }
    }
}

impl IntegrationPackage for FakeIntegration {
    fn version(&self, _: &InstallCtx, _: &Target) -> Option<String> {
        Some(self.version.to_string())
    }

    fn files(&self, _: &InstallCtx, _: &Target) -> Result<Vec<PackageFile>, String> {
        let manifest =
            PackageFile { path: "manifest.json".into(), content: format!("{{\"version\":\"{}\"}}", self.version) };
        let skipped = usize::from(self.flaw == Some(Flaw::PluginSkillMissing));
        let skills = PRESETS
            .iter()
            .skip(skipped)
            .map(|p| PackageFile { path: PluginSkills::skill_path(self, p), content: p.skill_text() });
        Ok(std::iter::once(manifest).chain(skills).collect())
    }

    fn is_installed(&self, _: &Target, _: &Detection) -> bool {
        self.installed
    }

    fn install(&self, _: &InstallCtx, _: &Target, _: &Detection) -> Result<String, String> {
        Ok(String::new())
    }

    fn uninstall(&self, _: &InstallCtx, _: &Target, _: &Detection) -> Result<String, String> {
        Ok(String::new())
    }
}

/// Stdin finto di un hook che aspetta: `wait:<nome>`, con nomi propri
/// dell'agente finto (diversi da quelli di qualunque agente reale).
pub const FAKE_WAIT_PREFIX: &str = "wait:";
pub const FAKE_WAIT_APPROVAL: &str = "tool-approval";
pub const FAKE_WAIT_NUDGE: &str = "still-there";
pub const FAKE_WAIT_ASK: &str = "user-answer";

impl HookEvents for FakeIntegration {
    /// Solo il tipo di attesa, da `wait:<nome>`; un nome sconosciuto e'
    /// un'attesa generica, ogni altro stdin un evento senza dettagli.
    fn normalize(&self, _: HookEventKind, raw: &str) -> HookEvent {
        let wait = raw.strip_prefix(FAKE_WAIT_PREFIX).map(|name| match name {
            FAKE_WAIT_APPROVAL => WaitKind::Permission,
            FAKE_WAIT_NUDGE => WaitKind::Idle,
            FAKE_WAIT_ASK => WaitKind::Question,
            _ => WaitKind::Other,
        });
        HookEvent { wait, ..HookEvent::default() }
    }

    fn stop_block_output(&self, reason: &str) -> String {
        format!("fake-stop {reason}")
    }
}

impl SessionResume for FakeIntegration {
    fn resume_spec(&self) -> ResumeSpec {
        let program = self.interactive.split_whitespace().next().unwrap_or_default();
        let resume_template = if self.flaw == Some(Flaw::ResumeTemplateWithoutId) {
            "{command} --resume"
        } else {
            "{command} --resume {id}"
        };
        ResumeSpec { command_names: vec![program], strip_flags: Vec::new(), resume_template }
    }
}

impl WorkerAutomation for FakeIntegration {
    fn command(&self, _: TargetKind, req: &AutomationRequest) -> Option<AutomationLaunch> {
        let leaks_dollar = self.flaw == Some(Flaw::AutomationLetsDollarThrough);
        let unsafe_char =
            |c: char| matches!(c, '"' | '`' | '%' | '!' | '\\') || c.is_control() || (c == '$' && !leaks_dollar);
        if req.bootstrap_prompt.is_empty() || req.bootstrap_prompt.chars().any(unsafe_char) {
            return None;
        }
        let mut cmd = format!("{} \"{}\"", self.id, req.bootstrap_prompt);
        if let Some(file) = req.permissions_file {
            if !crate::agents::is_shell_safe_path(file) {
                return None;
            }
            cmd.push_str(&format!(" --rules \"{file}\""));
        }
        if let Some(model) = req.model {
            let valid = !model.is_empty()
                && model.len() <= 64
                && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
            if !valid {
                return None;
            }
            cmd.push_str(&format!(" --model {model}"));
        }
        if let Some(effort) = req.effort {
            cmd.push_str(&format!(" {}", EffortLevels::command_args(self, effort)?));
        }
        // Una variabile che dipende dalla richiesta, per provare il percorso fino al PTY.
        let name = if self.flaw == Some(Flaw::AutomationInvalidEnv) { "fake_agent_mode" } else { FAKE_ENV_NAME };
        let env = vec![(name.to_string(), req.permission_mode.as_str().to_string())];
        Some(AutomationLaunch { command: cmd, env })
    }
}

impl WorkerPermissions for FakeIntegration {
    fn rule_format(&self) -> RuleFormat {
        RuleFormat { max_rules: 5, max_rule_chars: 20, example: "fake-rule" }
    }

    fn guide(&self) -> PermissionsGuide {
        PermissionsGuide {
            rules_help: "Regole finte.",
            bypass_note: "Nota finta.",
            allow_placeholder: "fake-allow",
            deny_placeholder: "fake-deny",
        }
    }

    /// Una regola e' una parola senza spazi.
    fn validate_rules(&self, rules: &[String]) -> Result<Vec<String>, ValidationError> {
        let mut out: Vec<String> = Vec::new();
        for rule in rules.iter().map(|r| r.trim()).filter(|r| !r.is_empty()) {
            if rule.contains(char::is_whitespace) {
                return Err(ValidationError::new(ErrorCode::PermissionRuleBadShape).value(rule.to_string()));
            }
            if !out.iter().any(|r| r == rule) {
                out.push(rule.to_string());
            }
        }
        Ok(out)
    }

    fn prepare(
        &self,
        _: &Path,
        workspace_id: &str,
        _: TargetKind,
        allow: &[String],
        deny: &[String],
        readable: &[PathBuf],
    ) -> Result<Option<String>, String> {
        let any = !allow.is_empty() || !deny.is_empty() || !readable.is_empty();
        Ok(any.then(|| format!("rules/{workspace_id}.json")))
    }
}

impl PluginSkills for FakeIntegration {
    fn skill_path(&self, preset: &Preset) -> String {
        format!("skills/{}/SKILL.md", preset.id)
    }

    fn skill_name(&self, id: &str) -> String {
        format!("fake-plugin:{id}")
    }
}

impl UserSkills for FakeIntegration {
    fn windows_dir(&self) -> Result<PathBuf, String> {
        Ok(PathBuf::from("C:/fake/skills"))
    }

    fn wsl_home_subpath(&self) -> &'static str {
        ".fake/skills"
    }

    fn skill_name(&self, name: &str) -> String {
        name.to_string()
    }
}

impl UsageLimits for FakeIntegration {
    fn limits(&self, _: &Target) -> UsageLimitsReply {
        UsageLimitsReply::status("unavailable", Some("finto"))
    }

    fn notice(&self) -> &'static str {
        "Avviso finto."
    }
}

impl SessionUsageReader for FakeIntegration {
    fn session_usage(&self, _: &Target, transcript_path: &str) -> Result<SessionUsage, String> {
        if transcript_path.is_empty() || transcript_path.contains("..") {
            return Err("percorso non valido".to_string());
        }
        Ok(SessionUsage::default())
    }
}

impl InputProfileSource for FakeIntegration {
    fn input_profile(&self) -> InputProfile {
        InputProfile {
            rules: vec![InputRule { event: InputEvent::ImagePaste, env_kind: None, send: "\x1bv".to_string() }],
        }
    }
}

/// Variabile d'ambiente che l'automazione dell'agente finto imposta (valore: il livello di permessi).
pub const FAKE_ENV_NAME: &str = "FAKE_AGENT_MODE";

/// Modelli dell'agente finto, con nomi che non appartengono a nessun agente reale.
pub const FAKE_MODEL_SMALL: &str = "fake-small";
pub const FAKE_MODEL_LARGE: &str = "fake-large";
/// Variabile con cui il finto agente porta il modello al ripristino (`restore_env`).
pub const FAKE_MODEL_ENV: &str = "FAKE_MODEL";
const FAKE_MODEL_PREFIX: &str = "fake-";

const FAKE_SUGGESTED: &[SuggestedModel] = &[
    SuggestedModel { id: FAKE_MODEL_SMALL, label: "Fake small", tier: ModelTier::Standard },
    SuggestedModel { id: FAKE_MODEL_LARGE, label: "Fake large", tier: ModelTier::Strong },
];

/// Nome che l'agente accetta ma che non ha la forma generica sicura (`//`).
const FAKE_MODEL_OUTSIDE_SHAPE: &str = "fake-a//b";

const FAKE_SUGGESTED_OUTSIDE_SHAPE: &[SuggestedModel] =
    &[SuggestedModel { id: FAKE_MODEL_OUTSIDE_SHAPE, label: "Fake odd", tier: ModelTier::Standard }];

impl ModelCatalog for FakeIntegration {
    fn suggested(&self) -> &'static [SuggestedModel] {
        if self.flaw == Some(Flaw::SuggestedModelOutsideShape) {
            return FAKE_SUGGESTED_OUTSIDE_SHAPE;
        }
        FAKE_SUGGESTED
    }

    fn default_model(&self) -> Option<&'static str> {
        Some(FAKE_MODEL_SMALL)
    }

    /// Prefisso `fake-` e solo `[a-z0-9-]`, al massimo `MODEL_MAX` byte. Col
    /// difetto accetta qualunque nome non vuoto, anche con spazi.
    fn is_valid(&self, model: &str) -> bool {
        let lenient = self.flaw == Some(Flaw::ModelValidationAcceptsSpaces);
        if self.flaw == Some(Flaw::SuggestedModelOutsideShape) && model == FAKE_MODEL_OUTSIDE_SHAPE {
            return true;
        }
        (lenient || model.starts_with(FAKE_MODEL_PREFIX))
            && !model.is_empty()
            && model.len() <= crate::validate::MODEL_MAX
            && model.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || (lenient && c == ' '))
    }
}

/// Livelli dell'agente finto: un sottoinsieme, per provare i livelli non supportati.
pub const FAKE_EFFORT_LEVELS: [Effort; 2] = [Effort::Low, Effort::High];

impl EffortLevels for FakeIntegration {
    fn levels(&self) -> &'static [Effort] {
        &FAKE_EFFORT_LEVELS
    }

    fn command_args(&self, effort: Effort) -> Option<String> {
        let separator = if self.flaw == Some(Flaw::EffortArgsUnsafe) { "$" } else { " " };
        Some(format!("--think{separator}{}", effort.as_str()))
    }
}

/// Variabile con cui l'agente finto riceve l'indirizzo del proxy.
pub const FAKE_PROXY_ENV: &str = "FAKE_AGENT_PROXY";

impl ProxyRouting for FakeIntegration {
    fn proxy_env(&self, proxy: &ProxyTarget) -> Option<Vec<(String, String)>> {
        let name = if self.flaw == Some(Flaw::ProxyInvalidEnv) { "fake-proxy" } else { FAKE_PROXY_ENV };
        Some(vec![(name.to_string(), proxy.base_url.clone())])
    }
}

/// Invocazione dell'esploratore dell'agente finto.
pub const FAKE_EXPLORER_INVOCATION: &str = "spawn the fake explorer";

impl ExplorationDelegation for FakeIntegration {
    fn explorer_invocation(&self) -> String {
        if self.flaw == Some(Flaw::ExplorerInvocationMultiline) {
            return format!("{FAKE_EXPLORER_INVOCATION}\nthen wait");
        }
        FAKE_EXPLORER_INVOCATION.to_string()
    }
}

impl InteractiveLauncher for FakeIntegration {
    /// `<interattivo> [--model m] [<argomenti effort>] [--system-file "<file>"] ["/<skill>"]`, con la skill del plugin come `/plugin:<id>`.
    fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch> {
        let mut restore_command = self.interactive.to_string();
        if let Some(model) = req.model {
            if !is_safe_launch_token(model) {
                return None;
            }
            restore_command.push_str(&format!(" --model {model}"));
        }
        if let Some(effort) = req.effort {
            restore_command.push_str(&format!(" {}", EffortLevels::command_args(self, effort)?));
        }
        if let Some(file) =
            req.instructions_file.filter(|_| self.applies_instructions && self.flaw != Some(Flaw::InstructionsIgnored))
        {
            if !is_shell_safe_path(file) {
                return None;
            }
            restore_command.push_str(&format!(" --system-file \"{file}\""));
        }
        let lenient = self.flaw == Some(Flaw::LaunchAcceptsDollarSkill);
        let skill_ok = |name: &str| is_safe_launch_token(name) || (lenient && name.contains('$'));
        let prompt = match req.skill {
            Some(LaunchSkill::Plugin(id)) if skill_ok(id) => Some(format!("/fake-plugin:{id}")),
            Some(LaunchSkill::User(name)) if skill_ok(name) => Some(format!("/{name}")),
            Some(_) => return None,
            None => None,
        };
        let command = match &prompt {
            Some(prompt) => format!("{restore_command} \"{prompt}\""),
            None => restore_command.clone(),
        };
        let restore_command =
            if self.flaw == Some(Flaw::RestoreKeepsSkill) { command.clone() } else { restore_command };
        let restore_env = req.model.map(|m| (FAKE_MODEL_ENV.to_string(), m.to_string())).into_iter().collect();
        Some(InteractiveLaunch { command, restore_command, env: Vec::new(), restore_env })
    }

    fn applies_instructions(&self) -> bool {
        self.applies_instructions
    }
}
