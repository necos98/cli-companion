use super::super::validate::TASK_MAX_CHARS;
use super::*;
use crate::agents::presets::PRESETS as PLUGIN_SKILLS;
use std::collections::HashSet;

#[test]
fn the_presets_come_in_order() {
    let ids: Vec<_> = PRESETS.iter().map(|p| (p.id, p.name)).collect();
    assert_eq!(
        ids,
        [
            ("general", "General"),
            ("intake", "Intake"),
            ("setup", "Setup"),
            ("skill-wizard", "Skill Wizard"),
            ("reviewer", "Reviewer"),
            ("planner", "Planner"),
            ("quick-answer", "Quick Answer"),
            ("bug-fixer", "Bug Fixer"),
            ("researcher", "Researcher"),
            ("test-writer", "Test Writer"),
            ("docs-writer", "Docs Writer"),
            ("refactorer", "Refactorer"),
            ("git-operator", "Git Operator"),
            ("release", "Release"),
        ]
    );
    assert_eq!(PRESETS[0].id, GENERAL_ID);
}

#[test]
fn each_preset_has_the_model_effort_and_skill_of_its_job() {
    let fields = |id| {
        let p = find(id).unwrap();
        (p.model.clone(), p.effort, p.skill)
    };
    assert_eq!(fields("general"), (ModelChoice::Default, None, None));
    assert_eq!(fields("intake"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), Some("task-intake")));
    assert_eq!(fields("setup"), (ModelChoice::Tier(ModelTier::Standard), Some(Effort::Medium), Some("project-setup")));
    assert_eq!(
        fields("skill-wizard"),
        (ModelChoice::Tier(ModelTier::Standard), Some(Effort::Medium), Some("skill-wizard"))
    );
    for id in ["reviewer", "planner"] {
        assert_eq!(fields(id), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), None), "{id}");
    }
    assert_eq!(
        fields("quick-answer"),
        (ModelChoice::Tier(ModelTier::Standard), Some(Effort::Low), Some("code-answer"))
    );
    assert_eq!(fields("bug-fixer"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), Some("bug-fix")));
    assert_eq!(fields("researcher"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), Some("code-research")));
    assert_eq!(fields("test-writer"), (ModelChoice::Tier(ModelTier::Standard), Some(Effort::Medium), Some("test-qa")));
    assert_eq!(
        fields("docs-writer"),
        (ModelChoice::Tier(ModelTier::Standard), Some(Effort::Medium), Some("docs-writer"))
    );
    assert_eq!(fields("refactorer"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), Some("safe-refactor")));
    assert_eq!(fields("git-operator"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::Medium), Some("git-ops")));
    assert_eq!(fields("release"), (ModelChoice::Tier(ModelTier::Strong), Some(Effort::High), Some("release-prep")));
    assert!(find("nope").is_none());
}

#[test]
fn each_preset_runs_only_the_task_kinds_of_its_job() {
    let kinds = |id| find(id).unwrap().kinds;
    assert_eq!(kinds("general"), [TaskKind::Work]);
    assert_eq!(kinds("reviewer"), [TaskKind::Review]);
    assert_eq!(kinds("planner"), [TaskKind::Plan]);
    // Le skill da worker guidano un task automatico, non un terminale.
    assert_eq!(kinds("quick-answer"), [TaskKind::Plan]);
    assert_eq!(kinds("researcher"), [TaskKind::Plan]);
    for id in ["bug-fixer", "test-writer", "docs-writer", "refactorer", "git-operator", "release"] {
        assert_eq!(kinds(id), [TaskKind::Work], "{id}");
    }
    // Le skill dei preset sono pensate per un terminale interattivo.
    for id in ["intake", "setup", "skill-wizard"] {
        assert!(kinds(id).is_empty(), "{id}");
    }
    assert_eq!(find("reviewer").unwrap().bot().kinds, [TaskKind::Review]);
}

#[test]
fn presets_are_well_formed() {
    let ids: HashSet<_> = PRESETS.iter().map(|p| p.id).collect();
    let names: HashSet<_> = PRESETS.iter().map(|p| p.name.to_lowercase()).collect();
    assert_eq!((ids.len(), names.len()), (PRESETS.len(), PRESETS.len()));
    for p in &PRESETS {
        assert!(!p.id.starts_with(USER_ID_PREFIX), "{}", p.id);
        assert_eq!(super::super::validate::normalize_name(p.name).unwrap(), p.name);
        assert!(p.task.is_ascii() && !p.task.is_empty(), "{}", p.id);
        assert!(p.task.chars().count() <= TASK_MAX_CHARS, "{}", p.id);
        // Nessun preset fissa un modello concreto: deve valere con ogni agente.
        assert!(!matches!(p.model, ModelChoice::Id(_)), "{}", p.id);
        if let Some(skill) = p.skill {
            assert!(PLUGIN_SKILLS.iter().any(|s| s.id == skill), "{skill}");
        }
        assert_eq!(super::super::validate::normalize_kinds(p.kinds.to_vec()), p.kinds, "{}", p.id);
    }
}

/// Segretari e pianificatore scelgono il Bot dal `task`: fra i Bot di uno
/// stesso tipo di task il verbo iniziale non si ripete.
#[test]
fn the_tasks_of_the_bots_of_a_kind_start_with_different_verbs() {
    for kind in TaskKind::ALL {
        let verbs: Vec<_> =
            PRESETS.iter().filter(|p| p.kinds.contains(&kind)).filter_map(|p| p.task.split(' ').next()).collect();
        let unique: HashSet<_> = verbs.iter().collect();
        assert_eq!(unique.len(), verbs.len(), "{kind:?}: {verbs:?}");
    }
}

#[test]
fn a_preset_bot_follows_the_workspace_and_remembers_its_preset() {
    let bot = find("intake").unwrap().bot();
    assert_eq!(bot.agent, None);
    assert!(bot.enabled && bot.image.is_none());
    assert_eq!(bot.preset.as_deref(), Some("intake"));
    assert_eq!(bot.skill, Some(SkillRef::Plugin("task-intake".into())));
}

#[test]
fn general_is_put_back_and_kept_enabled() {
    let mut empty = Vec::new();
    ensure_general(&mut empty);
    assert_eq!(empty, vec![PRESETS[0].bot()]);

    let mut others = vec![find("setup").unwrap().bot()];
    ensure_general(&mut others);
    assert_eq!(others.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["general", "setup"]);

    let mut off = default_bots();
    off[0].enabled = false;
    off[0].name = "Generic".into();
    ensure_general(&mut off);
    assert!(off[0].enabled);
    assert_eq!(off[0].name, "Generic");
    assert_eq!(off.len(), PRESETS.len());
}

/// I Bot con effetti fuori dal flusso di consegna (git sul repository condiviso,
/// pubblicazione) nascono spenti: l'utente li accende di proposito. Gli altri accesi.
#[test]
fn only_the_risky_presets_start_disabled() {
    let off: Vec<_> = PRESETS.iter().filter(|p| !p.enabled_by_default).map(|p| p.id).collect();
    assert_eq!(off, ["git-operator", "release"]);
    for p in &PRESETS {
        assert_eq!(p.bot().enabled, p.enabled_by_default, "{}", p.id);
    }
    assert!(find(GENERAL_ID).unwrap().enabled_by_default, "general is never disabled");
}
