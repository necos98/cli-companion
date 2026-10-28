//! Hook per repository: setup di un worktree appena creato (copia dei file
//! ignorati, comandi) e pulizia prima della rimozione. Li esegue l'app, nel
//! thread dell'avvio o della pulizia, mai sotto il lock del registro. Lo
//! stato (cosa e' stato fatto) lo tiene chi chiama (`setup_done`).
//!
//! Shell: i comandi girano nel worktree. WSL: `wsl -d <distro> --cd <dir> -e
//! env K=V... sh -c <comando>` (argv, mai interpolato: `WSLENV` non porta
//! variabili arbitrarie, `env` si). Windows: `cmd /D /S /C` senza finestra.

use super::process::{new_run_marker, run_command, CancelFlag, KillTree, RUN_MARKER_VAR};
use super::worktrees::{join_path, output_with_timeout, GitOut};
use super::*;
use crate::workspaces::RepoConfig;

/// Tetto di un singolo comando di setup (una copia di `vendor` e' lenta).
pub(super) const HOOK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Tetto di un comando di pulizia.
pub(super) const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2 * 60);
/// Quanto dell'output di un passo si tiene nei messaggi.
const OUTPUT_TAIL: usize = 2048;

/// Ultimi `max` caratteri di `s` (con "..." davanti se tagliato).
pub(super) fn tail(s: &str, max: usize) -> String {
    let s = s.trim();
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let cut: String = s.chars().skip(n - max).collect();
    format!("...{cut}")
}

/// Percorso del worktree dentro il container: `container_root` + percorso
/// del worktree relativo alla cartella del repository, con `/`. Vuoto senza
/// `container_root`.
pub(super) fn container_worktree(cfg: &RepoConfig, repo_dir: &str, worktree: &str) -> String {
    let Some(root) = cfg.container_root.as_deref() else {
        return String::new();
    };
    let norm = |p: &str| p.replace('\\', "/");
    let (repo, wt) = (norm(repo_dir), norm(worktree));
    let repo = repo.trim_end_matches('/');
    let rel = match wt.strip_prefix(repo) {
        Some(r) if r.starts_with('/') => r.trim_start_matches('/').to_string(),
        // Fuori dal repository (non dovrebbe succedere): il solo nome.
        _ => format!(".worktrees/{}", wt.rsplit('/').next().unwrap_or("")),
    };
    format!("{}/{rel}", root.trim_end_matches('/'))
}

/// Variabili d'ambiente dei comandi.
pub(super) struct HookVars<'a> {
    pub repo_dir: &'a str,
    pub worktree: &'a str,
    pub task_id: &'a str,
    pub branch: &'a str,
    pub base: &'a str,
}

pub(super) fn hook_env(cfg: &RepoConfig, v: &HookVars) -> Vec<(String, String)> {
    let e = |k: &str, val: &str| (k.to_string(), val.to_string());
    vec![
        e("REPO", v.repo_dir),
        e("WORKTREE", v.worktree),
        e("TASK_ID", v.task_id),
        e("BRANCH", v.branch),
        e("BASE", v.base),
        e("CONTAINER_WORKTREE", &container_worktree(cfg, v.repo_dir, v.worktree)),
    ]
}

/// argv eseguito nella distro: `env K=V... sh -c <comando>`. Il comando e i
/// valori restano elementi di argv (nessuna shell di mezzo, nessun escape).
pub(super) fn wsl_hook_argv(command: &str, env: &[(String, String)]) -> Vec<String> {
    let mut argv = vec!["env".to_string()];
    argv.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
    argv.extend(["sh".to_string(), "-c".to_string(), command.to_string()]);
    argv
}

fn distro_of(ws: &Workspace) -> Result<&str, String> {
    ws.distro.as_deref().filter(|d| !d.is_empty()).ok_or_else(|| "WSL distro missing".to_string())
}

