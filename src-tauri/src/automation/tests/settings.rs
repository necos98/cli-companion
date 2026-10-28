//! Tool MCP delle impostazioni del workspace e decisione dell'utente.

use super::*;
use crate::agents::{builtin_launch_list, PermissionMode};
use crate::automation::settings::{settings_proposal_decide, SettingsDecision, REASON_MAX};
use crate::workspaces::{Launch, MAX_RULES_LEN};

const REASON: &str = "the project needs it";

fn propose(st: &AutomationState, h: &FakeHost, mut params: Value) -> Result<Value, String> {
    if params.get("reason").is_none() {
        params["reason"] = json!(REASON);
    }
    call(st, h, INTAKE, "propose_settings", params)
}

fn propose_ok(st: &AutomationState, h: &FakeHost, params: Value) -> String {
    propose(st, h, params).unwrap()["proposal"].as_str().unwrap().to_string()
}

fn read_settings(st: &AutomationState, h: &FakeHost) -> Value {
    call(st, h, INTAKE, "get_workspace_settings", json!({})).unwrap()
}

fn decide(st: &AutomationState, h: &FakeHost, id: &str, accept: bool) -> Result<SettingsDecision, String> {
    settings_proposal_decide(st, h, "ws-1", id, accept)
}

fn pending(st: &AutomationState, h: &FakeHost) -> usize {
    view(st, h).settings_proposals.len()
}

fn ws1(h: &FakeHost) -> Workspace {
    h.ws_for("ws-1").unwrap()
}

fn launch(id: &str) -> Launch {
    Launch { id: id.into(), name: format!("name {id}"), command: format!("cmd {id}"), is_default: false }
}

/// "ws-1" con due repository rilevati, uno configurato e uno solo configurato.
fn host_with_repos() -> FakeHost {
    let h = FakeHost::default();
    *h.repos.lock().unwrap() = vec!["api".into(), "web".into()];
    let mut cfg = h.repo_cfg.lock().unwrap();
    cfg.insert("api".into(), RepoConfig { base_branch: Some("dev".into()), setup: vec!["npm ci".into()], ..Default::default() });
    cfg.insert("old".into(), RepoConfig { remove_force: true, ..Default::default() });
    drop(cfg);
    h
}

// --- ruoli e congelamento ---

#[test]
fn workers_cannot_use_the_settings_tools() {
    let (st, h, worker) = running(DeliveryMode::Commit, false);
    for method in ["get_workspace_settings", "propose_settings"] {
        let err = call(&st, &h, &worker, method, json!({"reason": REASON, "worker_rules": "x"})).unwrap_err();
        assert_eq!(err, format!("{method} is not available to automated sessions"));
    }
    assert_eq!(pending(&st, &h), 0);
}

#[test]
fn an_archived_workspace_is_frozen() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    *h.archived.lock().unwrap() = true;
    for method in ["get_workspace_settings", "propose_settings"] {
        let err = call(&st, &h, INTAKE, method, json!({"reason": REASON, "worker_rules": "x"})).unwrap_err();
        assert_eq!(err, "workspace is archived");
    }
}

#[test]
fn a_tab_without_workspace_gets_a_clear_error() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let err = call(&st, &h, "tab-nowhere", "get_workspace_settings", json!({})).unwrap_err();
    assert_eq!(err, "the workspace of this tab was not found");
}

// --- get_workspace_settings ---

