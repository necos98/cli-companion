//! Il worker di un task con Bot: parte con l'effort della copia, legge la skill
//! in `get_task` (lavoro, piano, revisione) e non parte se la skill non c'e' piu'.

use super::skills::Sandbox;
use super::*;
use crate::agents::test_registry::SecondIntegration;
use crate::agents::testing::{FakeIntegration, FAKE_MODEL_LARGE};
use crate::agents::{registry, AutomationRequest, Effort, PermissionMode};
use crate::bots::{AgentBot, SkillRef};
use crate::user_skills::SkillInput;

const SECOND: &str = "fake-second";
/// Come il contratto di Claude Code carica la skill del plugin.
const INTAKE_SKILL: &str = "cli-companion-notify:task-intake";
const SKILL_LINE: &str = "Before starting, load the skill cli-companion-notify:task-intake with your skill tool; the instructions of this task take precedence over the skill where they conflict.";

fn with_bot(id: &str, bot: &str) -> Value {
    json!({"id": id, "title": format!("Task {id}"), "task": "x", "bot": bot})
}

/// Il comando con cui e' partito il worker del task.
fn command_of(st: &AutomationState, h: &FakeHost, id: &str) -> String {
    let session = session_of(st, h, id);
    h.spawns().into_iter().find(|s| s.tab_id == session).expect("worker avviato").command
}

fn get_task(st: &AutomationState, h: &FakeHost, id: &str) -> Value {
    call(st, h, &session_of(st, h, id), "get_task", json!({})).unwrap()
}

fn edit_bot(h: &FakeHost, id: &str, f: impl FnOnce(&mut AgentBot)) {
    h.edit_bots(|bots| f(bots.iter_mut().find(|b| b.id == id).unwrap()));
}

#[test]
fn the_worker_starts_with_the_effort_of_the_bot() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![with_bot("a", "intake")]);
    assert!(command_of(&st, &h, "a").contains("effort=high"), "{}", command_of(&st, &h, "a"));

    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![with_bot("b", "general")]);
    assert!(command_of(&st, &h, "b").contains("effort=-"));
}

#[test]
fn the_claude_code_command_carries_the_effort_flag() {
    let req = AutomationRequest {
        bootstrap_prompt: BOOTSTRAP_PROMPT,
        permission_mode: PermissionMode::Default,
        model: None,
        permissions_file: None,
        effort: Some(Effort::High),
    };
    let start = crate::agents::automation_command(registry::default_agent().id(), TargetKind::Windows, &req).unwrap();
    assert!(start.launch.command.ends_with(" --effort high"), "{}", start.launch.command);
    assert!(start.warnings.is_empty());
}

#[test]
fn an_effort_the_agent_does_not_support_is_dropped_with_a_warning_in_the_note() {
    let _second = SecondIntegration::enabled(FakeIntegration::full(SECOND));
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let pinned = h.bot_for_agent(SECOND);
    edit_bot(&h, &pinned, |b| b.effort = Some(Effort::Medium));
    let mut t = with_bot("a", &pinned);
    t["model"] = json!(FAKE_MODEL_LARGE);
    queue(&st, &h, vec![t]);

    assert_eq!(task(&st, &h, "a")["effort"], "medium", "the task keeps what the bot asked for");
    assert!(command_of(&st, &h, "a").contains("effort=-"));
    let note = task(&st, &h, "a")["note"].as_str().unwrap().to_string();
    assert!(note.contains("Fake does not support the effort level \"medium\": ignored"), "{note}");
}

#[test]
fn get_task_of_a_work_task_names_the_skill_and_leads_with_the_instruction() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![with_bot("a", "intake")]);
    let with_skill = get_task(&st, &h, "a");
    assert_eq!(with_skill["skill"], INTAKE_SKILL);
    let instructions = with_skill["instructions"].as_str().unwrap();
    assert!(instructions.starts_with(&format!("{SKILL_LINE}\n\n")), "{instructions}");
    assert!(instructions.contains(crate::board::texts::WORKER_GUARDRAILS.trim()), "the usual instructions follow");

    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    queue(&st, &h, vec![with_bot("b", "general")]);
    let without = get_task(&st, &h, "b");
    assert!(without.get("skill").is_none());
    assert!(!without["instructions"].as_str().unwrap().contains("load the skill"));
}

