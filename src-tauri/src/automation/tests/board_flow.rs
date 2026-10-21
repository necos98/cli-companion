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
    assert!(git.contains(&"C:\\p|branch -D task/a".to_string()), "{git:?}");
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
    assert_eq!(st.inner.lock().unwrap().list_sessions(ORCH, 0)[0].status, WorkerStatus::Closed);
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
    h.git_rule("merge-base", false, "", "");
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
    // Un orchestratore avviato a mano (registrato, mai diventato owner) apre il gate dello scheduler.
    st.orchestrator_open(&h, "ws-1", "man-1", now_millis()).unwrap();
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
    assert_eq!(st.inner.lock().unwrap().list_sessions(ORCH, 0).len(), 1);
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
    assert!(h.git_log().iter().any(|l| l.ends_with("branch -D task/a")));
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
        board_bases: Default::default(),
        integration_template: None,
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

#[test]
fn done_from_queued_over_rpc_needs_a_note_and_unblocks_dependents() {
    let (st, h) = st_h();
    st.run(&h, |r| hook(r, ORCH, "finished", None, 1));
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true}), t("b", &["a"])]);
    let e = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap_err();
    assert!(e.contains("note"), "{e}");
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done", "note": "done by hand in main"})).unwrap();
    assert_eq!(r["task"]["column"], "done");
    assert_eq!(r["task"]["external"], true);
    assert!(r.get("warnings").is_none());
    assert!(h.git_log().iter().all(|l| !l.contains("worktree remove") && !l.contains("branch -")));
    // Il dipendente parte.
    let b = board_json(&st, &h);
    assert_eq!(task_of(&b, "b")["column"], "running");
    // Piano completo: conteggi nel testo.
    let w = session_of(&st, &h, "b");
    finish(&st, &h, &w, "completed");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "b", "status": "done"})).unwrap();
    let ev = events(&st, &h, ORCH);
    let pc = ev.iter().find(|e| e["kind"] == "plan_completed").expect("plan_completed");
    let text = pc["text"].as_str().unwrap();
    assert!(text.contains("2 done (of which 1 completed outside the board)"), "{text}");
}

#[test]
fn plan_completed_is_not_sent_when_every_task_was_cancelled_unstarted() {
    let (st, h) = st_h();
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "held": true}), json!({"id": "b", "task": "y", "held": true})]);
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled"})).unwrap();
    call(&st, &h, ORCH, "set_task_status", json!({"id": "b", "status": "cancelled"})).unwrap();
    assert!(events(&st, &h, ORCH).is_empty());
}

#[test]
fn workspace_rules_reach_board_and_adhoc_workers_but_orchestrator_note_does_not() {
    let (st, h) = st_h();
    *h.worker_rules.lock().unwrap() = "Never run the e2e tests.".to_string();
    submit(&st, &h, vec![json!({"id": "a", "task": "do a", "orchestrator_note": "secret merge plan"})]);
    let w = session_of(&st, &h, "a");
    let task = call(&st, &h, &w, "get_task", json!({})).unwrap();
    let ins = task["instructions"].as_str().unwrap();
    assert!(ins.starts_with(crate::automation::WORKER_INSTRUCTIONS));
    assert!(ins.contains("Workspace rules (always apply):\nNever run the e2e tests."), "{ins}");
    assert!(!task["task"].as_str().unwrap().contains("e2e"));
    assert!(!task.to_string().contains("secret merge plan"), "{task}");
    // L'orchestratore la vede, il worker no.
    let d = call(&st, &h, ORCH, "get_task_details", json!({"id": "a"})).unwrap();
    assert_eq!(d["task"]["orchestrator_note"], "secret merge plan");
    assert_eq!(call(&st, &h, ORCH, "get_board", json!({})).unwrap()["tasks"][0]["orchestrator_note"], "secret merge plan");

    // Worker ad-hoc (spawn_session): stesse regole, lette al get_task.
    let r = call(&st, &h, ORCH, "spawn_session", json!({"task": "adhoc"})).unwrap();
    let s = r["session"].as_str().unwrap().to_string();
    *h.worker_rules.lock().unwrap() = "Only edit docs.".to_string();
    let t = call(&st, &h, &s, "get_task", json!({})).unwrap();
    assert!(t["instructions"].as_str().unwrap().contains("Workspace rules (always apply):\nOnly edit docs."));
    assert!(t.get("environment").is_none());

    // Senza regole: istruzioni invariate.
    *h.worker_rules.lock().unwrap() = "  ".to_string();
    let r = call(&st, &h, ORCH, "spawn_session", json!({"task": "adhoc2"})).unwrap();
    let t = call(&st, &h, r["session"].as_str().unwrap(), "get_task", json!({})).unwrap();
    assert_eq!(t["instructions"], crate::automation::WORKER_INSTRUCTIONS);
}

#[test]
fn list_sessions_reports_seconds_since_spawn_until_get_task() {
    let (st, h) = st_h();
    let w = st.run(&h, |r| r.spawn(ORCH, new_worker("w"), 1_000)).unwrap().tab_id;
    let ls = st.run(&h, |r| r.list_sessions(ORCH, 8_500));
    assert_eq!(ls[0].status, WorkerStatus::Starting);
    assert_eq!(ls[0].starting_secs, Some(7));
    st.run(&h, |r| r.get_task(&w, 9_000)).unwrap();
    assert_eq!(st.run(&h, |r| r.list_sessions(ORCH, 20_000))[0].starting_secs, None);
}

// --- workspace con piu' repository (la root non e' un repo) ---

/// Root non-git `C:\p` con i repository `r1` e `r2`.
fn multi_repo() -> (AutomationState, FakeHost) {
    let (st, h) = st_h();
    h.git_rule("C:\\p|rev-parse --is-inside-work-tree", true, "false\n", "");
    h.dirs.lock().unwrap().extend(["C:\\p\\r1".to_string(), "C:\\p\\r2".to_string()]);
    (st, h)
}

fn repo_t(id: &str, repo: &str) -> Value {
    json!({"id": id, "task": format!("do {id}"), "repo": repo})
}

