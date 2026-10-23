//! Effetti verso l'esterno dell'automazione: il trait `Host` (eventi Tauri,
//! git, file, timer, workspace) e la sua implementazione reale `TauriHost`.
//! I test usano un host finto (vedi `tests`).

use super::*;
use crate::agents::{self, AutomationRequest};
use crate::schedule::{Schedule, ScheduleState};
use crate::workspaces::WorkspaceStore;
use tauri::Emitter;

/// Eventi Tauri verso il frontend.
const SPAWN_EVENT: &str = "automation-spawn";
const CLOSE_EVENT: &str = "automation-close";
const UPDATE_EVENT: &str = "automation-update";
const BOARD_EVENT: &str = "board-update";
const NOTICE_EVENT: &str = "task-notify";
const SCHEDULE_EVENT: &str = "schedule-state";

/// Effetti verso l'esterno. `Clone + Send + 'static`: i timer li portano in
/// un thread.
pub trait Host: Clone + Send + 'static {
    /// Workspace in cui e' aperto il tab (None se il tab non ha un pty vivo).
    fn tab_workspace(&self, tab_id: &str) -> Option<Workspace>;
    /// La cartella esiste nell'ambiente del workspace?
    fn dir_exists(&self, ws: &Workspace, path: &str) -> bool;
    fn launch_command(&self, agent: &str, env_kind: &str, req: &AutomationRequest) -> Result<String, String>;
    /// File settings con le regole di permesso pre-approvate/vietate del
    /// workspace per l'agente `agent` (solo Claude Code), nel formato di path
    /// del suo ambiente. `None` = nessuna regola o agente senza supporto.
    fn worker_settings_path(&self, _ws: &Workspace, _agent: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn emit_spawn(&self, ev: &SpawnEvent);
    fn emit_close(&self, tab_id: &str);
    fn emit_update(&self, info: &AutomationTabInfo);
    /// Esegue `job` dopo `delay` su un altro thread.
    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>);
    /// Workspace per id.
    fn workspace(&self, id: &str) -> Option<Workspace>;
    /// Esegue `git <args>` in `dir` nell'ambiente del workspace (argv, mai
    /// una shell). `Err` solo se git non parte.
    fn git(&self, ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String>;
    /// Aggiunge `line` al file `file` (info/exclude) se manca.
    fn append_exclude(&self, ws: &Workspace, file: &str, line: &str) -> Result<(), String>;
    /// Repository del workspace: "." se la root e' un repo, piu' le
    /// sottocartelle dirette con un `.git`.
    fn list_repos(&self, ws: &Workspace) -> Result<Vec<String>, String>;
    /// Il percorso (file o cartella) esiste nell'ambiente del workspace?
    fn path_exists(&self, ws: &Workspace, path: &str) -> bool;
    /// Copia ricorsiva `src` -> `dst` (file o cartella), creando le cartelle padre.
    fn copy_path(&self, ws: &Workspace, src: &str, dst: &str) -> Result<(), String>;
    /// Esegue un comando dell'utente (setup / pulizia) con la shell della
    /// piattaforma, cwd `dir`, `env` in piu'. `Err` solo se non parte o scade.
    fn run_hook(
        &self,
        ws: &Workspace,
        dir: &str,
        command: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<GitOut, String>;
    fn save_board(&self, board: &Board) -> Result<(), String>;
    fn delete_board(&self, ws_id: &str);
    fn emit_board(&self, view: &BoardView);
    /// Notifica di sistema (evento `task-notify`): il frontend la mostra.
    fn emit_notice(&self, notice: &Notice);
    /// Tutti i workspace.
    fn workspaces(&self) -> Vec<Workspace>;
    /// Minuti locali del giorno all'istante `now` (unix ms).
    fn local_minute(&self, now: i64) -> u16;
    fn emit_schedule(&self, state: &ScheduleState);
    fn save_schedule(&self, s: &Schedule) -> Result<(), String>;
    /// Prende o rilascia il blocco dello standby.
    fn set_keep_awake(&self, on: bool);
}

/// Timer di `Host::defer`: un solo thread con coda a priorita' al posto di un
/// thread per timer. Il job gira su un thread del pool bloccante (puo' fare
/// git e I/O) cosi' non ritarda gli altri timer.
mod timer {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    struct Entry {
        at: Instant,
        seq: u64,
        job: Box<dyn FnOnce() + Send>,
    }

    // BinaryHeap e' un max-heap: ordine invertito, scade prima il piu' vicino.
    impl Ord for Entry {
        fn cmp(&self, other: &Self) -> Ordering {
            (other.at, other.seq).cmp(&(self.at, self.seq))
        }
    }
    impl PartialOrd for Entry {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl PartialEq for Entry {
        fn eq(&self, other: &Self) -> bool {
            (self.at, self.seq) == (other.at, other.seq)
        }
    }
    impl Eq for Entry {}

    #[derive(Default)]
    struct Queue {
        heap: BinaryHeap<Entry>,
        seq: u64,
    }

    struct Timer {
        queue: Mutex<Queue>,
        wake: Condvar,
    }

    static TIMER: OnceLock<&'static Timer> = OnceLock::new();

    fn timer() -> &'static Timer {
        TIMER.get_or_init(|| {
            let t: &'static Timer = Box::leak(Box::new(Timer { queue: Mutex::new(Queue::default()), wake: Condvar::new() }));
            std::thread::Builder::new()
                .name("automation-timer".to_string())
                .spawn(move || t.run())
                .expect("thread timer automazione");
            t
        })
    }

    impl Timer {
        fn run(&self) {
            let mut q = self.queue.lock().unwrap();
            loop {
                let now = Instant::now();
                match q.heap.peek().map(|e| e.at) {
                    None => q = self.wake.wait(q).unwrap(),
                    Some(at) if at > now => q = self.wake.wait_timeout(q, at - now).unwrap().0,
                    Some(_) => {
                        let job = q.heap.pop().unwrap().job;
                        drop(q);
                        tauri::async_runtime::spawn_blocking(job);
                        q = self.queue.lock().unwrap();
                    }
                }
            }
        }
    }

    pub(super) fn schedule(delay: Duration, job: Box<dyn FnOnce() + Send>) {
        let t = timer();
        let mut q = t.queue.lock().unwrap();
        q.seq += 1;
        let seq = q.seq;
        q.heap.push(Entry { at: Instant::now() + delay, seq, job });
        drop(q);
        t.wake.notify_one();
    }
}

#[derive(Clone)]
pub struct TauriHost(pub AppHandle);

impl Host for TauriHost {
    fn tab_workspace(&self, tab_id: &str) -> Option<Workspace> {
        let id = crate::pty::workspace_of_tab(&self.0, tab_id)?;
        self.0.state::<WorkspaceStore>().find(&id)
    }

    fn dir_exists(&self, ws: &Workspace, path: &str) -> bool {
        if ws.kind == "wsl" {
            crate::wsl::wsl_dir_exists(ws.distro.as_deref().unwrap_or(""), path)
        } else {
            std::path::Path::new(path).is_dir()
        }
    }

    fn launch_command(&self, agent: &str, env_kind: &str, req: &AutomationRequest) -> Result<String, String> {
        agents::automation_command(agent, env_kind, req)
    }

    fn worker_settings_path(&self, ws: &Workspace, agent: &str) -> Result<Option<String>, String> {
        // Solo Claude Code sa leggere `--settings`.
        if agent != DEFAULT_AGENT {
            return Ok(None);
        }
        let data_dir = auto::data_dir(&self.0)?;
        crate::worker_permissions::prepare_settings_file(&data_dir, &ws.id, &ws.kind, &ws.worker_allow, &ws.worker_deny)
    }

    fn emit_spawn(&self, ev: &SpawnEvent) {
        let _ = self.0.emit(SPAWN_EVENT, ev);
    }

    fn emit_close(&self, tab_id: &str) {
        let _ = self.0.emit(CLOSE_EVENT, serde_json::json!({ "tab_id": tab_id }));
    }

    fn emit_update(&self, info: &AutomationTabInfo) {
        let _ = self.0.emit(UPDATE_EVENT, info);
    }

    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
        timer::schedule(delay, job);
    }

    fn workspace(&self, id: &str) -> Option<Workspace> {
        self.0.state::<WorkspaceStore>().find(id)
    }

    fn git(&self, ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
        worktrees::run_git(ws, dir, args)
    }

    fn append_exclude(&self, ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
        worktrees::append_exclude_line(ws, file, line)
    }

    fn list_repos(&self, ws: &Workspace) -> Result<Vec<String>, String> {
        worktrees::detect_repos(ws)
    }

    fn path_exists(&self, ws: &Workspace, path: &str) -> bool {
        hooks::path_exists_real(ws, path)
    }

    fn copy_path(&self, ws: &Workspace, src: &str, dst: &str) -> Result<(), String> {
        hooks::copy_path_real(ws, src, dst)
    }

    fn run_hook(
        &self,
        ws: &Workspace,
        dir: &str,
        command: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<GitOut, String> {
        hooks::run_hook_real(ws, dir, command, env, timeout)
    }

    fn save_board(&self, board: &Board) -> Result<(), String> {
        let dir = worktrees::boards_dir(&self.0)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("impossibile creare cartella board: {e}"))?;
        let raw = serde_json::to_string_pretty(board).map_err(|e| e.to_string())?;
        let file = worktrees::board_file(&dir, &board.workspace_id)?;
        crate::write_atomic(&file, &raw).map_err(|e| format!("impossibile salvare la board: {e}"))
    }

    fn delete_board(&self, ws_id: &str) {
        if let Ok(dir) = worktrees::boards_dir(&self.0) {
            if let Ok(file) = worktrees::board_file(&dir, ws_id) {
                let _ = std::fs::remove_file(file);
            }
        }
    }

    fn emit_board(&self, view: &BoardView) {
        let _ = self.0.emit(BOARD_EVENT, view);
    }

    fn emit_notice(&self, notice: &Notice) {
        let _ = self.0.emit(NOTICE_EVENT, notice);
    }

    fn workspaces(&self) -> Vec<Workspace> {
        self.0.state::<WorkspaceStore>().all()
    }

    fn local_minute(&self, now: i64) -> u16 {
        use chrono::{Local, TimeZone, Timelike};
        Local
            .timestamp_millis_opt(now)
            .single()
            .map(|t| (t.hour() * 60 + t.minute()) as u16)
            .unwrap_or(0)
    }

    fn emit_schedule(&self, state: &ScheduleState) {
        let _ = self.0.emit(SCHEDULE_EVENT, state);
    }

    fn save_schedule(&self, s: &Schedule) -> Result<(), String> {
        crate::schedule::save_schedule(&auto::data_dir(&self.0)?, s)
    }

    fn set_keep_awake(&self, on: bool) {
        crate::awake::set(on);
    }
}
