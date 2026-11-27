//! Il motore in esecuzione (stato Tauri): le sessioni (`Sessions`) sotto un
//! solo lock, un esecutore async per ogni thread con lavori in coda e lo
//! schedulatore delle scadenze.
//!
//! - **Esecutori.** Una coda per thread, svuotata da un task di
//!   `tauri::async_runtime` che esegue i lavori uno alla volta (`jobs`). Il lock
//!   si prende solo per leggere o aggiornare le sessioni: mai durante una
//!   chiamata al modello ne' mentre si tengono i lock dei thread o della board.
//! - **Scadenze.** Un solo thread (`hub-secretary-sessions`) per tutte le
//!   sessioni, non uno per sessione: dorme fino alla prossima scadenza (al
//!   massimo `SCHEDULER_MAX_WAIT`, cosi' una modifica di `idle_minutes` vale
//!   entro quel tempo), chiude le sessioni scadute accodandone il riassunto e
//!   si ferma quando non ci sono piu' sessioni. Il controllo e' anche pigro:
//!   `route` non usa come titolare una sessione scaduta che lo schedulatore non
//!   ha ancora raccolto.
//! - **Spegnimento.** Spegnere l'HUB butta tutte le sessioni e annulla gli
//!   esecutori (`abort`); l'epoca cresce, cosi' un lavoro gia' oltre l'ultimo
//!   punto di attesa non scrive piu' nulla nei thread (`is_current`).

use super::jobs;
use super::sessions::{Fallback, Job, Notice, Routing, SessionTurn, Sessions, ThreadKey};
use super::status::{self, SecretaryStatus};
use super::texts;
use crate::hub::append::{append_text, TextMessage};
use crate::hub::channel::Channel;
use crate::hub::commands::hub_dirs_of;
use crate::hub::events::EventWake;
use crate::hub::model::{Author, Message};
use crate::hub::settings::{HubSettingsStore, SessionSettings};
use crate::hub::state::HubState;
use crate::hub::threads::THREAD_NOT_FOUND;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager};

const SCHEDULER_NAME: &str = "hub-secretary-sessions";
/// Attesa massima dello schedulatore tra due controlli.
const SCHEDULER_MAX_WAIT: Duration = Duration::from_secs(30);
/// Attesa minima: una scadenza gia' passata non fa girare il thread a vuoto.
const SCHEDULER_MIN_WAIT: Duration = Duration::from_millis(200);
const MINUTE_MS: i64 = 60_000;

#[derive(Default)]
struct Inner {
    sessions: Sessions,
    executors: HashMap<ThreadKey, JoinHandle<()>>,
    /// Cresce a ogni spegnimento: i lavori di un'epoca vecchia non scrivono.
    epoch: u64,
    scheduler: bool,
}

#[derive(Default)]
pub struct SecretaryEngine {
    inner: Mutex<Inner>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Inattivita' dopo cui una sessione si chiude, in millisecondi.
pub fn idle_ms(session: &SessionSettings) -> i64 {
    i64::from(session.idle_minutes) * MINUTE_MS
}

impl SecretaryEngine {
    /// Un messaggio dell'utente appena scritto nel thread: sveglia chi deve
    /// rispondere (`Sessions::route`) e scrive gli avvisi delle menzioni
    /// rifiutate o del messaggio che nessuno ha ricevuto. Bloccante (legge e
    /// scrive i thread): fuori dal main thread.
    pub fn on_message(&self, app: &AppHandle, channel: &Channel, thread_id: &str, message: &Message) {
        let settings = app.state::<HubSettingsStore>().get();
        if !settings.is_on() || message.author != Author::User {
            return;
        }
        let key = ThreadKey::new(channel, thread_id);
        // Prima del lock del motore: mai insieme ai lock dei thread.
        let fallback = load_fallback(app, &key, message);
        let routing = {
            let mut inner = self.lock();
            let routing = inner.sessions.route(
                &key,
                message,
                &fallback,
                &settings.secretaries,
                idle_ms(&settings.session),
                now_ms(),
            );
            inner.run_routing(app, &key, &routing);
            routing
        };
        for notice in &routing.notices {
            let text = match notice {
                Notice::NotConfigured(name) => texts::not_configured(name),
                Notice::NotAllowed(name) => texts::not_allowed(name),
                Notice::Unanswered => texts::UNANSWERED.to_string(),
            };
            let system = TextMessage { author: Author::System, body: text, reply_to: Some(message.id.clone()) };
            if let Err(e) = append_text(app, channel, thread_id, system) {
                log::error!("hub: avviso del messaggio non scritto nel thread {thread_id}: {e}");
            }
        }
        if !routing.woken.is_empty() {
            self.emit_status(app);
        }
    }

