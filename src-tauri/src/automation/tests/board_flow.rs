//! Board dei task attraverso `dispatch` con l'host finto: submit_plan ->
//! scheduler -> worker -> review -> done, avvio fallito, cancel, proprietario,
//! riavvio.

use super::*;
use crate::board;

fn st_h() -> (AutomationState, FakeHost) {
    (AutomationState::default(), FakeHost::default())
}

fn board_json(st: &AutomationState, h: &FakeHost) -> Value {
    // Vista completa: tutte le colonne, poi il dettaglio di ogni task.
    let all = json!(["backlog", "ready", "running", "review", "done", "failed", "blocked", "cancelled"]);
    let mut b = call(st, h, ORCH, "get_board", json!({"columns": all})).unwrap();
    let ids: Vec<String> = b["tasks"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    let full: Vec<Value> =
        ids.iter().map(|id| call(st, h, ORCH, "get_task_details", json!({"id": id})).unwrap()["task"].clone()).collect();
    b["tasks"] = json!(full);
    b
}

fn task_of(board: &Value, id: &str) -> Value {
    board["tasks"].as_array().unwrap().iter().find(|t| t["id"] == id).cloned().unwrap_or(Value::Null)
}

fn session_of(st: &AutomationState, h: &FakeHost, id: &str) -> String {
    task_of(&board_json(st, h), id)["session"].as_str().expect("task con worker").to_string()
}

fn events(st: &AutomationState, h: &FakeHost, orch: &str) -> Vec<Value> {
    call(st, h, orch, "get_events", json!({})).unwrap()["events"].as_array().unwrap().clone()
}

fn kinds_of(ev: &[Value]) -> Vec<String> {
    ev.iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
}

fn t(id: &str, deps: &[&str]) -> Value {
    json!({"id": id, "task": format!("do {id}"), "depends_on": deps})
}

fn submit(st: &AutomationState, h: &FakeHost, tasks: Vec<Value>) -> Value {
    call(st, h, ORCH, "submit_plan", json!({"tasks": tasks})).unwrap()
}

fn finish(st: &AutomationState, h: &FakeHost, session: &str, status: &str) {
    call(st, h, session, "get_task", json!({})).unwrap();
    call(st, h, session, "complete_task", json!({"status": status, "summary": "fatto"})).unwrap();
}

fn wake_count(h: &FakeHost, orch: &str) -> usize {
    h.log().iter().filter(|l| l.starts_with(&format!("write:{orch}:")) && l.contains("200~")).count()
}

#[test]
fn submit_plan_runs_dependencies_through_review_and_done() {
    let (st, h) = st_h();
    *h.setup.lock().unwrap() = Some("npm ci".to_string());
    let board = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({"max_parallel": 2, "tasks": [
            {"id": "a", "task": "do a", "touches": ["src/a"]},
            {"id": "b", "task": "do b", "depends_on": ["a"]},
            {"id": "c", "task": "do c", "touches": ["src/a/deep"]},
        ]}),
    )
    .unwrap();
    assert_eq!(board["added"], json!(["a", "b", "c"]));
    assert_eq!(board["max_parallel"], 2);
    assert_eq!(board["owner"], ORCH);
    // a parte; b aspetta a; c confligge con a (area annidata).
    assert_eq!(task_of(&board, "a")["column"], "running");
    assert_eq!(task_of(&board, "a")["column"], "running");
    assert_eq!(task_of(&board, "a")["attempts"], 1);
    assert_eq!(task_of(&board, "b")["column"], "backlog");
    assert_eq!(task_of(&board, "c")["column"], "ready");
    assert!(task_of(&board, "a").get("task").is_none(), "il testo non e' nella risposta di submit_plan");

    // Worktree creato dall'app: base = branch corrente, .worktrees escluso.
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|rev-parse --abbrev-ref HEAD".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a main".to_string()), "{git:?}");
    assert_eq!(h.excludes.lock().unwrap().as_slice(), ["C:\\p\\.git\\info\\exclude|.worktrees/"]);
    let sp = h.spawns();
    assert_eq!(sp.len(), 1);
    assert_eq!(sp[0].cwd.as_deref(), Some("C:\\p\\.worktrees\\a"));
    assert_eq!(sp[0].parent_tab_id, ORCH);
    assert_eq!(sp[0].title, "do a");
    assert!(sp[0].command.contains("AcceptEdits"));
    let w = sp[0].tab_id.clone();

    // get_task: testo, id e ambiente (setup come primo passo).
    let task = call(&st, &h, &w, "get_task", json!({})).unwrap();
    assert_eq!(task["task"], "do a");
    assert_eq!(task["task_id"], "a");
    let env = task["environment"].as_str().unwrap();
    assert!(env.contains("npm ci") && env.contains("task/a (created from main)") && env.contains("src/a"), "{env}");
    assert!(env.contains("C:\\p\\.worktrees\\a"));
    assert_eq!(task_of(&board_json(&st, &h), "a")["worker_status"], "running");

    // complete_task strutturato: review + report + diffstat calcolato dall'app.
    call(
        &st,
        &h,
        &w,
        "complete_task",
        json!({"status": "completed", "summary": "ok", "commit": "abc1234", "files_changed": ["src/a/x.rs"],
               "tests": [{"command": "cargo test", "result": "passed"}], "follow_ups": ["docs"]}),
    )
    .unwrap();
    let b = board_json(&st, &h);
    let a = task_of(&b, "a");
    assert_eq!(a["status"], "review");
    assert_eq!(a["column"], "review");
    assert_eq!(a["report"]["status"], "completed");
    assert_eq!(a["report"]["commit"], "abc1234");
    assert_eq!(a["report"]["tests"][0]["result"], "passed");
    assert_eq!(a["report"]["files_changed"], json!(["src/a/x.rs"]));
    assert_eq!(a["report"]["follow_ups"], json!(["docs"]));
    assert!(a["report"]["diffstat"].as_str().unwrap().contains("2 files changed"));
    assert!(h.git_log().iter().any(|l| l == "C:\\p\\.worktrees\\a|diff --stat main...HEAD"), "{:?}", h.git_log());
    assert_eq!(a["last_report"], "ok");
    assert!(a["session"].is_null());
    // In review il dipendente non parte ancora; c (conflitto sparito) si'.
    assert_eq!(task_of(&b, "b")["column"], "backlog");
    assert_eq!(task_of(&b, "c")["column"], "running");

    // Eventi con l'id del task.
    let ev = events(&st, &h, ORCH);
    let completed = ev.iter().find(|e| e["kind"] == "completed").unwrap();
    assert_eq!(completed["task"], "a");
    assert_eq!(completed["session"], w);

    // L'orchestratore integra e segna done: pulizia e il dipendente parte.
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert_eq!(r["task"]["status"], "done");
    assert!(r.get("warnings").is_none());
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|branch -d task/a".to_string()), "{git:?}");
    let b = board_json(&st, &h);
    assert_eq!(task_of(&b, "b")["column"], "running");
    assert_eq!(task_of(&b, "a")["column"], "done");
    // La base e' riletta al momento dell'avvio del dipendente.
    let n = h.git_log().iter().filter(|l| l.ends_with("rev-parse --abbrev-ref HEAD")).count();
    assert_eq!(n, 3, "una lettura del branch per task avviato");
}

