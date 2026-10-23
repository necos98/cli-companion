use super::texts::{self, EnvInfo};
use super::*;
use crate::workspaces::{DeliveryMode, MergeMethod, RepoConfig};
use serde_json::json;

fn spec(id: &str, deps: &[&str]) -> TaskSpec {
    TaskSpec {
        id: id.to_string(),
        title: format!("Task {id}"),
        task: format!("do {id}"),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..TaskSpec::default()
    }
}

fn board_with(specs: Vec<TaskSpec>) -> Board {
    let mut b = Board::new("ws-1");
    b.check_specs(&specs, &HashSet::new()).unwrap();
    b.push_specs(specs, 1);
    b
}

fn ids(b: &Board) -> Vec<&str> {
    b.tasks.iter().map(|t| t.id.as_str()).collect()
}

fn set_status(b: &mut Board, id: &str, s: TaskStatus) {
    b.task_mut(id).unwrap().status = s;
}

fn delivery(mode: DeliveryMode, parallel: bool) -> Delivery {
    Delivery { mode, parallel, merge_method: MergeMethod::Squash }
}

fn open(mode: DeliveryMode, parallel: bool) -> SchedCtx {
    SchedCtx { delivery: delivery(mode, parallel), gate_open: true }
}

// --- normalizzazione dei file della v0.11 ---

#[test]
fn v011_board_is_normalized_on_load() {
    let raw = json!({
        "workspace_id": "ws-1",
        "max_parallel": 40,
        "base_branch": "main",
        "base_branches": {"api": "dev"},
        "integration_branch": "integration/{date}",
        "integrations": {".": {"branch": "integration/x", "path": "p", "base": "main"}},
        "owner": "orch-1",
        "requests": [{"id": "r-1", "kind": "question", "status": "pending", "created_at": 1}],
        "request_seq": 1,
        "tasks": [
            {"id": "queued", "title": "q", "task": "t", "status": "queued", "held": true, "touches": ["src"], "priority": 5, "isolation": "none"},
            {"id": "running", "title": "r", "task": "t", "status": "running", "session": "auto-1", "worker_status": "running", "depends_on": ["queued"]},
            {"id": "review", "title": "v", "task": "t", "status": "review", "report": {"status": "completed", "summary": "done work"}},
            {"id": "blocked", "title": "b", "task": "t", "status": "blocked", "note": "stuck", "report": {"status": "blocked", "summary": "cannot"}},
            {"id": "failed", "title": "f", "task": "t", "status": "failed"},
            {"id": "cancelled", "title": "c", "task": "t", "status": "cancelled"},
            {"id": "done", "title": "d", "task": "t", "status": "done", "external": true, "orchestrator_note": "n", "depends_on": ["cancelled", "queued"]},
        ]
    })
    .to_string();
    let b = Board::from_json(&raw, 99).unwrap();
    assert_eq!(b.max_parallel, MAX_PARALLEL_LIMIT);
    assert_eq!(ids(&b), ["queued", "running", "review", "blocked", "failed", "done"], "cancelled is removed");
    let st = |id: &str| b.task(id).unwrap().status;
    assert_eq!(st("queued"), TaskStatus::Queued);
    assert_eq!(st("running"), TaskStatus::Failed);
    assert_eq!(st("review"), TaskStatus::Failed);
    assert_eq!(st("blocked"), TaskStatus::Failed);
    assert_eq!(st("failed"), TaskStatus::Failed);
    assert_eq!(st("done"), TaskStatus::Done);
    let running = b.task("running").unwrap();
    assert_eq!(running.note.as_deref(), Some(INTERRUPTED_NOTE));
    assert_eq!((running.session.as_deref(), running.worker_status, running.finished_at), (None, None, Some(99)));
    assert_eq!(
        b.task("review").unwrap().note.as_deref(),
        Some("completed but not integrated (board v0.11): retry to let the worker deliver it")
    );
    assert_eq!(b.task("review").unwrap().report.as_ref().unwrap().summary, "done work");
    assert_eq!(b.task("blocked").unwrap().note.as_deref(), Some("stuck"));
    assert_eq!(b.task("blocked").unwrap().report.as_ref().unwrap().status, "failed");
    // Il cancelled sparisce dai depends_on degli altri.
    assert_eq!(b.task("done").unwrap().depends_on, ["queued"]);
    // I campi rimossi non tornano al salvataggio.
    let out = serde_json::to_string(&b).unwrap();
    for gone in ["owner", "requests", "integrations", "base_branch", "touches", "orchestrator_note", "held", "priority", "isolation"] {
        assert!(!out.contains(&format!("\"{gone}\"")), "{gone} in {out}");
    }
    assert!(b.proposals.is_empty());
    assert!(Board::from_json("{\"tasks\":[]}", 1).is_err(), "workspace_id missing");
    assert!(Board::from_json("zzz", 1).is_err());
}

