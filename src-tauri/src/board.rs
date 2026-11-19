//! Board dei task (v0.12): parte PURA. Ogni workspace ha una coda di task con
//! dipendenze e una lista di proposte pendenti dell'Intake; l'app la esegue con
//! uno scheduler deterministico. Qui ci sono solo tipi, migrazione dei file
//! vecchi, validazione, operazioni sulla coda e scheduler: niente I/O, niente
//! `AppHandle`, tempo iniettato. L'integrazione con worker, git e disco e' nel
//! modulo `automation`. Il contratto condiviso col frontend e' `docs/BOARD.MD`.
//!
//! Struttura: `model` (task, board e costanti), `state` (stato del ciclo di
//! vita con i suoi dati), `lifecycle` (le transizioni e la tabella delle azioni:
//! l'unico posto che scrive lo stato), `record` (formato sul filo del task),
//! `task` (accessi), `question` (domande del worker all'utente), `agent` (agente di un task), `queue` (specifiche, proposte,
//! modifiche e riordino),
//! `cascade` (dipendenze inverse), `scheduler`, `view` (vista per frontend e
//! MCP), `worker` (stati del worker e tabella degli hook), `persist` (busta e
//! schema del file, riga dell'archivio), `legacy` (migrazioni dello schema),
//! `recovery` (recupero dopo un riavvio), `archive` (task conclusi da archiviare),
//! `review`, `plan` e `deliver` (regole dei task di revisione, pianificazione e consegna),
//! `goal` (obiettivi: appartenenza dei task, fase, chiusura e archivio),
//! `relocation` (task che impediscono di cambiare la cartella del workspace).

mod agent;
mod agent_gate;
mod archive;
mod cascade;
mod deliver;
mod e2e;
mod goal;
mod handoff;
mod legacy;
mod legacy_goals;
mod legacy_settings;
mod lifecycle;
mod model;
mod persist;
mod plan;
mod question;
mod queue;
mod record;
mod recovery;
mod relocation;
mod report;
mod review;
mod scheduler;
mod settings;
mod spec;
mod state;
mod task;
pub mod texts;
mod view;
mod worker;

#[cfg(test)]
mod tests;

pub use agent::SpecContext;
pub use agent_gate::{AgentGate, AgentUnavailableView};
pub use archive::MAX_FINISHED_KEPT;
pub use cascade::CascadeDelete;
pub use deliver::{check_deliver_report, goal_task_entries, DELIVER_KIND_RESERVED};
pub use e2e::{
    check_gate, strip_ansi, CodeFingerprint, E2eFailure, E2eOutcome, E2eRun, E2eStatus, E2eTotals, GateOutcome,
    E2E_ERROR_MAX, E2E_GATE_SKIPPED_NOTE, E2E_OUTPUT_TAIL_MAX,
};
pub use goal::{
    auto_fix_decision, goal_branch_name, parse_goal_id, parse_goal_spec, Acceptance, AutoFix, AutoFixDecision,
    AutoFixHold, DecisionSource, DeliveredGoal, FollowUp, FollowUpState, Goal, GoalAdvance, GoalBranch, GoalDecision,
    GoalOutcome, GoalPhase, GoalProgress, GoalReport, GoalReview, GoalSpec, GoalView, GoalWaiting, ProposalGoal,
    DECISIONS_KEPT, DECISION_MAX, DEFAULT_DELIVER_BOT, DONE_WHEN_ITEM_MAX, DONE_WHEN_MAX, GOAL_BRANCH_PREFIX,
    GOAL_REQUEST_MAX, GOAL_TITLE_MAX, MAX_AUTO_FIX_ROUNDS, MAX_CLOSED_GOALS_KEPT, MAX_FOLLOW_UPS, MAX_OPEN_GOALS,
};
pub use handoff::Handoff;
pub use lifecycle::{permits, LifecycleError, Op, Outcome, TaskAction, TaskEvent};
pub use model::{
    Board, PreviousAttempt, Proposal, Task, TaskStatus, WorktreeInfo, ARCHIVED_NOTE, DEFAULT_MAX_PARALLEL,
    INTERRUPTED_NOTE, MAX_PARALLEL_LIMIT, MAX_PENDING_PROPOSALS, MAX_TASKS_PER_BOARD, MIN_PARALLEL_LIMIT, NOTE_MAX,
    STOPPED_NOTE, SUMMARY_MAX, TASK_MAX, TITLE_MAX,
};
pub use persist::{archive_goal_line, archive_line, board_store};
pub use plan::{check_kind_input, check_plan_report, check_plan_spec, PLAN_KIND_RESERVED};
pub use question::{parse_question, TaskQuestion, ANSWER_MAX, OPTIONS_MAX, OPTION_MAX, QUESTIONS_KEPT, QUESTION_MAX};
pub use relocation::{relocation_blockers, relocation_refusal};
pub use report::{build_report, parse_report_fields, ReportFields, TaskReport, TestRun, DELIVERED_MAX, DIFFSTAT_MAX};
pub use review::{check_review_deps, check_verdict, reviewed_tasks, TaskKind, Verdict};
pub use scheduler::{schedule, SchedCtx};
pub use settings::{SettingsProposal, SettingsProposalView};
pub use spec::{
    id_from_title, parse_max_parallel, parse_new_task, parse_proposal, parse_spec, repo_key, valid_id, validate_graph,
    NewTaskSpec, ProposalInput, TaskPatch, TaskSpec, BOT_ID_MAX, ID_MAX, MAX_DEPENDS,
};
pub use state::{FailCause, FailReason, Failure, Finished, Run, RunPhase, TaskState};
pub use view::{compact_row, BoardLimits, BoardView, Bounds, TaskView};
pub use worker::{WorkerActivity, WorkerEnd, WorkerSignal, WorkerStatus, WorkerStep};
