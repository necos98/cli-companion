//! Test del Task Manager con un host finto: i tool MCP passano da `dispatch`,
//! i comandi della UI da `boards::*`, i timer dell'host girano in linea (quindi
//! un task scelto dallo scheduler e' gia' avviato quando `run` ritorna).

use super::process::CancelFlag;
use super::*;
use crate::agents::{AutomationLaunch, AutomationRequest, AutomationStart};
use crate::bots::AgentBot;
use crate::platform::TargetKind;
use crate::schedule::{Schedule, ScheduleState};
use crate::workspaces::{AgentWorkerSettings, DeliveryMode, MergeMethod, RepoConfig, SettingsPatch};
use serde_json::json;

mod agent_ready;
mod archive;
mod auth;
mod auto_fix;
mod board_flow;
mod bot_kinds;
mod bot_settings;
mod bot_tasks;
mod bot_worker;
mod deliver;
mod deliver_flow;
mod e2e;
mod e2e_cancel;
mod e2e_manual;
mod free_mode;
mod goal_branch;
mod goals;
mod handoff;
mod hub_hook;
mod plan;
mod questions;
mod ready;
mod reconcile;
mod reconcile_git;
mod review;
mod roles;
mod root_plan;
mod schedule;
mod second_agent;
mod session_end;
mod settings;
mod skills;
mod task_agent;
mod task_update_check;
mod worker;
mod worker_model;
mod worker_state;

/// Tab dell'Intake (una sessione normale del workspace "ws-1").
const INTAKE: &str = "tab-intake";
/// La voce del solo Bot abilitato dell'host finto.
pub(super) const FAKE_BOT_LAUNCH: &str = "bot:intake";

pub(super) fn ws(id: &str, name: &str, kind: &str) -> Workspace {
    let kind = TargetKind::parse(kind).expect("kind del workspace di test");
    let wsl = kind == TargetKind::Wsl;
    let path = if wsl { "/home/u" } else { "C:\\p" };
    let mut workspace = Workspace::new(id.to_string(), name.to_string(), kind, path.to_string());
    workspace.distro = wsl.then(|| "Ubuntu".to_string());
    workspace
}

/// Ambiente e cartella (o errore) di una cartella target delle skill.
type SkillDir = (String, Result<std::path::PathBuf, String>);

/// Variabili d'ambiente preparate per un tab: (tab, variabili).
type StagedEnv = (String, Vec<(String, String)>);

/// Scrittura in un terminale: (tab, byte).
type TabWrite = (String, Vec<u8>);

