//! Fine della sessione del worker: eventi di altre sessioni dell'agente
//! (processi secondari) ignorati, legame che segue `/clear` e `/resume`,
//! riapertura del task se il worker riferisce dopo un `session-end`.

use super::*;
use agent_session::SESSION_END_NOTE;

fn on(st: &AutomationState, h: &FakeHost, tab: &str, ev: HookEvent) -> Option<String> {
    st.run(h, |r| r.on_hook(tab, &ev, 1))
}

/// Evento di un hook dalla sessione `id` dell'agente.
fn from<'a>(event: &'a str, id: &'a str) -> HookEvent<'a> {
    HookEvent { event, agent_session_id: Some(id), ..Default::default() }
}

fn started<'a>(id: &'a str, source: &'a str) -> HookEvent<'a> {
    HookEvent { source: Some(source), ..from("session-start", id) }
}

fn ended<'a>(id: &'a str, reason: &'a str) -> HookEvent<'a> {
    HookEvent { reason: Some(reason), ..from("session-end", id) }
}

fn note(st: &AutomationState, h: &FakeHost) -> String {
    task(st, h, "a")["note"].as_str().unwrap_or_default().to_string()
}

/// Worker di `a` legato alla sessione `own` dell'agente, `get_task` fatto.
fn bound(mode: DeliveryMode, parallel: bool) -> (AutomationState, FakeHost, String) {
    let (st, h, w) = running(mode, parallel);
    on(&st, &h, &w, started("own", "startup"));
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    (st, h, w)
}

#[test]
fn hooks_of_another_agent_session_are_ignored() {
    let (st, h, w) = bound(DeliveryMode::Commit, false);
    // Un claude figlio del worker: parte, chiede un permesso, chiude il turno ed esce.
    on(&st, &h, &w, started("child", "startup"));
    on(&st, &h, &w, from("needs-input", "child"));
    assert_eq!(on(&st, &h, &w, from("finished", "child")), None, "nessun promemoria per un'altra sessione");
    on(&st, &h, &w, ended("child", "other"));
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["worker_status"].as_str()), (Some("running"), Some("running")));
    assert!(h.notices().is_empty());
    assert_eq!(note(&st, &h), "ignored a session-end: it came from another agent session (child)");
    // Ripetuto: la traccia non si duplica.
    on(&st, &h, &w, ended("child", "other"));
    assert_eq!(note(&st, &h).matches("ignored a session-end").count(), 1);
    // La sessione del worker funziona come prima.
    assert_eq!(on(&st, &h, &w, from("finished", "own")).as_deref(), Some(STOP_BLOCK_REASON));
    on(&st, &h, &w, ended("own", "other"));
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["note"].as_str()), (Some("failed"), Some(SESSION_END_NOTE)));
    assert_eq!(h.notices().len(), 1);
}

#[test]
fn hooks_without_a_session_id_keep_the_old_behaviour() {
    let (st, h, w) = bound(DeliveryMode::Commit, false);
    // Legato a `own`, un evento senza id (plugin vecchio) vale ancora.
    hook(&st, &h, &w, "needs-input");
    assert_eq!(task(&st, &h, "a")["worker_status"], "needs_input");
    hook(&st, &h, &w, "session-end");
    assert_eq!(task(&st, &h, "a")["status"], "failed");
}

#[test]
fn a_continuing_session_start_rebinds_the_worker() {
    for source in ["clear", "resume", "compact"] {
        let (st, h, w) = bound(DeliveryMode::Commit, false);
        on(&st, &h, &w, started("next", source));
        // Il vecchio id non e' piu' del worker, il nuovo si'.
        on(&st, &h, &w, ended("own", "other"));
        assert_eq!(task(&st, &h, "a")["status"], "running", "{source}");
        assert_eq!(on(&st, &h, &w, from("finished", "next")).as_deref(), Some(STOP_BLOCK_REASON), "{source}");
        on(&st, &h, &w, ended("next", "other"));
        assert_eq!(task(&st, &h, "a")["status"], "failed", "{source}");
    }
}