    /// Eventi della board appena scritti nei thread del canale: sveglia i
    /// titolari (`events::route`). Da chiamare dopo aver rilasciato il lock dei
    /// thread; veloce: accoda soltanto.
    pub fn on_events(&self, app: &AppHandle, channel: &Channel, wakes: &[EventWake]) {
        if wakes.is_empty() {
            return;
        }
        let settings = app.state::<HubSettingsStore>().get();
        if !settings.is_on() {
            return;
        }
        let idle = idle_ms(&settings.session);
        let mut woken = false;
        {
            let mut inner = self.lock();
            for wake in wakes {
                let key = ThreadKey::new(channel, &wake.thread_id);
                let routing = inner.sessions.route_event(
                    &key,
                    &wake.secretary,
                    &wake.trigger,
                    &settings.secretaries,
                    idle,
                    now_ms(),
                );
                woken |= inner.run_wake(app, &key, &wake.secretary, &routing);
            }
        }
        if woken {
            self.emit_status(app);
        }
    }

    /// Un elemento esterno appena salvato come messaggio `trigger_id` del
    /// thread: sveglia `secretary` (gia' risolto da chi chiama: il titolare o
    /// il segretario nominato dalla regola di consegna), mai per le menzioni
    /// del testo. Da chiamare dopo aver rilasciato il lock dei thread; veloce:
    /// accoda soltanto.
    pub fn on_external(&self, app: &AppHandle, channel: &Channel, thread_id: &str, trigger_id: &str, secretary: &str) {
        let settings = app.state::<HubSettingsStore>().get();
        if !settings.is_on() {
            return;
        }
        let key = ThreadKey::new(channel, thread_id);
        let woken = {
            let mut inner = self.lock();
            let routing = inner.sessions.route_external(
                &key,
                secretary,
                trigger_id,
                &settings.secretaries,
                idle_ms(&settings.session),
                now_ms(),
            );
            inner.run_wake(app, &key, secretary, &routing)
        };
        if woken {
            self.emit_status(app);
        }
    }

    /// L'HUB e' stato spento: tutte le sessioni si chiudono (senza riassunto)
    /// e i lavori in corso si annullano.
    pub fn shutdown(&self, app: &AppHandle) {
        {
            let mut inner = self.lock();
            inner.epoch += 1;
            inner.sessions.clear();
            for (_, executor) in inner.executors.drain() {
                executor.abort();
            }
        }
        self.emit_status(app);
    }

    /// Il workspace del canale e' stato rimosso: via le sue sessioni e i suoi lavori.
    pub fn forget_channel(&self, app: &AppHandle, channel: &Channel) {
        {
            let mut inner = self.lock();
            inner.sessions.forget_channel(channel);
            inner.executors.retain(|key, executor| {
                let keep = &key.channel != channel;
                if !keep {
                    executor.abort();
                }
                keep
            });
        }
        self.emit_status(app);
    }

