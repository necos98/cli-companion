//! Annullamento delle esecuzioni E2E (il task lascia `running`, il workspace
//! viene archiviato) e ripresa dopo un riavvio.

use super::e2e::{archive, complete_failed, flagged, run_e2e, COMMAND};
use super::*;
use crate::board::E2eStatus;

type Leave = fn(&AutomationState, &FakeHost, &str);

#[test]
fn leaving_running_cancels_a_queued_run() {
    let cases: [(&str, Leave); 3] = [
        ("stop", |st, h, _| {
            boards::task_action(st, h, "ws-1", "a", "stop").unwrap();
        }),
        ("tab closed", tab_closed),
        ("complete failed", |st, h, w| {
            complete_failed(st, h, w).unwrap();
        }),
    ];
    for (name, leave) in cases {
        let (st, h, w) = flagged(DeliveryMode::Commit, false);
        h.hold_defers();
        run_e2e(&st, &h, &w);
        leave(&st, &h, &w);
        h.run_held();
        let a = task(&st, &h, "a");
        assert_eq!((a["e2e_run"]["status"].as_str(), a["e2e_run"]["error"].as_str()), (Some("error"), Some("cancelled")), "{name}");
        assert!(h.hook_log.lock().unwrap().iter().all(|l| !l.contains(COMMAND)), "{name}: the command never ran");
        assert!(st.e2e.reserved_ports().is_empty(), "{name}");
        if name == "complete failed" {
            assert_eq!(a["report"]["e2e"]["error"], "cancelled", "the report copy follows the run");
        }
    }
}

#[test]
fn archiving_the_workspace_cancels_every_run() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    run_e2e(&st, &h, &w);
    archive(&st, &h);
    h.run_held();
    assert_eq!(task(&st, &h, "a")["status"], "queued");
    assert!(h.hook_log.lock().unwrap().iter().all(|l| !l.contains(COMMAND)));
}

#[test]
fn a_new_attempt_ignores_the_result_of_a_stale_run() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    run_e2e(&st, &h, &w);
    boards::task_action(&st, &h, "ws-1", "a", "stop").unwrap();
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    h.run_held();
    assert!(task(&st, &h, "a").get("e2e_run").is_none());
}

// --- caricamento ---

#[test]
fn a_run_in_progress_is_an_interrupted_error_after_a_restart() {
    let (st, h, w) = flagged(DeliveryMode::Commit, false);
    h.hold_defers();
    run_e2e(&st, &h, &w);
    let raw = serde_json::to_string(st.inner.lock().unwrap().boards.get("ws-1").unwrap()).unwrap();
    let loaded = Board::from_json(&raw, 1).unwrap();
    let run = loaded.task("a").unwrap().e2e_run.as_ref().unwrap();
    assert_eq!((run.status, run.error.as_deref()), (E2eStatus::Error, Some(board::INTERRUPTED_NOTE)));
}
