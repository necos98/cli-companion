//! Test del Task Manager con un host finto: i tool MCP passano da `dispatch`,
//! i comandi della UI da `boards::*`, i timer dell'host girano in linea (quindi
//! un task scelto dallo scheduler e' gia' avviato quando `run` ritorna).

use super::*;
use crate::agents::AutomationRequest;
use crate::schedule::{Schedule, ScheduleState};
use crate::workspaces::{DeliveryMode, MergeMethod, RepoConfig};
use serde_json::json;

mod board_flow;
mod schedule;
mod session_end;
mod worker;

/// Tab dell'Intake (una sessione normale del workspace "ws-1").
const INTAKE: &str = "tab-intake";

pub(super) fn ws(id: &str, name: &str, kind: &str) -> Workspace {
    Workspace {
        id: id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        path: if kind == "wsl" { "/home/u".to_string() } else { "C:\\p".to_string() },
        distro: (kind == "wsl").then(|| "Ubuntu".to_string()),
        shell: None,
        created_at: 0,
        launches: Vec::new(),
        worker_permission: "accept-edits".to_string(),
        worker_model: None,
        worker_allow: Vec::new(),
        worker_deny: Vec::new(),
        delivery: Delivery::default(),
        use_schedule: false,
        default_launch: None,
        worker_rules: String::new(),
        repos: Default::default(),
        archived: false,
    }
}

#[derive(Clone, Default)]
struct FakeHost {
    log: Arc<Mutex<Vec<String>>>,
    /// "<dir>|<args>" di ogni git eseguito.
    git_log: Arc<Mutex<Vec<String>>>,
    /// Risposte git forzate: la prima il cui testo e' contenuto in "<dir>|<args>".
    git_rules: Arc<Mutex<Vec<(String, GitOut)>>>,
    /// Cartelle che "esistono" oltre a quelle con "esiste" nel nome.
    dirs: Arc<Mutex<Vec<String>>>,
    spawns: Arc<Mutex<Vec<SpawnEvent>>>,
    /// Board salvate (ultima per workspace) e viste emesse.
    saved: Arc<Mutex<HashMap<String, Board>>>,
    board_events: Arc<Mutex<Vec<BoardView>>>,
    deleted: Arc<Mutex<Vec<String>>>,
    excludes: Arc<Mutex<Vec<String>>>,
    notices: Arc<Mutex<Vec<Notice>>>,
    /// Minuti locali "correnti" dell'host finto.
    minute: Arc<Mutex<u16>>,
    schedule_events: Arc<Mutex<Vec<ScheduleState>>>,
    saved_schedule: Arc<Mutex<Option<Schedule>>>,
    awake: Arc<Mutex<Vec<bool>>>,
    /// Impostazioni del workspace "ws-1".
    delivery: Arc<Mutex<Delivery>>,
    use_schedule: Arc<Mutex<bool>>,
    archived: Arc<Mutex<bool>>,
    worker_model: Arc<Mutex<Option<String>>>,
    worker_rules: Arc<Mutex<String>>,
    repo_cfg: Arc<Mutex<std::collections::BTreeMap<String, RepoConfig>>>,
    /// Branch corrente dei checkout (`rev-parse --abbrev-ref HEAD`).
    branch: Arc<Mutex<Option<String>>>,
    repos: Arc<Mutex<Vec<String>>>,
    /// "<dir>|<comando>|<ENV=V,...>" di ogni hook eseguito, e copie "src->dst".
    hook_log: Arc<Mutex<Vec<String>>>,
    /// Esiti forzati degli hook: il primo il cui testo e' contenuto in "<dir>|<comando>".
    hook_rules: Arc<Mutex<Vec<(String, GitOut)>>>,
    /// Percorsi che "esistono" per `path_exists` (copie dei file ignorati).
    files: Arc<Mutex<Vec<String>>>,
    /// Errore forzato di `append_exclude`.
    exclude_err: Arc<Mutex<Option<String>>>,
}

