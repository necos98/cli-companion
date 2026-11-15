use super::*;
use crate::agents::testing::{FakeIntegration, FAKE_MODEL_ENV, FAKE_MODEL_LARGE};
use crate::board::TaskKind;
use crate::bots::agent::ModelChoice;
use crate::bots::presets::{self, GENERAL_ID};

const FULL: &str = "fake-full";
const BARE: &str = "fake-bare";
const OFF: &str = "fake-off";
/// Un agente che non applica istruzioni di sistema.
const PLAIN: &str = "fake-plain";
/// Il file dello standard come lo prepara l'app.
const STANDARD: &str = "C:/data/bot-instructions/engineering-standard.md";

fn standard() -> StandardFile {
    Ok(STANDARD.to_string())
}

fn with_catalog<R>(f: impl FnOnce(&AgentCatalog) -> R) -> R {
    let full = FakeIntegration::full(FULL);
    let bare = FakeIntegration::bare(BARE, "fake-cli");
    let off = FakeIntegration::full(OFF);
    let plain = FakeIntegration::full(PLAIN).without_instructions();
    let registered = [&full as &dyn AgentIntegration, &bare, &off, &plain];
    f(&AgentCatalog::new(&registered, vec![FULL.into(), BARE.into(), PLAIN.into()]))
}

fn preset(id: &str) -> AgentBot {
    presets::find(id).unwrap().bot()
}

fn build(bot: &AgentBot, workspace: &str) -> Result<(BotTabLaunch, Vec<(String, String)>), String> {
    with_catalog(|c| build_bot_launch(bot, workspace, c, |name| name == "my-skill", &standard()))
}

#[test]
fn only_enabled_bots_are_listed_in_order_with_the_prefix() {
    let off = AgentBot { enabled: false, ..preset("intake") };
    let bots = [preset(GENERAL_ID), off, preset("setup")];
    let list = with_catalog(|c| bot_launches(&bots, FULL, c));
    let ids: Vec<_> = list.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["bot:general", "bot:setup"]);
    assert_eq!(list[0].bot_id, "general");
    assert_eq!(list[0].name, "General");
}

#[test]
fn a_launchable_bot_shows_its_effective_agent() {
    let list = with_catalog(|c| bot_launches(&[preset(GENERAL_ID)], FULL, c));
    assert_eq!(
        list[0],
        BotLaunch {
            id: "bot:general".into(),
            bot_id: "general".into(),
            name: "General".into(),
            agent_id: Some(FULL.into()),
            agent_name: Some("Fake".into()),
            blocked_reason: None,
        }
    );
}

#[test]
fn an_unusable_agent_blocks_the_entry_in_italian() {
    let disabled = AgentBot { agent: Some(OFF.into()), ..preset(GENERAL_ID) };
    let unknown = AgentBot { agent: Some("gone".into()), ..preset("setup") };
    let list = with_catalog(|c| bot_launches(&[disabled, unknown], FULL, c));
    assert_eq!(list[0].blocked_reason.as_deref(), Some("Fake è disabilitato: abilitalo nelle Integrazioni."));
    assert_eq!(list[1].blocked_reason.as_deref(), Some("L'agente «gone» del Bot non è registrato."));
    for entry in &list {
        assert_eq!((&entry.agent_id, &entry.agent_name), (&None, &None));
    }
}

#[test]
fn an_agent_without_the_contract_blocks_but_keeps_its_identity() {
    let list = with_catalog(|c| bot_launches(&[preset(GENERAL_ID)], BARE, c));
    assert_eq!(list[0].blocked_reason.as_deref(), Some("Fake non supporta l'avvio dei Bot."));
    assert_eq!(list[0].agent_id.as_deref(), Some(BARE));
    assert_eq!(list[0].agent_name.as_deref(), Some("Fake"));
}

#[test]
fn a_bot_becomes_the_command_of_its_agent_with_model_effort_and_skill() {
    let (tab, env) = build(&preset("intake"), FULL).unwrap();
    assert_eq!(tab.name, "Intake");
    assert_eq!(tab.agent_id, FULL);
    assert_eq!(tab.command, format!("fake --yes --model {FAKE_MODEL_LARGE} --think high \"/fake-plugin:task-intake\""));
    assert_eq!(tab.restore_command, format!("fake --yes --model {FAKE_MODEL_LARGE} --think high"));
    assert!(tab.warnings.is_empty() && env.is_empty());
}

