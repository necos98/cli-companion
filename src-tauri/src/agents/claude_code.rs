//! Provider Claude Code: plugin locale con gli hook che chiamano
//! `cli-companion --notify` e il server MCP `cli-companion --mcp`.
//!
//! Layout generato in `<app_data>/agent-plugins/claude-code/<env>/`:
//! ```text
//! .claude-plugin/marketplace.json
//! plugins/cli-companion-notify/.claude-plugin/plugin.json
//! plugins/cli-companion-notify/hooks/hooks.json
//! plugins/cli-companion-notify/.mcp.json
//! plugins/cli-companion-notify/skills/<preset id>/SKILL.md   (uno per preset)
//! ```
//! Il marketplace locale e' caricato sul posto: la cartella deve restare su
//! disco finche' il plugin e' installato.

use std::path::Path;

use super::presets::{Preset, PRESETS};
use super::{
    claude_usage, parse_version, DETECT_TIMEOUT, PLUGIN_LIST_TIMEOUT, resolve_wsl_command_cached, AgentProvider, AutomationRequest, Detection, Env, InputEvent,
    InputProfile, InputRule, InstallCtx, PermissionMode, SessionUsage, TranscriptCache, UsageLimitsReply,
};

pub struct ClaudeCode;

pub(crate) const AGENT_ID: &str = "claude-code";
const MARKETPLACE: &str = "cli-companion-local";
const PLUGIN: &str = "cli-companion-notify";
/// Id come lo riporta `claude plugin list --json`.
const PLUGIN_ID: &str = "cli-companion-notify@cli-companion-local";
/// Notification che richiedono una risposta dell'utente.
const NEEDS_INPUT_MATCHER: &str = "permission_prompt|idle_prompt|elicitation_dialog";
const HOOK_TIMEOUT_SECS: u32 = 10;
/// Chiave del server MCP nel plugin.
const MCP_SERVER: &str = "cli-companion";
/// Regola di permesso che copre tutti i tool del server MCP del plugin
/// (`mcp__plugin_<plugin>_<server>__<tool>`; senza nome tool = tutti).
/// `acceptEdits` non approva i tool MCP, quindi serve sempre.
const MCP_ALLOW_RULE: &str = "mcp__plugin_cli-companion-notify_cli-companion";
/// Tutte le skill dei preset in un solo testo (serve alla versione del plugin).
fn skills_blob() -> String {
    PRESETS.iter().map(Preset::skill_text).collect::<Vec<_>>().join("\n")
}

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
            // Nessun output e exit 0: non decide, il dialogo di permesso
            // compare come al solito. Porta tool e comando al needs_input.
            "PermissionRequest": [ { "hooks": [ hook("permission-request") ] } ],
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Server MCP del plugin (`.mcp.json` alla radice del plugin): stesso exe
/// degli hook, in modalita' `--mcp` (stdio). Eredita l'ambiente di `claude`,
/// quindi vede `CLI_COMPANION_SESSION` e `CLI_COMPANION_TOKEN`.
fn mcp_json(exe: &str) -> String {
    let v = serde_json::json!({
        "mcpServers": {
            MCP_SERVER: { "command": exe, "args": ["--mcp"] }
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// FNV-1a a 32 bit: serve solo a derivare la versione del plugin.
fn fnv32(s: &str) -> u32 {
    s.bytes()
        .fold(0x811c9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x01000193))
}

/// La versione dipende dalla release dell'app e da hook, server MCP e skill:
/// Claude Code copia il plugin in una cache per versione, quindi se cambia il
/// percorso dell'exe (app spostata), il testo della skill o la release (ogni
/// aggiornamento forza la reinstallazione) la versione deve cambiare, altrimenti
/// resterebbe la copia vecchia. Semver valido: il suffisso e' un prerelease alfanumerico.
fn plugin_version(hooks: &str, mcp: &str, skills: &str) -> String {
    format!(
        "{}-h{:08x}",
        env!("CARGO_PKG_VERSION"),
        fnv32(&format!("{hooks}\n{mcp}\n{skills}"))
    )
}

/// Versione che avrebbe il plugin generato ora per `exe` (come lo vede l'agente).
fn plugin_version_for(exe: &str) -> String {
    plugin_version(&hooks_json(exe), &mcp_json(exe), &skills_blob())
}

fn plugin_json(version: &str) -> String {
    let v = serde_json::json!({
        "name": PLUGIN,
        "description": "Notifies cli-companion when a Claude Code session finishes or waits for an answer, \
                        and provides the MCP tools and the agent presets (skills) of its Tasks Manager.",
        "version": version,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// File del plugin (path relativo alla radice del marketplace, contenuto).
/// `String` -> file: UTF-8 senza BOM.
fn plugin_files(exe: &str) -> Vec<(String, String)> {
    let hooks = hooks_json(exe);
    let mcp = mcp_json(exe);
    let version = plugin_json(&plugin_version(&hooks, &mcp, &skills_blob()));
    let mut files = vec![
        (".claude-plugin/marketplace.json".to_string(), marketplace_json()),
        (
            format!("plugins/{PLUGIN}/.claude-plugin/plugin.json"),
            version,
        ),
        (format!("plugins/{PLUGIN}/hooks/hooks.json"), hooks),
        (format!("plugins/{PLUGIN}/.mcp.json"), mcp),
    ];
    files.extend(
        PRESETS
            .iter()
            .map(|p| (format!("plugins/{PLUGIN}/skills/{}/SKILL.md", p.id), p.skill_text())),
    );
    files
}

/// Toglie da `skills/` le cartelle che non sono piu' preset (es. le vecchie
/// `orchestrate` e `workflow`): la reinstallazione non deve lasciarle nel plugin.
fn remove_obsolete_skills(plugin_dir: &Path) -> std::io::Result<()> {
    let skills = plugin_dir.join("skills");
    let entries = match std::fs::read_dir(&skills) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.filter_map(Result::ok) {
        let current = entry.file_name().to_str().is_some_and(|n| PRESETS.iter().any(|p| p.id == n));
        if !current {
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(path)?;
            } else {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
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

    fn plugin_version(&self, ctx: &InstallCtx, env: &Env) -> Option<String> {
        ctx.exe_for(env).ok().map(|exe| plugin_version_for(&exe))
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    /// Il tasto per incollare un'immagine e' Alt+V su Windows nativo
    /// ("Ctrl+V (Alt+V su Windows e WSL)"; su WSL valgono entrambi): xterm
    /// lo invia come ESC v.
    fn input_profile(&self) -> InputProfile {
        let rule = |env_kind| InputRule {
            event: InputEvent::ImagePaste,
            env_kind: Some(env_kind),
            send: "\x1bv".to_string(),
        };
        InputProfile {
            rules: vec![rule("windows"), rule("wsl")],
        }
    }

    /// `claude "<prompt>" --permission-mode <m> --allowedTools <regola> [--model <m>]`.
    /// Il prompt va PRIMA dei flag (`--allowedTools` e' variadico). La stringa
    /// e' digitata in PowerShell, cmd o bash/zsh: un prompt con caratteri
    /// speciali per una qualunque di queste shell viene rifiutato.
    fn automation_command(&self, _env_kind: &str, req: &AutomationRequest) -> Option<String> {
        let unsafe_char = |c: char| matches!(c, '"' | '$' | '`' | '%' | '!' | '\\') || c.is_control();
        if req.bootstrap_prompt.is_empty() || req.bootstrap_prompt.chars().any(unsafe_char) {
            return None;
        }
        let mode = match req.permission_mode {
            PermissionMode::Default => "default",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Plan => "plan",
            PermissionMode::Bypass => "bypassPermissions",
        };
        // `--settings` prima di `--allowedTools` (variadico). Il path sta tra
        // doppi apici, valido in PowerShell, cmd e bash se privo di
        // `" $ ` % !` (altrimenti niente comando).
        let settings = match req.settings_path {
            Some(p) if crate::worker_permissions::is_shell_safe_path(p) => format!(" --settings \"{p}\""),
            Some(_) => return None,
            None => String::new(),
        };
        let mut cmd = format!(
            "claude \"{}\" --permission-mode {mode}{settings} --allowedTools {MCP_ALLOW_RULE}",
            req.bootstrap_prompt
        );
        if let Some(m) = req.model {
            let ok = !m.is_empty()
                && m.len() <= 64
                && m.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
            if !ok {
                return None;
            }
            cmd.push_str(" --model ");
            cmd.push_str(m);
        }
        Some(cmd)
    }

    fn interactive_command(&self) -> Option<&'static str> {
        Some("claude --dangerously-skip-permissions")
    }

    /// Come l'interattivo, con la skill del preset (`/<plugin>:<id>`) come primo prompt.
    fn preset_command(&self, preset: &Preset) -> Option<String> {
        Some(format!("claude --dangerously-skip-permissions \"/{PLUGIN}:{}\"", preset.id))
    }

    fn usage_limits(&self, env: &Env) -> Option<UsageLimitsReply> {
        Some(claude_usage::limits(env))
    }

    fn session_usage(
        &self,
        env: &Env,
        transcript_path: &str,
        cache: &TranscriptCache,
    ) -> Option<Result<SessionUsage, String>> {
        Some(claude_usage::session_usage(env, transcript_path, cache))
    }

    fn detect(&self, env: &Env) -> Detection {
        let path = match env {
            Env::Windows => Some("claude".to_string()),
            Env::Wsl(d) => resolve_wsl_command_cached(d, "claude"),
        };
        let Some(path) = path else {
            return Detection::default();
        };
        match env.run_timeout(&path, &["--version"], DETECT_TIMEOUT) {
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
        env.run_timeout(program, &["plugin", "list", "--json"], PLUGIN_LIST_TIMEOUT)
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
        remove_obsolete_skills(&root.join("plugins").join(PLUGIN))
            .map_err(|e| format!("pulizia skill obsolete: {e}"))?;
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
            .filter(|(p, c)| {
                assert_ne!(c.as_bytes().first(), Some(&0xEF), "BOM in {p}");
                p.ends_with(".json")
            })
            .map(|(p, c)| (p, serde_json::from_str(&c).expect("JSON valido")))
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
            ("PermissionRequest", "permission-request", None),
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
        assert!(wsl[3].1.contains("/mnt/c/a/cli-companion.exe"));
        // Exe diverso => hooks diversi => versione diversa (cache di Claude).
        assert_ne!(win[1].1, wsl[1].1);
        assert_eq!(win[1].1, plugin_files("C:/a/cli-companion.exe")[1].1);
    }

    #[test]
    fn plugin_declares_mcp_server() {
        for exe in ["C:/Program Files/cli-companion/cli-companion.exe", "/mnt/c/a/cli-companion.exe"] {
            let f = files(exe);
            let m = &f["plugins/cli-companion-notify/.mcp.json"]["mcpServers"]["cli-companion"];
            assert_eq!(m["command"], exe);
            assert_eq!(m["args"], serde_json::json!(["--mcp"]));
        }
        let f = files("x");
        let desc = &f["plugins/cli-companion-notify/.claude-plugin/plugin.json"]["description"];
        assert!(desc.as_str().unwrap().contains("MCP"));
    }

    #[test]
    fn version_depends_on_hooks_mcp_and_skill() {
        let (hooks, mcp) = (hooks_json("C:/a.exe"), mcp_json("C:/a.exe"));
        let skill = skills_blob();
        let v1 = plugin_version(&hooks, &mcp, &skill);
        assert_eq!(v1, plugin_version(&hooks, &mcp, &skill));
        assert_eq!(v1, plugin_version_for("C:/a.exe"));
        // Stessi hook, MCP diverso => versione diversa.
        assert_ne!(v1, plugin_version(&hooks, &mcp_json("C:/b.exe"), &skill));
        assert_ne!(v1, plugin_version(&hooks_json("C:/b.exe"), &mcp, &skill));
        assert_ne!(v1, plugin_version(&hooks, &mcp, "altra skill"));
    }

    #[test]
    fn version_tracks_the_app_release() {
        let v = plugin_version_for("C:/a.exe");
        let (release, hash) = v.split_once("-h").unwrap();
        assert_eq!(release, env!("CARGO_PKG_VERSION"));
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        let files = plugin_files("C:/a.exe");
        let (_, manifest) = files
            .iter()
            .find(|(p, _)| p.ends_with(".claude-plugin/plugin.json") && p.starts_with("plugins/"))
            .unwrap();
        let m: serde_json::Value = serde_json::from_str(manifest).unwrap();
        assert_eq!(m["version"], v.as_str());
    }

    #[test]
    fn plugin_ships_one_skill_per_preset_and_nothing_else() {
        let files = plugin_files("C:/a.exe");
        let skills: Vec<_> = files
            .iter()
            .filter(|(p, _)| p.contains("/skills/"))
            .map(|(p, _)| p.as_str())
            .collect();
        assert_eq!(skills.len(), PRESETS.len());
        for preset in PRESETS {
            let path = format!("plugins/cli-companion-notify/skills/{}/SKILL.md", preset.id);
            let (_, skill) = files.iter().find(|(p, _)| *p == path).expect("SKILL.md del preset nel plugin");
            assert_eq!(*skill, preset.skill_text());
            assert!(!skill.contains('\r'));
        }
        assert!(files.iter().any(|(p, _)| p == "plugins/cli-companion-notify/skills/task-intake/SKILL.md"));
        for gone in ["orchestrate", "workflow"] {
            assert!(!files.iter().any(|(p, _)| p.contains(&format!("skills/{gone}/"))), "{gone}");
        }
    }

    #[test]
    fn preset_command_runs_the_plugin_skill() {
        let preset = &PRESETS[0];
        assert_eq!(
            ClaudeCode.preset_command(preset).as_deref(),
            Some("claude --dangerously-skip-permissions \"/cli-companion-notify:task-intake\"")
        );
    }

    #[test]
    fn install_removes_obsolete_skill_folders() {
        let dir = std::env::temp_dir().join(format!("mc-obsolete-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let skills = dir.join("skills");
        for old in ["orchestrate", "workflow", "task-intake"] {
            std::fs::create_dir_all(skills.join(old)).unwrap();
            std::fs::write(skills.join(old).join("SKILL.md"), "x").unwrap();
        }
        std::fs::write(skills.join("stray.txt"), "x").unwrap();
        remove_obsolete_skills(&dir).unwrap();
        assert!(!skills.join("orchestrate").exists() && !skills.join("workflow").exists());
        assert!(!skills.join("stray.txt").exists());
        // Le cartelle dei preset restano (verranno riscritte).
        assert!(skills.join("task-intake").join("SKILL.md").exists());
        // Senza cartella skills non c'e' nulla da fare.
        remove_obsolete_skills(&dir.join("missing")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_installs_are_flagged_outdated_by_their_marker() {
        // La versione dipende dal contenuto delle skill: il marcatore di un'installazione
        // con le vecchie skill (o senza marcatore) non coincide e `find_outdated` la segnala.
        let current = plugin_version_for("C:/a.exe");
        let old = plugin_version(&hooks_json("C:/a.exe"), &mcp_json("C:/a.exe"), "orchestrate\nworkflow");
        assert_ne!(current, old);
    }

    fn req<'a>(mode: PermissionMode, model: Option<&'a str>, prompt: &'a str) -> AutomationRequest<'a> {
        AutomationRequest { bootstrap_prompt: prompt, permission_mode: mode, model, settings_path: None }
    }

    #[test]
    fn automation_command_adds_quoted_settings_before_allowed_tools() {
        let mut r = req(PermissionMode::Default, Some("opus"), "ciao");
        r.settings_path = Some(r"C:\Users\Mario Rossi\AppData\Roaming\cli-companion\worker-settings\ws-1.json");
        let c = ClaudeCode.automation_command("windows", &r).unwrap();
        assert_eq!(
            c,
            "claude \"ciao\" --permission-mode default --settings \"C:\\Users\\Mario Rossi\\AppData\\Roaming\\cli-companion\\worker-settings\\ws-1.json\" --allowedTools mcp__plugin_cli-companion-notify_cli-companion --model opus"
        );
        r.settings_path = Some("/mnt/c/Users/me/AppData/Roaming/cli-companion/worker-settings/ws-1.json");
        let c = ClaudeCode.automation_command("wsl", &r).unwrap();
        assert!(c.contains(
            " --settings \"/mnt/c/Users/me/AppData/Roaming/cli-companion/worker-settings/ws-1.json\" --allowedTools"
        ));
        // Path con caratteri speciali per la shell: nessun comando.
        for bad in ["C:\\a$b\\x.json", "C:\\a`b", "C:\\100%\\x", "C:\\a\"b", "C:\\hi!\\x"] {
            r.settings_path = Some(bad);
            assert!(ClaudeCode.automation_command("windows", &r).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn automation_command_every_env_and_mode() {
        let p = "Sei una sessione automatica. Chiama get_task.";
        let cases = [
            (PermissionMode::Default, "default"),
            (PermissionMode::AcceptEdits, "acceptEdits"),
            (PermissionMode::Plan, "plan"),
            (PermissionMode::Bypass, "bypassPermissions"),
        ];
        for env in ["windows", "wsl"] {
            for (mode, flag) in cases {
                let c = ClaudeCode.automation_command(env, &req(mode, None, p)).unwrap();
                assert_eq!(
                    c,
                    format!(
                        "claude \"{p}\" --permission-mode {flag} --allowedTools mcp__plugin_cli-companion-notify_cli-companion"
                    )
                );
            }
        }
    }

    #[test]
    fn automation_command_model_and_rejections() {
        let c = ClaudeCode
            .automation_command("wsl", &req(PermissionMode::Plan, Some("claude-opus-4.1"), "ciao"))
            .unwrap();
        assert!(c.ends_with(" --model claude-opus-4.1"));
        for bad in ["a\"b", "a$b", "a`b", "a%b", "a!b", "a\\b", "a\nb", ""] {
            let r = req(PermissionMode::Default, None, bad);
            assert!(ClaudeCode.automation_command("windows", &r).is_none(), "{bad:?}");
        }
        let long = "a".repeat(65);
        for bad in ["", "m x", "m;ls", "m\"", long.as_str()] {
            let r = req(PermissionMode::Default, Some(bad), "ok");
            assert!(ClaudeCode.automation_command("windows", &r).is_none(), "{bad:?}");
        }
        // Tramite l'helper pubblico.
        let r = req(PermissionMode::Default, None, "ok");
        assert!(crate::agents::automation_command("claude-code", "wsl", &r).is_ok());
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
