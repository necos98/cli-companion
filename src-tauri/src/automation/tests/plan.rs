//! Task di pianificazione: creazione da parte dell'app, kind riservato, ruolo
//! del pianificatore, `get_task`, `complete_task`, la proposta unica e
//! l'esecuzione in un worktree ripulito senza consegna.

use super::*;
use crate::automation::planner::PLAN_PROPOSED_ONCE;
use crate::board::texts::{PLANNER_INSTRUCTIONS, WORKER_GUARDRAILS};
use crate::board::PLAN_KIND_RESERVED;

fn request(title: &str) -> PlanRequest {
    PlanRequest {
        title: title.to_string(),
        task: "Add a dark theme to the settings page".to_string(),
        model: Some("opus".to_string()),
        repo: None,
        bot: None,
    }
}

/// Crea il task `plan` "Plan dark theme" e ritorna (id, sessione del pianificatore).
pub(super) fn started_plan(st: &AutomationState, h: &FakeHost) -> (String, String) {
    let id = create_plan_task(st, h, "ws-1", &request("Plan dark theme")).unwrap();
    let session = session_of(st, h, &id);
    (id, session)
}

fn planner(mode: DeliveryMode, parallel: bool) -> (AutomationState, FakeHost, String, String) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
    let (id, session) = started_plan(&st, &h);
    (st, h, id, session)
}

fn complete(st: &AutomationState, h: &FakeHost, session: &str, extra: Value) -> Result<Value, String> {
    let mut params = json!({"status": "completed", "summary": "proposed 2 tasks"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    call(st, h, session, "complete_task", params)
}

#[test]
fn the_app_creates_a_plan_task_straight_in_the_queue() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let id = create_plan_task(&st, &h, "ws-1", &request("Plan dark theme")).unwrap();
    assert_eq!(id, "plan-dark-theme");
    let t = task(&st, &h, &id);
    assert_eq!(
        (t["kind"].as_str(), t["status"].as_str(), t["model"].as_str(), t["task"].as_str()),
        (Some("plan"), Some("queued"), Some("opus"), Some("Add a dark theme to the settings page"))
    );
    assert!(t["e2e"].is_null() && t["depends_on"] == json!([]), "{t}");
    assert!(view(&st, &h).proposals.is_empty(), "no proposal: straight in the queue");
    // Stesso titolo: id libero successivo; modello assente = quello del workspace.
    let again = create_plan_task(&st, &h, "ws-1", &PlanRequest { model: None, ..request("Plan dark theme") }).unwrap();
    assert_eq!(again, "plan-dark-theme-2");
    assert!(task(&st, &h, &again)["model"].is_null());
    // Persistito com'e' e riletto dal disco.
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    let reloaded = Board::from_json(&serde_json::to_string(&saved).unwrap(), 1).unwrap();
    assert_eq!(reloaded.task(&id).unwrap().kind, TaskKind::Plan);
}

#[test]
fn a_plan_request_is_validated_like_any_task() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    let bad = [
        (PlanRequest { title: " ".into(), ..request("x") }, "title is missing"),
        (PlanRequest { task: String::new(), ..request("x") }, "task"),
        (PlanRequest { model: Some("bad model!".into()), ..request("x") }, "invalid model"),
        (PlanRequest { repo: Some("../out".into()), ..request("x") }, "repo"),
        (PlanRequest { repo: Some("missing".into()), ..request("x") }, "does not exist"),
    ];
    for (req, needle) in bad {
        let e = create_plan_task(&st, &h, "ws-1", &req).unwrap_err();
        assert!(e.contains(needle), "{req:?}: {e}");
    }
    assert!(create_plan_task(&st, &h, "nope", &request("x")).unwrap_err().contains("workspace non trovato"));
    *h.archived.lock().unwrap() = true;
    assert_eq!(create_plan_task(&st, &h, "ws-1", &request("x")).unwrap_err(), "workspace archiviato");
    assert!(view(&st, &h).tasks.is_empty());
}

#[test]
fn a_plan_in_a_folder_without_git_is_created_even_without_branches() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    // La root senza git e senza sottocartelle git, e una sottocartella senza git.
    let root = create_plan_task(&st, &h, "ws-1", &request("x")).unwrap();
    h.dirs.lock().unwrap().push("C:\\p\\docs".to_string());
    let sub = create_plan_task(&st, &h, "ws-1", &PlanRequest { repo: Some("docs".into()), ..request("y") }).unwrap();
    assert!(view(&st, &h).tasks.iter().all(|t| t.task.kind == TaskKind::Plan && t.task.no_git), "{root} {sub}");
}