/// Esegue un comando dell'utente nella shell della piattaforma (vedi il
/// modulo), con output limitato e albero di processi ucciso a timeout o
/// annullamento. `Err` solo se non parte, scade o viene annullato.
pub(super) fn run_hook_real(
    ws: &Workspace,
    dir: &str,
    command: &str,
    env: &[(String, String)],
    timeout: Duration,
    cancel: Option<&CancelFlag>,
) -> Result<GitOut, String> {
    let mut kill = KillTree::Native;
    let cmd = if ws.kind == "wsl" {
        let distro = distro_of(ws)?;
        // Il marcatore e' ereditato da tutti i discendenti Linux: serve a ucciderli.
        let marker = new_run_marker();
        let mut env = env.to_vec();
        env.push((RUN_MARKER_VAR.to_string(), marker.clone()));
        let argv = wsl_hook_argv(command, &env);
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        kill = KillTree::Wsl { distro: distro.to_string(), marker };
        crate::wsl::wsl_run_command(distro, dir, &refs)
    } else {
        let mut cmd;
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd = std::process::Command::new("cmd.exe");
            // /S toglie le virgolette esterne: il comando arriva com'e'.
            cmd.raw_arg(format!("/D /S /C \"{command}\"")).creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        #[cfg(not(windows))]
        {
            cmd = std::process::Command::new("sh");
            cmd.args(["-c", command]);
        }
        cmd.current_dir(dir).envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        cmd
    };
    run_command(cmd, timeout, cancel, &kill)
}

/// Percorso del filesystem Windows per un percorso nel formato del workspace
/// (WSL: `\\wsl.localhost\<distro>\...`).
fn fs_path(ws: &Workspace, path: &str) -> Result<std::path::PathBuf, String> {
    if ws.kind == "wsl" {
        let converted = crate::wsl::wsl_to_windows_path(distro_of(ws)?, path);
        return converted.map(std::path::PathBuf::from).ok_or_else(|| format!("invalid WSL path: {path}"));
    }
    Ok(std::path::PathBuf::from(path))
}

/// Legge un file di testo UTF-8 (senza BOM) di al piu' `max_bytes`. `Ok(None)`
/// se non esiste. La dimensione si controlla prima di leggere.
pub(super) fn read_text_file_real(ws: &Workspace, path: &str, max_bytes: u64) -> Result<Option<String>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(fs_path(ws, path)?) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {path}: {e}")),
    };
    let too_large = || format!("file too large (max {max_bytes} bytes)");
    let len = file.metadata().map_err(|e| format!("cannot read {path}: {e}"))?.len();
    if len > max_bytes {
        return Err(too_large());
    }
    let mut buf = Vec::new();
    file.take(max_bytes.saturating_add(1)).read_to_end(&mut buf).map_err(|e| format!("cannot read {path}: {e}"))?;
    if buf.len() as u64 > max_bytes {
        return Err(too_large());
    }
    let bytes = buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&buf);
    String::from_utf8(bytes.to_vec()).map(Some).map_err(|_| format!("{path} is not valid UTF-8"))
}

/// Elimina un file (non una cartella). Ok anche se non esiste.
pub(super) fn remove_file_real(ws: &Workspace, path: &str) -> Result<(), String> {
    match std::fs::remove_file(fs_path(ws, path)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove {path}: {e}")),
    }
}

/// Il percorso (file o cartella) esiste?
pub(super) fn path_exists_real(ws: &Workspace, path: &str) -> bool {
    if ws.kind == "wsl" {
        let Ok(distro) = distro_of(ws) else {
            return false;
        };
        return crate::wsl::wsl_command()
            .args(["-d", distro, "-e", "test", "-e", path])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }
    std::path::Path::new(path).exists()
}

fn copy_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Copia ricorsiva `src` -> `dst` (file o cartella), creando le cartelle
/// padre. WSL: `cp -a` (conserva permessi e link).
pub(super) fn copy_path_real(ws: &Workspace, src: &str, dst: &str) -> Result<(), String> {
    if ws.kind == "wsl" {
        let script = "mkdir -p \"$(dirname \"$2\")\" && cp -a \"$1\" \"$2\"";
        let cmd = crate::wsl::wsl_run_command(distro_of(ws)?, "/", &["sh", "-c", script, "sh", src, dst]);
        let out = output_with_timeout(cmd, HOOK_TIMEOUT)?;
        return if out.status.success() {
            Ok(())
        } else {
            Err(tail(&String::from_utf8_lossy(&out.stderr), 300))
        };
    }
    copy_recursive(std::path::Path::new(src), std::path::Path::new(dst)).map_err(|e| e.to_string())
}

