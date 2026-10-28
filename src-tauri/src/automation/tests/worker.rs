//! Ciclo di vita del worker: get_task, hook (promemoria dello Stop, attenzione,
//! background), complete_task, chiusura del tab, pulizia, guardiano.

use super::*;
use crate::board::texts::WORKER_GUARDRAILS;

#[test]
fn get_task_returns_task_instructions_environment_and_delivery() {
    let h = FakeHost::with_delivery(DeliveryMode::Pr, true);
    *h.worker_rules.lock().unwrap() = "never run e2e".to_string();
    let st = AutomationState::default();
    queue(&st, &h, vec![spec("a", &[])]);
    let w = session_of(&st, &h, "a");
    assert_eq!(task(&st, &h, "a")["worker_status"], "starting");
    let t = call(&st, &h, &w, "get_task", json!({})).unwrap();
    assert_eq!(keys(&t), ["delivery", "environment", "instructions", "task", "title"]);
    assert_eq!((t["task"].as_str(), t["title"].as_str()), (Some("do a"), Some("Task a")));
    let instr = t["instructions"].as_str().unwrap();
    assert!(instr.contains("Work only inside C:\\p\\.worktrees\\a") && instr.contains("complete_task"));
    // Standard di ingegneria prima delle regole del workspace.
    let g = instr.find(WORKER_GUARDRAILS.trim_end()).expect("guardrails in get_task instructions");
    let rules = instr.find("Workspace rules (always apply").unwrap();
    assert!(g < rules && instr.ends_with("never run e2e"));
    let env = t["environment"].as_str().unwrap();
    assert!(env.contains("branch task/a (created from main)") && env.contains("base branch: main"), "{env}");
    assert!(t["delivery"].as_str().unwrap().contains("gh pr create --base main --head task/a"));
    assert_eq!(task(&st, &h, "a")["worker_status"], "running");
    // Seconda lettura: stesso risultato, nessun errore.
    assert!(call(&st, &h, &w, "get_task", json!({})).is_ok());
}

#[test]
fn roles_gate_the_tools() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    for m in ["propose_tasks", "list_tasks", "get_task_details", "update_task", "delete_task"] {
        let e = call(&st, &h, &w, m, json!({})).unwrap_err();
        assert!(e.contains("not available to automated sessions"), "{m}: {e}");
    }
    for m in ["get_task", "complete_task"] {
        let e = call(&st, &h, INTAKE, m, json!({})).unwrap_err();
        assert!(e.contains("reserved for automated sessions"), "{m}: {e}");
    }
    assert_eq!(call(&st, &h, &w, "whoami", json!({})).unwrap()["role"], "worker");
    let me = call(&st, &h, INTAKE, "whoami", json!({})).unwrap();
    assert_eq!(me["role"], "session");
    assert_eq!(me["workspace"], json!({"name":"Proj","kind":"windows","path":"C:\\p"}));
    assert!(call(&st, &h, "tab-unknown", "propose_tasks", json!({"tasks":[spec("x", &[])]})).unwrap_err().contains("workspace"));
    assert!(call(&st, &h, INTAKE, "get_events", json!({})).unwrap_err().contains("unknown method"));
    assert!(call(&st, &h, INTAKE, "list_tasks", json!([1])).is_err());
    // Un workspace archiviato e' congelato.
    *h.archived.lock().unwrap() = true;
    assert_eq!(call(&st, &h, INTAKE, "list_tasks", json!({})).unwrap_err(), "workspace is archived");
}

