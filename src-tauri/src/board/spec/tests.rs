use super::*;
use serde_json::json;

/// I tipi dei comandi `task_create` e `task_update` usano solo chiavi che `parse_spec` legge.
#[test]
fn command_arguments_are_read_by_parse_spec() {
    let new = NewTaskSpec {
        title: "T".into(),
        task: "do it".into(),
        depends_on: Some(vec!["a".into()]),
        repo: Some("api".into()),
        agent: Some("agent-a".into()),
        model: Some("opus".into()),
        bot: Some("intake".into()),
        kind: Some(TaskKind::Review),
        e2e: Some(true),
    };
    let spec = parse_spec(&serde_json::to_value(&new).unwrap(), None).unwrap();
    assert_eq!((spec.title.as_str(), spec.kind, spec.e2e), ("T", TaskKind::Review, true));
    assert_eq!(spec.agent.as_deref(), Some("agent-a"));
    assert_eq!(spec.bot.as_deref(), Some("intake"));

    let patch = TaskPatch {
        title: Some("U".into()),
        task: Some("again".into()),
        depends_on: Some(vec![]),
        repo: Some("web".into()),
        agent: Some("agent-b".into()),
        model: Some("sonnet".into()),
        bot: Some("setup".into()),
        kind: Some(TaskKind::Work),
        e2e: Some(false),
    };
    let updated = parse_spec(&serde_json::to_value(&patch).unwrap(), Some(&spec)).unwrap();
    assert_eq!((updated.title.as_str(), updated.repo.as_deref(), updated.kind), ("U", Some("web"), TaskKind::Work));
    assert_eq!(updated.agent.as_deref(), Some("agent-b"));
    assert_eq!(updated.bot.as_deref(), Some("setup"));
    assert_eq!(serde_json::to_value(TaskPatch::default()).unwrap(), json!({}));
}

fn bot_of(v: serde_json::Value, base: Option<&TaskSpec>) -> Result<Option<String>, String> {
    parse_spec(&v, base).map(|s| s.bot)
}

#[test]
fn the_bot_is_trimmed_and_empty_or_null_means_none() {
    let create = |bot: serde_json::Value| bot_of(json!({"title": "T", "task": "x", "bot": bot}), None);
    assert_eq!(create(json!(" intake ")), Ok(Some("intake".into())));
    assert_eq!(create(json!("")), Ok(None));
    assert_eq!(create(json!(null)), Ok(None));
    assert_eq!(bot_of(json!({"title": "T", "task": "x"}), None), Ok(None));
}

#[test]
fn a_bot_id_keeps_the_shape_of_the_ids_bots_can_have() {
    let long_user_id = format!("custom-{}-99", "a".repeat(32));
    assert!(long_user_id.len() > ID_MAX);
    assert_eq!(bot_of(json!({"title": "T", "task": "x", "bot": long_user_id}), None), Ok(Some(long_user_id)));
    for bad in ["Intake", "-intake", "in take", "intake!", &"a".repeat(BOT_ID_MAX + 1)] {
        let err = bot_of(json!({"title": "T", "task": "x", "bot": bad}), None).unwrap_err();
        assert!(err.starts_with("invalid bot id: "), "{bad:?}: {err}");
    }
    assert_eq!(bot_of(json!({"title": "T", "task": "x", "bot": 3}), None), Err("bot must be a string".to_string()));
}

#[test]
fn a_patch_keeps_the_bot_and_the_copy_unless_it_names_the_bot() {
    let base = TaskSpec {
        id: "a".into(),
        bot: Some("intake".into()),
        effort: Some(Effort::High),
        skill: Some(SkillRef::Plugin("task-intake".into())),
        ..TaskSpec::default()
    };
    let kept = parse_spec(&json!({"title": "New"}), Some(&base)).unwrap();
    assert_eq!(
        (kept.bot.as_deref(), kept.effort, kept.skill.clone()),
        (Some("intake"), base.effort, base.skill.clone())
    );
    assert_eq!(bot_of(json!({"bot": null}), Some(&base)), Ok(None));
    assert_eq!(bot_of(json!({"bot": "setup"}), Some(&base)), Ok(Some("setup".into())));
}

#[test]
fn effort_and_skill_are_not_input_fields() {
    for field in ["effort", "skill"] {
        let err = parse_spec(&json!({"title": "T", "task": "x", field: "high"}), None).unwrap_err();
        assert!(err.starts_with(&format!("unknown field '{field}'")), "{err}");
    }
}

#[test]
fn a_proposal_task_needs_a_bot_and_no_longer_a_model() {
    let task = |extra: serde_json::Value| {
        let mut t = json!({"id": "a", "title": "T", "task": "x"});
        t.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        parse_proposal(&json!({"tasks": [t]}))
    };
    assert_eq!(
        task(json!({"model": "sonnet"})).unwrap_err(),
        "tasks[0]: task a: bot is required (see get_workspace_settings.bots)"
    );
    assert_eq!(
        task(json!({"bot": null})).unwrap_err(),
        "tasks[0]: task a: bot is required (see get_workspace_settings.bots)"
    );
    let specs = task(json!({"bot": "general"})).unwrap();
    assert_eq!((specs[0].bot.as_deref(), specs[0].model.as_deref()), (Some("general"), None));
}

#[test]
fn the_snapshot_fields_are_written_only_when_present() {
    let plain = serde_json::to_value(TaskSpec::default()).unwrap();
    for field in ["bot", "effort", "skill"] {
        assert!(plain.get(field).is_none(), "{field}");
    }
    let snapshot = TaskSpec {
        bot: Some("intake".into()),
        effort: Some(Effort::High),
        skill: Some(SkillRef::User("s".into())),
        ..TaskSpec::default()
    };
    let wire = serde_json::to_value(&snapshot).unwrap();
    assert_eq!((&wire["bot"], &wire["effort"]), (&json!("intake"), &json!("high")));
    assert_eq!(wire["skill"], json!({"kind": "user", "id": "s"}));
    assert_eq!(serde_json::from_value::<TaskSpec>(wire).unwrap(), snapshot);
}
