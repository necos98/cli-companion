//! Esecuzione automatica nelle finestre orarie (contratto:
//! `docs/SCHEDULE.MD`): integrazione del calendario (`crate::schedule`) con
//! il `Registry`. Stesso schema del resto dell'automazione: decisioni PURE
//! nel registro (tempo, ora locale e stato dei tab iniettati dal chiamante),
//! gli effetti (spawn del tab, testo digitato, chiusura, standby, evento)
//! sono `Effect` applicati fuori dal lock.
//!
//! Ogni giro (`auto_tick`: timer ogni 30 s, subito a ogni cambio di
//! configurazione) riallinea lo stato: e' a livello, non a fronti, quindi
//! copre anche l'avvio dell'app dentro una finestra e le modifiche a meta'
//! finestra. Per ogni workspace automatico:
//! - apertura: lo scheduler della board riparte (gate); se c'e' lavoro e
//!   nessun owner con tab vivo si apre l'orchestratore automatico;
//! - chiusura: nessun task nuovo; all'orchestratore automatico si digita
//!   `WINDOW_CLOSED_TEXT` quando e' idle; poi drain: idle, nessun task in
//!   esecuzione e nessun evento azionabile in coda -> il tab si chiude.
//!
//! Un orchestratore aperto dall'utente non viene mai toccato.

use super::*;
use std::collections::HashSet;
use crate::schedule::{self, AutomationInfo, AwakeSignal, Schedule, ScheduleState};
use crate::board::TaskStatus;
use crate::workspaces::AutoOrchestrate;

/// Evento Tauri con lo stato dello schedule.
const SCHEDULE_EVENT: &str = "schedule-state";

/// Giro del timer e attesa iniziale (il frontend deve aver registrato i
/// listener prima del primo `automation-spawn`).
pub const TICK_EVERY: Duration = Duration::from_secs(30);
const STARTUP_DELAY: Duration = Duration::from_secs(10);
/// Tempo concesso al frontend per aprire il tab prima di darlo per perso.
const SPAWN_GRACE_MS: i64 = 60_000;
/// Tentativi di apertura per finestra (il tab non e' mai comparso).
const MAX_SPAWN_FAILS: u8 = 3;

/// Tempo massimo di drain: scaduto, il tab dell'orchestratore automatico si
/// chiude comunque (un orchestratore bloccato o un task che non finisce non
/// tengono il PC acceso per sempre).
pub const DRAIN_MAX_MS: i64 = 30 * 60_000;

/// Il drain iniziato a `since` ha superato il tempo massimo?
pub(super) fn drain_expired(since: Option<i64>, now: i64) -> bool {
    since.is_some_and(|s| now - s > DRAIN_MAX_MS)
}

/// Titolo del tab dell'orchestratore automatico (lo vede l'utente).
const ORCHESTRATOR_TITLE: &str = "Orchestratore automatico";

/// Prompt di avvio dell'orchestratore automatico: ASCII sicuro per la riga
/// di comando (niente virgolette, `$`, backtick, `%`, `!`, `\`). Lo legge il
/// modello: inglese.
pub const ORCHESTRATOR_BOOTSTRAP_PROMPT: &str = "You are the automatic orchestrator of this workspace for the current execution window. Load the cli-companion-notify:orchestrate skill if your agent supports skills, call get_workflow and get_board, then carry the plan forward: integrate the tasks in review, handle failures and help requests, and add work only through the board. When nothing is left to do right now, end your turn.";

/// Digitato nel prompt dell'orchestratore automatico alla chiusura della
/// finestra (quando e' idle).
pub const WINDOW_CLOSED_TEXT: &str = "[cli-companion] The execution window is closed: no new tasks will start. Call get_events, integrate the tasks that complete (merge, test, set_task_status), handle failures, do not plan new work, then end your turn.";

// ---------------------------------------------------------------------------
// Stato
// ---------------------------------------------------------------------------