#[test]
fn non_git_root_with_two_repos_runs_worktree_tasks_per_repo() {
    let (st, h) = multi_repo();
    h.git_rule("C:\\p\\r2|symbolic-ref", true, "origin/main\n", "");
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({
            "base_branches": {"r1": "master"},
            "tasks": [
                {"id": "a1", "task": "x", "repo": "r1", "touches": ["src"]},
                {"id": "a2", "task": "x", "repo": "r1", "touches": ["docs"]},
                {"id": "b1", "task": "x", "repo": "r2", "touches": ["src"]},
            ]
        }),
    )
    .unwrap();
    // `src` in r1 e in r2 non confligge: partono tutti.
    for id in ["a1", "a2", "b1"] {
        assert_eq!(task_of(&b, id)["column"], "running", "{id}");
    }
    assert_eq!(task_of(&b, "a1")["repo"], "r1");
    assert_eq!(b["base_branches"], json!({"r1": "master"}));
    let git = h.git_log();
    // Cartella git = il repository; r1 usa l'impostazione, r2 il default di origin.
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\a1 -b task/a1 master".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\a2 -b task/a2 master".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r2|worktree add C:\\p\\r2\\.worktrees\\b1 -b task/b1 main".to_string()), "{git:?}");
    // Un solo fetch per (repo, base) anche con due task che partono insieme.
    assert_eq!(git.iter().filter(|l| l.as_str() == "C:\\p\\r1|fetch origin master").count(), 1, "{git:?}");
    assert_eq!(git.iter().filter(|l| l.as_str() == "C:\\p\\r2|fetch origin main").count(), 1, "{git:?}");
    // Mai nulla nella root che non e' un repo, e mai checkout.
    assert!(!git.iter().any(|l| l.starts_with("C:\\p|worktree") || l.contains("checkout") || l.contains("switch")), "{git:?}");
    let ex = h.excludes.lock().unwrap().clone();
    assert!(ex.iter().any(|l| l.starts_with("C:\\p\\r1\\.git\\info\\exclude|")), "{ex:?}");
    assert!(ex.iter().any(|l| l.starts_with("C:\\p\\r2\\.git\\info\\exclude|")), "{ex:?}");
    let cwds: Vec<String> = h.spawns().iter().filter_map(|s| s.cwd.clone()).collect();
    assert!(cwds.contains(&"C:\\p\\r1\\.worktrees\\a1".to_string()) && cwds.contains(&"C:\\p\\r2\\.worktrees\\b1".to_string()), "{cwds:?}");
    // Il worker vede il repository.
    let w = session_of(&st, &h, "a1");
    let env = call(&st, &h, &w, "get_task", json!({})).unwrap()["environment"].as_str().unwrap().to_string();
    assert!(env.contains("Repository: r1") && env.contains("C:\\p\\r1\\.worktrees\\a1"), "{env}");
    // done: pulizia nel repository del task.
    finish(&st, &h, &w, "completed");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a1", "status": "done"})).unwrap();
    assert!(h.git_log().contains(&"C:\\p\\r1|branch -D task/a1".to_string()), "{:?}", h.git_log());
    // diffstat nel worktree del repo.
    assert!(h.git_log().iter().any(|l| l == "C:\\p\\r1\\.worktrees\\a1|diff --stat master...HEAD"), "{:?}", h.git_log());
}

#[test]
fn repo_validation_names_the_repository() {
    let (st, h) = multi_repo();
    h.dirs.lock().unwrap().push("C:\\p\\plain".to_string());
    h.git_rule("C:\\p\\plain|rev-parse --is-inside-work-tree", true, "false\n", "");
    // Un task della root con worktree: la root non e' un repo.
    let e = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a", &[])]})).unwrap_err();
    assert!(e.contains("not a git repository") && e.contains("workspace folder"), "{e}");
    let e = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [repo_t("a", "plain")]})).unwrap_err();
    assert!(e.contains("'plain'") && e.contains("not a git repository"), "{e}");
    let e = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [repo_t("a", "r1"), repo_t("b", "ghost")]})).unwrap_err();
    assert!(e.contains("'ghost'") && e.contains("does not exist"), "{e}");
    for bad in ["../x", "/abs", "C:\\x", ""] {
        assert!(call(&st, &h, ORCH, "submit_plan", json!({"tasks": [repo_t("a", bad)]})).is_err(), "{bad}");
    }
    // Niente e' stato aggiunto; isolation none non richiede un repo git.
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(b["tasks"], json!([]));
    let b = submit(&st, &h, vec![json!({"id": "n", "task": "x", "isolation": "none", "repo": "plain"})]);
    assert_eq!(task_of(&b, "n")["column"], "running");
    assert_eq!(h.spawns()[0].cwd.as_deref(), Some("C:\\p\\plain"));
    // update_task: il nuovo repo (con worktree) si verifica; "." e' la root.
    submit(&st, &h, vec![json!({"id": "q", "task": "x", "repo": "r1", "held": true})]);
    let e = call(&st, &h, ORCH, "update_task", json!({"id": "q", "repo": "plain"})).unwrap_err();
    assert!(e.contains("'plain'"), "{e}");
    let e = call(&st, &h, ORCH, "update_task", json!({"id": "q", "repo": "."})).unwrap_err();
    assert!(e.contains("workspace folder"), "{e}");
    call(&st, &h, ORCH, "update_task", json!({"id": "q", "repo": "r2"})).unwrap();
    assert_eq!(task_of(&board_json(&st, &h), "q")["repo"], "r2");
}

#[test]
fn repo_base_resolution_order_fetch_and_remote_ref() {
    let (st, h) = multi_repo();
    h.git_rule("C:\\p\\r2|symbolic-ref", true, "origin/trunk\n", "");
    // r1: origin/dev esiste -> si parte dal riferimento remoto.
    h.git_rule("C:\\p\\r1|rev-parse --verify --quiet refs/remotes/origin/dev", true, "", "");
    // La root non e' un repo: un task della root con worktree si rifiuta a monte.
    let e = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("root-iso", &[])]})).unwrap_err();
    assert!(e.contains("workspace folder"), "{e}");
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({
            "base_branch": "board-wide", "base_branches": {"r1": "dev"},
            "tasks": [
                repo_t("r1-board", "r1"),
                {"id": "r1-own", "task": "x", "repo": "r1", "base_branch": "release/1.0", "touches": ["x"]},
                repo_t("r2-default", "r2"),
            ]
        }),
    )
    .unwrap();
    assert_eq!(task_of(&b, "r2-default")["column"], "running");
    let git = h.git_log();
    // task > base_branches[repo] (con origin/) > default di origin.
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\r1-board -b task/r1-board origin/dev".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\r1-own -b task/r1-own release/1.0".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r2|worktree add C:\\p\\r2\\.worktrees\\r2-default -b task/r2-default trunk".to_string()), "{git:?}");
    // La base della board non vale per i task dei repo.
    assert!(!git.iter().any(|l| l.contains("board-wide")), "{git:?}");
    let a = task_of(&board_json(&st, &h), "r1-board");
    assert_eq!(a["worktree"]["base"], "origin/dev");
    assert_eq!(a["worktree"]["repo"], "r1");
}

