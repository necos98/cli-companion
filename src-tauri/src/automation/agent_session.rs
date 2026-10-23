//! Legame tra un worker e la sessione del suo agente. Gli hook del plugin
//! scattano anche nei processi secondari avviati dal worker (un `claude`
//! figlio eredita `CLI_COMPANION_SESSION` e gli hook): i loro eventi portano
//! il tab del worker ma un altro `session_id`. Il worker lega a se' il primo
//! id che vede e ignora gli eventi degli altri. Se un `session-end` lo chiude
//! lo stesso mentre il tab e' aperto, il suo `complete_task` riapre il task.

use super::boards::NOTE_MAX;
use super::*;

/// Nota del task fallito perche' la sessione dell'agente e' finita.
pub(super) const SESSION_END_NOTE: &str = "the session ended without a complete_task report";
/// Nota del task riaperto da `complete_task` dopo un `session-end`.
const REOPENED_NOTE: &str = "reopened: complete_task arrived after a session-end while the worker tab was still open";
/// `source` di un `session-start` con cui la stessa sessione utente continua
/// con un nuovo id (`/clear`, `/resume`, compattazione).
const CONTINUING_SOURCES: [&str; 3] = ["clear", "resume", "compact"];
/// `reason` di un `session-end` dopo cui l'agente prosegue nello stesso
/// processo con un'altra sessione (`/clear`, `/resume`).
const SWITCHING_REASONS: [&str; 2] = ["clear", "resume"];

/// Id della sessione dell'agente legata al worker (`session_id` degli hook).
#[derive(Debug, Default)]
pub(super) struct AgentSession {
    id: Option<String>,
}

impl AgentSession {
    /// L'evento viene dalla sessione del worker? Il primo id visto si lega;
    /// un id diverso e' ammesso (e rilegato) solo da un `session-start` della
    /// stessa sessione utente che continua. Eventi senza id: ammessi, come
    /// prima del legame (vecchi plugin).
    pub(super) fn admit(&mut self, hook: &HookEvent) -> bool {
        let Some(id) = hook.agent_session_id else {
            return true;
        };
        let own = match self.id.as_deref() {
            None => true,
            Some(bound) => bound == id || continues_session(hook),
        };
        if own && self.id.as_deref() != Some(id) {
            self.id = Some(id.to_string());
        }
        own
    }
}

fn continues_session(hook: &HookEvent) -> bool {
    hook.event == "session-start" && hook.source.is_some_and(|s| CONTINUING_SOURCES.contains(&s))
}

/// `session-end` dopo cui l'agente resta vivo con un'altra sessione.
pub(super) fn switches_session(hook: &HookEvent) -> bool {
    hook.reason.is_some_and(|r| SWITCHING_REASONS.contains(&r))
}

impl Registry {
    /// Traccia sul task in corso di un `session-end` ignorato (`why`), per la
    /// diagnosi. Non ripete una traccia gia' presente.
    pub(super) fn note_ignored_session_end(&mut self, session: &str, why: &str, now: i64) {
        let Some(w) = self.workers.get(session) else {
            return;
        };
        let (ws, tid) = (w.workspace_id.clone(), w.task_id.clone());
        let line = format!("ignored a session-end: {why}");
        let changed = self
            .boards
            .get_mut(&ws)
            .and_then(|b| b.task_mut(&tid))
            .filter(|t| t.status == TaskStatus::Running && t.session.as_deref() == Some(session))
            .filter(|t| !t.note.as_deref().is_some_and(|n| n.contains(&line)))
            .map(|t| {
                let note = match t.note.take() {
                    Some(n) => format!("{n}; {line}"),
                    None => line,
                };
                t.note = Some(clip_chars(&note, NOTE_MAX));
                t.updated_at = now;
            })
            .is_some();
        if changed {
            self.mark_dirty(&ws);
        }
    }

    /// Prima di `complete_task`: se il worker e' stato chiuso da un
    /// `session-end` ma il suo tab e' ancora aperto, riapre il suo task
    /// (stesso tentativo, nessun retry nel frattempo) e il worker torna
    /// `running`. Altrimenti non fa nulla.
    pub(super) fn reopen_after_session_end(&mut self, session: &str, now: i64) {
        let Some(w) = self.workers.get(session).filter(|w| w.tab_open && w.status == WorkerStatus::Closed) else {
            return;
        };
        let (ws, tid, attempt) = (w.workspace_id.clone(), w.task_id.clone(), w.attempt);
        let reopened = self
            .boards
            .get_mut(&ws)
            .is_some_and(|b| b.reopen_failed(&tid, session, attempt, SESSION_END_NOTE, REOPENED_NOTE, now));
        if !reopened {
            return;
        }
        if let Some(w) = self.workers.get_mut(session) {
            w.status = WorkerStatus::Running;
        }
        self.mark_dirty(&ws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev<'a>(event: &'a str, id: Option<&'a str>) -> HookEvent<'a> {
        HookEvent { event, agent_session_id: id, ..Default::default() }
    }

    #[test]
    fn the_first_id_binds_and_other_ids_are_foreign() {
        let mut s = AgentSession::default();
        assert!(s.admit(&ev("finished", None)), "nessun id: ammesso, nessun legame");
        assert!(s.admit(&ev("finished", Some("a"))));
        assert!(s.admit(&ev("session-end", Some("a"))));
        assert!(s.admit(&ev("needs-input", None)));
        for e in ["session-end", "finished", "needs-input"] {
            assert!(!s.admit(&ev(e, Some("b"))), "{e}");
        }
        assert!(!s.admit(&HookEvent { source: Some("startup"), ..ev("session-start", Some("b")) }));
        assert!(!s.admit(&HookEvent { source: Some("fork"), ..ev("session-start", Some("b")) }));
        assert!(s.admit(&ev("finished", Some("a"))), "il legame resta su a");
    }

    #[test]
    fn a_continuing_session_start_rebinds() {
        for source in CONTINUING_SOURCES {
            let mut s = AgentSession::default();
            assert!(s.admit(&ev("session-start", Some("a"))));
            assert!(s.admit(&HookEvent { source: Some(source), ..ev("session-start", Some("b")) }), "{source}");
            assert!(s.admit(&ev("session-end", Some("b"))), "{source}");
            assert!(!s.admit(&ev("session-end", Some("a"))), "{source}: il vecchio id non e' piu' suo");
        }
    }

    #[test]
    fn only_clear_and_resume_switch_session() {
        let end = |reason| HookEvent { reason, ..ev("session-end", Some("a")) };
        assert!(switches_session(&end(Some("clear"))) && switches_session(&end(Some("resume"))));
        for r in [None, Some("other"), Some("logout"), Some("prompt_input_exit")] {
            assert!(!switches_session(&end(r)), "{r:?}");
        }
    }
}
