//! Worker (tab PTY automatici): tipi condivisi con il frontend e macchina a
//! stati guidata dagli hook del coding agent e dalla vita del tab.

use super::*;
use ts_rs::TS;

/// Prompt passato in riga di comando: ASCII semplice, sicuro in ogni shell
/// (niente virgolette, `$`, backtick, `%`, `!`, `\`). Lo legge il modello:
/// in inglese.
pub const BOOTSTRAP_PROMPT: &str =
    "You are an automated cli-companion session. Call the get_task tool right away and follow its instructions.";

/// Motivo del blocco dello Stop hook: il worker ha chiuso il turno senza
/// riferire (lo legge il modello).
pub const STOP_BLOCK_REASON: &str = "You ended your turn without calling complete_task. If the task is done, deliver it and call complete_task. If you are waiting for the user's decision, end your turn again: the user will answer here.";

/// Stato di un worker per il frontend (evento `automation-update` e comando
/// `automation_sessions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct AutomationTabInfo {
    pub tab_id: String,
    pub status: WorkerStatus,
    pub title: String,
    /// Sempre vuoto: i worker non hanno piu' un genitore.
    pub parent_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_report: Option<String>,
}

/// Payload di `automation-spawn`: il frontend apre il tab e digita `command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct SpawnEvent {
    pub tab_id: String,
    pub workspace_id: String,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cwd: Option<String>,
    pub title: String,
    /// Sempre vuoto (compatibilita' col frontend).
    pub parent_tab_id: String,
    pub agent_id: String,
}

/// Payload degli eventi che nominano solo un tab: `automation-close` (chiudi il
/// tab worker) e `notification-activated` (click su un toast).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TabEvent {
    pub tab_id: String,
}

/// Dati di un worker da registrare (gia' validati e con il comando pronto).
#[derive(Debug, Clone)]
pub struct NewWorker {
    pub workspace_id: String,
    pub agent_id: String,
    pub title: String,
    pub cwd: String,
    /// Comando da digitare nella shell del nuovo tab.
    pub command: String,
    /// Variabili d'ambiente che il backend imposta sul processo del tab prima
    /// che la shell parta (mai sul filo verso il frontend).
    pub env: Vec<(String, String)>,
    /// Tipo del task: decide ruolo e strumenti del worker.
    pub kind: TaskKind,
}

/// Dove e come lavora un worker (calcolato all'avvio, fuori dal lock): serve a
/// generare le istruzioni di `get_task`.
#[derive(Debug, Clone)]
pub(crate) struct WorkerEnv {
    /// Consegna del workspace quando il task e' partito.
    pub delivery: Delivery,
    pub repo_dir: String,
    pub workdir: String,
    pub branch: Option<String>,
    /// Branch base (nome semplice), nelle modalita' con branch.
    pub base: Option<String>,
    /// `goal/<id>` per un task di un goal in una cartella git (il goal si consegna
    /// per intero piu' tardi: il worker integra qui, non sul base).
    pub goal_branch: Option<String>,
    /// Pianificatore sul posto in una cartella senza git (vedi `planner::root`);
    /// si toglie quando parte il controllo di sola lettura.
    pub root_plan: Option<planner::root::RootPlan>,
    /// Modalita' libera: la cartella del task non e' un repository git, si
    /// lavora sul posto senza worktree, branch ne' consegna git.
    pub free: bool,
}

pub(super) struct Worker {
    pub(super) session: String,
    pub(super) workspace_id: String,
    pub(super) task_id: String,
    /// Tipo del task del worker (un `review` e' un revisore).
    pub(super) kind: TaskKind,
    /// Tentativo (`attempts` del task) per cui il worker e' partito.
    pub(super) attempt: u32,
    pub(super) title: String,
    pub(super) env: WorkerEnv,
    /// Come e' finito, se e' finito. Finche' e' `None` lo stato e' quello del suo
    /// task (vedi `Registry::worker_status`).
    pub(super) ended: Option<WorkerEnd>,
    /// Sessione dell'agente legata al worker (vedi `agent_session`).
    pub(super) agent: agent_session::AgentSession,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
    pub(super) last_report: Option<String>,
    /// `get_task` chiamato almeno una volta.
    pub(super) started_sent: bool,
    /// Il tab esiste ancora.
    pub(super) tab_open: bool,
    /// Task concluso con un worktree: da ripulire quando il tab si chiude.
    pub(super) cleanup_on_close: bool,
    /// Solo pianificatori: la proposta (una sola) e' stata registrata.
    pub(super) proposed: bool,
}

impl Worker {
    pub(super) fn info(&self, status: WorkerStatus) -> AutomationTabInfo {
        AutomationTabInfo {
            tab_id: self.session.clone(),
            status,
            title: self.title.clone(),
            parent_tab_id: String::new(),
            last_report: self.last_report.clone(),
        }
    }
}

/// Evento di un hook come lo vede il registro.
#[derive(Debug, Clone, Copy, Default)]
pub struct HookEvent<'a> {
    pub event: &'a str,
    /// Solo `needs-input`: cosa aspetta l'agente.
    pub wait: Option<WaitKind>,
    pub stop_hook_active: Option<bool>,
    pub background_work: Option<u32>,
    /// `session_id` dello stdin dell'hook.
    pub agent_session_id: Option<&'a str>,
    /// Solo `session-start`: `source` dello stdin.
    pub source: Option<&'a str>,
    /// Solo `session-end`: `reason` dello stdin.
    pub reason: Option<&'a str>,
}