/// Tab dell'orchestratore automatico di un workspace.
struct AutoTab {
    id: String,
    spawned_at: i64,
    /// Il pty del tab e' stato visto vivo almeno una volta.
    seen_live: bool,
    /// La finestra e' chiusa (o l'automatismo spento): drain in corso.
    /// Resta tale anche se la finestra riapre prima della chiusura del tab.
    closing: bool,
    /// `WINDOW_CLOSED_TEXT` gia' digitato.
    text_sent: bool,
    /// Quando e' iniziato il drain (vedi `DRAIN_MAX_MS`).
    closing_since: Option<i64>,
}

/// Orchestratore avviato a mano (avvio integrato) registrato dal frontend.
struct ManualOrch {
    ws: String,
    registered_at: i64,
}

#[derive(Default)]
struct AutoWs {
    /// Ultimo gate applicato (finestra aperta per questo workspace).
    gate: bool,
    /// L'utente ha chiuso il tab durante la finestra: non si riapre finche'
    /// la finestra non si richiude e riapre.
    dismissed: bool,
    /// Aperture fallite (tab mai comparso) o orchestratori usciti/crashati
    /// in questa finestra: al terzo non si riapre piu'.
    fails: u8,
    /// Drain scaduto: tab chiuso d'ufficio, i task ancora in corso non
    /// tengono piu' acceso il PC fino alla finestra successiva.
    drain_forced: bool,
    tab: Option<AutoTab>,
}

#[derive(Default)]
pub(super) struct AutoCore {
    pub(super) sched: Schedule,
    pub(super) window_open: bool,
    /// Ultimo stato emesso (`schedule-state`).
    last_state: Option<ScheduleState>,
    ws: HashMap<String, AutoWs>,
    /// Orchestratori manuali aperti (tab -> workspace): al massimo uno vivo
    /// per workspace, insieme a quello automatico.
    manual: HashMap<String, ManualOrch>,
    /// `automation` esposto per ogni workspace automatico.
    pub(super) info: HashMap<String, AutomationInfo>,
    /// Workspace con l'orchestratore automatico attivo (ultimo giro).
    enabled: HashSet<String>,
    awake: bool,
    /// Numero d'ordine degli effetti `KeepAwake`.
    awake_seq: u64,
    /// Primo giro dopo l'avvio gia' fatto.
    started: bool,
}

/// Cio' che serve a un giro, fornito da chi chiama (il nucleo non conosce
/// ne' orologio, ne' workspace, ne' pty).
pub struct TickEnv<'a> {
    /// Minuti locali del giorno.
    pub minute: u16,
    pub workspaces: &'a [Workspace],
    /// Il tab ha un pty vivo?
    pub live: &'a dyn Fn(&str) -> bool,
    /// Comando di avvio dell'orchestratore automatico.
    pub command: &'a dyn Fn(&Workspace, &AutoOrchestrate) -> Result<String, String>,
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

impl Registry {
    /// Carica lo schedule e calcola la finestra senza effetti (avvio).
    pub fn init_schedule(&mut self, s: Schedule, minute: u16) {
        self.auto.window_open = schedule::window_open(&s, minute);
        self.auto.sched = s;
    }

    /// Nuovo schedule (gia' validato e salvato); il giro che segue applica.
    pub fn set_schedule(&mut self, s: Schedule) {
        self.auto.sched = s;
    }

    pub fn schedule_state(&self, minute: u16, now: i64) -> ScheduleState {
        schedule::state(&self.auto.sched, minute, now)
    }

    /// C'e' lavoro per un orchestratore: task in coda non trattenuti o task
    /// in review da integrare.
    fn has_work(&self, ws: &str) -> bool {
        self.boards.get(ws).is_some_and(|b| {
            b.tasks.iter().any(|t| match t.status {
                TaskStatus::Queued => !t.held,
                TaskStatus::Review => true,
                _ => false,
            })
        })
    }

