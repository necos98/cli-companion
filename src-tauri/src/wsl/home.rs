//! `$HOME` dell'utente di default di una distro WSL, risolto una volta per
//! processo. Serve a raggiungere `~/.claude` della distro da Windows (path UNC).

use super::{is_valid_distro_name, output_with_timeout, wsl_command};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Precede `$HOME` nell'output: shell di login e wsl.exe possono scrivere
/// rumore (motd, nvm) prima, e la home e' quel che segue l'ultimo marcatore.
const HOME_MARKER: &str = "__cli_companion_home__";

/// Una distro ferma deve avviarsi: oltre questo tempo la si considera irraggiungibile.
const HOME_TIMEOUT: Duration = Duration::from_secs(10);

/// Caratteri che romperebbero un path UNC Windows (oltre ai controlli).
const UNC_FORBIDDEN: [char; 8] = ['\\', ':', '*', '?', '"', '<', '>', '|'];

/// Home Linux (es. `/home/me`) dell'utente di default di `distro`.
pub fn linux_home(distro: &str) -> Result<String, String> {
    static HOMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let homes = HOMES.get_or_init(Default::default);
    if let Some(home) = homes.lock().unwrap().get(distro) {
        return Ok(home.clone());
    }
    let home = resolve_home(distro)
        .map_err(|reason| format!("Cannot resolve the home folder of WSL distro '{distro}': {reason}"))?;
    homes.lock().unwrap().insert(distro.to_string(), home.clone());
    Ok(home)
}

fn resolve_home(distro: &str) -> Result<String, String> {
    if !is_valid_distro_name(distro) {
        return Err("invalid distro name".to_string());
    }
    let script = format!("printf '%s\\n%s' {HOME_MARKER} \"$HOME\"");
    let out = output_with_timeout(wsl_command().args(["-d", distro, "-e", "sh", "-c", &script]), HOME_TIMEOUT)
        .map_err(|e| e.to_string())?;
    // Il rumore puo' esserci anche con exit != 0: si interpreta comunque lo stdout.
    parse_home(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| {
        format!("unexpected output ({}): {}", out.status, String::from_utf8_lossy(&out.stderr).trim())
    })
}

/// Estrae la home dall'output dello script: assoluta, senza `..` e senza
/// caratteri che spezzerebbero un path UNC.
pub(crate) fn parse_home(output: &str) -> Option<String> {
    let (_, tail) = output.rsplit_once(HOME_MARKER)?;
    let tail = tail.strip_prefix("\r\n").or_else(|| tail.strip_prefix('\n')).unwrap_or(tail);
    let home = tail.trim_end_matches(['\r', '\n']);
    let valid = home.starts_with('/')
        && !home.contains(UNC_FORBIDDEN)
        && !home.contains(char::is_control)
        && !home.split('/').any(|c| c == "..");
    valid.then(|| home.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(home: &str) -> String {
        format!("{HOME_MARKER}\n{home}")
    }

    #[test]
    fn parses_the_home_after_the_marker() {
        assert_eq!(parse_home(&out("/home/me")).as_deref(), Some("/home/me"));
        assert_eq!(parse_home(&out("/root")).as_deref(), Some("/root"));
    }

    #[test]
    fn ignores_login_shell_noise_before_the_marker() {
        let noisy = format!("Welcome to Ubuntu\nnvm: using v20\n{HOME_MARKER} fake\n/tmp/decoy\n{}", out("/home/me"));
        assert_eq!(parse_home(&noisy).as_deref(), Some("/home/me"));
    }

    #[test]
    fn tolerates_crlf_and_trailing_newlines_but_keeps_a_trailing_slash() {
        assert_eq!(parse_home(&format!("{HOME_MARKER}\r\n/home/me\r\n")).as_deref(), Some("/home/me"));
        assert_eq!(parse_home(&out("/home/me/\n")).as_deref(), Some("/home/me/"));
    }

    #[test]
    fn rejects_missing_marker_and_unusable_homes() {
        assert_eq!(parse_home("/home/me"), None);
        assert_eq!(parse_home(""), None);
        for bad in ["", "home/me", "/home/../etc", "/home/a\u{7}b", "/home/a\0b", "/home/a:b", "/home\\me", "/home/a*b"] {
            assert_eq!(parse_home(&out(bad)), None, "{bad:?}");
        }
        assert_eq!(parse_home(&out("/home/me\nextra")), None);
    }

    #[test]
    fn linux_home_rejects_invalid_distro_names() {
        let err = linux_home("a\\b").unwrap_err();
        assert!(err.contains("'a\\b'") && err.contains("invalid distro name"), "{err}");
    }
}
