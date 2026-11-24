//! Migrazioni del file della board legate ai goal (obiettivi), introdotti nel
//! passo 2 -> 3 e rimossi dal 3 -> 4. Il primo resta perche' un file della
//! versione 2 deve poter salire fino a oggi; il secondo toglie ogni traccia dei
//! goal da una board salvata con la funzione ormai eliminata.

use serde_json::{json, Value};
use std::collections::HashSet;

/// Chiave dei goal nella board.
const GOALS_KEY: &str = "goals";
/// Tipo del task di consegna del goal, che non esiste piu'.
const DELIVER_KIND: &str = "deliver";
/// Campi di un task che appartenevano ai goal (appartenenza e giro).
const TASK_GOAL_KEYS: [&str; 2] = ["goal", "round"];
/// Campi di una proposta che appartenevano ai goal (goal nuovo o aperto, correzione trattenuta).
const PROPOSAL_GOAL_KEYS: [&str; 3] = ["goal", "goal_id", "auto_fix_hold"];

/// Passo 2 -> 3: aggiunge `goals: []` se manca; il resto passa invariato.
pub(super) fn v2_to_v3(mut board: Value) -> Result<Value, String> {
    if let Some(o) = board.as_object_mut() {
        o.entry(GOALS_KEY).or_insert_with(|| json!([]));
    }
    Ok(board)
}

/// Passo 3 -> 4: toglie i goal. Via la lista `goals`, via i task di consegna
/// (`kind: "deliver"`, anche dalle `depends_on` degli altri task e delle
/// proposte) e via i campi dei goal da task e proposte. I task del goal restano
/// task sciolti, con il loro stato; i branch `goal/<id>` su disco non si toccano.
pub(super) fn v3_to_v4(mut board: Value) -> Result<Value, String> {
    let Some(o) = board.as_object_mut() else {
        return Ok(board);
    };
    o.remove(GOALS_KEY);
    let mut gone: HashSet<String> = HashSet::new();
    if let Some(tasks) = o.get_mut("tasks").and_then(Value::as_array_mut) {
        tasks.retain(|t| {
            let deliver = t.get("kind").and_then(Value::as_str) == Some(DELIVER_KIND);
            if deliver {
                gone.extend(t.get("id").and_then(Value::as_str).map(str::to_string));
            }
            !deliver
        });
        for t in tasks.iter_mut() {
            strip(t, &TASK_GOAL_KEYS);
            drop_dependencies(t, &gone);
        }
    }
    if let Some(proposals) = o.get_mut("proposals").and_then(Value::as_array_mut) {
        for p in proposals.iter_mut() {
            strip(p, &PROPOSAL_GOAL_KEYS);
            for spec in p.get_mut("tasks").and_then(Value::as_array_mut).into_iter().flatten() {
                drop_dependencies(spec, &gone);
            }
        }
    }
    Ok(board)
}

/// Toglie dall'oggetto `v` le chiavi `keys`.
fn strip(v: &mut Value, keys: &[&str]) {
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
}

/// Toglie da `depends_on` di `v` gli id in `gone`.
fn drop_dependencies(v: &mut Value, gone: &HashSet<String>) {
    if gone.is_empty() {
        return;
    }
    if let Some(deps) = v.get_mut("depends_on").and_then(Value::as_array_mut) {
        deps.retain(|d| d.as_str().is_none_or(|d| !gone.contains(d)));
    }
}

#[cfg(test)]
mod tests;