/// Un worktree da preparare.
pub(super) struct SetupCtx<'a> {
    pub ws: &'a Workspace,
    pub cfg: &'a RepoConfig,
    pub repo: &'a str,
    pub repo_dir: &'a str,
    pub worktree: &'a str,
    pub task_id: &'a str,
    pub branch: &'a str,
    pub base: &'a str,
}

fn describe(out: &GitOut) -> String {
    let all = format!("{}\n{}", out.stdout.trim(), out.stderr.trim());
    let t = tail(&all, OUTPUT_TAIL);
    if t.is_empty() {
        "no output".to_string()
    } else {
        t
    }
}

/// Setup di un worktree appena creato: copia dei file ignorati, poi i
/// comandi in ordine (cwd = worktree). `report` riceve il passo che sta per
/// partire. Ok = avvisi (percorsi non copiati); Err = messaggio con il
/// passo e la coda dell'output.
pub(super) fn run_setup<H: Host>(host: &H, c: &SetupCtx, report: &dyn Fn(&str)) -> Result<Vec<String>, String> {
    let (ws, kind) = (c.ws, c.ws.kind.as_str());
    let mut warnings = Vec::new();
    for p in &c.cfg.copy_ignored {
        report(&format!("copy {p}"));
        let ignored = host.git(ws, c.repo_dir, &["check-ignore", "-q", "--", p]).map(|o| o.ok).unwrap_or(false);
        if !ignored {
            warnings.push(format!("{p} not copied: not ignored by git (or git unavailable)"));
            continue;
        }
        let src = join_path(kind, c.repo_dir, p);
        if !host.path_exists(ws, &src) {
            warnings.push(format!("{p} not copied: not found in the repository"));
            continue;
        }
        let dst = join_path(kind, c.worktree, p);
        host.copy_path(ws, &src, &dst)
            .map_err(|e| format!("worktree setup of '{}': copying {p} failed: {e}", c.repo))?;
    }
    let env = hook_env(c.cfg, &HookVars { repo_dir: c.repo_dir, worktree: c.worktree, task_id: c.task_id, branch: c.branch, base: c.base });
    let n = c.cfg.setup.len();
    for (i, cmd) in c.cfg.setup.iter().enumerate() {
        let step = format!("setup {}/{n}: {}", i + 1, clip_chars(cmd, 60));
        report(&step);
        let fail = |why: String| format!("worktree setup of '{}' failed at {step}: {why}", c.repo);
        match host.run_hook(ws, c.worktree, cmd, &env, HOOK_TIMEOUT, None) {
            Ok(o) if o.ok => {}
            Ok(o) => return Err(fail(describe(&o))),
            Err(e) => return Err(fail(e)),
        }
    }
    Ok(warnings)
}

