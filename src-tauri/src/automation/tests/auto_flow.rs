//! Esecuzione automatica con l'host finto: finestre, orchestratore
//! automatico (apertura, chiusura, drain), gate dello scheduler, standby,
//! workflow. L'ora locale e' un campo dell'host finto (`minute`).

use super::*;
use crate::board;
use crate::schedule::{parse_hhmm, Schedule, Window};
use crate::workspaces::AutoOrchestrate;

const NOW: i64 = 1_700_000_000_000;
/// Uguale a `SPAWN_GRACE_MS` di auto.rs.
const GRACE_MS: i64 = 60_000;

fn night() -> Schedule {
    Schedule {
        enabled: true,
        windows: vec![Window { start: "20:00".into(), end: "13:00".into() }],
        keep_awake: true,
    }
}

fn at(h: &FakeHost, hhmm: &str) {
    *h.minute.lock().unwrap() = parse_hhmm(hhmm).unwrap();
}

fn auto_cfg(enabled: bool) -> AutoOrchestrate {
    AutoOrchestrate { enabled, agent: "claude-code".into(), model: Some("opus".into()) }
}

/// Stato con ws-1 automatico, schedule notturno e ora locale `hhmm`.
fn setup(hhmm: &str) -> (AutomationState, FakeHost) {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(true));
    at(&h, hhmm);
    st.schedule_set(&h, night(), NOW).unwrap();
    (st, h)
}

fn tick(st: &AutomationState, h: &FakeHost) {
    st.tick(h, NOW);
}

/// Board senza proprietario (come dopo un riavvio dell'app).
fn load_work(st: &AutomationState, h: &FakeHost, tasks: Vec<Value>) {
    let plan = board::parse_plan(&json!({ "tasks": tasks })).unwrap();
    let mut b = Board::new("ws-1");
    b.add_plan(plan.specs, PermissionMode::AcceptEdits, 1).unwrap();
    st.run(h, |r| r.load_board(b));
}

fn t(id: &str) -> Value {
    json!({"id": id, "task": format!("do {id}")})
}

fn dep(id: &str, on: &str) -> Value {
    json!({"id": id, "task": format!("do {id}"), "depends_on": [on]})
}

fn spawns_of(h: &FakeHost, role: Role) -> Vec<SpawnEvent> {
    h.spawns().into_iter().filter(|s| s.role == role).collect()
}

fn orch_tabs(h: &FakeHost) -> Vec<String> {
    spawns_of(h, Role::Orchestrator).into_iter().map(|s| s.tab_id).collect()
}

fn finished(st: &AutomationState, h: &FakeHost, tab: &str) {
    st.run(h, |r| r.on_hook(tab, &HookEvent { event: "finished", ..Default::default() }, NOW));
}

fn hook(st: &AutomationState, h: &FakeHost, tab: &str, event: &str) {
    st.run(h, |r| r.on_hook(tab, &HookEvent { event, ..Default::default() }, NOW));
}

fn writes_to(h: &FakeHost, tab: &str) -> Vec<String> {
    h.log().into_iter().filter(|l| l.starts_with(&format!("write:{tab}:"))).collect()
}

fn closed_texts(h: &FakeHost, tab: &str) -> usize {
    writes_to(h, tab).iter().filter(|l| l.contains("execution window is closed")).count()
}

fn is_closed(h: &FakeHost, tab: &str) -> bool {
    h.log().contains(&format!("close:{tab}"))
}

fn board_task(st: &AutomationState, _h: &FakeHost, id: &str) -> String {
    let v = st.inner.lock().unwrap().task_view("ws-1", id);
    v.map(|v| v.task.status.as_str().to_string()).unwrap_or_default()
}

/// Il tab del pty compare (il frontend lo ha aperto).
fn pty_up(h: &FakeHost, tab: &str) {
    h.live.lock().unwrap().push(tab.to_string());
}

fn pty_down(h: &FakeHost, tab: &str) {
    h.live.lock().unwrap().retain(|t| t != tab);
}

// ---------------------------------------------------------------------------
// Apertura
// ---------------------------------------------------------------------------

#[test]
fn opening_with_work_spawns_the_automatic_orchestrator_and_restarts_the_scheduler() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    // Finestra chiusa: niente orchestratore, il task non parte.
    assert!(h.spawns().is_empty());
    assert_eq!(board_task(&st, &h, "a"), "queued");
    let view = st.run(&h, |r| r.board_view("ws-1"));
    let auto = view.automation.expect("workspace automatico");
    assert!(auto.enabled && !auto.window_open && auto.next_change_at.is_some());

    at(&h, "20:00");
    tick(&st, &h);
    let orch = spawns_of(&h, Role::Orchestrator);
    assert_eq!(orch.len(), 1);
    let o = &orch[0];
    assert_eq!(o.workspace_id, "ws-1");
    assert_eq!(o.parent_tab_id, "");
    assert_eq!(o.agent_id, "claude-code");
    assert!(o.cwd.is_none() && !o.title.is_empty());
    // Permessi = tetto del workspace, modello dalla configurazione, prompt dedicato.
    assert!(o.command.contains("AcceptEdits") && o.command.contains("model=opus"), "{}", o.command);
    assert!(o.command.contains(ORCHESTRATOR_BOOTSTRAP_PROMPT));
    // Lo scheduler e' ripartito: il task e' stato avviato come worker.
    assert_eq!(spawns_of(&h, Role::Worker).len(), 1);
    assert_eq!(board_task(&st, &h, "a"), "running");
    let auto = st.run(&h, |r| r.board_view("ws-1")).automation.unwrap();
    assert!(auto.window_open);
    // Il payload porta il ruolo.
    let v = serde_json::to_value(o).unwrap();
    assert_eq!(v["role"], "orchestrator");
    assert_eq!(serde_json::to_value(&spawns_of(&h, Role::Worker)[0]).unwrap()["role"], "worker");

    // Un altro giro non ne apre un secondo.
    tick(&st, &h);
    tick(&st, &h);
    assert_eq!(spawns_of(&h, Role::Orchestrator).len(), 1);
}

