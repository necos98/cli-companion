use super::*;
use crate::agents::registry;
use crate::agents::testing::{FakeIntegration, FAKE_MODEL_LARGE, FAKE_MODEL_SMALL};
use crate::agents::{resolve_in, Capability, Effort, IntegrationsState};
use crate::board::TaskKind;
use crate::bots::SkillRef;

const SECOND: &str = "fake-second";
/// Valido per l'agente predefinito (forma sicura), non per quello finto.
const FOREIGN_MODEL: &str = "other-model";

fn first_id() -> &'static str {
    registry::all()[0].id()
}

fn spec(agent: Option<&str>, model: Option<&str>) -> TaskSpec {
    TaskSpec {
        id: "t1".into(),
        agent: agent.map(str::to_string),
        model: model.map(str::to_string),
        ..TaskSpec::default()
    }
}

/// Risoluzione con l'agente predefinito piu' un secondo agente finto `second`;
/// `enabled`: quali dei due sono abilitati, `default_agent` il predefinito globale.
fn resolve(
    second: &FakeIntegration,
    enabled: &[&str],
    default_agent: &str,
    spec: &TaskSpec,
    workspace_agent: Option<&str>,
) -> Result<String, String> {
    let registered = [registry::all()[0], second as &dyn AgentIntegration];
    let state = IntegrationsState {
        enabled: enabled.iter().map(|s| s.to_string()).collect(),
        default_agent: default_agent.into(),
    };
    resolve_spec_agent_with(
        &|explicit, workspace, purpose| resolve_in(&registered, &state, explicit, workspace, purpose),
        spec,
        workspace_agent,
    )
}

fn resolve_both(spec: &TaskSpec, workspace_agent: Option<&str>) -> Result<String, String> {
    resolve(&FakeIntegration::full(SECOND), &[first_id(), SECOND], first_id(), spec, workspace_agent)
}

#[test]
fn the_agent_of_the_spec_wins_then_the_workspace_then_the_global_default() {
    let first = first_id().to_string();
    let second = SECOND.to_string();
    assert_eq!(resolve_both(&spec(None, None), None), Ok(first.clone()));
    assert_eq!(resolve_both(&spec(None, None), Some(SECOND)), Ok(second.clone()));
    assert_eq!(resolve_both(&spec(Some(first_id()), None), Some(SECOND)), Ok(first));
    assert_eq!(resolve_both(&spec(Some(SECOND), None), Some(first_id())), Ok(second.clone()));
    let fake_default = FakeIntegration::full(SECOND);
    let both = [first_id(), SECOND];
    assert_eq!(resolve(&fake_default, &both, SECOND, &spec(None, None), None), Ok(second));
}

#[test]
fn an_unavailable_agent_is_an_error_prefixed_with_the_task() {
    let unknown = resolve_both(&spec(Some("nope"), None), None);
    assert_eq!(unknown, Err("task 't1': unknown agent: nope".to_string()));
    let from_workspace = resolve_both(&spec(None, None), Some("gone"));
    assert_eq!(from_workspace, Err("task 't1': unknown agent: gone".to_string()));

    let second = FakeIntegration::full(SECOND);
    let disabled = resolve(&second, &[first_id()], first_id(), &spec(Some(SECOND), None), None);
    assert_eq!(disabled, Err("task 't1': Fake is disabled: enable it in the integrations settings".to_string()));

    let bare = FakeIntegration::bare(SECOND, "fake-cli");
    let both = [first_id(), SECOND];
    let unsupported = resolve(&bare, &both, first_id(), &spec(Some(SECOND), None), None);
    assert_eq!(unsupported, Err("task 't1': Fake does not support automated sessions".to_string()));
}