#[test]
fn a_current_board_roundtrips() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &["a"])]);
    b.add_proposal("p-1".into(), "tab-1", vec![spec("c", &[])], 5).unwrap();
    let back = Board::from_json(&serde_json::to_string(&b).unwrap(), 9).unwrap();
    assert_eq!(back.tasks, b.tasks);
    assert_eq!(back.proposals, b.proposals);
    assert_eq!(back.max_parallel, DEFAULT_MAX_PARALLEL);
}

// --- validazione ---

#[test]
fn ids_and_branches() {
    for ok in ["a", "0", "a-b-1", &"a".repeat(40)] {
        assert!(valid_id(ok), "{ok}");
    }
    for bad in ["", "-a", "A", "a_b", "a b", &"a".repeat(41), "è"] {
        assert!(!valid_id(bad), "{bad}");
    }
    assert!(valid_branch("release/1.0") && !valid_branch("-x") && !valid_branch("a..b") && !valid_branch("a b"));
    assert_eq!(normalize_repo(" ./api/ ").unwrap(), Some("api".to_string()));
    assert_eq!(normalize_repo(".").unwrap(), None);
    for bad in ["", "../x", "/abs", "C:\\x", "a/../b"] {
        assert!(normalize_repo(bad).is_err(), "{bad}");
    }
}

#[test]
fn ids_are_generated_from_the_title() {
    let none = |_: &str| false;
    assert_eq!(id_from_title("Modalità di consegna", &none), "modalit-di-consegna");
    assert_eq!(id_from_title("  --Fix: the BUG!!  ", &none), "fix-the-bug");
    let long = id_from_title(&"word ".repeat(30), &none);
    assert!(valid_id(&long) && long.len() <= ID_MAX_LEN, "{long}");
    let taken: HashSet<&str> = ["fix", "fix-2"].into();
    assert_eq!(id_from_title("Fix", &|id| taken.contains(id)), "fix-3");
    // Suffisso anche su un id al limite dei 40 caratteri.
    let full = "a".repeat(40);
    let t2: HashSet<String> = [full.clone()].into();
    let id = id_from_title(&full, &|id| t2.contains(id));
    assert!(valid_id(&id) && id.len() <= 40 && id.ends_with("-2"), "{id}");
    // Titolo senza caratteri ASCII: task-<n>.
    let t3: HashSet<&str> = ["task-1"].into();
    assert_eq!(id_from_title("日本語", &|id| t3.contains(id)), "task-2");
    assert_eq!(id_from_title("!!!", &none), "task-1");
}

const ID_MAX_LEN: usize = spec::ID_MAX;

