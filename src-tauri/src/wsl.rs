use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub(crate) fn wsl_command() -> Command {
    let mut cmd = Command::new("wsl.exe");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// wsl.exe scrive in UTF-16LE con BOM: decodifica tollerante (UTF-16LE/BE o UTF-8).
fn decode_output(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

pub fn installed_distros() -> Result<Vec<String>, String> {
    let output = wsl_command()
        .args(["-l", "-q"])
        .output()
        .map_err(|e| format!("wsl.exe non disponibile: {e}"))?;
    let text = decode_output(&output.stdout);
    let mut names: Vec<String> = text
        .lines()
        .map(|l| l.replace('\0', "").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    // Difesa contro output localizzati o senza flag -q (vecchie build).
    names.retain(|n| {
        let lower = n.to_lowercase();
        !(lower.starts_with("windows subsystem")
            || lower.starts_with("per ")
            || lower.contains("installer"))
    });
    Ok(names)
}

pub fn wsl_dir_exists(distro: &str, path: &str) -> bool {
    wsl_command()
        .args(["-d", distro, "-e", "test", "-d", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Comando `argv` nella distro, con `dir` come cartella corrente. `-e`
/// (exec) e' indispensabile: con `--` wsl.exe passa la riga alla shell di
/// default della distro, che rompe spazi, `;` e apici negli argomenti.
pub fn wsl_run_command(distro: &str, dir: &str, argv: &[&str]) -> Command {
    let mut cmd = wsl_command();
    cmd.args(["-d", distro, "--cd", dir, "-e"]).args(argv).stdin(std::process::Stdio::null());
    cmd
}

/// `git <args>` nella distro, in `dir` (vedi `wsl_run_command`).
pub fn wsl_git_command(distro: &str, dir: &str, args: &[&str]) -> Command {
    let mut argv = vec!["git"];
    argv.extend_from_slice(args);
    wsl_run_command(distro, dir, &argv)
}

/// Converte path Windows in path Linux WSL:
/// - `C:\Users\me\proj` -> `/mnt/c/Users/me/proj`
/// - `\\wsl$\Ubuntu\home\me` -> `/home/me`
pub fn windows_to_wsl_path(win: &str) -> Option<String> {
    let t = win.trim();
    let slashed = t.replace('\\', "/");
    let lower = slashed.to_lowercase();
    if lower.starts_with("//wsl$/") {
        let mut parts = slashed.split('/').filter(|p| !p.is_empty());
        let _ = parts.next()?; // wsl$
        let _ = parts.next()?; // distro
        let rest: Vec<&str> = parts.collect();
        return Some(format!("/{}", rest.join("/")));
    }
    let bytes = t.as_bytes();
    if t.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
        let drive = bytes[0] as char;
        if drive.is_ascii_alphabetic() {
            let rest = t[2..].replace('\\', "/");
            return Some(format!(
                "/mnt/{}/{}",
                drive.to_ascii_lowercase(),
                rest.trim_start_matches('/')
            ));
        }
    }
    None
}

/// Inverso di `windows_to_wsl_path`: path Linux di `distro` -> path Windows
/// leggibile dall'host.
/// - `/mnt/c/Users/me` -> `C:\Users\me`
/// - `/home/me/x` -> `\\wsl.localhost\Ubuntu\home\me\x`
///
/// `None` se il path non e' assoluto.
pub fn wsl_to_windows_path(distro: &str, linux: &str) -> Option<String> {
    let t = linux.trim();
    if !t.starts_with('/') {
        return None;
    }
    let comps: Vec<&str> = t.split('/').filter(|c| !c.is_empty()).collect();
    if let [mnt, drive, rest @ ..] = comps.as_slice() {
        if *mnt == "mnt" && drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
            let d = drive.to_ascii_uppercase();
            return Some(if rest.is_empty() {
                format!("{d}:\\")
            } else {
                format!("{d}:\\{}", rest.join("\\"))
            });
        }
    }
    Some(format!("\\\\wsl.localhost\\{distro}\\{}", comps.join("\\")))
}

#[tauri::command]
pub async fn list_wsl_distros() -> Result<Vec<String>, String> {
    crate::blocking(installed_distros).await
}

#[tauri::command]
pub async fn check_wsl_path(distro: String, path: String) -> Result<bool, String> {
    if distro.trim().is_empty() || path.trim().is_empty() {
        return Ok(false);
    }
    let path = path.trim();
    let linux_path = if path.starts_with('/') {
        path.to_string()
    } else if let Some(converted) = windows_to_wsl_path(path) {
        converted
    } else {
        return Ok(false);
    };
    crate::blocking(move || Ok(wsl_dir_exists(distro.trim(), &linux_path))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsl_to_windows_converts_mnt_and_distro_paths() {
        let w = |p: &str| wsl_to_windows_path("Ubuntu", p);
        assert_eq!(w("/mnt/c/Users/me/x.jsonl").as_deref(), Some(r"C:\Users\me\x.jsonl"));
        assert_eq!(w("/mnt/d").as_deref(), Some(r"D:\"));
        assert_eq!(
            w("/home/u/.claude/projects/p/s.jsonl").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\u\.claude\projects\p\s.jsonl")
        );
        // `/mnt/cd` non e' un drive.
        assert_eq!(w("/mnt/cd/x").as_deref(), Some(r"\\wsl.localhost\Ubuntu\mnt\cd\x"));
        assert_eq!(w("relativo/x"), None);
        assert_eq!(w(r"C:\x"), None);
        // Inverso di windows_to_wsl_path per i drive.
        let back = windows_to_wsl_path(&w("/mnt/c/a/b").unwrap()).unwrap();
        assert_eq!(back, "/mnt/c/a/b");
    }

    #[test]
    fn run_and_git_use_exec_so_argv_skips_the_distro_shell() {
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        let c = wsl_run_command("Ubuntu", "/p q", &["sh", "-c", "echo $1", "sh", "a b;c"]);
        assert_eq!(args(&c), ["-d", "Ubuntu", "--cd", "/p q", "-e", "sh", "-c", "echo $1", "sh", "a b;c"]);
        let g = wsl_git_command("Ubuntu", "/p", &["worktree", "add", "/p/.worktrees/a b"]);
        assert_eq!(args(&g), ["-d", "Ubuntu", "--cd", "/p", "-e", "git", "worktree", "add", "/p/.worktrees/a b"]);
    }
}
