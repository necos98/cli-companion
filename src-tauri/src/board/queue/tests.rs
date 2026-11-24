//! L'agente dei task nelle operazioni della coda. Il registro dei test ha un
//! solo agente (il predefinito): le scelte fra piu' agenti sono provate in
//! `agent/tests.rs`, qui contano quando e dove si risolve e si salva.

use super::*;
use crate::agents::registry;
use crate::board::{parse_proposal, TaskKind};
use serde_json::json;

/// Id che nessun agente registrato ha.
const GONE: &str = "gone";

fn default_id() -> &'static str {
    registry::default_agent().id()
}

fn spec(id: &str, agent: Option<&str>) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        agent: agent.map(str::to_string),
        ..TaskSpec::default()
    }
}

fn agent_of(b: &Board, id: &str) -> String {
    b.task(id).unwrap().agent.clone()
}

#[test]
fn a_created_task_saves_the_resolved_agent() {
    let mut b = Board::new("ws-1");
    b.create_task(spec("explicit", Some(default_id())), SpecContext::workspace(Some(GONE)), 1).unwrap();
    b.create_task(spec("workspace", None), SpecContext::workspace(Some(default_id())), 2).unwrap();
    b.create_task(spec("global", None), SpecContext::default(), 3).unwrap();
    for id in ["explicit", "workspace", "global"] {
        assert_eq!(agent_of(&b, id), default_id(), "{id}");
    }
}

#[test]
fn an_unusable_agent_rejects_the_creation_with_a_clear_error() {
    let mut b = Board::new("ws-1");
    let explicit = b.create_task(spec("a", Some(GONE)), SpecContext::default(), 1).unwrap_err();
    assert_eq!(explicit, "task 'a': unknown agent: gone");
    let from_workspace = b.create_task(spec("b", None), SpecContext::workspace(Some(GONE)), 1).unwrap_err();
    assert_eq!(from_workspace, "task 'b': unknown agent: gone");
    assert!(b.tasks.is_empty());
}

#[test]
fn a_model_the_agent_does_not_accept_rejects_the_creation() {
    let mut b = Board::new("ws-1");
    let bad = TaskSpec { model: Some("a b".into()), ..spec("a", None) };
    let expected = format!("task 'a': invalid model for {}: a b", registry::default_agent().display_name());
    assert!(b.create_task(bad, SpecContext::default(), 1).unwrap_err().starts_with(&expected));
}

#[test]
fn a_proposal_keeps_the_agent_as_written_and_resolves_it_when_accepted() {
    let mut b = Board::new("ws-1");
    let specs = vec![spec("plain", None), spec("explicit", Some(default_id()))];
    b.add_proposal("p-1".into(), "tab-1", None, specs, SpecContext::default(), 1).unwrap();
    let kept: Vec<_> = b.proposal("p-1").unwrap().tasks.iter().map(|t| t.agent.clone()).collect();
    assert_eq!(kept, [None, Some(default_id().to_string())]);

    // Vale il workspace al momento dell'accettazione: se non e' utilizzabile la proposta resta.
    let err = b.accept_proposal("p-1", SpecContext::workspace(Some(GONE)), 2).unwrap_err();
    assert_eq!(err, "task 'plain': unknown agent: gone");
    assert_eq!(b.proposals.len(), 1);
    assert!(b.tasks.is_empty());

    b.accept_proposal("p-1", SpecContext::workspace(Some(default_id())), 3).unwrap();
    assert_eq!(agent_of(&b, "plain"), default_id());
    assert_eq!(agent_of(&b, "explicit"), default_id());
}

#[test]
fn a_proposal_with_an_unusable_agent_is_rejected_when_proposed() {
    let mut b = Board::new("ws-1");
    let err = b
        .add_proposal("p-1".into(), "tab-1", None, vec![spec("a", Some(GONE))], SpecContext::default(), 1)
        .unwrap_err();
    assert_eq!(err, "task 'a': unknown agent: gone");
    assert!(b.proposals.is_empty());
}

#[test]
fn the_saved_agent_does_not_follow_later_changes_of_the_defaults() {
    let mut b = Board::new("ws-1");
    b.create_task(spec("a", None), SpecContext::default(), 1).unwrap();
    // Il workspace ora ha un agente inutilizzabile: il task non lo guarda
    // finche' non si toccano agente o modello.
    b.update_task("a", &json!({"title": "Renamed"}), SpecContext::workspace(Some(GONE)), 2).unwrap();
    assert_eq!(b.task("a").unwrap().title, "Renamed");
    assert_eq!(agent_of(&b, "a"), default_id());
}