/// Lavoro differito di `Host::defer`.
type Job = Box<dyn FnOnce() + Send>;

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
    /// Variabili d'ambiente preparate per il PTY di ogni tab (tab, variabili).
    staged: Arc<Mutex<Vec<StagedEnv>>>,
    /// Board salvate (ultima per workspace) e viste emesse.
    saved: Arc<Mutex<HashMap<String, Board>>>,
    board_events: Arc<Mutex<Vec<BoardView>>>,
    /// Revisioni passate a `board_published`, in ordine (per workspace e revisione).
    published: Arc<Mutex<Vec<(String, u64)>>>,
    deleted: Arc<Mutex<Vec<String>>>,
    /// Task archiviati (workspace, task) e l'errore forzato di `archive_tasks`.
    archive_log: Arc<Mutex<Vec<(String, Task)>>>,
    /// Goal archiviati (workspace, goal); `archive_err` vale anche per loro.
    goal_archive_log: Arc<Mutex<Vec<(String, Goal)>>>,
    archive_err: Arc<Mutex<Option<String>>>,
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
    worker_agents: Arc<Mutex<std::collections::BTreeMap<String, AgentWorkerSettings>>>,
    worker_rules: Arc<Mutex<String>>,
    repo_cfg: Arc<Mutex<std::collections::BTreeMap<String, RepoConfig>>>,
    /// Branch corrente dei checkout (`rev-parse --abbrev-ref HEAD`).
    branch: Arc<Mutex<Option<String>>>,
    repos: Arc<Mutex<Vec<String>>>,
    /// Errore forzato di `list_repos`.
    repos_err: Arc<Mutex<Option<String>>>,
    /// Le impostazioni di "ws-1" che sopra non hanno un campo proprio
    /// (avvii, permessi, regole): `None` = quelle di `ws`.
    ws1_base: Arc<Mutex<Option<Workspace>>>,
    /// Chiamate a `apply_settings` e relativo errore forzato.
    applies: Arc<Mutex<u32>>,
    apply_err: Arc<Mutex<Option<String>>>,
    /// "<dir>|<comando>|<ENV=V,...>" di ogni hook eseguito, e copie "src->dst".
    hook_log: Arc<Mutex<Vec<String>>>,
    /// Esiti forzati degli hook: il primo il cui testo e' contenuto in "<dir>|<comando>".
    hook_rules: Arc<Mutex<Vec<(String, GitOut)>>>,
    /// Percorsi che "esistono" per `path_exists` (copie dei file ignorati).
    files: Arc<Mutex<Vec<String>>>,
    /// Job di `defer` messi in coda invece che eseguiti in linea (`None` = in linea).
    held: Arc<Mutex<Option<Vec<Job>>>>,
    /// File scritti da un hook quando parte: (testo del comando, percorso, contenuto).
    hook_writes: Arc<Mutex<Vec<(String, String, String)>>>,
    /// Eventi `e2e-update` emessi.
    e2e_events: Arc<Mutex<Vec<crate::automation::e2e::E2eEvent>>>,
    /// File di testo per `read_text_file` / `remove_file` (percorso -> contenuto).
    text_files: Arc<Mutex<HashMap<String, String>>>,
    /// Errore forzato di `append_exclude`.
    exclude_err: Arc<Mutex<Option<String>>>,
    /// Archivio delle skill utente e cartelle target (ambiente, cartella).
    skills_store: Arc<Mutex<Option<std::path::PathBuf>>>,
    skill_dirs: Arc<Mutex<Vec<SkillDir>>>,
    /// Con `true` `git` e `dir_exists` usano la macchina vera (vedi `reconcile_git`).
    real_git: Arc<std::sync::atomic::AtomicBool>,
    /// Prontezza forzata per agente (`None` = ancora in controllo); gli altri sono pronti.
    readiness: Arc<Mutex<HashMap<String, Option<Readiness>>>>,
    /// Agenti di cui `probe_agent` e' stato chiesto.
    probed: Arc<Mutex<Vec<String>>>,
    /// Agents HUB acceso (spento di default).
    hub: Arc<std::sync::atomic::AtomicBool>,
    /// Scritture nei terminali (tab, byte) e interruttore che le fa fallire.
    tab_writes: Arc<Mutex<Vec<TabWrite>>>,
    write_fails: Arc<std::sync::atomic::AtomicBool>,
    /// I Bot-agente (di default i predefiniti).
    bots: FakeBots,
}

/// I Bot dell'host finto: parte dai predefiniti, ognuno abilitato a ogni tipo
/// di task (cosi' i test scelgono il Bot per agente, modello, effort e skill);
/// i test li modificano. I tipi dei predefiniti veri li prova `bot_kinds`.
#[derive(Clone)]
struct FakeBots(Arc<Mutex<Vec<AgentBot>>>);

