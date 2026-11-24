use super::*;
use crate::board::{Board, TaskKind, TaskStatus};

const NOW: i64 = 1_800_000_000_000;

#[test]
fn a_board_gains_an_empty_goal_list_and_keeps_its_tasks_loose() {
    let before = json!({
        "workspace_id": "ws-1",
        "tasks": [{ "id": "a", "status": "done" }, { "id": "b", "depends_on": ["a"] }],
        "proposals": [{ "id": "p-1", "source_tab": "t", "created_at": 1, "tasks": [] }]
    });
    let after = v2_to_v3(before.clone()).unwrap();
    assert_eq!(after["goals"], json!([]));
    let mut rest = after.clone();
    rest.as_object_mut().unwrap().remove("goals");
    assert_eq!(rest, before, "nothing else changes: no goal is rebuilt from the tasks");
}

#[test]
fn existing_goals_and_non_objects_pass_unchanged() {
    let with_goals = json!({ "workspace_id": "ws-1", "goals": [{ "id": "g" }] });
    assert_eq!(v2_to_v3(with_goals.clone()).unwrap(), with_goals);
    assert_eq!(v2_to_v3(json!([1, 2])).unwrap(), json!([1, 2]));
}

/// Una board della versione 3 con un goal, due task del goal, il task di consegna
/// che dipende da loro, un task sciolto che dipende dalla consegna e due proposte.
fn v3_board() -> Value {
    json!({
        "workspace_id": "ws-1",
        "max_parallel": 3,
        "goals": [{ "id": "g-1", "title": "Export", "request": "Export the orders", "round": 2 }],
        "tasks": [
            { "id": "a", "title": "A", "task": "do a", "status": "done", "goal": "g-1", "round": 1 },
            { "id": "b", "title": "B", "task": "do b", "status": "queued", "depends_on": ["a"], "goal": "g-1", "round": 2 },
            { "id": "d", "title": "Deliver", "task": "deliver", "status": "queued", "kind": "deliver",
              "depends_on": ["a", "b"], "goal": "g-1", "round": 2 },
            { "id": "loose", "title": "L", "task": "do l", "status": "queued", "depends_on": ["d", "a"] }
        ],
        "proposals": [
            { "id": "p-1", "source_tab": "t", "created_at": 1, "goal": { "title": "G", "request": "r" },
              "tasks": [{ "id": "n", "title": "N", "task": "do n", "depends_on": ["a"] }] },
            { "id": "p-2", "source_tab": "t", "source_task": "a", "goal_id": "g-1", "created_at": 2,
              "auto_fix_hold": { "reason": "x" },
              "tasks": [{ "id": "m", "title": "M", "task": "do m", "depends_on": ["d"] }] }
        ],
        "settings_proposals": []
    })
}

#[test]
fn the_fourth_step_drops_goals_deliveries_and_goal_fields() {
    let v = v3_to_v4(v3_board()).unwrap();
    assert!(v.get("goals").is_none());
    let ids: Vec<&str> = v["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a", "b", "loose"]);
    for t in v["tasks"].as_array().unwrap() {
        assert!(t.get("goal").is_none() && t.get("round").is_none(), "{t}");
    }
    assert_eq!(v["tasks"][1]["depends_on"], json!(["a"]));
    assert_eq!(v["tasks"][2]["depends_on"], json!(["a"]), "the delivery is gone from the dependencies");
    for p in v["proposals"].as_array().unwrap() {
        for k in PROPOSAL_GOAL_KEYS {
            assert!(p.get(k).is_none(), "{k}");
        }
    }
    assert_eq!(v["proposals"][1]["source_task"], "a");
    assert_eq!(v["proposals"][0]["tasks"][0]["depends_on"], json!(["a"]));
    assert_eq!(v["proposals"][1]["tasks"][0]["depends_on"], json!([]));
}

#[test]
fn a_board_without_goals_passes_the_fourth_step_unchanged() {
    let plain = json!({
        "workspace_id": "ws-1",
        "tasks": [{ "id": "a", "depends_on": [] }, { "id": "b", "depends_on": ["a"], "kind": "review" }],
        "proposals": []
    });
    assert_eq!(v3_to_v4(plain.clone()).unwrap(), plain);
    assert_eq!(v3_to_v4(json!([1])).unwrap(), json!([1]));
}

#[test]
fn a_version_three_file_loads_as_a_board_of_loose_tasks() {
    let raw = json!({ "schema_version": 3, "data": v3_board() }).to_string();
    let b = Board::from_json(&raw, NOW).unwrap();
    let ids: Vec<&str> = b.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "loose"]);
    assert_eq!(b.task("a").unwrap().status(), TaskStatus::Done);
    assert!(b.tasks.iter().all(|t| t.kind == TaskKind::Work));
    assert_eq!(b.proposals.len(), 2);
    assert_eq!(b.task("loose").unwrap().depends_on, ["a"]);
}

#[test]
fn a_version_two_file_climbs_through_both_goal_steps() {
    let mut v2 = v3_board();
    v2.as_object_mut().unwrap().remove("goals");
    let raw = json!({ "schema_version": 2, "data": v2 }).to_string();
    let b = Board::from_json(&raw, NOW).unwrap();
    assert_eq!(b.tasks.len(), 3);
}
