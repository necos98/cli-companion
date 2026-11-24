//! Le board su disco con cartelle vere. L'elenco dei `JsonFile` e' unico per
//! processo: ogni test usa id di workspace tutti suoi.

use super::*;
use crate::board::{Finished, TaskSpec, TaskState, TaskStatus, INTERRUPTED_NOTE};
use serde_json::Value;

/// La board vera di oggi (senza busta) usata anche dai test dello schema.
const V0: &str = include_str!("../../board/fixtures/board_v0.json");

const NOW: i64 = 1_800_000_000_000;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cc-board-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn board_of(ws: &str) -> Board {
    let mut b = Board::new(ws);
    let spec = TaskSpec { id: "a".into(), title: "A".into(), task: "t".into(), ..TaskSpec::default() };
    b.tasks.push(Task::from_spec(spec, 1));
    b
}

fn done_task(id: &str) -> Task {
    let spec = TaskSpec { id: id.into(), title: id.into(), task: "t\nsu due righe".into(), ..TaskSpec::default() };
    let mut t = Task::from_spec(spec, 1);
    t.set_state(TaskState::Done(Finished { started_at: None, finished_at: 9, report: None }));
    t
}

#[test]
fn a_saved_board_is_in_the_envelope_and_loads_back() {
    let dir = temp_dir("roundtrip");
    let board = board_of("ws-bf-roundtrip");
    save(&dir, &board).unwrap();
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("ws-bf-roundtrip.json")).unwrap()).unwrap();
    assert_eq!(raw["schema_version"], 4);
    assert_eq!(raw["data"]["workspace_id"], "ws-bf-roundtrip");
    let loaded = load_all(&dir, NOW);
    assert_eq!(loaded, [board]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_in_the_format_of_today_is_migrated_and_its_running_task_recovered() {
    let dir = temp_dir("v0");
    std::fs::write(dir.join("ws-bf-v0.json"), V0.replace("\"ws-1\"", "\"ws-bf-v0\"")).unwrap();
    let loaded = load_all(&dir, NOW);
    assert_eq!(loaded.len(), 1);
    let t = loaded[0].task("task-1").unwrap();
    assert_eq!((t.status(), t.note(), t.finished_at()), (TaskStatus::Failed, Some(INTERRUPTED_NOTE), Some(NOW)));
    // Il file su disco resta com'e' finche' non si salva: nessuna riscrittura implicita.
    assert!(!std::fs::read_to_string(dir.join("ws-bf-v0.json")).unwrap().contains("schema_version"));
    // Il primo salvataggio lo porta alla busta.
    save(&dir, &loaded[0]).unwrap();
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("ws-bf-v0.json")).unwrap()).unwrap();
    assert_eq!(raw["schema_version"], 4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_file_is_set_aside_with_a_dated_backup_and_the_others_still_load() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.join("ws-bf-corrupt.json"), "{ non json").unwrap();
    save(&dir, &board_of("ws-bf-fine")).unwrap();
    let loaded = load_all(&dir, NOW);
    assert_eq!(loaded.iter().map(|b| b.workspace_id.as_str()).collect::<Vec<_>>(), ["ws-bf-fine"]);
    let files = names(&dir);
    let backup = files.iter().find(|n| n.starts_with("ws-bf-corrupt.json.bak-")).expect("backup datato");
    assert_eq!(std::fs::read_to_string(dir.join(backup)).unwrap(), "{ non json");
    assert!(
        !files.contains(&"ws-bf-corrupt.json".to_string()) && !files.contains(&"ws-bf-corrupt.json.bak".to_string())
    );
    // I backup non si ricaricano come board.
    assert_eq!(load_all(&dir, NOW).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_of_a_newer_version_is_set_aside_untouched() {
    let dir = temp_dir("newer");
    let original = r#"{"schema_version":42,"data":{"workspace_id":"ws-bf-newer"}}"#;
    std::fs::write(dir.join("ws-bf-newer.json"), original).unwrap();
    assert!(load_all(&dir, NOW).is_empty());
    let files = names(&dir);
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(std::fs::read_to_string(dir.join(&files[0])).unwrap(), original);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unreadable_file_stays_and_its_board_is_never_overwritten() {
    let dir = temp_dir("unreadable");
    // Una cartella al posto del file: la lettura fallisce (non "non trovato") ovunque.
    std::fs::create_dir_all(dir.join("ws-bf-blocked.json")).unwrap();
    assert!(load_all(&dir, NOW).is_empty());
    let err = save(&dir, &board_of("ws-bf-blocked")).unwrap_err();
    assert!(err.contains("ws-bf-blocked.json"), "{err}");
    assert!(dir.join("ws-bf-blocked.json").is_dir());
    // Altre board non ne risentono.
    save(&dir, &board_of("ws-bf-other")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleting_a_board_forgets_the_write_block_and_removes_the_archive_too() {
    let dir = temp_dir("delete");
    std::fs::create_dir_all(dir.join("ws-bf-gone.json")).unwrap();
    assert!(load_all(&dir, NOW).is_empty());
    assert!(save(&dir, &board_of("ws-bf-gone")).is_err());
    std::fs::remove_dir(dir.join("ws-bf-gone.json")).unwrap();
    save(&dir, &board_of("ws-bf-del")).unwrap();
    append_archive(&dir, "ws-bf-del", &[done_task("old")]).unwrap();
    delete(&dir, "ws-bf-del");
    delete(&dir, "ws-bf-gone");
    assert!(names(&dir).iter().all(|n| n != "ws-bf-del.json"));
    assert!(!dir.join("archive").join("ws-bf-del.jsonl").exists());
    save(&dir, &board_of("ws-bf-gone")).unwrap();
    delete(&dir, "../fuori");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn invalid_ids_and_stray_files_are_ignored() {
    let dir = temp_dir("stray");
    for name in ["spazio nel nome.json", "a.b.json", "notes.txt", "ws-bf-x.json.bak"] {
        std::fs::write(dir.join(name), r#"{"workspace_id":"ws-bf-stray"}"#).unwrap();
    }
    assert!(load_all(&dir, NOW).is_empty());
    assert!(load_all(&dir.join("non-esiste"), NOW).is_empty());
    assert!(save(&dir, &board_of("../fuori")).is_err());
    assert_eq!(names(&dir).len(), 4, "nulla e' stato toccato");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn archive_lines_are_appended_one_per_task() {
    let dir = temp_dir("archive");
    append_archive(&dir, "ws-bf-arch", &[done_task("a"), done_task("b")]).unwrap();
    append_archive(&dir, "ws-bf-arch", &[done_task("c")]).unwrap();
    let text = std::fs::read_to_string(dir.join("archive").join("ws-bf-arch.jsonl")).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let ids: Vec<&str> = lines.iter().map(|l| l["data"]["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert!(lines.iter().all(|l| l["schema_version"] == 1 && l["data"]["status"] == "done"));
    // La cartella dell'archivio non e' una board.
    assert!(load_all(&dir, NOW).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_archive_refuses_an_invalid_workspace_id() {
    let dir = temp_dir("archive-bad");
    assert!(append_archive(&dir, "../fuori", &[done_task("a")]).is_err());
    assert!(names(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