#[test]
fn a_task_whose_agent_disappeared_can_be_edited_but_not_re_modelled() {
    let mut b = Board::new("ws-1");
    b.create_task(spec("a", None), SpecContext::default(), 1).unwrap();
    b.task_mut("a").unwrap().agent = GONE.into();
    b.update_task("a", &json!({"title": "Still editable"}), SpecContext::default(), 2).unwrap();
    assert_eq!(agent_of(&b, "a"), GONE);
    let err = b.update_task("a", &json!({"model": "other-model"}), SpecContext::default(), 3).unwrap_err();
    assert_eq!(err, "task 'a': unknown agent: gone");
    assert_eq!(b.task("a").unwrap().model, None);
}

#[test]
fn updating_the_agent_validates_it_and_null_goes_back_to_the_defaults() {
    let mut b = Board::new("ws-1");
    b.create_task(spec("a", None), SpecContext::default(), 1).unwrap();
    b.task_mut("a").unwrap().agent = GONE.into();

    let unknown = b.update_task("a", &json!({"agent": "nope"}), SpecContext::default(), 2).unwrap_err();
    assert_eq!(unknown, "task 'a': unknown agent: nope");
    assert_eq!(agent_of(&b, "a"), GONE);

    // `null`: l'agente del workspace, risolto adesso (qui inutilizzabile).
    let bad_workspace = b.update_task("a", &json!({"agent": null}), SpecContext::workspace(Some(GONE)), 3).unwrap_err();
    assert_eq!(bad_workspace, "task 'a': unknown agent: gone");
    b.update_task("a", &json!({"agent": null}), SpecContext::default(), 4).unwrap();
    assert_eq!(agent_of(&b, "a"), default_id());

    b.task_mut("a").unwrap().agent = GONE.into();
    b.update_task("a", &json!({"agent": default_id()}), SpecContext::workspace(Some(GONE)), 5).unwrap();
    assert_eq!(agent_of(&b, "a"), default_id());
}

#[test]
fn a_model_in_an_update_is_checked_against_the_agent_of_the_task() {
    let mut b = Board::new("ws-1");
    b.create_task(spec("a", None), SpecContext::default(), 1).unwrap();
    b.update_task("a", &json!({"model": "other-model"}), SpecContext::workspace(Some(GONE)), 2).unwrap();
    assert_eq!(b.task("a").unwrap().model.as_deref(), Some("other-model"));
    assert_eq!(agent_of(&b, "a"), default_id());
}

// --- Bot ---

/// I Bot predefiniti, con Intake e Setup abilitati anche ai task `work` (come puo'
/// configurarli l'utente): sono i Bot di prova con effort e skill.
fn bots() -> Vec<crate::bots::AgentBot> {
    let mut list = crate::bots::default_agent_bots();
    list.iter_mut()
        .filter(|b| ["intake", "setup"].contains(&b.id.as_str()))
        .for_each(|b| b.kinds = vec![TaskKind::Work]);
    list
}

fn with_bot(id: &str, bot: &str) -> TaskSpec {
    TaskSpec { bot: Some(bot.into()), ..spec(id, None) }
}

#[test]
fn a_task_created_with_a_bot_keeps_the_copy_and_survives_the_bot_changing() {
    let mut list = bots();
    let mut b = Board::new("ws-1");
    b.create_task(with_bot("a", "intake"), SpecContext { workspace_agent: None, bots: &list }, 1).unwrap();
    let t = b.task("a").unwrap().clone();
    assert_eq!((t.bot.as_deref(), t.effort), (Some("intake"), Some(crate::agents::Effort::High)));
    assert!(t.skill.is_some() && t.model.is_some());

    list.retain(|bot| bot.id != "intake");
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    // Il titolo non rilegge il Bot, nemmeno se e' sparito.
    b.update_task("a", &json!({"title": "Renamed"}), ctx, 2).unwrap();
    let after = b.task("a").unwrap();
    assert_eq!(
        (after.bot.clone(), after.effort, after.skill.clone(), after.model.clone()),
        (t.bot, t.effort, t.skill, t.model)
    );
    // Cambiare il Bot invece lo rilegge.
    let err = b.update_task("a", &json!({"bot": "intake"}), ctx, 3).unwrap_err();
    assert_eq!(err, "task 'a': unknown bot: intake (see get_workspace_settings.bots)");
}

