//! Comandi pre-approvati dei worker Claude Code: regole `permissions.allow` /
//! `permissions.deny` per workspace, scritte in un file settings passato a
//! `claude --settings <file>` all'avvio del worker.
//!
//! Il file sta in `<app_data>/worker-settings/<workspace_id>.json`. Claude
//! Code unisce le regole di `--settings` a quelle degli altri livelli (le
//! liste `allow`/`deny` si sommano; `deny` vince sempre su `allow`, anche in
//! `bypassPermissions`).

use crate::wsl::windows_to_wsl_path;
use std::path::{Path, PathBuf};

pub const MAX_RULES: usize = 100;
pub const MAX_RULE_CHARS: usize = 300;

/// Valida e normalizza una lista di regole: trim, righe vuote e doppioni
/// scartati. Forma ammessa: `Nome` oppure `Nome(...)`, con Nome in
/// `[A-Za-z0-9_*-]+` (anche `mcp__server__tool`). Errori in italiano (UI).
pub fn validate_rules(rules: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in rules {
        let rule = raw.trim();
        if rule.is_empty() {
            continue;
        }
        check_rule(rule).map_err(|e| format!("regola \"{}\": {e}", clip_for_error(rule)))?;
        if !out.iter().any(|r| r == rule) {
            out.push(rule.to_string());
        }
    }
    if out.len() > MAX_RULES {
        return Err(format!("troppe regole (massimo {MAX_RULES})"));
    }
    Ok(out)
}

fn clip_for_error(rule: &str) -> String {
    let mut s: String = rule.chars().take(40).collect();
    if rule.chars().count() > 40 {
        s.push('…');
    }
    s
}

fn check_rule(rule: &str) -> Result<(), String> {
    if rule.chars().count() > MAX_RULE_CHARS {
        return Err(format!("piu' lunga di {MAX_RULE_CHARS} caratteri"));
    }
    if rule.chars().any(char::is_control) {
        return Err("contiene a capo o caratteri di controllo".to_string());
    }
    let name_len = rule
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '*' | '-')))
        .unwrap_or(rule.len());
    if name_len == 0 {
        return Err("forma attesa: Nome oppure Nome(...), es. Bash(git:*)".to_string());
    }
    let (name, rest) = rule.split_at(name_len);
    if rest.is_empty() {
        return Ok(());
    }
    if !(rest.starts_with('(') && rest.ends_with(')') && rest.len() > 2) {
        return Err("forma attesa: Nome oppure Nome(...), es. Bash(git:*)".to_string());
    }
    if name.starts_with("mcp__") {
        return Err("le regole mcp__ non accettano parentesi".to_string());
    }
    Ok(())
}

/// Contenuto del file settings per `--settings`.
pub fn settings_json(allow: &[String], deny: &[String]) -> String {
    let v = serde_json::json!({ "permissions": { "allow": allow, "deny": deny } });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Cartella dei file settings dei worker.
pub fn settings_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("worker-settings")
}

/// Nome file sicuro per un id workspace (gli id sono generati dall'app, ma il
/// file finisce in un path: niente separatori).
fn file_name(workspace_id: &str) -> String {
    let safe: String = workspace_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '_' })
        .collect();
    format!("{safe}.json")
}

/// Path del file come lo vede `claude`: invariato su Windows, `/mnt/<d>/...`
/// per i workspace WSL.
pub fn path_for_env(file: &Path, env_kind: &str) -> Result<String, String> {
    let win = file.to_string_lossy().to_string();
    if env_kind == "wsl" {
        windows_to_wsl_path(&win).ok_or_else(|| format!("path non convertibile per WSL: {win}"))
    } else {
        Ok(win)
    }
}

/// Scrive (solo se cambia) il file settings del workspace e ritorna il path
/// per `--settings`, o `None` se non ci sono regole. Il path finisce in una
/// riga digitata nella shell del tab: se contiene caratteri che nessuna
/// shell tra PowerShell, cmd e bash tratta in modo letterale tra doppi apici
/// il worker non parte (errore chiaro invece di un comando rotto).
pub fn prepare_settings_file(
    data_dir: &Path,
    workspace_id: &str,
    env_kind: &str,
    allow: &[String],
    deny: &[String],
) -> Result<Option<String>, String> {
    if allow.is_empty() && deny.is_empty() {
        return Ok(None);
    }
    let dir = settings_dir(data_dir);
    let file = dir.join(file_name(workspace_id));
    let content = settings_json(allow, deny);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(content.as_str()) {
        std::fs::create_dir_all(&dir).map_err(|e| format!("cartella delle regole dei worker: {e}"))?;
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, &content)
            .and_then(|_| std::fs::rename(&tmp, &file))
            .map_err(|e| format!("scrittura delle regole dei worker: {e}"))?;
    }
    let path = path_for_env(&file, env_kind)?;
    if !is_shell_safe_path(&path) {
        return Err(format!(
            "the app data path contains characters that are unsafe in a shell command line: {path}"
        ));
    }
    Ok(Some(path))
}