impl FakeHost {
    fn with_delivery(mode: DeliveryMode, parallel: bool) -> FakeHost {
        let h = FakeHost::default();
        h.set_delivery(mode, parallel);
        h
    }
    fn set_delivery(&self, mode: DeliveryMode, parallel: bool) {
        *self.delivery.lock().unwrap() = Delivery { mode, parallel, merge_method: MergeMethod::Squash };
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn git_log(&self) -> Vec<String> {
        self.git_log.lock().unwrap().clone()
    }
    fn git_rule(&self, needle: &str, ok: bool, stdout: &str, stderr: &str) {
        let out = GitOut { ok, stdout: stdout.to_string(), stderr: stderr.to_string() };
        self.git_rules.lock().unwrap().push((needle.to_string(), out));
    }
    fn spawns(&self) -> Vec<SpawnEvent> {
        self.spawns.lock().unwrap().clone()
    }
    fn notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap().clone()
    }
    fn ws_for(&self, id: &str) -> Option<Workspace> {
        match id {
            "ws-1" => {
                let mut w = ws("ws-1", "Proj", "windows");
                w.delivery = *self.delivery.lock().unwrap();
                w.use_schedule = *self.use_schedule.lock().unwrap();
                w.worker_model = self.worker_model.lock().unwrap().clone();
                w.worker_rules = self.worker_rules.lock().unwrap().clone();
                w.archived = *self.archived.lock().unwrap();
                w.repos = self.repo_cfg.lock().unwrap().clone();
                Some(w)
            }
            "ws-2" => Some(ws("ws-2", "Linux", "wsl")),
            _ => None,
        }
    }
}

impl Host for FakeHost {
    fn tab_workspace(&self, tab_id: &str) -> Option<Workspace> {
        match tab_id {
            INTAKE | "tab-other" => self.ws_for("ws-1"),
            "tab-wsl" => self.ws_for("ws-2"),
            t if t.starts_with("auto-") => self.ws_for("ws-1"),
            _ => None,
        }
    }
    fn dir_exists(&self, _ws: &Workspace, path: &str) -> bool {
        path.contains("esiste") || self.dirs.lock().unwrap().iter().any(|d| d == path)
    }
    fn launch_command(&self, agent: &str, env_kind: &str, req: &AutomationRequest) -> Result<String, String> {
        if agent != "claude-code" {
            return Err(format!("unknown agent: {agent}"));
        }
        Ok(format!("fake {env_kind} {:?} {} model={}", req.permission_mode, req.bootstrap_prompt, req.model.unwrap_or("-")))
    }
    fn emit_spawn(&self, ev: &SpawnEvent) {
        self.spawns.lock().unwrap().push(ev.clone());
        self.log.lock().unwrap().push(format!("spawn:{}:{}", ev.tab_id, ev.command));
    }
    fn emit_close(&self, tab_id: &str) {
        self.log.lock().unwrap().push(format!("close:{tab_id}"));
    }
    fn emit_update(&self, info: &AutomationTabInfo) {
        self.log.lock().unwrap().push(format!("update:{}:{:?}", info.tab_id, info.status));
    }
    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
        self.log.lock().unwrap().push(format!("defer:{}", delay.as_millis()));
        job();
    }
    fn workspace(&self, id: &str) -> Option<Workspace> {
        self.ws_for(id)
    }
    fn git(&self, _ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
        let line = format!("{dir}|{}", args.join(" "));
        self.git_log.lock().unwrap().push(line.clone());
        if let Some((_, out)) = self.git_rules.lock().unwrap().iter().find(|(n, _)| line.contains(n.as_str())) {
            return Ok(out.clone());
        }
        let ok = |s: &str| GitOut { ok: true, stdout: s.to_string(), stderr: String::new() };
        let fail = || GitOut { ok: false, stdout: String::new(), stderr: String::new() };
        let a = args.join(" ");
        Ok(match a.as_str() {
            "rev-parse --is-inside-work-tree" => ok("true\n"),
            "rev-parse --abbrev-ref HEAD" => ok(&format!("{}\n", self.branch.lock().unwrap().clone().unwrap_or_else(|| "main".into()))),
            "rev-parse --git-path info/exclude" => ok(".git/info/exclude\n"),
            "symbolic-ref --short refs/remotes/origin/HEAD" => fail(),
            s if s.starts_with("rev-parse --verify") => fail(),
            s if s.starts_with("remote get-url") => fail(),
            s if s.starts_with("diff --stat") => ok(" 2 files changed, 10 insertions(+)\n"),
            _ => ok(""),
        })
    }
    fn append_exclude(&self, _ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
        if let Some(e) = self.exclude_err.lock().unwrap().clone() {
            return Err(e);
        }
        self.excludes.lock().unwrap().push(format!("{file}|{line}"));
        Ok(())
    }
    fn list_repos(&self, _ws: &Workspace) -> Result<Vec<String>, String> {
        Ok(self.repos.lock().unwrap().clone())
    }
    fn path_exists(&self, _ws: &Workspace, path: &str) -> bool {
        self.files.lock().unwrap().iter().any(|f| f == path)
    }
    fn copy_path(&self, _ws: &Workspace, src: &str, dst: &str) -> Result<(), String> {
        self.hook_log.lock().unwrap().push(format!("copy {src}->{dst}"));
        Ok(())
    }
    fn run_hook(&self, _ws: &Workspace, dir: &str, command: &str, env: &[(String, String)], _timeout: Duration) -> Result<GitOut, String> {
        let vars: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let line = format!("{dir}|{command}");
        self.hook_log.lock().unwrap().push(format!("{line}|{}", vars.join(",")));
        // Anche nel log di git: serve a provare l'ordine rispetto ai comandi git.
        self.git_log.lock().unwrap().push(format!("hook|{command}"));
        if let Some((_, out)) = self.hook_rules.lock().unwrap().iter().find(|(n, _)| line.contains(n.as_str())) {
            return Ok(out.clone());
        }
        Ok(GitOut { ok: true, stdout: String::new(), stderr: String::new() })
    }
    fn save_board(&self, board: &Board) -> Result<(), String> {
        self.saved.lock().unwrap().insert(board.workspace_id.clone(), board.clone());
        Ok(())
    }
    fn delete_board(&self, ws_id: &str) {
        self.deleted.lock().unwrap().push(ws_id.to_string());
        self.saved.lock().unwrap().remove(ws_id);
    }
    fn emit_board(&self, view: &BoardView) {
        self.board_events.lock().unwrap().push(view.clone());
    }
    fn emit_notice(&self, notice: &Notice) {
        self.notices.lock().unwrap().push(notice.clone());
    }
    fn workspaces(&self) -> Vec<Workspace> {
        ["ws-1", "ws-2"].iter().filter_map(|id| self.ws_for(id)).collect()
    }
    fn local_minute(&self, _now: i64) -> u16 {
        *self.minute.lock().unwrap()
    }
    fn emit_schedule(&self, state: &ScheduleState) {
        self.schedule_events.lock().unwrap().push(state.clone());
    }
    fn save_schedule(&self, s: &Schedule) -> Result<(), String> {
        *self.saved_schedule.lock().unwrap() = Some(s.clone());
        Ok(())
    }
    fn set_keep_awake(&self, on: bool) {
        self.awake.lock().unwrap().push(on);
    }
}