#[test]
fn a_patch_with_a_bot_drops_the_old_agent_and_model_unless_it_writes_them() {
    let list = bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = Board::new("ws-1");
    b.create_task(TaskSpec { model: Some("sonnet".into()), ..with_bot("a", "general") }, ctx, 1).unwrap();
    b.update_task("a", &json!({"bot": "intake"}), ctx, 2).unwrap();
    assert_ne!(b.task("a").unwrap().model.as_deref(), Some("sonnet"), "the model of the new bot");
    b.update_task("a", &json!({"bot": "setup", "model": "opus"}), ctx, 3).unwrap();
    assert_eq!(b.task("a").unwrap().model.as_deref(), Some("opus"));
    assert_eq!(b.task("a").unwrap().bot.as_deref(), Some("setup"));
}

#[test]
fn a_task_without_a_bot_edits_exactly_as_before() {
    let list = bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = Board::new("ws-1");
    b.create_task(spec("a", None), ctx, 1).unwrap();
    b.update_task("a", &json!({"model": "opus"}), ctx, 2).unwrap();
    let t = b.task("a").unwrap();
    assert_eq!((t.model.as_deref(), &t.bot, &t.effort, &t.skill), (Some("opus"), &None, &None, &None));
}

// --- Tipi di task dei Bot ---

fn proposal(kind: &str, bot: &str) -> Vec<TaskSpec> {
    parse_proposal(&json!({"tasks": [
        {"id": "p", "title": "T", "task": "x", "bot": bot, "kind": kind, "depends_on": ["a"]}
    ]}))
    .unwrap()
}

/// Una board con il task `a` (Bot `general`) da cui le proposte dipendono.
fn board_with_a(ctx: SpecContext<'_>) -> Board {
    let mut b = Board::new("ws-1");
    b.create_task(with_bot("a", "general"), ctx, 1).unwrap();
    b
}

#[test]
fn a_proposal_needs_a_bot_that_runs_the_kind_of_each_task() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let b = board_with_a(ctx);
    let refused = |kind: &str, bot: &str| b.check_specs(&proposal(kind, bot), &HashSet::new(), ctx).unwrap_err();
    assert_eq!(
        refused("review", "general"),
        "task 'p': bot General cannot run review tasks: choose a bot whose kinds include review (see \
         get_workspace_settings.bots)"
    );
    assert!(refused("work", "intake").starts_with("task 'p': bot Intake cannot run work tasks"));
    assert!(b.check_specs(&proposal("review", "reviewer"), &HashSet::new(), ctx).is_ok());
    assert!(b.check_specs(&proposal("work", "general"), &HashSet::new(), ctx).is_ok());
}

#[test]
fn changing_the_kind_of_a_task_checks_its_bot_again() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_a(ctx);
    b.create_task(TaskSpec { depends_on: vec!["a".into()], ..with_bot("b", "general") }, ctx, 2).unwrap();
    let err = b.update_task("b", &json!({"kind": "review"}), ctx, 3).unwrap_err();
    assert!(err.starts_with("task 'b': bot General cannot run review tasks"), "{err}");
    assert_eq!(b.task("b").unwrap().kind, TaskKind::Work, "a refused change changes nothing");
    // Il Bot nuovo nello stesso patch si verifica con il tipo nuovo.
    b.update_task("b", &json!({"kind": "review", "bot": "reviewer"}), ctx, 4).unwrap();
    assert_eq!((b.task("b").unwrap().kind, b.task("b").unwrap().bot.as_deref()), (TaskKind::Review, Some("reviewer")));
    assert!(b.update_task("b", &json!({"bot": "general"}), ctx, 5).unwrap_err().contains("cannot run review"));
}

#[test]
fn a_task_without_a_bot_changes_kind_freely_and_other_edits_skip_the_kind_check() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_a(ctx);
    b.create_task(TaskSpec { depends_on: vec!["a".into()], ..spec("b", None) }, ctx, 2).unwrap();
    b.update_task("b", &json!({"kind": "review"}), ctx, 3).unwrap();
    // Un task gia' in coda con un Bot che oggi non puo' eseguirlo si modifica comunque.
    let mut narrowed = list.clone();
    narrowed.iter_mut().filter(|bot| bot.id == "general").for_each(|bot| bot.kinds.clear());
    let ctx = SpecContext { workspace_agent: None, bots: &narrowed };
    b.update_task("a", &json!({"title": "Renamed"}), ctx, 4).unwrap();
}
