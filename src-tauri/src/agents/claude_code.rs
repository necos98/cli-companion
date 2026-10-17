//! Provider Claude Code: plugin locale con gli hook che chiamano
//! `cli-companion --notify`.
//!
//! Layout generato in `<app_data>/agent-plugins/claude-code/<env>/`:
//! ```text
//! .claude-plugin/marketplace.json
//! plugins/cli-companion-notify/.claude-plugin/plugin.json
//! plugins/cli-companion-notify/hooks/hooks.json
//! ```
//! Il marketplace locale e' caricato sul posto: la cartella deve restare su
//! disco finche' il plugin e' installato.

use super::{
    parse_version, resolve_wsl_command, AgentProvider, Detection, Env, InstallCtx,
};

pub struct ClaudeCode;

const AGENT_ID: &str = "claude-code";
const MARKETPLACE: &str = "cli-companion-local";
const PLUGIN: &str = "cli-companion-notify";
/// Id come lo riporta `claude plugin list --json`.
const PLUGIN_ID: &str = "cli-companion-notify@cli-companion-local";
/// Notification che richiedono una risposta dell'utente.
const NEEDS_INPUT_MATCHER: &str = "permission_prompt|idle_prompt|elicitation_dialog";
const HOOK_TIMEOUT_SECS: u32 = 10;