#[test]
fn clear_and_resume_end_the_session_but_not_the_worker() {
    for reason in ["clear", "resume"] {
        let (st, h, w) = bound(DeliveryMode::Commit, false);
        // `/clear`: finisce la sessione `own`, l'agente prosegue con `next`.
        on(&st, &h, &w, ended("own", reason));
        assert_eq!(task(&st, &h, "a")["status"], "running", "{reason}");
        assert_eq!(note(&st, &h), format!("ignored a session-end: the agent continues in a new session ({reason})"));
        on(&st, &h, &w, started("next", reason));
        call(&st, &h, &w, "complete_task", json!({"status": "completed", "summary": "ok"})).unwrap();
        assert_eq!(task(&st, &h, "a")["status"], "done", "{reason}");
        assert!(h.notices().is_empty(), "{reason}");
    }
}

#[test]
fn complete_task_after_a_session_end_reopens_the_task() {
    for (status, expected, notices) in [("completed", "done", 1), ("failed", "failed", 2)] {
        let (st, h, w) = bound(DeliveryMode::Pr, true);
        h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
        on(&st, &h, &w, ended("own", "other"));
        assert_eq!(task(&st, &h, "a")["status"], "failed");
        // Il worker era vivo: riferisce lo stesso.
        let r = call(&st, &h, &w, "complete_task", json!({"status": status, "summary": "late report"})).unwrap();
        assert_eq!(r, json!({"closing": true}));
        let a = task(&st, &h, "a");
        assert_eq!((a["status"].as_str(), a["report"]["status"].as_str()), (Some(expected), Some(status)), "{status}");
        assert_eq!(a["last_report"], "late report");
        assert!(a["session"].is_null() && a["worker_status"].is_null(), "{a}");
        assert!(a["note"].as_str().unwrap().starts_with("reopened:"), "{a}");
        assert!(h.log().contains(&format!("close:{w}")));
        // La notifica di fallimento gia' partita resta; un `failed` riferito notifica come sempre.
        assert_eq!(h.notices().iter().filter(|n| n.kind == "failed").count(), notices, "{status}");
        // Chiuso il tab, la conclusione e' quella normale (pulizia solo se consegnato).
        tab_closed(&st, &h, &w);
        let removed = h.git_log().iter().any(|l| l.contains("worktree remove"));
        assert_eq!(removed, status == "completed", "{status}");
        assert!(call(&st, &h, &w, "complete_task", json!({"status": status, "summary": "again"})).is_err());
    }
}

#[test]
fn complete_task_stays_closed_after_the_tab_closes_or_a_retry() {
    let closed = |st: &AutomationState, h: &FakeHost, w: &str| {
        let e = call(st, h, w, "complete_task", json!({"status": "completed", "summary": "late"})).unwrap_err();
        assert_eq!(e, "the session is closed");
    };
    // Tab chiuso davvero.
    let (st, h, w) = bound(DeliveryMode::Commit, false);
    hook(&st, &h, &w, "session-end");
    tab_closed(&st, &h, &w);
    closed(&st, &h, &w);
    assert_eq!(task(&st, &h, "a")["note"], SESSION_END_NOTE);
    // Ripreso da un retry: il nuovo worker e' in corso.
    let (st, h, w) = bound(DeliveryMode::Commit, false);
    hook(&st, &h, &w, "session-end");
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    let w2 = session_of(&st, &h, "a");
    closed(&st, &h, &w);
    assert_eq!(task(&st, &h, "a")["status"], "running");
    // Anche se il nuovo tentativo fallisce con la stessa nota.
    call(&st, &h, &w2, "get_task", json!({})).unwrap();
    hook(&st, &h, &w2, "session-end");
    let a = task(&st, &h, "a");
    assert_eq!((a["attempts"].as_u64(), a["note"].as_str()), (Some(2), Some(SESSION_END_NOTE)));
    closed(&st, &h, &w);
    assert_eq!(task(&st, &h, "a")["status"], "failed");
}