#[test]
fn what_the_agent_cannot_apply_comes_back_as_warnings() {
    let (tab, _) = build(&AgentBot { skill: None, ..preset("setup") }, FULL).unwrap();
    assert_eq!(tab.warnings, ["Fake does not support the effort level \"medium\": ignored"]);
    assert!(!tab.command.contains("--think"));
}

#[test]
fn a_missing_user_skill_fails_instead_of_falling_back() {
    let bot = |name: &str| AgentBot { skill: Some(SkillRef::User(name.into())), ..preset(GENERAL_ID) };
    let (tab, _) = build(&bot("my-skill"), FULL).unwrap();
    assert_eq!(tab.command, format!("fake --yes --system-file \"{STANDARD}\" \"/my-skill\""));
    assert_eq!(
        build(&bot("gone"), FULL).err().as_deref(),
        Some("La skill «gone» del Bot «General» non esiste più: sceglila di nuovo nelle impostazioni del Bot.")
    );
}

#[test]
fn unsafe_skill_names_are_refused_with_a_message_for_the_user() {
    let bot = AgentBot { skill: Some(SkillRef::User("a b".into())), ..preset(GENERAL_ID) };
    let error = with_catalog(|c| build_bot_launch(&bot, FULL, c, |_| true, &standard())).err().unwrap();
    assert!(error.contains("caratteri non ammessi"), "{error}");
}

#[test]
fn a_model_the_agent_knows_is_passed_through() {
    let bot = AgentBot { agent: Some(FULL.into()), model: ModelChoice::Id("fake-small".into()), ..preset(GENERAL_ID) };
    assert_eq!(
        build(&bot, BARE).unwrap().0.command,
        format!("fake --yes --model fake-small --system-file \"{STANDARD}\"")
    );
}

#[test]
fn disabled_bots_and_unusable_agents_do_not_launch() {
    let off = AgentBot { enabled: false, ..preset(GENERAL_ID) };
    assert_eq!(build(&off, FULL).err().as_deref(), Some("Il Bot «General» è disabilitato."));
    assert_eq!(build(&preset(GENERAL_ID), "gone").err().as_deref(), Some("L'agente «gone» del Bot non è registrato."));
    assert_eq!(build(&preset(GENERAL_ID), BARE).err().as_deref(), Some("Fake non supporta l'avvio dei Bot."));
    let disabled = AgentBot { agent: Some(OFF.into()), ..preset(GENERAL_ID) };
    assert_eq!(build(&disabled, FULL).err().as_deref(), Some("Fake è disabilitato: abilitalo nelle Integrazioni."));
}

fn restore(bot: Option<&AgentBot>, saved_agent: &str, workspace: &str) -> RestoredEnv {
    with_catalog(|c| build_bot_restore(bot, saved_agent, workspace, c, &standard()))
}

#[test]
fn a_restored_tab_gets_the_environment_of_the_bot_model() {
    let (env, warning) = restore(Some(&preset("intake")), FULL, FULL);
    assert_eq!(env, [(FAKE_MODEL_ENV.to_string(), FAKE_MODEL_LARGE.to_string())]);
    assert_eq!(warning, None);
}

#[test]
fn a_restored_tab_without_a_model_has_no_environment() {
    let bot = AgentBot { model: ModelChoice::Default, ..preset(GENERAL_ID) };
    assert_eq!(restore(Some(&bot), FULL, FULL), (Vec::new(), None));
}

#[test]
fn the_restore_never_needs_the_skill_to_exist() {
    let bot = AgentBot { skill: Some(SkillRef::User("gone".into())), ..preset("intake") };
    let (env, warning) = restore(Some(&bot), FULL, FULL);
    assert_eq!((env.len(), warning), (1, None));
}

#[test]
fn a_restored_tab_whose_bot_changed_still_opens_with_a_warning() {
    let other_agent = AgentBot { agent: Some(BARE.into()), ..preset("intake") };
    let unknown = AgentBot { agent: Some("gone".into()), ..preset("intake") };
    let cases = [
        (None, FULL, "Il Bot di questo terminale non esiste più."),
        (Some(&other_agent), FULL, "Il Bot «Intake» ora usa un altro agente."),
        (Some(&unknown), FULL, "L'agente del Bot «Intake» non è più disponibile."),
        (Some(&preset("intake")), BARE, "Il Bot «Intake» ora usa un altro agente."),
    ];
    for (bot, saved_agent, reason) in cases {
        let (env, warning) = restore(bot, saved_agent, FULL);
        assert!(env.is_empty(), "{reason}");
        assert_eq!(
            warning.as_deref(),
            Some(format!("{reason} Il terminale riparte con il comando salvato, senza le scelte del Bot.").as_str())
        );
    }
}

