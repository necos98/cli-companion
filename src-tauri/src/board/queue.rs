//! Operazioni sulla coda: task nuovi e proposte (con il loro goal, vedi
//! `goal`), modifiche, cancellazioni e riordino. Ogni operazione verifica lo stato del task e non tocca il suo
//! ciclo di vita (vedi `lifecycle`).

use super::agent::{check_bot_kind, resolve_spec, SpecContext};
use super::{
    check_plan_spec, check_review_deps, parse_spec, permits, valid_id, validate_graph, Board, LifecycleError, Op,
    Proposal, ProposalGoal, ProposalInput, Task, TaskAction, TaskSpec, TaskStatus, MAX_PENDING_PROPOSALS,
    MAX_TASKS_PER_BOARD,
};
use crate::validate::clip_chars;
use serde_json::Value;
use std::collections::HashSet;

/// Campi del patch che obbligano a ricontrollare chi esegue il task: l'agente
/// stesso, un modello (che deve appartenergli) o il Bot.
const AGENT_PATCH_KEYS: [&str; 3] = ["agent", "model", "bot"];
/// Campo del patch che cambia il tipo del task: il suo Bot deve poterlo eseguire.
const KIND_KEY: &str = "kind";

fn patch_has(patch: &Value, key: &str) -> bool {
    patch.as_object().is_some_and(|o| o.contains_key(key))
}

fn touches_agent(patch: &Value) -> bool {
    AGENT_PATCH_KEYS.iter().any(|k| patch_has(patch, k))
}

impl Board {
    // --- task nuovi e proposte ---

    /// Task ancora da concludere (`queued` o `running`): quelli che contano nel limite.
    fn unfinished_count(&self) -> usize {
        self.tasks.iter().filter(|t| matches!(t.status(), TaskStatus::Queued | TaskStatus::Running)).count()
    }

    /// Verifica i task nuovi, destinati al goal `goal` (`None` = sciolti),
    /// contro la board: id validi e unici (anche rispetto a `reserved`, gli id
    /// delle altre proposte pendenti), dipendenze verso la proposta o task
    /// esistenti dello stesso goal, niente cicli, limiti, Bot e agente
    /// utilizzabili. Ritorna le specifiche risolte (agente, modello, effort e
    /// skill del Bot copiati): le proposte tengono le originali, i task nuovi queste.
    pub fn check_specs(
        &self,
        specs: &[TaskSpec],
        reserved: &HashSet<&str>,
        goal: Option<&str>,
        ctx: SpecContext<'_>,
    ) -> Result<Vec<TaskSpec>, String> {
        if specs.is_empty() {
            return Err("tasks is empty".to_string());
        }
        if self.unfinished_count() + specs.len() > MAX_TASKS_PER_BOARD {
            return Err(format!(
                "too many unfinished tasks on the board (max {MAX_TASKS_PER_BOARD} queued or running)"
            ));
        }
        let mut seen: HashSet<&str> = self.tasks.iter().map(|t| t.id.as_str()).collect();
        let mut resolved = Vec::with_capacity(specs.len());
        for s in specs {
            if !valid_id(&s.id) {
                return Err(format!("invalid task id: {}", clip_chars(&s.id, 50)));
            }
            check_review_deps(s)?;
            check_plan_spec(s)?;
            if reserved.contains(s.id.as_str()) {
                return Err(format!("duplicate task id: {} (already used by a pending proposal)", s.id));
            }
            if !seen.insert(s.id.as_str()) {
                return Err(format!("duplicate task id: {}", s.id));
            }
            resolved.push(resolve_spec(s, ctx)?);
        }
        let mut all = self.tasks.clone();
        all.extend(resolved.iter().cloned().map(|s| Task::from_spec(s, 0)));
        validate_graph(&all)?;
        self.check_goal_deps(specs, goal)?;
        Ok(resolved)
    }

    fn pending_ids(&self, except: Option<&str>) -> HashSet<&str> {
        self.proposals
            .iter()
            .filter(|p| Some(p.id.as_str()) != except)
            .flat_map(|p| p.tasks.iter().map(|t| t.id.as_str()))
            .collect()
    }