#[test]
fn board_is_published_on_every_change_in_order() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.task("a").unwrap().status, board::TaskStatus::Review);
    let ev = h.board_events.lock().unwrap().clone();
    assert!(ev.len() >= 3);
    assert_eq!(ev.last().unwrap().tasks[0].column, board::Column::Review);
    assert_eq!(ev.last().unwrap().workspace_id, "ws-1");
    // La revisione non torna indietro.
    let r = st.inner.lock().unwrap().boards["ws-1"].rev;
    assert_eq!(saved.rev, r);
}

#[test]
fn start_failure_marks_failed_and_wakes_the_owner() {
    let (st, h) = st_h();
    h.git_rule("worktree add", false, "", "fatal: '.worktrees/a' already exists");
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    let b = submit(&st, &h, vec![t("a", &[]), t("b", &[])]);
    // Falliscono entrambi (stessa regola git), nessun worker.
    assert_eq!(task_of(&b, "a")["column"], "failed");
    let a = task_of(&board_json(&st, &h), "a");
    assert!(a["note"].as_str().unwrap().contains("already exists"), "{a}");
    assert!(a["session"].is_null() && a["worker_status"].is_null());
    assert!(h.spawns().is_empty());
    let ev = events(&st, &h, ORCH);
    let failed: Vec<_> = ev.iter().filter(|e| e["kind"] == "start_failed").collect();
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0]["task"], "a");
    assert!(failed[0]["text"].as_str().unwrap().contains("could not be started"));
    assert_eq!(wake_count(&h, ORCH), 1, "start_failed e' azionabile");
    // Retry dopo aver sistemato git: parte.
    h.git_rules.lock().unwrap().clear();
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(r["task"]["attempts"], 2);
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "running");
    assert_eq!(h.spawns().len(), 1);
}

#[test]
fn command_launch_error_is_a_start_failure_too() {
    let (st, h) = st_h();
    let b = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [{"id": "a", "task": "x", "isolation": "none"}]})).unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    // Agente sconosciuto: rifiutato prima di toccare la board.
    let err = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [{"id": "z", "task": "x", "agent": "altro"}]})).unwrap_err();
    assert!(err.contains("unknown agent"), "{err}");
    assert!(task_of(&board_json(&st, &h), "z").is_null());
}

#[test]
fn cancelling_a_running_task_closes_its_tab_without_exited() {
    let (st, h) = st_h();
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![t("a", &[]), t("b", &["a"])]);
    let w = session_of(&st, &h, "a");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    events(&st, &h, ORCH);
    let before = wake_count(&h, ORCH);
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled", "note": "non serve"})).unwrap();
    assert_eq!(r["task"]["status"], "cancelled");
    assert_eq!(r["task"]["note"], "non serve");
    assert!(h.log().contains(&format!("close:{w}")));
    // Il tab si chiude: nessun evento `exited`, il task resta cancelled.
    st.run(&h, |r| r.tab_closed(&w, 100));
    assert!(events(&st, &h, ORCH).iter().all(|e| e["kind"] != "exited"));
    assert_eq!(wake_count(&h, ORCH), before);
    let b = board_json(&st, &h);
    assert_eq!(task_of(&b, "a")["status"], "cancelled");
    // Una dipendenza cancelled non sblocca il dipendente.
    assert_eq!(task_of(&b, "b")["column"], "backlog");
    assert_eq!(st.inner.lock().unwrap().list_sessions(ORCH)[0].status, WorkerStatus::Closed);
    // Cancel di nuovo / done da cancelled: errori leggibili.
    assert!(call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled"})).is_err());
    assert!(call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).is_err());
    assert!(call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "boh"})).is_err());
    assert!(call(&st, &h, ORCH, "set_task_status", json!({"id": "zz", "status": "done"})).is_err());
}

#[test]
fn tab_closed_or_session_end_without_report_fails_the_task() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &[]), t("c", &[])]);
    let (wa, wb) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"));
    call(&st, &h, &wa, "get_task", json!({})).unwrap();
    call(&st, &h, &wb, "get_task", json!({})).unwrap();
    // Tab chiuso dall'utente.
    st.run(&h, |r| r.tab_closed(&wa, 1));
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("closed"), "{a}");
    // session-end dell'agente.
    st.run(&h, |r| hook(r, &wb, "session-end", None, 2));
    assert_eq!(task_of(&board_json(&st, &h), "b")["status"], "failed");
    // Gli eventi exited portano il task.
    let ev = events(&st, &h, ORCH);
    let exited: Vec<_> = ev.iter().filter(|e| e["kind"] == "exited").collect();
    assert_eq!(exited.len(), 2);
    assert!(exited.iter().all(|e| e["task"].is_string()));
    // Il tab chiuso dopo l'exited non cambia nulla.
    st.run(&h, |r| r.tab_closed(&wb, 3));
    assert_eq!(task_of(&board_json(&st, &h), "b")["status"], "failed");
}

#[test]
fn closing_a_board_worker_with_close_session_fails_the_task_when_the_tab_closes() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    call(&st, &h, ORCH, "close_session", json!({"session": w})).unwrap();
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "running");
    st.run(&h, |r| r.tab_closed(&w, 5));
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "failed");
}

#[test]
fn stopped_without_report_keeps_the_task_running_and_idle() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    st.run(&h, |r| stop(r, &w, None, None, 1));
    st.run(&h, |r| stop(r, &w, Some(true), None, 2));
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "running");
    assert_eq!(a["worker_status"], "idle");
    let ev = events(&st, &h, ORCH);
    let e = ev.iter().find(|e| e["kind"] == "stopped_without_report").unwrap();
    assert_eq!(e["task"], "a");
    // L'orchestratore scrive al worker: riprende.
    call(&st, &h, ORCH, "send_message", json!({"session": w, "text": "continua"})).unwrap();
    assert_eq!(task_of(&board_json(&st, &h), "a")["worker_status"], "running");
    // Background e needs_input si riflettono sul task.
    st.run(&h, |r| stop(r, &w, None, Some(2), 3));
    assert_eq!(task_of(&board_json(&st, &h), "a")["worker_status"], "background");
    st.run(&h, |r| hook(r, &w, "needs-input", Some("permesso"), 4));
    assert_eq!(task_of(&board_json(&st, &h), "a")["worker_status"], "needs_input");
    call(&st, &h, &w, "report_fact", json!({"text": "trovato"})).unwrap();
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["worker_status"], "running");
    assert_eq!(a["last_report"], "trovato");
}