#[test]
fn opening_without_work_or_with_only_held_tasks_spawns_nothing() {
    let (st, h) = setup("21:00");
    tick(&st, &h);
    assert!(h.spawns().is_empty(), "nessuna board");
    load_work(&st, &h, vec![json!({"id": "a", "task": "x", "held": true})]);
    tick(&st, &h);
    assert!(h.spawns().is_empty(), "task trattenuto: niente lavoro");
    // Un task in review e' lavoro da integrare.
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a")]);
    st.run(&h, |r| {
        let b = r.boards.get_mut("ws-1").unwrap();
        b.start_task("a", 5);
        b.finish_running("a", None, board::TaskStatus::Review, None, None, 6);
    });
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
}

#[test]
fn a_live_owner_is_not_duplicated_and_a_dead_one_is_replaced() {
    let (st, h) = setup("15:00");
    // Plan sottoposto da un orchestratore dell'utente (tab vivo): diventa owner.
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    // Finestra chiusa: il gate trattiene il task, la pausa non si tocca.
    assert_eq!(board_task(&st, &h, "a"), "queued");
    assert!(!st.run(&h, |r| r.board_view("ws-1")).paused);
    at(&h, "20:00");
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty(), "owner vivo");
    assert_eq!(board_task(&st, &h, "a"), "running", "lo scheduler riparte comunque");

    // Owner con tab morto: nuova apertura con lavoro in coda.
    let (st, h) = setup("15:00");
    call(&st, &h, "orch-b", "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    // "orch-b" e' sempre vivo nell'host finto: usiamo un owner inesistente.
    st.run(&h, |r| r.boards.get_mut("ws-1").unwrap().owner = Some("tab-morto".into()));
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
}

#[test]
fn schedule_disabled_or_non_automatic_workspaces_ignore_windows() {
    // Schedule spento: i workspace automatici si comportano come i normali.
    let (st, h) = (AutomationState::default(), FakeHost::default());
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(true));
    at(&h, "15:00");
    st.schedule_set(&h, Schedule { enabled: false, ..night() }, NOW).unwrap();
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty());
    // Workspace non automatico con schedule attivo e finestra chiusa.
    let (st, h) = (AutomationState::default(), FakeHost::default());
    at(&h, "15:00");
    st.schedule_set(&h, night(), NOW).unwrap();
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert!(call(&st, &h, ORCH, "get_board", json!({})).unwrap()["automation"].is_null());
    // Workspace disabilitato (enabled: false): idem.
    let (st, h) = (AutomationState::default(), FakeHost::default());
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(false));
    at(&h, "15:00");
    st.schedule_set(&h, night(), NOW).unwrap();
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
}

#[test]
fn closed_window_blocks_starts_without_touching_pause() {
    let (st, h) = setup("15:00");
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a"), t("b")], "max_parallel": 2})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "queued");
    assert_eq!(board_task(&st, &h, "b"), "queued");
    // Anche un retry/unhold dell'utente non aggira il gate.
    st.run(&h, |r| r.set_held("ws-1", "b", true, NOW)).unwrap();
    st.run(&h, |r| r.set_held("ws-1", "b", false, NOW)).unwrap();
    assert_eq!(board_task(&st, &h, "b"), "queued");
    assert!(h.spawns().is_empty());
    let b = call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(b["paused"], false);
    assert_eq!(b["automation"]["window_open"], false);
    assert_eq!(b["automation"]["enabled"], true);
    assert!(b["automation"]["next_change_at"].is_i64());
    // L'utente mette in pausa: l'apertura non la toglie.
    st.run(&h, |r| r.set_board("ws-1", None, Some(true), NOW)).unwrap();
    at(&h, "20:30");
    tick(&st, &h);
    assert_eq!(board_task(&st, &h, "a"), "queued");
    assert!(st.run(&h, |r| r.board_view("ws-1")).paused);
    st.run(&h, |r| r.set_board("ws-1", None, Some(false), NOW)).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert_eq!(board_task(&st, &h, "b"), "running");
    // Chiusura: i running finiscono, ma niente nuovi task; la pausa resta false.
    at(&h, "13:00");
    tick(&st, &h);
    assert!(!st.run(&h, |r| r.board_view("ws-1")).paused);
}

// ---------------------------------------------------------------------------
// Chiusura e drain
// ---------------------------------------------------------------------------

/// Apertura con un task in corso e l'orchestratore automatico che ha
/// reclamato la board. Ritorna (tab orchestratore, sessione del worker).
fn running_night() -> (AutomationState, FakeHost, String, String) {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), json!({"id": "b", "task": "x", "depends_on": ["a"]})]);
    at(&h, "21:00");
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    tick(&st, &h);
    call(&st, &h, &tab, "get_board", json!({})).unwrap();
    let w = spawns_of(&h, Role::Worker)[0].tab_id.clone();
    (st, h, tab, w)
}

