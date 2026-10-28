//! Tracker degli snapshot di agent-browser: mantiene l'albero corrente e applica i delta.

use serde_json::Value;
use std::collections::BTreeSet;

const KIND_FULL: &str = "full";
const KIND_DELTA: &str = "delta";
/// Risposta di agent-browser quando l'albero non e' cambiato: nessun `treeChange`.
const KIND_UNCHANGED: &str = "unchanged";
const REF_MARKER: &str = "ref=";

/// Come lo snapshot differisce da quello precedente.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotChange {
    Full,
    Delta {
        removed: Vec<String>,
        added: Vec<String>,
    },
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotView {
    pub tree: String,
    pub refs: BTreeSet<String>,
    pub change: SnapshotChange,
}

#[derive(Default)]
pub struct SnapshotTracker {
    lines: Vec<String>,
    revision: Option<u64>,
    report_full_next: bool,
}

impl SnapshotTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Il prossimo `apply` riporta `Full` anche se e' un delta (lo stato attuale
    /// e' stato consumato da una lettura che il modello non ha visto).
    pub fn mark_unreported(&mut self) {
        self.report_full_next = true;
    }

    /// Applica il `data` restituito da `snapshot` (testo semplice oppure oggetto full/delta).
    pub fn apply(&mut self, data: &Value) -> Result<SnapshotView, String> {
        let snapshot = data
            .get("snapshot")
            .ok_or("snapshot assente nella risposta")?;
        let change = match snapshot {
            Value::String(tree) => {
                self.replace(tree, None);
                SnapshotChange::Full
            }
            Value::Object(_) => self.apply_object(snapshot)?,
            _ => return Err("snapshot con tipo inatteso".to_string()),
        };
        let change = if self.report_full_next {
            self.report_full_next = false;
            SnapshotChange::Full
        } else {
            change
        };
        Ok(SnapshotView {
            tree: self.lines.join("\n"),
            refs: collect_refs(&self.lines),
            change,
        })
    }

    fn apply_object(&mut self, snapshot: &Value) -> Result<SnapshotChange, String> {
        let revision = snapshot.get("revision").and_then(Value::as_u64);
        match snapshot.get("kind").and_then(Value::as_str) {
            Some(KIND_FULL) => {
                let tree = snapshot
                    .get("tree")
                    .and_then(Value::as_str)
                    .ok_or("snapshot full senza tree")?;
                self.replace(tree, revision);
                Ok(SnapshotChange::Full)
            }
            Some(KIND_DELTA | KIND_UNCHANGED) => self.apply_delta(snapshot, revision),
            other => Err(format!("kind di snapshot sconosciuto: {other:?}")),
        }
    }

    fn replace(&mut self, tree: &str, revision: Option<u64>) {
        self.lines = tree.lines().map(str::to_string).collect();
        self.revision = revision;
    }

    fn apply_delta(
        &mut self,
        snapshot: &Value,
        revision: Option<u64>,
    ) -> Result<SnapshotChange, String> {
        let base = snapshot.get("baseRevision").and_then(Value::as_u64);
        if base != self.revision {
            return Err(format!(
                "delta con baseRevision {base:?} ma la revisione corrente e' {:?}",
                self.revision
            ));
        }
        self.revision = revision;
        let Some(change) = snapshot.get("treeChange").filter(|v| v.is_object()) else {
            return Ok(SnapshotChange::Unchanged);
        };
        let start = field_usize(change, "startLine")?;
        let delete = field_usize(change, "deleteCount")?;
        let added: Vec<String> = change
            .get("lines")
            .and_then(Value::as_array)
            .ok_or("treeChange senza lines")?
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or("riga non testuale"))
            .collect::<Result<_, _>>()?;
        let end = start
            .checked_add(delete)
            .filter(|end| *end <= self.lines.len())
            .ok_or_else(|| {
                format!(
                    "treeChange fuori intervallo (start {start}, delete {delete}, righe {})",
                    self.lines.len()
                )
            })?;
        let removed: Vec<String> = self.lines.splice(start..end, added.clone()).collect();
        if removed.is_empty() && added.is_empty() {
            return Ok(SnapshotChange::Unchanged);
        }
        Ok(SnapshotChange::Delta { removed, added })
    }
}

fn field_usize(value: &Value, key: &str) -> Result<usize, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("treeChange.{key} mancante o non valido"))
}

