//! Esecuzione manuale degli E2E di un repository (comandi della UI).

use super::e2e::{archive, e2e_repo, flagged, run_e2e, set_code, writes_junit, CHECKOUT, COMMAND, JUNIT_PASS, STATUS_OUT};
use super::*;
use crate::automation::e2e::manual;
use crate::board::E2eStatus;

fn manual_setup() -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    h.repo_cfg.lock().unwrap().insert(".".into(), e2e_repo());
    set_code(&h, "c1", "t1", STATUS_OUT);
    writes_junit(&h, CHECKOUT, JUNIT_PASS);
    (st, h)
}

#[test]
fn a_manual_run_stores_its_result_and_emits_two_events() {
    let (st, h) = manual_setup();
    h.hold_defers();
    let started = manual::run_repo(&st, &h, "ws-1", None).unwrap();
    assert_eq!(started.status, E2eStatus::Running);
    assert_eq!(manual::last_result(&st, &h, "ws-1", None).unwrap().unwrap().status, E2eStatus::Running);
    assert_eq!(manual::run_repo(&st, &h, "ws-1", Some(".")).unwrap_err(), "esecuzione E2E gia' in corso per questo repository");
    h.run_held();
    let last = manual::last_result(&st, &h, "ws-1", Some(".")).unwrap().unwrap();
    assert_eq!((last.run_id.as_str(), last.status), (started.run_id.as_str(), E2eStatus::Passed));
    let events = h.e2e_events();
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].run.status, events[1].run.status), (E2eStatus::Running, E2eStatus::Passed));
    assert!(events.iter().all(|e| e.workspace_id == "ws-1" && e.repo == "."));
    // Terminata: se ne puo' lanciare un'altra.
    assert!(manual::run_repo(&st, &h, "ws-1", None).is_ok());
}

#[test]
fn a_manual_run_validates_workspace_repository_and_config() {
    let (st, h) = manual_setup();
    assert_eq!(manual::run_repo(&st, &h, "nope", None).unwrap_err(), "workspace non trovato");
    assert!(manual::run_repo(&st, &h, "ws-1", Some("../x")).is_err());
    assert!(manual::run_repo(&st, &h, "ws-1", Some("app")).unwrap_err().contains("does not exist"));
    h.dirs.lock().unwrap().push("C:\\p\\app".to_string());
    assert_eq!(manual::run_repo(&st, &h, "ws-1", Some("app")).unwrap_err(), "E2E non configurati per il repository");
    assert_eq!(manual::last_result(&st, &h, "ws-1", None).unwrap(), None);
    assert_eq!(manual::last_result(&st, &h, "nope", None).unwrap_err(), "workspace non trovato");
    *h.archived.lock().unwrap() = true;
    assert_eq!(manual::run_repo(&st, &h, "ws-1", None).unwrap_err(), "workspace archiviato");
}

#[test]
fn a_manual_run_has_no_task_variables_and_runs_in_the_repository_folder() {
    let (st, h) = manual_setup();
    manual::run_repo(&st, &h, "ws-1", None).unwrap();
    let log = h.hook_log.lock().unwrap().clone();
    let line = log.iter().find(|l| l.starts_with(&format!("{CHECKOUT}|{COMMAND}|"))).unwrap_or_else(|| panic!("{log:?}"));
    assert!(line.contains("TASK_ID=,") && line.contains("CLI_COMPANION_E2E_PORT="), "{line}");
}

#[test]
fn archiving_the_workspace_cancels_a_manual_run() {
    let (st, h) = manual_setup();
    h.hold_defers();
    manual::run_repo(&st, &h, "ws-1", None).unwrap();
    archive(&st, &h);
    h.run_held();
    let last = manual::last_result(&st, &h, "ws-1", None).unwrap().unwrap();
    assert_eq!((last.status, last.error.as_deref()), (E2eStatus::Error, Some("cancelled")));
}

#[test]
fn concurrent_runs_in_different_folders_get_different_ports() {
    let (st, h, w) = flagged(DeliveryMode::Pr, true);
    h.hold_defers();
    run_e2e(&st, &h, &w);
    manual::run_repo(&st, &h, "ws-1", None).unwrap();
    let ports = st.e2e.reserved_ports();
    assert_eq!(ports.len(), 2);
    assert_ne!(ports[0], ports[1]);
    h.run_held();
    assert!(st.e2e.reserved_ports().is_empty());
}