fn marketplace_json() -> String {
    let v = serde_json::json!({
        "name": MARKETPLACE,
        "owner": { "name": "cli-companion" },
        "plugins": [
            { "name": PLUGIN, "source": format!("./plugins/{PLUGIN}") }
        ],
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Hook in forma exec (`command` + `args`, nessuna shell): il comando e'
/// l'exe di cli-companion e non servono quoting ne' escape dei path.
fn hooks_json(exe: &str) -> String {
    let hook = |event: &str| {
        serde_json::json!({
            "type": "command",
            "command": exe,
            "args": ["--notify", event, AGENT_ID],
            "timeout": HOOK_TIMEOUT_SECS,
        })
    };
    let v = serde_json::json!({
        "hooks": {
            "SessionStart": [ { "hooks": [ hook("session-start") ] } ],
            "Stop": [ { "hooks": [ hook("finished") ] } ],
            "SessionEnd": [ { "hooks": [ hook("session-end") ] } ],
            "Notification": [
                { "matcher": NEEDS_INPUT_MATCHER, "hooks": [ hook("needs-input") ] }
            ],
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// FNV-1a a 32 bit: serve solo a derivare la versione del plugin.
fn fnv32(s: &str) -> u32 {
    s.bytes()
        .fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193))
}

/// La versione dipende dagli hook: Claude Code copia il plugin in una cache
/// per versione, quindi se cambia il percorso dell'exe (app spostata o
/// aggiornata) la versione deve cambiare, altrimenti resterebbe la copia
/// vecchia con l'exe sbagliato.
fn plugin_json(hooks: &str) -> String {
    let v = serde_json::json!({
        "name": PLUGIN,
        "description": "Avvisa cli-companion quando una sessione Claude Code finisce o aspetta una risposta.",
        "version": format!("1.0.{}", fnv32(hooks) % 1_000_000),
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// File del plugin (path relativo alla radice del marketplace, contenuto).
/// `String` -> file: UTF-8 senza BOM.
fn plugin_files(exe: &str) -> Vec<(String, String)> {
    let hooks = hooks_json(exe);
    vec![
        (".claude-plugin/marketplace.json".to_string(), marketplace_json()),
        (
            format!("plugins/{PLUGIN}/.claude-plugin/plugin.json"),
            plugin_json(&hooks),
        ),
        (format!("plugins/{PLUGIN}/hooks/hooks.json"), hooks),
    ]
}

/// `true` se l'output di `claude plugin list --json` contiene il nostro
/// plugin (match esatto su `id`). Tollera un array o un oggetto con `plugins`,
/// e rumore prima/dopo il JSON.
fn plugin_listed(list_json: &str, id: &str) -> bool {
    let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s).ok();
    let value = parse(list_json).or_else(|| {
        let start = list_json.find(['[', '{'])?;
        let end = list_json.rfind([']', '}'])?;
        parse(list_json.get(start..=end)?)
    });
    let Some(value) = value else {
        return false;
    };
    let list = match &value {
        serde_json::Value::Array(a) => Some(a),
        v => v.get("plugins").and_then(|p| p.as_array()),
    };
    list.is_some_and(|a| {
        a.iter()
            .any(|p| p.get("id").and_then(|v| v.as_str()) == Some(id))
    })
}

/// Esegue un passo del flusso e lo registra nel log; in caso di errore
/// ritorna il messaggio con il log accumulato.
fn step(
    log: &mut Vec<String>,
    env: &Env,
    program: &str,
    label: &str,
    args: &[&str],
) -> Result<(), String> {
    match env.run(program, args) {
        Ok(_) => {
            log.push(format!("{label}: ok"));
            Ok(())
        }
        Err(e) => Err(format!("{label} fallita: {e}\n{}", log.join("\n"))),
    }
}

/// Come `step` ma gli errori non fermano il flusso (pulizia best-effort).
fn step_lenient(log: &mut Vec<String>, env: &Env, program: &str, label: &str, args: &[&str]) {
    match env.run(program, args) {
        Ok(_) => log.push(format!("{label}: ok")),
        Err(e) => log.push(format!("{label}: {}", e.lines().next().unwrap_or(""))),
    }
}

impl AgentProvider for ClaudeCode {
    fn id(&self) -> &'static str {
        AGENT_ID
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, env: &Env) -> Detection {
        let path = match env {
            Env::Windows => Some("claude".to_string()),
            Env::Wsl(d) => resolve_wsl_command(d, "claude"),
        };
        let Some(path) = path else {
            return Detection::default();
        };
        match env.run(&path, &["--version"]) {
            Ok(out) => Detection {
                found: true,
                version: parse_version(&out),
                path: Some(path),
            },
            Err(_) => Detection::default(),
        }
    }

    fn is_installed(&self, env: &Env, det: &Detection) -> bool {
        let Some(program) = det.path.as_deref() else {
            return false;
        };
        env.run(program, &["plugin", "list", "--json"])
            .map(|out| plugin_listed(&out, PLUGIN_ID))
            .unwrap_or(false)
    }

    fn install(&self, ctx: &InstallCtx, env: &Env, det: &Detection) -> Result<String, String> {
        let program = det
            .path
            .as_deref()
            .filter(|_| det.found)
            .ok_or_else(|| "Claude Code non trovato in questo env".to_string())?;
        let root = ctx.plugin_root(AGENT_ID, env);
        let exe = ctx.exe_for(env)?;
        for (rel, content) in plugin_files(&exe) {
            let file = root.join(&rel);
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("scrittura plugin: {e}"))?;
            }
            std::fs::write(&file, content).map_err(|e| format!("scrittura plugin: {e}"))?;
        }
        let market = env.path_arg(&root)?;
        let plugin_dir = format!("{market}/plugins/{PLUGIN}");
        let mut log = vec![format!("plugin scritto in {}", root.display())];

        // Valida prima di installare: un errore qui e' un nostro bug.
        step(&mut log, env, program, "validazione plugin", &["plugin", "validate", &plugin_dir])?;
        step(&mut log, env, program, "validazione marketplace", &["plugin", "validate", &market])?;

        // Reinstallazione deterministica: pulizia (errori ignorati, la prima
        // volta non c'e' nulla da togliere), poi aggiunta e installazione.
        step_lenient(
            &mut log,
            env,
            program,
            "pulizia plugin precedente",
            &["plugin", "uninstall", PLUGIN_ID, "--scope", "user"],
        );
        step_lenient(
            &mut log,
            env,
            program,
            "pulizia marketplace precedente",
            &["plugin", "marketplace", "remove", MARKETPLACE],
        );
        step(&mut log, env, program, "aggiunta marketplace", &["plugin", "marketplace", "add", &market])?;
        step(
            &mut log,
            env,
            program,
            "installazione plugin",
            &["plugin", "install", PLUGIN_ID, "--scope", "user"],
        )?;

        log.push(if self.is_installed(env, det) {
            "stato: installato".to_string()
        } else {
            "stato: NON rilevato in 'plugin list'".to_string()
        });
        Ok(log.join("\n"))
    }

    fn uninstall(&self, ctx: &InstallCtx, env: &Env, det: &Detection) -> Result<String, String> {
        let mut log = Vec::new();
        if let Some(program) = det.path.as_deref().filter(|_| det.found) {
            step_lenient(
                &mut log,
                env,
                program,
                "rimozione plugin",
                &["plugin", "uninstall", PLUGIN_ID, "--scope", "user"],
            );
            step_lenient(
                &mut log,
                env,
                program,
                "rimozione marketplace",
                &["plugin", "marketplace", "remove", MARKETPLACE],
            );
        } else {
            log.push("Claude Code non trovato: rimossi solo i file generati".to_string());
        }
        let root = ctx.plugin_root(AGENT_ID, env);
        match std::fs::remove_dir_all(&root) {
            Ok(()) => log.push("file generati eliminati".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log.push(format!("file generati: {e}")),
        }
        Ok(log.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn files(exe: &str) -> std::collections::HashMap<String, Value> {
        plugin_files(exe)
            .into_iter()
            .map(|(p, c)| {
                assert_ne!(c.as_bytes().first(), Some(&0xEF), "BOM in {p}");
                (p, serde_json::from_str(&c).expect("JSON valido"))
            })
            .collect()
    }

    #[test]
    fn marketplace_and_plugin_manifests() {
        let f = files("C:/app/cli-companion.exe");
        let m = &f[".claude-plugin/marketplace.json"];
        assert_eq!(m["name"], "cli-companion-local");
        assert_eq!(m["owner"]["name"], "cli-companion");
        assert_eq!(m["plugins"][0]["name"], "cli-companion-notify");
        let src = m["plugins"][0]["source"].as_str().unwrap();
        assert!(src.starts_with("./") && !src.contains('\\') && !src.contains(".."));
        assert_eq!(src, "./plugins/cli-companion-notify");
        assert!(f.contains_key("plugins/cli-companion-notify/.claude-plugin/plugin.json"));
        let p = &f["plugins/cli-companion-notify/.claude-plugin/plugin.json"];
        assert_eq!(p["name"], "cli-companion-notify");
    }

    #[test]
    fn hooks_use_exec_form_with_events() {
        let f = files("C:/Program Files/cli-companion/cli-companion.exe");
        let h = &f["plugins/cli-companion-notify/hooks/hooks.json"]["hooks"];
        let cases = [
            ("SessionStart", "session-start", None),
            ("Stop", "finished", None),
            ("SessionEnd", "session-end", None),
            (
                "Notification",
                "needs-input",
                Some("permission_prompt|idle_prompt|elicitation_dialog"),
            ),
        ];
        for (name, event, matcher) in cases {
            let group = &h[name][0];
            assert_eq!(group["matcher"].as_str(), matcher, "{name}");
            let hook = &group["hooks"][0];
            assert_eq!(hook["type"], "command");
            assert_eq!(hook["command"], "C:/Program Files/cli-companion/cli-companion.exe");
            assert_eq!(hook["args"], serde_json::json!(["--notify", event, "claude-code"]));
            assert_eq!(hook["timeout"], 10);
        }
    }

    #[test]
    fn hooks_support_wsl_exe_path_and_version_tracks_it() {
        let win = plugin_files("C:/a/cli-companion.exe");
        let wsl = plugin_files("/mnt/c/a/cli-companion.exe");
        let hooks = &wsl[2].1;
        assert!(hooks.contains("/mnt/c/a/cli-companion.exe"));
        // Exe diverso => hooks diversi => versione diversa (cache di Claude).
        assert_ne!(win[1].1, wsl[1].1);
        assert_eq!(win[1].1, plugin_files("C:/a/cli-companion.exe")[1].1);
    }

    #[test]
    fn plugin_listed_matches_exact_id_in_either_shape() {
        let id = PLUGIN_ID;
        let entry = format!(r#"{{"id":"{id}","version":"1.0.1","scope":"user","enabled":true}}"#);
        assert!(plugin_listed(&format!("[{entry}]"), id));
        assert!(plugin_listed(&format!(r#"{{"plugins":[{entry}]}}"#), id));
        // Rumore attorno al JSON.
        assert!(plugin_listed(&format!("avviso\n[{entry}]\n"), id));
        // Nessun match.
        assert!(!plugin_listed("[]", id));
        assert!(!plugin_listed("{}", id));
        assert!(!plugin_listed("non json", id));
        assert!(!plugin_listed("", id));
        assert!(!plugin_listed(r#"[{"id":"cli-companion-notify@altro"}]"#, id));
        assert!(!plugin_listed(r#"[{"id":"x-cli-companion-notify@cli-companion-local"}]"#, id));
        assert!(!plugin_listed(r#"[{"name":"cli-companion-notify@cli-companion-local"}]"#, id));
    }

    /// Con `claude` installato: il plugin generato deve passare la
    /// validazione reale. Non installa nulla nella config dell'utente.
    /// Esegui con `cargo test -- --ignored claude_validate`.
    #[test]
    #[ignore]
    fn claude_validate_accepts_generated_plugin() {
        let dir = std::env::temp_dir().join(format!("mc-claude-validate-{}", std::process::id()));
        let ctx = InstallCtx {
            exe: "C:/Program Files/cli-companion/cli-companion.exe".to_string(),
            data_dir: dir.clone(),
        };
        let root = ctx.plugin_root(AGENT_ID, &Env::Windows);
        for (rel, content) in plugin_files(&ctx.exe) {
            let file = root.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
        }
        let market = Env::Windows.path_arg(&root).unwrap();
        let plugin = format!("{market}/plugins/{PLUGIN}");
        let a = Env::Windows.run("claude", &["plugin", "validate", &plugin]);
        let b = Env::Windows.run("claude", &["plugin", "validate", &market]);
        let _ = std::fs::remove_dir_all(&dir);
        a.expect("validate plugin");
        b.expect("validate marketplace");
    }

    /// Rilevamento reale (richiede claude in Windows e/o nella distro).
    /// `cargo test -- --ignored claude_detect --nocapture`
    #[test]
    #[ignore]
    fn claude_detect_real_envs() {
        let mut envs = vec![Env::Windows];
        envs.extend(crate::wsl::installed_distros().unwrap_or_default().into_iter().map(Env::Wsl));
        for env in envs {
            let det = ClaudeCode.detect(&env);
            println!("{}: {:?} installato={}", env.key(), det, det.found && ClaudeCode.is_installed(&env, &det));
        }
    }

    /// Verifica reale dell'argv via WSL (`-e`): `plugin list --json` deve
    /// tornare JSON. `cargo test -- --ignored claude_wsl_argv --nocapture`
    #[test]
    #[ignore]
    fn claude_wsl_argv_survives_wsl_exe() {
        let distro = std::env::var("CC_TEST_DISTRO").unwrap_or_else(|_| "Ubuntu-24.04".to_string());
        let path = crate::agents::resolve_wsl_command(&distro, "claude").expect("claude nella distro");
        println!("claude: {path}");
        let out = Env::Wsl(distro).run(&path, &["plugin", "list", "--json"]).expect("plugin list");
        println!("{out}");
        assert!(serde_json::from_str::<serde_json::Value>(out.trim()).is_ok(), "non e' JSON: {out}");
    }
}