#[test]
fn plan_is_never_proposed_created_or_set_from_outside() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let mut plan = spec("p", &[]);
    plan["kind"] = json!("plan");
    let e = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [plan]})).unwrap_err();
    assert!(e.contains(PLAN_KIND_RESERVED), "{e}");
    let manual = json!({"title": "Plan", "task": "plan it", "kind": "plan"});
    assert!(boards::task_create(&st, &h, "ws-1", &manual).unwrap_err().contains(PLAN_KIND_RESERVED));
    queue(&st, &h, vec![spec("w", &[])]);
    let e = call(&st, &h, INTAKE, "update_task", json!({"id": "w", "kind": "plan"})).unwrap_err();
    assert!(e.contains(PLAN_KIND_RESERVED), "{e}");
    assert!(boards::task_update(&st, &h, "ws-1", "w", &json!({"kind": "plan"})).unwrap_err().contains("reserved"));
    assert_eq!(task(&st, &h, "w")["kind"], "work");
}

#[test]
fn a_queued_plan_keeps_its_kind_and_takes_no_e2e() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    boards::board_set(&st, &h, "ws-1", None, Some(true)).unwrap();
    let id = create_plan_task(&st, &h, "ws-1", &request("Plan dark theme")).unwrap();
    for kind in [json!("work"), json!("review"), Value::Null] {
        let e = call(&st, &h, INTAKE, "update_task", json!({"id": id, "kind": kind})).unwrap_err();
        assert!(e.contains("cannot be changed"), "{kind}: {e}");
    }
    h.repo_cfg.lock().unwrap().insert(
        ".".into(),
        RepoConfig {
            e2e: Some(crate::workspaces::E2eConfig {
                command: "npm run e2e".into(),
                junit: "j.xml".into(),
                timeout_min: 5,
            }),
            ..Default::default()
        },
    );
    let e = call(&st, &h, INTAKE, "update_task", json!({"id": id, "e2e": true})).unwrap_err();
    assert!(e.contains("a plan task cannot require E2E tests"), "{e}");
    call(&st, &h, INTAKE, "update_task", json!({"id": id, "title": "Plan the theme"})).unwrap();
    let t = task(&st, &h, &id);
    assert_eq!((t["kind"].as_str(), t["title"].as_str()), (Some("plan"), Some("Plan the theme")));
}

#[test]
fn the_worker_of_a_plan_is_a_planner_with_its_tools() {
    let (st, h, id, p) = planner(DeliveryMode::Commit, false);
    let me = call(&st, &h, &p, "whoami", json!({})).unwrap();
    assert_eq!(me["role"], "planner");
    assert!(me.get("workspace").is_none());
    let list = call(&st, &h, &p, "list_tasks", json!({})).unwrap();
    let row = list["tasks"].as_array().unwrap().iter().find(|t| t["id"] == id.as_str()).cloned().unwrap();
    assert_eq!((row["kind"].as_str(), row["status"].as_str()), (Some("plan"), Some("running")));
    let details = call(&st, &h, &p, "get_task_details", json!({"id": id})).unwrap();
    assert_eq!(details["task"]["kind"], "plan");
    let settings = call(&st, &h, &p, "get_workspace_settings", json!({})).unwrap();
    assert!(settings.get("settings").is_some() && settings.get("agents").is_some(), "{settings}");
    for m in [
        "update_task",
        "delete_task",
        "run_e2e",
        "get_e2e_result",
        "propose_settings",
        "save_skill",
        "list_skills",
        "delete_skill",
    ] {
        let e = call(&st, &h, &p, m, json!({"id": id})).unwrap_err();
        assert_eq!(e, format!("{m} is not available to automated sessions"));
    }
}

#[test]
fn get_task_of_a_plan_has_planner_instructions_and_no_delivery() {
    let h = FakeHost::with_delivery(DeliveryMode::Pr, true);
    *h.worker_rules.lock().unwrap() = "specs in Italian".to_string();
    let st = AutomationState::default();
    let (id, p) = started_plan(&st, &h);
    let t = call(&st, &h, &p, "get_task", json!({})).unwrap();
    assert_eq!(keys(&t), ["environment", "instructions", "task", "title"]);
    assert_eq!(
        (t["task"].as_str(), t["title"].as_str()),
        (Some("Add a dark theme to the settings page"), Some("Plan dark theme"))
    );
    let instr = t["instructions"].as_str().unwrap();
    assert!(instr.starts_with(
        "You are an automated cli-companion planner. Work only inside C:\\p\\.worktrees\\plan-dark-theme"
    ));
    assert!(instr.contains(PLANNER_INSTRUCTIONS.trim_end()));
    assert!(!instr.contains(WORKER_GUARDRAILS.trim_end()));
    assert!(
        instr.contains(
            "specs in Italian

Exploration delegation."
        ) && instr.contains("Workspace rules (always apply")
    );
    let env = t["environment"].as_str().unwrap();
    assert!(env.contains("branch task/plan-dark-theme (created from main)"), "{env}");
    assert!(env.contains("C:\\p (the user's copy: never touch it)") && !env.contains("Delivery mode"), "{env}");
    assert_eq!(task(&st, &h, &id)["worker_status"], "running");
}

