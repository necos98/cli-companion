//! Suite di conformita': ogni integrazione registrata deve rispettare i
//! contratti che dichiara, senza che nessuno debba ricordarsi di testarla.
//! `violations` ritorna le violazioni come testo, cosi' la suite si prova
//! anche contro un'integrazione volutamente rotta.

use super::contracts::*;
use super::presets::PRESETS;
use super::registry;
use super::testing::{FakeIntegration, Flaw};
use super::worker_skill::is_plain_skill_name;
use crate::platform::{Target, TargetKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

mod interactive_launch;

use interactive_launch::interactive_launch;

/// Cartella temporanea unica (pid + contatore + nome), eliminata al drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mc-conformance-{}-{n}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Riga di comando digitata in PowerShell, cmd e bash: niente caratteri di
/// controllo, niente `$`, backtick, `%`, `!`, virgolette bilanciate.
fn shell_safe(command: &str) -> bool {
    !command.chars().any(|c| c.is_control() || matches!(c, '$' | '`' | '%' | '!'))
        && command.matches('"').count().is_multiple_of(2)
}

fn core(i: &dyn AgentIntegration) -> Vec<String> {
    let mut v = Vec::new();
    let id = i.id();
    let id_ok = !id.is_empty()
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-');
    if !id_ok {
        v.push(format!("id non valido: {id:?} (solo [a-z0-9-], senza trattini ai bordi)"));
    }
    if i.display_name().trim().is_empty() {
        v.push("display_name vuoto".to_string());
    }
    let interactive = i.interactive_command();
    if interactive.trim().is_empty() || !shell_safe(interactive) {
        v.push(format!("interactive_command non valido o non sicuro per la shell: {interactive:?}"));
    }
    let caps = i.capabilities();
    let present = [
        (caps.package.is_some(), Capability::Package),
        (caps.hook_events.is_some(), Capability::HookEvents),
        (caps.session_resume.is_some(), Capability::SessionResume),
        (caps.automation.is_some(), Capability::Automation),
        (caps.worker_permissions.is_some(), Capability::WorkerPermissions),
        (caps.plugin_skills.is_some(), Capability::PluginSkills),
        (caps.user_skills.is_some(), Capability::UserSkills),
        (caps.usage_limits.is_some(), Capability::UsageLimits),
        (caps.session_usage.is_some(), Capability::SessionUsage),
        (caps.input_profile.is_some(), Capability::InputProfile),
        (caps.models.is_some(), Capability::Models),
        (caps.effort.is_some(), Capability::Effort),
        (caps.interactive_launch.is_some(), Capability::InteractiveLaunch),
    ];
    let expected: Vec<_> = present.into_iter().filter_map(|(on, c)| on.then_some(c)).collect();
    if caps.declared() != expected {
        v.push(format!("declared() {:?} non coincide con i contratti presenti {expected:?}", caps.declared()));
    }
    v
}

fn install_ctx(data_dir: &Path) -> InstallCtx {
    InstallCtx { exe: "C:/Program Files/cli-companion/cli-companion.exe".to_string(), data_dir: data_dir.to_path_buf() }
}

fn conformance_targets() -> [Target; 2] {
    [Target::Windows, Target::Wsl { distro: "Ubuntu-24.04".to_string() }]
}

fn package(package: &dyn IntegrationPackage, data_dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let ctx = install_ctx(data_dir);
    for target in conformance_targets() {
        let key = target.key();
        match package.version(&ctx, &target) {
            None => v.push(format!("package: version() None per {key}")),
            Some(version) => {
                if version.is_empty() || version.chars().any(char::is_whitespace) {
                    v.push(format!("package: version() vuota o con spazi per {key}: {version:?}"));
                }
                if package.version(&ctx, &target) != Some(version) {
                    v.push(format!("package: version() non stabile per {key}"));
                }
            }
        }
        match package.files(&ctx, &target) {
            Err(e) => v.push(format!("package: files() fallisce per {key}: {e}")),
            Ok(files) => {
                for (n, f) in files.iter().enumerate() {
                    let p = f.path.as_str();
                    let drive = p.as_bytes().get(1) == Some(&b':');
                    if p.is_empty() || p.starts_with('/') || drive {
                        v.push(format!("package: path vuoto o non relativo: {p:?}"));
                    }
                    if p.contains('\\') {
                        v.push(format!("package: path con backslash: {p:?}"));
                    }
                    if p.split('/').any(|c| c == "..") {
                        v.push(format!("package: path con '..': {p:?}"));
                    }
                    if files[..n].iter().any(|other| other.path == f.path) {
                        v.push(format!("package: path duplicato: {p:?}"));
                    }
                    if f.content.starts_with('\u{feff}') {
                        v.push(format!("package: BOM in {p:?}"));
                    }
                }
            }
        }
    }
    v
}

fn plugin_skills(i: &dyn AgentIntegration, skills: &dyn PluginSkills, data_dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let Some(pkg) = i.capabilities().package else {
        return vec!["plugin-skills: richiede il contratto package (la skill va spedita)".to_string()];
    };
    let files = pkg.files(&install_ctx(data_dir), &Target::Windows).unwrap_or_default();
    let mut seen: Vec<String> = Vec::new();
    for skill in PRESETS {
        let path = skills.skill_path(skill);
        if seen.contains(&path) {
            v.push(format!("plugin-skills: skill_path duplicato: {path:?}"));
        }
        match files.iter().find(|f| f.path == path) {
            None => v.push(format!("plugin-skills: la skill {} manca nel pacchetto ({path})", skill.id)),
            Some(f) if f.content != skill.skill_text() => {
                v.push(format!("plugin-skills: la skill {} nel pacchetto non coincide col catalogo", skill.id));
            }
            Some(_) => {}
        }
        seen.push(path);
        let name = skills.skill_name(skill.id);
        if !is_plain_skill_name(&name) {
            v.push(format!("plugin-skills: skill_name non semplice (lettere, cifre e -_.:): {name:?}"));
        }
    }
    v
}

fn request<'a>(prompt: &'a str, model: Option<&'a str>, permissions_file: Option<&'a str>) -> AutomationRequest<'a> {
    AutomationRequest {
        bootstrap_prompt: prompt,
        permission_mode: PermissionMode::Default,
        model,
        permissions_file,
        effort: None,
    }
}

