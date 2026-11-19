//! Consegna del goal, lato dati (docs/GOALS.MD, "Consegna del goal"): le
//! impostazioni che cambia solo l'utente (`auto_deliver`, il Bot di consegna),
//! il rapporto che resta nel goal consegnato e il task di consegna (`task`).
//! Funzioni pure, nessun I/O.

use super::Goal;
use crate::board::agent::resolve_spec;
use crate::board::report::DELIVERED_MAX;
use crate::board::{Board, SpecContext, TaskKind, TaskReport, TaskSpec, BOT_ID_MAX, SUMMARY_MAX};
use crate::validate::clip_chars;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod task;
#[cfg(test)]
mod tests;

/// Il Bot che esegue la consegna se il goal non ne sceglie un altro.
pub const DEFAULT_DELIVER_BOT: &str = "general";

/// Cosa ha consegnato il task di consegna, copiato nel goal alla chiusura.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct GoalReport {
    /// Cosa e' stato consegnato, per l'utente.
    pub summary: String,
    /// Commit finali, uno per repository.
    pub commits: Vec<String>,
    /// Link delle PR (modalita' `pr`), una per repository.
    pub pr_urls: Vec<String>,
}

impl GoalReport {
    /// Il rapporto dal report del task di consegna: `commit` e `commits`, `pr_url`
    /// e `pr_urls` si uniscono senza doppioni.
    pub(super) fn of_delivery(r: &TaskReport) -> GoalReport {
        let merged = |one: Option<&String>, many: &[String]| {
            let mut out: Vec<String> = Vec::new();
            for s in one.into_iter().chain(many) {
                if !out.contains(s) {
                    out.push(s.clone());
                }
            }
            out
        };
        let mut report = GoalReport {
            summary: r.summary.clone(),
            commits: merged(r.commit.as_ref(), &r.commits),
            pr_urls: merged(r.pr_url.as_ref(), &r.pr_urls),
        };
        report.clamp();
        report
    }

    /// Dentro i limiti: il contenuto del file non e' fidato.
    fn clamp(&mut self) {
        self.summary = clip_chars(&self.summary, SUMMARY_MAX);
        self.commits.truncate(DELIVERED_MAX);
        self.pr_urls.truncate(DELIVERED_MAX);
    }
}

impl Goal {
    pub fn auto_deliver(&self) -> bool {
        self.auto_deliver
    }

    /// L'id del Bot che esegue la consegna.
    pub fn deliver_bot(&self) -> &str {
        self.deliver_bot.as_deref().unwrap_or(DEFAULT_DELIVER_BOT)
    }

    /// Invarianti dopo la lettura: il Bot ha la forma di un id e il rapporto c'e'
    /// solo su un goal consegnato (e dentro i limiti).
    pub(super) fn sanitize_delivery(&mut self) {
        self.deliver_bot = self.deliver_bot.take().filter(|b| !b.trim().is_empty() && b.chars().count() <= BOT_ID_MAX);
        if self.outcome != Some(super::GoalOutcome::Delivered) {
            self.report = None;
        }
        if let Some(report) = &mut self.report {
            report.clamp();
        }
    }
}

impl Board {
    /// Cambia le impostazioni di consegna di un goal aperto: `auto_deliver` e/o il
    /// Bot (deve esistere, essere acceso e poter eseguire task `work`, come lo
    /// verifica `resolve_spec`). Le chiama solo l'utente.
    pub fn set_goal_delivery(
        &mut self,
        goal_id: &str,
        auto_deliver: Option<bool>,
        bot: Option<String>,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        if auto_deliver.is_none() && bot.is_none() {
            return Err("nothing to change: give auto_deliver or bot".to_string());
        }
        self.open_goal(goal_id)?;
        let bot = bot.map(|b| b.trim().to_string());
        if let Some(bot) = &bot {
            let probe = TaskSpec {
                id: self.deliver_task_id(goal_id),
                kind: TaskKind::Deliver,
                bot: Some(bot.clone()),
                ..TaskSpec::default()
            };
            resolve_spec(&probe, ctx)?;
        }
        if let Some(goal) = self.goal_mut(goal_id) {
            goal.auto_deliver = auto_deliver.unwrap_or(goal.auto_deliver);
            goal.deliver_bot = bot.or_else(|| goal.deliver_bot.take());
        }
        self.updated_at = now;
        Ok(())
    }
}
