//! Git dell'automazione: avvio di un task (base, worktree, setup, spawn del
//! worker), pulizia a consegna avvenuta, rilevamento dei repository, esecuzione
//! di git e accesso ai file della board. Generico sull'`Host`, quindi si prova
//! con un host finto; gira sempre fuori dal lock del registro, su un thread.

use super::hooks;
use super::*;
use crate::agents::AutomationRequest;
use crate::board::{repo_key, valid_branch, Task, WorktreeInfo};
use crate::workspaces::{DeliveryMode, RepoConfig};
use std::path::PathBuf;

/// Esito di un comando git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOut {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Avvio di un task scelto dallo scheduler: tutto cio' che serve fuori dal
/// lock (git + spawn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartJob {
    pub ws: Workspace,
    pub task: Task,
    /// `attempts` al momento della scelta: scarta avvii superati da uno
    /// stop/retry.
    pub attempt: u32,
}

/// Worktree da rimuovere (task concluso o eliminato).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupTarget {
    pub ws_id: String,
    pub task_id: String,
    pub repo: Option<String>,
    pub wt: WorktreeInfo,
}

const EXCLUDE_LINE: &str = ".worktrees/";
/// Quanto vale un `git fetch` gia' fatto per (repo, base).
const FETCH_TTL: Duration = Duration::from_secs(60);
/// Tetto di un `git fetch` (il resto dei comandi git ha `GIT_TIMEOUT`).
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Tetto di un comando git (o wsl.exe): oltre, il processo si uccide e
/// l'avvio fallisce invece di bloccare per sempre il thread.
pub(super) const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Cache dei fetch: (workspace|cartella|base) -> (istante, avviso).
pub(super) type FetchCache = HashMap<String, (std::time::Instant, Option<String>)>;

// ---------------------------------------------------------------------------
// Git e percorsi
// ---------------------------------------------------------------------------

/// Esegue git e ritorna lo stdout (trim); fallimento = errore leggibile.
fn git<H: Host>(host: &H, ws: &Workspace, dir: &str, args: &[&str]) -> Result<String, String> {
    let out = host.git(ws, dir, args).map_err(|e| format!("git is not available: {e}"))?;
    if out.ok {
        Ok(out.stdout.trim().to_string())
    } else {
        let why = if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() };
        Err(format!("git {} failed: {}", args.join(" "), clip_chars(why, 500)))
    }
}

/// Unisce un path relativo (con `/`) al path della root nell'ambiente del
/// workspace: `\` su Windows, `/` su WSL.
pub(super) fn join_path(kind: &str, root: &str, rel: &str) -> String {
    let sep = if kind == "wsl" { '/' } else { '\\' };
    let root = root.trim_end_matches(['/', '\\']);
    let rel: String = rel.chars().map(|c| if c == '/' { sep } else { c }).collect();
    format!("{root}{sep}{rel}")
}

fn is_abs(kind: &str, p: &str) -> bool {
    if kind == "wsl" {
        return p.starts_with('/');
    }
    let b = p.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\');
    drive || p.starts_with("\\\\") || p.starts_with("//")
}

/// Cartella del repository di un task nell'ambiente del workspace: la root
/// se `repo` e' `None`, altrimenti `repo` sotto la root.
pub(super) fn repo_dir(ws: &Workspace, repo: Option<&str>) -> String {
    match repo {
        Some(r) => join_path(&ws.kind, &ws.path, r),
        None => ws.path.clone(),
    }
}

fn repo_cfg<'a>(ws: &'a Workspace, repo: Option<&str>) -> Option<&'a RepoConfig> {
    ws.repos.get(&repo_key(repo))
}

