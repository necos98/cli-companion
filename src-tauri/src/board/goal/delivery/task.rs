//! Il task di consegna di un goal (`kind: "deliver"`): lo crea l'app, uno per
//! goal, in cima alla coda, quando il goal e' pronto. Qui la costruzione pura;
//! i Bot li legge chi chiama (`SpecContext`).

use crate::board::agent::resolve_spec;
use crate::board::goal::{Goal, GoalPhase};
use crate::board::{id_from_title, repo_key, Board, SpecContext, Task, TaskKind, TaskSpec, TaskStatus, TITLE_MAX};
use crate::validate::clip_chars;

impl Board {
    /// Id del task di consegna del goal: `deliver-<goal-id>` se libero, con un
    /// suffisso numerico altrimenti.
    pub(super) fn deliver_task_id(&self, goal_id: &str) -> String {
        id_from_title(&format!("deliver-{goal_id}"), &|id| self.id_taken(id))
    }

    /// Crea il task di consegna del goal aperto `goal_id` e lo mette in cima alla
    /// coda. Il goal deve essere `ready`, avere branch e non avere ancora un task
    /// di consegna (un task fallito si rilancia). Il Bot e' quello del goal e
    /// deve poter eseguire task `work`. Ritorna l'id del task.
    pub fn create_deliver_task(&mut self, goal_id: &str, ctx: SpecContext<'_>, now: i64) -> Result<String, String> {
        let goal = self.open_goal(goal_id)?;
        if self.goal_tasks(goal_id).any(|t| t.kind == TaskKind::Deliver) {
            return Err(format!("goal {goal_id} already has a delivery task: retry it"));
        }
        let phase = self.goal_view(goal_id).map(|v| v.phase);
        if let Some(phase) = phase.filter(|p| *p != GoalPhase::Ready) {
            return Err(format!("goal {goal_id} is not ready to deliver: it is {}", phase.as_str()));
        }
        if goal.branches.is_empty() {
            return Err(format!("goal {goal_id} has no branches: there is nothing to deliver"));
        }
        let round = goal.round();
        let (repo, e2e) = self.deliver_repo(goal);
        let spec = TaskSpec {
            id: self.deliver_task_id(goal_id),
            title: clip_chars(&format!("Consegna: {}", goal.title), TITLE_MAX),
            task: format!(
                "Deliver the goal \"{}\" to its base as a whole, following the instructions and the delivery steps.",
                goal.title
            ),
            repo,
            bot: Some(goal.deliver_bot().to_string()),
            kind: TaskKind::Deliver,
            e2e,
            ..TaskSpec::default()
        };
        let spec = resolve_spec(&spec, ctx)?;
        let id = spec.id.clone();
        let mut task = Task::from_spec(spec, now);
        task.join_goal(goal_id, round);
        let top = self.tasks.iter().position(|t| t.status() == TaskStatus::Queued).unwrap_or(self.tasks.len());
        self.tasks.insert(top, task);
        self.updated_at = now;
        Ok(id)
    }

    /// Il repository principale della consegna (`None` = la root) e se richiede
    /// gli E2E. E' quello di un task `work` del goal con `e2e`, altrimenti la
    /// root, altrimenti il primo dei branch; gli E2E si richiedono se un task
    /// `work` con `e2e` e' in quel repository.
    fn deliver_repo(&self, goal: &Goal) -> (Option<String>, bool) {
        let e2e_keys: Vec<String> = self
            .goal_tasks(&goal.id)
            .filter(|t| t.kind == TaskKind::Work && t.e2e)
            .map(|t| repo_key(t.repo.as_deref()))
            .collect();
        let root = repo_key(None);
        let has = |key: &str| goal.branches.contains_key(key);
        let main = e2e_keys
            .iter()
            .find(|k| has(k))
            .cloned()
            .or_else(|| has(&root).then(|| root.clone()))
            .or_else(|| goal.branches.keys().next().cloned())
            .unwrap_or_else(|| root.clone());
        let e2e = e2e_keys.contains(&main);
        ((main != root).then_some(main), e2e)
    }
}