#[test]
fn a_model_that_is_no_longer_valid_restores_without_environment_and_warns() {
    let bot = AgentBot { model: ModelChoice::Id("a b;x".into()), ..preset(GENERAL_ID) };
    let (env, warning) = restore(Some(&bot), FULL, FULL);
    assert!(env.is_empty());
    assert!(warning.unwrap().starts_with("Il modello del Bot «General» non è più disponibile."));
}

#[test]
fn a_work_bot_gets_the_interactive_standard_also_on_restore() {
    let (tab, env) = build(&preset(GENERAL_ID), FULL).unwrap();
    let flag = format!("--system-file \"{STANDARD}\"");
    assert!(tab.command.contains(&flag) && tab.restore_command.contains(&flag), "{tab:?}");
    assert!(tab.warnings.is_empty() && env.is_empty());
    for id in ["bug-fixer", "refactorer", "test-writer", "docs-writer", "git-operator", "release"] {
        let bot = AgentBot { enabled: true, ..preset(id) };
        assert!(build(&bot, FULL).unwrap().0.command.contains(&flag), "{id}");
    }
}

#[test]
fn bots_without_work_never_get_the_standard() {
    for id in ["intake", "setup", "skill-wizard"] {
        let (tab, _) = build(&preset(id), FULL).unwrap();
        assert!(!tab.command.contains("--system-file") && !tab.restore_command.contains("--system-file"), "{id}");
    }
    for kinds in [vec![TaskKind::Review], vec![TaskKind::Plan], vec![TaskKind::Review, TaskKind::Plan]] {
        let bot = AgentBot { kinds, ..preset(GENERAL_ID) };
        assert!(!build(&bot, FULL).unwrap().0.command.contains("--system-file"));
    }
}

#[test]
fn an_agent_without_instructions_launches_the_work_bot_with_a_warning() {
    let bot = AgentBot { agent: Some(PLAIN.into()), ..preset(GENERAL_ID) };
    let (tab, _) = build(&bot, FULL).unwrap();
    assert!(!tab.command.contains("--system-file"));
    assert_eq!(tab.warnings, ["Fake does not support system instructions: the engineering standard is not applied"]);
    // Nessun avviso per un Bot che lo standard non lo riceve comunque.
    let intake = AgentBot { agent: Some(PLAIN.into()), ..preset("intake") };
    assert!(build(&intake, FULL).unwrap().0.warnings.is_empty());
}

#[test]
fn a_standard_file_that_could_not_be_prepared_launches_with_a_warning() {
    let failed: StandardFile = Err("writing C:/data: denied".into());
    let (tab, _) = with_catalog(|c| build_bot_launch(&preset(GENERAL_ID), FULL, c, |_| true, &failed)).unwrap();
    assert!(!tab.command.contains("--system-file"));
    assert_eq!(tab.warnings, ["the engineering standard could not be prepared (writing C:/data: denied): not applied"]);
    let with_model =
        AgentBot { agent: Some(FULL.into()), model: ModelChoice::Id("fake-small".into()), ..preset(GENERAL_ID) };
    let (env, warning) = with_catalog(|c| build_bot_restore(Some(&with_model), FULL, FULL, c, &failed));
    assert_eq!(env, [(FAKE_MODEL_ENV.to_string(), "fake-small".to_string())], "the model is still restored");
    assert_eq!(
        warning.as_deref(),
        Some("Lo standard di ingegneria del Bot «General» non si è potuto preparare (writing C:/data: denied): il terminale riparte senza.")
    );
}

#[test]
fn the_restore_asks_the_agent_for_the_standard_of_a_work_bot_only() {
    // L'agente finto porta il file nella riga: il ripristino lo chiede e non avvisa.
    assert_eq!(restore(Some(&preset(GENERAL_ID)), FULL, FULL).1, None);
    let plain = AgentBot { agent: Some(PLAIN.into()), ..preset(GENERAL_ID) };
    assert_eq!(restore(Some(&plain), PLAIN, FULL).1, None, "the launch already warned");
}