/// I repository esistono e, se la modalita' di consegna usa i branch, sono
/// repo git. Gira fuori dal lock (git).
pub(super) fn ensure_repos<'a, H: Host>(
    host: &H,
    ws: &Workspace,
    repos: impl IntoIterator<Item = Option<&'a str>>,
) -> Result<(), String> {
    let mut seen: Vec<Option<&str>> = Vec::new();
    for repo in repos {
        if seen.contains(&repo) {
            continue;
        }
        seen.push(repo);
        let dir = repo_dir(ws, repo);
        if let Some(r) = repo {
            if !host.dir_exists(ws, &dir) {
                return Err(format!("the repository folder '{r}' ({dir}) does not exist in the workspace"));
            }
        }
        if ws.delivery.uses_branches() {
            let inside = host.git(ws, &dir, &["rev-parse", "--is-inside-work-tree"]).map_err(|e| format!("git is not available: {e}"))?;
            if !(inside.ok && inside.stdout.trim() == "true") {
                return Err(format!(
                    "{dir} is not a git repository: the delivery mode of this workspace works on branches and needs one. \
                     Fix the repo of the task or ask the user to run git init there"
                ));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Base, fetch, worktree
// ---------------------------------------------------------------------------

/// Branch base di un repository: `base_branch` del repository, poi (solo `pr`)
/// il default di `origin`, poi il branch corrente della cartella.
fn resolve_base<H: Host>(host: &H, ws: &Workspace, dir: &str, repo: Option<&str>) -> Result<String, String> {
    if let Some(b) = repo_cfg(ws, repo).and_then(|c| c.base_branch.as_deref()).filter(|b| valid_branch(b)) {
        return Ok(b.to_string());
    }
    if ws.delivery.mode == DeliveryMode::Pr {
        if let Ok(r) = git(host, ws, dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
            let b = r.strip_prefix("origin/").unwrap_or(&r);
            if valid_branch(b) {
                return Ok(b.to_string());
            }
        }
    }
    let cur = current_branch(host, ws, dir)?;
    if cur.is_empty() || cur == "HEAD" {
        let what = repo.map_or("the workspace".to_string(), |r| format!("the repository '{r}'"));
        return Err(format!("{what} is in detached HEAD state: check out a branch or set the repository base branch in the workspace settings"));
    }
    Ok(cur)
}

fn current_branch<H: Host>(host: &H, ws: &Workspace, dir: &str) -> Result<String, String> {
    git(host, ws, dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

fn ref_exists<H: Host>(host: &H, ws: &Workspace, dir: &str, name: &str) -> bool {
    host.git(ws, dir, &["rev-parse", "--verify", "--quiet", name]).map(|o| o.ok).unwrap_or(false)
}

/// `git fetch origin <base>` (al massimo una volta ogni `FETCH_TTL` per
/// repository e base; salta se manca `origin`) e riferimento da cui creare:
/// `origin/<base>` se esiste, altrimenti il branch locale. Un fetch fallito
/// non blocca: si prosegue con i riferimenti locali e c'e' un avviso.
fn remote_or_local<H: Host>(
    host: &H,
    ws: &Workspace,
    dir: &str,
    base: &str,
    cache: &mut FetchCache,
    warnings: &mut Vec<String>,
) -> String {
    let key = format!("{}|{dir}|{base}", ws.id);
    let warn = match cache.get(&key).filter(|(at, _)| at.elapsed() < FETCH_TTL) {
        Some((_, w)) => w.clone(),
        None => {
            let has_origin = host.git(ws, dir, &["remote", "get-url", "origin"]).map(|o| o.ok).unwrap_or(false);
            let warn = if !has_origin {
                None
            } else {
                match host.git(ws, dir, &["fetch", "origin", base]) {
                    Ok(o) if o.ok => None,
                    Ok(o) => Some(format!(
                        "git fetch origin {base} failed ({}): using the local refs",
                        clip_chars(o.stderr.trim(), 200)
                    )),
                    Err(e) => Some(format!("git fetch origin {base} failed ({e}): using the local refs")),
                }
            };
            cache.insert(key, (std::time::Instant::now(), warn.clone()));
            warn
        }
    };
    if let Some(w) = warn.filter(|w| !warnings.contains(w)) {
        warnings.push(w);
    }
    let remote = format!("origin/{base}");
    if ref_exists(host, ws, dir, &format!("refs/remotes/{remote}")) {
        remote
    } else {
        base.to_string()
    }
}

/// `.worktrees/` fuori dal git status del repository, senza toccare .gitignore.
fn add_worktrees_exclude<H: Host>(host: &H, ws: &Workspace, dir: &str, warnings: &mut Vec<String>) {
    match git(host, ws, dir, &["rev-parse", "--git-path", "info/exclude"]) {
        Ok(rel) => {
            let file = if is_abs(&ws.kind, &rel) { rel } else { join_path(&ws.kind, dir, &rel) };
            if let Err(e) = host.append_exclude(ws, &file, EXCLUDE_LINE) {
                warnings.push(format!("{EXCLUDE_LINE} not added to the git exclude file ({e}): it may show up in git status"));
            }
        }
        Err(e) => warnings.push(format!("git exclude file not found ({e}): {EXCLUDE_LINE} may show up in git status")),
    }
}

/// Setup di un worktree appena creato (file ignorati, comandi), se il
/// repository ne ha uno. Ogni passo e' pubblicato e fa da battito del guardiano.
fn setup_worktree<H: Host>(st: &AutomationState, host: &H, job: &StartJob, dir: &str, wt: &WorktreeInfo) -> Result<Vec<String>, String> {
    let (ws, task) = (&job.ws, &job.task);
    let Some(cfg) = repo_cfg(ws, task.repo.as_deref()).filter(|c| c.has_setup()) else {
        return Ok(Vec::new());
    };
    let key = repo_key(task.repo.as_deref());
    let ctx = hooks::SetupCtx {
        ws,
        cfg,
        repo: &key,
        repo_dir: dir,
        worktree: &wt.path,
        task_id: &task.id,
        branch: &wt.branch,
        base: &wt.base,
    };
    let report = |label: &str| {
        st.run(host, |r| r.touch_preparing(&ws.id, &task.id, job.attempt, Some(label), now_millis(), true));
    };
    hooks::run_setup(host, &ctx, &report)
}

/// Crea (o riusa) il worktree del task nel suo repository, dal branch base:
/// `origin/<base>` dopo un fetch (`pr`) o il `<base>` locale (`local_merge`).
/// Ritorna anche gli avvisi (non bloccanti) per la nota del task.
fn prepare_worktree<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    base: &str,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let dir = repo_dir(ws, task.repo.as_deref());
    // Retry: il worktree gia' creato si riusa cosi' com'e'.
    if let Some(wt) = task.worktree.as_ref().filter(|wt| host.dir_exists(ws, &wt.path)) {
        return Ok((wt.clone(), Vec::new()));
    }
    let mut warnings = Vec::new();
    let base_ref = if ws.delivery.mode == DeliveryMode::Pr {
        // Un fetch alla volta: la cache evita di ripeterli per ogni task.
        let mut cache = st.fetches.lock().unwrap_or_else(|e| e.into_inner());
        remote_or_local(host, ws, &dir, base, &mut cache, &mut warnings)
    } else {
        base.to_string()
    };
    let path = task
        .worktree
        .as_ref()
        .map(|w| w.path.clone())
        .unwrap_or_else(|| join_path(&ws.kind, &dir, &format!(".worktrees/{}", task.id)));
    let mut info = WorktreeInfo { path, branch: format!("task/{}", task.id), base: base_ref, setup_done: true };
    // Cartella gia' presente ma non registrata (non e' nostra): niente setup.
    if host.dir_exists(ws, &info.path) {
        return Ok((info, warnings));
    }
    add_worktrees_exclude(host, ws, &dir, &mut warnings);
    let _ = git(host, ws, &dir, &["worktree", "prune"]);
    if ref_exists(host, ws, &dir, &format!("refs/heads/{}", info.branch)) {
        git(host, ws, &dir, &["worktree", "add", &info.path, &info.branch])?;
    } else {
        git(host, ws, &dir, &["worktree", "add", &info.path, "-b", &info.branch, &info.base])?;
    }
    // Appena creato: il setup e' da fare. Si registra subito, cosi' un setup
    // fallito (o un'app chiusa a meta') lascia traccia e il retry lo rifa.
    info.setup_done = false;
    st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, info.clone()));
    Ok((info, warnings))
}

/// Worktree del task pronto: creato (o riusato) e con il setup del
/// repository fatto. Se c'e' qualcosa di lento da fare il task risulta "in
/// preparazione" e il guardiano dell'avvio aspetta (vedi `PREPARING_TIMEOUT_MS`).
/// Un setup fallito lascia il worktree com'e' (registrato con `setup_done`
/// falso) e fallisce l'avvio.
fn prepare_and_setup<H: Host>(
    st: &AutomationState,
    host: &H,
    job: &StartJob,
    base: &str,
) -> Result<(WorktreeInfo, Vec<String>), String> {
    let (ws, task) = (&job.ws, &job.task);
    let slow = repo_cfg(ws, task.repo.as_deref()).is_some_and(|c| c.has_setup());
    if slow {
        st.run(host, |r| r.touch_preparing(&ws.id, &task.id, job.attempt, Some("worktree"), now_millis(), true));
    }
    let end = || {
        if slow {
            st.run(host, |r| r.end_preparing(&ws.id, &task.id, job.attempt, now_millis()));
        }
    };
    let result = prepare_worktree(st, host, job, base).and_then(|(mut wt, mut warnings)| {
        if !wt.setup_done {
            let dir = repo_dir(ws, task.repo.as_deref());
            warnings.extend(setup_worktree(st, host, job, &dir, &wt)?);
            wt.setup_done = true;
            // Registrato fatto subito: se un passo successivo dell'avvio fallisce, il retry non rifa il setup.
            st.run(host, |r| r.record_worktree(&ws.id, &task.id, job.attempt, wt.clone()));
        }
        Ok((wt, warnings))
    });
    end();
    result
}

/// `git diff --stat <base>...HEAD` piu' i file non committati, troncato.
/// Gira nel worktree del task, che sta nel repository del task.
pub(super) fn compute_diffstat<H: Host>(host: &H, ws: &Workspace, wt: &WorktreeInfo) -> Option<String> {
    let range = format!("{}...HEAD", wt.base);
    let stat = git(host, ws, &wt.path, &["diff", "--stat", &range]).unwrap_or_default();
    let dirty = git(host, ws, &wt.path, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    let mut out = if stat.is_empty() { "no committed changes".to_string() } else { stat };
    if dirty > 0 {
        out.push_str(&format!("\n{dirty} file(s) with uncommitted changes"));
    }
    Some(clip_chars(&out, board::DIFFSTAT_MAX))
}

// ---------------------------------------------------------------------------
// Pulizia
// ---------------------------------------------------------------------------

/// Esito della pulizia: gli avvisi non bloccano mai.
pub(super) struct CleanupOutcome {
    pub removed: bool,
    pub warnings: Vec<String>,
}

/// Esegue i comandi di pulizia del repository (se ci sono), poi rimuove il
/// worktree (con `--force` se l'impostazione lo chiede) e il branch del task:
/// `-D` per `pr` (il merge e' squash/rebase lato server, la consegna l'ha
/// confermata il worker), `-d` altrimenti (non cancella lavoro non unito).
pub(super) fn cleanup_worktree<H: Host>(host: &H, ws: &Workspace, target: &CleanupTarget) -> CleanupOutcome {
    let (wt, mut warnings) = (&target.wt, Vec::new());
    let root = repo_dir(ws, target.repo.as_deref());
    let cfg = repo_cfg(ws, target.repo.as_deref()).cloned().unwrap_or_default();
    if host.dir_exists(ws, &wt.path) {
        if !cfg.cleanup.is_empty() {
            let vars = hooks::HookVars { repo_dir: &root, worktree: &wt.path, task_id: &target.task_id, branch: &wt.branch, base: &wt.base };
            warnings.extend(hooks::run_cleanup(host, ws, &cfg, &vars));
        }
        let mut args = vec!["worktree", "remove"];
        if cfg.remove_force {
            args.push("--force");
        }
        args.push(&wt.path);
        if let Err(e) = git(host, ws, &root, &args) {
            warnings.push(format!("worktree not removed: {e}"));
            return CleanupOutcome { removed: false, warnings };
        }
    } else {
        let _ = git(host, ws, &root, &["worktree", "prune"]);
    }
    let flag = if ws.delivery.mode == DeliveryMode::Pr { "-D" } else { "-d" };
    if let Err(e) = git(host, ws, &root, &["branch", flag, &wt.branch]) {
        warnings.push(format!("branch not deleted: {e}"));
    }
    CleanupOutcome { removed: true, warnings }
}

/// Pulizia di un worktree dopo la chiusura del tab (thread del timer).
pub(super) fn run_cleanup<H: Host>(st: &AutomationState, host: &H, target: CleanupTarget) {
    let Some(ws) = host.workspace(&target.ws_id) else {
        return;
    };
    let outcome = cleanup_worktree(host, &ws, &target);
    st.run(host, |r| r.cleanup_done(&target.ws_id, &target.task_id, outcome.removed, &outcome.warnings, now_millis()));
}

// ---------------------------------------------------------------------------
// Rilevamento dei repository del workspace
// ---------------------------------------------------------------------------

/// Elenco dei repository da un elenco di nomi (uno per riga): normalizzati,
/// senza doppioni, "." per primo.
pub(super) fn parse_repo_list(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|l| board::normalize_repo(l).ok().map(|r| r.unwrap_or_else(|| ".".to_string())))
        .collect();
    v.sort_by(|a, b| (a != ".", a).cmp(&(b != ".", b)));
    v.dedup();
    v.truncate(200);
    v
}

/// Repository del workspace: "." se la root stessa e' un repo, piu' le
/// sottocartelle dirette (profondita' 1, senza quelle nascoste) con un `.git`
/// (cartella o file). WSL: una sola chiamata `sh`.
pub fn detect_repos(ws: &Workspace) -> Result<Vec<String>, String> {
    if ws.kind == "wsl" {
        let distro = ws.distro.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| "WSL distro missing".to_string())?;
        let script = "for d in */; do [ -e \"${d}.git\" ] && printf '%s\\n' \"${d%/}\"; done; [ -e .git ] && printf '.\\n'; exit 0";
        let cmd = crate::wsl::wsl_run_command(distro, &ws.path, &["sh", "-c", script]);
        let out = output_with_timeout(cmd, GIT_TIMEOUT)?;
        if !out.status.success() {
            return Err(format!("repository detection failed: {}", clip_chars(String::from_utf8_lossy(&out.stderr).trim(), 200)));
        }
        return Ok(parse_repo_list(&String::from_utf8_lossy(&out.stdout)));
    }
    let root = std::path::Path::new(&ws.path);
    let mut names = Vec::new();
    if root.join(".git").exists() {
        names.push(".".to_string());
    }
    for entry in std::fs::read_dir(root).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && entry.path().is_dir() && entry.path().join(".git").exists() {
            names.push(name);
        }
    }
    Ok(parse_repo_list(&names.join("\n")))
}

// ---------------------------------------------------------------------------
// Avvio di un task (thread separato)
// ---------------------------------------------------------------------------

struct Prepared {
    worktree: Option<WorktreeInfo>,
    warnings: Vec<String>,
    new: NewWorker,
    env: WorkerEnv,
}

/// Con `local_merge` il fast-forward avviene nel checkout del repository: deve
/// stare sul branch base.
fn require_checkout_on_base<H: Host>(host: &H, ws: &Workspace, dir: &str, base: &str) -> Result<(), String> {
    let cur = current_branch(host, ws, dir)?;
    if cur == base {
        Ok(())
    } else {
        Err(format!("local_merge needs {dir} checked out on {base} (now on {cur})"))
    }
}

/// Base del task: quella del worktree da riusare (retry), altrimenti risolta adesso.
fn task_base<H: Host>(host: &H, job: &StartJob, dir: &str) -> Result<String, String> {
    match &job.task.worktree {
        Some(wt) => Ok(wt.base.strip_prefix("origin/").unwrap_or(&wt.base).to_string()),
        None => resolve_base(host, &job.ws, dir, job.task.repo.as_deref()),
    }
}

fn prepare_start<H: Host>(st: &AutomationState, host: &H, job: &StartJob) -> Result<Prepared, String> {
    let (ws, task) = (&job.ws, &job.task);
    let delivery = ws.delivery;
    let dir = repo_dir(ws, task.repo.as_deref());
    let branch = delivery.uses_branches().then(|| format!("task/{}", task.id));
    let (worktree, warnings, base) = if delivery.worktrees() {
        let base = task_base(host, job, &dir)?;
        if delivery.mode == DeliveryMode::LocalMerge {
            require_checkout_on_base(host, ws, &dir, &base)?;
        }
        let (wt, w) = prepare_and_setup(st, host, job, &base)?;
        (Some(wt), w, Some(base))
    } else {
        if let Some(r) = &task.repo {
            if !host.dir_exists(ws, &dir) {
                return Err(format!("the repository folder '{r}' ({dir}) does not exist in the workspace"));
            }
        }
        // Senza worktree l'app non tocca git: legge solo la base per le istruzioni.
        let base = if delivery.uses_branches() { Some(task_base(host, job, &dir)?) } else { None };
        (None, Vec::new(), base)
    };
    let workdir = worktree.as_ref().map_or_else(|| dir.clone(), |w| w.path.clone());
    let settings_path = host.worker_settings_path(ws, &task.agent)?;
    let req = AutomationRequest {
        bootstrap_prompt: BOOTSTRAP_PROMPT,
        permission_mode: ws.worker_permission_mode(),
        model: task.model.as_deref().or(ws.worker_model.as_deref()),
        settings_path: settings_path.as_deref(),
    };
    let command = host.launch_command(&task.agent, &ws.kind, &req)?;
    Ok(Prepared {
        worktree,
        warnings,
        new: NewWorker {
            workspace_id: ws.id.clone(),
            agent_id: task.agent.clone(),
            title: task.title.clone(),
            cwd: workdir.clone(),
            command,
        },
        env: WorkerEnv { delivery, repo_dir: dir, workdir, branch, base },
    })
}

/// Lavoro dell'avvio: git, comando di lancio, registrazione del worker e
/// spawn del tab. Gira su un thread dell'host; rientra nello stato solo per
/// registrare il risultato.
pub(super) fn run_start_job<H: Host>(st: &AutomationState, host: &H, job: StartJob) {
    let (ws_id, tid, attempt) = (job.ws.id.clone(), job.task.id.clone(), job.attempt);
    // Un solo avvio per volta per task: stop + retry mentre il primo e'
    // ancora in git non devono sovrapporre due `worktree add` sullo stesso path.
    let key = format!("{ws_id}/{tid}");
    let lock = st.starts.lock().unwrap().entry(key.clone()).or_default().clone();
    {
        let _turn = lock.lock().unwrap_or_else(|e| e.into_inner());
        // Superato mentre aspettava il turno: niente git.
        if st.run(host, |r| r.start_is_current(&ws_id, &tid, attempt)) {
            match prepare_start(st, host, &job) {
                Ok(p) => {
                    let ev = st.run(host, |r| r.register_task_worker(&job, p.worktree, p.warnings, p.new, p.env, now_millis()));
                    if let Some(ev) = ev {
                        host.emit_spawn(&ev);
                    }
                }
                Err(e) => st.run(host, |r| r.start_failed(&ws_id, &tid, attempt, &e, now_millis())),
            }
        }
    }
    let mut starts = st.starts.lock().unwrap();
    if starts.get(&key).is_some_and(|l| Arc::strong_count(l) <= 2) {
        starts.remove(&key);
    }
}

// ---------------------------------------------------------------------------
// Host reale: git, file
// ---------------------------------------------------------------------------

/// Esegue `cmd` con un tempo massimo. Allo scadere uccide il figlio e ritorna
/// errore; non aspetta i thread di lettura (un nipote potrebbe tenere le
/// pipe aperte).
pub(super) fn output_with_timeout(mut cmd: std::process::Command, limit: Duration) -> Result<std::process::Output, String> {
    use std::io::Read;
    use std::process::Stdio;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    fn drain<R: Read + Send + 'static>(r: Option<R>) -> std::sync::mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    }
    let (out_rx, err_rx) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} s", limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(e.to_string()),
        }
    };
    // Processo chiuso: le pipe si chiudono, breve attesa per l output.
    let grab = |rx: std::sync::mpsc::Receiver<Vec<u8>>| rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    Ok(std::process::Output { status, stdout: grab(out_rx), stderr: grab(err_rx) })
}