#[test]
fn board_base_branch_still_rules_root_tasks_and_current_branch_is_the_last_resort() {
    let (st, h) = st_h();
    h.dirs.lock().unwrap().push("C:\\p\\sub".to_string());
    // Root repo (default del fake): la base della board vale solo per la root; un repo senza
    // origin/HEAD parte dal suo branch corrente (qui "main").
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({"base_branch": "integration", "tasks": [t("r", &[]), repo_t("s", "sub")]}),
    )
    .unwrap();
    assert_eq!(task_of(&b, "s")["column"], "running");
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|worktree add C:\\p\\.worktrees\\r -b task/r integration".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\sub|worktree add C:\\p\\sub\\.worktrees\\s -b task/s main".to_string()), "{git:?}");
    // Nessun fetch per i task della root (comportamento invariato).
    assert!(!git.iter().any(|l| l.starts_with("C:\\p|fetch")), "{git:?}");
}

#[test]
fn failed_fetch_warns_and_missing_origin_skips_it() {
    let (st, h) = multi_repo();
    h.git_rule("C:\\p\\r1|fetch origin", false, "", "could not resolve host");
    h.git_rule("C:\\p\\r2|remote get-url origin", false, "", "no such remote");
    let b = submit(&st, &h, vec![repo_t("a", "r1"), repo_t("b", "r2")]);
    // Il fetch fallito non blocca l'avvio, ma lascia un avviso nella nota.
    assert_eq!(task_of(&b, "a")["column"], "running");
    assert_eq!(task_of(&b, "b")["column"], "running");
    let full = board_json(&st, &h);
    let note = task_of(&full, "a")["note"].as_str().unwrap().to_string();
    assert!(note.contains("start warnings") && note.contains("git fetch origin") && note.contains("could not resolve host"), "{note}");
    assert!(task_of(&full, "b")["note"].is_null());
    assert!(!h.git_log().iter().any(|l| l.starts_with("C:\\p\\r2|fetch")), "{:?}", h.git_log());
}

#[test]
fn integration_branch_is_created_once_per_repo_and_tasks_start_from_it() {
    let (st, h) = multi_repo();
    // Un branch "int" esiste gia' e non e' nostro: si usa il suffisso (le regole si valutano in ordine).
    h.git_rule("rev-parse --verify --quiet refs/heads/int-2", false, "", "");
    h.git_rule("C:\\p\\r1|rev-parse --verify --quiet refs/heads/int", true, "", "");
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({
            "base_branches": {"r1": "master"}, "integration_branch": "int",
            "max_parallel": 1,
            "tasks": [repo_t("a", "r1"), {"id": "b", "task": "x", "repo": "r1", "depends_on": ["a"]}]
        }),
    )
    .unwrap();
    assert_eq!(b["integration_branch"], "int");
    let git = h.git_log();
    let ib = "C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\integration -b int-2 master".to_string();
    assert!(git.contains(&ib), "{git:?}");
    // Il task parte dall'integrazione (riferimento locale), non da master.
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\a -b task/a int-2".to_string()), "{git:?}");
    // L'integrazione e' esposta all'orchestratore.
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(g["integrations"]["r1"]["branch"], "int-2");
    assert_eq!(g["integrations"]["r1"]["path"], "C:\\p\\r1\\.worktrees\\integration");
    assert_eq!(g["integrations"]["r1"]["base"], "master");
    // Il dipendente riusa l'integrazione: nessun secondo branch.
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\integration".to_string());
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    let git = h.git_log();
    assert_eq!(git.iter().filter(|l| l.contains("worktree add") && l.contains("-b int")).count(), 1, "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\b -b task/b int-2".to_string()), "{git:?}");
    // Mai checkout/switch/push nella cartella principale.
    assert!(!git.iter().any(|l| l.contains("checkout") || l.contains("switch") || l.contains("push")), "{git:?}");
    // Persistita nel file della board (sopravvive al riavvio).
    let saved = st.inner.lock().unwrap().boards["ws-1"].clone();
    let again = Board::from_json(&serde_json::to_string(&saved).unwrap(), 5).unwrap();
    assert_eq!(again.integrations["r1"].branch, "int-2");
    assert_eq!(again.integrations["r1"].path, "C:\\p\\r1\\.worktrees\\integration");
    // Cambiare il modello scarta le integrazioni, spegnerlo anche.
    let g = call(&st, &h, ORCH, "set_board", json!({"integration_branch": "other/{date}"})).unwrap();
    assert_eq!(g["integration_branch"], "other/{date}");
    assert!(g.get("integrations").is_none());
    let g = call(&st, &h, ORCH, "set_board", json!({"integration_branch": ""})).unwrap();
    assert!(g.get("integration_branch").is_none());
    assert!(call(&st, &h, ORCH, "set_board", json!({"integration_branch": "a b"})).is_err());
    assert!(call(&st, &h, ORCH, "set_board", json!({})).is_err());
    // base_branches si fonde: la stringa vuota toglie la voce.
    let g = call(&st, &h, ORCH, "set_board", json!({"base_branches": {"r2": "main"}})).unwrap();
    assert_eq!(g["base_branches"], json!({"r1": "master", "r2": "main"}));
    let g = call(&st, &h, ORCH, "set_board", json!({"base_branches": {"r1": ""}})).unwrap();
    assert_eq!(g["base_branches"], json!({"r2": "main"}));
}

#[test]
fn integration_applies_to_root_tasks_and_expands_the_date() {
    let (st, h) = st_h();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    call(&st, &h, ORCH, "submit_plan", json!({"integration_branch": "integration/{date}", "tasks": [t("a", &[])]})).unwrap();
    let git = h.git_log();
    assert!(
        git.contains(&format!("C:\\p|worktree add C:\\p\\.worktrees\\integration -b integration/{today} main")),
        "{git:?}"
    );
    assert!(git.contains(&format!("C:\\p|worktree add C:\\p\\.worktrees\\a -b task/a integration/{today}")), "{git:?}");
    assert!(!git.iter().any(|l| l.contains("fetch")), "{git:?}");
}

