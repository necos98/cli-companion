//! Follow-up di un goal (docs/GOALS.MD, "Follow-up"): alla chiusura la board
//! copia nel goal i `follow_ups` dei report dei suoi task e i `findings` delle
//! review con verdetto `passed` (i suggerimenti), perche' i task verranno
//! archiviati. Nessuna voce diventa un task da sola e nessun tool ne cambia lo
//! stato: lo fa solo l'utente, dall'interfaccia (`Board::set_follow_up_state`).
//! Funzioni pure.

use super::Goal;
use crate::board::report::LIST_ITEM_MAX;
use crate::board::{Board, Task, TaskKind, Verdict};
use crate::validate::clip_chars;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

#[cfg(test)]
mod tests;

/// Voci massime per goal: le eccedenti si scartano, con una traccia nel log.
pub const MAX_FOLLOW_UPS: usize = 100;

/// Cosa ne ha fatto l'utente.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FollowUpState {
    /// Ancora da decidere.
    #[default]
    Open,
    /// Scartata dall'utente.
    Dismissed,
    /// Mandata all'Intake come nuova richiesta.
    Promoted,
}

impl FollowUpState {
    fn as_str(self) -> &'static str {
        match self {
            FollowUpState::Open => "open",
            FollowUpState::Dismissed => "dismissed",
            FollowUpState::Promoted => "promoted",
        }
    }
}

/// Un follow-up raccolto alla chiusura del goal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct FollowUp {
    pub text: String,
    /// Il task il cui report l'ha scritto.
    pub source_task: String,
    pub state: FollowUpState,
}

/// Il testo senza differenze di spaziatura e maiuscole: la chiave dei doppioni.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// I testi che un task lascia come follow-up: i suoi `follow_ups` e, se e' una
/// review con verdetto `passed`, i `findings`.
fn texts_of(task: &Task) -> Vec<&str> {
    let Some(report) = task.report() else { return Vec::new() };
    let suggestions = task.kind == TaskKind::Review && report.verdict == Some(Verdict::Passed);
    let findings: &[String] = if suggestions { &report.findings } else { &[] };
    report.follow_ups.iter().chain(findings).map(String::as_str).collect()
}

/// Le voci raccolte dai task (in ordine) e quante ne ha scartate il tetto.
struct Collected {
    items: Vec<FollowUp>,
    dropped: usize,
}

/// Raccoglie i follow-up dei task: senza vuoti ne' doppioni (testo normalizzato),
/// al massimo `MAX_FOLLOW_UPS`.
fn collect<'a>(tasks: impl Iterator<Item = &'a Task>) -> Collected {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut dropped = 0;
    for task in tasks {
        for text in texts_of(task) {
            let key = normalized(text);
            if key.is_empty() || !seen.insert(key) {
                continue;
            }
            if items.len() < MAX_FOLLOW_UPS {
                let text = clip_chars(text.trim(), LIST_ITEM_MAX);
                items.push(FollowUp { text, source_task: task.id.clone(), state: FollowUpState::Open });
            } else {
                dropped += 1;
            }
        }
    }
    Collected { items, dropped }
}

impl Goal {
    pub fn follow_ups(&self) -> &[FollowUp] {
        &self.follow_ups
    }

    /// Invarianti dopo la lettura: i follow-up esistono solo su un goal chiuso,
    /// senza voci vuote e nei limiti (il file non e' fidato).
    pub(super) fn sanitize_follow_ups(&mut self) {
        if self.closed_at.is_none() {
            self.follow_ups.clear();
            return;
        }
        self.follow_ups.retain(|f| !f.text.trim().is_empty());
        self.follow_ups.truncate(MAX_FOLLOW_UPS);
        for f in &mut self.follow_ups {
            f.text = clip_chars(&f.text, LIST_ITEM_MAX);
        }
    }
}

impl Board {
    /// I follow-up dei task del goal `id`, per `close`. Le voci oltre il tetto
    /// si scartano: lasciano una traccia nel log.
    pub(super) fn collect_follow_ups(&self, id: &str) -> Vec<FollowUp> {
        let Collected { items, dropped } = collect(self.goal_tasks(id));
        if dropped > 0 {
            log::warn!(
                "board {}, goal {id}: {dropped} follow-ups over the limit of {MAX_FOLLOW_UPS} were discarded",
                self.workspace_id
            );
        }
        items
    }

    /// Scarta (`Dismissed`) o manda all'Intake (`Promoted`) il follow-up `index`
    /// di un goal chiuso. Solo l'utente, mai un tool; una voce gia' decisa non
    /// cambia piu'.
    pub fn set_follow_up_state(
        &mut self,
        goal_id: &str,
        index: usize,
        state: FollowUpState,
        now: i64,
    ) -> Result<(), String> {
        if state == FollowUpState::Open {
            return Err("a follow-up can only be dismissed or promoted".to_string());
        }
        let goal = self.goal_mut(goal_id).ok_or_else(|| format!("unknown goal: {goal_id}"))?;
        let entry = goal.follow_ups.get_mut(index).ok_or_else(|| format!("unknown follow-up: {index}"))?;
        if entry.state != FollowUpState::Open {
            return Err(format!("follow-up {index} is already {}", entry.state.as_str()));
        }
        entry.state = state;
        self.updated_at = now;
        Ok(())
    }
}
