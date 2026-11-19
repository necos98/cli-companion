//! Appartenenza dei task ai goal (docs/GOALS.MD, "Modello dati"): a quale goal
//! vanno i task nuovi, quale goal eredita la proposta di un revisore e da chi
//! puo' dipendere un task. Un task sta al piu' in un goal; un task di un goal
//! dipende solo da task dello stesso goal; un task sciolto non dipende da task
//! di un goal aperto; un goal chiuso non riceve task.

use super::{Goal, ProposalGoal, MAX_OPEN_GOALS};
use crate::board::{Board, Task, TaskKind, TaskSpec, TaskStatus};
use std::collections::HashSet;

/// Errore di una proposta di un worker che indica da se' un goal.
pub(in crate::board) const GOAL_SET_BY_APP: &str =
    "goal and goal_id cannot be set by an automated session: the proposal of a reviewer inherits the goal of its review";
/// Errore di un task `plan` in un goal.
pub(in crate::board) const PLAN_WITHOUT_GOAL: &str = "a plan task cannot belong to a goal";

impl Board {
    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|g| g.id == id)
    }

    pub(in crate::board) fn goal_mut(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.iter_mut().find(|g| g.id == id)
    }

    /// I task del goal `id`, in ordine di coda.
    pub fn goal_tasks<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Task> + 'a {
        self.tasks.iter().filter(move |t| t.goal() == Some(id))
    }

    /// Il task appartiene a un goal ancora aperto.
    pub(in crate::board) fn in_open_goal(&self, t: &Task) -> bool {
        t.goal().and_then(|g| self.goal(g)).is_some_and(Goal::is_open)
    }

    /// Il goal aperto `id`, a cui si aggiungono task.
    pub(in crate::board) fn open_goal(&self, id: &str) -> Result<&Goal, String> {
        match self.goal(id) {
            None => Err(format!("unknown goal: {id}")),
            Some(g) if !g.is_open() => Err(format!("goal {id} is closed: propose a new goal")),
            Some(g) => Ok(g),
        }
    }

    /// Il goal ha un task di consegna non `done`.
    pub(in crate::board) fn is_delivering(&self, id: &str) -> bool {
        self.goal_tasks(id).any(|t| t.kind == TaskKind::Deliver && t.status() != TaskStatus::Done)
    }

    /// Il goal aperto `id` che accetta task nuovi: uno in consegna non ne accetta.
    pub(in crate::board) fn joinable_goal(&self, id: &str) -> Result<&Goal, String> {
        let goal = self.open_goal(id)?;
        if self.is_delivering(id) {
            return Err(format!("goal {id} is being delivered: new tasks cannot join it"));
        }
        Ok(goal)
    }

    /// Un goal nuovo: id libero fra i goal della board e i goal nuovi delle
    /// altre proposte pendenti (`except` = la proposta stessa), e posto fra i
    /// goal aperti.
    fn check_new_goal(&self, id: &str, except: Option<&str>) -> Result<(), String> {
        let proposed = self
            .proposals
            .iter()
            .filter(|p| Some(p.id.as_str()) != except)
            .filter_map(|p| p.goal.as_ref())
            .any(|g| g.id == id);
        if proposed || self.goal(id).is_some() {
            return Err(format!("goal id already in use: {id}"));
        }
        if self.goals.iter().filter(|g| g.is_open()).count() >= MAX_OPEN_GOALS {
            return Err(format!(
                "too many open goals on the board (max {MAX_OPEN_GOALS}): add the tasks to an open goal with goal_id"
            ));
        }
        Ok(())
    }

    /// Il goal di destinazione di una proposta (`except` = la proposta stessa,
    /// quando si accetta).
    pub(in crate::board) fn check_goal_target(&self, goal: &ProposalGoal, except: Option<&str>) -> Result<(), String> {
        match goal {
            ProposalGoal::New(spec) => self.check_new_goal(&spec.id, except),
            ProposalGoal::Existing(id) => self.joinable_goal(id).map(|_| ()),
        }
    }

    /// Il goal di una proposta nuova. Quella di un worker (`source_task`) non
    /// lo indica: la proposta del revisore eredita il goal della sua review,
    /// le altre restano senza goal.
    pub(in crate::board) fn proposal_goal(
        &self,
        source_task: Option<&str>,
        requested: Option<ProposalGoal>,
    ) -> Result<Option<ProposalGoal>, String> {
        let Some(source) = source_task else {
            return Ok(requested);
        };
        if requested.is_some() {
            return Err(GOAL_SET_BY_APP.to_string());
        }
        let inherited = self
            .task(source)
            .filter(|t| t.kind == TaskKind::Review)
            .and_then(Task::goal)
            .map(|g| ProposalGoal::Existing(g.to_string()));
        Ok(inherited)
    }

    /// Le dipendenze dei task `specs` (nuovi, o uno modificato) destinati al
    /// goal `goal` (`None` = sciolti). Le dipendenze interne a `specs` sono nello
    /// stesso goal per costruzione; quelle sconosciute le rifiuta `validate_graph`.
    pub(in crate::board) fn check_goal_deps(&self, specs: &[TaskSpec], goal: Option<&str>) -> Result<(), String> {
        let own: HashSet<&str> = specs.iter().map(|s| s.id.as_str()).collect();
        for s in specs {
            if goal.is_some() && s.kind == TaskKind::Plan {
                return Err(format!("task '{}': {PLAN_WITHOUT_GOAL}", s.id));
            }
            let external = s.depends_on.iter().filter(|d| !own.contains(d.as_str()));
            for dep in external.filter_map(|d| self.task(d)) {
                match (goal, dep.goal()) {
                    (Some(g), Some(of_dep)) if g == of_dep => {}
                    (Some(_), Some(_)) => {
                        return Err(format!("task '{}': depends on '{}', which belongs to another goal", s.id, dep.id))
                    }
                    (Some(g), None) => {
                        return Err(format!(
                            "task '{}': depends on '{}', which is not part of goal '{g}'",
                            s.id, dep.id
                        ))
                    }
                    (None, Some(of_dep)) if self.in_open_goal(dep) => {
                        return Err(format!(
                            "task '{}': depends on '{}', which belongs to the open goal '{of_dep}'",
                            s.id, dep.id
                        ))
                    }
                    (None, _) => {}
                }
            }
        }
        Ok(())
    }
}
