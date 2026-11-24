use super::pieces::size;
use super::*;
use crate::board::SpecContext;
use crate::board::{
    build_report, AgentGate, Board, Finished, ReportFields, TaskQuestion, TaskReport, TaskSpec, TaskState, TestRun,
    Verdict,
};
use crate::hub::engine::fixtures::names;

fn board(configure: impl FnOnce(&mut Board)) -> TaskView {
    let mut board = Board::new("ws-1");
    let spec = TaskSpec {
        id: "t-1".into(),
        title: "Login".into(),
        task: "Build the sign-in page".into(),
        ..TaskSpec::default()
    };
    board.create_task(spec, SpecContext::default(), 1_700_000_000_000).unwrap();
    configure(&mut board);
    board.view_with(None, false, &AgentGate::default()).tasks.remove(0)
}

fn finished(report: TaskReport) -> TaskState {
    TaskState::Done(Finished {
        started_at: Some(1_700_000_001_000),
        finished_at: 1_700_000_002_000,
        report: Some(report),
    })
}

fn report(summary: &str, items: usize, item: impl Fn(usize) -> String) -> TaskReport {
    let fields = ReportFields {
        commit: Some("abc1234".into()),
        pr_url: Some("https://example.com/pr/7".into()),
        files_changed: (0..items).map(|i| format!("src/file_{i}.rs")).collect(),
        tests: (0..items)
            .map(|i| TestRun { command: format!("cargo test {i}"), result: "passed".into(), details: Some(item(i)) })
            .collect(),
        follow_ups: (0..items).map(&item).collect(),
        verdict: Some(Verdict::Issues),
        findings: vec!["Missing test".into()],
    };
    build_report("completed", summary, fields, Some(" 3 files changed".into()))
}

fn details(v: &TaskView) -> Value {
    let out = task_details(v, "ws-1", &names(), None).unwrap();
    assert!(size(&out) <= TOOL_RESULT_MAX, "{}", size(&out));
    out
}

fn page(v: &TaskView, field: &str, offset: usize) -> Result<Value, String> {
    let page = TaskPage { field: field.into(), offset };
    let out = task_details(v, "ws-1", &names(), Some(&page))?;
    assert!(size(&out) <= TOOL_RESULT_MAX, "{field}@{offset}: {}", size(&out));
    Ok(out)
}

/// Il campo `field` ricostruito come lo farebbe il modello: `acc` (il pezzo
/// gia' letto) piu' le pagine da `offset` finche' `next_offset` manca, con le
/// voci troppo lunghe rilette per intero dai loro `more`.
fn read_rest(v: &TaskView, field: &str, mut offset: usize, mut acc: Value) -> Value {
    loop {
        let mut out = page(v, field, offset).unwrap();
        assert_eq!((out["task_id"].as_str(), out["field"].as_str()), (Some("t-1"), Some(field)));
        assert_eq!(out["untrusted"], true);
        complete_lone_item(v, &mut out);
        match (&mut acc, out["content"].as_str(), out["items"].as_array()) {
            (Value::String(text), Some(piece), _) => text.push_str(piece),
            (Value::Array(items), _, Some(piece)) => items.extend(piece.iter().cloned()),
            _ => panic!("{field}: {out}"),
        }
        match out["next_offset"].as_u64().map(|n| n as usize) {
            Some(next) => {
                assert!(next > offset, "{field}: no progress at {offset}");
                offset = next;
            }
            None => return acc,
        }
    }
}

fn complete_lone_item(v: &TaskView, out: &mut Value) {
    let Some(more) = out.get("more").and_then(Value::as_object).cloned() else {
        return;
    };
    for (name, entry) in more {
        let next = entry["next_offset"].as_u64().unwrap() as usize;
        let slot = match name.rsplit_once('.') {
            Some((_, part)) if part.parse::<usize>().is_err() => &mut out["items"][0][part],
            _ => &mut out["items"][0],
        };
        *slot = read_rest(v, &name, next, slot.take());
    }
    out.as_object_mut().unwrap().remove("more");
}