#[test]
fn close_types_the_window_text_then_drains_and_closes_the_tab() {
    let (st, h, tab, w) = running_night();
    assert_eq!(st.run(&h, |r| r.board_view("ws-1")).owner.as_deref(), Some(tab.as_str()));
    finished(&st, &h, &tab); // l'orchestratore ha finito il suo turno
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);

    at(&h, "13:00");
    tick(&st, &h);
    // Idle: il testo di chiusura viene digitato (una volta sola).
    assert_eq!(closed_texts(&h, &tab), 1, "{:?}", h.log());
    assert!(writes_to(&h, &tab)[0].contains("\\u{1b}[200~") || writes_to(&h, &tab)[0].contains("200~"));
    tick(&st, &h);
    assert_eq!(closed_texts(&h, &tab), 1);
    // Il task in corso non e' interrotto e nessun task nuovo parte.
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert!(!is_closed(&h, &tab));

    // L'orchestratore chiude il turno ma il worker lavora ancora: tab aperto.
    finished(&st, &h, &tab);
    tick(&st, &h);
    assert!(!is_closed(&h, &tab));
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true], "ancora sveglio durante il drain");

    // Il worker finisce: l'orchestratore (idle) viene svegliato, non chiuso.
    call(&st, &h, &w, "get_task", json!({})).unwrap();
    call(&st, &h, &w, "complete_task", json!({"status": "completed", "summary": "ok"})).unwrap();
    assert!(wake_in_log(&h, &tab) >= 1, "{:?}", h.log());
    assert!(!is_closed(&h, &tab), "evento azionabile in coda");
    // Integra: get_events, done sul task; poi chiude il turno -> drain.
    let ev = call(&st, &h, &tab, "get_events", json!({})).unwrap();
    assert!(!ev["events"].as_array().unwrap().is_empty());
    call(&st, &h, &tab, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    // b e' ora pronto ma la finestra e' chiusa: non parte.
    assert_eq!(board_task(&st, &h, "b"), "queued");
    finished(&st, &h, &tab);
    assert!(is_closed(&h, &tab), "{:?}", h.log());
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false]);
    pty_down(&h, &tab);
    // Nessuna riapertura finche' la finestra e' chiusa.
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);

    // Finestra successiva: contesto pulito, nuovo orchestratore e b parte.
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 2);
    assert_eq!(board_task(&st, &h, "b"), "running");
}

fn wake_in_log(h: &FakeHost, tab: &str) -> usize {
    writes_to(h, tab).iter().filter(|l| l.contains("New events from your automated sessions")).count()
}

#[test]
fn window_text_waits_for_idle_and_never_lands_in_a_permission_dialog() {
    let (st, h, tab, _w) = running_night();
    // Mai idle finora (nessun hook): niente testo alla chiusura.
    at(&h, "13:00");
    tick(&st, &h);
    assert_eq!(closed_texts(&h, &tab), 0);
    // In attesa di un permesso: niente.
    hook(&st, &h, &tab, "needs-input");
    tick(&st, &h);
    assert_eq!(closed_texts(&h, &tab), 0);
    // Appena idle: consegnato al volo dall'hook, una volta sola.
    finished(&st, &h, &tab);
    assert_eq!(closed_texts(&h, &tab), 1);
    tick(&st, &h);
    finished(&st, &h, &tab);
    assert_eq!(closed_texts(&h, &tab), 1);
}

#[test]
fn drain_closes_when_idle_and_nothing_is_running() {
    let (st, h) = setup("15:00");
    // Solo un task in review: nessun task in corso.
    load_work(&st, &h, vec![t("a")]);
    st.run(&h, |r| {
        let b = r.boards.get_mut("ws-1").unwrap();
        b.start_task("a", 5);
        b.finish_running("a", None, board::TaskStatus::Review, None, None, 6);
    });
    at(&h, "21:00");
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    finished(&st, &h, &tab);
    at(&h, "13:00");
    tick(&st, &h); // testo di chiusura
    assert_eq!(closed_texts(&h, &tab), 1);
    assert!(!is_closed(&h, &tab));
    finished(&st, &h, &tab); // ha finito il turno: drain
    assert!(is_closed(&h, &tab));
    assert_eq!(h.awake.lock().unwrap().last(), Some(&false));
}

#[test]
fn user_orchestrator_is_never_closed_or_typed_into() {
    let (st, h) = setup("15:00");
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    finished(&st, &h, ORCH);
    at(&h, "21:00");
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty());
    at(&h, "13:00");
    tick(&st, &h);
    finished(&st, &h, ORCH);
    tick(&st, &h);
    assert_eq!(closed_texts(&h, ORCH), 0);
    assert!(!h.log().iter().any(|l| l.starts_with("close:orch-1")), "{:?}", h.log());
    // Il gate vale comunque: nessun task nuovo di giorno.
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("late")]})).unwrap();
    assert_eq!(board_task(&st, &h, "late"), "queued");
}

#[test]
fn reopening_during_a_drain_keeps_draining_then_a_fresh_orchestrator_starts() {
    let (st, h, tab, _w) = running_night();
    finished(&st, &h, &tab);
    at(&h, "13:00");
    tick(&st, &h);
    assert_eq!(closed_texts(&h, &tab), 1);
    // La finestra riapre prima che il worker finisca: il tab continua il drain.
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
    assert!(!is_closed(&h, &tab));
}

// ---------------------------------------------------------------------------
// Configurazione, avvio, tab chiusi dall'utente
// ---------------------------------------------------------------------------