    /// Il tab manuale e' ancora vivo (pty presente o appena registrato:
    /// il pty compare solo quando il terminale e' montato).
    fn manual_alive(m: &ManualOrch, tab: &str, env: &TickEnv, now: i64) -> bool {
        (env.live)(tab) || now - m.registered_at <= SPAWN_GRACE_MS
    }

    /// C'e' un orchestratore manuale vivo in questo workspace?
    fn manual_orchestrator_alive(&self, ws: &str, env: &TickEnv, now: i64) -> bool {
        self.auto.manual.iter().any(|(tab, m)| m.ws == ws && Self::manual_alive(m, tab, env, now))
    }

    /// Registra un orchestratore manuale. Errore se il workspace ne ha gia'
    /// uno vivo (l'automatico o un altro manuale). I tab spariti si scartano.
    pub fn manual_orchestrator_open(&mut self, ws: &str, tab: &str, env: &TickEnv, now: i64) -> Result<(), String> {
        self.auto.manual.retain(|t, m| Self::manual_alive(m, t, env, now));
        let auto_alive = self
            .auto
            .ws
            .get(ws)
            .and_then(|e| e.tab.as_ref())
            .is_some_and(|t| (env.live)(&t.id) || now - t.spawned_at <= SPAWN_GRACE_MS);
        let other = self.auto.manual.iter().any(|(t, m)| m.ws == ws && t != tab);
        if auto_alive || other {
            return Err("Un orchestratore e' gia' attivo in questo workspace".to_string());
        }
        self.auto.manual.insert(tab.to_string(), ManualOrch { ws: ws.to_string(), registered_at: now });
        Ok(())
    }

    fn owner_alive(&self, ws: &str, env: &TickEnv) -> bool {
        self.boards
            .get(ws)
            .and_then(|b| b.owner.as_deref())
            .filter(|o| !o.is_empty())
            .is_some_and(|o| (env.live)(o))
    }

    /// Il tab puo' chiudersi: nessun task in esecuzione (o in avvio) e
    /// nessun evento azionabile ancora da consegnare.
    fn drainable(&self, ws: &str, tab: &str) -> bool {
        let running = self.boards.get(ws).is_some_and(|b| b.running_count() > 0);
        let pending = self.queues.get(tab).is_some_and(|q| q.events.iter().any(|e| e.kind.is_actionable()));
        !running && !pending
    }

    /// Passi dell'orchestratore automatico quando e' idle al prompt (hook
    /// `finished`): prima il testo di chiusura, poi il drain.
    fn auto_idle(&mut self, ws: &str, e: &mut AutoWs) {
        let Some(t) = e.tab.as_mut() else {
            return;
        };
        let idle = self.last_hook.get(&t.id).map(String::as_str) == Some("finished");
        if !t.closing || !idle {
            return;
        }
        if !t.text_sent {
            t.text_sent = true;
            self.effects.push(Effect::Type { tab: t.id.clone(), text: WINDOW_CLOSED_TEXT });
            // Come il risveglio: ora lavora, niente altro testo fino al
            // prossimo `finished`.
            self.last_hook.insert(t.id.clone(), "woken".to_string());
            return;
        }
        if self.drainable(ws, &t.id) {
            self.effects.push(Effect::Close { tab_id: t.id.clone(), delay: Duration::ZERO });
            e.tab = None;
        }
    }

    /// Hook di un tab che non e' un worker: se e' l'orchestratore automatico
    /// di un workspace, valuta testo di chiusura e drain.
    pub(super) fn auto_hook(&mut self, tab: &str, event: &str) {
        let Some(ws) = self
            .auto
            .ws
            .iter()
            .find(|(_, e)| e.tab.as_ref().is_some_and(|t| t.id == tab))
            .map(|(k, _)| k.clone())
        else {
            return;
        };
        if let Some(mut e) = self.auto.ws.remove(&ws) {
            if event == "session-end" {
                // L'agente e' uscito (o crashato) ma la shell del tab e' viva:
                // chiusura del tab e, se non era un drain, un tentativo in
                // piu' consumato (non e' un'azione dell'utente).
                self.last_hook.remove(tab);
                if e.tab.as_ref().is_some_and(|t| !t.closing) {
                    e.fails = e.fails.saturating_add(1);
                }
                e.tab = None;
                self.effects.push(Effect::Close { tab_id: tab.to_string(), delay: Duration::ZERO });
                self.auto.ws.insert(ws, e);
                self.update_awake();
                return;
            }
            self.auto_idle(&ws, &mut e);
            self.auto.ws.insert(ws, e);
            self.update_awake();
        }
    }

