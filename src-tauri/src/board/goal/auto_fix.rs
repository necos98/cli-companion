//! Correzione automatica con tetto di giri (docs/GOALS.MD, "Giri e correzione
//! automatica"): l'impostazione del goal, che cambia solo l'utente, la decisione
//! su una proposta di correzione (funzione pura: l'effetto lo esegue
//! l'automazione con lo stesso percorso dell'accettazione manuale) e cio' che
//! un'accettazione scrive nel goal. Nessun I/O.

use super::{DecisionSource, Goal};
use crate::board::{repo_key, Board, Proposal, TaskKind};
use crate::bots::bot_off_by_default;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

#[cfg(test)]
mod tests;

/// Tetto dei giri di correzione automatica di un goal.
pub const MAX_AUTO_FIX_ROUNDS: u8 = 3;

/// Errore di un tetto fuori dai limiti.
fn rounds_out_of_range() -> String {
    format!("auto_fix max_rounds must be between 0 and {MAX_AUTO_FIX_ROUNDS}")
}

/// Correzione automatica di un goal. La cambia solo l'utente, mai un tool.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct AutoFix {
    /// Giri di correzione che l'app accetta da sola (0..=3); 0 = ogni proposta
    /// la decide l'utente (default).
    pub max_rounds: u8,
}

/// Perche' una proposta di correzione non si accetta da sola, pur con i giri
/// automatici a disposizione: resta pendente e la card lo mostra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "reason", rename_all = "snake_case")]
#[ts(export)]
pub enum AutoFixHold {
    /// Non viene dal revisore di una review del goal (la review non c'e' piu').
    NotFromReview,
    /// Un task non e' `work` ne' `review`.
    Kind { task: String, kind: TaskKind },
    /// Un task lavora in un repository che il goal non usa (chiave `repo_key`).
    Repo { task: String, repo: String },
    /// Un task usa un Bot spento di serie (anche se l'utente l'ha acceso).
    Bot { task: String, bot: String },
    /// La validazione dell'accettazione automatica e' fallita.
    Invalid { error: String },
}

/// Cosa fa l'app di una proposta pendente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoFixDecision {
    /// La decide l'utente, come sempre: non e' una correzione, o il goal non ha
    /// correzione automatica.
    Manual,
    /// Correzione, ma i giri automatici del goal sono finiti.
    RoundLimit,
    /// Correzione non ammissibile all'accettazione automatica.
    Hold(AutoFixHold),
    /// L'app la accetta da sola.
    Accept,
}

/// La decisione su una proposta pendente di `board`: si accetta da sola se e'
/// una correzione (proposta del revisore di una review di un goal aperto), il
/// goal ha ancora giri automatici (`round - 1 < max_rounds`) ed e' ammissibile.
pub fn auto_fix_decision(board: &Board, proposal: &Proposal) -> AutoFixDecision {
    let goal = proposal.goal_id.as_deref().filter(|_| proposal.is_correction()).and_then(|g| board.goal(g));
    let Some(goal) = goal.filter(|g| g.is_open() && g.auto_fix.max_rounds > 0) else {
        return AutoFixDecision::Manual;
    };
    if goal.round.saturating_sub(1) >= u32::from(goal.auto_fix.max_rounds) {
        return AutoFixDecision::RoundLimit;
    }
    match hold_of(board, goal, proposal) {
        Some(hold) => AutoFixDecision::Hold(hold),
        None => AutoFixDecision::Accept,
    }
}

/// Il primo criterio di ammissibilita' che la proposta non rispetta: viene dal
/// revisore di una review del goal; solo task `work` e `review`, nei repository
/// gia' usati dal goal, senza Bot spenti di serie; l'ultima accettazione
/// automatica tentata non e' fallita.
fn hold_of(board: &Board, goal: &Goal, proposal: &Proposal) -> Option<AutoFixHold> {
    let from_review = proposal
        .source_task
        .as_deref()
        .and_then(|s| board.task(s))
        .is_some_and(|t| t.kind == TaskKind::Review && t.goal() == Some(goal.id.as_str()));
    if !from_review {
        return Some(AutoFixHold::NotFromReview);
    }
    let repos: HashSet<String> = board.goal_tasks(&goal.id).map(|t| repo_key(t.repo.as_deref())).collect();
    for spec in &proposal.tasks {
        let task = spec.id.clone();
        if !matches!(spec.kind, TaskKind::Work | TaskKind::Review) {
            return Some(AutoFixHold::Kind { task, kind: spec.kind });
        }
        let repo = repo_key(spec.repo.as_deref());
        if !repos.contains(&repo) {
            return Some(AutoFixHold::Repo { task, repo });
        }
        if let Some(bot) = spec.bot.as_deref().filter(|b| bot_off_by_default(b)) {
            return Some(AutoFixHold::Bot { task, bot: bot.to_string() });
        }
    }
    proposal.auto_fix_hold.clone().filter(|h| matches!(h, AutoFixHold::Invalid { .. }))
}

/// Come si accetta una proposta: le scelte dell'utente per il goal nuovo che
/// crea, o l'accettazione automatica di una correzione da parte dell'app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Acceptance {
    /// Consegna automatica del goal nuovo.
    pub auto_deliver: Option<bool>,
    /// Giri di correzione automatica del goal nuovo.
    pub auto_fix: Option<u8>,
    /// L'accetta l'app (correzione automatica), non l'utente.
    pub automatic: bool,
}