#[test]
fn repos_are_detected_lazily_and_cached_until_the_board_changes() {
    let (st, h) = multi_repo();
    *h.repos.lock().unwrap() = vec!["DDB".to_string(), "r1".to_string()];
    use std::sync::atomic::Ordering::SeqCst;
    // Un piano o il dettaglio di un task non scandagliano il disco.
    submit(&st, &h, vec![json!({"id": "n", "task": "x", "isolation": "none", "held": true})]);
    call(&st, &h, ORCH, "get_task_details", json!({"id": "n"})).unwrap();
    assert_eq!(h.list_repos_calls.load(SeqCst), 0);
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(g["repos"], json!(["DDB", "r1"]));
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    call(&st, &h, ORCH, "get_board", json!({"columns": ["done"]})).unwrap();
    assert_eq!(h.list_repos_calls.load(SeqCst), 1, "in cache");
    // submit_plan e set_board la invalidano.
    submit(&st, &h, vec![json!({"id": "m", "task": "x", "isolation": "none", "held": true})]);
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(h.list_repos_calls.load(SeqCst), 2);
    call(&st, &h, ORCH, "set_board", json!({"paused": true})).unwrap();
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(h.list_repos_calls.load(SeqCst), 3);
}

#[test]
fn repo_list_parsing_and_local_detection() {
    assert_eq!(boards::parse_repo_list("b\n.\n\n a \n../x\nb\n/abs\n"), [".", "a", "b"]);
    assert!(boards::parse_repo_list("").is_empty());
    let root = std::env::temp_dir().join(format!("cc-repos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["r1/.git", "r2", "r3", ".hidden/.git", "node_modules/x"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join("r3/.git"), "gitdir: ../elsewhere").unwrap(); // worktree o submodule: file
    let mut ws = FakeHost::default().ws_for("ws-1").unwrap();
    ws.path = root.to_string_lossy().into_owned();
    assert_eq!(boards::detect_repos(&ws).unwrap(), ["r1", "r3"]);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    assert_eq!(boards::detect_repos(&ws).unwrap(), [".", "r1", "r3"]);
    let _ = std::fs::remove_dir_all(&root);
}

// --- hook per repository: setup e pulizia ---

fn cfg_of(h: &FakeHost, repo: &str, cfg: crate::workspaces::RepoConfig) {
    h.repo_cfg.lock().unwrap().insert(repo.to_string(), cfg);
}

fn hooks_of(h: &FakeHost) -> Vec<String> {
    h.hook_log.lock().unwrap().clone()
}

fn fail_out(stdout: &str, stderr: &str) -> GitOut {
    GitOut { ok: false, stdout: stdout.to_string(), stderr: stderr.to_string() }
}

fn setup_cfg() -> crate::workspaces::RepoConfig {
    crate::workspaces::RepoConfig {
        container_root: Some("/var/www/html".to_string()),
        copy_ignored: vec!["configuration.php".to_string()],
        setup: vec!["cp -a vendor.src vendor".to_string(), "composer dump-autoload".to_string()],
        cleanup: vec!["docker exec svc rm -rf cache".to_string()],
        ..Default::default()
    }
}

#[test]
fn setup_runs_in_order_in_the_new_task_and_integration_worktrees() {
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", setup_cfg());
    h.files.lock().unwrap().push("C:\\p\\r1\\configuration.php".to_string());
    let b = call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({"integration_branch": "int", "tasks": [repo_t("a", "r1"), repo_t("b", "r2")]}),
    )
    .unwrap();
    assert_eq!(task_of(&b, "a")["column"], "running");
    let log = hooks_of(&h);
    let (iw, tw) = ("C:\\p\\r1\\.worktrees\\integration", "C:\\p\\r1\\.worktrees\\a");
    // Prima l'integrazione, poi il task: copia, poi i comandi in ordine, con cwd = worktree.
    let pos = |needle: String| log.iter().position(|l| l.starts_with(&needle)).unwrap_or_else(|| panic!("{needle} in {log:?}"));
    let order = [
        format!("copy C:\\p\\r1\\configuration.php->{iw}\\configuration.php"),
        format!("{iw}|cp -a vendor.src vendor|"),
        format!("{iw}|composer dump-autoload|"),
        format!("copy C:\\p\\r1\\configuration.php->{tw}\\configuration.php"),
        format!("{tw}|cp -a vendor.src vendor|"),
        format!("{tw}|composer dump-autoload|"),
    ];
    let idx: Vec<usize> = order.iter().cloned().map(pos).collect();
    assert!(idx.windows(2).all(|w| w[0] < w[1]), "{idx:?} {log:?}");
    // Variabili.
    let step = log.iter().find(|l| l.starts_with(&order[5])).unwrap();
    for v in [
        "REPO=C:\\p\\r1",
        &format!("WORKTREE={tw}"),
        "TASK_ID=a",
        "BRANCH=task/a",
        "BASE=int",
        "CONTAINER_WORKTREE=/var/www/html/.worktrees/a",
    ] {
        assert!(step.contains(v), "{v} in {step}");
    }
    let istep = log.iter().find(|l| l.starts_with(&order[2])).unwrap();
    assert!(
        istep.contains("TASK_ID=integration")
            && istep.contains("BRANCH=int")
            && istep.contains("CONTAINER_WORKTREE=/var/www/html/.worktrees/integration"),
        "{istep}"
    );
    // Il repo senza configurazione non esegue nulla; l'integrazione si prepara una sola volta.
    assert!(!log.iter().any(|l| l.contains("r2")), "{log:?}");
    assert_eq!(log.iter().filter(|l| l.contains("|composer dump-autoload|")).count(), 2, "{log:?}");
    // Fatto: registrato nel task e nell'integrazione; il worker lo sa.
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["worktree"]["setup_done"], true);
    assert!(a.get("preparing").is_none());
    let w = session_of(&st, &h, "a");
    let env = call(&st, &h, &w, "get_task", json!({})).unwrap()["environment"].as_str().unwrap().to_string();
    assert!(env.contains("already prepared") && env.contains("composer dump-autoload") && env.contains("/var/www/html"), "{env}");
    let saved = st.inner.lock().unwrap().boards["ws-1"].clone();
    assert!(saved.integrations["r1"].setup_done);
}