/// Modello accettato dalla forma generica, per le integrazioni senza catalogo.
const SAFE_MODEL: &str = "model-1.0";

fn automation(i: &dyn AgentIntegration, automation: &dyn WorkerAutomation) -> Vec<String> {
    let mut v = Vec::new();
    // Un modello valido per l'agente: il primo consigliato dal suo catalogo.
    let valid_model = i.capabilities().models.and_then(|c| c.suggested().first().map(|m| m.id)).unwrap_or(SAFE_MODEL);
    let prompt = crate::automation::BOOTSTRAP_PROMPT;
    for kind in [TargetKind::Windows, TargetKind::Wsl] {
        for mode in PermissionMode::ALL {
            let req = AutomationRequest { permission_mode: mode, ..request(prompt, None, None) };
            match automation.command(kind, &req) {
                None => v.push(format!("automation: nessun comando per {kind:?}/{mode:?}")),
                Some(AutomationLaunch { command: cmd, env }) => {
                    // Nome, valore e duplicati: la stessa regola che applica il registro.
                    if let Some(name) = first_invalid_env_name(&env) {
                        let invalid =
                            env.iter().any(|(n, val)| n == name && !(is_valid_env_name(n) && is_valid_env_value(val)));
                        let what = if invalid { "non valida" } else { "duplicata" };
                        v.push(format!("automation: variabile d'ambiente {what} ({kind:?}/{mode:?}): {name:?}"));
                    }
                    if !cmd.contains(prompt) {
                        v.push(format!("automation: il comando non contiene il prompt ({kind:?}/{mode:?})"));
                    }
                    if !shell_safe(&cmd) {
                        v.push(format!("automation: comando non sicuro per la shell: {cmd:?}"));
                    }
                }
            }
        }
        let launch = |req: &AutomationRequest| automation.command(kind, req);
        let run = |req: &AutomationRequest| launch(req).map(|launch| launch.command);
        for c in ['"', '$', '`', '%', '!', '\\', '\n'] {
            let bad = format!("a{c}b");
            if run(&request(&bad, None, None)).is_some() {
                v.push(format!("automation: prompt con {c:?} accettato"));
            }
        }
        if run(&request("", None, None)).is_some() {
            v.push("automation: prompt vuoto accettato".to_string());
        }
        if run(&request(prompt, Some(valid_model), None)).is_none() {
            v.push("automation: modello valido rifiutato".to_string());
        }
        let long = "m".repeat(65);
        for model in ["", "m x", "m;ls", "m\"", long.as_str()] {
            if run(&request(prompt, Some(model), None)).is_some() {
                v.push(format!("automation: modello non valido accettato: {model:?}"));
            }
        }
        for file in ["C:/a$b/x.json", "C:/a`b/x.json", "C:/100%/x.json", "C:/a\"b/x.json", "C:/hi!/x.json"] {
            if run(&request(prompt, None, Some(file))).is_some() {
                v.push(format!("automation: permissions_file non sicuro accettato: {file:?}"));
            }
        }
        let safe = "C:/data/worker-permissions/ws-1.json";
        // Il path arriva all'agente nel comando o in una variabile d'ambiente.
        match launch(&request(prompt, None, Some(safe))) {
            Some(l) if l.command.contains(safe) || l.env.iter().any(|(_, value)| value.contains(safe)) => {}
            other => v.push(format!("automation: permissions_file sicuro non usato: {other:?}")),
        }
    }
    v
}