#[test]
fn get_task_of_a_review_task_names_the_skill_too() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let mut review = with_bot("rev", "intake");
    review["kind"] = json!("review");
    review["depends_on"] = json!(["a"]);
    queue(&st, &h, vec![with_bot("a", "general"), review]);
    let a = session_of(&st, &h, "a");
    call(&st, &h, &a, "get_task", json!({})).unwrap();
    call(&st, &h, &a, "complete_task", json!({"status": "completed", "summary": "done a"})).unwrap();

    let out = get_task(&st, &h, "rev");
    assert_eq!(out["skill"], INTAKE_SKILL);
    let instructions = out["instructions"].as_str().unwrap();
    assert!(instructions.starts_with(&format!("{SKILL_LINE}\n\n")), "{instructions}");
    assert!(instructions.contains(crate::board::texts::REVIEWER_INSTRUCTIONS.lines().next().unwrap()));
}

#[test]
fn get_task_of_a_plan_task_names_the_skill_too() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let request = PlanRequest {
        title: "Plan dark theme".into(),
        task: "Add a dark theme".into(),
        model: None,
        repo: None,
        bot: None,
    };
    let id = create_plan_task(&st, &h, "ws-1", &request).unwrap();
    // I task di pianificazione nascono senza Bot: la skill la mette l'utente scegliendone uno.
    st.run(&h, |r| {
        let task = r.boards.get_mut("ws-1").unwrap().tasks.iter_mut().find(|t| t.id == id).unwrap();
        task.skill = Some(SkillRef::Plugin("task-intake".into()));
    });
    boards::board_set(&st, &h, "ws-1", None, Some(false)).unwrap();

    let out = get_task(&st, &h, &id);
    assert_eq!(out["skill"], INTAKE_SKILL);
    assert!(out["instructions"].as_str().unwrap().starts_with(&format!("{SKILL_LINE}\n\n")));
}

/// Un Bot dell'utente con una skill personale.
fn user_skill_bot(h: &FakeHost, skill: &str) -> String {
    h.edit_bots(|bots| {
        let general = bots[0].clone();
        bots.push(AgentBot {
            id: "custom-notes".into(),
            name: "Notes".into(),
            skill: Some(SkillRef::User(skill.into())),
            preset: None,
            ..general
        });
    });
    "custom-notes".into()
}

#[test]
fn a_user_skill_that_no_longer_exists_fails_the_task_and_no_worker_starts() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let sandbox = Sandbox::new();
    *h.skills_store.lock().unwrap() = Some(sandbox.store());
    let bot = user_skill_bot(&h, "gone-skill");
    queue(&st, &h, vec![with_bot("a", &bot)]);

    let t = task(&st, &h, "a");
    assert_eq!(t["status"], "failed");
    assert_eq!(
        t["note"],
        "the skill gone-skill of bot custom-notes no longer exists: choose it again in the bots settings or change the task's bot"
    );
    assert!(h.spawns().is_empty());
    assert!(h.git_log_without_detection().is_empty(), "nothing is prepared for a task that cannot start");
}

#[test]
fn an_existing_user_skill_starts_the_worker_with_its_name() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let sandbox = Sandbox::new();
    let input = SkillInput {
        name: "notes-skill".into(),
        description: "Use when taking notes".into(),
        body: "# Steps".into(),
        files: Vec::new(),
    };
    crate::user_skills::save(&sandbox.store(), &input).unwrap();
    *h.skills_store.lock().unwrap() = Some(sandbox.store());
    let bot = user_skill_bot(&h, "notes-skill");
    queue(&st, &h, vec![with_bot("a", &bot)]);

    assert_eq!(task(&st, &h, "a")["status"], "running");
    assert_eq!(get_task(&st, &h, "a")["skill"], "notes-skill");
}

#[test]
fn a_task_whose_agent_cannot_load_the_skill_fails_with_a_clear_note() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    boards::task_create(&st, &h, "ws-1", &json!({"id": "a", "title": "A", "task": "x", "bot": "intake"})).unwrap();
    // Una copia salvata con una skill che l'agente non sa piu' caricare.
    st.run(&h, |r| {
        let task = r.boards.get_mut("ws-1").unwrap().tasks.iter_mut().find(|t| t.id == "a").unwrap();
        task.skill = Some(SkillRef::Plugin("bad skill".into()));
    });
    boards::board_set(&st, &h, "ws-1", None, Some(false)).unwrap();

    let t = task(&st, &h, "a");
    assert_eq!(t["status"], "failed");
    let expected = format!(
        "the skill bad skill of bot intake cannot be loaded by the agent {}: choose another bot",
        registry::default_agent().id()
    );
    assert_eq!(t["note"], expected.as_str());
    assert!(h.spawns().is_empty());
}

