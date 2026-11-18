//! Migrazione del file della board per i goal (docs/GOALS.MD, "Migrazione"):
//! la board guadagna `goals`, vuoto. Nessun goal si ricostruisce dai task
//! esistenti: restano task sciolti.

use serde_json::{json, Value};

/// Chiave dei goal nella board.
const GOALS_KEY: &str = "goals";

/// Passo 2 -> 3: aggiunge `goals: []` se manca; il resto passa invariato.
pub(super) fn v2_to_v3(mut board: Value) -> Result<Value, String> {
    if let Some(o) = board.as_object_mut() {
        o.entry(GOALS_KEY).or_insert_with(|| json!([]));
    }
    Ok(board)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
