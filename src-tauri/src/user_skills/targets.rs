//! Dove le skill utente vengono sincronizzate: `~/.claude/skills` di Windows
//! e quello della home Linux di ogni distro WSL "in uso", cioe' dove e'
//! installato il plugin Claude Code di cli-companion.
//!
//! Scelta di design: la scrittura nelle distro passa dal path UNC
//! (`\\wsl.localhost\<distro>\home\<u>\.claude\skills`) visto da Windows, cosi'
//! tutta la logica di `user_skills` (marcatore `.cli-companion-managed`,
//! cartelle non gestite mai toccate, symlink non seguiti, staging + rename,
//! slug validati contro il path traversal) vale identica, senza costruire
//! alcun comando di shell con contenuto dell'utente.
//!
//! Accedere a un UNC di una distro ferma la avvia: e' accettabile solo perche'
//! i target sono limitati alle distro in uso. La home si risolve prima (con
//! timeout, in parallelo fra le distro): una distro irraggiungibile o appesa
//! diventa `dir: Err(..)` (`ok: false` nei report) senza bloccare le altre.
//! Le operazioni su file via UNC non hanno un timeout proprio, ma partono solo
//! dopo che la distro ha risposto alla risoluzione della home.

use crate::agents::{plugins_dir, sanitize_env_key, CLAUDE_CODE_ID};
use crate::wsl::{is_valid_distro_name, wsl_to_windows_path};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// Nome dell'ambiente Windows nei risultati dei tool delle skill.
pub const WINDOWS_ENV: &str = "windows";

/// Prefisso delle distro interne di Docker, senza una shell utilizzabile.
const DOCKER_DISTRO_PREFIX: &str = "docker-desktop";

/// Cartella delle skill dentro la home (`~/.claude/skills`) di una distro.
const WSL_SKILLS_SUBPATH: &str = ".claude/skills";

/// Ambiente in cui le skill devono comparire e la sua cartella delle skill
/// (`Err` se l'ambiente non e' raggiungibile). Windows piu' le distro WSL in uso.
pub struct SkillTarget {
    pub env: String,
    pub dir: Result<PathBuf, String>,
}

/// Cartella delle skill personali di Claude Code su Windows
/// (`CLAUDE_CONFIG_DIR` o `~/.claude`, come il resto dell'app).
pub fn windows_skills_dir() -> Result<PathBuf, String> {
    crate::agents::claude_usage::windows_claude_dir().map(|dir| dir.join("skills"))
}

/// Il solo target Windows (sempre presente, sempre primo).
pub fn windows_only() -> Vec<SkillTarget> {
    vec![SkillTarget { env: WINDOWS_ENV.to_string(), dir: windows_skills_dir() }]
}

/// Tutti i target: Windows per primo, poi le distro in uso.
pub fn all(data_dir: &Path) -> Vec<SkillTarget> {
    targets_with(data_dir, crate::wsl::installed_distros, crate::wsl::linux_home)
}

/// Distro installate (nell'ordine di `installed`) in cui c'e' il plugin Claude
/// Code: nome valido per un UNC, non interna di Docker e con la cartella del
/// plugin fra `plugin_env_dirs`.
pub fn distros_in_use(plugin_env_dirs: &[String], installed: &[String]) -> Vec<String> {
    installed
        .iter()
        .filter(|d| !d.to_lowercase().starts_with(DOCKER_DISTRO_PREFIX))
        .filter(|d| is_valid_distro_name(d))
        .filter(|d| plugin_env_dirs.contains(&sanitize_env_key(d)))
        .cloned()
        .collect()
}

/// Cartella skill UNC di una distro data la sua home Linux.
pub fn wsl_skills_dir(distro: &str, home: &str) -> Result<PathBuf, String> {
    let linux = format!("{}/{WSL_SKILLS_SUBPATH}", home.trim_end_matches('/'));
    wsl_to_windows_path(distro, &linux)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Cannot build the skills path of WSL distro '{distro}'."))
}

/// Nomi delle cartelle ambiente del plugin Claude Code, escluso Windows.
/// Cartella assente = nessun ambiente.
fn plugin_env_dirs(data_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(plugins_dir(data_dir, CLAUDE_CODE_ID)) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name != WINDOWS_ENV)
        .collect()
}

/// Nucleo di `all` con gli effetti iniettati. `installed` (lento) viene
/// chiamato solo se c'e' almeno una cartella plugin non Windows; le home delle
/// distro in uso si risolvono in parallelo, l'ordine dei target e' preservato.
fn targets_with(
    data_dir: &Path,
    installed: impl FnOnce() -> Result<Vec<String>, String>,
    home: impl Fn(&str) -> Result<String, String> + Sync,
) -> Vec<SkillTarget> {
    let mut targets = windows_only();
    let env_dirs = plugin_env_dirs(data_dir);
    if env_dirs.is_empty() {
        return targets;
    }
    let installed = match installed() {
        Ok(installed) => installed,
        Err(e) => {
            eprintln!("user skills: cannot list the WSL distros: {e}");
            return targets;
        }
    };
    let distros = distros_in_use(&env_dirs, &installed);
    let home = &home;
    let dirs: Vec<Result<PathBuf, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = distros
            .iter()
            .map(|d| scope.spawn(move || home(d).and_then(|h| wsl_skills_dir(d, &h))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("Home resolution panicked.".to_string())))
            .collect()
    });
    targets.extend(distros.into_iter().zip(dirs).map(|(env, dir)| SkillTarget { env, dir }));
    targets
}