    /// Il thread e' stato eliminato: via le sue sessioni (senza riassunto), la
    /// sua coda e il suo esecutore. Un lavoro gia' oltre l'ultimo punto di
    /// attesa non ricrea il thread: le sue scritture falliscono con
    /// `THREAD_NOT_FOUND`.
    pub fn forget_thread(&self, app: &AppHandle, channel: &Channel, thread_id: &str) {
        let key = ThreadKey::new(channel, thread_id);
        {
            let mut inner = self.lock();
            inner.sessions.forget_thread(&key);
            if let Some(executor) = inner.executors.remove(&key) {
                executor.abort();
            }
        }
        self.emit_status(app);
    }

    pub fn status(&self, app: &AppHandle) -> Vec<SecretaryStatus> {
        let idle = app.state::<HubSettingsStore>().read(|s| idle_ms(&s.session));
        self.lock().sessions.awake(idle).into_iter().map(SecretaryStatus::from).collect()
    }

    pub(super) fn emit_status(&self, app: &AppHandle) {
        status::emit(app, &self.status(app));
    }

    /// Il lavoro dell'epoca `epoch` puo' ancora scrivere nei thread.
    pub(super) fn is_current(&self, epoch: u64) -> bool {
        self.lock().epoch == epoch
    }

    pub(super) fn begin_turn(&self, key: &ThreadKey, name: &str, epoch: u64) -> Option<SessionTurn> {
        let mut inner = self.lock();
        if inner.epoch != epoch {
            return None;
        }
        inner.sessions.begin_turn(key, name)
    }

    pub(super) fn end_turn(&self, key: &ThreadKey, name: &str, turn: SessionTurn, epoch: u64) {
        let mut inner = self.lock();
        if inner.epoch == epoch {
            inner.sessions.end_turn(key, name, turn, now_ms());
        }
    }

    /// Chiude la sessione senza riassunto (il segretario non c'e' piu').
    pub(super) fn close(&self, key: &ThreadKey, name: &str) {
        self.lock().sessions.close(key, name, false);
    }

    /// Il prossimo lavoro della coda del thread; `None` se la coda e' vuota
    /// (l'esecutore si ferma e si toglie) o l'epoca e' cambiata.
    pub(super) fn next_job(&self, key: &ThreadKey, epoch: u64) -> Option<Job> {
        let mut inner = self.lock();
        if inner.epoch != epoch {
            return None;
        }
        let job = inner.sessions.next_job(key);
        if job.is_none() {
            inner.executors.remove(key);
        }
        job
    }

    pub(super) fn job_done(&self, key: &ThreadKey, epoch: u64) {
        let mut inner = self.lock();
        if inner.epoch == epoch {
            inner.sessions.job_done(key);
        }
    }

