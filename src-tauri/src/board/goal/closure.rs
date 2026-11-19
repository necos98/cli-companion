//! Cio' che la board scrive di un goal: la nascita (o il giro successivo)
//! all'accettazione di una proposta, le note dell'utente e la chiusura a mano
//! ("Chiudi goal": `abandoned`). La chiusura da sola, alla consegna o quando non
//! resta nulla, e' in `advance`.

use super::phase::last_review_of;
use super::{DecisionSource, Goal, GoalOutcome, GoalReport, GoalReview, ProposalGoal, DECISION_MAX};
use crate::board::{Board, Task, TaskStatus, Verdict};
use crate::validate::req_text;
use std::collections::HashSet;

impl From<Verdict> for GoalReview {
    fn from(v: Verdict) -> GoalReview {
        match v {
            Verdict::Passed => GoalReview::Passed,
            Verdict::Issues => GoalReview::Issues,
        }
    }
}

impl Board {
    /// I task di una proposta accettata entrano nel goal `goal`: uno nuovo nasce
    /// al primo giro; una correzione (la proposta di un revisore) fa avanzare il
    /// giro del goal; le altre aggiunte entrano nel giro corrente. Ritorna l'id
    /// del goal e il giro dei task.
    pub(in crate::board) fn enter_goal(
        &mut self,
        goal: ProposalGoal,
        correction: bool,
        now: i64,
    ) -> Result<(String, u32), String> {
        match goal {
            ProposalGoal::New(spec) => {
                let goal = Goal::from_spec(spec, now);
                let entry = (goal.id.clone(), goal.round);
                self.goals.push(goal);
                Ok(entry)
            }
            ProposalGoal::Existing(id) => {
                let goal = self.goal_mut(&id).ok_or_else(|| format!("unknown goal: {id}"))?;
                if correction {
                    goal.round += 1;
                }
                Ok((id, goal.round))
            }
        }
    }

    /// Nota dell'utente nel registro delle decisioni di un goal aperto.
    pub fn add_goal_note(&mut self, id: &str, text: &str, now: i64) -> Result<(), String> {
        let text = req_text("text", text, DECISION_MAX)?;
        self.open_goal(id)?;
        if let Some(goal) = self.goal_mut(id) {
            goal.record(DecisionSource::User, text, now);
        }
        self.updated_at = now;
        Ok(())
    }

    /// "Chiudi goal": elimina i task `queued` e `failed` del goal (con i task
    /// non `done` che ne dipendono) e scarta le sue proposte pendenti, poi lo
    /// chiude `abandoned`. Tutto o niente: se uno dei task da togliere e'
    /// `running` non cambia nulla. Ritorna i task tolti (i loro worktree vanno
    /// ripuliti da chi chiama).
    pub fn close_goal(&mut self, id: &str, now: i64) -> Result<Vec<Task>, String> {
        match self.goal(id) {
            None => return Err(format!("unknown goal: {id}")),
            Some(g) if !g.is_open() => return Err(format!("goal {id} is already closed")),
            Some(_) => {}
        }
        let doomed = self.goal_cleanup_of(id)?;
        let (removed, kept): (Vec<Task>, Vec<Task>) =
            std::mem::take(&mut self.tasks).into_iter().partition(|t| doomed.contains(t.id.as_str()));
        self.tasks = kept;
        let removed_ids: Vec<&str> = removed.iter().map(|t| t.id.as_str()).collect();
        self.drop_references(&removed_ids);
        self.proposals.retain(|p| p.goal_id.as_deref() != Some(id));
        self.close(id, GoalOutcome::Abandoned, None, now);
        self.updated_at = now;
        Ok(removed)
    }

    /// Gli id dei task che "Chiudi goal" toglierebbe; errore se uno e' `running`.
    fn goal_cleanup_of(&self, id: &str) -> Result<HashSet<String>, String> {
        let waiters = self.waiters();
        let mut doomed = HashSet::new();
        let mut running = Vec::new();
        for t in self.goal_tasks(id) {
            match t.status() {
                TaskStatus::Done => {}
                TaskStatus::Running => running.push(t.id.clone()),
                TaskStatus::Queued | TaskStatus::Failed => {
                    let cascade = waiters.cascade_of(t);
                    running.extend(cascade.running);
                    doomed.insert(t.id.clone());
                    doomed.extend(cascade.dependents);
                }
            }
        }
        if !running.is_empty() {
            running.sort_unstable();
            running.dedup();
            return Err(format!("cannot close goal '{id}': {} are running, stop them first", running.join(", ")));
        }
        Ok(doomed)
    }

    /// Scrive la chiusura del goal, con l'esito della sua ultima review e, se e'
    /// stato consegnato da un task di consegna, il suo rapporto.
    pub(super) fn close(&mut self, id: &str, outcome: GoalOutcome, report: Option<GoalReport>, now: i64) {
        let tasks: Vec<&Task> = self.goal_tasks(id).collect();
        let review = last_review_of(&tasks);
        if let Some(goal) = self.goal_mut(id) {
            goal.closed_at = Some(now);
            goal.outcome = Some(outcome);
            goal.review = Some(review);
            goal.report = report;
        }
    }
}
