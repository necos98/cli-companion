//! Tool MCP delle skill globali: salvataggio, elenco, cancellazione, ruoli.

use super::*;
use crate::user_skills::{MANAGED_MARKER, SKILL_FILE};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Sessione di un tab senza workspace (nessun pty vivo).
const NO_WORKSPACE_TAB: &str = "tab-nowhere";

/// Cartella temporanea unica per test, rimossa a fine test.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mc-rpc-skills-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn store(&self) -> PathBuf {
        self.0.join("store")
    }

    fn target(&self) -> PathBuf {
        self.0.join("target")
    }

    fn host(&self) -> FakeHost {
        let h = FakeHost::default();
        *h.skills_store.lock().unwrap() = Some(self.store());
        h.skill_dirs.lock().unwrap().push(("windows".to_string(), Ok(self.target())));
        h
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn skill(name: &str) -> Value {
    json!({"name": name, "description": "Use when reviewing notes", "body": "# Steps\n\nRead them."})
}

fn save(st: &AutomationState, h: &FakeHost, params: Value) -> Result<Value, String> {
    call(st, h, INTAKE, "save_skill", params)
}

#[test]
fn save_writes_the_store_and_syncs_the_target() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    let mut params = skill("release-notes");
    params["files"] = json!([{"path": "refs/style.md", "content": "Be brief."}]);
    let r = save(&st, &h, params).unwrap();
    assert_eq!(r["name"], "release-notes");
    assert_eq!(r["invocation"], "/release-notes");
    assert_eq!(r["store_path"], sb.store().join("release-notes").display().to_string());
    assert_eq!(r["synced"], json!([{"env": "windows", "ok": true}]));
    let skill_md = fs::read_to_string(sb.target().join("release-notes").join(SKILL_FILE)).unwrap();
    assert!(skill_md.starts_with("---\nname: release-notes\n") && skill_md.contains("# Steps"));
    assert!(sb.target().join("release-notes").join(MANAGED_MARKER).is_file());
    assert_eq!(fs::read_to_string(sb.target().join("release-notes/refs/style.md")).unwrap(), "Be brief.");
}

#[test]
fn save_updates_a_managed_skill() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    let mut first = skill("notes");
    first["files"] = json!([{"path": "old.md", "content": "x"}]);
    save(&st, &h, first).unwrap();
    let mut second = skill("notes");
    second["body"] = json!("New body.");
    let r = save(&st, &h, second).unwrap();
    assert_eq!(r["synced"][0]["ok"], true);
    let skill_md = fs::read_to_string(sb.target().join("notes").join(SKILL_FILE)).unwrap();
    assert!(skill_md.contains("New body.") && !skill_md.contains("# Steps"));
    assert!(!sb.target().join("notes/old.md").exists());
    assert_eq!(call(&st, &h, INTAKE, "list_skills", json!({})).unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn save_refuses_a_hand_written_skill_without_touching_anything() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    let theirs = sb.target().join("mine");
    fs::create_dir_all(&theirs).unwrap();
    fs::write(theirs.join(SKILL_FILE), "hand written").unwrap();
    let err = save(&st, &h, skill("mine")).unwrap_err();
    assert!(err.contains("not created by cli-companion"), "{err}");
    assert_eq!(fs::read_to_string(theirs.join(SKILL_FILE)).unwrap(), "hand written");
    assert!(!sb.store().join("mine").exists(), "nothing reaches the store either");
}

#[test]
fn save_reports_an_unreachable_target_but_keeps_the_store() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    h.skill_dirs.lock().unwrap().push(("wsl:Ubuntu".to_string(), Err("distro is stopped".to_string())));
    let r = save(&st, &h, skill("notes")).unwrap();
    assert_eq!(
        r["synced"],
        json!([{"env": "windows", "ok": true}, {"env": "wsl:Ubuntu", "ok": false, "error": "distro is stopped"}])
    );
    assert!(sb.store().join("notes").join(SKILL_FILE).is_file());
}

#[test]
fn validation_and_parameter_errors_are_rpc_errors() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    let mut bad_name = skill("Bad Name");
    bad_name["name"] = json!("Bad Name");
    assert!(save(&st, &h, bad_name).unwrap_err().contains("Invalid skill name"));
    let mut reserved = skill("x");
    reserved["name"] = json!("skill-wizard");
    assert!(save(&st, &h, reserved).unwrap_err().contains("reserved"));
    let mut empty_body = skill("x");
    empty_body["body"] = json!("  ");
    assert!(save(&st, &h, empty_body).unwrap_err().contains("body must not be empty"));
    let mut escaping = skill("x");
    escaping["files"] = json!([{"path": "../evil.md", "content": "x"}]);
    assert!(save(&st, &h, escaping).unwrap_err().contains("Invalid file path"));
    assert!(save(&st, &h, json!({"name": "x"})).unwrap_err().contains("invalid parameters"));
    assert!(save(&st, &h, json!({"name": 3, "description": "d", "body": "b"})).unwrap_err().contains("invalid parameters"));
    assert!(call(&st, &h, INTAKE, "delete_skill", json!({})).unwrap_err().contains("name is missing"));
    assert!(!sb.store().exists(), "invalid input writes nothing");
}