    /// Il pty del tab e' uscito da solo (nessuno lo ha chiuso): se era
    /// l'orchestratore automatico e non stava chiudendo, e' un'uscita o un
    /// crash, non una scelta dell'utente: niente `dismissed`, un tentativo
    /// consumato (max `MAX_SPAWN_FAILS` per finestra) e il giro successivo
    /// lo riapre se c'e' ancora lavoro.
    pub(super) fn auto_tab_exited(&mut self, tab: &str, _now: i64) {
        self.last_hook.remove(tab);
        self.auto.manual.remove(tab);
        for e in self.auto.ws.values_mut() {
            if e.tab.as_ref().is_some_and(|t| t.id == tab) {
                if e.tab.as_ref().is_some_and(|t| !t.closing) {
                    e.fails = e.fails.saturating_add(1);
                }
                e.tab = None;
            }
        }
        self.update_awake();
    }

    /// Il frontend ha chiuso un tab: se era un orchestratore automatico lo
    /// dimentica (e, se non stava chiudendo, e' stato l'utente: l'uscita
    /// spontanea arriva prima via `auto_tab_exited` e qui non c'e' piu').
    pub(super) fn auto_tab_closed(&mut self, tab: &str) {
        self.last_hook.remove(tab);
        self.auto.manual.remove(tab);
        for e in self.auto.ws.values_mut() {
            if e.tab.as_ref().is_some_and(|t| t.id == tab) {
                if e.tab.as_ref().is_some_and(|t| !t.closing) {
                    e.dismissed = true;
                }
                e.tab = None;
            }
        }
    }

    /// Il workspace non esiste piu'.
    pub fn auto_forget(&mut self, ws: &str) {
        self.auto.ws.remove(ws);
        self.auto.manual.retain(|_, m| m.ws != ws);
        if self.auto.info.remove(ws).is_some() {
            self.mark_dirty(ws);
        }
    }

    /// Un giro: aggiorna la finestra, emette `schedule-state` se cambia e
    /// riallinea ogni workspace. `force_emit`: emette comunque (modifica di
    /// configurazione).
    pub fn auto_tick(&mut self, env: &TickEnv, now: i64, force_emit: bool) {
        // Primo giro dopo l'avvio: tutte le board da schedulare (quelle non
        // automatiche con task pronti ripartono dopo un riavvio).
        if !self.auto.started {
            self.auto.started = true;
            let ids: Vec<String> = self.boards.keys().cloned().collect();
            for id in ids {
                self.mark_dirty(&id);
            }
        }
        // Task bloccati in avvio.
        self.watchdog(now);
        let sc = schedule::state(&self.auto.sched, env.minute, now);
        self.auto.window_open = sc.window_open;
        let info = schedule::automation_info(&self.auto.sched, env.minute, now);
        if force_emit || self.auto.last_state.as_ref() != Some(&sc) {
            self.auto.last_state = Some(sc.clone());
            self.effects.push(Effect::Schedule(Box::new(sc.clone())));
        }
        let enabled: HashMap<&str, (&Workspace, &AutoOrchestrate)> = env
            .workspaces
            .iter()
            .filter_map(|w| {
                let a = w.auto_orchestrate.as_ref().filter(|a| a.enabled)?;
                Some((w.id.as_str(), (w, a)))
            })
            .collect();
        // `automation` esposto: cambia solo alle transizioni.
        for id in enabled.keys() {
            if self.auto.info.get(*id) != Some(&info) {
                self.auto.info.insert(id.to_string(), info.clone());
                self.mark_dirty(id);
            }
        }
        let gone: Vec<String> = self.auto.info.keys().filter(|k| !enabled.contains_key(k.as_str())).cloned().collect();
        for id in gone {
            self.auto.info.remove(&id);
            self.mark_dirty(&id);
        }
        let mut ids: Vec<String> = self.auto.ws.keys().cloned().collect();
        ids.extend(enabled.keys().map(|k| k.to_string()).filter(|k| !self.auto.ws.contains_key(k)));
        for id in &ids {
            self.auto_step(id, enabled.get(id.as_str()).copied(), sc.window_open, env, now);
        }
        self.auto.enabled = enabled.keys().map(|k| k.to_string()).collect();
        self.update_awake();
    }

