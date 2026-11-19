use super::presets::{self, default_bots, GENERAL_ID};
use super::validate::NAME_MAX_CHARS;
use super::*;
use crate::board::TaskKind;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| n.to_string()).collect()
}

#[test]
fn default_is_enabled_with_the_builtin_pool() {
    let s = BotSettings::default();
    assert!(s.enabled);
    assert_eq!(s.names.len(), DEFAULT_NAMES.len());
    assert_eq!(s.names[0], "Bolt");
    assert_eq!(s.names.last().map(String::as_str), Some("Zed"));
    assert_eq!(new_store().get(), s);
}

#[test]
fn restoring_names_keeps_enabled() {
    for enabled in [true, false] {
        let mut s = BotSettings { enabled, names: names(&["Ada"]), images: BTreeMap::new(), ..BotSettings::default() };
        restore_default_names(&mut s);
        assert_eq!(
            s,
            BotSettings { enabled, names: default_names(), images: BTreeMap::new(), ..BotSettings::default() }
        );
    }
}

fn img(color: &str) -> BotImage {
    BotImage { png: "AAAA".into(), color: color.into() }
}

fn with_images(list: &[&str], images: &[(&str, &str)]) -> BotSettings {
    BotSettings {
        enabled: true,
        names: names(list),
        images: images.iter().map(|(n, c)| (n.to_string(), img(c))).collect(),
        agents: default_bots(),
    }
}