#[test]
fn get_settings_reports_values_repos_allowed_values_and_limits() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    let mut base = ws("ws-1", "Proj", "windows");
    base.launches = vec![launch("l-1")];
    base.worker_allow = vec!["Bash(git:*)".into()];
    base.default_launch = Some("l-1".into());
    *h.ws1_base.lock().unwrap() = Some(base);
    *h.worker_rules.lock().unwrap() = "Never push".into();
    *h.worker_model.lock().unwrap() = Some("opus".into());
    h.set_delivery(DeliveryMode::Pr, true);
    let id = propose_ok(&st, &h, json!({"use_schedule": true}));

    let out = read_settings(&st, &h);
    assert_eq!(
        out["settings"],
        json!({
            "worker_rules": "Never push",
            "worker_model": "opus",
            "worker_permission": "accept-edits",
            "worker_allow": ["Bash(git:*)"],
            "worker_deny": [],
            "delivery": {"mode": "pr", "parallel": true, "merge_method": "squash"},
            "use_schedule": false,
            "default_launch": "l-1",
            "launches": [{"id": "l-1", "name": "name l-1", "command": "cmd l-1"}],
        })
    );
    let repos = out["repos"].as_array().unwrap();
    let rows: Vec<(&str, bool, bool)> =
        repos.iter().map(|r| (r["repo"].as_str().unwrap(), r["detected"].as_bool().unwrap(), !r["config"].is_null())).collect();
    assert_eq!(rows, [("api", true, true), ("old", false, true), ("web", true, false)]);
    assert_eq!(repos[0]["config"]["base_branch"], "dev");
    assert_eq!(repos[0]["config"]["setup"], json!(["npm ci"]));
    assert!(out.get("repos_error").is_none());

    let allowed = &out["allowed"];
    assert_eq!(allowed["worker_permission"], json!(["plan", "default", "accept-edits", "bypass"]));
    assert_eq!(allowed["delivery_mode"], json!(["pr", "local_merge", "push", "commit"]));
    assert_eq!(allowed["merge_method"], json!(["squash", "merge", "rebase"]));
    let launches: Vec<&str> = allowed["default_launch"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(&launches[..2], ["shell", "l-1"]);
    let builtin: Vec<String> = builtin_launch_list().into_iter().map(|b| b.id).collect();
    assert!(!builtin.is_empty() && launches[2..] == builtin[..]);

    assert_eq!(out["limits"]["reason_chars"], REASON_MAX);
    assert_eq!(out["limits"]["worker_rules_chars"], MAX_RULES_LEN);
    for key in [
        "permission_rules", "permission_rule_chars", "launches", "launch_name_chars", "launch_command_chars",
        "repos_per_proposal", "repo_commands", "repo_command_chars",
    ] {
        assert!(out["limits"][key].as_u64().unwrap() > 0, "{key}");
    }
    assert_eq!(out["pending_settings_proposals"], json!([{"id": id, "reason": REASON}]));
}

#[test]
fn a_failing_repository_listing_does_not_fail_the_read() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    *h.repos_err.lock().unwrap() = Some("git not found".into());
    let out = read_settings(&st, &h);
    assert_eq!(out["repos_error"], "git not found");
    let rows: Vec<(&str, bool)> =
        out["repos"].as_array().unwrap().iter().map(|r| (r["repo"].as_str().unwrap(), r["detected"].as_bool().unwrap())).collect();
    assert_eq!(rows, [("api", false), ("old", false)]);
    assert!(out["settings"].is_object() && out["limits"].is_object());
}

#[test]
fn allowed_values_come_from_the_rust_types() {
    for m in DeliveryMode::ALL {
        assert_eq!(serde_json::to_value(m).unwrap(), m.as_str());
    }
    for m in MergeMethod::ALL {
        assert_eq!(serde_json::to_value(m).unwrap(), m.as_str());
    }
    for m in PermissionMode::ALL {
        assert_eq!(PermissionMode::parse(m.as_str()), Some(m));
    }
}

// --- propose_settings: rifiuti ---

#[test]
fn invalid_values_are_rejected_in_english_and_create_nothing() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    let too_long_rules = "x".repeat(MAX_RULES_LEN + 1);
    let cases = [
        (json!({"worker_rules": too_long_rules}), "worker_rules too long"),
        (json!({"worker_model": "bad model!"}), "invalid worker_model"),
        (json!({"worker_permission": "root"}), "invalid worker_permission"),
        (json!({"worker_allow": ["not a rule!"]}), "worker_allow: rule"),
        (json!({"worker_deny": ["Bash("]}), "worker_deny: rule"),
        (json!({"delivery": {}}), "delivery: has no fields"),
        (json!({"delivery": {"mode": "zip"}}), "invalid settings"),
        (json!({"use_schedule": "yes"}), "invalid settings"),
        (json!({"default_launch": "nope"}), "invalid default_launch"),
        (json!({"launches": [{"name": "", "command": "x"}]}), "launches[0]: launch name is missing"),
        (json!({"launches": [{"id": "ghost", "name": "n", "command": "x"}]}), "launches[0]: launch not found"),
        (json!({"repos": {"api": {"base_branch": "bad branch"}}}), "repos['api']: invalid base_branch"),
        (json!({"repos": {"api": {}}}), "repos['api']: has no fields"),
        (json!({"repos": {"api": {"setup": ["a\nb"]}}}), "repos['api']: setup commands must be single-line"),
    ];
    for (params, expected) in cases {
        let err = propose(&st, &h, params.clone()).unwrap_err();
        assert!(err.contains(expected), "{params}: {err}");
        assert!(err.is_ascii(), "{err}");
    }
    assert_eq!(pending(&st, &h), 0);
}