#[test]
fn parse_spec_rules() {
    let s = parse_spec(&json!({"id":"a","title":" T ","task":" x ","depends_on":["b","b"],"repo":"./api","model":"sonnet"}), None).unwrap();
    assert_eq!((s.id.as_str(), s.title.as_str(), s.task.as_str()), ("a", "T", "x"));
    assert_eq!(s.depends_on, ["b"]);
    assert_eq!((s.repo.as_deref(), s.model.as_deref()), (Some("api"), Some("sonnet")));
    // Senza id: da generare.
    assert_eq!(parse_spec(&json!({"title":"T","task":"x"}), None).unwrap().id, "");
    for bad in [
        json!({"id":"a","task":"x"}),
        json!({"id":"a","title":"T"}),
        json!({"id":"A","title":"T","task":"x"}),
        json!({"id":"a","title":"x".repeat(81),"task":"x"}),
        json!({"id":"a","title":"T","task":"   "}),
        json!({"id":"a","title":"T","task":"x","touches":["src"]}),
        json!({"id":"a","title":"T","task":"x","held":true}),
        json!({"id":"a","title":"T","task":"x","depends_on":["BAD"]}),
        json!({"id":"a","title":"T","task":"x","depends_on":"b"}),
        json!({"id":"a","title":"T","task":"x","model":"a b"}),
        json!({"id":"a","title":"T","task":"x","repo":"../x"}),
        json!({"id":"a","title":"T","task":"x","repo":3}),
    ] {
        assert!(parse_spec(&bad, None).is_err(), "{bad}");
    }
    // Modifica parziale: l'id non cambia, null azzera repo e model.
    let base = TaskSpec { repo: Some("api".into()), model: Some("opus".into()), ..spec("a", &["b"]) };
    let p = parse_spec(&json!({"id":"a","title":"New"}), Some(&base)).unwrap();
    assert_eq!((p.title.as_str(), p.task.as_str(), p.repo.as_deref()), ("New", "do a", Some("api")));
    let p = parse_spec(&json!({"repo":null,"model":null,"depends_on":null}), Some(&base)).unwrap();
    assert_eq!((p.repo, p.model, p.depends_on), (None, None, Vec::new()));
    assert!(parse_spec(&json!({"id":"zz"}), Some(&base)).is_err());
}

#[test]
fn proposals_are_parsed_and_limited() {
    let ok = parse_proposal(&json!({"tasks":[{"id":"a","title":"T","task":"x"}]})).unwrap();
    assert_eq!(ok.len(), 1);
    assert!(parse_proposal(&json!({})).unwrap_err().contains("tasks is missing"));
    assert!(parse_proposal(&json!({"tasks":[]})).unwrap_err().contains("empty"));
    assert!(parse_proposal(&json!({"tasks":[{"title":"T","task":"x"}]})).unwrap_err().contains("id is missing"));
    assert!(parse_proposal(&json!({"tasks":[{"id":"a","title":"T","task":"x"}],"max_parallel":2})).unwrap_err().contains("unknown field"));
    let many: Vec<Value> = (0..21).map(|i| json!({"id":format!("t{i}"),"title":"T","task":"x"})).collect();
    assert!(parse_proposal(&json!({"tasks":many})).unwrap_err().contains("too many"));
    let max: Vec<Value> = (0..20).map(|i| json!({"id":format!("t{i}"),"title":"T","task":"x"})).collect();
    assert_eq!(parse_proposal(&json!({"tasks":max})).unwrap().len(), 20);
}

#[test]
fn check_specs_validates_ids_graph_and_limits() {
    let b = board_with(vec![spec("a", &[]), spec("b", &["a"])]);
    let none = HashSet::new();
    // Dipendenze verso la proposta stessa o task esistenti.
    assert!(b.check_specs(&[spec("c", &["a", "d"]), spec("d", &[])], &none).is_ok());
    // Cicli, auto-dipendenze, dipendenze sconosciute, id duplicati.
    assert!(b.check_specs(&[spec("c", &["d"]), spec("d", &["c"])], &none).unwrap_err().contains("dependency cycle"));
    assert!(b.check_specs(&[spec("c", &["c"])], &none).unwrap_err().contains("itself"));
    assert!(b.check_specs(&[spec("c", &["zz"])], &none).unwrap_err().contains("unknown task"));
    assert!(b.check_specs(&[spec("a", &[])], &none).unwrap_err().contains("duplicate"));
    assert!(b.check_specs(&[spec("c", &[]), spec("c", &[])], &none).unwrap_err().contains("duplicate"));
    assert!(b.check_specs(&[], &none).is_err());
    // Id gia' usato da un'altra proposta pendente.
    let reserved: HashSet<&str> = ["x"].into();
    assert!(b.check_specs(&[spec("x", &[])], &reserved).unwrap_err().contains("pending proposal"));
    // Limite della board.
    let mut full = Board::new("w");
    for i in 0..MAX_TASKS_PER_BOARD {
        full.tasks.push(Task::from_spec(spec(&format!("t{i}"), &[]), 0));
    }
    assert!(full.check_specs(&[spec("extra", &[])], &none).unwrap_err().contains("too many tasks"));
}