impl Registry {
    /// Cambia l'attivita' di un worker (sul suo task) e la pubblica; l'ingresso
    /// in attenzione o in attesa di una risposta notifica l'utente.
    pub(super) fn set_activity(&mut self, session: &str, activity: WorkerActivity, now: i64) {
        let Some(w) = self.workers.get(session) else {
            return;
        };
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        let before = self.worker_activity(session);
        let was_attention = before.is_some_and(WorkerActivity::needs_attention);
        if self.boards.get_mut(&ws).is_some_and(|b| b.worker_changed(&tid, session, activity, now)) {
            self.mark_dirty(&ws);
        }
        self.touch(session, now);
        if activity.needs_attention() && !was_attention {
            self.notify(&ws, &tid, Some(session), "attention");
        }
        if activity == WorkerActivity::AwaitingAnswer && before != Some(activity) {
            self.notify(&ws, &tid, Some(session), "question");
        }
    }

    /// Applica `signal` al worker `session` se e' attivo (i finiti ignorano
    /// tutto). Ritorna se lo Stop hook va bloccato.
    fn signal_worker(&mut self, session: &str, signal: WorkerSignal, now: i64) -> bool {
        let Some(activity) = self.worker_activity(session) else {
            return false;
        };
        let step = activity.on(signal);
        if let Some(to) = step.to {
            self.set_activity(session, to, now);
        }
        step.block_stop
    }

    // --- hook e chiusura tab ---

    /// Evento di un hook con token valido. Per i worker aggiorna lo stato;
    /// ritorna `Some(motivo)` se lo Stop hook va bloccato: il worker ha chiuso
    /// il turno senza riferire e l'agente deve proseguire. Gli eventi di
    /// un'altra sessione dell'agente (processi secondari) o senza id dopo il
    /// legame sono ignorati.
    pub fn on_hook(&mut self, tab: &str, hook: &HookEvent, now: i64) -> Option<String> {
        let w = self.workers.get_mut(tab).filter(|w| w.tab_open)?;
        if !w.agent.admit(hook) {
            if hook.event == "session-end" {
                self.note_ignored_session_end(tab, &agent_session::foreign_reason(hook), now);
            }
            return None;
        }
        match hook.event {
            // L'attesa inattiva (dopo la fine del turno) non e' una
            // richiesta di permesso: il worker non e' fermo su un dialogo.
            "needs-input" if hook.wait == Some(WaitKind::Idle) => {}
            "needs-input" => {
                self.signal_worker(tab, WorkerSignal::PermissionPrompt, now);
            }
            "finished" => {
                let signal = WorkerSignal::TurnEnded {
                    background_work: hook.background_work.is_some_and(|n| n > 0),
                    stop_hook_active: hook.stop_hook_active == Some(true),
                    question_pending: self.has_pending_question(tab),
                };
                if self.signal_worker(tab, signal, now) {
                    return Some(STOP_BLOCK_REASON.to_string());
                }
            }
            "session-end" if self.worker_activity(tab).is_some() => {
                if agent_session::switches_session(hook) {
                    let why = format!("the agent continues in a new session ({})", hook.reason.unwrap_or_default());
                    self.note_ignored_session_end(tab, &why, now);
                } else {
                    let cause = FailCause::SessionEnded { session: tab.to_string() };
                    self.close_worker(tab, agent_session::SESSION_END_NOTE, cause, now);
                }
            }
            _ => {}
        }
        None
    }

    /// L'utente ha premuto Invio nel terminale del worker: se aspettava lui,
    /// riprende a lavorare (nessun hook segnala la risposta). Se aspettava la
    /// risposta a una domanda, l'utente ha risposto qui: la domanda si chiude.
    pub fn user_input(&mut self, tab: &str, now: i64) {
        if self.workers.get(tab).is_some_and(|w| w.tab_open) {
            if self.worker_activity(tab) == Some(WorkerActivity::AwaitingAnswer) {
                self.close_question_answered_in_terminal(tab, now);
            }
            self.signal_worker(tab, WorkerSignal::UserInput, now);
        }
    }

    /// Il worker e' finito senza report: stato `closed`, task `failed` con la
    /// nota e la causa.
    pub(super) fn close_worker(&mut self, session: &str, note: &str, cause: FailCause, now: i64) {
        let Some(w) = self.workers.get_mut(session) else {
            return;
        };
        w.ended.get_or_insert(WorkerEnd::Closed);
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        self.touch(session, now);
        if self.fail_task(&ws, &tid, Some(session), note, Failure { cause, reason: FailReason::WorkerExited }, now) {
            self.notify(&ws, &tid, Some(session), "failed");
        }
    }

    /// Il tab del worker non esiste piu' (chiuso dall'utente, dall'app o
    /// shell uscita). Un task ancora in corso fallisce; uno concluso con un
    /// worktree viene ripulito.
    pub fn tab_closed(&mut self, tab: &str, now: i64) {
        let Some(w) = self.workers.get_mut(tab) else {
            return;
        };
        if !w.tab_open {
            return;
        }
        w.tab_open = false;
        let (ws, tid, cleanup) = (w.workspace_id.clone(), w.task_id.clone(), std::mem::take(&mut w.cleanup_on_close));
        if self.worker_activity(tab).is_some() {
            self.close_worker(tab, "the tab was closed before complete_task", FailCause::Other, now);
        } else {
            self.touch(tab, now);
        }
        self.queue_read_only_check(tab);
        if cleanup {
            self.queue_cleanup(&ws, &tid);
        }
    }
}