#[test]
fn failed_and_blocked_reports_set_the_column() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &[])]);
    let (wa, wb) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"));
    finish(&st, &h, &wa, "failed");
    finish(&st, &h, &wb, "blocked");
    let b = board_json(&st, &h);
    assert_eq!(task_of(&b, "a")["column"], "failed");
    assert_eq!(task_of(&b, "b")["column"], "blocked");
    assert_eq!(task_of(&b, "a")["report"]["status"], "failed");
    // Failed/blocked -> done (l'orchestratore ha risolto a mano) e retry.
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    call(&st, &h, ORCH, "set_task_status", json!({"id": "b", "status": "queued"})).unwrap();
    assert_eq!(task_of(&board_json(&st, &h), "b")["attempts"], 2);
}

#[test]
fn retry_reuses_the_existing_worktree() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    let adds = |h: &FakeHost| h.git_log().iter().filter(|l| l.contains("worktree add")).count();
    assert_eq!(adds(&h), 1);
    // Il worktree esiste ancora: si riusa senza ricrearlo.
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(adds(&h), 1);
    let sp = h.spawns();
    assert_eq!(sp.len(), 2);
    assert_eq!(sp[1].cwd.as_deref(), Some("C:\\p\\.worktrees\\a"));
    assert_eq!(task_of(&board_json(&st, &h), "a")["worktree"]["branch"], "task/a");
    // Esiste solo il branch: worktree add senza -b.
    let w2 = session_of(&st, &h, "a");
    finish(&st, &h, &w2, "failed");
    h.dirs.lock().unwrap().clear();
    h.git_rule("rev-parse --verify --quiet refs/heads/task/a", true, "abc\n", "");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert!(h.git_log().contains(&"C:\\p|worktree add C:\\p\\.worktrees\\a task/a".to_string()), "{:?}", h.git_log());
}

#[test]
fn done_cleanup_warnings_do_not_block() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &[])]);
    let (wa, wb) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"));
    finish(&st, &h, &wa, "completed");
    finish(&st, &h, &wb, "completed");
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    h.git_rule("worktree remove", false, "", "contains modified or untracked files");
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert_eq!(r["task"]["status"], "done");
    assert!(r["warnings"][0].as_str().unwrap().contains("worktree not removed"), "{r}");
    assert!(r["task"]["note"].as_str().unwrap().contains("cleanup warnings"));
    // cleanup=false: nessun git.
    let n = h.git_log().len();
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "b", "status": "done", "cleanup": false})).unwrap();
    assert_eq!(r["task"]["status"], "done");
    assert_eq!(h.git_log().len(), n);
    // Branch non cancellabile: avviso, ma done.
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.git_rule("branch -d", false, "", "error: the branch 'task/a' is not fully merged");
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert!(r["warnings"][0].as_str().unwrap().contains("not fully merged"));
}

#[test]
fn non_git_workspace_rejects_worktree_tasks_only() {
    let (st, h) = st_h();
    h.git_rule("rev-parse --is-inside-work-tree", false, "", "fatal: not a git repository");
    let err = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a", &[])]})).unwrap_err();
    assert!(err.contains("not a git repository") && err.contains("isolation"), "{err}");
    assert!(task_of(&board_json(&st, &h), "a").is_null());
    // Con isolation none va bene: lavora nella root.
    let b = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [{"id": "n", "task": "x", "isolation": "none"}]})).unwrap();
    assert_eq!(task_of(&b, "n")["column"], "running");
    let sp = h.spawns();
    assert_eq!(sp[0].cwd.as_deref(), Some("C:\\p"));
    assert!(!h.git_log().iter().any(|l| l.contains("worktree add")));
    // L'ambiente non parla di worktree ne' di setup.
    *h.setup.lock().unwrap() = Some("npm ci".into());
    let env = call(&st, &h, &sp[0].tab_id, "get_task", json!({})).unwrap()["environment"].as_str().unwrap().to_string();
    assert!(env.contains("No dedicated worktree") && !env.contains("npm ci"), "{env}");
    // update_task verso worktree richiede il repo.
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [{"id": "m", "task": "x", "isolation": "none", "held": true}]})).unwrap();
    let err = call(&st, &h, ORCH, "update_task", json!({"id": "m", "isolation": "worktree"})).unwrap_err();
    assert!(err.contains("not a git repository"));
}

#[test]
fn plan_validation_errors_come_back_as_tool_errors() {
    let (st, h) = st_h();
    let err = |p: Value| call(&st, &h, ORCH, "submit_plan", p).unwrap_err();
    assert!(err(json!({"tasks": [t("a", &["b"]), t("b", &["a"])]})).contains("dependency cycle"));
    assert!(err(json!({"tasks": [t("a", &["zz"])]})).contains("unknown task"));
    assert!(err(json!({"tasks": [t("a", &[]), t("a", &[])]})).contains("duplicate"));
    assert!(err(json!({"tasks": [{"id": "a", "task": "x", "permission_mode": "bypass"}]})).contains("not allowed"));
    assert!(err(json!({"tasks": [{"id": "BAD", "task": "x"}]})).contains("invalid id"));
    assert!(err(json!({})).contains("tasks is missing"));
    assert!(task_of(&board_json(&st, &h), "a").is_null());
    assert!(st.inner.lock().unwrap().boards["ws-1"].tasks.is_empty());
    // Nessun task salvato per le richieste respinte (la sola presa di
    // proprieta' della board e' una modifica della vista).
    assert!(h.saved.lock().unwrap().values().all(|b| b.tasks.is_empty()));
}