    /// Id non ancora usati da task o proposte (per i task creati a mano).
    pub fn id_taken(&self, id: &str) -> bool {
        self.task(id).is_some() || self.pending_ids(None).contains(id)
    }

    /// Aggiunge in fondo alla coda le specifiche ritornate da `check_specs`: il
    /// task tiene agente, modello, effort e skill risolti e non cambia piu' al
    /// variare dei predefiniti o del Bot. `goal`: il goal e il giro in cui entrano.
    pub(super) fn push_specs(&mut self, specs: Vec<TaskSpec>, goal: Option<(&str, u32)>, now: i64) -> Vec<String> {
        let ids = specs.iter().map(|s| s.id.clone()).collect();
        self.tasks.extend(specs.into_iter().map(|s| {
            let mut task = Task::from_spec(s, now);
            if let Some((id, round)) = goal {
                task.join_goal(id, round);
            }
            task
        }));
        self.updated_at = now;
        ids
    }

    /// Task sciolto creato a mano, senza proposta.
    pub fn create_task(&mut self, spec: TaskSpec, ctx: SpecContext<'_>, now: i64) -> Result<String, String> {
        self.create_task_in(spec, None, ctx, now)
    }

    /// Task creato a mano, senza proposta, nel goal aperto `goal` (al suo giro
    /// corrente) o sciolto.
    pub fn create_task_in(
        &mut self,
        spec: TaskSpec,
        goal: Option<&str>,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<String, String> {
        let membership = goal.map(|g| self.joinable_goal(g).map(|g| (g.id.clone(), g.round()))).transpose()?;
        let resolved = self.check_specs(std::slice::from_ref(&spec), &self.pending_ids(None), goal, ctx)?;
        Ok(self.push_specs(resolved, membership.as_ref().map(|(id, round)| (id.as_str(), *round)), now).remove(0))
    }

    /// Nuova proposta pendente (dell'Intake o di un revisore). Bot e agente si
    /// verificano ma la proposta tiene le specifiche com'erano: senza agente,
    /// vale quello del workspace al momento dell'accettazione, e il Bot si
    /// copia nel task solo allora. Il goal di `input` e' quello chiesto
    /// dall'Intake; la proposta di un revisore eredita il goal della sua review
    /// (`source_task`).
    pub fn add_proposal(
        &mut self,
        id: String,
        source_tab: &str,
        source_task: Option<String>,
        input: ProposalInput,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        if self.proposals.len() >= MAX_PENDING_PROPOSALS {
            return Err(format!(
                "too many pending proposals (max {MAX_PENDING_PROPOSALS}): wait for the user to accept or discard some"
            ));
        }
        let ProposalInput { tasks: specs, goal } = input;
        let goal = self.proposal_goal(source_task.as_deref(), goal)?;
        if let Some(g) = &goal {
            self.check_goal_target(g, None)?;
        }
        self.check_specs(&specs, &self.pending_ids(None), goal.as_ref().map(ProposalGoal::id), ctx)?;
        let (goal, goal_id) = match goal {
            Some(ProposalGoal::New(spec)) => (Some(spec), None),
            Some(ProposalGoal::Existing(id)) => (None, Some(id)),
            None => (None, None),
        };
        self.proposals.push(Proposal {
            id,
            source_tab: source_tab.to_string(),
            source_task,
            goal,
            goal_id,
            created_at: now,
            tasks: specs,
        });
        self.updated_at = now;
        Ok(())
    }

    pub fn proposal(&self, id: &str) -> Result<&Proposal, String> {
        self.proposals.iter().find(|p| p.id == id).ok_or_else(|| format!("proposal not found: {id}"))
    }

    /// Accetta una proposta: dopo una nuova validazione i task entrano in
    /// coda, nell'ordine della proposta, e nel suo goal (creato adesso se
    /// nuovo; una correzione fa avanzare il giro). Se la validazione fallisce
    /// la proposta resta pendente.
    pub fn accept_proposal(&mut self, id: &str, ctx: SpecContext<'_>, now: i64) -> Result<Vec<String>, String> {
        self.accept_proposal_with(id, None, ctx, now)
    }

    /// Come `accept_proposal`; `auto_deliver` e' la scelta dell'utente per il goal
    /// NUOVO della proposta. `Some(true)` con una proposta che non crea un goal
    /// e' un errore (e la proposta resta pendente).
    pub fn accept_proposal_with(
        &mut self,
        id: &str,
        auto_deliver: Option<bool>,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<Vec<String>, String> {
        let proposal = self.proposal(id)?;
        if auto_deliver == Some(true) && proposal.goal.is_none() {
            return Err("auto_deliver applies only to a proposal that creates a new goal".to_string());
        }
        let (goal, correction) = (proposal.target_goal(), proposal.is_correction());
        let specs = proposal.tasks.clone();
        if let Some(g) = &goal {
            self.check_goal_target(g, Some(id))?;
        }
        let specs = self.check_specs(&specs, &HashSet::new(), goal.as_ref().map(ProposalGoal::id), ctx)?;
        let membership = goal.map(|g| self.enter_goal(g, correction, now)).transpose()?;
        if auto_deliver == Some(true) {
            if let Some(goal) = membership.as_ref().and_then(|(g, _)| self.goal_mut(g)) {
                goal.auto_deliver = true;
            }
        }
        self.proposals.retain(|p| p.id != id);
        Ok(self.push_specs(specs, membership.as_ref().map(|(g, round)| (g.as_str(), *round)), now))
    }

    pub fn discard_proposal(&mut self, id: &str, now: i64) -> Result<(), String> {
        self.proposal(id)?;
        self.proposals.retain(|p| p.id != id);
        self.updated_at = now;
        Ok(())
    }

    // --- modifiche dei task ---

    /// Modifica parziale di un task `queued` o `failed` (`patch` = campi di
    /// `TaskSpec`); `repo` solo senza worktree. Chi esegue il task si
    /// ricontrolla solo se il patch contiene `agent`, `model` o `bot` (`agent:
    /// null` = quello del workspace o il predefinito globale, risolto adesso):
    /// cosi' il titolo di un task il cui agente e' stato poi disabilitato si
    /// modifica comunque. Con un Bot, agente e modello vengono dal Bot salvo
    /// quelli scritti dal patch, e la copia di effort e skill si rifa; `bot:
    /// null` toglie il Bot e la sua copia. Un `kind` nuovo senza quei campi
    /// tiene la copia del Bot ma verifica che il Bot esegua quel tipo di task.
    pub fn update_task(&mut self, id: &str, patch: &Value, ctx: SpecContext<'_>, now: i64) -> Result<(), String> {
        let t = self.require(id, Op::Edit)?;
        let mut spec = parse_spec(patch, Some(&TaskSpec::from_task(t)))?;
        check_review_deps(&spec)?;
        check_plan_spec(&spec)?;
        if patch.as_object().is_some_and(|o| o.keys().all(|k| k == "id")) {
            return Err("no fields to update".to_string());
        }
        if spec.repo != t.repo && t.worktree.is_some() {
            return Err(format!("task '{id}' already has a worktree: its repo cannot be changed"));
        }
        if touches_agent(patch) {
            if spec.bot.is_some() {
                spec.agent = spec.agent.filter(|_| patch_has(patch, "agent"));
                spec.model = spec.model.filter(|_| patch_has(patch, "model"));
            }
            spec = resolve_spec(&spec, ctx)?;
        } else if patch_has(patch, KIND_KEY) {
            check_bot_kind(&spec, ctx.bots)?;
        }
        self.check_goal_deps(std::slice::from_ref(&spec), t.goal())?;
        let mut all = self.tasks.clone();
        if let Some(slot) = all.iter_mut().find(|x| x.id == id) {
            slot.apply_spec(spec, now);
        }
        validate_graph(&all)?;
        self.tasks = all;
        self.updated_at = now;
        Ok(())
    }

    /// Toglie un task `queued` o `failed` che nessun task non `done` aspetta.
    /// Ritorna il task rimosso (il suo worktree va ripulito da chi chiama).
    pub fn delete_task(&mut self, id: &str, now: i64) -> Result<Task, String> {
        self.require(id, Op::Action(TaskAction::Delete))?;
        let waiting: Vec<&str> = self
            .tasks
            .iter()
            .filter(|x| x.status() != TaskStatus::Done && x.depends_on.iter().any(|d| d == id))
            .map(|x| x.id.as_str())
            .collect();
        if !waiting.is_empty() {
            return Err(format!(
                "task '{id}' cannot be deleted: {} depend on it. Remove it from their depends_on (update_task) or delete them first",
                waiting.join(", ")
            ));
        }
        let removed = self.take_task(id)?;
        self.drop_references(&[removed.id.as_str()]);
        self.updated_at = now;
        Ok(removed)
    }

    /// Toglie un task `queued` o `failed` insieme a tutti i task non `done` che
    /// ne dipendono, direttamente o no. Tutto o niente: se uno dei task da
    /// togliere e' `running` non cambia nulla. Ritorna i task rimossi (i loro
    /// worktree vanno ripuliti da chi chiama).
    pub fn delete_with_dependents(&mut self, id: &str, now: i64) -> Result<Vec<Task>, String> {
        let t = self.require(id, Op::Action(TaskAction::DeleteCascade))?;
        let cascade = self.waiters().cascade_of(t);
        if !cascade.running.is_empty() {
            return Err(format!("cannot delete: {} are running, stop them first", cascade.running.join(", ")));
        }
        let doomed: HashSet<&str> = std::iter::once(id).chain(cascade.dependents.iter().map(String::as_str)).collect();
        let (removed, kept): (Vec<Task>, Vec<Task>) =
            std::mem::take(&mut self.tasks).into_iter().partition(|x| doomed.contains(x.id.as_str()));
        self.tasks = kept;
        let removed_ids: Vec<&str> = removed.iter().map(|x| x.id.as_str()).collect();
        self.drop_references(&removed_ids);
        self.updated_at = now;
        Ok(removed)
    }

    /// Toglie il task `id` dalla coda.
    fn take_task(&mut self, id: &str) -> Result<Task, LifecycleError> {
        let pos = self.tasks.iter().position(|x| x.id == id).ok_or_else(|| LifecycleError::NotFound(id.to_string()))?;
        Ok(self.tasks.remove(pos))
    }

    /// I task rimasti (anche `done`) non devono tenere un riferimento a un task sparito.
    pub(super) fn drop_references(&mut self, removed: &[&str]) {
        for x in &mut self.tasks {
            x.depends_on.retain(|d| !removed.contains(&d.as_str()));
        }
    }

    /// Sposta un `queued` prima di `before` (un altro `queued`) o in fondo.
    pub fn move_task(&mut self, id: &str, before: Option<&str>, now: i64) -> Result<(), String> {
        self.require(id, Op::Reorder)?;
        if let Some(b) = before {
            if b != id && !permits(self.known(b)?.status(), Op::Reorder) {
                return Err(format!("task '{b}' is not queued: a task can only be placed before a queued task"));
            }
        }
        if before == Some(id) {
            return Ok(());
        }
        let t = self.take_task(id)?;
        let at = before.and_then(|b| self.tasks.iter().position(|x| x.id == b)).unwrap_or(self.tasks.len());
        self.tasks.insert(at, t);
        self.updated_at = now;
        Ok(())
    }

    /// Porta un `queued` in testa alla coda.
    pub fn task_to_top(&mut self, id: &str, now: i64) -> Result<(), String> {
        self.require(id, Op::Action(TaskAction::ToTop))?;
        let t = self.take_task(id)?;
        let at = self.tasks.iter().position(|x| x.status() == TaskStatus::Queued).unwrap_or(self.tasks.len());
        self.tasks.insert(at, t);
        self.updated_at = now;
        Ok(())
    }

    pub fn check_stop(&self, id: &str) -> Result<(), String> {
        self.require(id, Op::Action(TaskAction::Stop))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
