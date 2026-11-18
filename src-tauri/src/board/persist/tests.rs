use super::*;
use crate::board::{
    FailCause, Finished, TaskSpec, TaskState, TaskStatus, DEFAULT_MAX_PARALLEL, INTERRUPTED_NOTE, MAX_PARALLEL_LIMIT,
};
use serde_json::json;
use std::path::PathBuf;

/// Un file scritto dall'app di oggi (senza busta), preso da una board vera.
const V0: &str = include_str!("../fixtures/board_v0.json");
/// Un file della v0.11, con stati e campi che non esistono piu'.
const V011: &str = include_str!("../fixtures/board_v011.json");

const NOW: i64 = 1_800_000_000_000;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cc-board-persist-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_current_schema_version_is_three() {
    assert_eq!(board_store("ws-1.json".into()).schema_version(), 3);
    assert_eq!(BOARD_FORMAT.schema_version(), 3);
}

// --- il file reale di oggi (versione 0) ---

#[test]
fn a_real_board_file_loads_field_by_field() {
    let b = Board::from_json(V0, NOW).unwrap();
    assert_eq!(
        (b.workspace_id.as_str(), b.max_parallel, b.paused, b.updated_at),
        ("ws-1", 3, false, 1_793_784_893_966)
    );
    assert!(b.proposals.is_empty() && b.settings_proposals.is_empty());
    let ids: Vec<&str> = b.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["task-1", "task-2", "task-3", "task-4", "task-5", "task-6"]);
    let status: Vec<TaskStatus> = b.tasks.iter().map(|t| t.status()).collect();
    use TaskStatus::{Done, Failed, Queued};
    assert_eq!(status, [Failed, Queued, Queued, Done, Done, Failed]);
}

#[test]
fn the_running_task_of_a_real_file_is_recovered_as_interrupted() {
    let b = Board::from_json(V0, NOW).unwrap();
    let t = b.task("task-1").unwrap();
    assert_eq!(t.note(), Some(INTERRUPTED_NOTE));
    assert_eq!((t.started_at(), t.finished_at(), t.updated_at), (Some(1_793_784_862_400), Some(NOW), NOW));
    assert_eq!((t.session(), t.worker_status(), t.report().is_none()), (None, None, true));
    assert_eq!((t.attempts(), t.model.as_deref(), t.agent.as_str()), (1, Some("opus"), "claude-code"));
    // Il worktree resta: serve a ripulirlo o a riprovare.
    let w = t.worktree.as_ref().unwrap();
    assert_eq!(
        (w.path.as_str(), w.branch.as_str(), w.base.as_str(), w.setup_done),
        ("C:p.worktrees\\task-1", "task/task-1", "main", true)
    );
    assert!(matches!(t.state(), TaskState::Failed { cause: FailCause::Other, .. }));
}

#[test]
fn queued_tasks_of_a_real_file_keep_their_dependencies() {
    let b = Board::from_json(V0, NOW).unwrap();
    let (t2, t3) = (b.task("task-2").unwrap(), b.task("task-3").unwrap());
    assert_eq!(
        (t2.depends_on.as_slice(), t3.depends_on.as_slice()),
        (&["task-1".to_string()][..], &["task-1".to_string()][..])
    );
    assert_eq!((t2.attempts(), t2.started_at(), t2.finished_at(), t2.worktree.is_none()), (0, None, None, true));
    assert_eq!((t2.model.as_deref(), t3.model.as_deref()), (Some("opus"), Some("sonnet")));
    assert_eq!((t2.created_at, t2.updated_at), (1_793_743_267_309, 1_793_743_267_309));
}

