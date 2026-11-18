//! Campi di contesto di `TaskView`: azioni permesse, modificabilita' e cascata.

use super::*;
use crate::agents::Readiness;
use crate::board::{
    parse_max_parallel, AgentGate, AgentUnavailableView, FailCause, FailReason, Finished, TaskEvent, TaskSpec,
    TaskState, WorkerActivity,
};
use serde_json::json;
use TaskAction::{Delete, DeleteCascade, MarkDone, Retry, Stop, ToTop};

/// Board con i task `(id, dipendenze, stato)` nell'ordine dato.
fn board(tasks: &[(&str, &[&str], TaskStatus)]) -> Board {
    let mut b = Board::new("ws-1");
    for (id, deps, status) in tasks {
        let spec = TaskSpec {
            id: id.to_string(),
            title: id.to_string(),
            task: format!("do {id}"),
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            ..TaskSpec::default()
        };
        let mut t = Task::from_spec(spec, 1);
        t.set_status(*status);
        b.tasks.push(t);
    }
    b
}

fn view(b: &Board, id: &str) -> TaskView {
    b.task_view(id, &AgentGate::default()).unwrap()
}

use TaskStatus::{Done, Failed, Queued, Running};

#[test]
fn a_task_without_dependents_gets_the_actions_of_its_status() {
    let expected: [(TaskStatus, &[TaskAction], bool); 4] = [
        (Queued, &[Delete, ToTop], true),
        (Running, &[Stop], false),
        (Failed, &[Retry, MarkDone, Delete], true),
        (Done, &[], false),
    ];
    for (status, actions, can_edit) in expected {
        let v = view(&board(&[("a", &[], status)]), "a");
        assert_eq!(v.allowed_actions, actions, "{status:?}");
        assert_eq!(v.can_edit, can_edit, "{status:?}");
        assert_eq!(v.can_reorder, status == Queued, "{status:?}");
        assert_eq!(v.cascade, CascadeDelete::default(), "{status:?}");
    }
}

#[test]
fn delete_gives_way_to_the_cascade_when_open_tasks_depend_on_the_task() {
    for status in [Queued, Failed] {
        let b = board(&[("a", &[], status), ("b", &["a"], Queued), ("c", &["b"], Queued), ("d", &["a"], Done)]);
        let v = view(&b, "a");
        assert!(!v.allowed_actions.contains(&Delete), "{status:?}: b la aspetta");
        assert!(v.allowed_actions.contains(&DeleteCascade), "{status:?}");
        assert_eq!(v.cascade.dependents, ["b", "c"], "{status:?}: d e' done e resta");
        assert!(v.cascade.running.is_empty());
        assert!(v.can_edit);
    }
    // I dipendenti a loro volta si possono togliere da soli.
    let b = board(&[("a", &[], Queued), ("b", &["a"], Queued), ("c", &["b"], Queued)]);
    assert_eq!(view(&b, "b").cascade.dependents, ["c"]);
    assert_eq!(view(&b, "c").allowed_actions, [Delete, ToTop]);
}

#[test]
fn a_done_dependent_does_not_block_delete() {
    let b = board(&[("a", &[], Failed), ("d", &["a"], Done)]);
    let v = view(&b, "a");
    assert_eq!(v.allowed_actions, [Retry, MarkDone, Delete]);
    assert_eq!(v.cascade, CascadeDelete::default());
}

#[test]
fn a_running_dependent_forbids_both_deletes_and_is_listed() {
    let b = board(&[("a", &[], Failed), ("b", &["a"], Running), ("c", &["b"], Queued)]);
    let v = view(&b, "a");
    assert_eq!(v.allowed_actions, [Retry, MarkDone]);
    assert_eq!(v.cascade.dependents, ["b", "c"]);
    assert_eq!(v.cascade.running, ["b"]);
    // La vista dice la verita': la cascata si rifiuterebbe davvero.
    let mut same = b.clone();
    assert!(same.delete_with_dependents("a", 2).unwrap_err().contains("b are running"));
}