#[test]
fn config_change_mid_window_spawns_or_drains() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    at(&h, "22:00");
    st.schedule_set(&h, night(), NOW).unwrap();
    load_work(&st, &h, vec![t("a")]);
    // In pausa nemmeno l'orchestratore su richiesta serve.
    st.run(&h, |r| r.set_board("ws-1", None, Some(true), 2)).unwrap();
    tick(&st, &h);
    assert!(h.spawns().is_empty(), "workspace non automatico");
    // L'utente attiva l'orchestratore automatico a meta' finestra.
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(true));
    st.reconcile_at(&h, NOW, true);
    let tab = orch_tabs(&h).remove(0);
    st.run(&h, |r| r.set_board("ws-1", None, Some(false), 3)).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    pty_up(&h, &tab);
    finished(&st, &h, &tab);
    // Lo spegne: chiusura come a fine finestra.
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(false));
    st.reconcile_at(&h, NOW, true);
    assert_eq!(closed_texts(&h, &tab), 1);
    // Cambio della finestra: ora e' chiusa.
    let (st, h, tab, _w) = running_night();
    finished(&st, &h, &tab);
    st.schedule_set(&h, Schedule { windows: vec![Window { start: "03:00".into(), end: "04:00".into() }], ..night() }, NOW)
        .unwrap();
    assert_eq!(closed_texts(&h, &tab), 1);
    // Schedule spento a meta' finestra: nessun effetto (niente chiusura nuova).
    let (st, h, tab, _w) = running_night();
    st.schedule_set(&h, Schedule { enabled: false, ..night() }, NOW).unwrap();
    assert!(!st.run(&h, |r| r.board_view("ws-1")).automation.unwrap().window_open);
    let _ = tab;
}

#[test]
fn app_start_inside_a_window_opens_once() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(true));
    at(&h, "23:30");
    // Come `init`: schedule caricato, finestra calcolata senza effetti.
    st.run(&h, |r| r.init_schedule(night(), parse_hhmm("23:30").unwrap()));
    load_work(&st, &h, vec![t("a")]);
    assert!(h.spawns().is_empty());
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
    assert_eq!(board_task(&st, &h, "a"), "running");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
    assert_eq!(h.schedule_events.lock().unwrap().len(), 1, "stato emesso una volta");
    assert!(h.schedule_events.lock().unwrap()[0].window_open);
}

#[test]
fn user_closing_the_automatic_tab_is_respected_until_the_next_window() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    at(&h, "21:00");
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    tick(&st, &h); // visto vivo
    pty_down(&h, &tab); // l'utente lo chiude
    tick(&st, &h);
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1, "non si riapre nella stessa finestra");
    at(&h, "13:00");
    tick(&st, &h);
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 2, "alla finestra successiva si riapre");
}

#[test]
fn a_tab_that_never_appeared_is_retried_a_few_times() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    at(&h, "21:00");
    for i in 0..6 {
        st.tick(&h, NOW + (i + 1) * 61_000);
    }
    // Mai vivo: dopo la tolleranza si riprova, ma solo 3 volte per finestra.
    assert_eq!(orch_tabs(&h).len(), 3);
}

#[test]
fn frontend_close_report_clears_the_tab() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    at(&h, "21:00");
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    st.run(&h, |r| r.tab_closed(&tab, NOW));
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1, "chiuso dall'utente: nessuna riapertura nella finestra");
}

// ---------------------------------------------------------------------------
// Schedule, standby, workflow
// ---------------------------------------------------------------------------

#[test]
fn schedule_state_events_and_validation() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    at(&h, "15:00");
    let s = st.schedule_set(&h, night(), NOW).unwrap();
    assert!(s.schedule.enabled && !s.window_open);
    assert_eq!(s.next_change_at, Some(NOW - NOW % 60_000 + 300 * 60_000));
    assert_eq!(h.saved_schedule.lock().unwrap().as_ref(), Some(&night()));
    assert_eq!(h.schedule_events.lock().unwrap().len(), 1);
    // Senza cambiamenti il timer non riemette.
    tick(&st, &h);
    assert_eq!(h.schedule_events.lock().unwrap().len(), 1);
    // Transizione: riemette con window_open.
    at(&h, "20:00");
    tick(&st, &h);
    let ev = h.schedule_events.lock().unwrap().clone();
    assert_eq!(ev.len(), 2);
    assert!(ev[1].window_open);
    assert_eq!(st.schedule_state(&h, NOW), ev[1]);
    // Non valido: niente salvataggio ne' effetti.
    let bad = Schedule { windows: vec![Window { start: "10:00".into(), end: "10:00".into() }], ..night() };
    assert!(st.schedule_set(&h, bad, NOW).is_err());
    assert_eq!(h.saved_schedule.lock().unwrap().as_ref(), Some(&night()));
    assert_eq!(h.schedule_events.lock().unwrap().len(), 2);
}

#[test]
fn keep_awake_follows_the_work_and_the_flag() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    assert!(h.awake.lock().unwrap().is_empty(), "finestra chiusa: niente blocco");
    at(&h, "21:00");
    tick(&st, &h);
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);
    // keep_awake spento: rilasciato.
    st.schedule_set(&h, Schedule { keep_awake: false, ..night() }, NOW).unwrap();
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false]);
}