#[test]
fn board_tools_are_for_orchestrators_of_the_workspace() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    for m in ["submit_plan", "get_board", "get_task_details", "update_task", "set_task_status", "set_board"] {
        let e = call(&st, &h, &w, m, json!({})).unwrap_err();
        assert!(e.contains("reserved for the orchestrator"), "{m}: {e}");
    }
    // Un orchestratore senza workspace noto non ha board.
    let e = call(&st, &h, "orch-2", "get_board", json!({})).unwrap_err();
    assert_eq!(e, "the orchestrator's workspace was not found");
    // Ogni workspace ha la sua board.
    let b = call(&st, &h, "orch-wsl", "get_board", json!({})).unwrap();
    assert_eq!(b["workspace_id"], "ws-2");
    assert_eq!(b["tasks"], json!([]));
    assert_eq!(call(&st, &h, ORCH, "get_board", json!({})).unwrap()["tasks"].as_array().unwrap().len(), 1);
    // get_board e' compatto: niente testo del task.
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert!(task_of(&b, "a").get("task").is_none());
    assert_eq!(task_of(&b, "a")["title"], "do a");
    // get_task_details da' il testo.
    let d = call(&st, &h, ORCH, "get_task_details", json!({"id": "a"})).unwrap();
    assert_eq!(d["task"]["task"], "do a");
    assert_eq!(d["task"]["column"], "running");
    let e = call(&st, &h, ORCH, "get_task_details", json!({"id": "zz"})).unwrap_err();
    assert_eq!(e, "unknown task: zz");
    assert!(call(&st, &h, ORCH, "get_task_details", json!({})).is_err());
}

#[test]
fn ownership_moves_live_workers_and_queued_events() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &[])]);
    let (wa, wb) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"));
    call(&st, &h, &wa, "get_task", json!({})).unwrap();
    call(&st, &h, &wb, "get_task", json!({})).unwrap();
    // Un worker ad-hoc dello stesso orchestratore non si sposta.
    let adhoc = call(&st, &h, ORCH, "spawn_session", json!({"task": "x", "cwd": "C:\\p\\esiste"})).unwrap()["session"]
        .as_str()
        .unwrap()
        .to_string();
    call(&st, &h, &wa, "report_fact", json!({"text": "f"})).unwrap();
    call(&st, &h, &adhoc, "get_task", json!({})).unwrap();
    // Un altro orchestratore dello stesso workspace prende la board.
    let b = call(&st, &h, "orch-b", "get_board", json!({})).unwrap();
    assert_eq!(b["owner"], "orch-b");
    let sessions = call(&st, &h, "orch-b", "list_sessions", json!({})).unwrap();
    let names: Vec<_> = sessions.as_array().unwrap().iter().map(|s| s["session"].as_str().unwrap().to_string()).collect();
    assert!(names.contains(&wa) && names.contains(&wb) && !names.contains(&adhoc));
    let sa = sessions.as_array().unwrap().iter().find(|s| s["session"] == wa.as_str()).unwrap();
    assert_eq!(sa["task"], "a");
    // Il vecchio owner tiene solo il suo worker ad-hoc e gli eventi relativi.
    let old = events(&st, &h, ORCH);
    assert!(old.iter().all(|e| e["session"] == adhoc), "{old:?}");
    let new = events(&st, &h, "orch-b");
    let k = kinds_of(&new);
    assert_eq!(k.iter().filter(|x| *x == "started").count(), 2);
    assert!(k.contains(&"fact".to_string()));
    assert!(new.iter().all(|e| e["task"].is_string()));
    assert!(new.windows(2).all(|p| p[0]["seq"].as_u64() < p[1]["seq"].as_u64()));
    // I nuovi eventi dei worker di board vanno al nuovo owner.
    finish(&st, &h, &wa, "completed");
    assert!(events(&st, &h, ORCH).is_empty());
    assert_eq!(kinds_of(&events(&st, &h, "orch-b")), vec!["completed"]);
    // Il vecchio owner non gestisce piu' i worker di board.
    assert!(call(&st, &h, ORCH, "send_message", json!({"session": wb, "text": "x"})).is_err());
    assert!(call(&st, &h, "orch-b", "send_message", json!({"session": wb, "text": "x"})).is_ok());
    // Un Update con il nuovo parent e' stato emesso per i tab aperti.
    assert!(h.log().iter().any(|l| l.starts_with(&format!("update:{wb}:"))));
}

#[test]
fn ownerless_board_queues_events_for_the_next_owner() {
    let (st, h) = st_h();
    // Board caricata dal disco: nessun owner, un task pronto.
    let mut b = Board::new("ws-1");
    b.add_plan(
        vec![board::parse_spec(&json!({"id": "a", "task": "do a"}), None).unwrap()],
        PermissionMode::Bypass,
        1,
    )
    .unwrap();
    let loaded = Board::from_json(&serde_json::to_string(&b).unwrap(), 5).unwrap();
    st.inner.lock().unwrap().load_board(loaded);
    // L'utente toglie la pausa/imposta il parallelismo: lo scheduler parte lo stesso.
    let v = boards::board_set_cmd(&st, &h, "ws-1", Some(2), Some(false)).unwrap();
    assert_eq!(v.tasks[0].column, board::Column::Running);
    assert_eq!(v.owner, None);
    let sp = h.spawns();
    assert_eq!(sp.len(), 1);
    assert_eq!(sp[0].parent_tab_id, "", "senza owner");
    let w = sp[0].tab_id.clone();
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    finish(&st, &h, &w, "completed");
    // Nessuna coda di orchestratore, ma un evento in attesa nella board.
    assert_eq!(st.inner.lock().unwrap().board_queues["ws-1"].len(), 2);
    // Il primo orchestratore che usa un tool di board li riceve.
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert!(!st.inner.lock().unwrap().board_queues.contains_key("ws-1"));
    let ev = events(&st, &h, ORCH);
    assert_eq!(kinds_of(&ev), vec!["started", "completed"]);
    assert!(ev.iter().all(|e| e["task"] == "a"));
    assert_eq!(st.inner.lock().unwrap().list_sessions(ORCH).len(), 1);
}

#[test]
fn restart_turns_running_tasks_into_failed() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &["a"])]);
    let saved = h.saved.lock().unwrap().get("ws-1").cloned().unwrap();
    assert_eq!(saved.task("a").unwrap().status, board::TaskStatus::Running);
    let raw = serde_json::to_string_pretty(&saved).unwrap();
    // Nuova app: stato vuoto, board ricaricata.
    let (st2, h2) = st_h();
    st2.inner.lock().unwrap().load_board(Board::from_json(&raw, 77).unwrap());
    let view = st2.run(&h2, |r| r.board_view("ws-1"));
    assert_eq!(view.owner, None);
    let a = &view.tasks[0];
    assert_eq!(a.task.status, board::TaskStatus::Failed);
    assert_eq!(a.column, board::Column::Failed);
    assert_eq!(a.task.note.as_deref(), Some("interrupted: the app was restarted"));
    assert!(a.task.session.is_none());
    assert_eq!(view.tasks[1].column, board::Column::Backlog);
    // Il caricamento non avvia nulla.
    assert!(h2.spawns().is_empty());
    // Retry dell'orchestratore: riparte.
    let r = call(&st2, &h2, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(r["task"]["attempts"], 2);
    assert_eq!(h2.spawns().len(), 1);
}