#[test]
fn completed_pr_task_is_done_and_cleaned_after_the_tab_closes() {
    let (st, h, w) = running(DeliveryMode::Pr, true);
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    let r = call(
        &st,
        &h,
        &w,
        "complete_task",
        json!({"status": "completed", "summary": "ok", "commit": "abc1234", "pr_url": "https://github.com/o/r/pull/7",
               "files_changed": ["src/a.rs"], "tests": [{"command": "cargo test", "result": "passed"}], "follow_ups": ["docs"]}),
    )
    .unwrap();
    assert_eq!(r, json!({"closing": true}));
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["last_report"].as_str()), (Some("done"), Some("ok")));
    assert!(a["session"].is_null() && a["worker_status"].is_null());
    assert_eq!(a["report"]["status"], "completed");
    assert_eq!(a["report"]["pr_url"], "https://github.com/o/r/pull/7");
    assert_eq!(a["report"]["tests"][0]["result"], "passed");
    assert!(a["report"]["diffstat"].as_str().unwrap().contains("2 files changed"));
    assert!(h.git_log().iter().any(|l| l == "C:\\p\\.worktrees\\a|diff --stat main...HEAD"), "{:?}", h.git_log());
    // Il tab si chiude dopo 1,5 s; fino ad allora il worktree non si tocca.
    let log = h.log();
    assert!(log.contains(&"defer:1500".to_string()) && log.contains(&format!("close:{w}")));
    assert!(!h.git_log().iter().any(|l| l.contains("worktree remove") || l.contains("branch -")), "{:?}", h.git_log());
    assert!(a["worktree"].is_object());
    // Tab chiuso: pulizia (cleanup, worktree remove, branch -D per le PR).
    tab_closed(&st, &h, &w);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|worktree remove C:\\p\\.worktrees\\a".to_string()), "{git:?}");
    assert!(git.contains(&"C:\\p|branch -D task/a".to_string()), "{git:?}");
    assert!(h.log().contains(&"defer:2000".to_string()));
    let a = task(&st, &h, "a");
    assert!(a["worktree"].is_null() && a["note"].is_null());
    // Il tab e' chiuso: i tool non rispondono piu'.
    assert!(call(&st, &h, &w, "get_task", json!({})).unwrap_err().contains("closed"));
    // Chiudere di nuovo non ripulisce due volte.
    let n = h.git_log().len();
    tab_closed(&st, &h, &w);
    assert_eq!(h.git_log().len(), n);
}

#[test]
fn local_merge_cleanup_deletes_the_branch_without_force() {
    let (st, h, w) = running(DeliveryMode::LocalMerge, true);
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    finish(&st, &h, &w, "completed");
    tab_closed(&st, &h, &w);
    let git = h.git_log();
    assert!(git.contains(&"C:\\p|branch -d task/a".to_string()), "{git:?}");
    assert!(!git.iter().any(|l| l.contains("branch -D")));
}

#[test]
fn cleanup_runs_the_repo_hooks_and_warnings_never_block() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    h.repo_cfg.lock().unwrap().insert(".".into(), RepoConfig { cleanup: vec!["docker compose down".into()], ..Default::default() });
    queue(&st, &h, vec![spec("a", &[])]);
    let w = session_of(&st, &h, "a");
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    h.git_rule("worktree remove", false, "", "contains modified or untracked files");
    finish(&st, &h, &w, "completed");
    tab_closed(&st, &h, &w);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "done");
    assert!(a["note"].as_str().unwrap().contains("worktree not removed"), "{a}");
    assert!(a["worktree"].is_object(), "il worktree resta per chi lo ripulisce a mano");
    let hooks = h.hook_log.lock().unwrap().clone();
    assert!(hooks[0].starts_with("C:\\p\\.worktrees\\a|docker compose down|"), "{hooks:?}");
    assert!(!h.git_log().iter().any(|l| l.contains("branch -")), "branch kept when the worktree could not be removed");
}

#[test]
fn failed_task_keeps_its_worktree_and_notifies() {
    let (st, h, w) = running(DeliveryMode::Pr, true);
    h.dirs.lock().unwrap().push("C:\\p\\.worktrees\\a".to_string());
    finish(&st, &h, &w, "failed");
    tab_closed(&st, &h, &w);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert_eq!(a["report"]["status"], "failed");
    assert!(a["worktree"]["branch"] == "task/a");
    assert!(!h.git_log().iter().any(|l| l.contains("worktree remove") || l.contains("branch -")), "{:?}", h.git_log());
    let n = h.notices();
    assert_eq!(n.len(), 1);
    assert_eq!((n[0].kind, n[0].task_id.as_str(), n[0].body.as_str()), ("failed", "a", "Task a è fallito"));
}