#[test]
fn workflow_tools_roundtrip_per_workspace() {
    let (st, h) = (AutomationState::default(), FakeHost::default());
    assert_eq!(call(&st, &h, ORCH, "get_workflow", json!({})).unwrap(), json!({"workflow": null}));
    let r = call(&st, &h, ORCH, "save_workflow", json!({"content": "  # Identity\nYou are X.  "})).unwrap();
    assert_eq!(r["saved"], true);
    assert_eq!(
        call(&st, &h, "orch-b", "get_workflow", json!({})).unwrap()["workflow"],
        "# Identity\nYou are X."
    );
    // Altro workspace: il suo workflow e' separato.
    assert_eq!(call(&st, &h, "orch-wsl", "get_workflow", json!({})).unwrap()["workflow"], Value::Null);
    // Errori in inglese.
    assert_eq!(call(&st, &h, ORCH, "save_workflow", json!({})).unwrap_err(), "content is missing");
    assert_eq!(call(&st, &h, ORCH, "save_workflow", json!({"content": "   "})).unwrap_err(), "content is missing");
    let too_long = call(&st, &h, ORCH, "save_workflow", json!({"content": "x".repeat(50_001)})).unwrap_err();
    assert!(too_long.contains("too long") && too_long.contains("50000"), "{too_long}");
    assert!(call(&st, &h, ORCH, "save_workflow", json!({"content": "x".repeat(50_000)})).is_ok());
    // Tab senza workspace.
    assert!(call(&st, &h, "tab-sconosciuto", "get_workflow", json!({})).is_err());
    // Solo orchestratori: un worker no.
    let w = {
        call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
        spawns_of(&h, Role::Worker)[0].tab_id.clone()
    };
    assert_eq!(
        call(&st, &h, &w, "get_workflow", json!({})).unwrap_err(),
        "get_workflow is reserved for the orchestrator"
    );
    // Via Tauri: vuoto o None elimina; workspace inesistente = errore.
    st.workflow_save(&h, "ws-1", Some("   ")).unwrap();
    assert_eq!(h.workflow_read("ws-1"), Ok(None));
    st.workflow_save(&h, "ws-1", Some("# W")).unwrap();
    assert_eq!(h.workflow_read("ws-1"), Ok(Some("# W".into())));
    assert!(st.workflow_save(&h, "ws-1", Some(&"x".repeat(50_001))).unwrap_err().contains("troppo lungo"));
    assert_eq!(st.workflow_save(&h, "ws-9", Some("x")).unwrap_err(), "workspace non trovato");
    // Rimosso con il workspace.
    st.remove_board(&h, "ws-1");
    assert_eq!(h.workflow_read("ws-1"), Ok(None));
}

#[test]
fn model_facing_texts_are_english_and_prompt_is_shell_safe() {
    let p = ORCHESTRATOR_BOOTSTRAP_PROMPT;
    assert!(p.is_ascii() && !p.is_empty());
    assert!(p.chars().all(|c| c.is_ascii_alphanumeric() || " .,:;_()'-".contains(c)), "{p}");
    for needle in ["get_workflow", "get_board", "orchestrate"] {
        assert!(p.contains(needle), "{needle}");
    }
    assert!(WINDOW_CLOSED_TEXT.is_ascii());
    assert!(WINDOW_CLOSED_TEXT.contains("get_events") && WINDOW_CLOSED_TEXT.contains("end your turn"));
    // L'agente accetta il prompt come comando di avvio.
    let req = AutomationRequest { bootstrap_prompt: p, permission_mode: PermissionMode::Bypass, model: Some("opus"), settings_path: None };
    assert!(crate::agents::automation_command("claude-code", "windows", &req).is_ok());
}

// ---------------------------------------------------------------------------
// Robustezza: riavvio, gate, drain massimo, crash dell'orchestratore
// ---------------------------------------------------------------------------

#[test]
fn first_tick_after_startup_schedules_boards_loaded_from_disk() {
    // Workspace normale, nessuno schedule: dopo un riavvio la board caricata
    // dal disco ha task pronti che nessuno ha segnato come modificati.
    let (st, h) = (AutomationState::default(), FakeHost::default());
    load_work(&st, &h, vec![t("a")]);
    assert_eq!(board_task(&st, &h, "a"), "queued");
    tick(&st, &h);
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert_eq!(spawns_of(&h, Role::Worker).len(), 1);
}

#[test]
fn scheduler_gate_uses_the_window_of_the_moment_not_the_last_tick() {
    let (st, h) = setup("21:00"); // finestra aperta all'ultimo giro
    load_work(&st, &h, vec![t("a")]);
    // La finestra si chiude fra due giri del timer; una modifica della board
    // schedula adesso: il task non deve partire.
    at(&h, "13:30");
    boards::board_set_cmd(&st, &h, "ws-1", Some(3), None).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "queued");
    assert!(spawns_of(&h, Role::Worker).is_empty());
    // Riaperta: riparte.
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(board_task(&st, &h, "a"), "running");
}

#[test]
fn drain_has_a_deadline_after_which_the_tab_is_closed_and_awake_released() {
    use auto::{drain_expired, DRAIN_MAX_MS};
    assert!(!drain_expired(None, 10 * DRAIN_MAX_MS));
    assert!(!drain_expired(Some(0), DRAIN_MAX_MS));
    assert!(drain_expired(Some(0), DRAIN_MAX_MS + 1));

    let (st, h, tab, _w) = running_night();
    finished(&st, &h, &tab);
    at(&h, "13:00");
    tick(&st, &h); // inizia il drain: il task "a" gira ancora
    assert!(!is_closed(&h, &tab));
    st.tick(&h, NOW + DRAIN_MAX_MS - 1_000);
    assert!(!is_closed(&h, &tab));
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true]);
    st.tick(&h, NOW + DRAIN_MAX_MS + 1_000);
    assert!(is_closed(&h, &tab), "{:?}", h.log());
    assert_eq!(h.awake.lock().unwrap().as_slice(), [true, false], "standby rilasciato");
    assert_eq!(board_task(&st, &h, "a"), "running", "il task non si interrompe");
    // Alla finestra successiva riparte tutto normalmente.
    pty_down(&h, &tab);
    at(&h, "20:00");
    st.tick(&h, NOW + DRAIN_MAX_MS + 2_000);
    assert_eq!(orch_tabs(&h).len(), 2);
}

