//! Il formato sul filo non cambia: ogni stato si scrive come lo scriveva il
//! vecchio `Task` piatto (stessi campi, stesso ordine, stessi byte), si rilegge
//! uguale, e le combinazioni impossibili dei file si normalizzano.

use super::*;
use crate::board::{build_report, AgentGate, ReportFields, TaskSpec};
use serde_json::{json, Value};

/// Il vecchio `Task` piatto, com'era prima della macchina a stati (stessi
/// campi e stessi attributi serde): il riferimento per i byte.
#[derive(Debug, Default, Serialize)]
struct OldTask {
    id: String,
    title: String,
    task: String,
    depends_on: Vec<String>,
    repo: Option<String>,
    agent: String,
    model: Option<String>,
    kind: TaskKind,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    e2e: bool,
    status: TaskStatus,
    worker_status: Option<WorkerStatus>,
    session: Option<String>,
    attempts: u32,
    worktree: Option<WorktreeInfo>,
    last_report: Option<String>,
    note: Option<String>,
    report: Option<TaskReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    e2e_run: Option<E2eRun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_attempt: Option<PreviousAttempt>,
    created_at: i64,
    updated_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preparing: Option<String>,
}

fn report() -> TaskReport {
    build_report("completed", "all done", ReportFields::default(), Some(" 1 file changed".into()))
}

fn previous() -> PreviousAttempt {
    PreviousAttempt { summary: Some("red".into()), note: Some("failed".into()) }
}

fn worktree() -> WorktreeInfo {
    WorktreeInfo { path: "p".into(), branch: "task/a".into(), base: "main".into(), setup_done: false }
}

fn finished() -> Finished {
    Finished { started_at: Some(20), finished_at: 30, report: Some(report()) }
}

/// Un task in `state` e il suo gemello nel vecchio formato piatto.
fn pair(state: TaskState, old: OldTask) -> (Task, OldTask) {
    let spec = TaskSpec { id: "a".into(), title: "T".into(), task: "x".into(), e2e: true, ..TaskSpec::default() };
    let mut t = Task::from_spec(spec, 1);
    t.set_state(state);
    t.set_attempts(old.attempts);
    t.set_note(old.note.as_deref());
    t.set_last_report(old.last_report.as_deref());
    t.worktree = Some(worktree());
    t.updated_at = 2;
    let old = OldTask {
        id: "a".into(),
        title: "T".into(),
        task: "x".into(),
        agent: t.agent.clone(),
        e2e: true,
        worktree: Some(worktree()),
        created_at: 1,
        updated_at: 2,
        ..old
    };
    (t, old)
}

/// Un caso per ogni forma di stato.
fn cases() -> Vec<(&'static str, Task, OldTask)> {
    let live = |worker| RunPhase::Live { session: "w1".into(), worker };
    let running = |phase| TaskState::Running(Run { started_at: 20, previous_attempt: Some(previous()), phase });
    let base = || OldTask { attempts: 1, ..OldTask::default() };
    let mut cases = Vec::new();
    let mut add = |name, state, old| {
        let (t, o) = pair(state, old);
        cases.push((name, t, o));
    };
    add(
        "queued",
        TaskState::Queued { previous_attempt: Some(previous()) },
        OldTask { previous_attempt: Some(previous()), attempts: 1, note: Some("n".into()), ..OldTask::default() },
    );
    add("queued, first time", TaskState::default(), OldTask::default());
    add(
        "preparing",
        TaskState::Running(Run {
            started_at: 20,
            previous_attempt: None,
            phase: RunPhase::Preparing { step: Some("setup 1/1: npm ci".into()), heartbeat: 25 },
        }),
        OldTask {
            status: TaskStatus::Running,
            worker_status: Some(WorkerStatus::Starting),
            started_at: Some(20),
            preparing: Some("setup 1/1: npm ci".into()),
            ..base()
        },
    );
    add(
        "preparing, no step",
        running(RunPhase::Preparing { step: None, heartbeat: 20 }),
        OldTask {
            status: TaskStatus::Running,
            worker_status: Some(WorkerStatus::Starting),
            started_at: Some(20),
            previous_attempt: Some(previous()),
            ..base()
        },
    );
    for (activity, status) in [
        (WorkerActivity::Starting, WorkerStatus::Starting),
        (WorkerActivity::Running, WorkerStatus::Running),
        (WorkerActivity::Background, WorkerStatus::Background),
        (WorkerActivity::NeedsInput, WorkerStatus::NeedsInput),
        (WorkerActivity::Idle, WorkerStatus::Idle),
        (WorkerActivity::AwaitingAnswer, WorkerStatus::AwaitingAnswer),
    ] {
        add(
            "live",
            running(live(activity)),
            OldTask {
                status: TaskStatus::Running,
                worker_status: Some(status),
                session: Some("w1".into()),
                started_at: Some(20),
                previous_attempt: Some(previous()),
                ..base()
            },
        );
    }
    add(
        "done",
        TaskState::Done(finished()),
        OldTask {
            status: TaskStatus::Done,
            report: Some(report()),
            started_at: Some(20),
            finished_at: Some(30),
            last_report: Some("all done".into()),
            ..base()
        },
    );
    for cause in [FailCause::Other, FailCause::SessionEnded { session: "w1".into() }] {
        add(
            "failed",
            TaskState::Failed { finished: finished(), cause, reason: None },
            OldTask {
                status: TaskStatus::Failed,
                report: Some(report()),
                started_at: Some(20),
                finished_at: Some(30),
                note: Some("boom".into()),
                ..base()
            },
        );
    }
    cases
}