#[test]
fn the_reason_is_required_trimmed_and_limited() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let patch = |reason: Value| propose(&st, &h, json!({"reason": reason, "worker_rules": "x"}));
    assert_eq!(propose(&st, &h, json!({"worker_rules": "x", "reason": null})).unwrap_err(), "reason is missing");
    assert_eq!(patch(json!("   ")).unwrap_err(), "reason is missing");
    assert_eq!(patch(json!(5)).unwrap_err(), "reason must be a string");
    assert!(patch(json!("r".repeat(REASON_MAX + 1))).unwrap_err().contains("too long"));
    assert_eq!(pending(&st, &h), 0);
    patch(json!(format!("  {}  ", "r".repeat(REASON_MAX)))).unwrap();
    assert_eq!(view(&st, &h).settings_proposals[0].proposal.reason, "r".repeat(REASON_MAX));
}

#[test]
fn malformed_empty_and_excluded_proposals_are_rejected() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let err = |params: Value| call(&st, &h, INTAKE, "propose_settings", params).unwrap_err();
    assert_eq!(err(Value::Null), "params must be an object");
    assert_eq!(err(json!({"reason": REASON})), "nothing to change: pass at least one setting besides reason");
    assert!(err(json!({"reason": REASON, "path": "C:\\other"})).contains("`path` cannot be changed"));
    assert!(err(json!({"reason": REASON, "archived": true})).contains("cannot be changed"));
    assert!(err(json!({"reason": REASON, "theme": "dark"})).contains("unknown field"));
    assert_eq!(pending(&st, &h), 0);
}

#[test]
fn repositories_must_be_detected_or_already_configured() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    let err = propose(&st, &h, json!({"repos": {"api": {"remove_force": true}, "nope": {"remove_force": true}}})).unwrap_err();
    assert_eq!(err, "unknown repository: nope (detected repositories: api, web)");
    // Configurato ma non rilevato: si puo' ancora rimuovere.
    propose(&st, &h, json!({"repos": {"old": null, "web": {"base_branch": "main"}}})).unwrap();
    assert_eq!(pending(&st, &h), 1);

    *h.repos_err.lock().unwrap() = Some("git not found".into());
    let err = propose(&st, &h, json!({"repos": {"api": {"remove_force": true}}})).unwrap_err();
    assert_eq!(err, "cannot list the repositories of the workspace: git not found");
    // Senza repository nella proposta l'elenco non serve.
    propose(&st, &h, json!({"worker_rules": "x"})).unwrap();
}

// --- propose_settings: successo ---

#[test]
fn a_valid_proposal_is_pending_with_current_values_and_sensitive_fields() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    *h.worker_rules.lock().unwrap() = "old rules".into();
    let events = h.board_events.lock().unwrap().len();
    let out = propose(
        &st,
        &h,
        json!({
            "worker_rules": "  new rules  ",
            "worker_permission": "bypass",
            "delivery": {"mode": "pr"},
            "repos": {"api": {"setup": ["npm i"], "base_branch": null}},
        }),
    )
    .unwrap();
    assert_eq!(out["status"], "pending");
    assert!(out["message"].as_str().unwrap().contains("do not wait"));
    let id = out["proposal"].as_str().unwrap();
    assert!(id.starts_with("s-"), "{id}");

    assert!(h.board_events.lock().unwrap().len() > events, "board-update emitted");
    assert_eq!(ws1(&h).worker_rules, "old rules", "nothing changes before the decision");
    let v = view(&st, &h);
    let p = &v.settings_proposals[0];
    assert_eq!((p.proposal.id.as_str(), p.proposal.source_tab.as_str(), p.proposal.reason.as_str()), (id, INTAKE, REASON));
    assert_eq!(p.proposal.patch.worker_rules.as_deref(), Some("new rules"), "stored normalized");
    assert_eq!(p.current.worker_rules.as_deref(), Some("old rules"));
    assert_eq!(p.current.delivery.unwrap().mode, Some(DeliveryMode::Commit));
    assert_eq!(p.current.worker_permission.as_deref(), Some("accept-edits"));
    let api = p.current.repos.as_ref().unwrap()["api"].as_ref().unwrap();
    assert_eq!((api.setup.clone(), api.base_branch.clone()), (Some(vec!["npm ci".to_string()]), Some(Some("dev".to_string()))));
    assert_eq!(p.sensitive, ["worker_permission", "repos[api].setup"]);

    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["settings_proposals"][0]["patch"]["delivery"], json!({"mode": "pr"}));
    assert_eq!(json["settings_proposals"][0]["sensitive"], json!(["worker_permission", "repos[api].setup"]));
}