#[test]
fn an_exited_automatic_orchestrator_is_reopened_up_to_three_times_per_window() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    at(&h, "21:00");
    tick(&st, &h);
    let up = |h: &FakeHost, n: usize| {
        let tab = orch_tabs(h)[n - 1].clone();
        pty_up(h, &tab);
        tab
    };
    // 1: il pty esce da solo (il frontend lo riporta dopo).
    let tab1 = up(&h, 1);
    tick(&st, &h);
    st.run(&h, |r| r.auto_tab_exited(&tab1, NOW));
    pty_down(&h, &tab1);
    st.run(&h, |r| r.tab_closed(&tab1, NOW));
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 2, "non e' una chiusura dell'utente");
    // 2: l'agente esce ma la shell resta (hook session-end): il tab si chiude.
    let tab2 = up(&h, 2);
    tick(&st, &h);
    hook(&st, &h, &tab2, "session-end");
    assert!(is_closed(&h, &tab2));
    pty_down(&h, &tab2);
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 3);
    // 3: terzo crash, nessun altro tentativo in questa finestra.
    let tab3 = up(&h, 3);
    tick(&st, &h);
    st.run(&h, |r| r.auto_tab_exited(&tab3, NOW));
    pty_down(&h, &tab3);
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 3);
    // Finestra successiva: contatore azzerato.
    at(&h, "13:00");
    tick(&st, &h);
    at(&h, "20:00");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 4);
}

#[test]
fn a_user_close_still_dismisses_while_an_exit_during_drain_is_harmless() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    at(&h, "21:00");
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    tick(&st, &h);
    // Chiusura dell'utente: nessun `pty_exited`, solo il report del frontend.
    pty_down(&h, &tab);
    st.run(&h, |r| r.tab_closed(&tab, NOW));
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
    // Un'uscita in una finestra chiusa (drain) non consuma tentativi ne' riapre.
    at(&h, "13:00");
    tick(&st, &h);
    st.run(&h, |r| r.auto_tab_exited("auto-sconosciuto", NOW));
    assert_eq!(orch_tabs(&h).len(), 1);
}

// ---------------------------------------------------------------------------
// Orchestratore manuale: al massimo uno per workspace
// ---------------------------------------------------------------------------

fn open_manual(st: &AutomationState, h: &FakeHost, ws: &str, tab: &str, now: i64) -> Result<(), String> {
    st.orchestrator_open(h, ws, tab, now)
}

#[test]
fn a_manual_orchestrator_blocks_the_automatic_spawn() {
    let (st, h) = setup("20:00");
    load_work(&st, &h, vec![t("a")]);
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    pty_up(&h, "man-1");
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty());
    // Anche molto dopo, finche' il suo pty e' vivo.
    st.tick(&h, NOW + 10 * GRACE_MS);
    assert!(orch_tabs(&h).is_empty());
}

#[test]
fn a_second_orchestrator_in_the_same_workspace_fails_but_another_workspace_is_free() {
    let (st, h) = setup("20:00");
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    pty_up(&h, "man-1");
    let err = open_manual(&st, &h, "ws-1", "man-2", NOW + 1).unwrap_err();
    assert!(err.contains("gia' attivo"));
    // Lo stesso tab puo' registrarsi di nuovo (ripristino).
    open_manual(&st, &h, "ws-1", "man-1", NOW + 2).unwrap();
    open_manual(&st, &h, "ws-2", "man-3", NOW + 3).unwrap();
}

#[test]
fn registration_fails_while_the_automatic_orchestrator_is_open() {
    let (st, h) = setup("20:00");
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    let auto = orch_tabs(&h);
    assert_eq!(auto.len(), 1);
    pty_up(&h, &auto[0]);
    assert!(open_manual(&st, &h, "ws-1", "man-1", NOW + 1).is_err());
    // Chiuso l'automatico (tab sparito) il posto si libera.
    pty_down(&h, &auto[0]);
    st.run(&h, |r| r.tab_closed(&auto[0], NOW + 2));
    assert!(open_manual(&st, &h, "ws-1", "man-1", NOW + 3).is_ok());
}

#[test]
fn closing_or_exiting_the_manual_orchestrator_frees_the_workspace() {
    let (st, h) = setup("20:00");
    // `b` resta in coda (dipende da `a`): c'e' ancora lavoro dopo l'avvio di `a`.
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    pty_up(&h, "man-1");
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty());
    // Chiusura dal frontend.
    pty_down(&h, "man-1");
    st.run(&h, |r| r.tab_closed("man-1", NOW + 1));
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);

    // Uscita del pty: la registrazione sparisce.
    let (st, h) = setup("20:00");
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    pty_up(&h, "man-1");
    pty_down(&h, "man-1");
    st.run(&h, |r| r.auto_tab_exited("man-1", NOW + 1));
    assert!(open_manual(&st, &h, "ws-1", "man-2", NOW + 2).is_ok());
}

#[test]
fn a_registration_whose_tab_never_appeared_expires() {
    let (st, h) = setup("20:00");
    load_work(&st, &h, vec![t("a"), dep("b", "a")]);
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    st.tick(&h, NOW + GRACE_MS + 1);
    assert_eq!(orch_tabs(&h).len(), 1);
}

// ---------------------------------------------------------------------------
// Orchestratore su richiesta e gate dello scheduler
// ---------------------------------------------------------------------------

