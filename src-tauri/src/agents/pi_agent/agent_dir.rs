//! Cartella dell'agente Pi: `$PI_CODING_AGENT_DIR` se impostata, altrimenti
//! `~/.pi/agent`.

use crate::agents::detection::PLUGIN_LIST_TIMEOUT;
use crate::platform::{wsl_to_windows_path, Target};
use std::path::PathBuf;

/// Variabile d'ambiente che sposta la cartella dell'agente.
const DIR_ENV: &str = "PI_CODING_AGENT_DIR";
/// Cartella predefinita, relativa alla home.
const DEFAULT_DIR: &str = ".pi/agent";

/// Script per la shell della distro: stampa la cartella (variabile o
/// predefinita) con una `~` iniziale gia' espansa.
const WSL_SCRIPT: &str = "d=\"${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}\"; \
     case \"$d\" in \"~\") d=\"$HOME\";; \"~/\"*) d=\"$HOME/${d#\"~/\"}\";; esac; printf '%s' \"$d\"";

/// Espande una `~` iniziale (`~` o `~/...`, anche `~\...`) con la home.
fn expand_tilde(value: &str, home: &str) -> String {
    if value == "~" {
        return home.to_string();
    }
    match value.strip_prefix("~/").or_else(|| value.strip_prefix("~\\")) {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches(['/', '\\'])),
        None => value.to_string(),
    }
}

/// Cartella dell'agente su Windows, dati la variabile e la home (se presenti).
fn windows_dir(env_dir: Option<&str>, home: Option<&str>) -> Result<PathBuf, String> {
    let env_dir = env_dir.map(str::trim).filter(|v| !v.is_empty());
    match (env_dir, home) {
        (Some(dir), home) if !dir.starts_with('~') || home.is_some() => {
            Ok(PathBuf::from(expand_tilde(dir, home.unwrap_or_default())))
        }
        (None, Some(home)) => Ok(PathBuf::from(home).join(DEFAULT_DIR)),
        _ => Err("Cartella utente non trovata.".to_string()),
    }
}

/// Il path stampato dallo script della distro (ultima riga non vuota).
fn parse_wsl_output(output: &str) -> Option<String> {
    output.lines().map(str::trim).rfind(|line| !line.is_empty()).map(str::to_string)
}

/// Cartella dell'agente Pi nel `target`, accessibile da Windows.
pub(super) fn agent_dir(target: &Target) -> Result<PathBuf, String> {
    match target {
        Target::Windows => {
            let var = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty());
            let home = var("USERPROFILE").or_else(|| var("HOME"));
            windows_dir(var(DIR_ENV).as_deref(), home.as_deref())
        }
        Target::Wsl { distro } => {
            let output = target
                .run_timeout("bash", &["-lc", WSL_SCRIPT], PLUGIN_LIST_TIMEOUT)
                .map_err(|e| format!("cartella di Pi non leggibile: {e}"))?;
            let linux =
                parse_wsl_output(&output).ok_or_else(|| "cartella di Pi non trovata nella distro".to_string())?;
            wsl_to_windows_path(distro, &linux)
                .map(PathBuf::from)
                .ok_or_else(|| format!("cartella di Pi non traducibile: {linux}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_dot_pi_agent_in_the_home() {
        assert_eq!(windows_dir(None, Some(r"C:\Users\me")), Ok(PathBuf::from(r"C:\Users\me").join(".pi/agent")));
        assert_eq!(windows_dir(Some("  "), Some("/h")), Ok(PathBuf::from("/h").join(".pi/agent")));
        assert!(windows_dir(None, None).is_err());
    }

    #[test]
    fn the_variable_wins_and_expands_a_leading_tilde() {
        assert_eq!(windows_dir(Some(r"D:\pi"), Some("/h")), Ok(PathBuf::from(r"D:\pi")));
        assert_eq!(windows_dir(Some(r"D:\pi"), None), Ok(PathBuf::from(r"D:\pi")));
        assert_eq!(windows_dir(Some("~/pi"), Some("/h")), Ok(PathBuf::from("/h/pi")));
        assert_eq!(windows_dir(Some("~"), Some("/h/")), Ok(PathBuf::from("/h/")));
        assert!(windows_dir(Some("~/pi"), None).is_err());
    }

    #[test]
    fn tilde_is_expanded_only_at_the_start() {
        assert_eq!(expand_tilde("~", "/h"), "/h");
        assert_eq!(expand_tilde("~/a/b", "/h/"), "/h/a/b");
        assert_eq!(expand_tilde(r"~\a", "C:/h"), "C:/h/a");
        assert_eq!(expand_tilde("~user/a", "/h"), "~user/a");
        assert_eq!(expand_tilde("/x/~/a", "/h"), "/x/~/a");
    }

    #[test]
    fn wsl_output_is_the_last_non_empty_line() {
        assert_eq!(parse_wsl_output("/home/me/.pi/agent").as_deref(), Some("/home/me/.pi/agent"));
        assert_eq!(parse_wsl_output("motd\r\n/home/me/.pi/agent\r\n\n").as_deref(), Some("/home/me/.pi/agent"));
        assert_eq!(parse_wsl_output(" \n"), None);
    }

    #[test]
    fn the_wsl_path_is_readable_from_windows() {
        assert_eq!(
            wsl_to_windows_path("Ubuntu", "/home/me/.pi/agent").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\me\.pi\agent")
        );
    }
}
