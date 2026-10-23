//! Fasce orarie e standby dentro l'automazione (contratto: `docs/BOARD.MD`,
//! "Fasce orarie"). Le fasce sono solo un gate dello scheduler dei workspace
//! che le usano (`use_schedule`): fuori fascia non parte nessun task, quelli in
//! corso finiscono. Stesso schema del resto: decisioni PURE nel registro (tempo
//! e ora locale iniettati dal chiamante), gli effetti (evento, standby) sono
//! `Effect` applicati fuori dal lock.
//!
//! Ogni giro (`auto_tick`: timer ogni 30 s, subito a ogni cambio di
//! configurazione) riallinea lo stato: e' a livello, non a fronti, quindi
//! copre anche l'avvio dell'app dentro una fascia e le modifiche a meta' fascia.

use super::*;
use crate::schedule::{self, Schedule, ScheduleState};
use std::collections::HashSet;

/// Giro del timer e attesa iniziale (il frontend deve aver registrato i
/// listener prima del primo `automation-spawn`).
pub const TICK_EVERY: Duration = Duration::from_secs(30);
const STARTUP_DELAY: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(super) struct AutoCore {
    pub(super) sched: Schedule,
    /// Ultimo stato emesso (`schedule-state`).
    last_state: Option<ScheduleState>,
    /// Ultimo gate delle fasce visto per workspace (`true` = fuori fascia):
    /// una transizione fa ripartire lo scheduler della sua board.
    blocked: HashMap<String, bool>,
    awake: bool,
    /// Numero d'ordine degli effetti `KeepAwake`.
    awake_seq: u64,
    /// Primo giro dopo l'avvio gia' fatto.
    started: bool,
}

/// Cio' che serve a un giro, fornito da chi chiama (il nucleo non conosce
/// ne' orologio, ne' workspace).
pub struct TickEnv<'a> {
    /// Minuti locali del giorno.
    pub minute: u16,
    pub workspaces: &'a [Workspace],
}

impl Registry {
    /// Carica lo schedule (avvio).
    pub fn init_schedule(&mut self, s: Schedule) {
        self.auto.sched = s;
    }

    /// Nuovo schedule (gia' validato e salvato); il giro che segue applica.
    pub fn set_schedule(&mut self, s: Schedule) {
        self.auto.sched = s;
    }

    pub fn schedule_state(&self, minute: u16, now: i64) -> ScheduleState {
        schedule::state(&self.auto.sched, minute, now)
    }

    /// Il workspace usa le fasce, sono attive e adesso e' fuori fascia?
    pub(super) fn schedule_blocked(&self, ws: &Workspace, minute: u16) -> bool {
        ws.use_schedule && self.auto.sched.enabled && !schedule::window_open(&self.auto.sched, minute)
    }

    /// Un giro: guardiano degli avvii, `schedule-state` se cambia, e le board
    /// dei workspace il cui gate e' cambiato ripartono. `force`: configurazione
    /// modificata, tutte le board si rivalutano e lo stato si emette comunque.
    pub fn auto_tick(&mut self, env: &TickEnv, now: i64, force: bool) {
        // Primo giro dopo l'avvio: tutte le board da schedulare (i task in coda
        // ripartono dopo un riavvio).
        if !self.auto.started {
            self.auto.started = true;
            self.mark_all_dirty();
        }
        self.watchdog(now);
        let sc = self.schedule_state(env.minute, now);
        if force || self.auto.last_state.as_ref() != Some(&sc) {
            self.auto.last_state = Some(sc.clone());
            self.effects.push(Effect::Schedule(Box::new(sc)));
        }
        let ids: HashSet<&str> = env.workspaces.iter().map(|w| w.id.as_str()).collect();
        self.auto.blocked.retain(|id, _| ids.contains(id.as_str()));
        for ws in env.workspaces {
            let blocked = self.schedule_blocked(ws, env.minute);
            let changed = self.auto.blocked.insert(ws.id.clone(), blocked).is_some_and(|prev| prev != blocked);
            if changed {
                self.mark_dirty(&ws.id);
            }
        }
        if force {
            self.mark_all_dirty();
        }
        self.update_awake();
    }

    fn mark_all_dirty(&mut self) {
        let ids: Vec<String> = self.boards.keys().cloned().collect();
        for id in ids {
            self.mark_dirty(&id);
        }
    }

    /// Standby: il PC resta sveglio finche' un task e' in corso in un
    /// workspace non archiviato (l'archiviazione rimette in coda i suoi task).
    /// Prende/rilascia il blocco solo quando la decisione cambia.
    pub(super) fn update_awake(&mut self) {
        let need = self.auto.sched.keep_awake && self.boards.values().any(|b| b.running_count() > 0);
        if need != self.auto.awake {
            self.auto.awake = need;
            self.auto.awake_seq += 1;
            self.effects.push(Effect::KeepAwake { on: need, seq: self.auto.awake_seq });
        }
    }
}

impl AutomationState {
    /// Giro con l'ora locale dell'host. `force` vedi `auto_tick`.
    pub fn reconcile_at<H: Host>(&self, host: &H, now: i64, force: bool) {
        let workspaces = host.workspaces();
        let env = TickEnv { minute: host.local_minute(now), workspaces: &workspaces };
        self.run(host, |r| r.auto_tick(&env, now, force));
    }

    /// Il workspace (gia' salvato con il nuovo flag) e' stato archiviato o
    /// riattivato: l'archiviazione rimette in coda i task in corso, la
    /// riattivazione fa ripartire lo scheduler; poi un giro.
    pub fn set_archived<H: Host>(&self, host: &H, ws: &str, archived: bool) {
        let now = now_millis();
        if archived {
            self.run(host, |r| r.archive_workspace(ws, now));
        }
        self.reconcile_at(host, now, true);
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
}

/// Avvio dell'app: carica lo schedule e fa partire il timer.
pub fn init(app: &tauri::App) {
    let handle = app.handle().clone();
    let host = TauriHost(handle.clone());
    let st = handle.state::<AutomationState>().inner().clone();
    if let Ok(dir) = handle.path().app_data_dir() {
        let s = schedule::load_schedule(&dir);
        st.run(&host, |r| r.init_schedule(s));
    }
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        loop {
            st.tick(&host, now_millis());
            std::thread::sleep(TICK_EVERY);
        }
    });
}

pub(super) fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn schedule_get(app: AppHandle) -> Result<ScheduleState, String> {
    crate::blocking(move || Ok(app.state::<AutomationState>().schedule_state(&TauriHost(app.clone()), now_millis()))).await
}

#[tauri::command]
pub async fn schedule_set(app: AppHandle, schedule: Schedule) -> Result<ScheduleState, String> {
    crate::blocking(move || app.state::<AutomationState>().schedule_set(&TauriHost(app.clone()), schedule, now_millis())).await
}