/// Estrae gli id (`e12`) dalle righe dell'albero (`[... ref=e12]`).
fn collect_refs(lines: &[String]) -> BTreeSet<String> {
    let mut refs = BTreeSet::new();
    for line in lines {
        let mut rest = line.as_str();
        while let Some(pos) = rest.find(REF_MARKER) {
            rest = &rest[pos + REF_MARKER.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !id.is_empty() {
                refs.insert(id);
            }
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TREE: &str =
        "- textbox \"Name\" [ref=e1]\n- textbox \"Email\" [ref=e2]\n- button \"Go\" [ref=e3]";

    fn full(revision: u64, tree: &str) -> Value {
        json!({"snapshot": {"kind": "full", "revision": revision, "refs": {}, "tree": tree}})
    }

    fn delta(base: u64, start: usize, delete: usize, lines: &[&str]) -> Value {
        json!({"snapshot": {"kind": "delta", "baseRevision": base, "revision": base + 1, "changes": [],
            "treeChange": {"startLine": start, "deleteCount": delete, "lines": lines}}})
    }

    #[test]
    fn plain_snapshot_is_full_with_refs() {
        let mut tracker = SnapshotTracker::new();
        let view = tracker.apply(&json!({"snapshot": TREE})).unwrap();
        assert_eq!(view.change, SnapshotChange::Full);
        assert_eq!(view.tree, TREE);
        assert_eq!(view.refs.len(), 3);
        assert!(view.refs.contains("e2"));
    }

    #[test]
    fn delta_replaces_a_line() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        let view = tracker
            .apply(&delta(1, 1, 1, &["- textbox \"Email\" [ref=e2]: a@b.c"]))
            .unwrap();
        assert_eq!(
            view.change,
            SnapshotChange::Delta {
                removed: vec!["- textbox \"Email\" [ref=e2]".to_string()],
                added: vec!["- textbox \"Email\" [ref=e2]: a@b.c".to_string()],
            }
        );
        assert!(view.tree.contains("a@b.c"));
        assert_eq!(view.tree.lines().count(), 3);
    }

    #[test]
    fn sequential_deltas_apply_in_order() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        tracker
            .apply(&delta(1, 3, 0, &["- link \"Home\" [ref=e4]"]))
            .unwrap();
        let view = tracker.apply(&delta(2, 0, 1, &[])).unwrap();
        assert_eq!(view.tree.lines().count(), 3);
        assert!(!view.refs.contains("e1"));
        assert!(view.refs.contains("e4"));
    }

    #[test]
    fn full_resets_after_delta() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        tracker
            .apply(&delta(1, 0, 3, &["- link \"X\" [ref=e9]"]))
            .unwrap();
        let view = tracker
            .apply(&full(3, "- button \"New\" [ref=e1]"))
            .unwrap();
        assert_eq!(view.change, SnapshotChange::Full);
        assert_eq!(view.tree, "- button \"New\" [ref=e1]");
        let view = tracker
            .apply(&delta(3, 1, 0, &["- link \"Y\" [ref=e2]"]))
            .unwrap();
        assert!(matches!(view.change, SnapshotChange::Delta { .. }));
    }

    #[test]
    fn missing_tree_change_is_unchanged() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        let data =
            json!({"snapshot": {"kind": "delta", "baseRevision": 1, "revision": 2, "changes": []}});
        assert_eq!(
            tracker.apply(&data).unwrap().change,
            SnapshotChange::Unchanged
        );
    }

    #[test]
    fn unchanged_kind_keeps_tree_and_advances_revision() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        let data = json!({"snapshot": {"kind": "unchanged", "baseRevision": 1, "revision": 2}});
        let view = tracker.apply(&data).unwrap();
        assert_eq!(view.change, SnapshotChange::Unchanged);
        assert_eq!(view.tree, TREE);
        let next = tracker
            .apply(&delta(2, 2, 1, &["- button \"Send\" [ref=e3]"]))
            .unwrap();
        assert!(matches!(next.change, SnapshotChange::Delta { .. }));
    }

    #[test]
    fn empty_replacement_is_unchanged() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        let view = tracker.apply(&delta(1, 2, 0, &[])).unwrap();
        assert_eq!(view.change, SnapshotChange::Unchanged);
    }

    #[test]
    fn out_of_range_delta_is_an_error() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        assert!(tracker.apply(&delta(1, 2, 5, &[])).is_err());
    }

    #[test]
    fn wrong_base_revision_is_an_error() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(4, TREE)).unwrap();
        assert!(tracker.apply(&delta(1, 0, 0, &["x"])).is_err());
    }

    #[test]
    fn unreported_marker_forces_full_once() {
        let mut tracker = SnapshotTracker::new();
        tracker.apply(&full(1, TREE)).unwrap();
        tracker.mark_unreported();
        let view = tracker
            .apply(&delta(1, 0, 1, &["- link \"Z\" [ref=e1]"]))
            .unwrap();
        assert_eq!(view.change, SnapshotChange::Full);
        let view = tracker
            .apply(&delta(2, 0, 1, &["- link \"W\" [ref=e1]"]))
            .unwrap();
        assert!(matches!(view.change, SnapshotChange::Delta { .. }));
    }

    #[test]
    fn refs_are_read_from_mixed_attribute_lists() {
        let lines = vec![
            "- checkbox \"I accept\" [checked=false, ref=e3]".to_string(),
            "  - option \"Italy\" [selected, ref=e8]".to_string(),
        ];
        let refs = collect_refs(&lines);
        assert_eq!(refs.into_iter().collect::<Vec<_>>(), vec!["e3", "e8"]);
    }
}
