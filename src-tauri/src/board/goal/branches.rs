//! Branch del goal (docs/GOALS.MD, "Branch del goal"): per ogni repository in
//! cui lavora, un goal ha il branch `goal/<id>` e la base da cui e' nato. Qui il
//! modello e le operazioni pure della board; git lo esegue l'automazione, fuori
//! dal lock. La chiusura del goal non tocca i branch: li toglie solo
//! "Elimina branch", su un goal abbandonato.

use super::{Goal, GoalOutcome};
use crate::board::{repo_key, Board};
use crate::validate::{normalize_repo, valid_branch};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Prefisso dei branch dei goal: `goal/<id>`.
pub const GOAL_BRANCH_PREFIX: &str = "goal/";

/// Branch `goal/<id>` del goal `goal_id`.
pub fn goal_branch_name(goal_id: &str) -> String {
    format!("{GOAL_BRANCH_PREFIX}{goal_id}")
}

/// Il branch di un goal in un repository: nome e base (nome semplice, senza
/// `origin/`) risolta alla creazione, che da allora non cambia.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GoalBranch {
    pub branch: String,
    pub base: String,
}

impl Goal {
    /// Toglie i branch che l'app non puo' aver scritto: il contenuto del file non
    /// e' fidato. Il nome del branch si ricostruisce dall'id e la chiave e' un
    /// repository relativo alla root, in forma canonica (`repo_key`).
    pub(super) fn sanitize_branches(&mut self) {
        let name = goal_branch_name(&self.id);
        let canonical = |key: &str| normalize_repo(key).is_ok_and(|r| repo_key(r.as_deref()) == key);
        self.branches.retain(|key, b| b.branch == name && valid_branch(&b.base) && canonical(key));
    }
}

impl Board {
    /// Il branch del goal nel repository `repo_key`, se e' gia' stato creato.
    pub fn goal_branch(&self, goal_id: &str, repo_key: &str) -> Option<&GoalBranch> {
        self.goal(goal_id)?.branches.get(repo_key)
    }

    /// Registra il branch creato per il repository `repo_key`, solo se manca:
    /// ritorna la voce che resta (quella di chi e' arrivato prima, se due avvii
    /// dello stesso goal hanno creato il branch insieme). `None` se il goal non
    /// esiste piu'.
    pub fn record_goal_branch(
        &mut self,
        goal_id: &str,
        repo_key: &str,
        branch: GoalBranch,
        now: i64,
    ) -> Option<GoalBranch> {
        let goal = self.goal_mut(goal_id)?;
        let stored = match goal.branches.get(repo_key) {
            Some(existing) => existing.clone(),
            None => {
                goal.branches.insert(repo_key.to_string(), branch.clone());
                self.updated_at = now;
                branch
            }
        };
        Some(stored)
    }

    /// I branch (chiave del repository, branch) che "Elimina branch" toglie:
    /// solo di un goal abbandonato. Uno consegnato li tiene.
    pub fn abandoned_goal_branches(&self, goal_id: &str) -> Result<Vec<(String, GoalBranch)>, String> {
        let goal = self.goal(goal_id).ok_or_else(|| format!("unknown goal: {goal_id}"))?;
        match goal.outcome() {
            None => return Err(format!("goal {goal_id} is open: close it first")),
            Some(GoalOutcome::Delivered) => return Err(format!("goal {goal_id} was delivered: its branches are kept")),
            Some(GoalOutcome::Abandoned) => {}
        }
        if goal.branches.is_empty() {
            return Err(format!("goal {goal_id} has no branches"));
        }
        Ok(goal.branches.iter().map(|(k, b)| (k.clone(), b.clone())).collect())
    }

    /// Dimentica i branch dei repository `keys`, cancellati da chi chiama.
    pub fn forget_goal_branches(&mut self, goal_id: &str, keys: &[String], now: i64) {
        let Some(goal) = self.goal_mut(goal_id) else {
            return;
        };
        goal.branches.retain(|k, _| !keys.contains(k));
        self.updated_at = now;
    }
}

#[cfg(test)]
mod tests;