/// Ogni campo della scheda, ricostruito con le pagine dove `more` lo chiede.
fn read_all(v: &TaskView) -> Value {
    let mut card = details(v);
    let more = card.as_object_mut().unwrap().remove("more").unwrap_or_default();
    for (field, entry) in more.as_object().into_iter().flatten() {
        let slot = card.pointer_mut(&pointer(field)).unwrap();
        let next = entry["next_offset"].as_u64().unwrap() as usize;
        assert_eq!(Content::of(slot).unwrap().total(), next, "{field}: the card holds the first {next}");
        *slot = read_rest(v, field, next, slot.take());
    }
    card
}

fn full_card(v: &TaskView) -> Value {
    card(v, "ws-1", &names()).unwrap()
}

#[test]
fn a_done_task_carries_its_full_report_and_no_internal_field() {
    let v = board(|b| b.task_mut("t-1").unwrap().set_state(finished(report("Done.", 2, |i| format!("ok {i}")))));
    let out = details(&v);
    assert_eq!(
        (out["id"].as_str(), out["status"].as_str(), out["workspace_name"].as_str()),
        (Some("t-1"), Some("done"), Some("Muse Companion"))
    );
    assert_eq!(out["task"], "Build the sign-in page");
    let report = &out["report"];
    assert_eq!(
        (report["commit"].as_str(), report["pr_url"].as_str()),
        (Some("abc1234"), Some("https://example.com/pr/7"))
    );
    assert_eq!(report["files_changed"], json!(["src/file_0.rs", "src/file_1.rs"]));
    assert_eq!(report["tests"][0], json!({"command": "cargo test 0", "result": "passed", "details": "ok 0"}));
    assert_eq!((report["verdict"].as_str(), report["findings"][0].as_str()), (Some("issues"), Some("Missing test")));
    assert_eq!(report["diffstat"], " 3 files changed");
    assert_eq!(out["created_at"], "2023-11-14T22:13:20Z");
    assert_eq!(out["finished_at"], "2023-11-14T22:13:22Z");
    for hidden in ["session", "last_report", "questions", "previous_attempt", "can_reorder"] {
        assert!(out.get(hidden).is_none(), "{hidden}");
    }
    assert!(out["allowed_actions"].is_array() && out["can_edit"].is_boolean() && out["cascade"].is_object());
}

#[test]
fn what_an_agent_wrote_is_marked_untrusted() {
    assert_eq!(details(&board(|_| {}))["untrusted"], true);
}

#[test]
fn the_pending_question_shows_its_id_text_and_options_only() {
    let mut v = board(|_| {});
    v.question = Some(TaskQuestion {
        id: "q-1-1".into(),
        attempt: 1,
        text: "Which database?".into(),
        options: vec!["Postgres".into(), "SQLite".into()],
        asked_at: 0,
        answered_at: None,
        answer: None,
    });
    let out = details(&v);
    assert_eq!(out["question"], json!({"id": "q-1-1", "text": "Which database?", "options": ["Postgres", "SQLite"]}));
}

#[test]
fn the_worktree_shows_the_branch_but_not_the_path() {
    let v = board(|b| {
        b.task_mut("t-1").unwrap().worktree = Some(crate::board::WorktreeInfo {
            path: "C:/secret/place".into(),
            branch: "task/login".into(),
            base: "origin/main".into(),
            setup_done: true,
        });
    });
    let out = details(&v);
    assert_eq!(out["worktree"], json!({"branch": "task/login", "base": "origin/main"}));
    assert!(!out.to_string().contains("secret"));
}

#[test]
fn a_normal_task_arrives_whole_without_more() {
    let v = board(|b| {
        let t = b.task_mut("t-1").unwrap();
        t.task = "Add the login form with validation. ".repeat(80);
        t.set_note(Some("Started after the design review."));
        t.set_state(finished(report(&"The form works. ".repeat(30), 3, |i| format!("Check case {i}"))));
    });
    let out = details(&v);
    assert!(out.get("more").is_none(), "{}", out["more"]);
    assert_eq!(out, full_card(&v));
}