#[test]
fn per_orchestrator_worker_cap_limits_the_scheduler() {
    let (st, h) = st_h();
    let mut adhoc = Vec::new();
    for i in 0..(MAX_WORKERS_PER_ORCHESTRATOR - 1) {
        let r = call(&st, &h, ORCH, "spawn_session", json!({"task": format!("adhoc {i}")})).unwrap();
        adhoc.push(r["session"].as_str().unwrap().to_string());
    }
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({"max_parallel": 4, "tasks": [t("a", &[]), t("b", &[]), t("c", &[])]}),
    )
    .unwrap();
    let running = b["tasks"].as_array().unwrap().iter().filter(|x| x["column"] == "running").count();
    assert_eq!(running, 1, "un solo posto libero sui 16");
    // Chiuso un worker ad-hoc, al prossimo giro ne parte un altro.
    st.run(&h, |r| r.tab_closed(&adhoc[0], 9));
    let b = call(&st, &h, ORCH, "set_board", json!({"max_parallel": 5})).unwrap();
    let running = b["tasks"].as_array().unwrap().iter().filter(|x| x["column"] == "running").count();
    assert_eq!(running, 2);
}

#[test]
fn set_board_pause_and_parallelism() {
    let (st, h) = st_h();
    call(&st, &h, ORCH, "set_board", json!({"paused": true})).unwrap();
    let b = submit(&st, &h, vec![t("a", &[])]);
    assert_eq!(b["paused"], true);
    assert_eq!(task_of(&b, "a")["column"], "ready");
    assert!(h.spawns().is_empty());
    let b = call(&st, &h, ORCH, "set_board", json!({"paused": false})).unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    for bad in [json!({}), json!({"max_parallel": 0}), json!({"max_parallel": 17}), json!({"paused": "si"})] {
        assert!(call(&st, &h, ORCH, "set_board", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(call(&st, &h, ORCH, "set_board", json!({"max_parallel": 16})).unwrap()["max_parallel"], 16);
}

#[test]
fn held_tasks_and_update_task() {
    let (st, h) = st_h();
    let b = submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true}), t("b", &[])]);
    assert_eq!(task_of(&b, "a")["column"], "ready");
    assert_eq!(task_of(&b, "a")["held"], true);
    assert_eq!(task_of(&b, "b")["column"], "running");
    let err = call(&st, &h, ORCH, "update_task", json!({"id": "a", "foo": 1})).unwrap_err();
    assert!(err.contains("unknown field"));
    // update_task: rilascia, cambia priorita' e dipendenze; il task parte.
    let r = call(&st, &h, ORCH, "update_task", json!({"id": "a", "held": false, "priority": 3, "title": "Nuovo"})).unwrap();
    assert_eq!(r["task"]["title"], "Nuovo");
    assert!(r["task"].get("task").is_none(), "update_task ritorna il task senza testo");
    assert_eq!(r["task"]["column"], "running");
    // Un task in esecuzione non si modifica.
    let err = call(&st, &h, ORCH, "update_task", json!({"id": "b", "title": "x"})).unwrap_err();
    assert!(err.contains("only queued, failed, blocked or cancelled"), "{err}");
    assert!(call(&st, &h, ORCH, "update_task", json!({"title": "x"})).is_err());
}

#[test]
fn user_actions_notify_without_waking() {
    let (st, h) = st_h();
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true}), t("b", &["a"])]);
    let ws = "ws-1";
    let act = |task: &str, action: &str| boards::board_action(&st, &h, ws, task, action);
    let v = act("a", "unhold").unwrap();
    assert_eq!(v.tasks[0].task.status, board::TaskStatus::Running);
    let v = act("a", "cancel").unwrap();
    assert_eq!(v.tasks[0].column, board::Column::Cancelled);
    assert!(h.log().contains(&format!("close:{}", h.spawns()[0].tab_id)));
    let v = act("a", "retry").unwrap();
    assert_eq!(v.tasks[0].task.status, board::TaskStatus::Running);
    assert!(act("a", "boh").is_err());
    assert!(act("zz", "hold").is_err());
    assert!(act("b", "hold").is_ok());
    assert!(act("b", "done").is_err(), "queued non va a done");
    assert!(boards::board_action(&st, &h, "ws-9", "a", "hold").is_err());
    let ev = events(&st, &h, ORCH);
    let ua: Vec<_> = ev.iter().filter(|e| e["kind"] == "user_action").collect();
    assert_eq!(ua.len(), 4, "una per azione riuscita: {ev:?}");
    assert!(ua[0]["text"].as_str().unwrap().contains("released from hold"));
    assert!(ua[1]["text"].as_str().unwrap().contains("cancelled"));
    assert_eq!(ua[0]["task"], "a");
    assert_eq!(wake_count(&h, ORCH), 0, "user_action non sveglia");
}

#[test]
fn user_done_cleans_up_and_completes_the_plan_once() {
    let (st, h) = st_h();
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    events(&st, &h, ORCH);
    let v = boards::board_action(&st, &h, "ws-1", "a", "done").unwrap();
    assert_eq!(v.tasks[0].column, board::Column::Done);
    assert!(h.git_log().iter().any(|l| l.ends_with("branch -d task/a")));
    let ev = events(&st, &h, ORCH);
    assert_eq!(kinds_of(&ev), vec!["user_action", "plan_completed"]);
    assert!(ev[1]["task"].is_null() || ev[1].get("task").is_none());
    assert!(wake_count(&h, ORCH) >= 1);
    // Nessun secondo plan_completed senza una nuova transizione.
    boards::board_set_cmd(&st, &h, "ws-1", Some(4), None).unwrap();
    assert!(events(&st, &h, ORCH).is_empty());
    // Un task nuovo riapre il piano e la prossima chiusura lo rilancia.
    let b = submit(&st, &h, vec![json!({"id": "n", "task": "x", "isolation": "none", "held": true})]);
    assert_eq!(task_of(&b, "n")["column"], "ready");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "n", "status": "cancelled"})).unwrap();
    assert_eq!(kinds_of(&events(&st, &h, ORCH)), vec!["plan_completed"]);
}