#[test]
fn proposals_accept_discard_and_revalidate() {
    let mut b = board_with(vec![spec("a", &[])]);
    b.add_proposal("p-1".into(), "tab-1", vec![spec("b", &["a"]), spec("c", &["b"])], 5).unwrap();
    // Un id usato da una proposta pendente non si ripropone (ne' si crea a mano).
    assert!(b.add_proposal("p-2".into(), "tab-1", vec![spec("b", &[])], 6).is_err());
    assert!(b.create_task(spec("c", &[]), 6).is_err());
    assert!(b.id_taken("b") && b.id_taken("a") && !b.id_taken("zz"));
    b.add_proposal("p-2".into(), "tab-2", vec![spec("d", &[])], 6).unwrap();
    // Accetta: in fondo alla coda, nell'ordine della proposta.
    assert_eq!(b.accept_proposal("p-1", 7).unwrap(), ["b", "c"]);
    assert_eq!(ids(&b), ["a", "b", "c"]);
    assert_eq!(b.proposals.len(), 1);
    assert!(b.tasks.iter().all(|t| t.status == TaskStatus::Queued && t.agent == "claude-code"));
    // Scarta.
    b.discard_proposal("p-2", 8).unwrap();
    assert!(b.proposals.is_empty());
    assert!(b.accept_proposal("p-2", 9).unwrap_err().contains("proposal not found"));
    assert!(b.discard_proposal("p-9", 9).is_err());
    // Rivalidazione: un id nel frattempo occupato fa fallire l'accettazione e la proposta resta.
    b.add_proposal("p-3".into(), "tab-1", vec![spec("e", &[])], 10).unwrap();
    b.create_task(spec("e", &[]), 11).unwrap_err(); // riservato dalla proposta
    b.tasks.push(Task::from_spec(spec("e", &[]), 11)); // occupato aggirando i controlli (es. altra via)
    assert!(b.accept_proposal("p-3", 12).unwrap_err().contains("duplicate"));
    assert_eq!(b.proposals.len(), 1);
}

#[test]
fn pending_proposals_are_capped() {
    let mut b = Board::new("w");
    for i in 0..MAX_PENDING_PROPOSALS {
        b.add_proposal(format!("p-{i}"), "t", vec![spec(&format!("t{i}"), &[])], 0).unwrap();
    }
    assert!(b.add_proposal("p-x".into(), "t", vec![spec("zz", &[])], 0).unwrap_err().contains("too many pending"));
}

// --- coda: ordine, retry, delete, update ---

#[test]
fn queue_order_move_and_to_top() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &[]), spec("c", &[]), spec("d", &[])]);
    b.move_task("d", Some("b"), 2).unwrap();
    assert_eq!(ids(&b), ["a", "d", "b", "c"]);
    b.move_task("a", None, 3).unwrap();
    assert_eq!(ids(&b), ["d", "b", "c", "a"]);
    b.task_to_top("c", 4).unwrap();
    assert_eq!(ids(&b), ["c", "d", "b", "a"]);
    b.move_task("c", Some("c"), 5).unwrap();
    assert_eq!(ids(&b), ["c", "d", "b", "a"]);
    // Solo i queued si riordinano, e solo davanti a un queued.
    set_status(&mut b, "d", TaskStatus::Running);
    assert!(b.move_task("d", None, 6).is_err());
    assert!(b.task_to_top("d", 6).is_err());
    assert!(b.move_task("b", Some("d"), 6).unwrap_err().contains("not queued"));
    assert!(b.move_task("zz", None, 6).is_err());
    // to_top: davanti al primo queued, non davanti ai task in corso.
    b.task_to_top("a", 7).unwrap();
    assert_eq!(ids(&b), ["a", "c", "d", "b"]);
    set_status(&mut b, "a", TaskStatus::Done);
    b.task_to_top("b", 8).unwrap();
    assert_eq!(ids(&b), ["a", "b", "c", "d"]);
}