#[test]
fn every_state_serializes_byte_for_byte_like_the_old_flat_task() {
    for (name, task, old) in cases() {
        assert_eq!(serde_json::to_string(&task).unwrap(), serde_json::to_string(&old).unwrap(), "{name}");
    }
}

#[test]
fn every_state_reads_back_equal_except_the_failure_cause() {
    for (name, task, _) in cases() {
        let back: Task = serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
        let mut expected = task.clone();
        // La causa non va su file (il motivo si): un fallito riletto e' `Other`.
        if let TaskState::Failed { finished, reason, .. } = task.state() {
            expected.set_state(TaskState::Failed {
                finished: finished.clone(),
                cause: FailCause::Other,
                reason: *reason,
            });
        }
        // La fase di preparazione riparte dall'inizio: niente passo, battito = avvio.
        if let TaskState::Running(run @ Run { phase: RunPhase::Preparing { .. }, .. }) = task.state() {
            let phase = RunPhase::Preparing { step: None, heartbeat: run.started_at };
            expected.set_state(TaskState::Running(Run { phase, ..run.clone() }));
        }
        assert_eq!(back, expected, "{name}");
    }
}

#[test]
fn the_status_names_and_the_preparing_step_are_the_old_ones() {
    let by = |status| cases().into_iter().find(|(_, t, _)| t.status() == status).map(|(_, t, _)| t).unwrap();
    let v = |t: &Task| serde_json::to_value(t).unwrap();
    assert_eq!(v(&by(TaskStatus::Queued))["status"], "queued");
    let running = v(&by(TaskStatus::Running));
    assert_eq!((&running["status"], &running["worker_status"]), (&json!("running"), &json!("starting")));
    assert_eq!(running["preparing"], "setup 1/1: npm ci");
    assert_eq!(v(&by(TaskStatus::Done))["status"], "done");
    assert_eq!(v(&by(TaskStatus::Failed))["status"], "failed");
}