#[test]
fn remove_board_deletes_the_file_and_the_state() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    assert!(h.saved.lock().unwrap().contains_key("ws-1"));
    st.remove_board(&h, "ws-1");
    assert_eq!(h.deleted.lock().unwrap().as_slice(), ["ws-1"]);
    assert!(st.inner.lock().unwrap().boards.is_empty());
    let last = h.board_events.lock().unwrap().last().cloned().unwrap();
    assert!(last.tasks.is_empty());
    // Rimuovere una board inesistente non fa nulla.
    st.remove_board(&h, "ws-1");
    assert_eq!(h.deleted.lock().unwrap().len(), 1);
    // Ricreata: le revisioni ripartono e vengono pubblicate.
    submit(&st, &h, vec![t("z", &[])]);
    assert!(h.saved.lock().unwrap()["ws-1"].task("z").is_some());
}

#[test]
fn wsl_worktrees_use_posix_paths() {
    let (st, h) = st_h();
    let b = call(&st, &h, "orch-wsl", "submit_plan", json!({"tasks": [t("a", &[])]})).unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    assert!(h.git_log().contains(&"/home/u|worktree add /home/u/.worktrees/a -b task/a main".to_string()), "{:?}", h.git_log());
    assert_eq!(h.spawns()[0].cwd.as_deref(), Some("/home/u/.worktrees/a"));
    assert!(h.excludes.lock().unwrap()[0].starts_with("/home/u/.git/info/exclude"));
    assert!(h.spawns()[0].command.contains("wsl"));
    assert_eq!(boards::join_path("windows", "C:\\p\\", ".worktrees/x"), "C:\\p\\.worktrees\\x");
    assert_eq!(boards::join_path("wsl", "/home/u/", ".worktrees/x"), "/home/u/.worktrees/x");
}

#[test]
fn base_branch_resolution_order_and_detached_head() {
    let (st, h) = st_h();
    h.git_rule("rev-parse --abbrev-ref HEAD", true, "develop\n", "");
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [
        t("cur", &[]),
        {"id": "own", "task": "x", "base_branch": "release/1.0"},
    ]})).unwrap();
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\cur -b task/cur develop".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\own -b task/own release/1.0".to_string()), "{git:?}");
    // Base della board: vale per i task senza override.
    let (st, h) = st_h();
    call(&st, &h, ORCH, "submit_plan", json!({"base_branch": "integration", "tasks": [t("a", &[])]})).unwrap();
    assert!(h.git_log().contains(&"C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a integration".to_string()));
    assert!(!h.git_log().iter().any(|l| l.ends_with("--abbrev-ref HEAD")));
    // HEAD staccato: avvio fallito con un messaggio chiaro.
    let (st, h) = st_h();
    h.git_rule("rev-parse --abbrev-ref HEAD", true, "HEAD\n", "");
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a", &[])]})).unwrap();
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("detached HEAD"));
}

#[test]
fn adhoc_spawn_and_complete_task_still_work_with_structured_fields() {
    let (st, h) = st_h();
    let w = call(&st, &h, ORCH, "spawn_session", json!({"task": "ad hoc"})).unwrap()["session"].as_str().unwrap().to_string();
    let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
    assert!(t.get("environment").is_none() && t.get("task_id").is_none());
    assert!(call(&st, &h, &w, "complete_task", json!({"status": "completed", "summary": "s", "tests": [{"command": "x", "result": "ok"}]})).is_err());
    call(&st, &h, &w, "complete_task", json!({"status": "completed", "summary": "s", "commit": "abc", "files_changed": ["a"]})).unwrap();
    let ev = events(&st, &h, ORCH);
    assert!(ev.iter().all(|e| e.get("task").is_none()));
    assert!(st.inner.lock().unwrap().boards.is_empty(), "nessuna board creata dagli ad-hoc");
    assert!(h.saved.lock().unwrap().is_empty());
}

#[test]
fn touches_serialize_tasks_and_windows_ignores_case() {
    let (st, h) = st_h();
    let b = call(&st, &h, ORCH, "submit_plan", json!({"max_parallel": 4, "tasks": [
        {"id": "a", "task": "x", "touches": ["Src/Auth"]},
        {"id": "b", "task": "x", "touches": ["src\\auth\\login"]},
        {"id": "c", "task": "x", "touches": ["docs"]},
    ]})).unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    assert_eq!(task_of(&b, "b")["column"], "ready");
    assert_eq!(task_of(&b, "c")["column"], "running");
    // Su WSL le maiuscole contano.
    let b = call(&st, &h, "orch-wsl", "submit_plan", json!({"max_parallel": 4, "tasks": [
        {"id": "a", "task": "x", "touches": ["Src/Auth"]},
        {"id": "b", "task": "x", "touches": ["src/auth/login"]},
    ]})).unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    assert_eq!(task_of(&b, "b")["column"], "running");
}

// ---------------------------------------------------------------------------
// Robustezza: avvisi, guardiano, rev, serializzazione, lapide
// ---------------------------------------------------------------------------

fn running_count(st: &AutomationState, h: &FakeHost) -> usize {
    board_json(st, h)["tasks"].as_array().unwrap().iter().filter(|x| x["status"] == "running").count()
}

#[test]
fn exclude_failure_goes_into_the_task_note() {
    let (st, h) = st_h();
    *h.exclude_err.lock().unwrap() = Some("exclude not updated: boom".to_string());
    submit(&st, &h, vec![t("a", &[])]);
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "running", "l'avviso non blocca l'avvio");
    let note = a["note"].as_str().unwrap();
    assert!(note.contains("boom") && note.contains(".worktrees/"), "{note}");
}

#[test]
fn a_closing_worker_tab_alone_restarts_queued_tasks() {
    let (st, h) = st_h();
    let mut adhoc = Vec::new();
    for i in 0..MAX_WORKERS_PER_ORCHESTRATOR {
        let r = call(&st, &h, ORCH, "spawn_session", json!({"task": format!("adhoc {i}")})).unwrap();
        adhoc.push(r["session"].as_str().unwrap().to_string());
    }
    submit(&st, &h, vec![t("a", &[])]);
    assert_eq!(running_count(&st, &h), 0, "tetto di 16 pieno");
    // Nessun altro trigger: il solo tab chiuso libera il posto e il task parte.
    st.run(&h, |r| r.tab_closed(&adhoc[0], 9));
    assert_eq!(running_count(&st, &h), 1);
}