#[test]
fn a_model_must_belong_to_the_agent_that_runs_the_task() {
    assert_eq!(resolve_both(&spec(Some(SECOND), Some(FAKE_MODEL_SMALL)), None), Ok(SECOND.to_string()));
    assert_eq!(resolve_both(&spec(Some(first_id()), Some(FOREIGN_MODEL)), None), Ok(first_id().to_string()));

    let wrong = resolve_both(&spec(Some(SECOND), Some(FOREIGN_MODEL)), None);
    let expected = format!("task 't1': invalid model for Fake: {FOREIGN_MODEL} (see get_workspace_settings.agents)");
    assert_eq!(wrong, Err(expected));
    // Con l'agente del workspace: stesso controllo.
    assert!(resolve_both(&spec(None, Some(FOREIGN_MODEL)), Some(SECOND)).is_err());
    assert!(resolve_both(&spec(None, Some(FAKE_MODEL_SMALL)), Some(first_id())).is_ok());
}

#[test]
fn the_registered_agents_resolve_through_the_current_state() {
    let default_id = registry::default_agent().id();
    assert_eq!(resolve_spec_agent(&spec(None, None), None), Ok(default_id.to_string()));
    assert_eq!(resolve_spec_agent(&spec(Some(default_id), None), Some("gone")), Ok(default_id.to_string()));
    assert_eq!(resolve_spec_agent(&spec(None, None), Some("gone")), Err("task 't1': unknown agent: gone".to_string()));
}

// --- Bot ---

/// I Bot predefiniti, con Intake abilitato anche ai task `work` (come puo'
/// configurarlo l'utente): e' il Bot di prova con modello, effort e skill.
fn bots() -> Vec<AgentBot> {
    let mut list = crate::bots::default_agent_bots();
    bot_mut(&mut list, "intake").kinds = vec![TaskKind::Work];
    list
}

fn kind_spec(bot: &str, kind: TaskKind) -> TaskSpec {
    TaskSpec { kind, ..bot_spec(bot) }
}

fn bot_spec(bot: &str) -> TaskSpec {
    TaskSpec { id: "t1".into(), bot: Some(bot.into()), ..TaskSpec::default() }
}

/// Risoluzione di una specifica con Bot: l'agente predefinito piu' `second`,
/// entrambi abilitati, il predefinito globale e' il primo.
fn resolve_with(
    second: &FakeIntegration,
    spec: &TaskSpec,
    bots: &[AgentBot],
    workspace_agent: Option<&str>,
) -> Result<TaskSpec, String> {
    let registered = [registry::all()[0], second as &dyn AgentIntegration];
    let enabled = vec![first_id().to_string(), SECOND.to_string()];
    let state = IntegrationsState { enabled: enabled.clone(), default_agent: first_id().into() };
    let catalog = AgentCatalog::new(&registered, enabled);
    let env = AgentEnv {
        resolve: &|explicit, workspace, purpose| resolve_in(&registered, &state, explicit, workspace, purpose),
        catalog: &catalog,
        default_agent: first_id(),
    };
    resolve_with_bot(&env, spec, spec.bot.as_deref().unwrap(), SpecContext { workspace_agent, bots })
}

fn resolve_bot_spec(spec: &TaskSpec, bots: &[AgentBot], workspace_agent: Option<&str>) -> Result<TaskSpec, String> {
    resolve_with(&FakeIntegration::full(SECOND), spec, bots, workspace_agent)
}

fn bot_mut<'a>(bots: &'a mut [AgentBot], id: &str) -> &'a mut AgentBot {
    bots.iter_mut().find(|b| b.id == id).unwrap()
}

#[test]
fn a_bot_is_copied_into_the_spec_with_its_agent_model_effort_and_skill() {
    let resolved = resolve_bot_spec(&bot_spec("intake"), &bots(), Some(SECOND)).unwrap();
    assert_eq!(resolved.agent.as_deref(), Some(SECOND));
    assert_eq!(resolved.model.as_deref(), Some(FAKE_MODEL_LARGE));
    assert_eq!(resolved.effort, Some(Effort::High));
    assert_eq!(resolved.skill, Some(SkillRef::Plugin("task-intake".into())));
    assert_eq!(resolved.bot.as_deref(), Some("intake"));
}

#[test]
fn the_general_bot_follows_the_workspace_agent_then_the_global_default() {
    let on_workspace = resolve_bot_spec(&bot_spec("general"), &bots(), Some(SECOND)).unwrap();
    assert_eq!((on_workspace.agent.as_deref(), on_workspace.model, on_workspace.effort), (Some(SECOND), None, None));
    let on_default = resolve_bot_spec(&bot_spec("general"), &bots(), None).unwrap();
    assert_eq!(on_default.agent.as_deref(), Some(first_id()));
    assert_eq!(on_default.skill, None);
}