fn input_profile(source: &dyn InputProfileSource) -> Vec<String> {
    let mut v = Vec::new();
    let rules = source.input_profile().rules;
    for (n, rule) in rules.iter().enumerate() {
        if rule.send.is_empty() {
            v.push(format!("input_profile: regola {:?}/{:?} con send vuoto", rule.event, rule.env_kind));
        }
        if rules[..n].iter().any(|other| other.event == rule.event && other.env_kind == rule.env_kind) {
            v.push(format!("input_profile: regola duplicata {:?}/{:?}", rule.event, rule.env_kind));
        }
    }
    v
}

/// Nomi che una regola dei modelli deve rifiutare: vuoto e tutto cio' che non e'
/// sicuro come singolo argomento in PowerShell, cmd e bash.
const UNSAFE_MODEL_NAMES: [&str; 9] = [
    "", "a b", "a
b", "a$b", "a\"b", "a`b", "a;b", "a|b", "a&b",
];

fn models(i: &dyn AgentIntegration, catalog: &dyn ModelCatalog) -> Vec<String> {
    let mut v = Vec::new();
    let suggested = catalog.suggested();
    for (n, model) in suggested.iter().enumerate() {
        if suggested[..n].iter().any(|other| other.id == model.id) {
            v.push(format!("models: id consigliato duplicato: {:?}", model.id));
        }
        if model.label.trim().is_empty() {
            v.push(format!("models: etichetta vuota per {:?}", model.id));
        }
        if !catalog.is_valid(model.id) {
            v.push(format!("models: il modello consigliato {:?} non e' valido per l'agente", model.id));
        }
    }
    if let Some(default) = catalog.default_model().filter(|m| !catalog.is_valid(m)) {
        v.push(format!("models: il modello predefinito {default:?} non e' valido per l'agente"));
    }
    // La regola dell'agente sta dentro la barriera generica sulla forma.
    for model in suggested.iter().map(|m| m.id).chain(catalog.default_model()) {
        if !crate::validate::valid_model_shape(model) {
            v.push(format!("models: il modello {model:?} esce dalla forma generica (valid_model_shape)"));
        }
    }
    for name in UNSAFE_MODEL_NAMES {
        if catalog.is_valid(name) {
            v.push(format!("models: is_valid accetta un nome non sicuro: {name:?}"));
        }
    }
    if let Some(automation) = i.capabilities().automation {
        for kind in [TargetKind::Windows, TargetKind::Wsl] {
            for model in suggested {
                let req = request(crate::automation::BOOTSTRAP_PROMPT, Some(model.id), None);
                if automation.command(kind, &req).is_none() {
                    v.push(format!("models: automation rifiuta il modello consigliato {:?} ({kind:?})", model.id));
                }
            }
        }
    }
    v
}

