use super::*;
use crate::board::{build_report, parse_spec, validate_graph, TaskStatus, TestRun};
use serde_json::json;

fn spec(id: &str, kind: TaskKind, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        kind,
        ..TaskSpec::default()
    }
}

fn fields(verdict: Option<Verdict>, findings: &[&str]) -> ReportFields {
    ReportFields { verdict, findings: findings.iter().map(|f| f.to_string()).collect(), ..ReportFields::default() }
}

fn done(id: &str, f: ReportFields) -> Task {
    let mut t = Task::from_spec(spec(id, TaskKind::Work, &[]), 0);
    t.status = TaskStatus::Done;
    t.report = Some(build_report("completed", &format!("summary of {id}"), f, Some(" 1 file changed".to_string())));
    t
}

fn board(tasks: Vec<Task>) -> Board {
    Board { tasks, ..Board::new("ws-1") }
}

fn review_of(deps: &[&str]) -> Task {
    Task::from_spec(spec("rev", TaskKind::Review, deps), 0)
}

fn many(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("f{i}")).collect()
}

/// Riepilogo della prima dipendenza `a` di una revisione.
fn entry_of(t: Task) -> Value {
    reviewed_tasks(&board(vec![t]), &review_of(&["a"]))[0].clone()
}

#[test]
fn kind_and_verdict_parse_their_names() {
    for k in TaskKind::ALL {
        assert_eq!(TaskKind::parse(k.as_str()), Some(k));
    }
    for v in Verdict::ALL {
        assert_eq!(Verdict::parse(v.as_str()), Some(v));
    }
    assert_eq!(TaskKind::default(), TaskKind::Work);
    assert_eq!((TaskKind::parse("Review"), Verdict::parse("ok")), (None, None));
    assert_eq!(serde_json::to_value(TaskKind::Review).unwrap(), json!("review"));
    assert_eq!(serde_json::to_value(Verdict::Issues).unwrap(), json!("issues"));
}

#[test]
fn a_review_needs_at_least_one_dependency() {
    let err = check_review_deps(&spec("rev", TaskKind::Review, &[])).unwrap_err();
    assert!(err.contains("'rev'") && err.contains("at least one dependency"), "{err}");
    assert!(check_review_deps(&spec("rev", TaskKind::Review, &["a"])).is_ok());
    assert!(check_review_deps(&spec("a", TaskKind::Work, &[])).is_ok());
}

#[test]
fn check_verdict_on_work_tasks() {
    assert!(check_verdict(TaskKind::Work, true, &fields(None, &[])).is_ok());
    assert!(check_verdict(TaskKind::Work, false, &fields(None, &[])).is_ok());
    for f in [fields(Some(Verdict::Passed), &[]), fields(None, &["x"])] {
        let err = check_verdict(TaskKind::Work, true, &f).unwrap_err();
        assert!(err.contains("only accepted for review tasks"), "{err}");
    }
}

#[test]
fn check_verdict_on_completed_reviews() {
    let err = check_verdict(TaskKind::Review, true, &fields(None, &[])).unwrap_err();
    assert!(err.contains("verdict is missing"), "{err}");
    let err = check_verdict(TaskKind::Review, true, &fields(None, &["x"])).unwrap_err();
    assert!(err.contains("verdict is missing"), "{err}");
    let err = check_verdict(TaskKind::Review, true, &fields(Some(Verdict::Issues), &[])).unwrap_err();
    assert!(err.contains("findings is missing"), "{err}");
    assert!(check_verdict(TaskKind::Review, true, &fields(Some(Verdict::Issues), &["bug"])).is_ok());
    assert!(check_verdict(TaskKind::Review, true, &fields(Some(Verdict::Passed), &[])).is_ok());
    // Suggerimenti ammessi anche con `passed`.
    assert!(check_verdict(TaskKind::Review, true, &fields(Some(Verdict::Passed), &["nit"])).is_ok());
}

#[test]
fn check_verdict_on_failed_reviews() {
    assert!(check_verdict(TaskKind::Review, false, &fields(None, &[])).is_ok());
    assert!(check_verdict(TaskKind::Review, false, &fields(None, &["partial"])).is_ok());
    for v in Verdict::ALL {
        let err = check_verdict(TaskKind::Review, false, &fields(Some(v), &["x"])).unwrap_err();
        assert!(err.contains("only accepted with status \"completed\""), "{err}");
    }
}