#[test]
fn blocked_is_failed_and_bad_input_is_rejected() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    for bad in [
        json!({"status": "bogus", "summary": "x"}),
        json!({"status": "completed", "summary": "   "}),
        json!({"status": "completed", "summary": "x".repeat(SUMMARY_MAX + 1)}),
        json!({"status": "completed"}),
        json!({"summary": "x"}),
        json!({"status": "completed", "summary": "x", "pr_url": "javascript:alert(1)"}),
    ] {
        assert!(call(&st, &h, &w, "complete_task", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(task(&st, &h, "a")["status"], "running", "rejected input leaves the task running");
    call(&st, &h, &w, "complete_task", json!({"status": "blocked", "summary": "no credentials"})).unwrap();
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["report"]["status"].as_str()), (Some("failed"), Some("failed")));
}

#[test]
fn stop_hook_reminds_once_then_the_worker_needs_attention() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    // Primo Stop senza report: blocco con il promemoria.
    let reason = stop(&st, &h, &w, None, None).expect("reminder");
    assert_eq!(reason, STOP_BLOCK_REASON);
    assert!(reason.contains("call complete_task") && reason.contains("the user will answer here"));
    assert_eq!(stop(&st, &h, &w, Some(false), Some(0)).as_deref(), Some(STOP_BLOCK_REASON));
    let a = task(&st, &h, "a");
    assert_eq!(a["worker_status"], "running");
    assert!(h.notices().is_empty());
    // Secondo Stop (stop_hook_active): attende l'utente.
    assert_eq!(stop(&st, &h, &w, Some(true), None), None);
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["worker_status"].as_str()), (Some("running"), Some("idle")));
    assert!(view(&st, &h).tasks[0].attention);
    assert_eq!(h.notices().len(), 1);
    assert_eq!(h.notices()[0].kind, "attention");
    assert_eq!(h.notices()[0].body, "Task a richiede attenzione");
    assert_eq!(h.notices()[0].tab_id.as_deref(), Some(w.as_str()));
    // Altri Stop in idle: nessuna nuova notifica.
    assert_eq!(stop(&st, &h, &w, Some(true), None), None);
    assert_eq!(h.notices().len(), 1);
    // L'utente risponde nel terminale (Invio): riprende, l'attenzione sparisce.
    st.run(&h, |r| r.user_input(&w, 5));
    assert!(!view(&st, &h).tasks[0].attention);
    assert_eq!(task(&st, &h, "a")["worker_status"], "running");
    // Un nuovo turno finito senza report: di nuovo promemoria, poi attenzione.
    assert!(stop(&st, &h, &w, None, None).is_some());
    stop(&st, &h, &w, Some(true), None);
    assert_eq!(h.notices().len(), 2);
    // Un Invio in un tab che non aspetta non cambia nulla.
    st.run(&h, |r| r.user_input("auto-nope", 6));
}

#[test]
fn stop_before_get_task_is_nudged_then_idle() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    assert!(stop(&st, &h, &w, None, None).is_some());
    assert_eq!(task(&st, &h, "a")["worker_status"], "starting");
    assert!(stop(&st, &h, &w, Some(true), None).is_none());
    assert_eq!(task(&st, &h, "a")["worker_status"], "idle");
}

#[test]
fn background_work_is_not_an_abandonment() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    assert_eq!(stop(&st, &h, &w, None, Some(2)), None);
    assert_eq!(task(&st, &h, "a")["worker_status"], "background");
    assert!(!view(&st, &h).tasks[0].attention && h.notices().is_empty());
    let info = h.log().into_iter().rev().find(|l| l.starts_with("update:")).unwrap();
    assert!(info.ends_with("Background"), "{info}");
    // Il lavoro in background finisce: turno chiuso senza report, promemoria.
    assert!(stop(&st, &h, &w, None, Some(0)).is_some());
}

