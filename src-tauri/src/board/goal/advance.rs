//! Avanzamento dei goal al publish (docs/GOALS.MD, "Fase e attesa" e
//! "Consegna del goal"): la board chiude da sola i goal finiti e dice a chi
//! chiama quali consegne partono da sole. Le chiusure sono le sole che non
//! sono dell'utente: task di consegna `done` (`delivered`, con il rapporto),
//! goal rimasto senza task (`abandoned`) e goal pronto senza branch
//! (`delivered`, senza rapporto: nulla da consegnare). Funzioni pure.

use super::delivery::GoalReport;
use super::phase::delivers_by_itself;
use super::{GoalOutcome, GoalPhase, GoalView};
use crate::board::{Board, TaskKind, TaskStatus};

/// Un goal chiuso `delivered` dal suo task di consegna.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredGoal {
    pub goal_id: String,
    pub title: String,
    /// Il task di consegna `done`.
    pub deliver_task_id: String,
}

/// Cio' che `Board::advance_goals` chiede a chi chiama: notificare le consegne
/// e creare il task dei goal che si consegnano da soli.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GoalAdvance {
    /// Goal appena chiusi dal loro task di consegna.
    pub delivered: Vec<DeliveredGoal>,
    /// Goal pronti con `auto_deliver`: il task di consegna lo crea chi chiama
    /// (`Board::create_deliver_task`), perche' servono i Bot, letti fuori dal lock.
    pub auto_deliver: Vec<String>,
}

/// Cosa fare di un goal aperto.
enum Step {
    /// Chiuso `delivered` dal task di consegna `task_id`, con il suo rapporto.
    Delivered { task_id: String, report: Option<GoalReport> },
    /// Chiuso senza rapporto.
    Close(GoalOutcome),
    /// La consegna parte da sola.
    AutoDeliver,
}

impl Board {
    /// Chiude i goal aperti che sono finiti e ritorna le consegne da annunciare
    /// e quelle da avviare. Per ogni goal aperto, il primo caso che vale:
    /// 1. il task di consegna e' `done`: `delivered`, con il suo rapporto;
    /// 2. nessun task e nessuna proposta pendente: `abandoned`;
    /// 3. `ready` e senza branch (cartella senza git): `delivered`, senza rapporto;
    /// 4. `ready` e la consegna parte da sola (`auto_deliver`, ultima review non
    ///    `issues`): il goal e' nell'elenco `auto_deliver`, senza cambiare nulla.
    pub fn advance_goals(&mut self, now: i64) -> GoalAdvance {
        let steps: Vec<(String, String, Step)> = self
            .goal_views()
            .iter()
            .filter(|v| v.goal.is_open())
            .filter_map(|v| Some((v.goal.id.clone(), v.goal.title.clone(), self.step_of(v)?)))
            .collect();
        let mut out = GoalAdvance::default();
        for (id, title, step) in steps {
            match step {
                Step::Delivered { task_id, report } => {
                    self.close(&id, GoalOutcome::Delivered, report, now);
                    self.updated_at = now;
                    out.delivered.push(DeliveredGoal { goal_id: id, title, deliver_task_id: task_id });
                }
                Step::Close(outcome) => {
                    self.close(&id, outcome, None, now);
                    self.updated_at = now;
                }
                Step::AutoDeliver => out.auto_deliver.push(id),
            }
        }
        out
    }

    /// Il passo del goal aperto `v`, se ce n'e' uno.
    fn step_of(&self, v: &GoalView) -> Option<Step> {
        let id = v.goal.id.as_str();
        let delivered = self
            .goal_tasks(id)
            .filter(|t| t.kind == TaskKind::Deliver && t.status() == TaskStatus::Done)
            .max_by_key(|t| t.finished_at());
        if let Some(t) = delivered {
            return Some(Step::Delivered { task_id: t.id.clone(), report: t.report().map(GoalReport::of_delivery) });
        }
        if v.phase != GoalPhase::Ready {
            return None;
        }
        if v.tasks.total == 0 {
            Some(Step::Close(GoalOutcome::Abandoned))
        } else if v.goal.branches.is_empty() {
            Some(Step::Close(GoalOutcome::Delivered))
        } else if delivers_by_itself(&v.goal, v.last_review) {
            Some(Step::AutoDeliver)
        } else {
            None
        }
    }
}
