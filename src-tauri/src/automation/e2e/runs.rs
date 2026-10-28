//! Esecuzioni E2E in corso e ultimi esiti manuali. Un'esecuzione alla volta
//! per CARTELLA: copre una per task, una manuale per repository e impedisce
//! che una manuale e una di un task nella stessa cartella si calpestino il
//! report JUnit.
//!
//! Ordine dei lock: il mutex di `E2eRuns` e' una foglia. Non si prende mai
//! con `AutomationState::inner` (o altri lock) in mano e sotto di esso non si
//! prende nessun altro lock; l'attesa di fine esecuzione avviene fuori dal
//! mutex, sul segnale dell'esecuzione.

use crate::automation::process::CancelFlag;
use crate::board::E2eRun;
use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Tentativi di trovare una porta libera non gia' riservata da un'altra esecuzione.
const PORT_ATTEMPTS: usize = 20;

/// Chi possiede un'esecuzione.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Owner {
    Task { ws: String, task: String },
    Repo { ws: String, repo: String },
}

impl Owner {
    fn workspace(&self) -> &str {
        match self {
            Owner::Task { ws, .. } | Owner::Repo { ws, .. } => ws,
        }
    }
}

/// Chiave "<workspace>|<nome>": una cartella (esecuzioni in corso) o un repository (esiti manuali).
pub(super) fn scope_key(ws_id: &str, dir: &str) -> String {
    format!("{ws_id}|{dir}")
}

/// Perche' una cartella non si puo' prendere.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ClaimError {
    /// C'e' gia' un'esecuzione in corso (il suo id).
    Busy(String),
    NoFreePort,
}

/// Segnale di fine esecuzione.
type Done = Arc<(Mutex<bool>, Condvar)>;

struct ActiveRun {
    run_id: String,
    owner: Owner,
    cancel: CancelFlag,
    done: Done,
}

#[derive(Default)]
struct Inner {
    /// Esecuzioni in corso per cartella (`scope_key`).
    active: HashMap<String, ActiveRun>,
    /// Porte riservate dalle esecuzioni in corso.
    ports: HashSet<u16>,
    /// Ultimo esito manuale per "<workspace>|<repository>" (solo in memoria).
    last_manual: HashMap<String, E2eRun>,
    counter: u64,
}

impl Inner {
    /// Una porta libera di localhost non usata da un'altra esecuzione: si
    /// apre una porta 0, si legge il numero e si chiude. Con WSL2 la verifica
    /// e' sul lato Windows (il servizio nella distro la riapre da se').
    fn reserve_port(&mut self) -> Option<u16> {
        (0..PORT_ATTEMPTS).find_map(|_| {
            let port = TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).ok()?.port();
            self.ports.insert(port).then_some(port)
        })
    }
}

#[derive(Default)]
pub(crate) struct E2eRuns {
    inner: Mutex<Inner>,
}

impl E2eRuns {
    /// Il mutex avvelenato non blocca il rilascio delle cartelle: lo stato
    /// resta coerente perche' ogni modifica e' una singola operazione di mappa.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Prende la cartella per un'esecuzione e le riserva una porta. Il
    /// controllo e l'inserimento sono atomici. Il guard rilascia tutto al drop.
    pub(super) fn claim(self: &Arc<Self>, folder: String, owner: Owner, now: i64) -> Result<RunGuard, ClaimError> {
        let mut inner = self.lock();
        if let Some(active) = inner.active.get(&folder) {
            return Err(ClaimError::Busy(active.run_id.clone()));
        }
        let port = inner.reserve_port().ok_or(ClaimError::NoFreePort)?;
        inner.counter += 1;
        let run_id = format!("e2e-{now}-{}", inner.counter);
        let (cancel, done) = (CancelFlag::default(), Done::default());
        inner.active.insert(
            folder.clone(),
            ActiveRun { run_id: run_id.clone(), owner, cancel: cancel.clone(), done },
        );
        Ok(RunGuard { runs: Arc::clone(self), folder, run_id, port, cancel })
    }

    /// Annulla le esecuzioni in corso del task.
    pub(super) fn cancel_task(&self, ws: &str, task: &str) {
        self.cancel_where(|o| matches!(o, Owner::Task { ws: w, task: t } if w == ws && t == task));
    }

    /// Annulla tutte le esecuzioni del workspace, manuali comprese.
    pub(super) fn cancel_workspace(&self, ws: &str) {
        self.cancel_where(|o| o.workspace() == ws);
    }

    fn cancel_where(&self, matches: impl Fn(&Owner) -> bool) {
        for run in self.lock().active.values().filter(|r| matches(&r.owner)) {
            run.cancel.cancel();
        }
    }

    /// Aspetta fino a `timeout` che l'esecuzione `run_id` finisca (subito se
    /// non e' in corso). Il mutex della mappa non e' in mano durante l'attesa.
    pub(super) fn wait(&self, run_id: &str, timeout: Duration) {
        let done = self.lock().active.values().find(|r| r.run_id == run_id).map(|r| Arc::clone(&r.done));
        if let Some(done) = done {
            let (finished, signal) = &*done;
            let guard = finished.lock().unwrap_or_else(PoisonError::into_inner);
            // Scaduto o finito: in entrambi i casi chi chiama rilegge lo stato.
            drop(signal.wait_timeout_while(guard, timeout, |finished| !*finished));
        }
    }