#[test]
fn setup_is_not_rerun_on_reuse_after_success_but_is_after_a_failed_one() {
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", setup_cfg());
    submit(&st, &h, vec![repo_t("a", "r1")]);
    let count = |h: &FakeHost| hooks_of(h).iter().filter(|l| l.contains("|composer dump-autoload|")).count();
    assert_eq!(count(&h), 1);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    // Riuso del worktree dopo un setup riuscito: niente di nuovo.
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\a".to_string());
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(count(&h), 1);
    assert_eq!(h.spawns().len(), 2);

    // Setup fallito al secondo passo: l'avvio fallisce nominando il passo, con la coda dell'output.
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", setup_cfg());
    h.hook_rules
        .lock()
        .unwrap()
        .push(("composer dump-autoload".to_string(), fail_out("Generating autoload", "Could not open input file: composer.json")));
    submit(&st, &h, vec![repo_t("a", "r1")]);
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "failed");
    let note = a["note"].as_str().unwrap();
    assert!(note.contains("setup 2/2") && note.contains("composer dump-autoload") && note.contains("Could not open input file"), "{note}");
    assert!(note.contains("'r1'"), "{note}");
    assert!(h.spawns().is_empty());
    assert!(kinds_of(&events(&st, &h, ORCH)).contains(&"start_failed".to_string()));
    // Il worktree e' rimasto, registrato come non preparato.
    assert_eq!(a["worktree"]["setup_done"], false);
    assert!(a.get("preparing").is_none());
    assert_eq!(count(&h), 1);
    // Retry con il worktree ancora li': il setup si rifa e questa volta riesce.
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\a".to_string());
    h.hook_rules.lock().unwrap().clear();
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(count(&h), 2);
    assert_eq!(h.spawns().len(), 1);
    assert_eq!(task_of(&board_json(&st, &h), "a")["worktree"]["setup_done"], true);
    // Un secondo retry non lo rifa.
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "failed");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(count(&h), 2);
}

#[test]
fn a_failing_integration_setup_fails_the_start_and_is_retried_later() {
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", setup_cfg());
    // La riga delle regole e' "<cwd>|<comando>": il cwd dell'integrazione.
    h.hook_rules.lock().unwrap().push(("integration|cp -a".to_string(), fail_out("", "boom")));
    call(&st, &h, ORCH, "submit_plan", json!({"integration_branch": "int", "tasks": [repo_t("a", "r1")]})).unwrap();
    let b = board_json(&st, &h);
    let note = task_of(&b, "a")["note"].as_str().unwrap().to_string();
    assert!(note.contains("integration worktree") && note.contains("setup 1/2") && note.contains("boom"), "{note}");
    assert!(!st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].setup_done);
    // Niente worktree del task: non si e' arrivati a crearlo.
    assert!(!h.git_log().iter().any(|l| l.contains("worktree add C:\\p\\r1\\.worktrees\\a")), "{:?}", h.git_log());
    // Retry con l'integrazione esistente: il suo setup si ripete e poi il task parte.
    h.hook_rules.lock().unwrap().clear();
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\integration".to_string());
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "queued"})).unwrap();
    assert_eq!(h.spawns().len(), 1);
    assert!(st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].setup_done);
    let runs = hooks_of(&h).iter().filter(|l| l.contains("integration|composer dump-autoload|")).count();
    assert_eq!(runs, 1);
}

#[test]
fn copy_ignored_copies_only_ignored_existing_paths_and_warns_about_the_rest() {
    let (st, h) = multi_repo();
    let mut cfg = setup_cfg();
    cfg.setup.clear();
    cfg.copy_ignored = vec!["configuration.php".into(), "tracked.txt".into(), "missing.env".into(), "public/api/.env".into()];
    cfg_of(&h, "r1", cfg);
    h.git_rule("r1|check-ignore -q -- tracked.txt", false, "", "");
    h.files.lock().unwrap().extend(
        ["configuration.php", "tracked.txt", "public/api/.env"].map(|f| format!("C:\\p\\r1\\{}", f.replace('/', "\\"))),
    );
    let b = submit(&st, &h, vec![repo_t("a", "r1")]);
    let copies: Vec<String> = hooks_of(&h).into_iter().filter(|l| l.starts_with("copy ")).collect();
    assert_eq!(
        copies,
        [
            "copy C:\\p\\r1\\configuration.php->C:\\p\\r1\\.worktrees\\a\\configuration.php",
            "copy C:\\p\\r1\\public\\api\\.env->C:\\p\\r1\\.worktrees\\a\\public\\api\\.env",
        ]
    );
    assert_eq!(task_of(&b, "a")["column"], "running");
    let note = task_of(&board_json(&st, &h), "a")["note"].as_str().unwrap().to_string();
    assert!(note.starts_with("start warnings:"), "{note}");
    assert!(note.contains("tracked.txt not copied") && note.contains("missing.env not copied"), "{note}");
}

#[test]
fn cleanup_commands_run_before_worktree_remove_and_never_block_done() {
    let (st, h) = multi_repo();
    let mut cfg = setup_cfg();
    cfg.remove_force = true;
    cfg.cleanup.push("echo second".to_string());
    cfg_of(&h, "r1", cfg);
    submit(&st, &h, vec![repo_t("a", "r1")]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\a".to_string());
    h.hook_rules.lock().unwrap().push(("docker exec svc".to_string(), fail_out("", "no such container")));
    let r = call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert_eq!(r["task"]["status"], "done");
    let git = h.git_log();
    let at = |needle: &str| git.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle} in {git:?}"));
    let remove = at("C:\\p\\r1|worktree remove --force C:\\p\\r1\\.worktrees\\a");
    assert!(at("hook|docker exec svc rm -rf cache") < at("hook|echo second"));
    assert!(at("hook|echo second") < remove);
    assert!(remove < at("C:\\p\\r1|branch -D task/a"));
    // Cwd e variabili dei comandi di pulizia.
    let line = hooks_of(&h).into_iter().find(|l| l.contains("|echo second|")).unwrap();
    assert!(line.starts_with("C:\\p\\r1\\.worktrees\\a|") && line.contains("TASK_ID=a") && line.contains("BRANCH=task/a"), "{line}");
    // Il fallimento del primo comando e' un avviso, non un blocco.
    let warn = r["warnings"][0].as_str().unwrap();
    assert!(warn.contains("cleanup 1/2") && warn.contains("no such container"), "{r}");
}

