//! I task conclusi oltre il tetto escono dalla board prima che sia salvata:
//! `Effect::Archive` precede sempre `Effect::Publish`.

use super::*;
use crate::board::{Finished, TaskSpec, TaskState, TaskStatus, MAX_FINISHED_KEPT};

/// `n` task conclusi "t0".."t<n-1>" (il numero e' il tempo di fine), sulla board "ws-1".
fn board_with_finished(n: usize) -> Board {
    let mut b = Board::new("ws-1");
    for i in 0..n {
        let spec = TaskSpec { id: format!("t{i}"), title: format!("T{i}"), task: "t".into(), ..TaskSpec::default() };
        let mut t = Task::from_spec(spec, 1);
        t.set_state(TaskState::Done(Finished { started_at: None, finished_at: i as i64, report: None }));
        b.tasks.push(t);
    }
    b
}

fn flush(reg: &mut Registry, h: &FakeHost) -> Vec<Effect> {
    reg.mark_dirty("ws-1");
    reg.flush_boards(1000, 0, &|id| h.workspace(id), &|_, _| Some(Readiness::Ready));
    reg.take_effects()
}

fn position(effects: &[Effect], f: impl Fn(&Effect) -> bool) -> Option<usize> {
    effects.iter().position(f)
}

#[test]
fn archiving_comes_before_the_publication_of_the_trimmed_board() {
    let (mut reg, h) = (Registry::default(), FakeHost::default());
    reg.load_board(board_with_finished(MAX_FINISHED_KEPT + 5));
    let effects = flush(&mut reg, &h);
    let archive = position(&effects, |e| matches!(e, Effect::Archive { .. })).expect("Archive");
    let publish = position(&effects, |e| matches!(e, Effect::Publish(..))).expect("Publish");
    assert!(archive < publish, "{effects:?}");
    let Effect::Archive { ws_id, tasks } = &effects[archive] else { unreachable!() };
    let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!((ws_id.as_str(), ids), ("ws-1", vec!["t0", "t1", "t2", "t3", "t4"]));
    let Effect::Publish(board, view) = &effects[publish] else { unreachable!() };
    assert_eq!((board.tasks.len(), view.tasks.len()), (MAX_FINISHED_KEPT, MAX_FINISHED_KEPT));
    assert!(board.task("t4").is_none() && board.task("t5").is_some());
}

#[test]
fn nothing_is_archived_within_the_limit_or_twice() {
    let (mut reg, h) = (Registry::default(), FakeHost::default());
    reg.load_board(board_with_finished(MAX_FINISHED_KEPT));
    let effects = flush(&mut reg, &h);
    assert!(position(&effects, |e| matches!(e, Effect::Archive { .. })).is_none());
    assert!(position(&effects, |e| matches!(e, Effect::Publish(..))).is_some());
    reg.load_board(board_with_finished(MAX_FINISHED_KEPT + 1));
    assert!(position(&flush(&mut reg, &h), |e| matches!(e, Effect::Archive { .. })).is_some());
    assert!(position(&flush(&mut reg, &h), |e| matches!(e, Effect::Archive { .. })).is_none());
}

#[test]
fn the_host_archives_then_saves_the_board_without_those_tasks() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    st.load_board(board_with_finished(MAX_FINISHED_KEPT + 2));
    st.run(&h, |r| r.mark_dirty("ws-1"));
    let archived: Vec<(String, String)> =
        h.archive_log.lock().unwrap().iter().map(|(w, t)| (w.clone(), t.id.clone())).collect();
    assert_eq!(archived, [("ws-1".to_string(), "t0".to_string()), ("ws-1".to_string(), "t1".to_string())]);
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.tasks.len(), MAX_FINISHED_KEPT);
    assert!(saved.tasks.iter().all(|t| t.status() == TaskStatus::Done));
}

#[test]
fn a_failing_archive_does_not_stop_the_board_from_being_saved() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    *h.archive_err.lock().unwrap() = Some("disco pieno".to_string());
    st.load_board(board_with_finished(MAX_FINISHED_KEPT + 1));
    st.run(&h, |r| r.mark_dirty("ws-1"));
    assert!(h.archive_log.lock().unwrap().is_empty());
    // Salvata comunque, ora con anche il task che l'archivio ha rifiutato.
    assert_eq!(h.saved.lock().unwrap().get("ws-1").unwrap().tasks.len(), MAX_FINISHED_KEPT + 1);
}

fn ids(b: &Board) -> Vec<&str> {
    b.tasks.iter().map(|t| t.id.as_str()).collect()
}

#[test]
fn tasks_the_archive_refused_are_kept_and_archived_by_the_next_change() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    *h.archive_err.lock().unwrap() = Some("disco pieno".to_string());
    st.load_board(board_with_finished(MAX_FINISHED_KEPT + 2));
    st.run(&h, |r| r.mark_dirty("ws-1"));
    // Ne' persi ne' duplicati: restano sia nella board salvata sia nel registro.
    assert!(h.archive_log.lock().unwrap().is_empty());
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.tasks.len(), MAX_FINISHED_KEPT + 2);
    assert!(saved.task("t0").is_some() && saved.task("t1").is_some());
    {
        let reg = st.registry();
        let board = reg.boards.get("ws-1").unwrap();
        assert_eq!(board.tasks.len(), MAX_FINISHED_KEPT + 2, "{:?}", ids(board));
        assert!(board.task("t0").is_some() && board.task("t1").is_some());
    }
    // Il ripristino non ha segnato la board: nessun giro di ripubblicazione.
    assert!(st.registry().dirty.is_empty());
    // Archivio di nuovo scrivibile: la prossima modifica li archivia.
    *h.archive_err.lock().unwrap() = None;
    st.run(&h, |r| r.mark_dirty("ws-1"));
    let archived: Vec<String> = h.archive_log.lock().unwrap().iter().map(|(_, t)| t.id.clone()).collect();
    assert_eq!(archived, ["t0", "t1"]);
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.tasks.len(), MAX_FINISHED_KEPT);
    assert!(saved.task("t0").is_none() && saved.task("t1").is_none());
    assert_eq!(st.registry().boards.get("ws-1").unwrap().tasks.len(), MAX_FINISHED_KEPT);
}