impl Default for FakeBots {
    fn default() -> Self {
        let mut bots = crate::bots::default_agent_bots();
        bots.iter_mut().for_each(|b| b.kinds = TaskKind::ALL.to_vec());
        FakeBots(Arc::new(Mutex::new(bots)))
    }
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
    /// Il log di git senza il rilevamento "la cartella e' un repository?", che
    /// l'app fa a ogni creazione e avvio, qualunque sia la consegna.
    fn git_log_without_detection(&self) -> Vec<String> {
        let mut log = self.git_log();
        log.retain(|l| !l.contains("rev-parse --is-inside-work-tree"));
        log
    }
    fn git_rule(&self, needle: &str, ok: bool, stdout: &str, stderr: &str) {
        let out = GitOut { ok, stdout: stdout.to_string(), stderr: stderr.to_string() };
        self.git_rules.lock().unwrap().push((needle.to_string(), out));
    }
    /// Da ora `goal/<goal>` "esiste" nei repository (il git finto non tiene i branch).
    fn goal_branch_exists(&self, goal: &str) {
        self.git_rule(&format!("rev-parse --verify --quiet refs/heads/goal/{goal}"), true, "", "");
    }
    /// Variabili preparate per il tab `tab_id` (vuote se nessuna).
    fn staged_env(&self, tab_id: &str) -> Vec<(String, String)> {
        let staged = self.staged.lock().unwrap();
        staged.iter().filter(|(t, _)| t == tab_id).flat_map(|(_, env)| env.clone()).collect()
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
                let mut w = self.ws1_base.lock().unwrap().clone().unwrap_or_else(|| ws("ws-1", "Proj", "windows"));
                w.delivery = *self.delivery.lock().unwrap();
                w.use_schedule = *self.use_schedule.lock().unwrap();
                w.worker.agents = self.worker_agents.lock().unwrap().clone();
                w.worker.rules = self.worker_rules.lock().unwrap().clone();
                w.archived = *self.archived.lock().unwrap();
                w.repos = self.repo_cfg.lock().unwrap().clone();
                Some(w)
            }
            "ws-2" => Some(ws("ws-2", "Linux", "wsl")),
            _ => None,
        }
    }
    /// Salva `w` come nuovo "ws-1": i campi con un loro stato e il resto.
    fn store_ws1(&self, w: Workspace) {
        *self.delivery.lock().unwrap() = w.delivery;
        *self.use_schedule.lock().unwrap() = w.use_schedule;
        *self.worker_agents.lock().unwrap() = w.worker.agents.clone();
        *self.worker_rules.lock().unwrap() = w.worker.rules.clone();
        *self.repo_cfg.lock().unwrap() = w.repos.clone();
        *self.ws1_base.lock().unwrap() = Some(w);
    }
    fn set_text_file(&self, path: &str, content: &str) {
        self.text_files.lock().unwrap().insert(path.to_string(), content.to_string());
    }
    fn text_file(&self, path: &str) -> Option<String> {
        self.text_files.lock().unwrap().get(path).cloned()
    }
    fn applies(&self) -> u32 {
        *self.applies.lock().unwrap()
    }
    /// Da ora i job di `defer` restano in coda (vedi `run_held`).
    fn hold_defers(&self) {
        *self.held.lock().unwrap() = Some(Vec::new());
    }
    /// Esegue i job in coda e torna all'esecuzione in linea.
    fn run_held(&self) {
        let jobs = self.held.lock().unwrap().take().unwrap_or_default();
        jobs.into_iter().for_each(|job| job());
    }
    fn e2e_events(&self) -> Vec<crate::automation::e2e::E2eEvent> {
        self.e2e_events.lock().unwrap().clone()
    }
    /// Da ora git e le cartelle sono quelli veri della macchina.
    fn use_real_git(&self) {
        self.real_git.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn uses_real_git(&self) -> bool {
        self.real_git.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// Da ora `agent` risulta in questo stato per lo scheduler.
    fn set_readiness(&self, agent: &str, state: Option<Readiness>) {
        self.readiness.lock().unwrap().insert(agent.to_string(), state);
    }
    fn probed(&self) -> Vec<String> {
        self.probed.lock().unwrap().clone()
    }
    fn set_hub(&self, on: bool) {
        self.hub.store(on, std::sync::atomic::Ordering::SeqCst);
    }
    /// Da ora le scritture nei terminali falliscono.
    fn fail_tab_writes(&self) {
        self.write_fails.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Scritture riuscite nei terminali, in ordine.
    fn tab_writes(&self) -> Vec<TabWrite> {
        self.tab_writes.lock().unwrap().clone()
    }
    /// Modifica i Bot dell'host (aggiungerne, disabilitarli, cambiarli).
    fn edit_bots(&self, f: impl FnOnce(&mut Vec<AgentBot>)) {
        f(&mut self.bots.0.lock().unwrap());
    }
    /// Aggiunge un Bot generico con l'agente fissato e ne ritorna l'id: per i
    /// task di una proposta che devono girare con un agente preciso.
    fn bot_for_agent(&self, agent: &str) -> String {
        let id = format!("pinned-{agent}");
        self.edit_bots(|bots| {
            let general = bots[0].clone();
            bots.push(AgentBot {
                id: id.clone(),
                name: format!("Pinned {agent}"),
                agent: Some(agent.into()),
                ..general
            });
        });
        id
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
        if self.uses_real_git() {
            return std::path::Path::new(path).is_dir();
        }
        path.contains("esiste") || self.dirs.lock().unwrap().iter().any(|d| d == path)
    }
    /// Le verifiche del comando vero (agente del registro, abilitato e capace,
    /// modello suo), poi una riga leggibile con l'agente che parte.
    fn launch_command(
        &self,
        agent: &str,
        env_kind: TargetKind,
        req: &AutomationRequest,
    ) -> Result<AutomationStart, String> {
        let start = crate::agents::automation_command(agent, env_kind, req)?;
        let command = format!(
            "fake {agent} {} {:?} {} effort={} model={}",
            env_kind.as_str(),
            req.permission_mode,
            req.bootstrap_prompt,
            // L'effort resta se `automation_command` non lo ha tolto (unico avviso possibile).
            req.effort.filter(|_| start.warnings.is_empty()).map_or("-", crate::agents::Effort::as_str),
            req.model.unwrap_or("-")
        );
        // Le variabili sono quelle vere dell'integrazione.
        Ok(AutomationStart { launch: AutomationLaunch { command, env: start.launch.env }, warnings: start.warnings })
    }
    fn stage_env(&self, tab_id: &str, env: Vec<(String, String)>) {
        self.staged.lock().unwrap().push((tab_id.to_string(), env));
    }
    fn emit_spawn(&self, ev: &SpawnEvent) {
        self.spawns.lock().unwrap().push(ev.clone());
        self.log.lock().unwrap().push(format!("spawn:{}:{}", ev.tab_id, ev.command));
    }
    fn hub_enabled(&self) -> bool {
        self.hub.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn write_to_tab(&self, tab_id: &str, data: &[u8]) -> Result<(), String> {
        if self.write_fails.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("sessione chiusa".to_string());
        }
        self.tab_writes.lock().unwrap().push((tab_id.to_string(), data.to_vec()));
        Ok(())
    }
    fn emit_close(&self, tab_id: &str) {
        self.log.lock().unwrap().push(format!("close:{tab_id}"));
    }
    fn emit_update(&self, info: &AutomationTabInfo) {
        self.log.lock().unwrap().push(format!("update:{}:{:?}", info.tab_id, info.status));
    }
    fn defer(&self, delay: Duration, job: Box<dyn FnOnce() + Send>) {
        self.log.lock().unwrap().push(format!("defer:{}", delay.as_millis()));
        if let Some(queue) = self.held.lock().unwrap().as_mut() {
            queue.push(job);
            return;
        }
        job();
    }
    fn workspace(&self, id: &str) -> Option<Workspace> {
        self.ws_for(id)
    }
    fn git(&self, ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
        if self.uses_real_git() {
            return worktrees::run_git(ws, dir, args);
        }
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
            "rev-parse --abbrev-ref HEAD" => {
                ok(&format!("{}\n", self.branch.lock().unwrap().clone().unwrap_or_else(|| "main".into())))
            }
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
        if let Some(e) = self.repos_err.lock().unwrap().clone() {
            return Err(e);
        }
        Ok(self.repos.lock().unwrap().clone())
    }
    fn path_exists(&self, _ws: &Workspace, path: &str) -> bool {
        self.files.lock().unwrap().iter().any(|f| f == path)
    }
    fn copy_path(&self, _ws: &Workspace, src: &str, dst: &str) -> Result<(), String> {
        self.hook_log.lock().unwrap().push(format!("copy {src}->{dst}"));
        Ok(())
    }
    fn run_hook(
        &self,
        _ws: &Workspace,
        dir: &str,
        command: &str,
        env: &[(String, String)],
        _timeout: Duration,
        cancel: Option<&CancelFlag>,
    ) -> Result<GitOut, String> {
        let vars: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let line = format!("{dir}|{command}");
        self.hook_log.lock().unwrap().push(format!("{line}|{}", vars.join(",")));
        if cancel.is_some_and(CancelFlag::is_cancelled) {
            return Err("cancelled".to_string());
        }
        for (_, path, content) in self.hook_writes.lock().unwrap().iter().filter(|(n, ..)| line.contains(n.as_str())) {
            self.text_files.lock().unwrap().insert(path.clone(), content.clone());
        }
        // Anche nel log di git: serve a provare l'ordine rispetto ai comandi git.
        self.git_log.lock().unwrap().push(format!("hook|{command}"));
        if let Some((_, out)) = self.hook_rules.lock().unwrap().iter().find(|(n, _)| line.contains(n.as_str())) {
            return Ok(out.clone());
        }
        Ok(GitOut { ok: true, stdout: String::new(), stderr: String::new() })
    }
    fn read_text_file(&self, _ws: &Workspace, path: &str, max_bytes: u64) -> Result<Option<String>, String> {
        match self.text_files.lock().unwrap().get(path) {
            None => Ok(None),
            Some(t) if t.len() as u64 > max_bytes => Err(format!("file too large (max {max_bytes} bytes)")),
            Some(t) => Ok(Some(t.clone())),
        }
    }
    fn remove_file(&self, _ws: &Workspace, path: &str) -> Result<(), String> {
        self.text_files.lock().unwrap().remove(path);
        self.hook_log.lock().unwrap().push(format!("rm {path}"));
        Ok(())
    }
    fn save_board(&self, board: &Board) -> Result<(), String> {
        self.saved.lock().unwrap().insert(board.workspace_id.clone(), board.clone());
        Ok(())
    }
    fn delete_board(&self, ws_id: &str) {
        self.deleted.lock().unwrap().push(ws_id.to_string());
        self.saved.lock().unwrap().remove(ws_id);
    }
    fn archive_tasks(&self, ws_id: &str, tasks: &[Task]) -> Result<(), String> {
        if let Some(e) = self.archive_err.lock().unwrap().clone() {
            return Err(e);
        }
        self.archive_log.lock().unwrap().extend(tasks.iter().map(|t| (ws_id.to_string(), t.clone())));
        Ok(())
    }
    fn archive_goals(&self, ws_id: &str, goals: &[Goal]) -> Result<(), String> {
        if let Some(e) = self.archive_err.lock().unwrap().clone() {
            return Err(e);
        }
        self.goal_archive_log.lock().unwrap().extend(goals.iter().map(|g| (ws_id.to_string(), g.clone())));
        Ok(())
    }
    fn emit_board(&self, view: &BoardView) {
        self.board_events.lock().unwrap().push(view.clone());
    }
    fn board_published(&self, view: &BoardView) {
        self.published.lock().unwrap().push((view.workspace_id.clone(), view.rev));
    }
    fn emit_e2e(&self, ev: &crate::automation::e2e::E2eEvent) {
        self.e2e_events.lock().unwrap().push(ev.clone());
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
    fn skills_store_dir(&self) -> Result<std::path::PathBuf, String> {
        self.skills_store.lock().unwrap().clone().ok_or_else(|| "no skills store".to_string())
    }
    fn skill_targets(&self) -> Vec<SkillTarget> {
        let dirs = self.skill_dirs.lock().unwrap();
        dirs.iter().map(|(env, dir)| SkillTarget { env: env.clone(), dir: dir.clone() }).collect()
    }
    fn apply_settings(&self, ws_id: &str, patch: &SettingsPatch) -> Result<Workspace, String> {
        let call = {
            let mut applies = self.applies.lock().unwrap();
            *applies += 1;
            *applies
        };
        if let Some(e) = self.apply_err.lock().unwrap().clone() {
            return Err(e);
        }
        if ws_id != "ws-1" {
            return Err("workspace non trovato".to_string());
        }
        let mut w = self.ws_for(ws_id).expect("ws-1");
        let mut seq = 0;
        patch.apply(&mut w, &self.shared_launch_ids(), &mut || {
            seq += 1;
            format!("launch-{call}-{seq}")
        })?;
        self.store_ws1(w.clone());
        Ok(w)
    }
    fn shared_launch_ids(&self) -> Vec<String> {
        crate::workspaces::shared_launch_ids(vec![FAKE_BOT_LAUNCH.to_string()])
    }
    fn agent_bots(&self) -> Vec<AgentBot> {
        self.bots.0.lock().unwrap().clone()
    }
    fn agent_readiness(&self, _ws: &Workspace, agent: &str) -> Option<Readiness> {
        self.readiness.lock().unwrap().get(agent).copied().unwrap_or(Some(unforced_readiness(agent)))
    }
    fn probe_agent(&self, _ws: &Workspace, agent: &str) -> Readiness {
        self.probed.lock().unwrap().push(agent.to_string());
        self.readiness.lock().unwrap().get(agent).copied().flatten().unwrap_or_else(|| unforced_readiness(agent))
    }
}

/// Prontezza di un agente senza uno stato forzato: come in produzione un agente
/// disabilitato (o fuori dal registro) e' `Disabled`, gli altri sono pronti.
fn unforced_readiness(agent: &str) -> Readiness {
    if crate::agents::registry::is_enabled(agent) {
        Readiness::Ready
    } else {
        Readiness::Disabled
    }
}

// --- helper comuni ---

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

fn call(st: &AutomationState, h: &FakeHost, session: &str, method: &str, params: Value) -> Result<Value, String> {
    rpc::dispatch(st, h, session, method, &params)
}

fn spec(id: &str, deps: &[&str]) -> Value {
    json!({"id": id, "title": format!("Task {id}"), "task": format!("do {id}"), "depends_on": deps, "model": "sonnet", "bot": "general"})
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
fn stop(
    st: &AutomationState,
    h: &FakeHost,
    tab: &str,
    active: Option<bool>,
    background: Option<u32>,
) -> Option<String> {
    let ev =
        HookEvent { event: "finished", stop_hook_active: active, background_work: background, ..Default::default() };
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

#[test]
fn fake_host_text_files_and_cancelled_hooks() {
    let h = FakeHost::default();
    let w = ws("ws-1", "Proj", "windows");
    assert_eq!(h.read_text_file(&w, "/a.txt", 10), Ok(None));
    h.set_text_file("/a.txt", "0123456789ab");
    assert!(h.read_text_file(&w, "/a.txt", 10).unwrap_err().contains("too large"));
    assert_eq!(h.read_text_file(&w, "/a.txt", 100).unwrap().as_deref(), Some("0123456789ab"));
    h.remove_file(&w, "/a.txt").unwrap();
    assert_eq!(h.text_file("/a.txt"), None);
    assert_eq!(h.hook_log.lock().unwrap().last().map(String::as_str), Some("rm /a.txt"));

    let flag = CancelFlag::default();
    assert!(h.run_hook(&w, "/d", "x", &[], Duration::from_secs(1), Some(&flag)).is_ok());
    flag.cancel();
    assert_eq!(h.run_hook(&w, "/d", "x", &[], Duration::from_secs(1), Some(&flag)).unwrap_err(), "cancelled");
}