#[test]
fn reviewed_tasks_follow_depends_on_and_skip_missing_ids() {
    let f = ReportFields {
        commit: Some("abc1234".into()),
        pr_url: Some("https://github.com/o/r/pull/7".into()),
        files_changed: vec!["src/a.rs".into()],
        tests: vec![TestRun { command: "cargo test".into(), result: "passed".into(), details: Some("ok".into()) }],
        follow_ups: vec!["docs".into()],
        ..ReportFields::default()
    };
    let b = board(vec![done("a", f), done("b", ReportFields::default())]);
    let out = reviewed_tasks(&b, &review_of(&["b", "ghost", "a"]));
    let ids: Vec<&str> = out.as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(
        out[1],
        json!({
            "id": "a", "title": "Task a", "status": "done", "summary": "summary of a", "commit": "abc1234",
            "pr_url": "https://github.com/o/r/pull/7", "files_changed": ["src/a.rs"], "diffstat": " 1 file changed",
            "tests": [{"command": "cargo test", "result": "passed", "details": "ok"}], "follow_ups": ["docs"]
        })
    );
    assert!(out[0].get("truncated").is_none());
}

#[test]
fn a_dependency_without_report_has_null_and_empty_fields() {
    let e = entry_of(Task::from_spec(spec("a", TaskKind::Work, &[]), 0));
    assert_eq!(
        e,
        json!({
            "id": "a", "title": "Task a", "status": "queued", "summary": null, "commit": null, "pr_url": null,
            "files_changed": [], "diffstat": null, "tests": [], "follow_ups": []
        })
    );
}

#[test]
fn oversized_lists_and_items_are_cut_and_flagged() {
    let long = "é".repeat(REVIEWED_ITEM_MAX + 1);
    let test_run = |i: usize, details: Option<String>| TestRun { command: format!("t{i}"), result: "failed".into(), details };
    let cases = [
        ("files", ReportFields { files_changed: many(REVIEWED_LIST_MAX + 1), ..Default::default() }),
        ("follow_ups", ReportFields { follow_ups: many(REVIEWED_LIST_MAX + 1), ..Default::default() }),
        ("follow_up item", ReportFields { follow_ups: vec![long.clone()], ..Default::default() }),
        ("tests", ReportFields { tests: (0..=REVIEWED_LIST_MAX).map(|i| test_run(i, None)).collect(), ..Default::default() }),
        ("test details", ReportFields { tests: vec![test_run(0, Some(long))], ..Default::default() }),
    ];
    for (name, f) in cases {
        assert_eq!(entry_of(done("a", f))["truncated"], true, "{name}");
    }
    let e = entry_of(done("a", ReportFields { files_changed: many(REVIEWED_LIST_MAX + 1), ..Default::default() }));
    assert_eq!(e["files_changed"].as_array().unwrap().len(), REVIEWED_LIST_MAX);
    // Un elemento tagliato rispetta il limite in caratteri e finisce con i puntini.
    let e = entry_of(done("a", ReportFields { follow_ups: vec!["é".repeat(REVIEWED_ITEM_MAX + 1)], ..Default::default() }));
    let item = e["follow_ups"][0].as_str().unwrap();
    assert_eq!(item.chars().count(), REVIEWED_ITEM_MAX + 1);
    assert!(item.ends_with('…'));
    // Esattamente al limite: nessun taglio.
    let e = entry_of(done("a", ReportFields { files_changed: many(REVIEWED_LIST_MAX), ..Default::default() }));
    assert!(e.get("truncated").is_none());
}

#[test]
fn long_summary_and_diffstat_are_cut_and_flagged() {
    let mut t = done("a", ReportFields::default());
    t.report.as_mut().unwrap().summary = "s".repeat(REVIEWED_SUMMARY_MAX + 1);
    let e = entry_of(t);
    assert_eq!(e["truncated"], true);
    assert_eq!(e["summary"].as_str().unwrap().chars().count(), REVIEWED_SUMMARY_MAX + 1);

    let mut t = done("a", ReportFields::default());
    t.report.as_mut().unwrap().diffstat = Some("d".repeat(REVIEWED_DIFFSTAT_MAX + 1));
    assert_eq!(entry_of(t)["truncated"], true);

    let mut t = done("a", ReportFields::default());
    let r = t.report.as_mut().unwrap();
    r.summary = "s".repeat(REVIEWED_SUMMARY_MAX);
    r.diffstat = Some("d".repeat(REVIEWED_DIFFSTAT_MAX));
    assert!(entry_of(t).get("truncated").is_none());
}

