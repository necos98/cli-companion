use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn wsl_command() -> Command {
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