#[test]
fn retry_requeues_at_the_bottom_and_keeps_the_previous_outcome() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &[])]);
    assert!(b.retry_task("a", 2).unwrap_err().contains("only failed"));
    {
        let t = b.task_mut("a").unwrap();
        t.status = TaskStatus::Failed;
        t.attempts = 1;
        t.note = Some("tests failed".into());
        t.report = Some(build_report("failed", "two tests red", ReportFields::default(), None));
        t.last_report = Some("two tests red".into());
        t.worktree = Some(WorktreeInfo { path: "p".into(), branch: "task/a".into(), base: "main".into(), setup_done: true });
    }
    b.retry_task("a", 3).unwrap();
    assert_eq!(ids(&b), ["b", "a"]);
    let t = b.task("a").unwrap();
    assert_eq!((t.status, t.attempts, t.report.is_none(), t.note.is_none()), (TaskStatus::Queued, 1, true, true));
    assert!(t.worktree.is_some(), "worktree e branch si riusano");
    let p = t.previous_attempt.as_ref().unwrap();
    assert_eq!((p.summary.as_deref(), p.note.as_deref()), (Some("two tests red"), Some("tests failed")));
    // Al nuovo avvio il tentativo precedente resta per get_task; alla fine sparisce.
    b.start_task("a", 4);
    assert_eq!(b.task("a").unwrap().attempts, 2);
    assert!(b.task("a").unwrap().previous_attempt.is_some());
    assert!(b.finish_running("a", None, TaskStatus::Done, None, None, 5));
    assert!(b.task("a").unwrap().previous_attempt.is_none());
}

#[test]
fn delete_rules() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &["a"]), spec("c", &["a"])]);
    // Un queued con dipendenti non done non si elimina.
    assert!(b.delete_task("a", 2).unwrap_err().contains("b, c depend on it"));
    set_status(&mut b, "b", TaskStatus::Done);
    assert!(b.delete_task("a", 2).is_err());
    b.delete_task("c", 2).unwrap();
    // Un dipendente done non blocca e perde il riferimento.
    let removed = b.delete_task("a", 3).unwrap();
    assert_eq!(removed.id, "a");
    assert_eq!(ids(&b), ["b"]);
    assert!(b.task("b").unwrap().depends_on.is_empty());
    // Running e done non si eliminano; failed si'.
    set_status(&mut b, "b", TaskStatus::Running);
    assert!(b.delete_task("b", 4).unwrap_err().contains("only queued or failed"));
    set_status(&mut b, "b", TaskStatus::Failed);
    assert!(b.delete_task("b", 4).is_ok());
    assert!(b.delete_task("b", 4).is_err());
}

#[test]
fn update_rules() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &[])]);
    b.update_task("b", &json!({"id":"b","title":"New","depends_on":["a"],"model":"opus"}), 2).unwrap();
    let t = b.task("b").unwrap();
    assert_eq!((t.title.as_str(), t.depends_on.as_slice(), t.model.as_deref()), ("New", &["a".to_string()][..], Some("opus")));
    // Cicli e dipendenze sconosciute lasciano la board com'era.
    let before = b.tasks.clone();
    assert!(b.update_task("a", &json!({"depends_on":["b"]}), 3).unwrap_err().contains("cycle"));
    assert!(b.update_task("a", &json!({"depends_on":["zz"]}), 3).is_err());
    assert!(b.update_task("a", &json!({"id":"a"}), 3).unwrap_err().contains("no fields"));
    assert_eq!(b.tasks, before);
    // Solo queued o failed; il repo non cambia con un worktree.
    set_status(&mut b, "a", TaskStatus::Running);
    assert!(b.update_task("a", &json!({"title":"x"}), 3).unwrap_err().contains("only queued or failed"));
    set_status(&mut b, "a", TaskStatus::Failed);
    b.update_task("a", &json!({"repo":"api"}), 3).unwrap();
    b.task_mut("a").unwrap().worktree = Some(WorktreeInfo { path: "p".into(), branch: "task/a".into(), base: "main".into(), setup_done: true });
    assert!(b.update_task("a", &json!({"repo":null}), 3).unwrap_err().contains("worktree"));
    assert!(b.update_task("a", &json!({"title":"ok"}), 3).is_ok());
}

// --- scheduler ---

fn ids_of(v: Vec<String>) -> Vec<String> {
    v
}

