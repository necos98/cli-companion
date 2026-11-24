//! Modello dati della board: task, proposte e costanti di dominio.
//! Solo tipi e accessi in lettura; le operazioni stanno in `queue`,
//! `lifecycle` e `scheduler`.

use super::record::TaskRecord;
use super::{E2eRun, SettingsProposal, TaskKind, TaskQuestion, TaskSpec, TaskState};
use crate::agents::Effort;
use crate::bots::SkillRef;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Lunghezza massima del titolo di un task (in caratteri).
pub const TITLE_MAX: usize = 80;
/// Lunghezza massima del testo di un task (in caratteri).
pub const TASK_MAX: usize = 100_000;
/// Lunghezza massima del riassunto di un report (in caratteri).
pub const SUMMARY_MAX: usize = 50_000;
/// Lunghezza massima delle note scritte dall'app sul task (in caratteri).
pub const NOTE_MAX: usize = 2000;

/// Tetto ai task non conclusi (`queued` e `running`) di una board: `done` e
/// `failed` restano come storico e non contano.
pub const MAX_TASKS_PER_BOARD: usize = 200;
pub const MAX_PENDING_PROPOSALS: usize = 20;
pub const MIN_PARALLEL_LIMIT: u8 = 1;
pub const MAX_PARALLEL_LIMIT: u8 = 16;
pub const DEFAULT_MAX_PARALLEL: u8 = 3;

/// Note dell'app sui task (le leggono anche i modelli, via `previous_attempt`).
pub const INTERRUPTED_NOTE: &str = "interrupted: the app was restarted";
pub const ARCHIVED_NOTE: &str = "interrupted: the workspace was archived";
pub const STOPPED_NOTE: &str = "stopped by the user";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TaskStatus {
    #[default]
    Queued,
    Running,
    Done,
    Failed,
}

impl TaskStatus {
    pub const ALL: [TaskStatus; 4] = [TaskStatus::Queued, TaskStatus::Running, TaskStatus::Done, TaskStatus::Failed];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<TaskStatus> {
        TaskStatus::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
    /// Riferimento da cui il branch e' stato creato (`origin/<base>` o `<base>`).
    pub base: String,
    /// Setup del repository (file copiati, comandi) completato. `false` solo
    /// per un worktree creato dall'app il cui setup non e' finito: un retry lo
    /// rifa. I file vecchi, senza il campo, valgono come fatti.
    #[serde(default = "yes")]
    pub setup_done: bool,
}

fn yes() -> bool {
    true
}

/// Esito del tentativo precedente, mostrato al worker dopo un "Riprova".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreviousAttempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

/// Un task della board. Lo stato del ciclo di vita e i campi che ne dipendono
/// (`attempts`, `note`, `last_report`, `questions`) si leggono con gli accessori e li scrive
/// solo `lifecycle` (`Task::apply`). Sul filo (file e frontend) il task e' un
/// record piatto: vedi `record`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(from = "TaskRecord")]
pub struct Task {
    pub id: String,
    pub title: String,
    pub task: String,
    pub depends_on: Vec<String>,
    /// Repository (cartella relativa alla root del workspace); `None` = la root.
    pub repo: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    /// Il Bot che esegue il task, per l'interfaccia. Agente, modello, effort e
    /// skill sono la copia presa alla creazione: modificare o eliminare il Bot
    /// dopo non cambia il task. `None` = task senza Bot (o precedente ai Bot).
    pub bot: Option<String>,
    /// Effort della copia del Bot (grezzo: se l'agente non lo supporta lo toglie l'avvio).
    pub effort: Option<Effort>,
    /// Skill della copia del Bot, da caricare all'avvio del worker.
    pub skill: Option<SkillRef>,
    /// `work` consegna; `review` verifica il risultato integrato dei task da cui dipende.
    pub kind: TaskKind,
    /// Il task deve superare gli E2E del repository (richiede `RepoConfig::e2e`).
    pub e2e: bool,
    /// La cartella del task non e' un repository git (modalita' libera: si
    /// lavora sul posto, senza worktree, branch ne' consegna git). Si aggiorna
    /// alla creazione e all'accettazione e, in modo autorevole, all'avvio.
    pub no_git: bool,
    pub(in crate::board) state: TaskState,
    pub(in crate::board) attempts: u32,
    pub(in crate::board) last_report: Option<String>,
    pub(in crate::board) note: Option<String>,
    pub worktree: Option<WorktreeInfo>,
    /// Ultima esecuzione degli E2E del tentativo in corso (o dell'ultimo).
    pub e2e_run: Option<E2eRun>,
    /// Domande del worker all'utente, dalla piu' vecchia (le scrive solo `lifecycle`).
    pub(in crate::board) questions: Vec<TaskQuestion>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Proposta pendente dell'Intake: l'utente la accetta o la scarta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Proposal {
    pub id: String,
    /// Tab che l'ha proposta (Intake o revisore).
    pub source_tab: String,
    /// Task di revisione il cui revisore l'ha proposta (assente: viene dall'Intake).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_task: Option<String>,
    pub created_at: i64,
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub workspace_id: String,
    pub max_parallel: u8,
    pub paused: bool,
    pub updated_at: i64,
    /// L'ordine dell'array e' l'ordine della coda.
    pub tasks: Vec<Task>,
    pub proposals: Vec<Proposal>,
    /// Proposte pendenti di modifica delle impostazioni del workspace.
    pub settings_proposals: Vec<SettingsProposal>,
    /// Revisione in memoria (ordina i salvataggi/eventi concorrenti).
    #[serde(skip)]
    pub rev: u64,
}

impl Default for Board {
    fn default() -> Self {
        Board {
            workspace_id: String::new(),
            max_parallel: DEFAULT_MAX_PARALLEL,
            paused: false,
            updated_at: 0,
            tasks: Vec::new(),
            proposals: Vec::new(),
            settings_proposals: Vec::new(),
            rev: 0,
        }
    }
}

impl Board {
    pub fn new(workspace_id: &str) -> Board {
        Board { workspace_id: workspace_id.to_string(), ..Board::default() }
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    /// Tutte le dipendenze esistono e sono `done`.
    pub fn deps_done(&self, t: &Task) -> bool {
        t.depends_on.iter().all(|d| self.task(d).is_some_and(|x| x.status() == TaskStatus::Done))
    }

    pub fn running_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status() == TaskStatus::Running).count()
    }
}