#[test]
fn finished_tasks_of_a_real_file_keep_report_and_worktree() {
    let b = Board::from_json(V0, NOW).unwrap();
    let t4 = b.task("task-4").unwrap();
    assert_eq!(
        (t4.attempts(), t4.started_at(), t4.finished_at()),
        (2, Some(1_792_402_017_336), Some(1_792_404_253_083))
    );
    assert_eq!((t4.last_report(), t4.note()), (Some("Last report of task-4"), Some("Note of task-4")));
    let w = t4.worktree.as_ref().unwrap();
    assert_eq!((w.branch.as_str(), w.base.as_str()), ("task/task-4", "master"));
    let r = t4.report().unwrap();
    assert_eq!(
        (r.status.as_str(), r.summary.as_str(), r.commit.as_deref()),
        ("completed", "Summary of task-4", Some("abc1234"))
    );
    assert_eq!(
        (r.files_changed.as_slice(), r.follow_ups.as_slice()),
        (&["src/a.rs".to_string()][..], &["follow up".to_string()][..])
    );
    assert_eq!((r.tests[0].command.as_str(), r.tests[0].result.as_str()), ("cargo test", "passed"));
    assert!(r.diffstat.as_deref().is_some_and(|d| d.contains("1 file changed")));
    let t5 = b.task("task-5").unwrap();
    assert_eq!(
        (t5.status(), t5.worktree.is_none(), t5.report().unwrap().diffstat.is_none()),
        (TaskStatus::Done, true, true)
    );
    let t6 = b.task("task-6").unwrap();
    assert_eq!(
        (t6.finished_at(), t6.note(), t6.report().unwrap().status.as_str()),
        (Some(1_793_561_417_659), None, "failed")
    );
}