    /// Un panico isolato non deve fermare il motore: le sessioni restano
    /// utilizzabili (al peggio una conversazione e' vuota).
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Inner {
    /// Chiude un risveglio di un solo segretario (evento o elemento esterno):
    /// se non e' partito nessun turno lo dice nel log (il messaggio resta nel
    /// thread), altrimenti avvia la coda. `true` se ha svegliato qualcuno.
    fn run_wake(&mut self, app: &AppHandle, key: &ThreadKey, secretary: &str, routing: &Routing) -> bool {
        if routing.woken.is_empty() {
            log::info!("hub: @{secretary} non puo' rispondere, messaggio solo nel thread {}", key.thread_id);
        }
        self.run_routing(app, key, routing);
        !routing.woken.is_empty()
    }

    /// Avvia (o rianima) l'esecutore della coda e lo schedulatore se il
    /// risveglio ha accodato dei turni.
    fn run_routing(&mut self, app: &AppHandle, key: &ThreadKey, routing: &Routing) {
        if routing.woken.is_empty() {
            return;
        }
        if routing.start_lane {
            self.start_executor(app, key);
        } else {
            self.revive_executor(app, key);
        }
        self.ensure_scheduler(app);
    }

    /// Avvia l'esecutore della coda del thread. Sotto il lock: l'esecutore
    /// legge la coda solo dopo che la sua maniglia e' registrata.
    fn start_executor(&mut self, app: &AppHandle, key: &ThreadKey) {
        let handle = tauri::async_runtime::spawn(jobs::run_queue(app.clone(), key.clone(), self.epoch));
        self.executors.insert(key.clone(), handle);
    }

    /// Un esecutore che ha preso la coda e si e' fermato senza svuotarla (un
    /// panico in un lavoro) la lascerebbe bloccata: se ne avvia un altro.
    fn revive_executor(&mut self, app: &AppHandle, key: &ThreadKey) {
        if self.executors.get(key).is_some_and(|executor| executor.inner().is_finished()) {
            log::error!("hub: l'esecutore del thread {} si e' fermato in modo anomalo, lo riavvio", key.thread_id);
            self.start_executor(app, key);
        }
    }

    fn ensure_scheduler(&mut self, app: &AppHandle) {
        if self.scheduler {
            return;
        }
        let worker_app = app.clone();
        match std::thread::Builder::new().name(SCHEDULER_NAME.to_string()).spawn(move || run_scheduler(&worker_app)) {
            Ok(_) => self.scheduler = true,
            // Senza schedulatore le sessioni scadono comunque in modo pigro
            // (nessun titolare scaduto); il riassunto arriva alla prossima menzione.
            Err(e) => log::error!("hub: schedulatore delle sessioni non avviato: {e}"),
        }
    }
}

/// I candidati di ripiego del thread per `message`; vuoti (con un avviso nel
/// log) se il thread non si legge: restano le sessioni attive.
fn load_fallback(app: &AppHandle, key: &ThreadKey, message: &Message) -> Fallback {
    let loaded = hub_dirs_of(app).and_then(|dirs| {
        app.state::<HubState>()
            .read(&dirs, &key.channel, |list| list.thread(&key.thread_id).map(|t| Fallback::of(t, message)))
            .ok_or_else(|| THREAD_NOT_FOUND.to_string())
    });
    loaded.unwrap_or_else(|e| {
        log::warn!("hub: thread {} non letto per scegliere il segretario: {e}", key.thread_id);
        Fallback::default()
    })
}

fn run_scheduler(app: &AppHandle) {
    let engine = app.state::<SecretaryEngine>();
    loop {
        let idle = app.state::<HubSettingsStore>().read(|s| idle_ms(&s.session));
        let now = now_ms();
        let (closed, next) = {
            let mut inner = engine.lock();
            if inner.sessions.is_empty() {
                inner.scheduler = false;
                return;
            }
            let before = inner.sessions.open_count();
            for key in inner.sessions.expire_due(idle, now) {
                inner.start_executor(app, &key);
            }
            (inner.sessions.open_count() != before, inner.sessions.next_deadline(idle))
        };
        if closed {
            engine.emit_status(app);
        }
        std::thread::sleep(wait_until(next, now));
    }
}

/// Quanto dormire fino alla scadenza `next`, tra i due limiti.
fn wait_until(next: Option<i64>, now: i64) -> Duration {
    next.map(|deadline| Duration::from_millis(u64::try_from(deadline - now).unwrap_or(0)))
        .unwrap_or(SCHEDULER_MAX_WAIT)
        .clamp(SCHEDULER_MIN_WAIT, SCHEDULER_MAX_WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheduler_waits_until_the_deadline_within_bounds() {
        assert_eq!(wait_until(None, 0), SCHEDULER_MAX_WAIT);
        assert_eq!(wait_until(Some(5_000), 0), Duration::from_secs(5));
        assert_eq!(wait_until(Some(-10), 0), SCHEDULER_MIN_WAIT);
        assert_eq!(wait_until(Some(10 * 60_000), 0), SCHEDULER_MAX_WAIT);
    }

    #[test]
    fn idle_minutes_become_milliseconds() {
        assert_eq!(idle_ms(&SessionSettings::new(20, 8).unwrap()), 20 * 60_000);
    }
}
