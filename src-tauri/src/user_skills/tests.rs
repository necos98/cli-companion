use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cartella temporanea unica per test, rimossa a fine test.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mc-user-skills-{}-{id}", std::process::id()));
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
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn file(path: &str, content: &str) -> SkillFile {
    SkillFile { path: path.to_string(), content: content.to_string() }
}

fn input(name: &str) -> SkillInput {
    SkillInput {
        name: name.to_string(),
        description: "Use when reviewing release notes".to_string(),
        body: "# Steps\n\n1. Read the notes.".to_string(),
        files: Vec::new(),
    }
}

fn err(input: &SkillInput) -> String {
    validate(input).unwrap_err()
}

#[test]
fn valid_input_passes() {
    let mut i = input("release-notes");
    i.files = vec![file("refs/style.md", "x"), file("run.sh", "echo")];
    assert_eq!(validate(&i), Ok(()));
}

#[test]
fn invalid_slugs_are_rejected() {
    let too_long = "a".repeat(41);
    for bad in ["", "Upper", "-lead", "has space", "dot.name", "a/b", "..", "è", "under_score", too_long.as_str()] {
        assert!(err(&input(bad)).contains("Invalid skill name"), "{bad}");
    }
    assert_eq!(validate(&input(&"a".repeat(40))), Ok(()));
    assert_eq!(validate(&input("0-start")), Ok(()));
}

#[test]
fn reserved_names_are_rejected() {
    assert!(err(&input("skill-wizard")).contains("reserved"));
    for preset in crate::agents::presets::PRESETS {
        assert!(err(&input(preset.id)).contains("reserved"), "{}", preset.id);
    }
}

#[test]
fn description_rules() {
    let mut i = input("a");
    for bad in ["", "   ", "line1\nline2", "line1\r\nline2", "tab\there", "a\u{2028}b", "x --- y", "---"] {
        i.description = bad.to_string();
        assert!(validate(&i).is_err(), "{bad:?}");
    }
    i.description = "x".repeat(1025);
    assert!(err(&i).contains("longer than 1024"));
    i.description = "x".repeat(1024);
    assert_eq!(validate(&i), Ok(()));
    i.description = "è".repeat(1024);
    assert_eq!(validate(&i), Ok(()), "il limite conta i caratteri, non i byte");
}

#[test]
fn injected_frontmatter_is_rejected() {
    let mut i = input("a");
    i.description = "ok\n---\nname: evil".to_string();
    assert!(validate(&i).is_err());
}

#[test]
fn body_rules() {
    let mut i = input("a");
    i.body = "  \n ".to_string();
    assert!(err(&i).contains("empty"));
    i.body = "x".repeat(100 * 1024 + 1);
    assert!(err(&i).contains("100 KB"));
    i.body = "x".repeat(100 * 1024);
    assert_eq!(validate(&i), Ok(()));
    i.body = "a\0b".to_string();
    assert!(err(&i).contains("plain text"));
}

#[test]
fn unsafe_file_paths_are_rejected() {
    for bad in [
        "", "../x", "a/../x", "a/./x", "/abs", "C:/x", "a\\b", "a:b", "a//b", "a/", "dir/..", "SKILL.md", "skill.md",
        ".cli-companion-managed", ".cli-companion-managed/x", "trailing.", "sp ace /x", "a*b", "q?",
    ] {
        let mut i = input("a");
        i.files = vec![file(bad, "x")];
        assert!(err(&i).contains("Invalid file path"), "{bad:?}");
    }
}

#[test]
fn nested_skill_md_is_allowed() {
    let mut i = input("a");
    i.files = vec![file("sub/SKILL.md", "x")];
    assert_eq!(validate(&i), Ok(()));
}

#[test]
fn file_limits() {
    let mut i = input("a");
    i.files = (0..21).map(|n| file(&format!("f{n}.md"), "x")).collect();
    assert!(err(&i).contains("at most 20"));
    i.files.truncate(20);
    assert_eq!(validate(&i), Ok(()));
    i.files = vec![file("big.md", &"x".repeat(200 * 1024 + 1))];
    assert!(err(&i).contains("200 KB"));
    i.files = vec![file("bin.dat", "a\0b")];
    assert!(err(&i).contains("plain text"));
    i.files = vec![file("A.md", "x"), file("a.MD", "y")];
    assert!(err(&i).contains("Duplicate"));
}