pub(super) fn run_git(ws: &Workspace, dir: &str, args: &[&str]) -> Result<GitOut, String> {
    let cmd = if ws.kind == "wsl" {
        let distro = ws.distro.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| "WSL distro missing".to_string())?;
        crate::wsl::wsl_git_command(distro, dir, args)
    } else {
        let mut cmd = std::process::Command::new("git");
        cmd.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };
    let limit = if args.first() == Some(&"fetch") { FETCH_TIMEOUT } else { GIT_TIMEOUT };
    let output = output_with_timeout(cmd, limit).map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    Ok(GitOut {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Aggiunge `line` al file di esclusione (se manca). WSL: uno script `sh`
/// con file e riga passati come argv (mai interpolati; `-e`, senza la shell
/// di default della distro di mezzo).
pub(super) fn append_exclude_line(ws: &Workspace, file: &str, line: &str) -> Result<(), String> {
    if ws.kind == "wsl" {
        let distro = ws.distro.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| "WSL distro missing".to_string())?;
        let script = "mkdir -p \"$(dirname \"$1\")\" && { grep -qxF -e \"$2\" \"$1\" 2>/dev/null || printf '%s\n' \"$2\" >> \"$1\"; }";
        let cmd = crate::wsl::wsl_run_command(distro, &ws.path, &["sh", "-c", script, "sh", file, line]);
        let out = output_with_timeout(cmd, GIT_TIMEOUT)?;
        return if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            Err(format!("exclude not updated: {}", clip_chars(err.trim(), 200)))
        };
    }
    let path = std::path::Path::new(file);
    let current = std::fs::read_to_string(path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    std::fs::write(path, next).map_err(|e| e.to_string())
}

pub(super) fn boards_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join("boards")).map_err(|e| e.to_string())
}

/// File della board; l'id del workspace non deve poter uscire dalla cartella.
pub(super) fn board_file(dir: &std::path::Path, ws_id: &str) -> Result<PathBuf, String> {
    if ws_id.is_empty() || !ws_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("id workspace non valido".to_string());
    }
    Ok(dir.join(format!("{ws_id}.json")))
}