/// Workspace normale (non automatico), nessuno schedule.
fn plain() -> (AutomationState, FakeHost) {
    (AutomationState::default(), FakeHost::default())
}

fn orch_state(st: &AutomationState) -> Option<String> {
    st.inner.lock().unwrap().board_view("ws-1").orchestrator
}

fn go_review(st: &AutomationState, h: &FakeHost, id: &str) {
    st.run(h, |r| {
        let b = r.boards.get_mut("ws-1").unwrap();
        b.start_task(id, 5);
        b.finish_running(id, None, board::TaskStatus::Review, None, None, 6);
    });
}

fn finish_worker(st: &AutomationState, h: &FakeHost, session: &str) {
    call(st, h, session, "get_task", json!({})).unwrap();
    call(st, h, session, "complete_task", json!({"status": "completed", "summary": "fatto"})).unwrap();
}

#[test]
fn the_scheduler_starts_nothing_without_an_orchestrator_until_one_is_alive() {
    let (st, h) = plain();
    load_work(&st, &h, vec![t("a")]);
    // La board riparte (modifica dell'utente), ma nessun orchestratore: niente avvio.
    st.run(&h, |r| r.set_board("ws-1", Some(3), None, 2)).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "queued");
    assert!(spawns_of(&h, Role::Worker).is_empty());
    // Un orchestratore che chiama un tool di board e' la prova di vita: il task parte subito.
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert_eq!(orch_state(&st).as_deref(), Some("live"));
}

#[test]
fn a_manual_orchestrator_opens_the_gate_and_its_exit_closes_it_for_new_tasks() {
    let (st, h) = plain();
    load_work(&st, &h, vec![t("a"), t("b")]);
    st.run(&h, |r| r.set_board("ws-1", Some(1), None, 2)).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "queued");
    open_manual(&st, &h, "ws-1", "man-1", NOW).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running", "false->true: la board riparte subito");
    pty_up(&h, "man-1");
    st.run(&h, |r| r.auto_tab_exited("man-1", NOW + 1));
    // Un posto si libera ma nessun orchestratore: `b` non parte.
    st.run(&h, |r| {
        let b = r.boards.get_mut("ws-1").unwrap();
        b.finish_running("a", None, board::TaskStatus::Failed, None, None, 9);
        r.mark_dirty("ws-1");
    });
    assert_eq!(board_task(&st, &h, "b"), "queued");
}

#[test]
fn on_demand_orchestrator_spawns_for_a_plain_workspace_and_starts_the_tasks() {
    let (st, h) = plain();
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    let tabs = spawns_of(&h, Role::Orchestrator);
    assert_eq!(tabs.len(), 1);
    let o = &tabs[0];
    assert_eq!(o.title, "Orchestratore");
    assert_eq!(o.agent_id, "claude-code");
    assert!(o.command.contains(crate::automation::auto::ON_DEMAND_BOOTSTRAP_PROMPT) && !o.command.contains(ORCHESTRATOR_BOOTSTRAP_PROMPT));
    // Il tab in attesa vale come orchestratore disponibile.
    assert_eq!(board_task(&st, &h, "a"), "running");
    assert_eq!(orch_state(&st).as_deref(), Some("starting"));
    pty_up(&h, &o.tab_id);
    tick(&st, &h);
    assert_eq!(orch_state(&st).as_deref(), Some("live"));
    // Non ne apre un secondo.
    tick(&st, &h);
    assert_eq!(spawns_of(&h, Role::Orchestrator).len(), 1);
}

#[test]
fn on_demand_uses_the_configured_agent_and_model_even_when_disabled() {
    let (st, h) = plain();
    h.auto.lock().unwrap().insert("ws-1".into(), auto_cfg(false));
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    let tabs = spawns_of(&h, Role::Orchestrator);
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].command.contains("model=opus"), "{}", tabs[0].command);
}

#[test]
fn on_demand_does_not_spawn_for_a_paused_board_with_only_queued_tasks_but_does_for_review() {
    let (st, h) = plain();
    load_work(&st, &h, vec![t("a"), t("b")]);
    st.run(&h, |r| r.set_board("ws-1", None, Some(true), 2)).unwrap();
    tick(&st, &h);
    assert!(h.spawns().is_empty());
    assert_eq!(orch_state(&st), None);
    // Un task in review da integrare serve anche con la board in pausa.
    go_review(&st, &h, "a");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
    // Task trattenuti o con dipendenze non fatte non servono.
    let (st, h) = plain();
    load_work(&st, &h, vec![json!({"id": "a", "task": "x", "held": true}), dep("b", "a")]);
    tick(&st, &h);
    assert!(h.spawns().is_empty());
}

#[test]
fn on_demand_spawns_for_actionable_events_of_a_dead_owner() {
    let (st, h) = plain();
    // Task in corso con owner morto: nulla da fare finche' non arriva un evento azionabile.
    load_work(&st, &h, vec![t("a")]);
    pty_up(&h, "orch-x");
    call(&st, &h, "orch-x", "get_board", json!({})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "running");
    pty_down(&h, "orch-x");
    st.run(&h, |r| r.auto_tab_exited("orch-x", NOW));
    tick(&st, &h);
    assert!(orch_tabs(&h).is_empty());
    let w = spawns_of(&h, Role::Worker).remove(0).tab_id;
    st.run(&h, |r| r.push_event(&w, EventKind::HelpRequest, Some("?".into()), 7));
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 1);
}