#[test]
fn a_huge_task_fits_says_what_is_missing_and_reads_back_exactly() {
    let v = board(|b| {
        let t = b.task_mut("t-1").unwrap();
        t.task = (0..20_000).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        t.depends_on = (0..60).map(|i| format!("dependency-task-{i}")).collect();
        t.set_state(finished(report(&"Summary. ".repeat(556), 80, |i| format!("Follow up {i}: {}", "x".repeat(200)))));
    });
    let out = details(&v);
    let more = out["more"].as_object().unwrap();
    for field in ["task", "report.summary", "report.follow_ups", "report.tests", "depends_on"] {
        assert!(more.contains_key(field), "{field} missing from {more:?}");
    }
    assert_eq!(more["task"]["total_chars"], 20_000);
    assert_eq!(more["report.follow_ups"]["total_items"], 80);
    assert_eq!(more["depends_on"]["total_items"], 60);
    assert_eq!(
        (out["status"].as_str(), out["report"]["commit"].as_str(), out["report"]["verdict"].as_str()),
        (Some("done"), Some("abc1234"), Some("issues")),
        "the short facts survive"
    );
    assert_eq!(read_all(&v), full_card(&v));
}

#[test]
fn quotes_newlines_and_multibyte_text_fit_and_read_back_exactly() {
    let nasty = "\"\n€😀\\".repeat(2000);
    let v = board(|b| {
        let t = b.task_mut("t-1").unwrap();
        t.task = nasty.clone();
        t.set_note(Some(&nasty));
        t.set_state(finished(report(&nasty, 30, |i| format!("{i}\"{}", "\n😀".repeat(i * 40)))));
    });
    assert!(details(&v)["more"].as_object().is_some_and(|m| m.len() >= 3));
    assert_eq!(read_all(&v), full_card(&v));
}

#[test]
fn an_item_too_long_for_a_page_is_read_on_its_own() {
    let control = "\u{1}".repeat(2000);
    let v = board(|b| {
        let t = b.task_mut("t-1").unwrap();
        t.set_state(finished(report("Done.", 3, |_| control.clone())));
    });
    let first = page(&v, "report.follow_ups", 0).unwrap();
    assert_eq!(first["next_offset"], 1);
    assert!(first["more"]["report.follow_ups.0"]["next_offset"].as_u64().is_some(), "{first}");
    let tests = page(&v, "report.tests", 2).unwrap();
    assert!(tests.get("next_offset").is_none());
    assert_eq!(tests["items"][0]["command"], "cargo test 2");
    assert!(tests["more"].get("report.tests.2.details").is_some(), "{tests}");
    assert_eq!(read_all(&v), full_card(&v));
}

#[test]
fn a_page_of_a_short_field_is_the_whole_field() {
    let v = board(|b| b.task_mut("t-1").unwrap().depends_on = vec!["a".into(), "b".into()]);
    let out = page(&v, "task", 0).unwrap();
    assert_eq!(
        out,
        json!({
            "task_id": "t-1", "field": "task", "offset": 0, "untrusted": true,
            "content": "Build the sign-in page", "total_chars": 22
        })
    );
    let ids = page(&v, "depends_on", 1).unwrap();
    assert_eq!((ids["items"].clone(), ids["total_items"].as_u64()), (json!(["b"]), Some(2)));
    assert_eq!(page(&v, "waiting_on", 0).unwrap()["items"], json!(["a", "b"]));
    assert_eq!(page(&v, "cascade.running", 0).unwrap()["items"], json!([]));
}

#[test]
fn a_field_the_task_does_not_have_or_an_offset_past_its_end_is_an_error() {
    let v = board(|_| {});
    assert_eq!(page(&v, "report.summary", 0).unwrap_err(), "This task has no `report.summary`.");
    assert_eq!(page(&v, "question.text", 0).unwrap_err(), "This task has no `question.text`.");
    assert_eq!(page(&v, "task", 22).unwrap_err(), "`offset` 22 is beyond the end of `task`, which has 22 characters.");
    assert_eq!(
        page(&v, "depends_on", 1).unwrap_err(),
        "`offset` 1 is beyond the end of `depends_on`, which has 0 items."
    );
}

#[test]
fn a_short_task_has_no_more() {
    assert!(details(&board(|_| {})).get("more").is_none());
}
