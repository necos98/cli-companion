//! Correzioni del revisore: posizione in coda, cancello, riscrittura delle
//! dipendenze, `blocks`, errori e atomicita'.

use super::*;
use serde_json::json;

fn spec(id: &str, kind: TaskKind, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        kind,
        ..TaskSpec::default()
    }
}

fn work(id: &str, deps: &[&str]) -> TaskSpec {
    spec(id, TaskKind::Work, deps)
}

/// Board con i task dati, nell'ordine, ciascuno con lo stato indicato.
fn board(tasks: &[(TaskSpec, TaskStatus)]) -> Board {
    let mut b = Board::new("ws-1");
    for (s, status) in tasks {
        b.create_task(s.clone(), SpecContext::default(), 1).unwrap();
        b.tasks.last_mut().unwrap().set_status(*status);
    }
    b
}

fn ids(b: &Board) -> Vec<&str> {
    b.tasks.iter().map(|t| t.id.as_str()).collect()
}

fn deps(b: &Board, id: &str) -> Vec<String> {
    b.task(id).unwrap().depends_on.clone()
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// A (done), R = review di A (running), C non c'entra, B dipende da R, D dipende da B.
fn scenario() -> Board {
    use TaskStatus::{Done, Queued, Running};
    board(&[
        (work("a", &[]), Done),
        (spec("r", TaskKind::Review, &["a"]), Running),
        (work("c", &[]), Queued),
        (work("b", &["r"]), Queued),
        (work("d", &["b"]), Queued),
    ])
}

fn fixes() -> Vec<TaskSpec> {
    vec![work("f", &["a"]), spec("r2", TaskKind::Review, &["f"])]
}

fn queue(b: &mut Board, specs: Vec<TaskSpec>, blocks: &[&str]) -> Result<ReviewFixes, String> {
    b.queue_review_fixes("r", specs, &strings(blocks), SpecContext::default(), 9)
}

#[test]
fn the_fixes_go_before_the_first_queued_task() {
    let mut b = scenario();
    let out = queue(&mut b, fixes(), &[]).unwrap();
    assert_eq!(out.tasks, ["f", "r2"]);
    assert_eq!(ids(&b), ["a", "r", "f", "r2", "c", "b", "d"]);
    for id in ["f", "r2"] {
        assert_eq!(b.task(id).unwrap().origin.as_deref(), Some("r"), "{id}");
    }
    assert_eq!(b.task("c").unwrap().origin, None);
}

#[test]
fn without_queued_tasks_the_fixes_go_at_the_end() {
    use TaskStatus::{Done, Running};
    let mut b = board(&[(work("a", &[]), Done), (spec("r", TaskKind::Review, &["a"]), Running)]);
    queue(&mut b, fixes(), &[]).unwrap();
    assert_eq!(ids(&b), ["a", "r", "f", "r2"]);
}

#[test]
fn the_gate_is_the_new_review_and_dependents_wait_for_it() {
    let mut b = scenario();
    let out = queue(&mut b, fixes(), &[]).unwrap();
    assert_eq!(deps(&b, "b"), ["r", "r2"]);
    // D dipende da R solo indirettamente: aspetta anche lui.
    assert_eq!(deps(&b, "d"), ["b", "r2"]);
    // C non dipende dal lavoro revisionato: resta com'e'.
    assert!(deps(&b, "c").is_empty());
    assert_eq!(out.waiting, ["b", "d"]);
}

#[test]
fn dependents_of_a_reviewed_task_wait_too() {
    use TaskStatus::{Done, Queued, Running};
    let mut b =
        board(&[(work("a", &[]), Done), (spec("r", TaskKind::Review, &["a"]), Running), (work("x", &["a"]), Queued)]);
    let out = queue(&mut b, fixes(), &[]).unwrap();
    assert_eq!(deps(&b, "x"), ["a", "r2"]);
    assert_eq!(out.waiting, ["x"]);
}

#[test]
fn running_done_and_failed_tasks_are_never_rewired() {
    use TaskStatus::{Done, Failed, Queued, Running};
    let mut b = board(&[
        (work("a", &[]), Done),
        (spec("r", TaskKind::Review, &["a"]), Running),
        (work("run", &["a"]), Running),
        (work("fin", &["a"]), Done),
        (work("bad", &["a"]), Failed),
        (work("q", &["a"]), Queued),
    ]);
    let out = queue(&mut b, fixes(), &[]).unwrap();
    for id in ["run", "fin", "bad"] {
        assert_eq!(deps(&b, id), ["a"], "{id}");
    }
    assert_eq!(out.waiting, ["q"]);
}

#[test]
fn a_proposal_without_a_review_gates_on_every_proposed_task() {
    let mut b = scenario();
    let out = queue(&mut b, vec![work("f1", &["a"]), work("f2", &["a"])], &[]).unwrap();
    assert_eq!(deps(&b, "b"), ["r", "f1", "f2"]);
    assert_eq!(out.tasks, ["f1", "f2"]);
}

#[test]
fn a_task_both_dependent_and_blocked_gets_the_gate_once() {
    let mut b = scenario();
    let out = queue(&mut b, fixes(), &["b"]).unwrap();
    assert_eq!(deps(&b, "b"), ["r", "r2"]);
    assert_eq!(out.waiting, ["b", "d"]);
}

#[test]
fn blocks_make_unrelated_queued_tasks_wait() {
    let mut b = scenario();
    let out = queue(&mut b, fixes(), &["c"]).unwrap();
    assert_eq!(deps(&b, "c"), ["r2"]);
    assert_eq!(out.waiting, ["c", "b", "d"]);
}

#[test]
fn invalid_blocks_are_rejected_and_the_board_is_unchanged() {
    let cases: [(&str, &str); 5] = [
        ("ghost", "blocks: unknown task 'ghost'"),
        ("a", "blocks: task 'a' is done, only queued tasks can be made to wait"),
        ("r", "blocks cannot contain the review itself ('r')"),
        ("f", "blocks cannot contain a task of this proposal ('f')"),
        ("r2", "blocks cannot contain a task of this proposal ('r2')"),
    ];
    for (block, expected) in cases {
        let mut b = scenario();
        let before = b.clone();
        assert_eq!(queue(&mut b, fixes(), &[block]).unwrap_err(), expected);
        assert_eq!(b, before, "{block}");
    }
}

#[test]
fn a_cycle_created_by_the_rewiring_fails_the_whole_call() {
    let mut b = scenario();
    let before = b.clone();
    // F dipende da B, che dopo il riallacciamento aspetta R2, che aspetta F.
    let cyclic = vec![work("f", &["b"]), spec("r2", TaskKind::Review, &["f"])];
    let err = queue(&mut b, cyclic, &[]).unwrap_err();
    assert!(err.contains("dependency cycle"), "{err}");
    assert_eq!(b, before);
}

#[test]
fn the_usual_validations_of_a_proposal_still_apply() {
    let mut b = scenario();
    let before = b.clone();
    assert!(queue(&mut b, vec![work("b", &[])], &[]).unwrap_err().contains("duplicate task id: b"));
    assert!(queue(&mut b, vec![], &[]).unwrap_err().contains("tasks is empty"));
    assert!(queue(&mut b, vec![work("x", &["ghost"])], &[]).unwrap_err().contains("unknown task 'ghost'"));
    let missing = b.queue_review_fixes("nope", fixes(), &[], SpecContext::default(), 1);
    assert_eq!(missing.unwrap_err(), "review task not found: nope");
    assert_eq!(b, before);
}

#[test]
fn ids_reserved_by_a_pending_proposal_are_refused() {
    let mut b = scenario();
    b.add_proposal("p-1".into(), "tab", None, vec![work("f", &[])], SpecContext::default(), 1).unwrap();
    assert!(queue(&mut b, fixes(), &[]).unwrap_err().contains("already used by a pending proposal"));
}

#[test]
fn too_many_dependencies_after_the_rewiring_are_refused() {
    let mut b = scenario();
    let pads: Vec<String> = (0..MAX_DEPENDS - 1).map(|i| format!("pad-{i}")).collect();
    for id in &pads {
        b.create_task(work(id, &[]), SpecContext::default(), 1).unwrap();
        b.tasks.last_mut().unwrap().set_status(TaskStatus::Done);
    }
    b.tasks.iter_mut().find(|t| t.id == "b").unwrap().depends_on.extend(pads);
    let before = b.clone();
    assert!(queue(&mut b, fixes(), &[]).unwrap_err().contains("too many tasks"));
    assert_eq!(b, before);
}

#[test]
fn the_reviewer_parse_accepts_blocks_and_the_session_parse_still_rejects_it() {
    let task = json!({"id": "f", "title": "T", "task": "x", "bot": "general"});
    let params = json!({"tasks": [task], "blocks": ["b", "c"]});
    let (specs, blocks) = parse_review_proposal(&params).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(blocks, ["b", "c"]);
    assert!(parse_review_proposal(&json!({"tasks": [task]})).unwrap().1.is_empty());
    assert_eq!(crate::board::parse_proposal(&params).unwrap_err(), "unknown field 'blocks' (allowed: tasks)");
}

#[test]
fn the_reviewer_parse_rejects_malformed_blocks_and_other_fields() {
    let task = json!({"id": "f", "title": "T", "task": "x", "bot": "general"});
    let err = |p: Value| parse_review_proposal(&p).unwrap_err();
    assert_eq!(err(json!({"tasks": [task], "blocks": "b"})), "blocks must be an array of task ids");
    assert!(err(json!({"tasks": [task], "blocks": [1]})).contains("blocks must be an array of task ids, got 1"));
    assert!(err(json!({"tasks": [task], "extra": 1})).contains("unknown field 'extra' (allowed: tasks, blocks)"));
    let many = vec!["x"; MAX_TASKS_PER_BOARD + 1];
    assert!(err(json!({"tasks": [task], "blocks": many})).contains("too many entries"));
}
