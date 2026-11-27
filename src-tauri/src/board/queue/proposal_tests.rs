//! Correzione e scarto di una proposta pendente: `update_proposal` e `discard_proposal`.

use super::*;
use crate::board::parse_proposal;
use serde_json::json;

fn specs(ids: &[&str]) -> Vec<TaskSpec> {
    let tasks: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "title": format!("Task {id}"), "task": format!("do {id}"), "bot": "general"}))
        .collect();
    parse_proposal(&json!({ "tasks": tasks })).unwrap()
}

fn ids(b: &Board, proposal: &str) -> Vec<String> {
    b.proposal(proposal).unwrap().tasks.iter().map(|t| t.id.clone()).collect()
}

/// Due proposte pendenti: `p1` (a, b) e `p2` (z), dell'Intake e di un revisore.
fn board_with_proposals(ctx: SpecContext<'_>) -> Board {
    let mut b = Board::new("ws-1");
    b.add_proposal("p1".into(), "tab-1", None, specs(&["a", "b"]), ctx, 1).unwrap();
    b.add_proposal("p2".into(), "tab-2", Some("rev".into()), specs(&["z"]), ctx, 2).unwrap();
    b
}

#[test]
fn update_replaces_the_tasks_and_keeps_the_origin() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_proposals(ctx);
    b.update_proposal("p2", specs(&["y", "x"]), ctx, 9).unwrap();
    assert_eq!(ids(&b, "p2"), ["y", "x"]);
    let p = b.proposal("p2").unwrap();
    assert_eq!((p.source_tab.as_str(), p.source_task.as_deref(), p.created_at), ("tab-2", Some("rev"), 2));
    assert_eq!(b.updated_at, 9);
    assert_eq!(ids(&b, "p1"), ["a", "b"], "the other proposal is untouched");
    // Gli stessi id della proposta si possono riusare: non confliggono con se stessa.
    b.update_proposal("p1", specs(&["a"]), ctx, 10).unwrap();
    assert_eq!(ids(&b, "p1"), ["a"]);
}

#[test]
fn an_invalid_update_changes_nothing() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_proposals(ctx);
    let before = b.clone();
    let mut cyclic = specs(&["a", "b"]);
    cyclic[0].depends_on = vec!["b".into()];
    cyclic[1].depends_on = vec!["a".into()];
    assert!(b.update_proposal("p1", cyclic, ctx, 9).is_err());
    let mut bad_bot = specs(&["a"]);
    bad_bot[0].bot = Some("ghost".into());
    assert!(b.update_proposal("p1", bad_bot, ctx, 9).is_err());
    assert!(b.update_proposal("p1", Vec::new(), ctx, 9).unwrap_err().contains("tasks is empty"));
    assert_eq!(b, before);
}

#[test]
fn an_update_cannot_reuse_the_ids_of_another_proposal_or_of_the_queue() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_proposals(ctx);
    b.create_task(specs(&["q"]).remove(0), ctx, 3).unwrap();
    let before = b.clone();
    let clash = b.update_proposal("p1", specs(&["a", "z"]), ctx, 9).unwrap_err();
    assert_eq!(clash, "duplicate task id: z (already used by a pending proposal)");
    let queued = b.update_proposal("p1", specs(&["q"]), ctx, 9).unwrap_err();
    assert_eq!(queued, "duplicate task id: q");
    assert_eq!(b, before);
}

#[test]
fn updating_or_discarding_an_unknown_proposal_fails() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_proposals(ctx);
    assert_eq!(b.update_proposal("nope", specs(&["a"]), ctx, 9).unwrap_err(), "proposal not found: nope");
    assert_eq!(b.discard_proposal("nope", 9).unwrap_err(), "proposal not found: nope");
    assert_eq!(b.proposals.len(), 2);
}

#[test]
fn discard_removes_only_that_proposal_and_frees_its_ids() {
    let list = crate::bots::default_agent_bots();
    let ctx = SpecContext { workspace_agent: None, bots: &list };
    let mut b = board_with_proposals(ctx);
    b.discard_proposal("p1", 9).unwrap();
    assert!(b.proposal("p1").is_err());
    assert_eq!(ids(&b, "p2"), ["z"]);
    assert!(b.tasks.is_empty());
    b.update_proposal("p2", specs(&["a", "b"]), ctx, 10).unwrap();
}