/// Effort: livelli non vuoti e senza doppioni, argomenti sicuri per la shell e
/// presenti nel comando di automazione, che richiede il contratto `automation`.
fn effort(i: &dyn AgentIntegration, effort: &dyn EffortLevels) -> Vec<String> {
    let mut v = Vec::new();
    let levels = effort.levels();
    if levels.is_empty() {
        v.push("effort: nessun livello dichiarato".to_string());
    }
    let Some(automation) = i.capabilities().automation else {
        v.push("effort: richiede il contratto automation (l'effort si applica all'avvio dei worker)".to_string());
        return v;
    };
    for (n, level) in levels.iter().enumerate() {
        if levels[..n].contains(level) {
            v.push(format!("effort: livello duplicato: {level:?}"));
        }
        let Some(args) = effort.command_args(*level) else {
            v.push(format!("effort: nessun argomento per il livello dichiarato {level:?}"));
            continue;
        };
        if args.trim().is_empty() || !shell_safe(&args) {
            v.push(format!("effort: argomenti non sicuri per la shell ({level:?}): {args:?}"));
        }
        for kind in [TargetKind::Windows, TargetKind::Wsl] {
            let req =
                AutomationRequest { effort: Some(*level), ..request(crate::automation::BOOTSTRAP_PROMPT, None, None) };
            match automation.command(kind, &req) {
                Some(launch) if launch.command.contains(&args) => {}
                other => v.push(format!(
                    "effort: il comando non contiene gli argomenti del livello {level:?} ({kind:?}): {other:?}"
                )),
            }
        }
    }
    v
}

/// Nome di skill dell'utente usato dalla suite (solo se l'agente dichiara `user_skills`).
const CONFORMANCE_USER_SKILL: &str = "conformance-skill";

/// Nome di programma (o flag) utilizzabile cosi' com'e' in una riga di comando.
fn plain_token(token: &str) -> bool {
    !token.is_empty() && !token.contains(char::is_whitespace) && shell_safe(token) && !token.contains('"')
}

fn session_resume(i: &dyn AgentIntegration, resume: &dyn SessionResume) -> Vec<String> {
    let mut v = Vec::new();
    let spec = resume.resume_spec();
    if spec.command_names.is_empty() {
        v.push("session_resume: command_names vuoto".to_string());
    }
    for name in &spec.command_names {
        if !plain_token(name) || *name != name.to_lowercase() {
            v.push(format!("session_resume: nome di comando non valido (minuscolo, senza spazi): {name:?}"));
        }
    }
    let program = i.interactive_command().split_whitespace().next().unwrap_or_default().to_lowercase();
    if !spec.command_names.iter().any(|name| *name == program) {
        v.push(format!("session_resume: il comando interattivo ({program:?}) non e' tra i command_names"));
    }
    for flag in &spec.strip_flags {
        if flag.names.is_empty() {
            v.push("session_resume: flag senza nomi".to_string());
        }
        for name in &flag.names {
            if !plain_token(name) || !name.starts_with('-') {
                v.push(format!("session_resume: nome di flag non valido (deve iniziare con '-'): {name:?}"));
            }
        }
    }
    let template = spec.resume_template;
    for placeholder in [COMMAND_PLACEHOLDER, SESSION_ID_PLACEHOLDER] {
        let found = template.matches(placeholder).count();
        if found != 1 {
            v.push(format!(
                "session_resume: il modello deve contenere {placeholder} una volta, ne ha {found}: {template:?}"
            ));
        }
    }
    let canonical = spec.command_names.first().copied().unwrap_or_default();
    let resumed = template.replace(COMMAND_PLACEHOLDER, canonical).replace(SESSION_ID_PLACEHOLDER, "abc-123");
    if !shell_safe(&resumed) {
        v.push(format!("session_resume: comando di ripresa non sicuro per la shell: {resumed:?}"));
    }
    v
}

fn usage_limits(limits: &dyn UsageLimits) -> Vec<String> {
    if limits.notice().trim().is_empty() {
        vec!["usage_limits: notice vuoto".to_string()]
    } else {
        Vec::new()
    }
}

fn hook_events(events: &dyn HookEvents) -> Vec<String> {
    let mut v = Vec::new();
    for kind in HookEventKind::ALL {
        for raw in ["", "not json", "[]", "{}"] {
            if events.normalize(kind, raw) != HookEvent::default() {
                v.push(format!("hook_events: {kind:?} con {raw:?} non da' un evento senza dettagli"));
            }
        }
    }
    let line = events.stop_block_output("x");
    if line.is_empty() || !line.contains('x') {
        v.push(format!("hook_events: stop_block_output(\"x\") vuoto o senza il motivo: {line:?}"));
    }
    v
}

