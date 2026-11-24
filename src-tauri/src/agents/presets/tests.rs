use super::*;
use crate::agents::registry;

/// Agent and model names no skill may mention, registered or not: skills read agents and
/// models from `get_workspace_settings`.
const FOREIGN_NAMES: &[&str] = &["sonnet", "opus", "haiku", "fable", "claude", "codex", "gemini", "copilot"];

fn registered_names() -> Vec<String> {
    let mut names = Vec::new();
    for agent in registry::all() {
        names.extend([agent.id(), agent.display_name()].map(str::to_lowercase));
        let models = agent.capabilities().models.map_or(&[][..], |catalog| catalog.suggested());
        names.extend(models.iter().flat_map(|m| [m.id, m.label]).map(str::to_lowercase));
    }
    names
}

/// `name` compare come parola intera: un nome corto (es. "pi") non deve trovarsi dentro "api".
fn contains_word(text: &str, name: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric();
    text.match_indices(name).any(|(i, _)| {
        let before = text[..i].chars().next_back().is_none_or(|c| !is_word(c));
        let after = text[i + name.len()..].chars().next().is_none_or(|c| !is_word(c));
        before && after
    })
}

#[test]
fn names_match_as_whole_words() {
    assert!(contains_word("use pi here", "pi"));
    assert!(contains_word("pi", "pi"));
    assert!(contains_word("(pi-agent)", "pi-agent"));
    assert!(contains_word("claude-code", "claude"));
    assert!(!contains_word("the api and a pipeline", "pi"));
}

#[test]
fn skills_name_no_agent_and_no_model() {
    let mut banned = registered_names();
    banned.extend(FOREIGN_NAMES.iter().map(|name| name.to_string()));
    assert!(banned.len() > FOREIGN_NAMES.len(), "the registry names must be covered");
    for p in PRESETS {
        let skill = p.skill_text().to_lowercase();
        for name in &banned {
            assert!(!contains_word(&skill, name), "skill {} names `{name}`", p.id);
        }
    }
}

#[test]
fn registry_is_well_formed() {
    let mut ids: Vec<_> = PRESETS.iter().map(|p| p.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), PRESETS.len());
    for p in PRESETS {
        assert!(p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "{}", p.id);
        let skill = p.skill_text();
        assert!(!skill.contains('\r') && skill.is_ascii(), "{}", p.id);
        let front = skill.strip_prefix("---\n").and_then(|s| s.split_once("\n---\n")).unwrap().0;
        let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
        assert_eq!(field("name:"), Some(p.id));
        assert!(field("description:").is_some_and(|d| !d.is_empty() && d.len() <= 1024));
        assert_eq!(field("description:").map(str::to_string), Some(p.description()));
    }
}

/// The skills a worker loads from its bot: their description must keep them out of every
/// other session, where the agent lists them too.
const WORKER_SKILLS: &[&str] =
    &["code-answer", "bug-fix", "code-research", "test-qa", "docs-writer", "safe-refactor", "git-ops", "release-prep"];

/// The sections every worker skill has, in this order.
const WORKER_SKILL_SECTIONS: &[&str] = &["## When this applies", "## Rules", "## Workflow", "## Report"];

fn preset(id: &str) -> &'static Preset {
    PRESETS.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("no preset {id}"))
}

#[test]
fn worker_skills_apply_only_to_a_task_whose_bot_names_them() {
    for id in WORKER_SKILLS {
        let description = preset(id).description();
        assert!(description.contains("cli-companion"), "{id}");
        assert!(description.contains("task whose bot names this skill"), "{id}");
        assert!(description.contains("never use it in any other session"), "{id}");
    }
}

#[test]
fn worker_skills_have_the_common_sections_in_order() {
    for id in WORKER_SKILLS {
        let skill = preset(id).skill_text();
        let positions: Vec<usize> = WORKER_SKILL_SECTIONS
            .iter()
            .map(|section| skill.find(section).unwrap_or_else(|| panic!("{id}: {section}")))
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "{id}: sections out of order");
    }
}

#[test]
fn worker_skills_do_not_repeat_the_worker_standard_or_the_delivery() {
    for id in WORKER_SKILLS {
        let skill = preset(id).skill_text();
        for t in ["Engineering standard", "complete_task", "git rebase", "merge --ff-only"] {
            assert!(!skill.contains(t), "{id}: {t}");
        }
    }
}