#[test]
fn scheduler_serial_runs_one_task_per_repository() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &[]), spec("c", &[])]);
    b.task_mut("c").unwrap().repo = Some("api".into());
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::Push, true), (DeliveryMode::Pr, false), (DeliveryMode::LocalMerge, false)] {
        // a (root) e c (api) partono, b aspetta la root.
        assert_eq!(ids_of(schedule(&b, &open(mode, parallel))), ["a", "c"], "{mode:?}");
    }
    // Con a in corso la root e' occupata: parte solo c.
    b.start_task("a", 1);
    assert_eq!(schedule(&b, &open(DeliveryMode::Commit, false)), ["c"]);
    // Entro max_parallel comunque.
    b.max_parallel = 1;
    assert!(schedule(&b, &open(DeliveryMode::Commit, false)).is_empty());
}

#[test]
fn scheduler_parallel_worktrees_fill_max_parallel_in_queue_order() {
    let mut b = board_with((0..6).map(|i| spec(&format!("t{i}"), &[])).collect());
    b.max_parallel = 3;
    for mode in [DeliveryMode::Pr, DeliveryMode::LocalMerge] {
        assert_eq!(schedule(&b, &open(mode, true)), ["t0", "t1", "t2"]);
    }
    b.start_task("t0", 1);
    assert_eq!(schedule(&b, &open(DeliveryMode::Pr, true)), ["t1", "t2"]);
    // L'ordine della coda decide.
    b.task_to_top("t5", 2).unwrap();
    assert_eq!(schedule(&b, &open(DeliveryMode::Pr, true)), ["t5", "t1"]);
}

#[test]
fn scheduler_respects_dependencies_pause_and_the_gate() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &["a"]), spec("c", &["a", "b"])]);
    let ctx = open(DeliveryMode::Pr, true);
    assert_eq!(schedule(&b, &ctx), ["a"]);
    // Le dipendenze devono essere done: running o failed non bastano.
    b.start_task("a", 1);
    assert!(schedule(&b, &ctx).is_empty());
    set_status(&mut b, "a", TaskStatus::Failed);
    assert!(schedule(&b, &ctx).is_empty());
    set_status(&mut b, "a", TaskStatus::Done);
    assert_eq!(schedule(&b, &ctx), ["b"]);
    set_status(&mut b, "b", TaskStatus::Done);
    assert_eq!(schedule(&b, &ctx), ["c"]);
    // Pausa e fascia chiusa: niente di nuovo.
    b.paused = true;
    assert!(schedule(&b, &ctx).is_empty());
    b.paused = false;
    assert!(schedule(&b, &SchedCtx { gate_open: false, ..ctx }).is_empty());
    // waiting_on nella vista, solo per i queued.
    let mut b = board_with(vec![spec("a", &[]), spec("b", &["a"])]);
    let v = b.view_with(Delivery::default(), false);
    assert_eq!(v.tasks[1].waiting_on, ["a"]);
    assert!(v.tasks[0].waiting_on.is_empty());
    set_status(&mut b, "a", TaskStatus::Done);
    assert!(b.view_with(Delivery::default(), false).tasks[1].waiting_on.is_empty());
}

#[test]
fn attention_follows_the_worker_status() {
    let mut b = board_with(vec![spec("a", &[])]);
    b.start_task("a", 1);
    let attention = |b: &Board| b.view_with(Delivery::default(), false).tasks[0].attention;
    assert!(!attention(&b));
    for (ws, expected) in [
        (WorkerStatus::NeedsInput, true),
        (WorkerStatus::Idle, true),
        (WorkerStatus::Running, false),
        (WorkerStatus::Background, false),
    ] {
        b.task_mut("a").unwrap().worker_status = Some(ws);
        assert_eq!(attention(&b), expected, "{ws:?}");
    }
    // Un task non in corso non richiede attenzione.
    set_status(&mut b, "a", TaskStatus::Failed);
    b.task_mut("a").unwrap().worker_status = Some(WorkerStatus::Idle);
    assert!(!attention(&b));
}