#[test]
fn parse_spec_reads_the_kind() {
    let with = |kind: Value| json!({"id": "a", "title": "T", "task": "x", "kind": kind});
    let without = json!({"id": "a", "title": "T", "task": "x"});
    assert_eq!(parse_spec(&without, None).unwrap().kind, TaskKind::Work);
    assert_eq!(parse_spec(&with(json!(null)), None).unwrap().kind, TaskKind::Work);
    assert_eq!(parse_spec(&with(json!("review")), None).unwrap().kind, TaskKind::Review);
    assert_eq!(parse_spec(&with(json!("audit")), None).unwrap_err(), "invalid kind 'audit' (allowed: work, review)");
    assert_eq!(parse_spec(&with(json!(3)), None).unwrap_err(), "kind must be a string");
    // Una modifica senza `kind` lo lascia com'e'; con null torna `work`.
    let existing = spec("a", TaskKind::Review, &["b"]);
    assert_eq!(parse_spec(&json!({"title": "New"}), Some(&existing)).unwrap().kind, TaskKind::Review);
    assert_eq!(parse_spec(&json!({"kind": null}), Some(&existing)).unwrap().kind, TaskKind::Work);
}

#[test]
fn old_boards_load_as_work_and_new_fields_roundtrip() {
    let old = r#"{"workspace_id":"ws-1","tasks":[{"id":"a","title":"A","task":"x","status":"done",
        "report":{"status":"completed","summary":"s"}}],
        "proposals":[{"id":"p-1","source_tab":"t","created_at":1,"tasks":[{"id":"b","title":"B","task":"y"}]}]}"#;
    let b = Board::from_json(old, 1).unwrap();
    assert_eq!(b.tasks[0].kind, TaskKind::Work);
    let r = b.tasks[0].report.as_ref().unwrap();
    assert!(r.verdict.is_none() && r.findings.is_empty());
    assert_eq!(b.proposals[0].source_task, None);
    assert_eq!(b.proposals[0].tasks[0].kind, TaskKind::Work);
    let saved = serde_json::to_value(&b).unwrap();
    assert!(saved["proposals"][0].get("source_task").is_none());
    assert!(saved["tasks"][0]["report"].get("verdict").is_none() && saved["tasks"][0]["report"].get("findings").is_none());

    let mut b = Board::new("ws-1");
    b.create_task(spec("a", TaskKind::Work, &[]), 1).unwrap();
    b.create_task(spec("rev", TaskKind::Review, &["a"]), 2).unwrap();
    b.add_proposal("p-1".into(), "tab-1", Some("rev".into()), vec![spec("fix", TaskKind::Review, &["a"])], 3).unwrap();
    b.task_mut("rev").unwrap().report = Some(build_report("completed", "s", fields(Some(Verdict::Issues), &["bug"]), None));
    let back = Board::from_json(&serde_json::to_string(&b).unwrap(), 9).unwrap();
    assert_eq!(back.tasks, b.tasks);
    assert_eq!(back.tasks[1].kind, TaskKind::Review);
    let r = back.tasks[1].report.as_ref().unwrap();
    assert_eq!((r.verdict, r.findings.as_slice()), (Some(Verdict::Issues), ["bug".to_string()].as_slice()));
    assert_eq!(back.proposals, b.proposals);
    assert_eq!(back.proposals[0].source_task.as_deref(), Some("rev"));
    assert_eq!(back.proposals[0].tasks[0].kind, TaskKind::Review);
}

#[test]
fn review_dependencies_are_validated_on_every_entry_point() {
    let mut b = Board::new("ws-1");
    let orphan = spec("rev", TaskKind::Review, &[]);
    assert!(b.create_task(orphan.clone(), 1).unwrap_err().contains("at least one dependency"));
    assert!(b.add_proposal("p-1".into(), "t", None, vec![orphan], 1).unwrap_err().contains("at least one dependency"));
    b.create_task(spec("a", TaskKind::Work, &[]), 1).unwrap();
    b.create_task(spec("rev", TaskKind::Review, &["a"]), 1).unwrap();
    // Update: togliere le dipendenze a una revisione, o fare `review` un task senza dipendenze.
    let err = b.update_task("rev", &json!({"depends_on": []}), 2).unwrap_err();
    assert!(err.contains("at least one dependency"), "{err}");
    let err = b.update_task("a", &json!({"kind": "review"}), 2).unwrap_err();
    assert!(err.contains("at least one dependency"), "{err}");
    assert_eq!(b.task("a").unwrap().kind, TaskKind::Work);
    b.create_task(spec("b", TaskKind::Work, &["a"]), 3).unwrap();
    b.update_task("b", &json!({"kind": "review"}), 4).unwrap();
    assert_eq!(b.task("b").unwrap().kind, TaskKind::Review);
    // Un task `done` che resta senza dipendenze (delete_task) non invalida la board.
    b.task_mut("b").unwrap().status = TaskStatus::Failed;
    b.delete_task("b", 5).unwrap();
    b.task_mut("rev").unwrap().status = TaskStatus::Done;
    b.task_mut("a").unwrap().status = TaskStatus::Failed;
    b.delete_task("a", 6).unwrap();
    assert!(b.task("rev").unwrap().depends_on.is_empty());
    validate_graph(&b.tasks).unwrap();
}