#[test]
fn an_unknown_bot_is_an_error() {
    let err = resolve_bot_spec(&bot_spec("nope"), &bots(), None);
    assert_eq!(err, Err("task 't1': unknown bot: nope (see get_workspace_settings.bots)".to_string()));
}

#[test]
fn a_disabled_bot_is_an_error() {
    let mut list = bots();
    bot_mut(&mut list, "intake").enabled = false;
    let err = resolve_bot_spec(&bot_spec("intake"), &list, None);
    assert_eq!(err, Err("task 't1': Intake is disabled: enable it in the bots settings".to_string()));
}

#[test]
fn the_preset_bots_that_start_disabled_cannot_be_proposed_until_enabled() {
    let mut list = bots();
    for (id, name) in [("git-operator", "Git Operator"), ("release", "Release")] {
        let err = resolve_bot_spec(&bot_spec(id), &list, Some(SECOND));
        assert_eq!(err, Err(format!("task 't1': {name} is disabled: enable it in the bots settings")), "{id}");
    }
    bot_mut(&mut list, "git-operator").enabled = true;
    assert!(resolve_bot_spec(&bot_spec("git-operator"), &list, Some(SECOND)).is_ok());
}

#[test]
fn a_bot_runs_only_the_task_kinds_it_allows() {
    let presets = crate::bots::default_agent_bots();
    let refused = |name: &str, kind: &str| {
        format!(
            "task 't1': bot {name} cannot run {kind} tasks: choose a bot whose kinds include {kind} (see \
             get_workspace_settings.bots)"
        )
    };
    let run = |bot: &str, kind| resolve_bot_spec(&kind_spec(bot, kind), &presets, Some(SECOND)).map(drop);
    assert_eq!(run("general", TaskKind::Review), Err(refused("General", "review")));
    assert_eq!(run("general", TaskKind::Plan), Err(refused("General", "plan")));
    assert_eq!(run("intake", TaskKind::Work), Err(refused("Intake", "work")));
    assert_eq!(run("planner", TaskKind::Review), Err(refused("Planner", "review")));
    for (bot, kind) in [("general", TaskKind::Work), ("reviewer", TaskKind::Review), ("planner", TaskKind::Plan)] {
        assert_eq!(run(bot, kind), Ok(()), "{bot}");
    }
}

#[test]
fn a_general_bot_given_more_kinds_runs_them() {
    let mut list = bots();
    bot_mut(&mut list, "general").kinds = TaskKind::ALL.to_vec();
    for kind in TaskKind::ALL {
        assert!(resolve_bot_spec(&kind_spec("general", kind), &list, Some(SECOND)).is_ok(), "{kind:?}");
    }
}

#[test]
fn the_kind_is_checked_against_the_bot_without_resolving_it_again() {
    let presets = crate::bots::default_agent_bots();
    assert_eq!(check_bot_kind(&kind_spec("reviewer", TaskKind::Review), &presets), Ok(()));
    let err = check_bot_kind(&kind_spec("general", TaskKind::Review), &presets).unwrap_err();
    assert!(err.starts_with("task 't1': bot General cannot run review tasks"), "{err}");
    assert!(check_bot_kind(&kind_spec("gone", TaskKind::Work), &presets).unwrap_err().contains("unknown bot: gone"));
    assert_eq!(check_bot_kind(&TaskSpec { kind: TaskKind::Review, ..spec(None, None) }, &presets), Ok(()));
    // Un Bot disabilitato dopo la copia non blocca il cambio di tipo.
    let mut off = presets.clone();
    bot_mut(&mut off, "reviewer").enabled = false;
    assert_eq!(check_bot_kind(&kind_spec("reviewer", TaskKind::Review), &off), Ok(()));
}