fn read(extra: Value) -> Task {
    let mut v = json!({"id": "a", "title": "T", "task": "x", "created_at": 1, "updated_at": 7});
    v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_queued_record_drops_what_belongs_to_other_states() {
    let t = read(json!({"status": "queued", "session": "w1", "worker_status": "running", "report": report(),
        "started_at": 3, "finished_at": 4, "preparing": "x"}));
    assert_eq!(t.state(), &TaskState::Queued { previous_attempt: None });
    assert_eq!(
        (t.session(), t.worker_status(), t.report(), t.started_at(), t.finished_at()),
        (None, None, None, None, None)
    );
}

#[test]
fn a_finished_record_drops_the_session_and_the_worker() {
    for status in ["done", "failed"] {
        let t = read(
            json!({"status": status, "session": "w1", "worker_status": "idle", "previous_attempt": {"summary": "s"},
            "report": report(), "started_at": 3, "finished_at": 4}),
        );
        assert_eq!(t.state().status().as_str(), status);
        assert_eq!((t.session(), t.worker_status(), t.previous_attempt()), (None, None, None), "{status}");
        assert_eq!((t.started_at(), t.finished_at()), (Some(3), Some(4)));
        assert!(t.report().is_some());
    }
}

#[test]
fn a_finished_record_without_a_finish_time_takes_the_update_time() {
    for status in ["done", "failed"] {
        let t = read(json!({"status": status}));
        assert_eq!((t.finished_at(), t.started_at()), (Some(7), None), "{status}");
    }
}

#[test]
fn a_running_record_is_live_with_a_session_and_preparing_without() {
    let preparing = read(json!({"status": "running", "worker_status": "idle", "started_at": 3}));
    assert_eq!(preparing.preparing(), Some((None, 3)), "senza sessione non c'e' nessun worker");
    assert_eq!((preparing.session(), preparing.worker_status()), (None, Some(WorkerStatus::Starting)));

    let live = read(json!({"status": "running", "session": "w1", "worker_status": "needs_input", "started_at": 3}));
    assert_eq!(
        (live.session(), live.worker_status(), live.started_at()),
        (Some("w1"), Some(WorkerStatus::NeedsInput), Some(3))
    );
    assert!(live.needs_attention());

    // Uno stato del worker finale o assente, con la sessione, vale `running`.
    for worker in [json!("completed"), json!("closed"), Value::Null] {
        let t = read(json!({"status": "running", "session": "w1", "worker_status": worker}));
        assert_eq!(t.worker_status(), Some(WorkerStatus::Running));
    }
    // Senza inizio vale l'ultimo aggiornamento.
    assert_eq!(read(json!({"status": "running"})).started_at(), Some(7));
}

#[test]
fn an_empty_or_partial_record_is_a_queued_task() {
    let t: Task = serde_json::from_value(json!({"id": "a"})).unwrap();
    assert_eq!((t.status(), t.attempts()), (TaskStatus::Queued, 0));
    let extra: Task = serde_json::from_value(json!({"id": "a", "unknown_field": 1, "held": true})).unwrap();
    assert_eq!(extra.id, "a");
}

fn question(id: &str, answered: bool) -> TaskQuestion {
    TaskQuestion {
        id: id.into(),
        attempt: 1,
        text: "Which db?".into(),
        options: vec!["Postgres".into(), "SQLite".into()],
        asked_at: 5,
        answered_at: answered.then_some(6),
        answer: answered.then(|| "Postgres".to_string()),
    }
}

#[test]
fn a_file_without_questions_stays_valid_and_writes_none() {
    let t = read(json!({"status": "queued"}));
    assert!(t.questions().is_empty());
    assert!(serde_json::to_value(&t).unwrap().get("questions").is_none());
}

#[test]
fn questions_are_written_after_the_e2e_run_and_read_back() {
    let mut t = read(json!({"status": "queued"}));
    t.questions = vec![question("q-1-1", true), question("q-1-2", false)];
    let wire = serde_json::to_value(&t).unwrap();
    assert_eq!(wire["questions"][0]["answer"], "Postgres");
    assert!(wire["questions"][1].get("answered_at").is_none() && wire["questions"][1].get("answer").is_none());
    let text = serde_json::to_string(&t).unwrap();
    assert!(text.find("\"questions\"").unwrap() > text.find("\"attempts\"").unwrap());
    assert!(text.find("\"questions\"").unwrap() < text.find("\"created_at\"").unwrap());
    let back: Task = serde_json::from_str(&text).unwrap();
    assert_eq!(back.questions(), t.questions());
    // Una domanda senza opzioni non scrive il campo.
    let mut bare = question("q-1-3", false);
    bare.options.clear();
    assert!(serde_json::to_value(&bare).unwrap().get("options").is_none());
    let read_back: TaskQuestion = serde_json::from_value(serde_json::to_value(&bare).unwrap()).unwrap();
    assert_eq!(read_back, bare);
}

#[test]
fn the_view_json_is_the_task_json_plus_the_new_fields() {
    use crate::board::Board;
    const ADDED: [&str; 4] = ["allowed_actions", "can_edit", "can_reorder", "cascade"];
    for (name, task, _) in cases() {
        let mut b = Board::new("w");
        b.tasks.push(task.clone());
        let view = serde_json::to_value(&b.view_with(None, false, &AgentGate::default()).tasks[0]).unwrap();
        let mut wire = serde_json::to_value(&task).unwrap();
        wire["waiting_on"] = json!([]);
        wire["blocked_by"] = json!([]);
        wire["attention"] = json!(task.needs_attention());
        for added in ADDED {
            assert!(view.get(added).is_some(), "{name}: {added}");
        }
        let mut old_part = view.clone();
        for added in ADDED {
            old_part.as_object_mut().unwrap().remove(added);
        }
        assert_eq!(old_part, wire, "{name}");
    }
}

#[test]
fn a_record_saved_before_the_bots_loads_without_them_and_writes_the_same_fields() {
    let old =
        json!({"id": "a", "title": "T", "task": "x", "agent": "claude-code", "model": "opus", "status": "queued"});
    let t: Task = serde_json::from_value(old).unwrap();
    assert_eq!((&t.bot, &t.effort, &t.skill), (&None, &None, &None));
    let wire = serde_json::to_value(&t).unwrap();
    for field in ["bot", "effort", "skill"] {
        assert!(wire.get(field).is_none(), "{field}");
    }
}

#[test]
fn the_copy_of_the_bot_is_written_after_the_model_and_read_back() {
    let record = json!({"id": "a", "title": "T", "task": "x", "bot": "intake", "effort": "high",
        "skill": {"kind": "plugin", "id": "task-intake"}});
    let t = read(record);
    assert_eq!(t.bot.as_deref(), Some("intake"));
    assert_eq!(t.effort, Some(crate::agents::Effort::High));
    assert_eq!(t.skill, Some(crate::bots::SkillRef::Plugin("task-intake".into())));
    let text = serde_json::to_string(&t).unwrap();
    assert!(text.find("\"model\"").unwrap() < text.find("\"bot\"").unwrap());
    assert!(text.find("\"skill\"").unwrap() < text.find("\"kind\"").unwrap());
    let back: Task = serde_json::from_str(&text).unwrap();
    assert_eq!(back, t);
    assert_eq!(TaskSpec::from_task(&back).skill, t.skill);
}

#[test]
fn the_fail_reason_is_written_for_failed_tasks_and_read_back() {
    let spec = TaskSpec { id: "a".into(), title: "T".into(), task: "x".into(), ..TaskSpec::default() };
    let mut t = Task::from_spec(spec, 1);
    let state =
        TaskState::Failed { finished: finished(), cause: FailCause::Other, reason: Some(FailReason::StartFailed) };
    t.set_state(state);
    let json: Value = serde_json::to_value(&t).unwrap();
    assert_eq!(json["fail_reason"], "start_failed");
    let back: Task = serde_json::from_value(json).unwrap();
    assert_eq!(back.fail_reason(), Some(FailReason::StartFailed));
}

#[test]
fn a_failed_record_without_a_reason_is_unknown_and_other_states_ignore_it() {
    assert_eq!(read(json!({"status": "failed"})).fail_reason(), None);
    assert_eq!(read(json!({"status": "failed", "fail_reason": "stopped"})).fail_reason(), Some(FailReason::Stopped));
    assert_eq!(read(json!({"status": "queued", "fail_reason": "stopped"})).fail_reason(), None);
    assert!(serde_json::to_value(read(json!({"status": "failed"}))).unwrap().get("fail_reason").is_none());
}

#[test]
fn the_origin_review_is_written_only_when_set_and_read_back() {
    let plain = Task::from_spec(TaskSpec { id: "a".into(), ..TaskSpec::default() }, 1);
    assert!(serde_json::to_value(&plain).unwrap().get("origin").is_none());
    let fix = Task { origin: Some("rev".into()), ..plain.clone() };
    let wire = serde_json::to_value(&fix).unwrap();
    assert_eq!(wire["origin"], "rev");
    let back: Task = serde_json::from_value(wire).unwrap();
    assert_eq!(back.origin.as_deref(), Some("rev"));
    // Un file scritto prima del campo si legge come task senza origine.
    let old: Task = serde_json::from_value(serde_json::to_value(&plain).unwrap()).unwrap();
    assert_eq!(old.origin, None);
}