#[test]
fn the_planner_instructions_allow_an_answer_only_for_a_no_change_question_or_by_skill() {
    let answer = PLANNER_INSTRUCTIONS
        .lines()
        .find(|l| l.contains("starts with \"Answer:\""))
        .expect("the Answer outcome is described");
    // Regola stretta per un Bot senza skill: l'Answer non e' una scorciatoia.
    let (strict, exception) = answer.split_once("Only exception:").expect("the skill exception is separate");
    assert!(strict.contains("asks for no change: do not propose anything"), "{strict}");
    assert!(strict.contains("Never use this outcome to avoid proposing"), "{strict}");
    assert!(!strict.contains("skill"), "{strict}");
    // Una skill che dice di rispondere prevale, come vuole `code-answer`.
    assert!(exception.contains("the skill of your Bot says to answer without proposing"), "{exception}");
    assert!(exception.contains("the skill wins even if the request asks for a change"), "{exception}");
    assert!(exception.contains("suggest, in one line, that the user asks for the change"), "{exception}");
    assert!(PLANNER_INSTRUCTIONS.contains("asks for no change, unless the skill of your Bot says to answer"));
    // Gli altri due esiti restano.
    assert!(PLANNER_INSTRUCTIONS.contains("starts with \"Questions for the user:\""));
    assert!(PLANNER_INSTRUCTIONS.contains("after the proposal: status \"completed\""));
    assert!(PLANNER_INSTRUCTIONS.is_ascii());
}

#[test]
fn a_completed_plan_reports_no_delivery() {
    let (st, h, id, p) = planner(DeliveryMode::Pr, true);
    call(&st, &h, &p, "get_task", json!({})).unwrap();
    let rejected = [
        (json!({"commit": "abc1234"}), "not accepted for plan tasks"),
        (json!({"pr_url": "https://github.com/o/r/pull/1"}), "not accepted for plan tasks"),
        (json!({"verdict": "passed"}), "only accepted for review tasks"),
        (json!({"findings": ["x"]}), "only accepted for review tasks"),
    ];
    for (extra, needle) in rejected {
        let e = complete(&st, &h, &p, extra.clone()).unwrap_err();
        assert!(e.contains(needle), "{extra}: {e}");
    }
    assert_eq!(task(&st, &h, &id)["status"], "running", "rejected report leaves the plan running");
    complete(&st, &h, &p, json!({})).unwrap();
    let t = task(&st, &h, &id);
    assert_eq!((t["status"].as_str(), t["report"]["summary"].as_str()), (Some("done"), Some("proposed 2 tasks")));
    assert!(t["report"].get("diffstat").is_none(), "{t}");
    assert!(!h.git_log().iter().any(|l| l.contains("diff --stat")), "{:?}", h.git_log());
}

#[test]
fn a_plan_with_questions_completes_and_a_failed_plan_fails() {
    let (st, h, id, p) = planner(DeliveryMode::Commit, false);
    complete(&st, &h, &p, json!({"summary": "Questions for the user:\n1. Which pages?"})).unwrap();
    assert_eq!(task(&st, &h, &id)["status"], "done");
    let (st, h, id, p) = planner(DeliveryMode::Commit, false);
    call(&st, &h, &p, "complete_task", json!({"status": "failed", "summary": "no repo access", "commit": "abc"}))
        .unwrap();
    assert_eq!(task(&st, &h, &id)["status"], "failed");
}