const EXPLORER_INVOCATION: &str = "call the Agent tool with subagent_type `cli-companion-notify:explorer`";

fn delegation_paragraph(invocation: &str) -> String {
    crate::board::texts::exploration_delegation(invocation)
}

#[test]
fn get_task_appends_the_exploration_delegation_for_the_bots_that_explore() {
    for bot in ["general", "bug-fixer", "refactorer"] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
        queue(&st, &h, vec![with_bot("a", bot)]);
        let instructions = get_task(&st, &h, "a")["instructions"].as_str().unwrap().to_string();
        let paragraph = delegation_paragraph(EXPLORER_INVOCATION);
        assert!(instructions.ends_with(&format!("\n\n{paragraph}")), "{bot}: {instructions}");
        assert!(paragraph.contains(&format!("to delegate, {EXPLORER_INVOCATION}.")));
        assert!(!paragraph.contains("{invocation}"));
    }
}

#[test]
fn get_task_leaves_out_the_exploration_delegation_for_the_other_bots() {
    for bot in ["intake", "quick-answer", "test-writer", "docs-writer"] {
        let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
        queue(&st, &h, vec![with_bot("a", bot)]);
        let instructions = get_task(&st, &h, "a")["instructions"].as_str().unwrap().to_string();
        assert!(!instructions.contains("Exploration delegation."), "{bot}");
    }
}

#[test]
fn get_task_of_a_review_ends_with_the_exploration_delegation() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let mut review = with_bot("rev", "general");
    review["kind"] = json!("review");
    review["depends_on"] = json!(["a"]);
    queue(&st, &h, vec![with_bot("a", "general"), review]);
    let a = session_of(&st, &h, "a");
    call(&st, &h, &a, "get_task", json!({})).unwrap();
    call(&st, &h, &a, "complete_task", json!({"status": "completed", "summary": "done a"})).unwrap();

    let out = get_task(&st, &h, "rev");
    let instructions = out["instructions"].as_str().unwrap();
    assert!(instructions.ends_with(&delegation_paragraph(EXPLORER_INVOCATION)), "{instructions}");
}

#[test]
fn the_exploration_delegation_follows_the_capability_of_the_agent() {
    let _second = SecondIntegration::enabled(FakeIntegration::full(SECOND));
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let pinned = h.bot_for_agent(SECOND);
    let mut t = with_bot("a", &pinned);
    t["model"] = json!(FAKE_MODEL_LARGE);
    queue(&st, &h, vec![t]);
    let instructions = get_task(&st, &h, "a")["instructions"].as_str().unwrap().to_string();
    let expected = delegation_paragraph(crate::agents::testing::FAKE_EXPLORER_INVOCATION);
    assert!(instructions.ends_with(&format!("\n\n{expected}")), "{instructions}");
    assert!(expected.contains("to delegate, spawn the fake explorer."));

    drop(_second);
    let _bare = SecondIntegration::enabled(
        FakeIntegration::full(SECOND).without(crate::agents::Capability::ExplorationDelegation),
    );
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let pinned = h.bot_for_agent(SECOND);
    let mut t = with_bot("b", &pinned);
    t["model"] = json!(FAKE_MODEL_LARGE);
    queue(&st, &h, vec![t]);
    let instructions = get_task(&st, &h, "b")["instructions"].as_str().unwrap().to_string();
    assert!(!instructions.contains("Exploration delegation."), "{instructions}");
}

/// Un task di pianificazione con la skill del plugin `skill` (nascono senza Bot): il suo `get_task`.
fn plan_with_skill(skill: &str) -> Value {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let request = PlanRequest {
        title: "Plan dark theme".into(),
        task: "Add a dark theme".into(),
        model: None,
        repo: None,
        bot: None,
    };
    let id = create_plan_task(&st, &h, "ws-1", &request).unwrap();
    st.run(&h, |r| {
        let task = r.boards.get_mut("ws-1").unwrap().tasks.iter_mut().find(|t| t.id == id).unwrap();
        task.skill = Some(SkillRef::Plugin(skill.into()));
    });
    boards::board_set(&st, &h, "ws-1", None, Some(false)).unwrap();
    get_task(&st, &h, &id)
}

#[test]
fn a_plan_task_gets_the_exploration_delegation_only_for_code_research() {
    let research = plan_with_skill("code-research");
    assert!(research["instructions"].as_str().unwrap().ends_with(&delegation_paragraph(EXPLORER_INVOCATION)));
    let answer = plan_with_skill("code-answer");
    assert!(!answer["instructions"].as_str().unwrap().contains("Exploration delegation."));
}