#[test]
fn the_model_of_the_spec_overrides_the_model_of_the_bot() {
    let spec = TaskSpec { model: Some(FAKE_MODEL_SMALL.into()), ..bot_spec("intake") };
    let resolved = resolve_bot_spec(&spec, &bots(), Some(SECOND)).unwrap();
    assert_eq!(resolved.model.as_deref(), Some(FAKE_MODEL_SMALL));
    assert_eq!(resolved.effort, Some(Effort::High), "the rest still comes from the bot");

    let wrong = TaskSpec { model: Some(FOREIGN_MODEL.into()), ..bot_spec("intake") };
    let expected = format!("task 't1': invalid model for Fake: {FOREIGN_MODEL} (see get_workspace_settings.agents)");
    assert_eq!(resolve_bot_spec(&wrong, &bots(), Some(SECOND)), Err(expected));
}

#[test]
fn an_agent_in_the_spec_must_be_the_agent_of_the_bot() {
    let same = TaskSpec { agent: Some(SECOND.into()), ..bot_spec("general") };
    assert!(resolve_bot_spec(&same, &bots(), Some(SECOND)).is_ok());

    let other = TaskSpec { agent: Some(first_id().into()), ..bot_spec("general") };
    let expected = format!(
        "task 't1': agent {} does not match the agent of bot general ({SECOND}): omit agent or choose another bot",
        first_id()
    );
    assert_eq!(resolve_bot_spec(&other, &bots(), Some(SECOND)), Err(expected));

    let mut list = bots();
    bot_mut(&mut list, "general").agent = Some(first_id().into());
    let fixed = TaskSpec { agent: Some(SECOND.into()), ..bot_spec("general") };
    assert!(
        resolve_bot_spec(&fixed, &list, Some(SECOND)).is_err(),
        "the fixed agent of the bot wins over the workspace"
    );
}

#[test]
fn an_unusable_agent_of_the_bot_is_an_error_prefixed_with_the_task() {
    let gone = resolve_bot_spec(&bot_spec("general"), &bots(), Some("gone"));
    assert_eq!(gone, Err("task 't1': unknown agent: gone".to_string()));

    let bare = FakeIntegration::bare(SECOND, "fake-cli");
    let unsupported = resolve_with(&bare, &bot_spec("general"), &bots(), Some(SECOND));
    assert_eq!(unsupported, Err("task 't1': Fake does not support automated sessions".to_string()));
}

#[test]
fn a_skill_the_agent_cannot_load_rejects_the_bot() {
    let no_plugin = FakeIntegration::full(SECOND).without(Capability::PluginSkills);
    let err = resolve_with(&no_plugin, &bot_spec("intake"), &bots(), Some(SECOND));
    let expected = "task 't1': bot intake needs the skill task-intake, which Fake cannot load: choose another bot";
    assert_eq!(err, Err(expected.to_string()));

    let mut list = bots();
    bot_mut(&mut list, "general").skill = Some(SkillRef::User("my-skill".into()));
    let no_user = FakeIntegration::full(SECOND).without(Capability::UserSkills);
    assert!(resolve_with(&no_user, &bot_spec("general"), &list, Some(SECOND)).is_err());
    let user = resolve_bot_spec(&bot_spec("general"), &list, Some(SECOND)).unwrap();
    assert_eq!(user.skill, Some(SkillRef::User("my-skill".into())));
}

#[test]
fn the_effort_is_copied_raw_even_when_the_agent_cannot_apply_it() {
    let mut list = bots();
    bot_mut(&mut list, "general").effort = Some(Effort::Max);
    let resolved = resolve_bot_spec(&bot_spec("general"), &list, Some(SECOND)).unwrap();
    assert_eq!(resolved.effort, Some(Effort::Max), "the start drops it with a warning in the note");
}

#[test]
fn a_spec_without_a_bot_resolves_as_before_with_no_effort_or_skill() {
    let stale = TaskSpec { effort: Some(Effort::High), skill: Some(SkillRef::User("x".into())), ..spec(None, None) };
    let resolved = resolve_spec(&stale, SpecContext { workspace_agent: None, bots: &bots() }).unwrap();
    assert_eq!(resolved.agent.as_deref(), Some(registry::default_agent().id()));
    assert_eq!((resolved.bot, resolved.effort, resolved.skill), (None, None, None));
}