#[test]
fn permission_requests_need_attention_once() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    // `idle_prompt` non e' una richiesta di permesso.
    let idle = HookEvent { event: "needs-input", notification_type: Some("idle_prompt"), ..Default::default() };
    st.run(&h, |r| r.on_hook(&w, &idle, 1));
    assert_eq!(task(&st, &h, "a")["worker_status"], "running");
    hook(&st, &h, &w, "needs-input");
    hook(&st, &h, &w, "needs-input");
    let a = task(&st, &h, "a");
    assert_eq!(a["worker_status"], "needs_input");
    assert_eq!(h.notices().len(), 1, "ripetuto: nessun duplicato");
    assert!(view(&st, &h).tasks[0].attention);
    // Risolto il permesso l'utente preme Invio: riprende; un nuovo permesso notifica ancora.
    st.run(&h, |r| r.user_input(&w, 2));
    assert_eq!(task(&st, &h, "a")["worker_status"], "running");
    hook(&st, &h, &w, "needs-input");
    assert_eq!(h.notices().len(), 2);
    // Il Stop che segue un permesso risolto mette in moto il promemoria.
    assert!(stop(&st, &h, &w, None, None).is_some());
}

#[test]
fn session_end_or_closed_tab_without_report_fails_the_task() {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(DeliveryMode::Pr, true));
    queue(&st, &h, vec![spec("a", &[]), spec("b", &[]), spec("c", &[])]);
    let (wa, wb, wc) = (session_of(&st, &h, "a"), session_of(&st, &h, "b"), session_of(&st, &h, "c"));
    for w in [&wa, &wb, &wc] {
        call(&st, &h, w, "get_task", json!({})).unwrap();
    }
    // Tab chiuso dall'utente.
    tab_closed(&st, &h, &wa);
    let a = task(&st, &h, "a");
    assert_eq!(a["status"], "failed");
    assert!(a["note"].as_str().unwrap().contains("closed"), "{a}");
    // session-end dell'agente.
    hook(&st, &h, &wb, "session-end");
    let b = task(&st, &h, "b");
    assert_eq!(b["status"], "failed");
    assert!(b["note"].as_str().unwrap().contains("session ended"), "{b}");
    // Il tab chiuso dopo non cambia nulla e non notifica di nuovo.
    tab_closed(&st, &h, &wb);
    assert_eq!(task(&st, &h, "b")["status"], "failed");
    assert_eq!(h.notices().iter().filter(|n| n.kind == "failed").count(), 2);
    // Il worktree resta per il retry.
    assert!(task(&st, &h, "a")["worktree"].is_object());
    assert!(!h.git_log().iter().any(|l| l.contains("worktree remove")));
    // c e' ancora in corso.
    assert_eq!(task(&st, &h, "c")["status"], "running");
}

#[test]
fn a_finished_worker_is_not_failed_by_its_tab_closing() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    finish(&st, &h, &w, "completed");
    hook(&st, &h, &w, "session-end");
    tab_closed(&st, &h, &w);
    assert_eq!(task(&st, &h, "a")["status"], "done");
    assert!(h.notices().is_empty());
    assert!(st.open_tabs().is_empty());
}

