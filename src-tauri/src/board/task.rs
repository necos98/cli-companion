//! Accessi al task: costruzione dalla specifica e letture dello stato. Le
//! scritture dello stato sono tutte in `lifecycle`.

use super::{
    FailReason, PreviousAttempt, Run, RunPhase, Task, TaskQuestion, TaskReport, TaskSpec, TaskState, TaskStatus,
    WorkerActivity, WorkerStatus,
};
use crate::agents::registry;

impl Task {
    /// Nuovo task. L'agente e' quello di `s.agent`, gia' risolto da `resolve_spec_agent`
    /// per le operazioni della board; una specifica senza agente (uso diretto,
    /// nei test) prende il predefinito globale.
    pub fn from_spec(s: TaskSpec, now: i64) -> Task {
        let agent = s.agent.unwrap_or_else(|| registry::default_agent().id().to_string());
        Task {
            id: s.id,
            title: s.title,
            task: s.task,
            depends_on: s.depends_on,
            repo: s.repo,
            agent,
            model: s.model,
            bot: s.bot,
            effort: s.effort,
            skill: s.skill,
            kind: s.kind,
            e2e: s.e2e,
            created_at: now,
            updated_at: now,
            ..Task::default()
        }
    }

    /// Applica i campi modificati; `s.agent` `None` lascia l'agente del task.
    /// Bot, effort e skill sono la copia gia' risolta di `s`.
    pub(super) fn apply_spec(&mut self, s: TaskSpec, now: i64) {
        self.title = s.title;
        self.task = s.task;
        self.depends_on = s.depends_on;
        self.repo = s.repo;
        if let Some(agent) = s.agent {
            self.agent = agent;
        }
        self.model = s.model;
        self.bot = s.bot;
        self.effort = s.effort;
        self.skill = s.skill;
        self.kind = s.kind;
        self.e2e = s.e2e;
        self.updated_at = now;
    }

    /// Il task entra nel goal `goal` al giro `round` (solo all'ingresso in coda).
    pub(super) fn join_goal(&mut self, goal: &str, round: u32) {
        self.goal = Some(goal.to_string());
        self.round = round;
    }

    /// Goal del task (`None` = task sciolto).
    pub fn goal(&self) -> Option<&str> {
        self.goal.as_deref()
    }

    /// Il task lavora nel suo worktree, senza occupare il repository: un `plan`
    /// (sola lettura) o un task di un goal in una cartella git (docs/GOALS.MD,
    /// "Branch del goal"). Un task di un goal `no_git` lavora sul posto.
    pub fn runs_in_own_worktree(&self) -> bool {
        self.kind.isolated() || (self.goal.is_some() && !self.no_git)
    }

    /// Giro del goal in cui il task e' entrato in coda; 0 senza goal.
    pub fn round(&self) -> u32 {
        self.round
    }

    pub fn state(&self) -> &TaskState {
        &self.state
    }

    pub fn status(&self) -> TaskStatus {
        self.state.status()
    }

    /// Tentativi avviati finora.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Nota dell'app sull'ultimo evento del task (la leggono anche i modelli).
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Riassunto breve dell'ultimo report.
    pub fn last_report(&self) -> Option<&str> {
        self.last_report.as_deref()
    }

    fn run(&self) -> Option<&Run> {
        match &self.state {
            TaskState::Running(run) => Some(run),
            _ => None,
        }
    }

    /// Tab del worker, mentre il task e' in corso con un worker spawnato.
    pub fn session(&self) -> Option<&str> {
        match self.run().map(|r| &r.phase) {
            Some(RunPhase::Live { session, .. }) => Some(session),
            _ => None,
        }
    }

    /// Stato del worker come lo vede il frontend: `starting` anche durante la
    /// preparazione (nessun worker ancora), `None` fuori da `running`.
    pub fn worker_status(&self) -> Option<WorkerStatus> {
        match self.run()?.phase {
            RunPhase::Preparing { .. } => Some(WorkerStatus::Starting),
            RunPhase::Live { worker, .. } => Some(worker.into()),
        }
    }

    /// Il task e' in corso col worker `session`.
    pub fn is_live_for(&self, session: &str) -> bool {
        self.live_activity(session).is_some()
    }