#[test]
fn rev_grows_with_every_change_of_the_view() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let w = session_of(&st, &h, "a");
    let v0 = st.run(&h, |r| r.board_view("ws-1"));
    call(&st, &h, &w, "get_task", json!({})).unwrap(); // worker_status starting -> running
    let v1 = st.run(&h, |r| r.board_view("ws-1"));
    assert!(v1.rev > v0.rev, "worker_status e' nella vista");
    call(&st, &h, &w, "report_fact", json!({"text": "half way"})).unwrap(); // last_report
    let v2 = st.run(&h, |r| r.board_view("ws-1"));
    assert!(v2.rev > v1.rev);
    assert_eq!(v2.tasks[0].task.last_report.as_deref(), Some("half way"));
    assert!(v2.updated_at > 0);
    // Un altro orchestratore che prende la board cambia `owner`: nuova rev.
    let before = v2.rev;
    call(&st, &h, "orch-b", "get_board", json!({})).unwrap();
    let v3 = st.run(&h, |r| r.board_view("ws-1"));
    assert_eq!(v3.owner.as_deref(), Some("orch-b"));
    assert!(v3.rev > before);
    // Il tool risponde con la rev, e il flusso di eventi cresce stretto.
    assert_eq!(call(&st, &h, "orch-b", "get_board", json!({})).unwrap()["rev"], v3.rev);
    let revs: Vec<u64> = h.board_events.lock().unwrap().iter().map(|v| v.rev).collect();
    assert!(revs.windows(2).all(|p| p[0] < p[1]), "{revs:?}");
    assert_eq!(*revs.last().unwrap(), v3.rev);
}

#[test]
fn watchdog_fails_a_start_that_never_registered() {
    let (st, h) = st_h();
    let plan = board::parse_plan(&json!({"tasks": [{"id": "a", "task": "do a"}]})).unwrap();
    let mut b = Board::new("ws-1");
    b.add_plan(plan.specs, PermissionMode::AcceptEdits, 1).unwrap();
    b.start_task("a", 1_000); // running, senza sessione: il thread di avvio non risponde
    st.run(&h, |r| r.load_board(b));
    st.run(&h, |r| r.watchdog(1_000 + 60_000));
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "running", "troppo presto");
    st.run(&h, |r| r.watchdog(1_000 + boards::START_TIMEOUT_MS + 1));
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("start"), "{a}");
    let ev = events(&st, &h, ORCH);
    assert!(kinds_of(&ev).contains(&"start_failed".to_string()), "{ev:?}");
}

#[test]
fn watchdog_closes_a_worker_that_never_called_get_task() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), t("b", &[])]);
    let (wa, wb) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"));
    call(&st, &h, &wb, "get_task", json!({})).unwrap();
    events(&st, &h, ORCH);
    let late = now_millis() + boards::START_TIMEOUT_MS + 1;
    st.run(&h, |r| r.watchdog(late));
    let board = board_json(&st, &h);
    assert_eq!(task_of(&board, "a")["status"], "failed");
    assert!(task_of(&board, "a")["note"].as_str().unwrap().contains("get_task"));
    assert_eq!(task_of(&board, "b")["status"], "running", "chi ha letto il task non si tocca");
    assert!(h.log().contains(&format!("close:{wa}")), "{:?}", h.log());
    assert!(!h.log().contains(&format!("close:{wb}")));
    assert!(kinds_of(&events(&st, &h, ORCH)).contains(&"exited".to_string()));
    // Il tab chiuso dopo non genera altri eventi.
    st.run(&h, |r| r.tab_closed(&wa, late));
    assert!(events(&st, &h, ORCH).is_empty());
    // Il watchdog e' idempotente.
    st.run(&h, |r| r.watchdog(late + 1));
    assert!(events(&st, &h, ORCH).is_empty());
}

#[test]
fn start_jobs_of_the_same_task_never_overlap_in_git() {
    let (st, h) = st_h();
    let plan = board::parse_plan(&json!({"tasks": [{"id": "a", "task": "do a"}]})).unwrap();
    let mut b = Board::new("ws-1");
    b.add_plan(plan.specs, PermissionMode::AcceptEdits, 1).unwrap();
    b.start_task("a", 1); // tentativo 1
    let job = |b: &Board, attempt: u32| StartJob {
        ws: h.ws_for("ws-1").unwrap(),
        task: b.task("a").unwrap().clone(),
        board_base: None,
        attempt,
    };
    let job1 = job(&b, 1);
    st.run(&h, |r| r.load_board(b));
    *h.add_hold.0.lock().unwrap() = true;
    let t1 = {
        let (st, h) = (st.clone(), h.clone());
        std::thread::spawn(move || boards::run_start_job(&st, &h, job1))
    };
    let t0 = std::time::Instant::now();
    while h.add_in_flight.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(t0.elapsed() < Duration::from_secs(10), "il primo avvio non e' arrivato a git");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Cancel + retry mentre il primo e' ancora in git: tentativo 2.
    let job2 = {
        let mut reg = st.inner.lock().unwrap();
        let b = reg.boards.get_mut("ws-1").unwrap();
        b.start_task("a", 2);
        job(b, 2)
    };
    let t2 = {
        let (st, h) = (st.clone(), h.clone());
        std::thread::spawn(move || boards::run_start_job(&st, &h, job2))
    };
    std::thread::sleep(Duration::from_millis(150));
    let adds = h.git_log().iter().filter(|l| l.contains("worktree add")).count();
    assert_eq!(adds, 1, "il secondo avvio aspetta il primo");
    {
        let (m, c) = &*h.add_hold;
        *m.lock().unwrap() = false;
        c.notify_all();
    }
    t1.join().unwrap();
    t2.join().unwrap();
    assert_eq!(h.add_max_in_flight.load(std::sync::atomic::Ordering::SeqCst), 1);
    // Il primo e' superato e non registra nulla; il secondo si'.
    assert_eq!(h.spawns().len(), 1);
    assert!(st.inner.lock().unwrap().boards["ws-1"].task("a").unwrap().session.is_some());
    assert!(st.starts.lock().unwrap().is_empty(), "i lock per task non si accumulano");
}

#[test]
fn late_publish_does_not_resurrect_a_deleted_board() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[])]);
    let old = st.inner.lock().unwrap().boards["ws-1"].clone();
    st.remove_board(&h, "ws-1");
    assert!(!h.saved.lock().unwrap().contains_key("ws-1"));
    // Una pubblicazione in ritardo (effetto applicato da un altro thread).
    st.publish(&h, &old);
    assert!(!h.saved.lock().unwrap().contains_key("ws-1"), "file resuscitato");
    let last = h.board_events.lock().unwrap().last().cloned().unwrap();
    assert!(last.tasks.is_empty() && last.rev > old.rev);
}

