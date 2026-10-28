//! Installazione di agent-browser (npm) e di Chrome for Testing.

use crate::artifacts::AGENT_BROWSER_VERSION;
use crate::paths::Paths;
use crate::proc::output_with_timeout;
use std::process::{Command, Output};
use std::time::Duration;

const NPM_PROGRAM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };
const NPM_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const BROWSER_INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Installa agent-browser alla versione fissata (saltando `npm ci` se e' gia'
/// quella giusta), poi scarica Chrome for Testing. Ritorna la versione.
pub fn install_agent_browser(paths: &Paths) -> Result<String, String> {
    let exe = paths.agent_browser_exe();
    if installed_version(paths).as_deref() == Some(AGENT_BROWSER_VERSION) {
        println!("  agent-browser {AGENT_BROWSER_VERSION} gia' installato");
    } else {
        println!("  npm ci in {}", paths.root.display());
        let mut npm = Command::new(NPM_PROGRAM);
        npm.arg("ci").current_dir(&paths.root);
        checked(&mut npm, NPM_TIMEOUT)?;
    }
    if !exe.exists() {
        return Err(format!("{} non trovato dopo npm ci", exe.display()));
    }
    println!("  agent-browser install (Chrome for Testing)");
    checked(Command::new(&exe).arg("install"), BROWSER_INSTALL_TIMEOUT)?;
    installed_version(paths).ok_or_else(|| "agent-browser --version non riuscito".to_string())
}

/// Versione riportata dall'eseguibile installato, se presente e funzionante.
fn installed_version(paths: &Paths) -> Option<String> {
    let exe = paths.agent_browser_exe();
    if !exe.exists() {
        return None;
    }
    let out = output_with_timeout(Command::new(exe).arg("--version"), VERSION_TIMEOUT).ok()?;
    out.status
        .success()
        .then(|| parse_version(&String::from_utf8_lossy(&out.stdout)))
        .flatten()
}

/// Estrae il numero di versione dall'ultimo token dell'output (es. "agent-browser 0.40.0").
fn parse_version(output: &str) -> Option<String> {
    let token = output.split_whitespace().last()?;
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
        .then(|| token.to_string())
}

/// Esegue il comando e lo considera fallito se esce con errore.
fn checked(cmd: &mut Command, timeout: Duration) -> Result<Output, String> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let out = output_with_timeout(cmd, timeout)?;
    if out.status.success() {
        return Ok(out);
    }
    Err(format!(
        "{program} fallito ({}): {}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_last_numeric_token() {
        assert_eq!(
            parse_version("agent-browser 0.40.0\n").as_deref(),
            Some("0.40.0")
        );
        assert_eq!(parse_version("0.40.0").as_deref(), Some("0.40.0"));
        assert_eq!(parse_version("errore"), None);
        assert_eq!(parse_version(""), None);
    }
}