/// `true` se il path sta in modo letterale tra doppi apici in PowerShell,
/// cmd e bash.
pub fn is_shell_safe_path(p: &str) -> bool {
    !p.is_empty()
        && !p.chars().any(|c| matches!(c, '"' | '$' | '`' | '%' | '!') || c.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn valid_rules_are_trimmed_deduped_and_blank_lines_dropped() {
        let r = validate_rules(&v(&[
            "  Bash(git:*) ",
            "",
            "Bash(git:*)",
            "Edit",
            "WebFetch(domain:example.com)",
            "mcp__server__tool",
            "Bash(docker exec:*)",
        ]))
        .unwrap();
        assert_eq!(
            r,
            v(&["Bash(git:*)", "Edit", "WebFetch(domain:example.com)", "mcp__server__tool", "Bash(docker exec:*)"])
        );
    }

    #[test]
    fn invalid_rules_are_rejected() {
        for bad in [
            "git status",
            "(git)",
            "Bash(",
            "Bash()",
            "Bash(git:*) extra",
            "Bash(git\nstatus)",
            "Bash(git\u{7}x)",
            "mcp__srv__tool(x)",
            "Ba sh",
        ] {
            assert!(validate_rules(&v(&[bad])).is_err(), "{bad:?}");
        }
        let long = format!("Bash({})", "a".repeat(MAX_RULE_CHARS));
        assert!(validate_rules(&[long]).is_err());
        let many: Vec<String> = (0..=MAX_RULES).map(|i| format!("Bash(c{i}:*)")).collect();
        assert!(validate_rules(&many).is_err());
        let ok: Vec<String> = (0..MAX_RULES).map(|i| format!("Bash(c{i}:*)")).collect();
        assert_eq!(validate_rules(&ok).unwrap().len(), MAX_RULES);
    }

    #[test]
    fn settings_json_has_permissions_allow_and_deny() {
        let j: serde_json::Value =
            serde_json::from_str(&settings_json(&v(&["Bash(git:*)"]), &v(&["Bash(rm:*)"]))).unwrap();
        assert_eq!(j["permissions"]["allow"], serde_json::json!(["Bash(git:*)"]));
        assert_eq!(j["permissions"]["deny"], serde_json::json!(["Bash(rm:*)"]));
    }

    #[test]
    fn path_is_converted_for_wsl_and_plain_for_windows() {
        let f = Path::new(r"C:\Users\Me\AppData\Roaming\cli-companion\worker-settings\ws-1.json");
        assert_eq!(path_for_env(f, "windows").unwrap(), f.to_string_lossy());
        assert_eq!(
            path_for_env(f, "wsl").unwrap(),
            "/mnt/c/Users/Me/AppData/Roaming/cli-companion/worker-settings/ws-1.json"
        );
    }

    #[test]
    fn shell_safe_paths() {
        assert!(is_shell_safe_path(r"C:\Users\Mario Rossi\AppData\x.json"));
        assert!(is_shell_safe_path("/mnt/c/Users/o'neil/x.json"));
        for bad in ["C:\\a$b\\x.json", "C:\\a`b", "C:\\100%\\x", "C:\\a\"b", "C:\\hi!\\x", ""] {
            assert!(!is_shell_safe_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn prepare_writes_only_on_change_and_skips_when_empty() {
        let dir = std::env::temp_dir().join(format!("cc-rules-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(prepare_settings_file(&dir, "ws-1", "windows", &[], &[]).unwrap(), None);
        assert!(!dir.exists());
        let allow = v(&["Bash(git:*)"]);
        let p = prepare_settings_file(&dir, "ws-1", "windows", &allow, &[]).unwrap().unwrap();
        let on_disk = std::fs::read_to_string(&p).unwrap();
        assert_eq!(on_disk, settings_json(&allow, &[]));
        let m1 = std::fs::metadata(&p).unwrap().modified().unwrap();
        prepare_settings_file(&dir, "ws-1", "windows", &allow, &[]).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), m1);
        let p2 = prepare_settings_file(&dir, "ws-1", "windows", &allow, &v(&["Edit"])).unwrap().unwrap();
        assert_eq!(p, p2);
        assert!(std::fs::read_to_string(&p).unwrap().contains("Edit"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
