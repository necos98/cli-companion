use super::*;
use crate::board::TaskSpec;
use crate::workspaces::DeliveryMode;
use serde_json::json;

fn workspace() -> Workspace {
    let raw = r#"{"id":"ws-1","name":"p","kind":"windows","path":"C:\\p","created_at":1,"worker_rules":"old"}"#;
    serde_json::from_str(raw).unwrap()
}

fn patch(v: serde_json::Value) -> SettingsPatch {
    SettingsPatch::parse(&v).unwrap()
}

fn add(b: &mut Board, id: &str, now: i64) -> Result<(), String> {
    b.add_settings_proposal(id.into(), "tab-1", "because".into(), patch(json!({"worker_rules": "new"})), now)
}

#[test]
fn an_old_board_without_the_field_loads() {
    let b = Board::from_json(r#"{"workspace_id":"ws-1","tasks":[],"proposals":[]}"#, 1).unwrap();
    assert!(b.settings_proposals.is_empty());
}

#[test]
fn proposals_roundtrip_through_the_file() {
    let mut b = Board::new("ws-1");
    b.add_settings_proposal(
        "s-1".into(),
        "tab-1",
        "why".into(),
        patch(json!({"worker_model": null, "repos": {"api": {"base_branch": null}, "old": null}})),
        5,
    )
    .unwrap();
    let back = Board::from_json(&serde_json::to_string(&b).unwrap(), 9).unwrap();
    assert_eq!(back.settings_proposals, b.settings_proposals);
    assert_eq!(back.settings_proposals[0].patch.worker_model, Some(None));
}

#[test]
fn add_stores_the_proposal_and_touches_the_board() {
    let mut b = Board::new("ws-1");
    add(&mut b, "s-1", 7).unwrap();
    let p = b.settings_proposal("s-1").unwrap();
    assert_eq!((p.source_tab.as_str(), p.reason.as_str(), p.created_at), ("tab-1", "because", 7));
    assert_eq!(b.updated_at, 7);
}

#[test]
fn pending_settings_proposals_are_capped_apart_from_task_proposals() {
    let mut b = Board::new("ws-1");
    for i in 0..MAX_PENDING_PROPOSALS {
        add(&mut b, &format!("s-{i}"), 1).unwrap();
    }
    let err = add(&mut b, "s-over", 2).unwrap_err();
    assert!(err.contains("too many pending settings proposals"), "{err}");
    assert_eq!(b.settings_proposals.len(), MAX_PENDING_PROPOSALS);
    let spec = TaskSpec { id: "a".into(), title: "T".into(), task: "x".into(), ..TaskSpec::default() };
    b.add_proposal("p-1".into(), "tab-1", None, vec![spec], 3).unwrap();
}

#[test]
fn remove_returns_the_proposal_and_fails_for_unknown_ids() {
    let mut b = Board::new("ws-1");
    add(&mut b, "s-1", 1).unwrap();
    add(&mut b, "s-2", 1).unwrap();
    assert!(b.settings_proposal("s-9").unwrap_err().contains("settings proposal not found: s-9"));
    assert!(b.remove_settings_proposal("s-9", 5).is_err());
    assert_eq!(b.updated_at, 1, "a failed removal leaves the board alone");
    assert_eq!(b.remove_settings_proposal("s-1", 6).unwrap().id, "s-1");
    assert_eq!(b.updated_at, 6);
    assert_eq!(b.settings_proposals.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["s-2"]);
}

#[test]
fn the_view_computes_current_values_and_sensitive_fields_from_the_workspace() {
    let mut b = Board::new("ws-1");
    let p = patch(json!({"worker_rules": "new", "worker_permission": "bypass", "delivery": {"mode": "pr"}}));
    b.add_settings_proposal("s-1".into(), "tab-1", "why".into(), p, 1).unwrap();

    let view = b.view_with(Some(&workspace()), false);
    let v = &view.settings_proposals[0];
    assert_eq!(v.current.worker_rules.as_deref(), Some("old"));
    assert_eq!(v.current.delivery.unwrap().mode, Some(DeliveryMode::Commit));
    assert_eq!(v.sensitive, ["worker_permission"]);

    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["settings_proposals"][0]["id"], "s-1");
    assert_eq!(json["settings_proposals"][0]["reason"], "why");
    assert_eq!(json["settings_proposals"][0]["patch"]["worker_rules"], "new");
    assert_eq!(json["settings_proposals"][0]["current"]["worker_rules"], "old");

    let orphan = b.view_with(None, false);
    assert!(orphan.settings_proposals[0].current.is_empty());
    assert_eq!(orphan.settings_proposals[0].sensitive, ["worker_permission"]);
}
