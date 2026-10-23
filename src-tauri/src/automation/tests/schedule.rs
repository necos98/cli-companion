//! Fasce orarie come gate dello scheduler e standby.

use super::*;
use crate::schedule::Window;

fn hhmm(s: &str) -> u16 {
    crate::schedule::parse_hhmm(s).unwrap()
}

fn evening() -> Schedule {
    Schedule { enabled: true, windows: vec![Window { start: "20:00".into(), end: "23:00".into() }], keep_awake: true }
}

/// Host con consegna in place (un worker per repository) e la fascia 20-23.
fn scheduled(minute: &str, use_schedule: bool) -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Commit, false));
    *h.minute.lock().unwrap() = hhmm(minute);
    *h.use_schedule.lock().unwrap() = use_schedule;
    st.schedule_set(&h, evening(), now_millis()).unwrap();
    (st, h)
}

fn status(st: &AutomationState, h: &FakeHost, id: &str) -> String {
    task(st, h, id)["status"].as_str().unwrap().to_string()
}

#[test]
fn outside_the_window_nothing_starts_for_workspaces_that_use_the_schedule() {
    let (st, h) = scheduled("12:00", true);
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status(&st, &h, "a"), "queued");
    assert!(h.spawns().is_empty());
    assert!(view(&st, &h).schedule_blocked);
    // Il timer vede l'apertura: la board riparte da sola.
    *h.minute.lock().unwrap() = hhmm("20:30");
    st.tick(&h, now_millis());
    assert_eq!(status(&st, &h, "a"), "running");
    assert!(!view(&st, &h).schedule_blocked);
    // Chiusura: il task in corso finisce, il successivo non parte.
    queue(&st, &h, vec![spec("b", &[])]);
    *h.minute.lock().unwrap() = hhmm("23:30");
    st.tick(&h, now_millis());
    assert!(view(&st, &h).schedule_blocked);
    assert_eq!(status(&st, &h, "a"), "running");
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    assert_eq!((status(&st, &h, "a"), status(&st, &h, "b")), ("done".into(), "queued".into()));
    // Riapre il giorno dopo.
    *h.minute.lock().unwrap() = hhmm("20:01");
    st.tick(&h, now_millis());
    assert_eq!(status(&st, &h, "b"), "running");
}

#[test]
fn a_workspace_without_use_schedule_ignores_the_window() {
    let (st, h) = scheduled("12:00", false);
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status(&st, &h, "a"), "running");
    assert!(!view(&st, &h).schedule_blocked);
    // Fasce disattivate: anche chi le usa parte.
    let (st, h) = scheduled("12:00", true);
    st.schedule_set(&h, Schedule { enabled: false, ..evening() }, now_millis()).unwrap();
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status(&st, &h, "a"), "running");
}

#[test]
fn turning_use_schedule_on_or_off_applies_immediately() {
    let (st, h) = scheduled("12:00", false);
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[])]);
    assert_eq!((status(&st, &h, "a"), status(&st, &h, "b")), ("running".into(), "queued".into()));
    *h.use_schedule.lock().unwrap() = true;
    st.reconcile(&h);
    assert!(view(&st, &h).schedule_blocked);
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    assert_eq!(status(&st, &h, "b"), "queued", "now it waits for the window");
    *h.use_schedule.lock().unwrap() = false;
    st.reconcile(&h);
    assert_eq!(status(&st, &h, "b"), "running");
}

#[test]
fn schedule_set_validates_saves_and_emits_the_state() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    *h.minute.lock().unwrap() = hhmm("21:00");
    let state = st.schedule_set(&h, evening(), now_millis()).unwrap();
    assert!(state.window_open && state.next_change_at.is_some());
    assert_eq!(h.saved_schedule.lock().unwrap().clone().unwrap(), evening());
    assert_eq!(h.schedule_events.lock().unwrap().len(), 1);
    // Un orario non valido non salva nulla.
    let bad = Schedule { windows: vec![Window { start: "1:00".into(), end: "02:00".into() }], ..evening() };
    assert!(st.schedule_set(&h, bad, now_millis()).unwrap_err().contains("orario non valido"));
    assert_eq!(h.saved_schedule.lock().unwrap().clone().unwrap(), evening());
    // Il timer emette `schedule-state` solo quando cambia.
    let n = h.schedule_events.lock().unwrap().len();
    st.tick(&h, now_millis());
    st.tick(&h, now_millis());
    assert_eq!(h.schedule_events.lock().unwrap().len(), n);
    *h.minute.lock().unwrap() = hhmm("23:10");
    st.tick(&h, now_millis());
    let events = h.schedule_events.lock().unwrap().clone();
    assert_eq!(events.len(), n + 1);
    assert!(!events.last().unwrap().window_open);
    assert_eq!(st.schedule_state(&h, now_millis()).schedule, evening());
}

#[test]
fn the_system_stays_awake_while_any_task_runs() {
    let (st, h) = scheduled("21:00", false);
    assert!(h.awake.lock().unwrap().is_empty());
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);
    // Un altro task in coda non cambia la decisione.
    queue(&st, &h, vec![spec("b", &[])]);
    assert_eq!(h.awake.lock().unwrap().len(), 1);
    finish(&st, &h, &session_of(&st, &h, "a"), "completed");
    // b parte subito dopo a: resta sveglio.
    assert_eq!(h.awake.lock().unwrap().len(), 1);
    finish(&st, &h, &session_of(&st, &h, "b"), "completed");
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false]);
    // Vale anche fuori dalla fascia e senza fasce attive; keep_awake spento: mai.
    let (st, h) = scheduled("12:00", false);
    st.schedule_set(&h, Schedule { keep_awake: false, ..evening() }, now_millis()).unwrap();
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(status(&st, &h, "a"), "running");
    assert!(h.awake.lock().unwrap().is_empty());
    st.schedule_set(&h, Schedule { keep_awake: true, enabled: false, ..evening() }, now_millis()).unwrap();
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);
}

#[test]
fn an_archived_workspace_does_not_keep_the_system_awake() {
    let (st, h) = scheduled("21:00", false);
    queue(&st, &h, vec![spec("a", &[])]);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);
    *h.archived.lock().unwrap() = true;
    st.set_archived(&h, "ws-1", true);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false]);
}