#[test]
fn worktree_remove_is_not_forced_by_default() {
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", crate::workspaces::RepoConfig { cleanup: vec!["true".into()], ..Default::default() });
    submit(&st, &h, vec![repo_t("a", "r1")]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\a".to_string());
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert!(h.git_log().contains(&"C:\\p\\r1|worktree remove C:\\p\\r1\\.worktrees\\a".to_string()), "{:?}", h.git_log());
    assert_eq!(hooks_of(&h).len(), 1);
}

#[test]
fn base_branch_from_workspace_settings_sits_between_board_and_origin_default() {
    let (st, h) = multi_repo();
    h.git_rule("C:\\p\\r1|symbolic-ref", true, "origin/main\n", "");
    h.git_rule("C:\\p\\r2|symbolic-ref", true, "origin/main\n", "");
    cfg_of(&h, "r1", crate::workspaces::RepoConfig { base_branch: Some("develop".into()), ..Default::default() });
    cfg_of(&h, "r2", crate::workspaces::RepoConfig { base_branch: Some("develop".into()), ..Default::default() });
    call(
        &st,
        &h,
        ORCH,
        "submit_plan",
        json!({
            "base_branches": {"r2": "release"},
            "tasks": [repo_t("a", "r1"), repo_t("b", "r2"), {"id": "c", "task": "x", "repo": "r1", "base_branch": "hot"}]
        }),
    )
    .unwrap();
    let git = h.git_log();
    // Impostazione del workspace al posto del default di origin; la board vince su di essa; il task su tutto.
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\a -b task/a develop".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r2|worktree add C:\\p\\r2\\.worktrees\\b -b task/b release".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\c -b task/c hot".to_string()), "{git:?}");
}

#[test]
fn watchdog_waits_for_a_step_in_progress_but_not_forever() {
    let (st, h) = st_h();
    let new_board = || {
        let plan = board::parse_plan(&json!({"tasks": [{"id": "a", "task": "do a"}]})).unwrap();
        let mut b = Board::new("ws-1");
        b.add_plan(plan.specs, PermissionMode::AcceptEdits, 1).unwrap();
        b.start_task("a", 1_000);
        b
    };
    let mut b = new_board();
    let attempt = b.task("a").unwrap().attempts;
    // Un passo di setup parte dopo 4 minuti: i 5 minuti dell'avvio non contano piu'.
    assert!(b.touch_preparing("a", attempt, Some("setup 1/2: composer install"), 1_000 + 4 * 60_000));
    st.run(&h, |r| r.load_board(b));
    st.run(&h, |r| r.watchdog(1_000 + 9 * 60_000));
    let a = task_of(&board_json(&st, &h), "a");
    assert_eq!(a["status"], "running");
    assert_eq!(a["preparing"], "setup 1/2: composer install");
    // Un passo senza battiti oltre il suo tetto: un thread morto si scopre.
    st.run(&h, |r| r.watchdog(1_000 + 4 * 60_000 + boards::PREPARING_TIMEOUT_MS + 1));
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "failed");
    // A passo finito il tempo riparte da li' (5 minuti per il resto dell'avvio).
    let mut b = new_board();
    let attempt = b.task("a").unwrap().attempts;
    b.touch_preparing("a", attempt, Some("setup"), 2_000);
    b.clear_preparing("a", attempt, 20 * 60_000);
    st.run(&h, |r| r.load_board(b));
    st.run(&h, |r| r.watchdog(20 * 60_000 + 60_000));
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "running");
    st.run(&h, |r| r.watchdog(20 * 60_000 + boards::START_TIMEOUT_MS + 1));
    assert_eq!(task_of(&board_json(&st, &h), "a")["status"], "failed");
}

// --- integrazione: stato del setup, cartella sparita, chiusura ---

const INT_DIR: &str = "C:\\p\\r1\\.worktrees\\integration";

fn int_plan(st: &AutomationState, h: &FakeHost) -> Value {
    call(
        st,
        h,
        ORCH,
        "submit_plan",
        json!({"base_branches": {"r1": "master"}, "integration_branch": "int", "tasks": [repo_t("a", "r1")]}),
    )
    .unwrap()
}

#[test]
fn integration_setup_state_is_none_without_hooks_and_done_with_them() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(g["integrations"]["r1"]["setup"], "none");
    assert!(st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].setup_skipped);
    let (st, h) = multi_repo();
    cfg_of(&h, "r1", setup_cfg());
    int_plan(&st, &h);
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(g["integrations"]["r1"]["setup"], "done");
    assert!(!st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].setup_skipped);
}

#[test]
fn get_board_flags_an_integration_whose_folder_is_gone() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(g["integrations"]["r1"]["missing"], true);
    h.dirs.lock().unwrap().push(INT_DIR.to_string());
    let g = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert!(g["integrations"]["r1"].get("missing").is_none());
}

#[test]
fn close_integration_forgets_the_registration_only() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let n = h.git_log().len();
    // Chiave sconosciuta: errore, nulla cambia.
    let e = call(&st, &h, ORCH, "set_board", json!({"close_integration": "r2"})).unwrap_err();
    assert!(e.contains("r2") && e.contains("no integration"), "{e}");
    let e = call(&st, &h, ORCH, "set_board", json!({"close_integration": ["r1", "nope"]})).unwrap_err();
    assert!(e.contains("nope"), "{e}");
    assert!(call(&st, &h, ORCH, "set_board", json!({"close_integration": 3})).is_err());
    assert!(st.inner.lock().unwrap().boards["ws-1"].integrations.contains_key("r1"));
    let g = call(&st, &h, ORCH, "set_board", json!({"close_integration": ["r1/"]})).unwrap();
    assert!(g.get("integrations").is_none(), "{g}");
    // Ne' worktree ne' branch vengono toccati.
    assert_eq!(h.git_log().len(), n, "{:?}", h.git_log());
    let e = call(&st, &h, ORCH, "set_board", json!({})).unwrap_err();
    assert!(e.contains("close_integration"), "{e}");
}

fn second_task_after_int(h: &FakeHost, st: &AutomationState) -> Value {
    // Il branch `int` esiste (e `int-2` no); la cartella dell'integrazione non c'e'.
    h.git_rule("refs/heads/int-2", false, "", "");
    h.git_rule("refs/heads/int", true, "", "");
    call(st, h, ORCH, "submit_plan", json!({"tasks": [repo_t("b", "r1")]})).unwrap();
    board_json(st, h)
}

#[test]
fn a_merged_integration_with_a_missing_folder_is_replaced_by_a_fresh_one() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let b = second_task_after_int(&h, &st);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p\\r1|merge-base --is-ancestor int master".to_string()), "{git:?}");
    assert!(git.contains(&format!("C:\\p\\r1|worktree add {INT_DIR} -b int-2 master")), "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|worktree add C:\\p\\r1\\.worktrees\\b -b task/b int-2".to_string()), "{git:?}");
    assert_eq!(st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].branch, "int-2");
    assert!(task_of(&b, "b")["note"].as_str().unwrap().contains("already merged"), "{b}");
}