#[test]
fn the_cascade_lists_dependents_in_queue_order_through_a_diamond() {
    let b = board(&[
        ("c", &["a", "b"], Queued),
        ("a", &[], Queued),
        ("b", &["a"], Queued),
        ("e", &["c"], Queued),
        ("x", &[], Queued),
    ]);
    assert_eq!(view(&b, "a").cascade.dependents, ["c", "b", "e"]);
    assert_eq!(view(&b, "x").cascade.dependents, Vec::<String>::new());
}

#[test]
fn the_cascade_does_not_cross_a_done_task() {
    let b = board(&[("a", &[], Failed), ("d", &["a"], Done), ("e", &["d"], Queued)]);
    assert!(view(&b, "a").cascade.dependents.is_empty());
}

#[test]
fn the_view_agrees_with_the_operations_it_advertises() {
    let b = board(&[
        ("a", &[], Failed),
        ("b", &["a"], Queued),
        ("q", &[], Queued),
        ("r", &[], Running),
        ("f", &[], Failed),
        ("d", &[], Done),
    ]);
    for v in b.view_with(None, false, &AgentGate::default()).tasks {
        let id = v.task.id.as_str();
        let attempt = |action: TaskAction| {
            let mut copy = b.clone();
            match action {
                Retry => copy.retry_task(id, 2),
                MarkDone => copy.mark_done(id, 2),
                Stop => copy.check_stop(id),
                Delete => copy.delete_task(id, 2).map(|_| ()),
                DeleteCascade => copy.delete_with_dependents(id, 2).map(|_| ()),
                ToTop => copy.task_to_top(id, 2),
            }
        };
        let mut reordered = b.clone();
        assert_eq!(v.can_reorder, reordered.move_task(id, None, 2).is_ok(), "{id}");
        for action in TaskAction::ALL {
            let listed = v.allowed_actions.contains(&action);
            let ok = attempt(action).is_ok();
            // `DeleteCascade` senza dipendenti riesce ma e' la stessa cosa di `Delete`: non si propone.
            if action == DeleteCascade && v.cascade.dependents.is_empty() {
                assert!(!listed, "{id}");
                continue;
            }
            assert_eq!(listed, ok, "{id} {action:?}");
        }
    }
}

#[test]
fn the_new_fields_have_stable_names_after_the_old_ones() {
    let b = board(&[("a", &[], Failed), ("b", &["a"], Queued)]);
    let v = serde_json::to_value(view(&b, "a")).unwrap();
    assert_eq!(v["allowed_actions"], json!(["retry", "mark_done", "delete_cascade"]));
    assert_eq!(v["can_edit"], true);
    assert_eq!(v["can_reorder"], false);
    assert_eq!(v["cascade"], json!({"dependents": ["b"], "running": []}));
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert!(keys.contains(&"waiting_on") && keys.contains(&"attention") && keys.contains(&"status"));
    let text = serde_json::to_string(&view(&b, "a")).unwrap();
    let pos = |k: &str| text.find(&format!("\"{k}\":")).unwrap();
    assert!(pos("updated_at") < pos("waiting_on") && pos("attention") < pos("allowed_actions"));
    assert!(pos("allowed_actions") < pos("can_edit") && pos("can_edit") < pos("can_reorder"));
    assert!(pos("can_reorder") < pos("cascade"));
}

#[test]
fn the_board_view_publishes_the_limits_the_validation_enforces() {
    let b = board(&[]);
    let v = serde_json::to_value(b.view_with(None, false, &AgentGate::default())).unwrap();
    assert_eq!(v["limits"], json!({"max_parallel": {"min": 1, "max": 16}}));
    let bounds = BoardLimits::CURRENT.max_parallel;
    assert!(parse_max_parallel(&json!(bounds.min)).is_ok() && parse_max_parallel(&json!(bounds.max)).is_ok());
    assert!(parse_max_parallel(&json!(bounds.min - 1)).is_err() && parse_max_parallel(&json!(bounds.max + 1)).is_err());
}