impl Acceptance {
    /// L'accettazione automatica di una correzione.
    pub const AUTOMATIC: Acceptance = Acceptance { auto_deliver: None, auto_fix: None, automatic: true };
}

impl Goal {
    pub fn auto_fix(&self) -> AutoFix {
        self.auto_fix
    }

    /// I giri accettati in automatico, in ordine.
    pub fn auto_rounds(&self) -> &[u32] {
        &self.auto_rounds
    }

    /// Invarianti dopo la lettura: tetto nei limiti; giri automatici unici,
    /// ordinati e possibili (dal secondo al `MAX_AUTO_FIX_ROUNDS + 1`-esimo).
    pub(super) fn sanitize_auto_fix(&mut self) {
        self.auto_fix.max_rounds = self.auto_fix.max_rounds.min(MAX_AUTO_FIX_ROUNDS);
        let last = u32::from(MAX_AUTO_FIX_ROUNDS) + 1;
        self.auto_rounds.retain(|r| (2..=last).contains(r));
        self.auto_rounds.sort_unstable();
        self.auto_rounds.dedup();
    }
}

impl Board {
    /// Giri di correzione automatica di un goal aperto (0..=3). Li cambia solo
    /// l'utente. Alzarli con una correzione ferma al tetto la fa partire.
    pub fn set_goal_auto_fix(&mut self, goal_id: &str, max_rounds: u8, now: i64) -> Result<(), String> {
        if max_rounds > MAX_AUTO_FIX_ROUNDS {
            return Err(rounds_out_of_range());
        }
        self.open_goal(goal_id)?;
        if let Some(goal) = self.goal_mut(goal_id) {
            goal.auto_fix.max_rounds = max_rounds;
        }
        self.updated_at = now;
        Ok(())
    }

    /// Le scelte di `acceptance` valgono per la proposta pendente `proposal`?
    /// Quelle dell'utente solo per una proposta che crea un goal nuovo;
    /// l'accettazione automatica solo per una correzione che l'app accetterebbe
    /// adesso (l'utente puo' averla decisa, o aver cambiato il goal, nel frattempo).
    pub(in crate::board) fn check_acceptance(&self, proposal: &Proposal, acceptance: Acceptance) -> Result<(), String> {
        let new_goal = proposal.goal.is_some();
        if acceptance.auto_deliver == Some(true) && !new_goal {
            return Err("auto_deliver applies only to a proposal that creates a new goal".to_string());
        }
        match acceptance.auto_fix {
            Some(n) if n > MAX_AUTO_FIX_ROUNDS => return Err(rounds_out_of_range()),
            Some(n) if n > 0 && !new_goal => {
                return Err("auto_fix applies only to a proposal that creates a new goal".to_string())
            }
            _ => {}
        }
        if acceptance.automatic && auto_fix_decision(self, proposal) != AutoFixDecision::Accept {
            return Err(format!("proposal {} is not accepted automatically", proposal.id));
        }
        Ok(())
    }

    /// Cio' che un'accettazione scrive nel goal `goal_id`, in cui i `tasks` task
    /// sono entrati al giro `round`: le scelte dell'utente per il goal nuovo o,
    /// se l'ha accettata l'app, il giro automatico e la sua riga nel registro.
    pub(in crate::board) fn apply_acceptance(
        &mut self,
        goal_id: &str,
        round: u32,
        tasks: usize,
        acceptance: Acceptance,
        now: i64,
    ) {
        let Some(goal) = self.goal_mut(goal_id) else {
            return;
        };
        if acceptance.auto_deliver == Some(true) {
            goal.auto_deliver = true;
        }
        // Una scelta attiva vale solo per un goal nuovo (`check_acceptance`), che
        // nasce con il default: "no" non cambia nulla.
        if let Some(n) = acceptance.auto_fix.filter(|n| *n > 0) {
            goal.auto_fix.max_rounds = n;
        }
        if acceptance.automatic {
            goal.auto_rounds.push(round);
            let noun = if tasks == 1 { "task" } else { "tasks" };
            goal.record(DecisionSource::App, format!("Round {round} accepted automatically: {tasks} {noun}"), now);
        }
    }

    /// Al publish: aggiorna il motivo di blocco di ogni proposta pendente e
    /// ritorna quelle che l'app accetta da sola.
    pub(super) fn advance_auto_fix(&mut self) -> Vec<String> {
        let decisions: Vec<AutoFixDecision> = self.proposals.iter().map(|p| auto_fix_decision(self, p)).collect();
        let mut accept = Vec::new();
        for (p, decision) in self.proposals.iter_mut().zip(decisions) {
            p.auto_fix_hold = match decision {
                AutoFixDecision::Hold(hold) => Some(hold),
                AutoFixDecision::Accept => {
                    accept.push(p.id.clone());
                    None
                }
                AutoFixDecision::Manual | AutoFixDecision::RoundLimit => None,
            };
        }
        accept
    }

    /// L'accettazione automatica della proposta `id` e' fallita con `error`: la
    /// proposta resta pendente con il motivo e l'app non ci riprova. Niente se
    /// non e' piu' pendente o non e' piu' da accettare in automatico.
    pub fn hold_auto_fix(&mut self, id: &str, error: &str, now: i64) -> bool {
        let Some(pos) = self.proposals.iter().position(|p| p.id == id) else {
            return false;
        };
        if auto_fix_decision(self, &self.proposals[pos]) != AutoFixDecision::Accept {
            return false;
        }
        self.proposals[pos].auto_fix_hold = Some(AutoFixHold::Invalid { error: error.to_string() });
        self.updated_at = now;
        true
    }
}