#[test]
fn retry_hands_the_previous_outcome_to_the_new_worker() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    assert!(call(&st, &h, &w, "get_task", json!({})).unwrap().get("previous_attempt").is_none());
    call(&st, &h, &w, "complete_task", json!({"status": "failed", "summary": "two tests are red"})).unwrap();
    tab_closed(&st, &h, &w);
    boards::task_action(&st, &h, "ws-1", "a", "retry").unwrap();
    // Subito riparte (nessun altro in corso): nuovo worker, tentativo 2.
    let a = task(&st, &h, "a");
    assert_eq!((a["status"].as_str(), a["attempts"].as_u64()), (Some("running"), Some(2)));
    let w2 = session_of(&st, &h, "a");
    assert_ne!(w, w2);
    let t = call(&st, &h, &w2, "get_task", json!({})).unwrap();
    assert_eq!(t["previous_attempt"], json!({"summary": "two tests are red"}));
    // Il nuovo esito pulisce il precedente.
    finish(&st, &h, &w2, "completed");
    assert!(task(&st, &h, "a").get("previous_attempt").is_none_or(Value::is_null));
}

#[test]
fn silent_worker_is_failed_by_the_watchdog() {
    let (st, h, w) = running(DeliveryMode::Commit, false);
    // Ha chiamato get_task: non e' silenzioso.
    let late = now_millis() + 6 * 60_000;
    st.run(&h, |r| r.watchdog(late));
    assert_eq!(task(&st, &h, "a")["status"], "failed", "never called get_task");
    assert!(task(&st, &h, "a")["note"].as_str().unwrap().contains("never called get_task"));
    assert!(h.log().contains(&format!("close:{w}")));
    assert_eq!(h.notices().len(), 1);
    // Con get_task fatto il guardiano non scatta.
    let (st, h, w) = running(DeliveryMode::Commit, false);
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    st.run(&h, |r| r.watchdog(late));
    assert_eq!(task(&st, &h, "a")["status"], "running");
}

#[test]
fn tokens_and_rpc_authentication() {
    let t = token();
    assert_eq!(t.len(), 32);
    assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(t, token());
    assert_ne!(generate_token(), generate_token());
    assert!(token_matches(Some(t)) && !token_matches(Some("")) && !token_matches(Some("zzz")) && !token_matches(None));
    let req = |rpc, token: &str, session: &str| RpcRequest {
        rpc,
        token: token.to_string(),
        session: session.to_string(),
        method: "whoami".to_string(),
        params: Value::Null,
    };
    assert!(authenticate(&req(1, "abc", "tab-1"), "abc").is_ok());
    assert!(authenticate(&req(2, "abc", "tab-1"), "abc").is_err());
    assert!(authenticate(&req(1, "", "tab-1"), "").is_err());
    assert!(authenticate(&req(1, "x", "tab-1"), "abc").is_err());
    assert!(authenticate(&req(1, "abc", "a b"), "abc").is_err());
    assert!(authenticate(&req(1, "abc", ""), "abc").is_err());
}

#[test]
fn helpers_validate_text() {
    assert_eq!(req_text("t", "  x ", 3).unwrap(), "x");
    assert!(req_text("t", "  ", 3).is_err() && req_text("t", "abcd", 3).is_err());
    assert_eq!(clip_chars("àèìòù", 2), "àè");
    assert!(valid_model("claude-opus-4.1_x") && !valid_model("a b") && !valid_model(&"x".repeat(65)) && !valid_model(""));
    assert_eq!(req_str(&json!({"a": "x"}), "a").unwrap(), "x");
    assert!(req_str(&json!({"a": 1}), "a").is_err() && req_str(&json!({}), "a").is_err());
}

#[test]
fn closed_workers_are_pruned_beyond_the_cap() {
    let mut r = Registry::default();
    for i in 0..(MAX_TRACKED_WORKERS + 10) {
        let env = WorkerEnv { delivery: Delivery::default(), repo_dir: String::new(), workdir: String::new(), branch: None, base: None };
        let new = NewWorker { workspace_id: "w".into(), agent_id: "a".into(), title: "t".into(), cwd: String::new(), command: String::new(), kind: crate::board::TaskKind::Work };
        let id = r.spawn_worker("w", &format!("t{i}"), 1, new, env, i as i64).tab_id;
        r.tab_closed(&id, 1);
    }
    assert!(r.workers.len() <= MAX_TRACKED_WORKERS + 1);
}