    pub(super) fn set_last_manual(&self, ws: &str, repo: &str, run: E2eRun) {
        self.lock().last_manual.insert(scope_key(ws, repo), run);
    }

    pub(super) fn last_manual(&self, ws: &str, repo: &str) -> Option<E2eRun> {
        self.lock().last_manual.get(&scope_key(ws, repo)).cloned()
    }

    #[cfg(test)]
    pub(crate) fn reserved_ports(&self) -> Vec<u16> {
        self.lock().ports.iter().copied().collect()
    }
}

/// Possesso di una cartella: al drop (anche per un panic) la cartella e la
/// porta tornano libere e chi aspetta viene svegliato.
pub(super) struct RunGuard {
    runs: Arc<E2eRuns>,
    folder: String,
    pub(super) run_id: String,
    pub(super) port: u16,
    pub(super) cancel: CancelFlag,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        let removed = {
            let mut inner = self.runs.lock();
            inner.ports.remove(&self.port);
            inner.active.remove(&self.folder)
        };
        if let Some(run) = removed {
            let (finished, signal) = &*run.done;
            *finished.lock().unwrap_or_else(PoisonError::into_inner) = true;
            signal.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(t: &str) -> Owner {
        Owner::Task { ws: "ws-1".into(), task: t.into() }
    }

    fn claim(runs: &Arc<E2eRuns>, folder: &str, owner: Owner) -> RunGuard {
        runs.claim(folder.to_string(), owner, 7).unwrap()
    }

    #[test]
    fn one_run_per_folder_until_the_guard_drops() {
        let runs = Arc::new(E2eRuns::default());
        let first = claim(&runs, "f", task("a"));
        assert_eq!(first.run_id, "e2e-7-1");
        assert_eq!(runs.claim("f".into(), task("b"), 7).err(), Some(ClaimError::Busy("e2e-7-1".into())));
        assert!(runs.claim("g".into(), task("b"), 7).is_ok());
        drop(first);
        assert!(runs.claim("f".into(), task("b"), 7).is_ok());
    }

    #[test]
    fn run_ids_are_unique() {
        let runs = Arc::new(E2eRuns::default());
        let (a, b) = (claim(&runs, "f", task("a")), claim(&runs, "g", task("a")));
        assert_ne!(a.run_id, b.run_id);
    }

    #[test]
    fn active_runs_never_share_a_port_and_release_it() {
        let runs = Arc::new(E2eRuns::default());
        let guards: Vec<RunGuard> = (0..8).map(|i| claim(&runs, &format!("f{i}"), task("a"))).collect();
        let ports: HashSet<u16> = guards.iter().map(|g| g.port).collect();
        assert_eq!(ports.len(), guards.len());
        assert!(ports.iter().all(|p| *p > 0));
        assert_eq!(runs.reserved_ports().len(), guards.len());
        drop(guards);
        assert!(runs.reserved_ports().is_empty());
    }

    #[test]
    fn cancel_reaches_only_the_matching_owner() {
        let runs = Arc::new(E2eRuns::default());
        let (a, b) = (claim(&runs, "f", task("a")), claim(&runs, "g", task("b")));
        let manual = claim(&runs, "h", Owner::Repo { ws: "ws-1".into(), repo: ".".into() });
        let other_ws = claim(&runs, "i", Owner::Task { ws: "ws-2".into(), task: "a".into() });
        runs.cancel_task("ws-1", "a");
        assert_eq!([&a, &b, &manual, &other_ws].map(|g| g.cancel.is_cancelled()), [true, false, false, false]);
        runs.cancel_workspace("ws-1");
        assert_eq!([&b, &manual, &other_ws].map(|g| g.cancel.is_cancelled()), [true, true, false]);
    }

    #[test]
    fn wait_returns_when_the_guard_drops_or_the_time_runs_out() {
        let runs = Arc::new(E2eRuns::default());
        runs.wait("unknown", Duration::from_secs(5));
        let guard = claim(&runs, "f", task("a"));
        let id = guard.run_id.clone();
        let started = std::time::Instant::now();
        runs.wait(&id, Duration::from_millis(30));
        assert!(started.elapsed() >= Duration::from_millis(30));
        let waiter = {
            let (runs, id) = (Arc::clone(&runs), id.clone());
            std::thread::spawn(move || runs.wait(&id, Duration::from_secs(20)))
        };
        std::thread::sleep(Duration::from_millis(50));
        drop(guard);
        waiter.join().unwrap();
    }

    #[test]
    fn last_manual_results_are_kept_per_repository() {
        let runs = E2eRuns::default();
        let run = E2eRun::started("e2e-1-1".into(), "j.xml".into(), 1);
        runs.set_last_manual("ws-1", "app", run.clone());
        assert_eq!(runs.last_manual("ws-1", "app"), Some(run));
        assert_eq!(runs.last_manual("ws-1", "."), None);
    }
}