#[test]
fn settings_and_task_proposals_have_separate_caps_and_distinct_ids() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let mut ids = Vec::new();
    for i in 0..crate::board::MAX_PENDING_PROPOSALS {
        ids.push(propose_ok(&st, &h, json!({"worker_rules": format!("r{i}")})));
    }
    let err = propose(&st, &h, json!({"worker_rules": "over"})).unwrap_err();
    assert!(err.contains("too many pending settings proposals"), "{err}");
    assert_eq!(pending(&st, &h), crate::board::MAX_PENDING_PROPOSALS);

    let task = call(&st, &h, INTAKE, "propose_tasks", json!({"tasks": [spec("a", &[])]})).unwrap();
    ids.push(task["proposal"].as_str().unwrap().to_string());
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    // Decidere una libera un posto.
    decide(&st, &h, &ids[0], false).unwrap();
    propose(&st, &h, json!({"worker_rules": "again"})).unwrap();
}

// --- decisione ---

#[test]
fn accepting_applies_everything_at_once_and_removes_the_proposal() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    let id = propose_ok(
        &st,
        &h,
        json!({
            "worker_rules": "Never push",
            "worker_model": "opus",
            "worker_permission": "plan",
            "worker_allow": ["Bash(npm test:*)"],
            "worker_deny": ["Bash(rm:*)"],
            "delivery": {"mode": "pr", "parallel": true, "merge_method": "rebase"},
            "use_schedule": true,
            "default_launch": "shell",
            "launches": [{"name": "Dev", "command": "npm run dev"}],
            "repos": {"api": {"base_branch": "main", "setup": ["npm i"]}},
        }),
    );
    let events = h.board_events.lock().unwrap().len();
    let d = decide(&st, &h, &id, true).unwrap();

    assert_eq!(h.applies(), 1);
    let w = ws1(&h);
    assert_eq!(w.worker_rules, "Never push");
    assert_eq!(w.worker_model.as_deref(), Some("opus"));
    assert_eq!(w.worker_permission, "plan");
    assert_eq!((w.worker_allow.clone(), w.worker_deny.clone()), (vec!["Bash(npm test:*)".to_string()], vec!["Bash(rm:*)".to_string()]));
    assert_eq!((w.delivery.mode, w.delivery.parallel, w.delivery.merge_method), (DeliveryMode::Pr, true, MergeMethod::Rebase));
    assert!(w.use_schedule);
    assert_eq!(w.default_launch.as_deref(), Some("shell"));
    assert_eq!(w.launches.len(), 1);
    assert_eq!((w.launches[0].name.as_str(), w.launches[0].command.as_str()), ("Dev", "npm run dev"));
    assert_eq!(w.repos["api"].base_branch.as_deref(), Some("main"));
    assert_eq!(w.repos["api"].setup, ["npm i"]);
    assert!(w.repos.contains_key("old"), "untouched repos stay");

    assert_eq!(d.workspace.as_ref().map(|w| w.worker_rules.as_str()), Some("Never push"));
    assert!(d.board.settings_proposals.is_empty());
    assert_eq!(d.board.delivery.mode, DeliveryMode::Pr, "the view reflects the new delivery");
    assert!(h.board_events.lock().unwrap().len() > events, "board-update emitted");
    assert!(h.board_events.lock().unwrap().last().unwrap().settings_proposals.is_empty());
    assert!(h.saved.lock().unwrap()["ws-1"].settings_proposals.is_empty());
}

#[test]
fn the_scheduler_is_realigned_only_when_delivery_or_schedule_change() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let rounds = || h.schedule_events.lock().unwrap().len();

    let id = propose_ok(&st, &h, json!({"worker_rules": "x", "worker_permission": "plan"}));
    decide(&st, &h, &id, true).unwrap();
    assert_eq!(rounds(), 0);

    for patch in [json!({"use_schedule": true}), json!({"delivery": {"parallel": false}})] {
        let before = rounds();
        let id = propose_ok(&st, &h, patch);
        decide(&st, &h, &id, true).unwrap();
        assert_eq!(rounds(), before + 1);
    }

    // Scartare non cambia nulla.
    let before = rounds();
    let id = propose_ok(&st, &h, json!({"use_schedule": false}));
    decide(&st, &h, &id, false).unwrap();
    assert_eq!(rounds(), before);
}