#[test]
fn a_dismissed_on_demand_tab_is_not_reopened_until_the_user_asks() {
    let (st, h) = plain();
    // `b` resta in coda (parallelismo 1): la board continua ad avere bisogno di un orchestratore.
    load_work(&st, &h, vec![t("a"), t("b")]);
    st.run(&h, |r| r.set_board("ws-1", Some(1), None, 2)).unwrap();
    assert!(h.spawns().is_empty());
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    tick(&st, &h);
    // L'utente lo chiude.
    pty_down(&h, &tab);
    st.run(&h, |r| r.tab_closed(&tab, NOW + 1));
    for i in 0..3 {
        st.tick(&h, NOW + 2 + i);
    }
    assert_eq!(orch_tabs(&h).len(), 1, "niente riapertura");
    assert_eq!(orch_state(&st).as_deref(), Some("stopped"));
    // Lo chiede l'utente.
    let view = st.start_board_orchestrator(&h, "ws-1", NOW + 20).unwrap();
    assert_eq!(orch_tabs(&h).len(), 2);
    assert_eq!(view.orchestrator.as_deref(), Some("starting"));
    assert!(st.start_board_orchestrator(&h, "ws-x", NOW).is_err());
}

#[test]
fn on_demand_gives_up_after_the_spawn_failures_and_the_command_resets_them() {
    let (st, h) = plain();
    // `b` resta in coda (parallelismo 1): la board continua ad avere bisogno di un orchestratore.
    load_work(&st, &h, vec![t("a"), t("b")]);
    st.run(&h, |r| r.set_board("ws-1", Some(1), None, 2)).unwrap();
    assert!(h.spawns().is_empty());
    // Il tab non compare mai: un tentativo per giro oltre il tempo concesso.
    for i in 0..6 {
        st.tick(&h, NOW + i * (GRACE_MS + 1));
    }
    assert_eq!(orch_tabs(&h).len(), 3, "MAX_SPAWN_FAILS");
    assert_eq!(orch_state(&st).as_deref(), Some("stopped"));
    st.start_board_orchestrator(&h, "ws-1", NOW + 20 * GRACE_MS).unwrap();
    assert_eq!(orch_tabs(&h).len(), 4);
}

#[test]
fn a_crashed_on_demand_orchestrator_is_replaced() {
    let (st, h) = plain();
    // `b` resta in coda (parallelismo 1): la board continua ad avere bisogno di un orchestratore.
    load_work(&st, &h, vec![t("a"), t("b")]);
    st.run(&h, |r| r.set_board("ws-1", Some(1), None, 2)).unwrap();
    assert!(h.spawns().is_empty());
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    tick(&st, &h);
    pty_down(&h, &tab);
    st.run(&h, |r| r.auto_tab_exited(&tab, NOW + 1));
    assert_eq!(orch_state(&st), None, "non piu' vivo fino al giro");
    tick(&st, &h);
    assert_eq!(orch_tabs(&h).len(), 2, "crash: riaperto");
}

#[test]
fn the_on_demand_tab_closes_when_idle_and_the_board_needs_nothing() {
    let (st, h) = plain();
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    let tab = orch_tabs(&h).remove(0);
    pty_up(&h, &tab);
    // L'orchestratore legge la board (diventa owner) e finisce il turno: il worker gira.
    call(&st, &h, &tab, "get_board", json!({})).unwrap();
    let w = spawns_of(&h, Role::Worker).remove(0).tab_id;
    finished(&st, &h, &tab);
    assert!(!is_closed(&h, &tab), "task in corso");
    finish_worker(&st, &h, &w);
    // In review il lavoro serve ancora.
    finished(&st, &h, &tab);
    assert!(!is_closed(&h, &tab));
    // Integrato, ma eventi azionabili ancora in coda: prima si consegnano.
    call(&st, &h, &tab, "set_task_status", json!({"id": "a", "status": "done"})).unwrap();
    finished(&st, &h, &tab);
    assert!(!is_closed(&h, &tab));
    call(&st, &h, &tab, "get_events", json!({})).unwrap();
    // Idle, niente in corso/da consegnare e nessun bisogno: chiude (senza testo di finestra).
    finished(&st, &h, &tab);
    assert!(is_closed(&h, &tab), "{:?}", h.log());
    assert_eq!(closed_texts(&h, &tab), 0);
}

#[test]
fn a_user_orchestrator_is_never_closed_by_the_on_demand_logic() {
    let (st, h) = plain();
    call(&st, &h, ORCH, "submit_plan", json!({"tasks": [t("a")]})).unwrap();
    finished(&st, &h, ORCH);
    tick(&st, &h);
    assert!(h.log().iter().all(|l| !l.starts_with("close:")));
    assert!(orch_tabs(&h).is_empty());
}

#[test]
fn an_automatic_workspace_outside_the_window_still_starts_and_spawns_nothing() {
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a")]);
    tick(&st, &h);
    assert!(h.spawns().is_empty());
    assert_eq!(orch_state(&st).as_deref(), Some("window"));
    // Anche con un orchestratore vivo il gate della finestra tiene i task.
    call(&st, &h, ORCH, "get_board", json!({})).unwrap();
    assert_eq!(board_task(&st, &h, "a"), "queued");
    // Dentro la finestra: l'orchestratore automatico (non quello su richiesta).
    let (st, h) = setup("15:00");
    load_work(&st, &h, vec![t("a")]);
    at(&h, "21:00");
    tick(&st, &h);
    let tabs = spawns_of(&h, Role::Orchestrator);
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].title, "Orchestratore automatico");
    assert!(tabs[0].command.contains(ORCHESTRATOR_BOOTSTRAP_PROMPT));
}