#[test]
fn a_real_file_is_saved_inside_the_current_envelope_and_reloads_identically() {
    let board = Board::from_json(V0, NOW).unwrap();
    let dir = temp_dir("v0-envelope");
    let store = board_store("ws-1.json".into());
    store.save(&dir, &board).unwrap();
    let saved = std::fs::read_to_string(dir.join("ws-1.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(on_disk["schema_version"], 3);
    assert_eq!(on_disk["data"], serde_json::to_value(&board).unwrap());
    // La forma salvata si rilegge uguale, dal file e dal testo.
    let loaded = store.load(&dir);
    assert!(loaded.warning.is_none());
    assert_eq!(loaded.value, board);
    let reparsed = Board::from_json(&saved, NOW + 1).unwrap();
    assert_eq!(reparsed, board);
    assert_eq!(serde_json::to_value(&reparsed).unwrap(), on_disk["data"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_saved_form_keeps_the_wire_shape_of_the_old_file() {
    let board = Board::from_json(V0, NOW).unwrap();
    let out = serde_json::to_value(&board).unwrap();
    let original: Value = serde_json::from_str(V0).unwrap();
    // Task a riposo: identici al file originale, campo per campo.
    for id in ["task-2", "task-3", "task-4", "task-5", "task-6"] {
        let find = |v: &Value| v["tasks"].as_array().unwrap().iter().find(|t| t["id"] == id).cloned().unwrap();
        let (new, old) = (find(&out), find(&original));
        assert_eq!(new, old, "{id}");
    }
}

// --- v0.11 ---

#[test]
fn a_v011_file_loads_field_by_field() {
    let b = Board::from_json(V011, 99).unwrap();
    assert_eq!(b.max_parallel, MAX_PARALLEL_LIMIT);
    let ids: Vec<&str> = b.tasks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["queued", "running", "review", "blocked", "failed", "done"]);
    let st = |id: &str| b.task(id).unwrap().status();
    use TaskStatus::{Done, Failed, Queued};
    assert_eq!(
        ["queued", "running", "review", "blocked", "failed", "done"].map(st),
        [Queued, Failed, Failed, Failed, Failed, Done]
    );
    let running = b.task("running").unwrap();
    assert_eq!((running.note(), running.finished_at(), running.session()), (Some(INTERRUPTED_NOTE), Some(99), None));
    assert_eq!(running.depends_on, ["queued"]);
    let review = b.task("review").unwrap();
    assert!(review.note().is_some_and(|n| n.starts_with("completed but not integrated")));
    assert_eq!(review.report().unwrap().summary, "done work");
    assert_eq!(b.task("blocked").unwrap().report().unwrap().status, "failed");
    assert_eq!(b.task("done").unwrap().depends_on, ["queued"]);
    assert!(b.task("cancelled").is_none());
    assert!(b.tasks.iter().all(|t| t.agent == "claude-code"), "agente predefinito");
}

#[test]
fn a_v011_file_saved_in_the_envelope_has_no_removed_fields_and_is_stable() {
    let board = Board::from_json(V011, 99).unwrap();
    let dir = temp_dir("v011-envelope");
    let store = board_store("ws-1.json".into());
    store.save(&dir, &board).unwrap();
    let saved = std::fs::read_to_string(dir.join("ws-1.json")).unwrap();
    let on_disk: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(on_disk["schema_version"], 3);
    for gone in [
        "owner",
        "requests",
        "integrations",
        "base_branch",
        "touches",
        "orchestrator_note",
        "held",
        "priority",
        "isolation",
    ] {
        assert!(!saved.contains(&format!("\"{gone}\"")), "{gone} in {saved}");
    }
    // Rileggere il file salvato non cambia nulla (niente piu' migrazione ne' recupero).
    assert_eq!(store.load(&dir).value, board);
    assert_eq!(Board::from_json(&saved, 12_345).unwrap(), board);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- busta, versioni, invarianti ---

#[test]
fn a_file_of_a_newer_schema_is_set_aside_not_overwritten() {
    let dir = temp_dir("newer");
    let original = json!({"schema_version": 7, "data": {"workspace_id": "ws-1"}}).to_string();
    std::fs::write(dir.join("ws-1.json"), &original).unwrap();
    let store = board_store("ws-1.json".into());
    let loaded = store.load(&dir);
    assert!(loaded.value.workspace_id.is_empty(), "board vuota: il chiamante la scarta");
    let backup = loaded.warning.unwrap().backup.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(backup)).unwrap(), original);
    assert!(Board::from_json(&original, 1).unwrap_err().contains("schema 7"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_board_without_a_workspace_id_is_rejected() {
    assert!(Board::from_json("{\"tasks\":[]}", 1).unwrap_err().contains("workspace_id"));
    let enveloped = json!({"schema_version": 1, "data": {"workspace_id": ""}}).to_string();
    assert!(Board::from_json(&enveloped, 1).is_err());
    assert!(Board::from_json("zzz", 1).is_err());
}

#[test]
fn values_are_sanitized_on_every_read() {
    let enveloped = json!({
        "schema_version": 1,
        "data": {"workspace_id": "w", "max_parallel": 0, "tasks": [{"id": "a", "title": "A", "task": "t", "agent": ""}]}
    })
    .to_string();
    for raw in [enveloped, json!({"workspace_id": "w", "max_parallel": 99}).to_string()] {
        let b = Board::from_json(&raw, 1).unwrap();
        assert!((1..=MAX_PARALLEL_LIMIT).contains(&b.max_parallel), "{}", b.max_parallel);
        assert!(b.tasks.iter().all(|t| !t.agent.is_empty()));
    }
    assert_eq!(Board::from_json(r#"{"workspace_id":"w"}"#, 1).unwrap().max_parallel, DEFAULT_MAX_PARALLEL);
}

#[test]
fn a_current_envelope_is_recovered_too() {
    let enveloped = json!({
        "schema_version": 1,
        "data": {"workspace_id": "w", "tasks": [{"id": "a", "title": "A", "task": "t", "status": "running"}]}
    })
    .to_string();
    let b = Board::from_json(&enveloped, NOW).unwrap();
    assert_eq!(b.task("a").unwrap().status(), TaskStatus::Failed);
}

// --- archivio ---

#[test]
fn an_archive_line_is_one_enveloped_task_record() {
    let spec = TaskSpec { id: "a".into(), title: "A\nB".into(), task: "t\n\"q\"".into(), ..TaskSpec::default() };
    let mut task = Task::from_spec(spec, 5);
    task.set_state(TaskState::Done(Finished { started_at: Some(6), finished_at: 7, report: None }));
    let line = archive_line(&task).unwrap();
    assert!(!line.contains('\n'), "{line}");
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["data"], serde_json::to_value(&task).unwrap());
    assert_eq!(v["data"]["status"], "done");
    let back: Task = serde_json::from_value(v["data"].clone()).unwrap();
    assert_eq!(back, task);
}

// --- i file di prima dei Bot ---

#[test]
fn the_tasks_of_files_written_before_the_bots_have_no_bot_effort_or_skill_and_write_none() {
    for (name, file) in [("v0", V0), ("v011", V011)] {
        let board = Board::from_json(file, NOW).unwrap();
        assert!(!board.tasks.is_empty(), "{name}");
        for t in &board.tasks {
            assert_eq!((&t.bot, &t.effort, &t.skill), (&None, &None, &None), "{name}/{}", t.id);
            let wire = serde_json::to_value(t).unwrap();
            for field in ["bot", "effort", "skill"] {
                assert!(wire.get(field).is_none(), "{name}/{}: {field}", t.id);
            }
        }
    }
}