/// Comandi di pulizia di un worktree in uscita. Mai bloccanti: ogni
/// fallimento e' un avviso.
pub(super) fn run_cleanup<H: Host>(host: &H, ws: &Workspace, cfg: &RepoConfig, v: &HookVars) -> Vec<String> {
    let env = hook_env(cfg, v);
    let n = cfg.cleanup.len();
    let mut warnings = Vec::new();
    for (i, cmd) in cfg.cleanup.iter().enumerate() {
        let step = format!("cleanup {}/{n}: {}", i + 1, clip_chars(cmd, 60));
        match host.run_hook(ws, v.worktree, cmd, &env, CLEANUP_TIMEOUT, None) {
            Ok(o) if o.ok => {}
            Ok(o) => warnings.push(format!("{step} failed: {}", describe(&o))),
            Err(e) => warnings.push(format!("{step} failed: {e}")),
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(root: Option<&str>) -> RepoConfig {
        RepoConfig { container_root: root.map(str::to_string), ..RepoConfig::default() }
    }

    #[test]
    fn container_path_is_the_mount_plus_the_relative_worktree() {
        let c = cfg(Some("/var/www/html"));
        assert_eq!(
            container_worktree(&c, "/home/u/fc/DDB", "/home/u/fc/DDB/.worktrees/a"),
            "/var/www/html/.worktrees/a"
        );
        assert_eq!(
            container_worktree(&cfg(Some("/var/www/html/")), "C:\\p\\r1", "C:\\p\\r1\\.worktrees\\a"),
            "/var/www/html/.worktrees/a"
        );
        assert_eq!(container_worktree(&cfg(None), "/r", "/r/.worktrees/a"), "");
    }

    #[test]
    fn wsl_hook_keeps_command_and_values_as_single_argv_items() {
        let env = vec![("REPO".to_string(), "/home/u/my repo".to_string()), ("BRANCH".to_string(), "task/a'b".to_string())];
        let cmd = "cp -a \"$REPO/vendor\" . && echo 'x; y' | cat $(date)";
        let argv = wsl_hook_argv(cmd, &env);
        assert_eq!(argv, ["env", "REPO=/home/u/my repo", "BRANCH=task/a'b", "sh", "-c", cmd]);
        // Il comando wsl.exe finale: --cd e -e (exec), nessuna shell della distro.
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let c = crate::wsl::wsl_run_command("Ubuntu", "/home/u/r/.worktrees/a", &refs);
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&args[..5], ["-d", "Ubuntu", "--cd", "/home/u/r/.worktrees/a", "-e"]);
        assert_eq!(args.last().map(String::as_str), Some(cmd));
    }

    #[test]
    fn tail_keeps_the_end() {
        assert_eq!(tail("  abc  ", 10), "abc");
        assert_eq!(tail("0123456789", 4), "...6789");
        assert_eq!(tail("àèìòù", 2), "...òù");
    }

    #[cfg(windows)]
    fn windows_ws(dir: &std::path::Path) -> Workspace {
        Workspace {
            id: "w".into(),
            name: "w".into(),
            kind: "windows".into(),
            path: dir.to_string_lossy().into_owned(),
            distro: None,
            shell: None,
            created_at: 0,
            launches: Vec::new(),
            worker_permission: "accept-edits".into(),
            worker_model: None,
            worker_allow: Vec::new(),
            worker_deny: Vec::new(),
            delivery: Default::default(),
            use_schedule: false,
            default_launch: None,
            worker_rules: String::new(),
            repos: Default::default(),
            archived: false,
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_hook_runs_in_cwd_with_env() {
        let dir = std::env::temp_dir();
        let ws = windows_ws(&dir);
        let env = vec![("TASK_ID".to_string(), "t 1".to_string())];
        let o = run_hook_real(&ws, &dir.to_string_lossy(), "echo id=%TASK_ID% && cd", &env, Duration::from_secs(20), None).unwrap();
        assert!(o.ok, "{o:?}");
        assert!(o.stdout.contains("id=t 1"), "{o:?}");
        let fail = run_hook_real(&ws, &dir.to_string_lossy(), "exit 3", &[], Duration::from_secs(20), None).unwrap();
        assert!(!fail.ok);
    }

    #[cfg(windows)]
    #[test]
    fn text_files_real_read_and_remove() {
        let dir = std::env::temp_dir().join(format!("cc-hooks-{}", new_run_marker()));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = windows_ws(&dir);
        let p = |n: &str| dir.join(n).to_string_lossy().into_owned();

        assert_eq!(read_text_file_real(&ws, &p("missing.txt"), 100).unwrap(), None);
        std::fs::write(p("bom.txt"), b"\xEF\xBB\xBFciao").unwrap();
        assert_eq!(read_text_file_real(&ws, &p("bom.txt"), 100).unwrap().as_deref(), Some("ciao"));
        std::fs::write(p("big.txt"), vec![b'a'; 101]).unwrap();
        let e = read_text_file_real(&ws, &p("big.txt"), 100).unwrap_err();
        assert_eq!(e, "file too large (max 100 bytes)");
        std::fs::write(p("bad.txt"), [0xFF, 0xFE, 0x41]).unwrap();
        assert!(read_text_file_real(&ws, &p("bad.txt"), 100).unwrap_err().contains("UTF-8"));

        remove_file_real(&ws, &p("bom.txt")).unwrap();
        assert!(!dir.join("bom.txt").exists());
        remove_file_real(&ws, &p("bom.txt")).unwrap();
        assert!(remove_file_real(&ws, &dir.to_string_lossy()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wsl_run_gets_a_unique_marker_in_env() {
        let argv = wsl_hook_argv("true", &[(RUN_MARKER_VAR.to_string(), "1-2-3".to_string())]);
        assert_eq!(argv, ["env", "CLI_COMPANION_RUN=1-2-3", "sh", "-c", "true"]);
    }
}