    fn auto_step(
        &mut self,
        id: &str,
        cfg: Option<(&Workspace, &AutoOrchestrate)>,
        open: bool,
        env: &TickEnv,
        now: i64,
    ) {
        let active = open && cfg.is_some();
        self.auto.manual.retain(|t, m| Self::manual_alive(m, t, env, now));
        let mut e = self.auto.ws.remove(id).unwrap_or_default();
        // Gate dello scheduler: a ogni transizione la board riparte.
        if active != e.gate {
            e.gate = active;
            if active {
                e.dismissed = false;
                e.fails = 0;
                e.drain_forced = false;
            }
            self.mark_dirty(id);
        }
        // Tab sparito: chiuso dall'utente (era vivo) o mai comparso.
        if let Some(t) = e.tab.as_mut() {
            if (env.live)(&t.id) {
                t.seen_live = true;
            } else if t.seen_live {
                if !t.closing {
                    e.dismissed = true;
                }
                e.tab = None;
            } else if now - t.spawned_at > SPAWN_GRACE_MS {
                e.fails = e.fails.saturating_add(1);
                e.tab = None;
            }
        }
        // Chiusura: finestra chiusa o automatismo spento.
        if !active {
            if let Some(t) = e.tab.as_mut() {
                t.closing = true;
                t.closing_since.get_or_insert(now);
            }
        }
        self.auto_idle(id, &mut e);
        // Drain scaduto: chiusura d'ufficio.
        if e.tab.as_ref().is_some_and(|t| t.closing && drain_expired(t.closing_since, now)) {
            if let Some(t) = e.tab.take() {
                self.effects.push(Effect::Close { tab_id: t.id, delay: Duration::ZERO });
            }
            e.drain_forced = true;
        }
        // Apertura: lavoro in board e nessun owner con tab vivo.
        if let (true, Some((ws, a))) = (active, cfg) {
            if e.tab.is_none() && !e.dismissed && e.fails < MAX_SPAWN_FAILS && self.has_work(id) && !self.owner_alive(id, env)
                && !self.manual_orchestrator_alive(id, env, now)
            {
                match (env.command)(ws, a) {
                    Ok(command) => {
                        self.next_worker += 1;
                        let tab_id = format!("auto-{now}-{}", self.next_worker);
                        let ev = SpawnEvent {
                            tab_id: tab_id.clone(),
                            workspace_id: ws.id.clone(),
                            command,
                            cwd: None,
                            title: ORCHESTRATOR_TITLE.to_string(),
                            parent_tab_id: String::new(),
                            agent_id: a.agent.clone(),
                            role: Role::Orchestrator,
                        };
                        self.effects.push(Effect::SpawnTab(Box::new(ev)));
                        e.tab = Some(AutoTab { id: tab_id, spawned_at: now, seen_live: false, closing: false, text_sent: false, closing_since: None });
                    }
                    Err(err) => {
                        eprintln!("orchestratore automatico {id}: {err}");
                        e.fails = MAX_SPAWN_FAILS;
                    }
                }
            }
        }
        if e.tab.is_some() || cfg.is_some() || e.drain_forced {
            self.auto.ws.insert(id.to_string(), e);
        }
    }