#[test]
fn a_long_chain_gets_its_views_in_one_pass() {
    let n = 600;
    let mut tasks = vec![("t0".to_string(), vec![])];
    tasks.extend((1..n).map(|i| (format!("t{i}"), vec![format!("t{}", i - 1)])));
    let mut b = Board::new("ws-1");
    for (id, deps) in tasks {
        let spec = TaskSpec { id: id.clone(), title: id.clone(), task: id, depends_on: deps, ..TaskSpec::default() };
        b.tasks.push(Task::from_spec(spec, 1));
    }
    let v = b.view_with(None, false, &AgentGate::default());
    assert_eq!(v.tasks[0].cascade.dependents.len(), n - 1);
    assert_eq!(v.tasks[n - 1].cascade.dependents.len(), 0);
    assert_eq!(v.tasks[n / 2].cascade.dependents.len(), n - n / 2 - 1);
}

#[test]
fn a_queued_task_exposes_why_its_agent_is_not_ready() {
    let b = board(&[("a", &[], Queued), ("r", &[], Running)]);
    let agent = b.task("a").unwrap().agent.clone();
    let blocked = AgentGate::build([agent.as_str()], &mut |_| Some(Readiness::NotFound));
    let v = b.view_with(None, false, &blocked);
    let reason = AgentUnavailableView { agent: agent.clone(), reason: Readiness::NotFound };
    assert_eq!(v.tasks[0].agent_unavailable, Some(reason));
    assert_eq!(v.tasks[1].agent_unavailable, None, "in corso: l'agente e' gia' stato usato");
    assert_eq!(
        v.to_list_json(None)["tasks"][0]["agent_unavailable"],
        json!({"agent": agent, "reason": "not-found"}),
        "riga compatta di list_tasks"
    );
    assert!(v.to_list_json(None)["tasks"][1].get("agent_unavailable").is_none());
    let json = serde_json::to_value(&v.tasks[0]).unwrap();
    assert_eq!(json["agent_unavailable"]["reason"], "not-found");
}

#[test]
fn no_reason_is_exposed_while_the_agent_is_being_checked_or_is_ready() {
    let b = board(&[("a", &[], Queued)]);
    let agent = b.task("a").unwrap().agent.clone();
    let checking = AgentGate::build([agent.as_str()], &mut |_| None);
    let ready = AgentGate::build([agent.as_str()], &mut |_| Some(Readiness::Ready));
    for gate in [&checking, &ready, &AgentGate::default()] {
        let v = b.view_with(None, false, gate);
        assert_eq!(v.tasks[0].agent_unavailable, None);
        assert!(serde_json::to_value(&v.tasks[0]).unwrap().get("agent_unavailable").is_none());
    }
}

/// Board con il task `a` in corso e il worker `w1` con una domanda in attesa.
fn board_with_a_question() -> Board {
    let mut b = Board::new("ws-1");
    let mut t = Task::from_spec(TaskSpec { id: "a".into(), title: "A".into(), ..TaskSpec::default() }, 1);
    t.apply(TaskEvent::Start, 2).unwrap();
    t.apply(TaskEvent::WorkerSpawned { attempt: 1, session: "w1".into(), worktree: None, note: None }, 3).unwrap();
    let ask =
        TaskEvent::QuestionAsked { session: "w1".into(), text: "Which db?".into(), options: vec!["SQLite".into()] };
    t.apply(ask, 4).unwrap();
    t.apply(TaskEvent::WorkerChanged { session: "w1".into(), activity: WorkerActivity::AwaitingAnswer }, 5).unwrap();
    b.tasks.push(t);
    b
}