#[test]
fn a_planner_proposes_once_with_its_task_as_source() {
    let (st, h, id, p) = planner(DeliveryMode::Commit, false);
    // Una proposta rifiutata dalla validazione non conta.
    let e = call(&st, &h, &p, "propose_tasks", json!({"tasks": [spec("a", &["ghost"])]})).unwrap_err();
    assert!(e.contains("unknown task 'ghost'"), "{e}");
    let r = call(&st, &h, &p, "propose_tasks", json!({"tasks": [spec("a", &[]), spec("b", &["a"])]})).unwrap();
    assert_eq!(r["status"], "pending");
    let proposals = view(&st, &h).proposals;
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        (proposals[0].source_task.as_deref(), proposals[0].source_tab.as_str()),
        (Some(id.as_str()), p.as_str())
    );
    let e = call(&st, &h, &p, "propose_tasks", json!({"tasks": [spec("c", &[])]})).unwrap_err();
    assert_eq!(e, PLAN_PROPOSED_ONCE);
    // Anche dopo la decisione dell'utente.
    boards::proposal_decide(&st, &h, "ws-1", &proposals[0].id, false).unwrap();
    assert_eq!(call(&st, &h, &p, "propose_tasks", json!({"tasks": [spec("c", &[])]})).unwrap_err(), PLAN_PROPOSED_ONCE);
    assert!(view(&st, &h).proposals.is_empty());
    // Le regole dell'Intake valgono anche qui.
    let (st, h, _, p) = planner(DeliveryMode::Commit, false);
    let mut e2e = spec("e", &[]);
    e2e["e2e"] = json!(true);
    let err = call(&st, &h, &p, "propose_tasks", json!({"tasks": [e2e]})).unwrap_err();
    assert!(err.contains("e2e requires an E2E command"), "{err}");
    let mut nested = spec("n", &[]);
    nested["kind"] = json!("plan");
    assert!(call(&st, &h, &p, "propose_tasks", json!({"tasks": [nested]})).unwrap_err().contains(PLAN_KIND_RESERVED));
}

#[test]
fn a_plan_reads_the_code_in_its_own_worktree_even_without_worktree_delivery() {
    let h = FakeHost::with_delivery(DeliveryMode::LocalMerge, false);
    // Checkout dell'utente su un altro branch e setup configurato: al piano non importa.
    *h.branch.lock().unwrap() = Some("feature".into());
    h.repo_cfg.lock().unwrap().insert(
        ".".into(),
        RepoConfig { base_branch: Some("main".into()), setup: vec!["npm ci".into()], ..Default::default() },
    );
    let st = AutomationState::default();
    let (id, p) = started_plan(&st, &h);
    let t = task(&st, &h, &id);
    assert_eq!(
        (t["worktree"]["path"].as_str(), t["worktree"]["branch"].as_str(), t["worktree"]["base"].as_str()),
        (Some("C:\\p\\.worktrees\\plan-dark-theme"), Some("task/plan-dark-theme"), Some("main"))
    );
    assert!(h
        .git_log()
        .contains(&"C:\\p|worktree add C:\\p\\.worktrees\\plan-dark-theme -b task/plan-dark-theme main".to_string()));
    assert!(h.hook_log.lock().unwrap().is_empty(), "no setup for a plan: {:?}", h.hook_log.lock().unwrap());
    let spawn = h.spawns.lock().unwrap().iter().find(|s| s.tab_id == p).cloned().unwrap();
    assert_eq!(spawn.cwd.as_deref(), Some("C:\\p\\.worktrees\\plan-dark-theme"));
}

#[test]
fn a_running_plan_does_not_hold_the_checkout_in_place_modes() {
    let (st, h, plan_id, _) = planner(DeliveryMode::Commit, false);
    queue(&st, &h, vec![spec("w", &[]), spec("w2", &[])]);
    assert_eq!(task(&st, &h, &plan_id)["status"], "running");
    assert_eq!(task(&st, &h, "w")["status"], "running", "the plan does not occupy the repository");
    assert_eq!(task(&st, &h, "w2")["status"], "queued", "in place: still one work task per repository");
}

#[test]
fn a_finished_plan_is_cleaned_without_any_delivery() {
    for (mode, branch_cmd) in
        [(DeliveryMode::Pr, "branch -D task/plan-dark-theme"), (DeliveryMode::Commit, "branch -d task/plan-dark-theme")]
    {
        let (st, h, id, p) = planner(mode, mode == DeliveryMode::Pr);
        h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\plan-dark-theme".to_string());
        call(&st, &h, &p, "get_task", json!({})).unwrap();
        complete(&st, &h, &p, json!({})).unwrap();
        tab_closed(&st, &h, &p);
        let git = h.git_log();
        assert!(
            git.contains(&"C:\\p|worktree remove C:\\p\\.worktrees\\plan-dark-theme".to_string()),
            "{mode:?}: {git:?}"
        );
        assert!(git.contains(&format!("C:\\p|{branch_cmd}")), "{mode:?}: {git:?}");
        for forbidden in ["merge", "push", "rebase", "commit"] {
            assert!(!git.iter().any(|l| l.contains(forbidden)), "{mode:?} {forbidden}: {git:?}");
        }
        assert!(task(&st, &h, &id)["worktree"].is_null());
    }
}