#[test]
fn save_generates_coherent_skill_md_with_lf() {
    let tmp = TempDir::new();
    let mut i = input("my-skill");
    i.description = "  Say \"hi\": it's a \\ test  ".to_string();
    i.body = "line one\r\nline two\r\n\r\n".to_string();
    let saved = save(&tmp.store(), &i).unwrap();
    assert_eq!(saved, SavedSkill { name: "my-skill".into(), description: "Say \"hi\": it's a \\ test".into() });
    let text = fs::read_to_string(tmp.store().join("my-skill/SKILL.md")).unwrap();
    assert!(!text.contains('\r'));
    assert!(text.starts_with(
        "---\nname: my-skill\ndescription: \"Say \\\"hi\\\": it's a \\\\ test\"\n---\n\nline one\nline two\n"
    ));
    assert!(text.ends_with("line two\n"));
    assert_eq!(parse_description(&text).as_deref(), Some("Say \"hi\": it's a \\ test"));
}

#[test]
fn save_rejects_invalid_input_without_touching_the_disk() {
    let tmp = TempDir::new();
    assert!(save(&tmp.store(), &input("Bad Name")).is_err());
    assert!(!tmp.store().exists());
}

#[test]
fn list_is_sorted_and_ignores_foreign_folders() {
    let tmp = TempDir::new();
    assert_eq!(list(&tmp.store()), Ok(Vec::new()));
    save(&tmp.store(), &input("zeta")).unwrap();
    save(&tmp.store(), &input("alpha")).unwrap();
    fs::create_dir_all(tmp.store().join("no-skill-md")).unwrap();
    fs::create_dir_all(tmp.store().join(".hidden")).unwrap();
    let skills = list(&tmp.store()).unwrap();
    let names: Vec<_> = skills.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta"]);
    assert_eq!(skills[0].description, "Use when reviewing release notes");
}

#[test]
fn update_replaces_obsolete_accessory_files() {
    let tmp = TempDir::new();
    let mut i = input("a");
    i.files = vec![file("old.md", "1"), file("dir/nested.md", "2")];
    save(&tmp.store(), &i).unwrap();
    i.files = vec![file("new.md", "3")];
    i.body = "updated".to_string();
    save(&tmp.store(), &i).unwrap();
    let dir = tmp.store().join("a");
    assert!(!dir.join("old.md").exists() && !dir.join("dir").exists());
    assert_eq!(fs::read_to_string(dir.join("new.md")).unwrap(), "3");
    assert!(fs::read_to_string(dir.join("SKILL.md")).unwrap().ends_with("updated\n"));
    assert!(!tmp.store().join(".a.cli-companion-tmp").exists());
}

#[test]
fn delete_removes_only_existing_skills() {
    let tmp = TempDir::new();
    save(&tmp.store(), &input("a")).unwrap();
    delete(&tmp.store(), "a").unwrap();
    assert!(!tmp.store().join("a").exists());
    assert!(delete(&tmp.store(), "a").unwrap_err().contains("not found"));
    assert!(delete(&tmp.store(), "../x").unwrap_err().contains("Invalid skill name"));
}

#[test]
fn sync_one_copies_files_and_adds_the_marker() {
    let tmp = TempDir::new();
    let mut i = input("a");
    i.files = vec![file("refs/x.md", "ref")];
    save(&tmp.store(), &i).unwrap();
    sync_one(&tmp.store(), "a", &tmp.target()).unwrap();
    let dest = tmp.target().join("a");
    assert!(dest.join(MANAGED_MARKER).is_file());
    assert_eq!(fs::read_to_string(dest.join("refs/x.md")).unwrap(), "ref");
    assert_eq!(
        fs::read_to_string(dest.join(SKILL_FILE)).unwrap(),
        fs::read_to_string(tmp.store().join("a").join(SKILL_FILE)).unwrap()
    );
    assert!(!tmp.store().join("a").join(MANAGED_MARKER).exists());
}