#[test]
fn the_pending_question_is_in_the_view_and_does_not_need_attention() {
    let b = board_with_a_question();
    let v = view(&b, "a");
    assert_eq!(v.question.as_ref().map(|q| q.text.as_str()), Some("Which db?"));
    assert!(!v.attention, "awaiting_answer is not attention");
    let json = v.to_json(true);
    assert_eq!(json["worker_status"], "awaiting_answer");
    assert_eq!(json["question"]["id"], "q-1-1");
    assert_eq!(json["question"]["options"], json!(["SQLite"]));
    let list = b.view_with(None, false, &AgentGate::default()).to_list_json(None);
    assert_eq!(list["tasks"][0]["question"], "Which db?");
    assert!(list["tasks"][0].get("attention").is_none());
}

#[test]
fn an_answered_question_leaves_the_view() {
    let mut b = board_with_a_question();
    let answered = TaskEvent::QuestionAnswered { session: "w1".into(), question_id: "q-1-1".into(), answer: None };
    b.task_mut("a").unwrap().apply(answered, 9).unwrap();
    let v = view(&b, "a");
    assert!(v.question.is_none());
    assert!(v.to_json(true).get("question").is_none());
    assert_eq!(v.to_json(true)["questions"][0]["answered_at"], 9);
    let list = b.view_with(None, false, &AgentGate::default()).to_list_json(None);
    assert!(list["tasks"][0].get("question").is_none());
}

fn blocked_by(b: &Board, id: &str) -> Vec<String> {
    view(b, id).blocked_by
}

#[test]
fn a_queued_task_is_blocked_by_failed_dependencies_direct_and_indirect() {
    let b = board(&[
        ("f", &[], Failed),
        ("g", &[], Failed),
        ("direct", &["f"], Queued),
        ("indirect", &["direct"], Queued),
        ("both", &["g", "indirect"], Queued),
    ]);
    assert_eq!(blocked_by(&b, "direct"), ["f"]);
    assert_eq!(blocked_by(&b, "indirect"), ["f"]);
    assert_eq!(blocked_by(&b, "both"), ["f", "g"], "in ordine di coda, senza doppioni");
    assert_eq!(view(&b, "direct").waiting_on, ["f"], "waiting_on non cambia");
}

#[test]
fn a_task_is_not_blocked_while_its_dependencies_are_only_queued_or_running() {
    let b = board(&[("q", &[], Queued), ("r", &[], Running), ("a", &["q"], Queued), ("b", &["r"], Queued)]);
    assert!(blocked_by(&b, "a").is_empty());
    assert!(blocked_by(&b, "b").is_empty());
    assert!(!compact_row(&view(&b, "a")).as_object().unwrap().contains_key("blocked_by"));
}

#[test]
fn a_done_task_breaks_the_chain_and_only_queued_tasks_are_blocked() {
    let b = board(&[("f", &[], Failed), ("ok", &["f"], Done), ("after", &["ok"], Queued), ("other", &["f"], Failed)]);
    assert!(blocked_by(&b, "after").is_empty(), "il done in mezzo e' soddisfatto");
    assert!(blocked_by(&b, "other").is_empty(), "un failed non e' bloccato, e' fallito");
    assert!(blocked_by(&b, "f").is_empty());
}

#[test]
fn list_rows_report_blocked_by_and_fail_reason_only_when_present() {
    let mut b = board(&[("f", &[], Failed), ("q", &["f"], Queued), ("u", &[], Failed)]);
    b.task_mut("f").unwrap().set_state(TaskState::Failed {
        finished: Finished { started_at: None, finished_at: 0, report: None },
        cause: FailCause::Other,
        reason: Some(FailReason::Stopped),
    });
    let blocked = compact_row(&view(&b, "q"));
    assert_eq!(blocked["blocked_by"], json!(["f"]));
    assert!(blocked.get("fail_reason").is_none());
    let failed = compact_row(&view(&b, "f"));
    assert_eq!(failed["fail_reason"], "stopped");
    assert!(failed.get("blocked_by").is_none());
    let unknown = compact_row(&view(&b, "u"));
    assert!(unknown.get("fail_reason").is_none(), "motivo ignoto: assente");
}