#[test]
fn old_files_without_images_load_empty() {
    let a: BotSettings = serde_json::from_str("{}").unwrap();
    assert!(a.images.is_empty());
    let b: BotSettings = serde_json::from_str(r#"{"enabled":true,"names":["Ada"]}"#).unwrap();
    assert!(b.images.is_empty());
    assert_eq!(b.names, names(&["Ada"]));
}

#[test]
fn serde_roundtrip_keeps_images() {
    let s = with_images(&["Ada", "Bob"], &[("Ada", "#112233"), ("Bob", "#445566")]);
    let back: BotSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn pool_name_ignores_case_and_returns_pool_spelling() {
    let pool = names(&["Ada", "Bob"]);
    assert_eq!(pool_name(&pool, "aDA").map(String::as_str), Some("Ada"));
    assert_eq!(pool_name(&pool, "Bob").map(String::as_str), Some("Bob"));
    assert!(pool_name(&pool, "Eve").is_none());
}

#[test]
fn pruning_drops_removed_names_and_rekeys_to_pool_spelling() {
    let mut s = with_images(&["Ada", "Bob"], &[("Ada", "#111111"), ("bob", "#222222"), ("Eve", "#333333")]);
    prune_images(&mut s);
    assert_eq!(s.images.len(), 2);
    assert_eq!(s.images["Ada"], img("#111111"));
    assert_eq!(s.images["Bob"], img("#222222"));
}

#[test]
fn restoring_names_keeps_images_of_default_names_only() {
    let mut s = with_images(&["Ada", "Bolt"], &[("Ada", "#111111"), ("Bolt", "#222222")]);
    restore_default_names(&mut s);
    assert_eq!(s.images.len(), 1);
    assert_eq!(s.images["Bolt"], img("#222222"));
}

#[test]
fn default_pool_passes_its_own_validation() {
    assert_eq!(validate_names(default_names()).unwrap(), default_names());
}

#[test]
fn old_and_partial_files_fall_back_to_defaults() {
    let s: BotSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, BotSettings::default());
    let off: BotSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
    assert!(!off.enabled);
    assert_eq!(off.names, default_names());
    let custom: BotSettings = serde_json::from_str(r#"{"names":["Ada"]}"#).unwrap();
    assert!(custom.enabled);
    assert_eq!(custom.names, names(&["Ada"]));
    assert!(serde_json::from_str::<BotSettings>("non json").is_err());
}

#[test]
fn serde_roundtrip_keeps_an_empty_pool() {
    let s = BotSettings { enabled: false, names: vec![], images: BTreeMap::new(), ..BotSettings::default() };
    let back: BotSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(back, s);
}

#[test]
fn validation_trims_and_accepts_an_empty_list() {
    assert_eq!(validate_names(names(&["  Ada ", "Bob"])).unwrap(), names(&["Ada", "Bob"]));
    assert_eq!(validate_names(vec![]).unwrap(), Vec::<String>::new());
}

#[test]
fn validation_rejects_bad_lengths() {
    assert!(validate_names(names(&[""])).is_err());
    assert!(validate_names(names(&["   "])).is_err());
    assert!(validate_names(vec!["a".repeat(NAME_MAX_CHARS)]).is_ok());
    assert!(validate_names(vec!["a".repeat(NAME_MAX_CHARS + 1)]).is_err());
    // Il limite conta i caratteri, non i byte.
    assert!(validate_names(vec!["é".repeat(NAME_MAX_CHARS)]).is_ok());
}

#[test]
fn validation_rejects_control_characters() {
    assert!(validate_names(names(&["a\nb"])).is_err());
    assert!(validate_names(names(&["a\u{0}b"])).is_err());
    assert!(validate_names(names(&["a\tb"])).is_err());
}

#[test]
fn validation_rejects_duplicates_ignoring_case() {
    assert!(validate_names(names(&["Bolt", "bolt"])).is_err());
    assert!(validate_names(names(&["Bolt", " BOLT "])).is_err());
    assert!(validate_names(names(&["Bolt", "Bolt 2"])).is_ok());
}

#[test]
fn validation_caps_the_pool_size() {
    let at_cap: Vec<String> = (0..NAMES_MAX).map(|i| format!("Bot{i}")).collect();
    assert!(validate_names(at_cap.clone()).is_ok());
    let mut over = at_cap;
    over.push("Extra".into());
    assert!(validate_names(over).is_err());
}

// --- File reali delle versioni precedenti, caricati con la static vera ---

const FILE_NAME: &str = "bots.json";

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cc-bots-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load_fixture(name: &str, raw: &str) -> (std::path::PathBuf, BotSettingsStore) {
    let dir = temp_dir(name);
    std::fs::write(dir.join(FILE_NAME), raw).unwrap();
    let store = new_store();
    store.load(&dir);
    (dir, store)
}

fn backups(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("bots.json.bak-"))
        .collect()
}

#[test]
fn v0_empty_object_loads_defaults() {
    let (dir, store) = load_fixture("v0-empty", "{}");
    assert_eq!(store.get(), BotSettings::default());
    assert!(backups(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v0_disabled_keeps_the_default_pool() {
    let (dir, store) = load_fixture("v0-disabled", r#"{"enabled":false}"#);
    assert!(!store.get().enabled);
    assert_eq!(store.get().names, default_names());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v0_custom_names_stay_enabled() {
    let (dir, store) = load_fixture("v0-names", r#"{"names":["Ada"]}"#);
    assert!(store.get().enabled);
    assert_eq!(store.get().names, names(&["Ada"]));
    assert!(store.get().images.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v0_with_images_keeps_them() {
    let raw = r##"{"enabled":true,"names":["Ada","Bob"],"images":{"Ada":{"png":"AAAA","color":"#112233"}}}"##;
    let (dir, store) = load_fixture("v0-images", raw);
    let s = store.get();
    assert_eq!(s.names, names(&["Ada", "Bob"]));
    assert_eq!(s.images.len(), 1);
    assert_eq!(s.images["Ada"], img("#112233"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_file_is_backed_up_and_not_overwritten() {
    let garbage = "{\"names\": [\"Ada\"";
    let (dir, store) = load_fixture("corrupt", garbage);
    assert_eq!(store.get(), BotSettings::default());
    let found = backups(&dir);
    assert_eq!(found.len(), 1);
    assert_eq!(std::fs::read(&found[0]).unwrap(), garbage.as_bytes());
    assert!(!dir.join(FILE_NAME).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_writes_the_envelope_and_roundtrips() {
    let dir = temp_dir("roundtrip");
    let store = new_store();
    let next = store
        .update(&dir, |s| {
            s.enabled = false;
            s.names = names(&["Ada"]);
            s.images.insert("Ada".into(), img("#445566"));
            Ok(())
        })
        .unwrap();
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(FILE_NAME)).unwrap()).unwrap();
    assert_eq!(raw["schema_version"], 7);
    assert_eq!(raw["data"]["agents"][0]["id"], "general");
    assert_eq!(raw["data"]["enabled"], false);
    assert_eq!(raw["data"]["images"]["Ada"]["color"], "#445566");
    let reloaded = new_store();
    reloaded.load(&dir);
    assert_eq!(reloaded.get(), next);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_file_keeps_defaults_and_refuses_updates() {
    // Istanza locale: il blocco delle scritture e' appiccicoso per istanza.
    static LOCAL_FILE: JsonFile<BotSettings> = JsonFile::new("bots.json", &MIGRATIONS).decoded_with(decode_settings);
    let dir = temp_dir("unreadable");
    std::fs::create_dir_all(dir.join(FILE_NAME)).unwrap();
    let store = JsonStore::new(&LOCAL_FILE);
    store.load(&dir);
    assert_eq!(store.get(), BotSettings::default());
    let r = store.update(&dir, |s| {
        s.enabled = false;
        Ok(())
    });
    assert!(r.is_err());
    assert_eq!(store.get(), BotSettings::default());
    assert!(dir.join(FILE_NAME).is_dir());
    assert!(backups(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- v1 -> v2 -> v3 -> v4: i Bot-agente predefiniti e i loro tipi di task ---

fn agent_ids(s: &BotSettings) -> Vec<&str> {
    s.agents.iter().map(|b| b.id.as_str()).collect()
}

fn agent<'a>(s: &'a BotSettings, id: &str) -> &'a AgentBot {
    s.agents.iter().find(|b| b.id == id).unwrap()
}

const ALL_PRESET_IDS: [&str; 14] = [
    "general",
    "intake",
    "setup",
    "skill-wizard",
    "reviewer",
    "planner",
    "quick-answer",
    "bug-fixer",
    "researcher",
    "test-writer",
    "docs-writer",
    "refactorer",
    "git-operator",
    "release",
];

/// I Bot che il passo v6 aggiunge in fondo all'elenco.
const V6_PRESET_IDS: [&str; 4] = ["researcher", "test-writer", "docs-writer", "refactorer"];

/// I Bot, spenti, che il passo v7 aggiunge in fondo all'elenco.
const V7_PRESET_IDS: [&str; 2] = ["git-operator", "release"];

/// Gli id `before`, seguiti da quelli aggiunti dai passi v6 e v7.
fn with_v6<'a>(before: &[&'a str]) -> Vec<&'a str> {
    with_v7(&[before, &V6_PRESET_IDS[..]].concat())
}

/// Gli id `before`, seguiti da quelli aggiunti dal passo v7.
fn with_v7<'a>(before: &[&'a str]) -> Vec<&'a str> {
    [before, &V7_PRESET_IDS[..]].concat()
}

#[test]
fn v1_file_gets_the_preset_bots_and_keeps_names_and_images() {
    let raw = r##"{"schema_version":1,"data":{"enabled":false,"names":["Ada","Bob"],"images":{"Ada":{"png":"AAAA","color":"#112233"}}}}"##;
    let (dir, store) = load_fixture("v1-agents", raw);
    let s = store.get();
    assert!(!s.enabled);
    assert_eq!(s.names, names(&["Ada", "Bob"]));
    assert_eq!(s.images.len(), 1);
    assert_eq!(s.images["Ada"], img("#112233"));
    assert_eq!(s.agents, default_bots());
    assert_eq!(agent_ids(&s), ALL_PRESET_IDS);
    assert!(backups(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Un file v2 com'era su disco: Bot senza `kinds`, un preset cancellato, uno
/// rinominato, un derivato e un Bot dell'utente che ha gia' il nome "Planner".
const V2_FILE: &str = r#"{"schema_version":2,"data":{"enabled":true,"names":["Ada"],"agents":[
{"id":"general","name":"Generic","task":"General-purpose coding agent.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"enabled":true,"image":null,"preset":"general"},
{"id":"setup","name":"Setup","task":"Configures the workspace and the worker rules.","agent":null,"model":{"kind":"tier","value":"standard"},"effort":"medium","skill":{"kind":"plugin","id":"project-setup"},"enabled":false,"image":null,"preset":"setup"},
{"id":"intake-other","name":"Intake (Other)","task":"Turns requests into queue tasks.","agent":"other","model":{"kind":"tier","value":"strong"},"effort":"high","skill":{"kind":"plugin","id":"task-intake"},"enabled":true,"image":null,"preset":"intake"},
{"id":"custom-planner","name":"Planner","task":"My planner.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"enabled":true,"image":null,"preset":null}
]}}"#;

#[test]
fn v2_file_gets_task_kinds_and_the_new_presets_once() {
    let (dir, store) = load_fixture("v2-kinds", V2_FILE);
    let s = store.get();
    assert!(backups(&dir).is_empty());
    assert_eq!(
        agent_ids(&s),
        with_v6(&[
            "general",
            "setup",
            "intake-other",
            "custom-planner",
            "reviewer",
            "planner",
            "quick-answer",
            "bug-fixer"
        ])
    );
    assert_eq!(
        (agent(&s, "general").name.as_str(), agent(&s, "general").kinds.as_slice()),
        ("Generic", &[TaskKind::Work][..])
    );
    assert!(agent(&s, "setup").kinds.is_empty() && !agent(&s, "setup").enabled);
    for id in ["intake-other", "custom-planner"] {
        assert_eq!(agent(&s, id).kinds, [TaskKind::Work, TaskKind::Review, TaskKind::Plan], "{id}");
    }
    let reviewer = presets::find("reviewer").unwrap().bot();
    assert_eq!(agent(&s, "reviewer"), &reviewer);
    let planner = agent(&s, "planner");
    assert_eq!((planner.name.as_str(), planner.kinds.as_slice()), ("Planner 2", &[TaskKind::Plan][..]));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Un file v3 com'era su disco: il Reviewer cancellato e un Bot dell'utente
/// che ha gia' il nome "Quick Answer".
const V3_FILE: &str = r#"{"schema_version":3,"data":{"enabled":true,"names":["Ada"],"agents":[
{"id":"general","name":"General","task":"General-purpose coding agent.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":true,"image":null,"preset":"general"},
{"id":"planner","name":"Planner","task":"Explores the repository read-only and proposes queue tasks.","agent":null,"model":{"kind":"tier","value":"strong"},"effort":"high","skill":null,"kinds":["plan"],"enabled":false,"image":null,"preset":"planner"},
{"id":"custom-quick-answer","name":"Quick Answer","task":"Mine.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":true,"image":null,"preset":null}
]}}"#;

#[test]
fn v3_file_gets_quick_answer_and_bug_fixer_once() {
    let (dir, store) = load_fixture("v3-answer-fix", V3_FILE);
    let s = store.get();
    assert!(backups(&dir).is_empty());
    assert_eq!(agent_ids(&s), with_v6(&["general", "planner", "custom-quick-answer", "quick-answer", "bug-fixer"]));
    assert!(!agent(&s, "planner").enabled, "the existing bots are untouched");
    assert_eq!(agent(&s, "custom-quick-answer").name, "Quick Answer");
    let quick_answer = presets::find("quick-answer").unwrap().bot();
    assert_eq!(agent(&s, "quick-answer"), &AgentBot { name: "Quick Answer 2".into(), ..quick_answer });
    assert_eq!(agent(&s, "bug-fixer"), &presets::find("bug-fixer").unwrap().bot());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Un file v4 com'era su disco: `general` col vecchio `task` del preset,
/// `planner` con un `task` personalizzato.
const V4_FILE: &str = r#"{"schema_version":4,"data":{"enabled":true,"names":["Ada"],"agents":[
{"id":"general","name":"Generic","task":"General-purpose coding agent.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":true,"image":null,"preset":"general"},
{"id":"planner","name":"Planner","task":"Plans my way.","agent":"other","model":{"kind":"tier","value":"standard"},"effort":"low","skill":null,"kinds":["plan","work"],"enabled":false,"image":null,"preset":"planner"}
]}}"#;

#[test]
fn v4_file_gets_the_new_task_only_where_it_was_the_old_preset_text() {
    let (dir, store) = load_fixture("v4-tasks", V4_FILE);
    let s = store.get();
    assert!(backups(&dir).is_empty());
    assert_eq!(agent_ids(&s), with_v6(&["general", "planner"]));
    let preset = presets::find(GENERAL_ID).unwrap().bot();
    assert_eq!(agent(&s, "general"), &AgentBot { name: "Generic".into(), ..preset });
    let planner = agent(&s, "planner");
    assert_eq!(planner.task, "Plans my way.", "a customized task is kept");
    assert_eq!(
        (planner.agent.as_deref(), planner.effort, planner.enabled),
        (Some("other"), Some(crate::agents::Effort::Low), false)
    );
    assert_eq!(planner.kinds, [TaskKind::Plan, TaskKind::Work]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Un file v5 com'era su disco: il Refactorer non c'e' mai stato, un Bot
/// dell'utente ha gia' il nome "Test Writer" e `general` e' personalizzato.
const V5_FILE: &str = r#"{"schema_version":5,"data":{"enabled":true,"names":["Ada"],"agents":[
{"id":"general","name":"Generic","task":"Mine.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work","plan"],"enabled":true,"image":null,"preset":"general"},
{"id":"custom-tests","name":"Test Writer","task":"My tests.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":false,"image":null,"preset":null},
{"id":"refactorer","name":"Cleaner","task":"Cleans up.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":false,"image":null,"preset":"refactorer"}
]}}"#;

#[test]
fn v5_file_gets_the_four_new_bots_once() {
    let (dir, store) = load_fixture("v5-domain-bots", V5_FILE);
    let s = store.get();
    assert!(backups(&dir).is_empty());
    assert_eq!(
        agent_ids(&s),
        with_v7(&["general", "custom-tests", "refactorer", "researcher", "test-writer", "docs-writer"])
    );
    assert_eq!((agent(&s, "general").task.as_str(), agent(&s, "general").kinds.len()), ("Mine.", 2));
    assert_eq!((agent(&s, "custom-tests").name.as_str(), agent(&s, "custom-tests").enabled), ("Test Writer", false));
    assert_eq!(agent(&s, "refactorer").name, "Cleaner", "a present id is untouched");
    let test_writer = presets::find("test-writer").unwrap().bot();
    assert_eq!(agent(&s, "test-writer"), &AgentBot { name: "Test Writer 2".into(), ..test_writer });
    for id in ["researcher", "docs-writer"] {
        assert_eq!(agent(&s, id), &presets::find(id).unwrap().bot(), "{id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Un file v6 com'era su disco: un Bot dell'utente acceso ha gia' il nome "Git
/// Operator", il Release non c'e' mai stato e il Refactorer e' spento.
const V6_FILE: &str = r#"{"schema_version":6,"data":{"enabled":true,"names":["Ada"],"agents":[
{"id":"general","name":"General","task":"Mine.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":true,"image":null,"preset":"general"},
{"id":"custom-git","name":"Git Operator","task":"My git chores.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":true,"image":null,"preset":null},
{"id":"refactorer","name":"Refactorer","task":"Cleans up.","agent":null,"model":{"kind":"default"},"effort":null,"skill":null,"kinds":["work"],"enabled":false,"image":null,"preset":"refactorer"}
]}}"#;

#[test]
fn v6_file_gets_git_operator_and_release_once_and_disabled() {
    let (dir, store) = load_fixture("v6-risky-bots", V6_FILE);
    let s = store.get();
    assert!(backups(&dir).is_empty());
    assert_eq!(agent_ids(&s), with_v7(&["general", "custom-git", "refactorer"]));
    assert_eq!((agent(&s, "custom-git").name.as_str(), agent(&s, "custom-git").enabled), ("Git Operator", true));
    assert!(!agent(&s, "refactorer").enabled && agent(&s, GENERAL_ID).enabled, "the existing bots are untouched");
    let git_operator = presets::find("git-operator").unwrap().bot();
    assert_eq!(agent(&s, "git-operator"), &AgentBot { name: "Git Operator 2".into(), ..git_operator });
    assert_eq!(agent(&s, "release"), &presets::find("release").unwrap().bot());
    for id in V7_PRESET_IDS {
        assert!(!agent(&s, id).enabled, "{id} starts disabled");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_file_has_the_risky_bots_disabled_and_the_others_enabled() {
    let (dir, store) = load_fixture("v0-risky-bots", "{}");
    let s = store.get();
    let off: Vec<&str> = s.agents.iter().filter(|b| !b.enabled).map(|b| b.id.as_str()).collect();
    assert_eq!(off, V7_PRESET_IDS);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_file_has_the_exclusive_tasks_of_general_and_planner() {
    let (dir, store) = load_fixture("v0-tasks", "{}");
    let s = store.get();
    for id in [GENERAL_ID, "planner"] {
        assert_eq!(agent(&s, id).task, presets::find(id).unwrap().task, "{id}");
    }
    assert!(agent(&s, GENERAL_ID).task.contains("no specialized bot covers"));
    assert!(agent(&s, "planner").task.starts_with("Plans changes to the project"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_starts_with_the_preset_bots() {
    let dir = temp_dir("absent");
    let store = new_store();
    store.load(&dir);
    assert_eq!(store.get().agents, default_bots());
    assert_eq!(store.get().names, default_names());
    assert!(!dir.join(FILE_NAME).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_migrations_run_once_so_a_deleted_preset_stays_deleted() {
    for deleted in ["intake", "reviewer", "quick-answer", "bug-fixer", "researcher", "refactorer", "git-operator"] {
        let dir = temp_dir(&format!("deleted-preset-{deleted}"));
        std::fs::write(dir.join(FILE_NAME), r#"{"schema_version":1,"data":{}}"#).unwrap();
        let store = new_store();
        store.load(&dir);
        store.update(&dir, |s| agent_bots::delete(&mut s.agents, deleted)).unwrap();
        let reloaded = new_store();
        reloaded.load(&dir);
        let expected: Vec<&str> = ALL_PRESET_IDS.into_iter().filter(|id| *id != deleted).collect();
        assert_eq!(agent_ids(&reloaded.get()), expected, "{deleted}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_v4_file_without_general_gets_it_back_enabled() {
    let mut bots = default_bots();
    bots.retain(|b| b.id != GENERAL_ID);
    bots[0].enabled = false;
    let data = serde_json::json!({"enabled": true, "names": [], "agents": bots});
    let raw = serde_json::json!({"schema_version": 4, "data": data}).to_string();
    let (dir, store) = load_fixture("no-general", &raw);
    let s = store.get();
    assert_eq!(agent_ids(&s), ALL_PRESET_IDS);
    assert!(s.agents[0].enabled);
    assert_eq!(s.agents[0].kinds, [TaskKind::Work]);
    assert!(!s.agents[1].enabled, "the other bots keep their state");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_newer_schema_is_set_aside() {
    let (dir, store) = load_fixture("newer", r#"{"schema_version":8,"data":{}}"#);
    assert_eq!(store.get(), BotSettings::default());
    assert_eq!(backups(&dir).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