#[test]
fn an_unmerged_integration_with_a_missing_folder_is_recreated_on_its_branch() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    h.git_rule("merge-base --is-ancestor int", false, "", "");
    let b = second_task_after_int(&h, &st);
    let git = h.git_log();
    assert!(git.contains(&format!("C:\\p\\r1|worktree add {INT_DIR} int")), "{git:?}");
    assert_eq!(st.inner.lock().unwrap().boards["ws-1"].integrations["r1"].branch, "int");
    assert!(task_of(&b, "b")["note"].as_str().unwrap().contains("worktree recreated"), "{b}");
}

// --- avvisi, branch del task, testo del task ---

#[test]
fn paused_board_adds_a_warning_to_update_submit_and_status_responses() {
    let (st, h) = st_h();
    submit(&st, &h, vec![json!({"id": "a", "task": "x", "isolation": "none", "held": true})]);
    let r = call(&st, &h, ORCH, "update_task", json!({"id": "a", "title": "T"})).unwrap();
    assert!(r.get("warnings").is_none());
    call(&st, &h, ORCH, "set_board", json!({"paused": true})).unwrap();
    let has = |v: &Value| v["warnings"].as_array().is_some_and(|w| w.iter().any(|x| x.as_str().unwrap().contains("board is paused")));
    assert!(has(&call(&st, &h, ORCH, "update_task", json!({"id": "a", "title": "U"})).unwrap()));
    assert!(has(&call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "cancelled"})).unwrap()));
    let b = call(&st, &h, ORCH, "submit_plan", json!({"tasks": [{"id": "z", "task": "x", "isolation": "none"}]})).unwrap();
    assert!(has(&b), "{b}");
}