#[test]
fn view_json_shape_and_list_rows() {
    let mut b = board_with(vec![spec("a", &[]), spec("b", &["a"])]);
    b.start_task("a", 1);
    b.task_mut("a").unwrap().worker_status = Some(WorkerStatus::Idle);
    let view = b.view_with(delivery(DeliveryMode::Pr, true), true);
    let v = serde_json::to_value(&view).unwrap();
    for k in ["workspace_id", "rev", "max_parallel", "paused", "updated_at", "tasks", "proposals", "schedule_blocked", "delivery"] {
        assert!(v.get(k).is_some(), "{k}");
    }
    assert_eq!(v["schedule_blocked"], true);
    assert_eq!(v["delivery"], json!({"mode":"pr","parallel":true,"merge_method":"squash"}));
    assert_eq!(v["tasks"][0]["attention"], true);
    assert_eq!(v["tasks"][0]["worker_status"], "idle");
    assert_eq!(v["tasks"][1]["waiting_on"], json!(["a"]));
    assert_eq!(v["tasks"][1]["status"], "queued");
    let list = view.to_list_json(None);
    assert_eq!(list["counts"], json!({"queued":1,"running":1,"done":0,"failed":0}));
    assert_eq!(list["tasks"][0]["attention"], true);
    assert_eq!(list["tasks"][1]["waiting_on"], json!(["a"]));
    assert!(list["tasks"][0].get("task").is_none(), "no specification text in the rows");
    assert_eq!(view.to_list_json(Some(&[TaskStatus::Running]))["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(list["proposals_pending"], 0);
    // to_json senza testo.
    assert!(view.tasks[0].to_json(false).get("task").is_none() && view.tasks[0].to_json(true).get("task").is_some());
}

// --- report ---

#[test]
fn report_fields_are_validated() {
    let f = parse_report_fields(&json!({
        "commit":" abc1234 ","pr_url":"https://github.com/o/r/pull/7","files_changed":["a.rs"],
        "tests":[{"command":"cargo test","result":"passed"}],"follow_ups":["docs"]
    }))
    .unwrap();
    assert_eq!((f.commit.as_deref(), f.pr_url.as_deref()), (Some("abc1234"), Some("https://github.com/o/r/pull/7")));
    assert_eq!((f.files_changed.len(), f.tests.len(), f.follow_ups.len()), (1, 1, 1));
    assert_eq!(parse_report_fields(&json!({})).unwrap(), ReportFields::default());
    for bad in [
        json!({"commit":"a b"}),
        json!({"pr_url":"ftp://x"}),
        json!({"pr_url":"https://a b"}),
        json!({"tests":[{"command":"x","result":"maybe"}]}),
        json!({"tests":"x"}),
        json!({"files_changed":"a"}),
    ] {
        assert!(parse_report_fields(&bad).is_err(), "{bad}");
    }
    let r = build_report("completed", "ok", f, Some("x".repeat(DIFFSTAT_MAX + 10)));
    assert_eq!(r.diffstat.unwrap().chars().count(), DIFFSTAT_MAX);
}

// --- testi per il worker ---

fn env<'a>(d: &'a Delivery, task: &'a Task, cfg: Option<&'a RepoConfig>) -> EnvInfo<'a> {
    let branches = d.uses_branches();
    EnvInfo {
        delivery: d,
        root: "C:\\proj",
        repo_dir: "C:\\proj",
        workdir: if d.worktrees() { "C:\\proj\\.worktrees\\a" } else { "C:\\proj" },
        task,
        branch: branches.then_some("task/a"),
        base: branches.then_some("main"),
        repo_cfg: cfg,
    }
}

fn worktree_task(d: &Delivery) -> Task {
    let mut t = Task::from_spec(spec("a", &[]), 0);
    if d.worktrees() {
        t.worktree = Some(WorktreeInfo { path: "C:\\proj\\.worktrees\\a".into(), branch: "task/a".into(), base: "origin/main".into(), setup_done: true });
    }
    t
}