    /// Standby: rilascia/prende il blocco solo quando la decisione cambia.
    fn update_awake(&mut self) {
        let signals: Vec<AwakeSignal> = self
            .auto
            .ws
            .iter()
            .map(|(id, e)| {
                let active = self.auto.window_open && self.auto.enabled.contains(id);
                let running = !e.drain_forced && self.boards.get(id).is_some_and(|b| b.running_count() > 0);
                AwakeSignal {
                    open_with_work: active && self.has_work(id),
                    running,
                    draining: e.tab.as_ref().is_some_and(|t| t.closing),
                }
            })
            .collect();
        let s = &self.auto.sched;
        let need = s.enabled && schedule::needs_awake(s.keep_awake, &signals);
        if need != self.auto.awake {
            self.auto.awake = need;
            self.auto.awake_seq += 1;
            self.effects.push(Effect::KeepAwake { on: need, seq: self.auto.awake_seq });
        }
    }

    /// Il gate dello scheduler blocca l'avvio dei task di questo workspace?
    /// `open`: la finestra e' aperta ADESSO (calcolata da chi schedula, non
    /// l'ultimo valore del timer).
    pub(super) fn window_blocks(&self, ws: &Workspace, open: bool) -> bool {
        ws.auto_orchestrate.as_ref().is_some_and(|a| a.enabled) && self.auto.sched.enabled && !open
    }
}

// ---------------------------------------------------------------------------
// AutomationState: giri, comandi
// ---------------------------------------------------------------------------

impl AutomationState {
    /// Giro con l'ora locale dell'host. `force_emit` vedi `auto_tick`.
    pub fn reconcile_at<H: Host>(&self, host: &H, now: i64, force_emit: bool) {
        let minute = host.local_minute(now);
        let workspaces = host.workspaces();
        let live = |t: &str| host.tab_workspace(t).is_some();
        let command = |ws: &Workspace, a: &AutoOrchestrate| {
            let req = AutomationRequest {
                bootstrap_prompt: ORCHESTRATOR_BOOTSTRAP_PROMPT,
                permission_mode: ws.worker_permission_mode(),
                model: a.model.as_deref(),
            };
            host.launch_command(&a.agent, &ws.kind, &req)
        };
        let env = TickEnv { minute, workspaces: &workspaces, live: &live, command: &command };
        self.run(host, |r| r.auto_tick(&env, now, force_emit));
    }

    /// Un tab dell'avvio "Orchestratore" sta per aprirsi: registrato se il
    /// workspace non ha gia' un orchestratore vivo (automatico o manuale).
    pub fn orchestrator_open<H: Host>(&self, host: &H, ws: &str, tab: &str, now: i64) -> Result<(), String> {
        if host.workspace(ws).is_none() {
            return Err("workspace non trovato".to_string());
        }
        let live = |t: &str| host.tab_workspace(t).is_some();
        let command = |_: &Workspace, _: &AutoOrchestrate| Err(String::new());
        let env = TickEnv { minute: 0, workspaces: &[], live: &live, command: &command };
        self.run(host, |r| r.manual_orchestrator_open(ws, tab, &env, now))
    }

    /// Cambio di configurazione (schedule o workspace): giro immediato.
    pub fn reconcile<H: Host>(&self, host: &H) {
        self.reconcile_at(host, now_millis(), true);
    }

    /// Giro del timer.
    pub fn tick<H: Host>(&self, host: &H, now: i64) {
        self.reconcile_at(host, now, false);
    }

    pub fn schedule_state<H: Host>(&self, host: &H, now: i64) -> ScheduleState {
        let minute = host.local_minute(now);
        self.inner.lock().unwrap().schedule_state(minute, now)
    }

    /// `schedule_set`: valida, salva, applica e ritorna lo stato.
    pub fn schedule_set<H: Host>(&self, host: &H, s: Schedule, now: i64) -> Result<ScheduleState, String> {
        schedule::validate(&s)?;
        host.save_schedule(&s)?;
        self.run(host, |r| r.set_schedule(s));
        self.reconcile_at(host, now, true);
        Ok(self.schedule_state(host, now))
    }

