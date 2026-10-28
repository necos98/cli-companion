//! Task di revisione (`kind: "review"`): tipo del task, verdetto, validazione
//! del verdetto e riepilogo dei task revisionati per il revisore. Un task di
//! revisione e' un task normale con `depends_on` sui task da verificare: lo
//! scheduler lo avvia quando sono tutti `done`. Funzioni pure, nessun I/O.

use super::{clip_with_ellipsis, Board, ReportFields, Task, TaskSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(test)]
mod tests;

/// Lunghezza massima del riepilogo di un task revisionato.
const REVIEWED_SUMMARY_MAX: usize = 1_000;
/// Lunghezza massima del diffstat di un task revisionato.
const REVIEWED_DIFFSTAT_MAX: usize = 1_500;
/// Elementi massimi di `files_changed`, `tests` e `follow_ups` per task.
const REVIEWED_LIST_MAX: usize = 30;
/// Lunghezza massima di un follow-up e dei dettagli di un test.
const REVIEWED_ITEM_MAX: usize = 300;

/// Tipo di un task: `work` consegna, `review` verifica il risultato integrato.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    #[default]
    Work,
    Review,
}

impl TaskKind {
    pub const ALL: [TaskKind; 2] = [TaskKind::Work, TaskKind::Review];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::Work => "work",
            TaskKind::Review => "review",
        }
    }

    pub fn parse(s: &str) -> Option<TaskKind> {
        TaskKind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Esito di una revisione completata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Passed,
    Issues,
}

impl Verdict {
    pub const ALL: [Verdict; 2] = [Verdict::Passed, Verdict::Issues];

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Passed => "passed",
            Verdict::Issues => "issues",
        }
    }

    pub fn parse(s: &str) -> Option<Verdict> {
        Verdict::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Una revisione senza dipendenze non avrebbe nulla da verificare.
pub fn check_review_deps(spec: &TaskSpec) -> Result<(), String> {
    if spec.kind == TaskKind::Review && spec.depends_on.is_empty() {
        return Err(format!(
            "task '{}': a review task needs at least one dependency (depends_on: the tasks it reviews)",
            spec.id
        ));
    }
    Ok(())
}

/// Verdetto e problemi trovati sono propri delle revisioni: una revisione
/// completata chiude con un verdetto, e `issues` con l'elenco dei problemi.
pub fn check_verdict(kind: TaskKind, completed: bool, f: &ReportFields) -> Result<(), String> {
    match (kind, completed, f.verdict) {
        (TaskKind::Work, _, verdict) if verdict.is_some() || !f.findings.is_empty() => {
            Err("verdict and findings are only accepted for review tasks".to_string())
        }
        (TaskKind::Review, true, None) => {
            Err("verdict is missing: a completed review needs verdict \"passed\" or \"issues\"".to_string())
        }
        (TaskKind::Review, true, Some(Verdict::Issues)) if f.findings.is_empty() => {
            Err("findings is missing: list the problems found (required with verdict \"issues\")".to_string())
        }
        (TaskKind::Review, false, Some(_)) => Err("verdict is only accepted with status \"completed\"".to_string()),
        _ => Ok(()),
    }
}

/// Primi `REVIEWED_LIST_MAX` elementi di una lista, ognuno trasformato da `f`;
/// segna in `cut` se qualcosa e' stato tagliato.
fn bounded<T, U>(items: &[T], cut: &mut bool, f: impl Fn(&T, &mut bool) -> U) -> Vec<U> {
    *cut |= items.len() > REVIEWED_LIST_MAX;
    items.iter().take(REVIEWED_LIST_MAX).map(|i| f(i, cut)).collect()
}

/// Testo limitato a `max` caratteri; segna in `cut` se e' stato tagliato.
fn clipped(s: &str, max: usize, cut: &mut bool) -> String {
    *cut |= s.chars().count() > max;
    clip_with_ellipsis(s, max)
}

fn reviewed_entry(t: &Task) -> Value {
    let mut cut = false;
    let mut o = json!({ "id": t.id, "title": t.title, "status": t.status.as_str() });
    let r = t.report.as_ref();
    o["summary"] = r.map_or(Value::Null, |r| Value::from(clipped(&r.summary, REVIEWED_SUMMARY_MAX, &mut cut)));
    o["commit"] = json!(r.and_then(|r| r.commit.as_deref()));
    o["pr_url"] = json!(r.and_then(|r| r.pr_url.as_deref()));
    o["files_changed"] = json!(r.map_or_else(Vec::new, |r| bounded(&r.files_changed, &mut cut, |f, _| f.clone())));
    o["diffstat"] = r
        .and_then(|r| r.diffstat.as_deref())
        .map_or(Value::Null, |d| Value::from(clipped(d, REVIEWED_DIFFSTAT_MAX, &mut cut)));
    o["tests"] = Value::Array(r.map_or_else(Vec::new, |r| {
        bounded(&r.tests, &mut cut, |t, cut| {
            let mut v = json!({ "command": t.command, "result": t.result });
            if let Some(d) = &t.details {
                v["details"] = Value::from(clipped(d, REVIEWED_ITEM_MAX, cut));
            }
            v
        })
    }));
    o["follow_ups"] = json!(r.map_or_else(Vec::new, |r| bounded(&r.follow_ups, &mut cut, |f, cut| clipped(f, REVIEWED_ITEM_MAX, cut))));
    if cut {
        o["truncated"] = Value::Bool(true);
    }
    o
}

/// Riepilogo (senza il testo della specifica) dei task che `review` verifica,
/// uno per dipendenza diretta nell'ordine di `depends_on`. Dimensioni limitate:
/// con `truncated` il revisore completa con `get_task_details`.
pub fn reviewed_tasks(board: &Board, review: &Task) -> Value {
    Value::Array(review.depends_on.iter().filter_map(|d| board.task(d)).map(reviewed_entry).collect())
}
