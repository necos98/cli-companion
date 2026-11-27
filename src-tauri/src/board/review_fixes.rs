//! Correzioni proposte da un revisore: i task entrano subito in coda (senza
//! proposta pendente), davanti al primo task `queued`, e i task in coda che
//! aspettano il lavoro revisionato (o che il revisore indica) passano ad
//! aspettare le correzioni. Logica pura: la validazione dei task e' quella di
//! `check_specs`, il grafo si ricontrolla prima di toccare la board.
//!
//! Un task gia' `running` non si puo' bloccare: ha il suo worker, e le
//! dipendenze si leggono solo all'avvio. Per questo si riscrivono solo i
//! `queued`; `running`, `done` e `failed` restano come sono.

use super::agent::SpecContext;
use super::spec::parse_proposal_allowing;
use super::{validate_graph, Board, Task, TaskKind, TaskSpec, TaskStatus, MAX_DEPENDS, MAX_TASKS_PER_BOARD};
use crate::validate::clip_chars;
use serde_json::Value;
use std::collections::HashSet;

/// Campo di `propose_tasks` del revisore: task in coda da far aspettare.
const BLOCKS_KEY: &str = "blocks";

/// Esito di `Board::queue_review_fixes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFixes {
    /// Id dei task messi in coda, nell'ordine della proposta.
    pub tasks: Vec<String>,
    /// Task gia' in coda che ora aspettano la nuova revisione (o le correzioni), in ordine di coda.
    pub waiting: Vec<String>,
}

/// Parametri di `propose_tasks` del revisore: `{ tasks, blocks? }`. A differenza
/// delle sessioni, accetta `blocks`: gli id dei task in coda da far aspettare.
pub fn parse_review_proposal(p: &Value) -> Result<(Vec<TaskSpec>, Vec<String>), String> {
    let specs = parse_proposal_allowing(p, &[BLOCKS_KEY])?;
    let blocks = match p.get(BLOCKS_KEY) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => {
            if a.len() > MAX_TASKS_PER_BOARD {
                return Err(format!("blocks has too many entries (max {MAX_TASKS_PER_BOARD})"));
            }
            a.iter()
                .map(|v| {
                    v.as_str().map(str::to_string).ok_or_else(|| {
                        format!("blocks must be an array of task ids, got {}", clip_chars(&v.to_string(), 50))
                    })
                })
                .collect::<Result<_, _>>()?
        }
        Some(_) => return Err("blocks must be an array of task ids".to_string()),
    };
    Ok((specs, blocks))
}

/// Id che devono finire prima dei task in attesa: le nuove revisioni, o tutti i
/// task proposti se non ce ne sono.
fn gate_of(specs: &[TaskSpec]) -> Vec<String> {
    let reviews: Vec<String> = specs.iter().filter(|s| s.kind == TaskKind::Review).map(|s| s.id.clone()).collect();
    if reviews.is_empty() {
        specs.iter().map(|s| s.id.clone()).collect()
    } else {
        reviews
    }
}

impl Board {
    /// Controlla `blocks`: task esistenti, `queued`, fuori dalla proposta e diversi dalla revisione.
    fn check_blocks(&self, review_id: &str, specs: &[TaskSpec], blocks: &[String]) -> Result<(), String> {
        for id in blocks {
            if id == review_id {
                return Err(format!("blocks cannot contain the review itself ('{id}')"));
            }
            if specs.iter().any(|s| s.id == *id) {
                return Err(format!("blocks cannot contain a task of this proposal ('{id}')"));
            }
            let t = self.task(id).ok_or_else(|| format!("blocks: unknown task '{id}'"))?;
            if t.status() != TaskStatus::Queued {
                return Err(format!(
                    "blocks: task '{id}' is {}, only queued tasks can be made to wait",
                    t.status().as_str()
                ));
            }
        }
        Ok(())
    }

    /// Id dei task `queued` che dipendono, anche indirettamente, dalla revisione
    /// `review_id` o dai task che revisiona.
    fn queued_dependents_of_review(&self, review_id: &str) -> HashSet<String> {
        let waiters = self.waiters();
        let reviewed = self.task(review_id).map(|r| r.depends_on.as_slice()).unwrap_or_default();
        std::iter::once(review_id)
            .chain(reviewed.iter().map(String::as_str))
            .flat_map(|id| waiters.dependents_of(id))
            .filter(|t| t.status() == TaskStatus::Queued)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Mette in coda le correzioni del revisore di `review_id`, tutto o niente.
    ///
    /// I task entrano prima del primo `queued` della board (in fondo se non ce
    /// ne sono). Poi ogni `queued` fuori dalla proposta che dipende dalla
    /// revisione o dai task revisionati, o che e' in `blocks`, riceve in
    /// `depends_on` i task "cancello" (le nuove revisioni, o tutta la proposta
    /// se non ne contiene): parte solo a correzioni e verifica finite.
    pub fn queue_review_fixes(
        &mut self,
        review_id: &str,
        specs: Vec<TaskSpec>,
        blocks: &[String],
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<ReviewFixes, String> {
        if self.task(review_id).is_none() {
            return Err(format!("review task not found: {review_id}"));
        }
        self.check_blocks(review_id, &specs, blocks)?;
        let resolved = self.check_specs(&specs, &self.pending_ids(None), ctx)?;
        let gate = gate_of(&resolved);
        let mut targets = self.queued_dependents_of_review(review_id);
        targets.extend(blocks.iter().cloned());
        let at = self.tasks.iter().position(|t| t.status() == TaskStatus::Queued).unwrap_or(self.tasks.len());

        let mut all = self.tasks.clone();
        let mut waiting = Vec::new();
        for t in all.iter_mut().filter(|t| targets.contains(&t.id)) {
            for g in &gate {
                if !t.depends_on.contains(g) {
                    t.depends_on.push(g.clone());
                }
            }
            if t.depends_on.len() > MAX_DEPENDS {
                return Err(format!("task '{}' would depend on too many tasks (max {MAX_DEPENDS})", t.id));
            }
            t.updated_at = now;
            waiting.push(t.id.clone());
        }
        let tasks: Vec<String> = resolved.iter().map(|s| s.id.clone()).collect();
        let fixes =
            resolved.into_iter().map(|s| Task { origin: Some(review_id.to_string()), ..Task::from_spec(s, now) });
        all.splice(at..at, fixes);
        validate_graph(&all).map_err(|e| {
            format!("queueing the fixes would create a dependency cycle ({e}): a fix cannot depend on a task that has to wait for the fixes")
        })?;
        self.tasks = all;
        self.updated_at = now;
        Ok(ReviewFixes { tasks, waiting })
    }
}

#[cfg(test)]
mod tests;