    /// `workflow_save`: vuoto o `None` elimina.
    pub fn workflow_save<H: Host>(&self, host: &H, ws_id: &str, content: Option<&str>) -> Result<(), String> {
        if host.workspace(ws_id).is_none() {
            return Err("workspace non trovato".to_string());
        }
        let content = content.map(str::trim).filter(|c| !c.is_empty());
        if let Some(c) = content {
            if !schedule::workflow_len_ok(c) {
                return Err(format!("workflow troppo lungo (max {} caratteri)", schedule::WORKFLOW_MAX));
            }
        }
        host.workflow_write(ws_id, content)
    }
}

/// Avvio dell'app: carica lo schedule e fa partire il timer.
pub fn init(app: &tauri::App) {
    let handle = app.handle().clone();
    let host = TauriHost(handle.clone());
    let st = handle.state::<AutomationState>().inner().clone();
    if let Ok(dir) = handle.path().app_data_dir() {
        let s = schedule::load_schedule(&dir);
        let minute = host.local_minute(now_millis());
        st.run(&host, |r| r.init_schedule(s, minute));
    }
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        loop {
            st.tick(&host, now_millis());
            std::thread::sleep(TICK_EVERY);
        }
    });
}

pub(super) fn emit_schedule(app: &AppHandle, st: &ScheduleState) {
    let _ = app.emit(SCHEDULE_EVENT, st);
}

pub(super) fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

pub(super) fn workflows_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    data_dir(app).map(|d| d.join("workflows"))
}

#[tauri::command]
pub async fn schedule_get(app: AppHandle) -> Result<ScheduleState, String> {
    crate::blocking(move || Ok(app.state::<AutomationState>().schedule_state(&TauriHost(app.clone()), now_millis()))).await
}

#[tauri::command]
pub async fn schedule_set(app: AppHandle, schedule: Schedule) -> Result<ScheduleState, String> {
    crate::blocking(move || app.state::<AutomationState>().schedule_set(&TauriHost(app.clone()), schedule, now_millis()))
        .await
}

/// Workflow del workspace (None se non esiste).
#[tauri::command]
pub async fn workflow_get(app: AppHandle, workspace_id: String) -> Result<Option<String>, String> {
    crate::blocking(move || TauriHost(app).workflow_read(&workspace_id)).await
}

/// Salva il workflow del workspace (None o vuoto = elimina).
#[tauri::command]
pub async fn workflow_save(app: AppHandle, workspace_id: String, content: Option<String>) -> Result<(), String> {
    crate::blocking(move || {
        app.state::<AutomationState>().workflow_save(&TauriHost(app.clone()), &workspace_id, content.as_deref())
    })
    .await
}

// ---------------------------------------------------------------------------
// RPC dell'orchestratore: workflow
// ---------------------------------------------------------------------------

/// `get_workflow` / `save_workflow`: il workflow e' quello del workspace del
/// tab chiamante (qualunque orchestratore del workspace, anche il wizard in
/// un tab normale).
pub(super) fn rpc_workflow<H: Host>(host: &H, orch: &str, method: &str, params: &Value) -> Result<Value, String> {
    let ws = host
        .tab_workspace(orch)
        .ok_or_else(|| "the orchestrator's workspace was not found".to_string())?;
    match method {
        "get_workflow" => {
            let w = host.workflow_read(&ws.id).map_err(|_| "the workflow could not be read".to_string())?;
            Ok(json!({ "workflow": w }))
        }
        "save_workflow" => {
            let content = req_text("content", req_str(params, "content")?, schedule::WORKFLOW_MAX)?;
            host.workflow_write(&ws.id, Some(&content)).map_err(|_| "the workflow could not be saved".to_string())?;
            Ok(json!({ "saved": true, "characters": content.chars().count() }))
        }
        _ => Err(format!("unknown method: {method}")),
    }
}