#[test]
fn sync_one_updates_a_managed_target_and_drops_stale_files() {
    let tmp = TempDir::new();
    let mut i = input("a");
    i.files = vec![file("old.md", "1")];
    save(&tmp.store(), &i).unwrap();
    sync_one(&tmp.store(), "a", &tmp.target()).unwrap();
    i.files = Vec::new();
    i.body = "v2".to_string();
    save(&tmp.store(), &i).unwrap();
    sync_one(&tmp.store(), "a", &tmp.target()).unwrap();
    let dest = tmp.target().join("a");
    assert!(!dest.join("old.md").exists() && dest.join(MANAGED_MARKER).is_file());
    assert!(fs::read_to_string(dest.join(SKILL_FILE)).unwrap().ends_with("v2\n"));
}

#[test]
fn unmanaged_target_is_never_overwritten_or_deleted() {
    let tmp = TempDir::new();
    save(&tmp.store(), &input("a")).unwrap();
    let hand_written = tmp.target().join("a");
    fs::create_dir_all(&hand_written).unwrap();
    fs::write(hand_written.join(SKILL_FILE), "mine").unwrap();

    let e = sync_one(&tmp.store(), "a", &tmp.target()).unwrap_err();
    assert!(e.contains("not created by cli-companion"), "{e}");
    let e = remove_from(&tmp.target(), "a").unwrap_err();
    assert!(e.contains("not created by cli-companion"), "{e}");
    assert_eq!(fs::read_to_string(hand_written.join(SKILL_FILE)).unwrap(), "mine");
}

#[test]
fn a_plain_file_in_the_target_is_not_replaced() {
    let tmp = TempDir::new();
    save(&tmp.store(), &input("a")).unwrap();
    fs::create_dir_all(tmp.target()).unwrap();
    fs::write(tmp.target().join("a"), "file").unwrap();
    assert!(sync_one(&tmp.store(), "a", &tmp.target()).is_err());
    assert_eq!(fs::read_to_string(tmp.target().join("a")).unwrap(), "file");
}

#[test]
fn sync_one_requires_a_stored_skill_and_a_slug() {
    let tmp = TempDir::new();
    assert!(sync_one(&tmp.store(), "missing", &tmp.target()).unwrap_err().contains("not found"));
    assert!(sync_one(&tmp.store(), "../evil", &tmp.target()).unwrap_err().contains("Invalid skill name"));
    assert!(remove_from(&tmp.target(), "..").unwrap_err().contains("Invalid skill name"));
}

#[test]
fn remove_from_deletes_managed_and_tolerates_missing() {
    let tmp = TempDir::new();
    save(&tmp.store(), &input("a")).unwrap();
    sync_one(&tmp.store(), "a", &tmp.target()).unwrap();
    assert_eq!(remove_from(&tmp.target(), "a"), Ok(true));
    assert!(!tmp.target().join("a").exists());
    assert_eq!(remove_from(&tmp.target(), "a"), Ok(false));
}

#[test]
fn sync_all_rebuilds_missing_skills_in_an_empty_target() {
    let tmp = TempDir::new();
    for name in ["a", "b", "c"] {
        save(&tmp.store(), &input(name)).unwrap();
    }
    let report = sync_all(&tmp.store(), &tmp.target()).unwrap();
    assert_eq!(report.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert!(report.iter().all(|r| r.result.is_ok()));
    for name in ["a", "b", "c"] {
        assert!(tmp.target().join(name).join(SKILL_FILE).is_file(), "{name}");
        assert!(tmp.target().join(name).join(MANAGED_MARKER).is_file(), "{name}");
    }
}

#[test]
fn sync_all_reports_a_blocked_skill_and_continues() {
    let tmp = TempDir::new();
    save(&tmp.store(), &input("a")).unwrap();
    save(&tmp.store(), &input("b")).unwrap();
    fs::create_dir_all(tmp.target().join("a")).unwrap();
    let report = sync_all(&tmp.store(), &tmp.target()).unwrap();
    assert!(report[0].result.is_err());
    assert!(report[1].result.is_ok());
}

#[test]
fn sync_all_without_a_store_is_empty() {
    let tmp = TempDir::new();
    assert_eq!(sync_all(&tmp.store(), &tmp.target()), Ok(Vec::new()));
}