fn worker_permissions(permissions: &dyn WorkerPermissions, data_dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    let format = permissions.rule_format();
    if format.max_rules == 0 || format.max_rule_chars == 0 || format.example.trim().is_empty() {
        v.push(format!("worker_permissions: forma delle regole incompleta: {format:?}"));
    }
    let guide = permissions.guide();
    if guide.rules_help.trim().is_empty() || guide.bypass_note.trim().is_empty() {
        v.push("worker_permissions: guida vuota".to_string());
    }
    let blank = ["".to_string(), "  ".to_string()];
    match permissions.validate_rules(&blank) {
        Ok(rules) if rules.is_empty() => {}
        other => v.push(format!("worker_permissions: righe vuote attese scartate, ottenuto {other:?}")),
    }
    match permissions.validate_rules(&[format.example.to_string()]) {
        Ok(rules) if rules == [format.example] => {}
        other => {
            v.push(format!("worker_permissions: l'esempio {:?} non e' una regola valida: {other:?}", format.example))
        }
    }
    match permissions.prepare(data_dir, "ws-conformance", TargetKind::Windows, &[], &[], &[]) {
        Ok(None) => {}
        other => v.push(format!("worker_permissions: senza regole atteso Ok(None), ottenuto {other:?}")),
    }
    v
}

fn user_skills(skills: &dyn UserSkills) -> Vec<String> {
    let mut v = Vec::new();
    let sub = skills.wsl_home_subpath();
    let drive = sub.as_bytes().get(1) == Some(&b':');
    if sub.is_empty() || sub.starts_with('/') || drive || sub.contains('\\') || sub.split('/').any(|c| c == "..") {
        v.push(format!("user_skills: wsl_home_subpath non valido: {sub:?}"));
    }
    let name = skills.skill_name(CONFORMANCE_USER_SKILL);
    if !is_plain_skill_name(&name) {
        v.push(format!("user_skills: skill_name non semplice (lettere, cifre e -_.:): {name:?}"));
    }
    v
}

fn session_usage(reader: &dyn SessionUsageReader) -> Vec<String> {
    ["", "../x"]
        .into_iter()
        .filter(|path| reader.session_usage(&Target::Windows, path).is_ok())
        .map(|path| format!("session_usage: il path {path:?} dovrebbe essere rifiutato"))
        .collect()
}

/// Tutte le violazioni dei contratti dichiarati da `i` (vuoto = conforme).
fn violations(i: &dyn AgentIntegration) -> Vec<String> {
    let scratch = Scratch::new(i.id());
    let dir = scratch.0.as_path();
    let caps = i.capabilities();
    let mut v = core(i);
    if let Some(c) = caps.package {
        v.extend(package(c, dir));
    }
    if let Some(c) = caps.plugin_skills {
        v.extend(plugin_skills(i, c, dir));
    }
    if let Some(c) = caps.automation {
        v.extend(automation(i, c));
    }
    if let Some(c) = caps.input_profile {
        v.extend(input_profile(c));
    }
    if let Some(c) = caps.session_resume {
        v.extend(session_resume(i, c));
    }
    if let Some(c) = caps.hook_events {
        v.extend(hook_events(c));
    }
    if let Some(c) = caps.worker_permissions {
        v.extend(worker_permissions(c, dir));
    }
    if let Some(c) = caps.user_skills {
        v.extend(user_skills(c));
    }
    if let Some(c) = caps.session_usage {
        v.extend(session_usage(c));
    }
    if let Some(c) = caps.usage_limits {
        v.extend(usage_limits(c));
    }
    if let Some(c) = caps.models {
        v.extend(models(i, c));
    }
    if let Some(c) = caps.effort {
        v.extend(effort(i, c));
    }
    if let Some(c) = caps.interactive_launch {
        v.extend(interactive_launch(i, c));
    }
    v.into_iter().map(|m| format!("{}: {m}", i.id())).collect()
}

#[test]
fn every_registered_integration_conforms() {
    for integration in registry::all() {
        let found = violations(*integration);
        assert!(found.is_empty(), "{}", found.join("\n"));
    }
}

#[test]
fn fake_integrations_conform() {
    for fake in [FakeIntegration::full("fake-full"), FakeIntegration::bare("fake-bare", "fake --yes")] {
        let found = violations(&fake);
        assert!(found.is_empty(), "{}", found.join("\n"));
    }
}