#[test]
fn list_returns_saved_skills_sorted() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    assert_eq!(call(&st, &h, INTAKE, "list_skills", json!({})).unwrap(), json!([]));
    save(&st, &h, skill("zeta")).unwrap();
    save(&st, &h, skill("alpha")).unwrap();
    let r = call(&st, &h, INTAKE, "list_skills", Value::Null).unwrap();
    assert_eq!(
        r,
        json!([
            {"name": "alpha", "description": "Use when reviewing notes"},
            {"name": "zeta", "description": "Use when reviewing notes"}
        ])
    );
}

#[test]
fn delete_removes_store_and_target_but_never_an_unmanaged_skill() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    save(&st, &h, skill("notes")).unwrap();
    let r = call(&st, &h, INTAKE, "delete_skill", json!({"name": "notes"})).unwrap();
    assert_eq!(r, json!({"name": "notes", "removed": [{"env": "windows", "ok": true}]}));
    assert!(!sb.store().join("notes").exists() && !sb.target().join("notes").exists());
    assert!(call(&st, &h, INTAKE, "delete_skill", json!({"name": "notes"})).unwrap_err().contains("not found"));

    // Una skill scritta a mano con lo stesso nome di una gestita: resta dov'e'.
    save(&st, &h, skill("kept")).unwrap();
    fs::remove_file(sb.target().join("kept").join(MANAGED_MARKER)).unwrap();
    let r = call(&st, &h, INTAKE, "delete_skill", json!({"name": "kept"})).unwrap();
    assert_eq!(r["removed"][0]["ok"], false);
    assert!(sb.target().join("kept").join(SKILL_FILE).is_file());
}

#[test]
fn delete_reports_each_environment_and_cleans_the_reachable_ones() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    let second = sb.0.join("second");
    h.skill_dirs.lock().unwrap().push(("wsl:Debian".to_string(), Err("distro is stopped".to_string())));
    h.skill_dirs.lock().unwrap().push(("wsl:Ubuntu".to_string(), Ok(second.clone())));
    save(&st, &h, skill("notes")).unwrap();
    assert!(second.join("notes").join(SKILL_FILE).is_file());

    let r = call(&st, &h, INTAKE, "delete_skill", json!({"name": "notes"})).unwrap();
    assert_eq!(
        r["removed"],
        json!([
            {"env": "windows", "ok": true},
            {"env": "wsl:Debian", "ok": false, "error": "distro is stopped"},
            {"env": "wsl:Ubuntu", "ok": true},
        ])
    );
    assert!(!sb.target().join("notes").exists() && !second.join("notes").exists());
}

#[test]
fn delete_rejects_names_that_could_escape_the_folders() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    for name in ["../store", "a/b", "", "A"] {
        let err = call(&st, &h, INTAKE, "delete_skill", json!({"name": name})).unwrap_err();
        assert!(err.contains("Invalid skill name"), "{name}: {err}");
    }
}

#[test]
fn workers_cannot_use_skill_tools() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    queue(&st, &h, vec![spec("a", &[])]);
    let worker = session_of(&st, &h, "a");
    for method in ["save_skill", "list_skills", "delete_skill"] {
        let err = call(&st, &h, &worker, method, skill("x")).unwrap_err();
        assert!(err.contains("not available to automated sessions"), "{method}: {err}");
    }
    assert!(!sb.store().exists());
}

#[test]
fn skills_work_without_a_workspace_but_freeze_with_an_archived_one() {
    let sb = Sandbox::new();
    let (st, h) = (AutomationState::default(), sb.host());
    call(&st, &h, NO_WORKSPACE_TAB, "save_skill", skill("global")).unwrap();
    assert_eq!(call(&st, &h, NO_WORKSPACE_TAB, "list_skills", json!({})).unwrap().as_array().unwrap().len(), 1);
    *h.archived.lock().unwrap() = true;
    let err = call(&st, &h, INTAKE, "list_skills", json!({})).unwrap_err();
    assert_eq!(err, "workspace is archived");
}