// --- helper comuni ---

fn call(st: &AutomationState, h: &FakeHost, session: &str, method: &str, params: Value) -> Result<Value, String> {
    rpc::dispatch(st, h, session, method, &params)
}

fn spec(id: &str, deps: &[&str]) -> Value {
    json!({"id": id, "title": format!("Task {id}"), "task": format!("do {id}"), "depends_on": deps, "model": "sonnet"})
}

/// Propone e accetta (come l'utente) i task: finiscono in coda e, se lo
/// scheduler li sceglie, partono.
fn queue(st: &AutomationState, h: &FakeHost, tasks: Vec<Value>) {
    let r = call(st, h, INTAKE, "propose_tasks", json!({"tasks": tasks})).unwrap();
    boards::proposal_decide(st, h, "ws-1", r["proposal"].as_str().unwrap(), true).unwrap();
}

fn view(st: &AutomationState, h: &FakeHost) -> BoardView {
    st.view(h, "ws-1").unwrap()
}

/// Task come lo vede la UI (JSON della vista).
fn task(st: &AutomationState, h: &FakeHost, id: &str) -> Value {
    view(st, h).tasks.iter().find(|t| t.task.id == id).map(|t| t.to_json(true)).unwrap_or(Value::Null)
}

fn session_of(st: &AutomationState, h: &FakeHost, id: &str) -> String {
    task(st, h, id)["session"].as_str().expect("task con worker").to_string()
}

/// Hook semplice (senza dati dello Stop hook).
fn hook(st: &AutomationState, h: &FakeHost, tab: &str, event: &str) -> Option<String> {
    st.run(h, |r| r.on_hook(tab, &HookEvent { event, ..Default::default() }, 1))
}

/// Stop hook di un worker con i dati di Claude Code.
fn stop(st: &AutomationState, h: &FakeHost, tab: &str, active: Option<bool>, background: Option<u32>) -> Option<String> {
    let ev = HookEvent { event: "finished", stop_hook_active: active, background_work: background, ..Default::default() };
    st.run(h, |r| r.on_hook(tab, &ev, 1))
}

/// Una board con il task `a` gia' in corso; ritorna la sessione del worker.
fn running(mode: DeliveryMode, parallel: bool) -> (AutomationState, FakeHost, String) {
    let (st, h) = (AutomationState::default(), FakeHost::with_delivery(mode, parallel));
    queue(&st, &h, vec![spec("a", &[])]);
    let w = session_of(&st, &h, "a");
    (st, h, w)
}

fn tab_closed(st: &AutomationState, h: &FakeHost, tab: &str) {
    st.run(h, |r| r.tab_closed(tab, 1));
}

fn finish(st: &AutomationState, h: &FakeHost, session: &str, status: &str) {
    call(st, h, session, "get_task", json!({})).unwrap();
    call(st, h, session, "complete_task", json!({"status": status, "summary": "fatto"})).unwrap();
}