#[test]
fn task_branch_merged_into_the_integration_is_deleted_with_capital_d() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.dirs.lock().unwrap().push("C:\\p\\r1\\.worktrees\\a".to_string());
    // Il branch e' nell'integrazione, non nella base del worktree.
    h.git_rule("merge-base --is-ancestor task/a int", true, "", "");
    h.git_rule("merge-base --is-ancestor task/a", false, "", "");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    let git = h.git_log();
    assert!(git.contains(&"C:\\p\\r1|merge-base --is-ancestor task/a int".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p\\r1|branch -D task/a".to_string()), "{git:?}");
}

#[test]
fn unmerged_task_branch_keeps_the_safe_delete() {
    let (st, h) = multi_repo();
    submit(&st, &h, vec![repo_t("a", "r1")]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    h.git_rule("merge-base", false, "", "");
    call(&st, &h, ORCH, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    assert!(h.git_log().contains(&"C:\\p\\r1|branch -d task/a".to_string()), "{:?}", h.git_log());
}

#[test]
fn update_task_returns_the_text_only_when_the_patch_changes_it() {
    let (st, h) = st_h();
    submit(&st, &h, vec![json!({"id": "a", "task": "old", "isolation": "none", "held": true})]);
    let r = call(&st, &h, ORCH, "update_task", json!({"id": "a", "title": "T"})).unwrap();
    assert!(r["task"].get("task").is_none());
    let r = call(&st, &h, ORCH, "update_task", json!({"id": "a", "task": "new text"})).unwrap();
    assert_eq!(r["task"]["task"], "new text");
}

#[test]
fn get_workflow_exposes_workspace_rules_and_repo_settings_read_only() {
    let (st, h) = multi_repo();
    assert_eq!(call(&st, &h, ORCH, "get_workflow", json!({})).unwrap(), json!({"workflow": null}));
    *h.worker_rules.lock().unwrap() = "  never touch prod  ".to_string();
    let mut cfg = setup_cfg();
    cfg.base_branch = Some("develop".into());
    cfg.remove_force = true;
    cfg_of(&h, "r1", cfg);
    cfg_of(&h, "r2", crate::workspaces::RepoConfig::default());
    let r = call(&st, &h, ORCH, "get_workflow", json!({})).unwrap();
    assert_eq!(r["workspace_rules"], "never touch prod");
    let s = &r["repo_settings"];
    assert!(s.get("r2").is_none(), "{s}");
    assert_eq!(s["r1"]["base_branch"], "develop");
    assert_eq!(s["r1"]["container_root"], "/var/www/html");
    assert_eq!(s["r1"]["copy_ignored"], json!(["configuration.php"]));
    assert_eq!(s["r1"]["setup"][1], "composer dump-autoload");
    assert_eq!(s["r1"]["cleanup"], json!(["docker exec svc rm -rf cache"]));
    assert_eq!(s["r1"]["remove_force"], true);
    assert!(r["note"].as_str().unwrap().contains("read-only"));
}

// --- deleted ---

fn held(id: &str, deps: &[&str]) -> Value {
    json!({"id": id, "task": format!("do {id}"), "depends_on": deps, "held": true, "isolation": "none"})
}

fn status_of(st: &AutomationState, h: &FakeHost, id: &str, status: &str) -> Result<Value, String> {
    call(st, h, ORCH, "set_task_status", json!({"id": id, "status": status}))
}

#[test]
fn deleting_a_queued_task_removes_it_from_the_board() {
    let (st, h) = st_h();
    submit(&st, &h, vec![held("a", &[]), held("b", &[])]);
    let r = status_of(&st, &h, "a", "deleted").unwrap();
    assert_eq!(r, json!({"deleted": "a"}));
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(b["tasks"].as_array().unwrap().len(), 1, "{b}");
    assert_eq!(b["held"], 1);
    assert!(call(&st, &h, ORCH, "get_task_details", json!({"id": "a"})).is_err());
    // Un task annullato prima di partire si cancella allo stesso modo.
    status_of(&st, &h, "b", "cancelled").unwrap();
    assert_eq!(status_of(&st, &h, "b", "deleted").unwrap(), json!({"deleted": "b"}));
    assert!(status_of(&st, &h, "b", "deleted").unwrap_err().contains("not found"));
}

#[test]
fn deleting_the_last_open_task_completes_the_plan() {
    let (st, h) = st_h();
    submit(&st, &h, vec![t("a", &[]), held("b", &[])]);
    let w = session_of(&st, &h, "a");
    finish(&st, &h, &w, "completed");
    status_of(&st, &h, "a", "done").unwrap();
    assert!(!kinds_of(&events(&st, &h, ORCH)).contains(&"plan_completed".to_string()));
    status_of(&st, &h, "b", "deleted").unwrap();
    assert!(kinds_of(&events(&st, &h, ORCH)).contains(&"plan_completed".to_string()));
}

#[test]
fn delete_is_refused_for_started_running_done_and_depended_on_tasks() {
    let (st, h) = st_h();
    submit(&st, &h, vec![held("a", &[]), held("b", &["a"]), t("run", &[]), held("c", &[])]);
    // Ha dipendenti: l'errore li nomina.
    let e = status_of(&st, &h, "a", "deleted").unwrap_err();
    assert!(e.contains("depends") || e.contains("dependency"), "{e}");
    assert!(e.contains('b'), "{e}");
    // In esecuzione.
    let e = status_of(&st, &h, "run", "deleted").unwrap_err();
    assert!(e.contains("running"), "{e}");
    // Gia' partito e poi annullato (attempts > 0).
    let w = session_of(&st, &h, "run");
    let _ = w;
    status_of(&st, &h, "run", "cancelled").unwrap();
    let e = status_of(&st, &h, "run", "deleted").unwrap_err();
    assert!(e.contains("already started"), "{e}");
    // Concluso.
    status_of(&st, &h, "c", "done").unwrap_err(); // serve una nota
    call(&st, &h, ORCH, "set_task_status", json!({"id": "c", "status": "done", "note": "by hand"})).unwrap();
    let e = status_of(&st, &h, "c", "deleted").unwrap_err();
    assert!(e.contains("done"), "{e}");
    // Con un worktree registrato.
    st.inner.lock().unwrap().boards.get_mut("ws-1").unwrap().task_mut("b").unwrap().worktree = Some(board::WorktreeInfo {
        path: r"C:\p\.worktrees\b".into(),
        branch: "task/b".into(),
        base: "main".into(),
        repo: None,
        setup_done: true,
    });
    let e = status_of(&st, &h, "b", "deleted").unwrap_err();
    assert!(e.contains("already started") && e.contains("worktree"), "{e}");
    assert_eq!(st.inner.lock().unwrap().boards["ws-1"].tasks.len(), 4);
}

#[test]
fn user_can_delete_from_the_board_with_the_same_rules() {
    let (st, h) = st_h();
    submit(&st, &h, vec![held("a", &[]), held("b", &["a"])]);
    let act = |task: &str| boards::board_action(&st, &h, "ws-1", task, "delete");
    assert!(act("a").is_err(), "ha un dipendente");
    let v = act("b").unwrap();
    assert_eq!(v.tasks.len(), 1);
    let v = act("a").unwrap();
    assert!(v.tasks.is_empty());
    assert!(act("a").is_err());
    let ev = events(&st, &h, ORCH);
    let ua: Vec<_> = ev.iter().filter(|e| e["kind"] == "user_action").collect();
    assert_eq!(ua.len(), 2, "{ev:?}");
    assert!(ua[0]["text"].as_str().unwrap().contains("deleted"));
}

// --- close_integration_cleanup ---

fn close_cleanup(st: &AutomationState, h: &FakeHost) -> Value {
    call(st, h, ORCH, "set_board", json!({"close_integration": "r1", "close_integration_cleanup": true})).unwrap()
}

#[test]
fn close_integration_cleanup_removes_worktree_and_merged_branch() {
    let (st, h) = multi_repo();
    let mut cfg = setup_cfg();
    cfg.remove_force = true;
    cfg_of(&h, "r1", cfg);
    int_plan(&st, &h);
    h.dirs.lock().unwrap().push(INT_DIR.to_string());
    h.git_rule("rev-parse --abbrev-ref --symbolic-full-name int@{upstream}", true, "origin/int\n", "");
    let n = h.git_log().len();
    let r = close_cleanup(&st, &h);
    assert!(r.get("warnings").is_none(), "{r}");
    assert!(!st.inner.lock().unwrap().boards["ws-1"].integrations.contains_key("r1"));
    let git: Vec<String> = h.git_log()[n..].to_vec();
    let at = |needle: &str| git.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle} in {git:?}"));
    let remove = at(&format!(r"C:\p\r1|worktree remove --force {INT_DIR}"));
    assert!(at("hook|docker exec svc rm -rf cache") < remove);
    assert!(remove < at(r"C:\p\r1|worktree prune"));
    assert!(at(r"C:\p\r1|merge-base --is-ancestor int int@{upstream}") < at(r"C:\p\r1|branch -D int"));
    assert!(!git.iter().any(|l| l.contains("push")), "{git:?}");
}

#[test]
fn close_integration_cleanup_keeps_the_branch_without_upstream_or_unmerged() {
    // Nessun upstream (stdout vuoto).
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let r = close_cleanup(&st, &h);
    assert!(r["warnings"][0].as_str().unwrap().contains("no upstream"), "{r}");
    assert!(!h.git_log().iter().any(|l| l.contains("branch -D")), "{:?}", h.git_log());
    // Upstream che non contiene il branch.
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    h.dirs.lock().unwrap().push(INT_DIR.to_string());
    h.git_rule("rev-parse --abbrev-ref --symbolic-full-name int@{upstream}", true, "origin/int\n", "");
    h.git_rule("merge-base --is-ancestor int int@{upstream}", false, "", "");
    let r = close_cleanup(&st, &h);
    assert!(r["warnings"][0].as_str().unwrap().contains("not contained"), "{r}");
    let git = h.git_log();
    assert!(git.contains(&format!(r"C:\p\r1|worktree remove {INT_DIR}")), "{git:?}");
    assert!(!git.iter().any(|l| l.contains("branch -D")), "{git:?}");
    assert!(!st.inner.lock().unwrap().boards["ws-1"].integrations.contains_key("r1"));
}

#[test]
fn close_integration_cleanup_requires_close_integration() {
    let (st, h) = multi_repo();
    int_plan(&st, &h);
    let e = call(&st, &h, ORCH, "set_board", json!({"max_parallel": 2, "close_integration_cleanup": true})).unwrap_err();
    assert!(e.contains("requires close_integration"), "{e}");
    let e = call(&st, &h, ORCH, "set_board", json!({"close_integration": "r1", "close_integration_cleanup": "yes"})).unwrap_err();
    assert!(e.contains("boolean"), "{e}");
    assert!(st.inner.lock().unwrap().boards["ws-1"].integrations.contains_key("r1"));
}