#[test]
fn delivery_steps_per_mode() {
    let cases: [(DeliveryMode, bool, &[&str], &[&str]); 6] = [
        (DeliveryMode::Pr, true, &["git rebase origin/main", "git push -u origin task/a", "gh pr create --base main --head task/a", "gh pr merge <pr-url> --squash", "--auto", "git push origin --delete task/a", "pr_url", "stays on task/a"], &["git switch"]),
        (DeliveryMode::Pr, false, &["git switch -c task/a origin/main", "gh pr create --base main", "git switch main && git pull --ff-only", "git branch -D task/a", "uncommitted changes"], &["worktree stays"]),
        (DeliveryMode::LocalMerge, true, &["git rebase main", "git -C \"C:\\proj\" merge --ff-only task/a", "main advanced"], &["gh pr", "git push"]),
        (DeliveryMode::LocalMerge, false, &["git switch -c task/a main", "git switch main", "git merge --ff-only task/a", "git rebase main task/a, then git switch main and retry the merge", "git branch -d task/a"], &["gh pr", "git push"]),
        (DeliveryMode::Push, true, &["git pull --rebase", "git push"], &["task/a", "gh pr"]),
        (DeliveryMode::Commit, false, &["Commit your work on the current branch", "Do not push"], &["git push", "pull --rebase", "task/a"]),
    ];
    for (mode, parallel, must, must_not) in cases {
        let d = delivery(mode, parallel);
        let t = worktree_task(&d);
        let text = texts::delivery_steps(&env(&d, &t, None));
        for m in must {
            assert!(text.contains(m), "{mode:?}/{parallel}: missing {m:?} in\n{text}");
        }
        for m in must_not {
            assert!(!text.contains(m), "{mode:?}/{parallel}: unexpected {m:?} in\n{text}");
        }
        assert!(text.contains("complete_task") && text.is_ascii());
    }
    // Il metodo di merge e il titolo (senza virgolette) arrivano nei passi PR.
    let d = Delivery { mode: DeliveryMode::Pr, parallel: true, merge_method: MergeMethod::Rebase };
    let mut t = worktree_task(&d);
    t.title = "Say \"hi\"".into();
    let text = texts::delivery_steps(&env(&d, &t, None));
    assert!(text.contains("--rebase") && text.contains("--title \"Say 'hi'\""), "{text}");
}

#[test]
fn environment_describes_where_and_how_the_worker_runs() {
    let d = delivery(DeliveryMode::Pr, true);
    let mut t = worktree_task(&d);
    t.repo = Some("api".into());
    let cfg = RepoConfig { copy_ignored: vec![".env".into()], setup: vec!["npm ci".into()], container_root: Some("/srv".into()), ..Default::default() };
    let text = texts::environment(&env(&d, &t, Some(&cfg)));
    for m in ["Workspace root: C:\\proj", "Repository: api", ".worktrees\\a", "branch task/a (created from origin/main)", "base branch: main", ".env", "npm ci", "/srv", "do NOT redo"] {
        assert!(text.contains(m), "missing {m:?} in\n{text}");
    }
    // Setup non finito: niente promessa che sia gia' pronto.
    t.worktree.as_mut().unwrap().setup_done = false;
    assert!(!texts::environment(&env(&d, &t, Some(&cfg))).contains("do NOT redo"));
    // In place: nessun worktree.
    let d = delivery(DeliveryMode::Commit, false);
    let t = worktree_task(&d);
    let text = texts::environment(&env(&d, &t, None));
    assert!(text.contains("No dedicated worktree") && text.contains("currently checked out") && !text.contains("base branch"), "{text}");
    let d = delivery(DeliveryMode::LocalMerge, false);
    assert!(texts::environment(&env(&d, &worktree_task(&d), None)).contains("create the branch yourself"));
}

#[test]
fn instructions_carry_rules_guardrails_and_the_working_folder() {
    let d = delivery(DeliveryMode::Commit, false);
    let t = worktree_task(&d);
    let text = texts::instructions(&env(&d, &t, None), "  never run the e2e suite  ");
    for m in ["Work only inside C:\\proj", "Commit only the files you changed", "complete_task", "status \"failed\"", "end your turn"] {
        assert!(text.contains(m), "missing {m:?}");
    }
    // Standard di ingegneria, poi le regole del workspace (che non lo allentano).
    let g = text.find("Engineering standard (non-negotiable)").expect("guardrails");
    let rules = text.find("Workspace rules (always apply").expect("rules");
    assert!(text.find("Background commands").unwrap() < g && g < rules);
    assert!(text[rules..].contains("never relax the security rules") && text.ends_with("never run the e2e suite"));
    assert!(texts::WORKER_GUARDRAILS.contains("Security is a blocker"));
    // Senza regole niente blocco.
    assert!(!texts::instructions(&env(&d, &t, None), "  ").contains("Workspace rules"));
}