#[test]
fn code_answer_skill_answers_read_only() {
    let p = preset("code-answer");
    assert_eq!(p.name, "Code Answer");
    assert!(p.description().contains("planning task"));
    let skill = p.skill_text();
    for t in [
        "Read-only",
        "git only to read",
        "Do not propose tasks",
        "suggest, in one line, that the user asks for the change",
        "starts with \"Answer:\"",
        "at most 10 lines",
        "language of the question",
        "could not verify",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn bug_fix_skill_reproduces_before_fixing() {
    let p = preset("bug-fix");
    assert_eq!(p.name, "Bug Fix");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "Reproduce before touching the code",
        "documented steps",
        "root cause, not the symptom",
        "regression test",
        "`ask_user`",
        "`failed`",
        "never fix it blindly",
        "the cause",
        "the fix",
        "the test",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

/// The plugin skills of the interactive bots: with the worker skills, the whole registry.
const INTERACTIVE_SKILLS: &[&str] = &["task-intake", "skill-wizard", "project-setup"];

#[test]
fn registry_holds_the_interactive_and_the_worker_skills() {
    assert_eq!(PRESETS.len(), INTERACTIVE_SKILLS.len() + WORKER_SKILLS.len());
    for id in INTERACTIVE_SKILLS.iter().chain(WORKER_SKILLS) {
        preset(id);
    }
}

#[test]
fn code_research_skill_reports_findings_and_options() {
    let p = preset("code-research");
    assert_eq!(p.name, "Code Research");
    assert!(p.description().contains("planning task"));
    let skill = p.skill_text();
    for t in [
        "Read-only",
        "git only to read",
        "output is ignored by git",
        "network access: cite each one",
        "Propose tasks only if the request asks for them",
        "starts with\n\"Answer:\"",
        "relative effort",
        "could not verify",
    ] {
        assert!(skill.contains(t), "{t}");
    }
    let sections = ["`Question`:", "`Findings`:", "`Options`:", "`Recommendation`:", "`Open points`:"];
    let positions: Vec<usize> = sections.iter().map(|s| skill.find(s).unwrap_or_else(|| panic!("{s}"))).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "report sections out of order");
}

#[test]
fn test_qa_skill_follows_the_existing_framework() {
    let p = preset("test-qa");
    assert_eq!(p.name, "Test QA");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "the project already uses",
        "Never add a new test framework",
        "unless the specification asks for it",
        "Deterministic tests only",
        "no fixed sleeps",
        "A test that reveals a bug",
        "`follow_ups`",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn docs_writer_skill_verifies_what_it_cites() {
    let p = preset("docs-writer");
    assert_eq!(p.name, "Docs Writer");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "reader and the language from the specification",
        "language of the request",
        "non-technical reader",
        "no jargon",
        "numbered steps",
        "Verify in the repository every command, path",
        "Do not change code, except its comments",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn safe_refactor_skill_keeps_the_behaviour() {
    let p = preset("safe-refactor");
    assert_eq!(p.name, "Safe Refactor");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "Green tests before you start",
        "write them first",
        "Small steps",
        "No change of behaviour, public API or data format",
        "the list of steps",
        "proof of equivalence",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn git_ops_skill_keeps_the_blocking_safety_rules() {
    let p = preset("git-ops");
    assert_eq!(p.name, "Git Ops");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "These rules are blocking",
        "Never force-push",
        "never rewrite\n  the history of the main branch or of the base branch",
        "`reset --hard`, `clean -fd`, `branch -D`, `stash drop` or delete a remote branch",
        "confirmation obtained with `ask_user`",
        "lists exactly what is lost",
        "Before every destructive operation, create a backup ref",
        "`backup/<date>`",
        "in the worktree of the task, not in the user's checkout",
        "unless the specification asks",
        "Never use `git stash` without a unique message",
        "the stash stack is shared",
        "produces no commit of its own",
        "skip the\n  commit, push, pull request and merge steps",
        "`completed` without a `commit`",
        "the commands run",
        "the state before and after",
        "how to go back",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn release_prep_skill_publishes_only_after_confirmation() {
    let p = preset("release-prep");
    assert_eq!(p.name, "Release Prep");
    assert!(p.description().contains("work task"));
    let skill = p.skill_text();
    for t in [
        "Without release\n  instructions in the repository, change nothing and finish the task as `failed`",
        "document the release process",
        "Version bump and changelog come from the commits merged",
        "never invent an entry",
        "Run only the build and publish commands the project documents",
        "Publishing, remote tags and deploys only after a confirmation obtained with `ask_user`",
        "with a\n  summary of what will happen",
        "means do not publish",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn task_intake_skill_covers_the_method() {
    let p = PRESETS.iter().find(|p| p.id == "task-intake").unwrap();
    assert_eq!(p.name, "Task Intake");
    let skill = p.skill_text();
    for t in [
        "propose_tasks",
        "list_tasks",
        "get_task_details",
        "update_task",
        "delete_task",
        "Goal",
        "Context",
        "Constraints",
        "Done when",
        "Verify",
        "depends_on",
        "Task panel",
        "get_workspace_settings",
        "propose_settings",
        "Project Setup",
        "## Review",
        "kind: \"review\"",
        "all** the tasks",
        "E2E scenarios",
        "playwright.config",
        "CLI_COMPANION_E2E_PORT",
        "e2e-results/junit.xml",
        "open: \"never\"",
        "explicitly asks",
        "repos[...].e2e",
        "`e2e: true`",
        "blocks the\n  delivery",
        "Suite present but not registered",
        "at most one line",
        "Never set `e2e: true`",
        "## Bot, agent and model",
        "`get_workspace_settings.bots`",
        "`bot`: required",
        "`kinds` must include",
        "Never invent an id",
        "`tier: \"standard\"`",
        "`tier: \"strong\"`",
        "Never write a model name from memory",
        "`readiness` is `ready`",
        "an override of the Bot's model",
        "it must equal the agent of the Bot",
        "a capability that only another agent",
        "enabled and ready",
        "best matches the work of the task",
    ] {
        assert!(skill.contains(t), "{t}");
    }
    for fixed_bot_id in ["`general`", "`reviewer`", "`planner`"] {
        assert!(!skill.contains(fixed_bot_id), "{fixed_bot_id}");
    }
}

#[test]
fn skill_wizard_skill_covers_the_method() {
    let p = PRESETS.iter().find(|p| p.id == "skill-wizard").unwrap();
    assert_eq!(p.name, "Skill Wizard");
    let skill = p.skill_text();
    for t in [
        "save_skill",
        "list_skills",
        "delete_skill",
        "one question at a time",
        "1024",
        "third person",
        "frontmatter",
        "confirmation",
        "/<name>",
        "all workspaces",
        "restarting",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn skill_wizard_skill_does_not_duplicate_a_plugin_skill() {
    let skill = preset("skill-wizard").skill_text();
    for t in [
        "## Do not duplicate a plugin skill",
        "get_workspace_settings",
        "`plugin_skills`",
        "It does not list the plugin skills",
        "never copy or rewrite the\n  plugin skill",
        "the cli-companion plugin skill `<name>` first",
        "works with every agent",
        "A Bot loads one skill",
        "derived from Bug Fixer, not a copy of `bug-fix`",
        "you never propose settings",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn project_setup_skill_covers_the_method() {
    let p = PRESETS.iter().find(|p| p.id == "project-setup").unwrap();
    assert_eq!(p.name, "Project Setup");
    let skill = p.skill_text();
    for t in [
        "get_workspace_settings",
        "propose_settings",
        "Task panel",
        "container_root",
        "copy_ignored",
        "setup",
        "cleanup",
        "bypass",
        "end your turn",
        "$CONTAINER_WORKTREE",
        "repeatable",
        "single line",
        "one question at a time",
        "before -> after",
        "## Worker agent and per-agent settings",
        "worker_agents",
        "rule_format",
        "max_rule_chars",
        "`readiness` is not `ready`",
        "Never write a\n  model name from memory",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn project_setup_skill_covers_the_e2e_suite() {
    let p = PRESETS.iter().find(|p| p.id == "project-setup").unwrap();
    let skill = p.skill_text();
    for t in [
        "## E2E suite",
        "playwright.config",
        "cypress.config",
        "pytest-playwright",
        "JUnit",
        "repos[...].e2e",
        "npx playwright test",
        "timeout_min",
        "CLI_COMPANION_E2E_PORT",
        "propose nothing about E2E",
        "You do not edit",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}

#[test]
fn project_setup_skill_covers_lampo() {
    let skill = preset("project-setup").skill_text();
    for t in [
        "## Lampo",
        "see \"Lampo\" below",
        "worktree up \"%WORKTREE%\"",
        "worktree down \"%WORKTREE%\"",
        "--db",
        "fresh",
        "clone:<db>",
        "--drop-database",
        ".env",
        "WSL",
        "propose nothing about Lampo",
        "Windows: commands run in `cmd`, so `%WORKTREE%`",
        "WSL: commands run in `sh`, so `$WORKTREE`",
    ] {
        assert!(skill.contains(t), "{t}");
    }
}