#[test]
fn a_missing_plugin_skill_is_reported() {
    let broken = FakeIntegration::full("broken-skill").with_flaw(Flaw::PluginSkillMissing);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("plugin-skills:") && m.contains("manca nel pacchetto")), "{found:?}");
}

#[test]
fn an_invalid_automation_env_is_reported() {
    let broken = FakeIntegration::full("broken-env").with_flaw(Flaw::AutomationInvalidEnv);
    let found = violations(&broken);
    assert!(
        found.iter().any(|m| m.contains("automation:") && m.contains("variabile d'ambiente non valida")),
        "{found:?}"
    );
}

#[test]
fn an_unsafe_automation_command_is_reported() {
    let broken = FakeIntegration::full("broken-dollar").with_flaw(Flaw::AutomationLetsDollarThrough);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("automation:") && m.contains("'$'")), "{found:?}");
}

#[test]
fn a_resume_template_without_the_session_id_is_reported() {
    let broken = FakeIntegration::full("broken-resume").with_flaw(Flaw::ResumeTemplateWithoutId);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("session_resume:") && m.contains(SESSION_ID_PLACEHOLDER)), "{found:?}");
}

#[test]
fn a_model_rule_that_accepts_spaces_is_reported() {
    let broken = FakeIntegration::full("broken-models").with_flaw(Flaw::ModelValidationAcceptsSpaces);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("models:") && m.contains("\"a b\"")), "{found:?}");
}

#[test]
fn a_suggested_model_outside_the_generic_shape_is_reported() {
    let broken = FakeIntegration::full("broken-shape").with_flaw(Flaw::SuggestedModelOutsideShape);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("models:") && m.contains("valid_model_shape")), "{found:?}");
}

#[test]
fn unsafe_effort_arguments_are_reported() {
    let broken = FakeIntegration::full("broken-effort").with_flaw(Flaw::EffortArgsUnsafe);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("effort:") && m.contains("non sicuri")), "{found:?}");
}

#[test]
fn identity_and_declaration_problems_are_reported() {
    let found = violations(&FakeIntegration::bare("Bad Id-", "echo $HOME"));
    assert!(found.iter().any(|m| m.contains("id non valido")), "{found:?}");
    assert!(found.iter().any(|m| m.contains("interactive_command")), "{found:?}");
    // Skill del plugin senza pacchetto: la skill non avrebbe dove stare.
    let skills_only = FakeIntegration::plugin_skills_only("only-skills", "fake --yes");
    assert!(violations(&skills_only).iter().any(|m| m.contains("richiede il contratto package")));
}

#[test]
fn registry_ids_are_unique_and_the_default_worker_agent_can_automate() {
    let ids: Vec<_> = registry::all().iter().map(|i| i.id()).collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "id duplicati: {ids:?}");
    let default = registry::find(registry::default_agent().id()).expect("agente dei worker registrato");
    assert!(default.capabilities().automation.is_some(), "l'agente dei worker deve dichiarare automation");
}

#[test]
fn a_restore_command_that_keeps_the_skill_is_reported() {
    let broken = FakeIntegration::full("broken-restore").with_flaw(Flaw::RestoreKeepsSkill);
    let found = violations(&broken);
    assert!(
        found.iter().any(|m| m.contains("interactive_launch:") && m.contains("restore_command riporta")),
        "{found:?}"
    );
}

#[test]
fn declared_instructions_that_are_ignored_are_reported() {
    let broken = FakeIntegration::full("broken-instructions").with_flaw(Flaw::InstructionsIgnored);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("non applicato all'avvio")), "{found:?}");
    assert!(found.iter().any(|m| m.contains("perso al ripristino")), "{found:?}");
    // Un agente che non le dichiara non e' tenuto ad applicarle.
    assert!(violations(&FakeIntegration::full("no-instructions").without_instructions()).is_empty());
}

#[test]
fn an_unsafe_launch_skill_name_is_reported() {
    let broken = FakeIntegration::full("broken-launch").with_flaw(Flaw::LaunchAcceptsDollarSkill);
    let found = violations(&broken);
    assert!(found.iter().any(|m| m.contains("interactive_launch:") && m.contains("skill non sicura")), "{found:?}");
}