#[test]
fn discarding_removes_the_proposal_and_leaves_the_settings_alone() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let id = propose_ok(&st, &h, json!({"worker_rules": "x", "use_schedule": true}));
    let events = h.board_events.lock().unwrap().len();
    let d = decide(&st, &h, &id, false).unwrap();
    assert!(d.workspace.is_none());
    assert!(d.board.settings_proposals.is_empty());
    assert_eq!(pending(&st, &h), 0);
    assert_eq!(h.applies(), 0);
    let w = ws1(&h);
    assert_eq!((w.worker_rules.as_str(), w.use_schedule), ("", false));
    assert!(h.board_events.lock().unwrap().len() > events);
}

#[test]
fn a_failed_revalidation_keeps_the_proposal_and_changes_nothing() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let mut base = ws("ws-1", "Proj", "windows");
    base.launches = vec![launch("l-1")];
    *h.ws1_base.lock().unwrap() = Some(base);
    let id = propose_ok(&st, &h, json!({"worker_rules": "x", "launches": [{"id": "l-1", "name": "Renamed", "command": "go"}]}));

    // Il tool dell'avvio sparisce prima della decisione.
    h.ws1_base.lock().unwrap().as_mut().unwrap().launches.clear();
    let err = decide(&st, &h, &id, true).err().unwrap();
    assert!(err.contains("avvio non trovato"), "{err}");
    assert_eq!(h.applies(), 1);
    assert_eq!(ws1(&h).worker_rules, "");
    assert_eq!(pending(&st, &h), 1, "still pending");

    // Anche un errore dell'host lascia tutto com'e'.
    *h.apply_err.lock().unwrap() = Some("impossibile salvare".into());
    assert_eq!(decide(&st, &h, &id, true).err().unwrap(), "impossibile salvare");
    assert_eq!(pending(&st, &h), 1);
    // E la proposta si puo' ancora scartare.
    assert!(decide(&st, &h, &id, false).unwrap().workspace.is_none());
    assert_eq!(pending(&st, &h), 0);
}

#[test]
fn unknown_proposals_and_archived_workspaces_are_errors() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    for accept in [true, false] {
        let err = decide(&st, &h, "s-nope", accept).err().unwrap();
        assert_eq!(err, "settings proposal not found: s-nope");
    }
    let id = propose_ok(&st, &h, json!({"worker_rules": "x"}));
    *h.archived.lock().unwrap() = true;
    assert_eq!(decide(&st, &h, &id, true).err().unwrap(), "workspace archiviato");
    assert_eq!(settings_proposal_decide(&st, &h, "ws-9", &id, true).err().unwrap(), "workspace non trovato");
    assert_eq!(h.applies(), 0);
}

#[test]
fn deciding_twice_applies_once() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let id = propose_ok(&st, &h, json!({"launches": [{"name": "Dev", "command": "go"}]}));
    assert!(decide(&st, &h, &id, true).is_ok());
    assert!(decide(&st, &h, &id, true).err().unwrap().contains("settings proposal not found"));
    assert_eq!((h.applies(), ws1(&h).launches.len()), (1, 1));
}

#[test]
fn concurrent_decisions_apply_the_proposal_once() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    let id = propose_ok(&st, &h, json!({"launches": [{"name": "Dev", "command": "go"}]}));
    let results: Vec<bool> = std::thread::scope(|s| {
        let jobs: Vec<_> = (0..4).map(|_| s.spawn(|| decide(&st, &h, &id, true).is_ok())).collect();
        jobs.into_iter().map(|j| j.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|ok| **ok).count(), 1);
    assert_eq!((h.applies(), ws1(&h).launches.len()), (1, 1));
}

#[test]
fn null_values_clear_a_field_and_null_repos_are_removed_end_to_end() {
    let (st, h) = (AutomationState::default(), host_with_repos());
    *h.worker_model.lock().unwrap() = Some("opus".into());
    let id = propose_ok(&st, &h, json!({"worker_model": null, "repos": {"api": {"base_branch": null}, "old": null}}));
    decide(&st, &h, &id, true).unwrap();
    let w = ws1(&h);
    assert_eq!(w.worker_model, None);
    assert_eq!(w.repos["api"].base_branch, None);
    assert_eq!(w.repos["api"].setup, ["npm ci"], "other fields of the repository stay");
    assert!(!w.repos.contains_key("old"));
}