    /// Attivita' del worker `session`, se e' quello del task in corso.
    pub fn live_activity(&self, session: &str) -> Option<WorkerActivity> {
        match self.run().map(|r| &r.phase) {
            Some(RunPhase::Live { session: s, worker }) if s == session => Some(*worker),
            _ => None,
        }
    }

    /// Passo di preparazione e ultimo segno di vita (ms) di un avvio in corso
    /// (task `running` senza worker); `None` altrimenti.
    pub fn preparing(&self) -> Option<(Option<&str>, i64)> {
        match self.run().map(|r| &r.phase) {
            Some(RunPhase::Preparing { step, heartbeat }) => Some((step.as_deref(), *heartbeat)),
            _ => None,
        }
    }

    /// Inizio dell'ultimo tentativo.
    pub fn started_at(&self) -> Option<i64> {
        match &self.state {
            TaskState::Queued { .. } => None,
            TaskState::Running(run) => Some(run.started_at),
            TaskState::Done(f) | TaskState::Failed { finished: f, .. } => f.started_at,
        }
    }

    pub fn finished_at(&self) -> Option<i64> {
        match &self.state {
            TaskState::Done(f) | TaskState::Failed { finished: f, .. } => Some(f.finished_at),
            TaskState::Queued { .. } | TaskState::Running(_) => None,
        }
    }

    /// Report del tentativo concluso.
    pub fn report(&self) -> Option<&TaskReport> {
        match &self.state {
            TaskState::Done(f) | TaskState::Failed { finished: f, .. } => f.report.as_ref(),
            TaskState::Queued { .. } | TaskState::Running(_) => None,
        }
    }

    /// Perche' il task e' fallito (`None` se non e' `failed` o il motivo non e' noto).
    pub fn fail_reason(&self) -> Option<FailReason> {
        match &self.state {
            TaskState::Failed { reason, .. } => *reason,
            _ => None,
        }
    }

    /// Esito del tentativo precedente, finche' il task e' in coda o in corso.
    pub fn previous_attempt(&self) -> Option<&PreviousAttempt> {
        match &self.state {
            TaskState::Queued { previous_attempt } => previous_attempt.as_ref(),
            TaskState::Running(run) => run.previous_attempt.as_ref(),
            TaskState::Done(_) | TaskState::Failed { .. } => None,
        }
    }

    /// Il worker aspetta l'utente.
    pub fn needs_attention(&self) -> bool {
        self.run().is_some_and(|r| matches!(r.phase, RunPhase::Live { worker, .. } if worker.needs_attention()))
    }

    /// Le domande del worker all'utente, dalla piu' vecchia.
    pub fn questions(&self) -> &[TaskQuestion] {
        &self.questions
    }

    /// La domanda in attesa di risposta: l'ultima, se non ha risposta, il task e' in
    /// corso e l'ha fatta il tentativo attuale (le domande di un tentativo finito o
    /// precedente non aspettano piu' nessuno).
    pub fn pending_question(&self) -> Option<&TaskQuestion> {
        self.questions
            .last()
            .filter(|q| q.answered_at.is_none() && q.attempt == self.attempts && self.status() == TaskStatus::Running)
    }

    /// Dopo un riavvio dell'app nessuna esecuzione E2E e' piu' in corso
    /// (nemmeno la copia nel report).
    pub(super) fn interrupt_e2e(&mut self) {
        for run in self.e2e_runs_mut() {
            run.interrupt_if_running();
        }
    }
}

/// Costruzione dei dati di prova: i test impostano lo stato direttamente, senza
/// passare dalle transizioni.
#[cfg(test)]
impl Task {
    pub(crate) fn set_state(&mut self, state: TaskState) {
        self.state = state;
    }

    pub(crate) fn set_status(&mut self, status: TaskStatus) {
        self.state = TaskState::of(status);
    }

    pub(crate) fn set_attempts(&mut self, attempts: u32) {
        self.attempts = attempts;
    }

    pub(crate) fn set_note(&mut self, note: Option<&str>) {
        self.note = note.map(str::to_string);
    }

    pub(crate) fn set_last_report(&mut self, last_report: Option<&str>) {
        self.last_report = last_report.map(str::to_string);
    }
}