#[test]
fn save_gate_coalesces_close_saves_and_keeps_the_latest() {
    let mut g = SaveGate::default();
    let mut b = Board::new("ws-1");
    b.rev = 1;
    assert_eq!(g.offer(&b, 10_000), SaveDecision::Now);
    b.rev = 2;
    assert_eq!(g.offer(&b, 10_200), SaveDecision::Arm(Duration::from_millis(800)));
    b.rev = 3;
    assert_eq!(g.offer(&b, 10_300), SaveDecision::Queued);
    b.rev = 4;
    assert_eq!(g.offer(&b, 10_900), SaveDecision::Queued);
    // Scatta il timer: si salva solo l'ultimo stato, una volta.
    assert_eq!(g.take("ws-1", 11_000).map(|b| b.rev), Some(4));
    assert!(g.take("ws-1", 11_000).is_none());
    b.rev = 5;
    assert_eq!(g.offer(&b, 11_500), SaveDecision::Arm(Duration::from_millis(500)));
    assert_eq!(g.take_all().len(), 1, "all'uscita si svuota quel che resta");
    // Board diverse non si coalescono fra loro; dopo la pausa si salva subito.
    let other = Board::new("ws-2");
    assert_eq!(g.offer(&other, 11_600), SaveDecision::Now);
    assert!(g.take("ws-1", 12_000).is_none(), "il timer trova il pending gia' salvato");
    assert_eq!(g.offer(&b, 20_000), SaveDecision::Now);
}

#[test]
fn keep_awake_effects_applied_out_of_order_drop_the_stale_one() {
    let (st, h) = st_h();
    apply_effects(&st, &h, vec![Effect::KeepAwake { on: true, seq: 2 }, Effect::KeepAwake { on: false, seq: 1 }]);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true], "il vecchio rilascio non annulla il blocco");
    apply_effects(&st, &h, vec![Effect::KeepAwake { on: false, seq: 3 }]);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false]);
}

#[test]
fn git_timeout_kills_a_hung_process() {
    let cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("ping");
        c.args(["-n", "20", "127.0.0.1"]);
        c
    } else {
        let mut c = std::process::Command::new("sleep");
        c.arg("20");
        c
    };
    let t0 = std::time::Instant::now();
    let err = boards::output_with_timeout(cmd, Duration::from_millis(400)).unwrap_err();
    assert!(err.contains("timed out"), "{err}");
    assert!(t0.elapsed() < Duration::from_secs(10));
    // Un comando che finisce in tempo ritorna l'output.
    let ok = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "echo", "ciao"]);
        c
    } else {
        let mut c = std::process::Command::new("echo");
        c.arg("ciao");
        c
    };
    let out = boards::output_with_timeout(ok, Duration::from_secs(20)).unwrap();
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("ciao"));
}

#[test]
fn task_model_falls_back_to_workspace_worker_model() {
    let (st, h) = st_h();
    *h.worker_model.lock().unwrap() = Some("sonnet".to_string());
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![t("a", &[]), json!({"id": "b", "task": "do b", "model": "opus"})]);
    let spawns = h.spawns();
    let cmd_of = |id: &str| {
        let tab = session_of(&st, &h, id);
        spawns.iter().find(|s| s.tab_id == tab).unwrap().command.clone()
    };
    assert!(cmd_of("a").contains("model=sonnet"), "{}", cmd_of("a"));
    assert!(cmd_of("b").contains("model=opus"), "{}", cmd_of("b"));
}

#[test]
fn board_set_model_updates_task_and_notifies_orchestrator() {
    let (st, h) = st_h();
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true})]);
    let model_of = |v: &crate::board::BoardView| v.tasks[0].task.model.clone();
    let v = boards::board_set_model(&st, &h, "ws-1", "a", Some(" opus ")).unwrap();
    assert_eq!(model_of(&v).as_deref(), Some("opus"));
    let v = boards::board_set_model(&st, &h, "ws-1", "a", None).unwrap();
    assert_eq!(model_of(&v), None);
    assert!(boards::board_set_model(&st, &h, "ws-1", "a", Some("bad model!")).is_err());
    assert!(boards::board_set_model(&st, &h, "ws-1", "zz", Some("opus")).is_err());
    assert!(boards::board_set_model(&st, &h, "ws-9", "a", Some("opus")).is_err());
    let ev = events(&st, &h, ORCH);
    let ua: Vec<_> = ev.iter().filter(|e| e["kind"] == "user_action").collect();
    assert_eq!(ua.len(), 2, "{ev:?}");
    assert!(ua[0]["text"].as_str().unwrap().contains("set the model of task 'a' to 'opus'"));
    assert!(ua[1]["text"].as_str().unwrap().contains("cleared the model of task 'a'"));
}

#[test]
fn get_board_is_compact_and_filters_columns() {
    let (st, h) = st_h();
    submit(&st, &h, vec![json!({"id": "a", "task": "do a", "held": true}), t("b", &["a"])]);
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(b["counts"]["ready"], 1);
    assert_eq!(b["counts"]["backlog"], 1);
    assert!(b["counts"].get("done").is_none());
    assert_eq!(task_of(&b, "b")["waiting_on"], json!(["a"]));
    assert!(task_of(&b, "a").get("waiting_on").is_none());
    for k in ["task", "worktree", "touches", "created_at", "report", "depends_on"] {
        assert!(task_of(&b, "a").get(k).is_none(), "{k}");
    }
    // Un task cancelled esce dalle righe ma resta nei conteggi.
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled"})).unwrap();
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert!(task_of(&b, "a").is_null());
    assert_eq!(b["counts"]["cancelled"], 1);
    let b = call(&st, &h, ORCH, "get_board", json!({"columns": ["cancelled"]})).unwrap();
    assert_eq!(b["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(task_of(&b, "a")["column"], "cancelled");
    // Validazione di `columns`.
    let e = call(&st, &h, ORCH, "get_board", json!({"columns": ["nope"]})).unwrap_err();
    assert!(e.contains("backlog") && e.contains("cancelled"), "{e}");
    assert!(call(&st, &h, ORCH, "get_board", json!({"columns": "done"})).is_err());
    assert!(call(&st, &h, ORCH, "get_board", json!({"columns": [1]})).is_err());
}

#[test]
fn status_changes_return_the_task_without_its_text() {
    let (st, h) = st_h();
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true})]);
    let u = call(&st, &h, ORCH, "update_task", json!({"id": "a", "title": "nuovo"})).unwrap();
    assert_eq!(u["task"]["title"], "nuovo");
    assert!(u["task"].get("task").is_none());
    let s = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled"})).unwrap();
    assert_eq!(s["task"]["column"], "cancelled");
    assert!(s["task"].get("task").is_none());
    let p = call(&st, &h, ORCH, "set_board", json!({"paused": true})).unwrap();
    assert!(p["counts"].is_object() && p["tasks"].is_array());
}
