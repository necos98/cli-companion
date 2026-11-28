//! Generazione dei file del plugin Claude Code: hook che chiamano
//! `cli-companion --notify` e server MCP `cli-companion --mcp`.
//!
//! Layout generato in `<app_data>/agent-plugins/claude-code/<env>/`:
//! ```text
//! .claude-plugin/marketplace.json
//! plugins/cli-companion-notify/.claude-plugin/plugin.json
//! plugins/cli-companion-notify/hooks/hooks.json
//! plugins/cli-companion-notify/.mcp.json
//! plugins/cli-companion-notify/skills/<preset id>/SKILL.md   (uno per preset)
//! plugins/cli-companion-notify/agents/explorer.md            (sotto-agente di sola lettura)
//! ```
//! Il marketplace locale e' caricato sul posto: la cartella deve restare su
//! disco finche' il plugin e' installato.

use super::plugin_skills::skill_path;
use super::{AGENT_ID, EXPLORER_AGENT, MARKETPLACE, PLUGIN};
use crate::agents::fnv32;
use crate::agents::presets::{Preset, PRESETS};

/// Notification che richiedono una risposta dell'utente.
pub(super) const NEEDS_INPUT_MATCHER: &str = "permission_prompt|idle_prompt|elicitation_dialog";
const HOOK_TIMEOUT_SECS: u32 = 10;
/// Chiave del server MCP nel plugin.
const MCP_SERVER: &str = "cli-companion";

/// Tutte le skill dei preset in un solo testo (serve alla versione del plugin).
fn skills_blob() -> String {
    PRESETS.iter().map(Preset::skill_text).collect::<Vec<_>>().join("\n")
}

/// Il sotto-agente esploratore (`agents/explorer.md`), con fine riga LF qualunque sia il checkout.
fn explorer_text() -> String {
    include_str!("explorer.md").replace("\r\n", "\n")
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

/// La versione dipende dalla release dell'app e da hook, server MCP, skill e
/// sotto-agente esploratore:
/// Claude Code copia il plugin in una cache per versione, quindi se cambia il
/// percorso dell'exe (app spostata), il testo della skill o dell'esploratore o la release (ogni
/// aggiornamento forza la reinstallazione) la versione deve cambiare, altrimenti
/// resterebbe la copia vecchia. Semver valido: il suffisso e' un prerelease alfanumerico.
fn plugin_version(hooks: &str, mcp: &str, skills: &str, explorer: &str) -> String {
    format!("{}-h{:08x}", env!("CARGO_PKG_VERSION"), fnv32(&format!("{hooks}\n{mcp}\n{skills}\n{explorer}")))
}

/// Versione che avrebbe il plugin generato ora per `exe` (come lo vede l'agente).
pub(super) fn plugin_version_for(exe: &str) -> String {
    plugin_version(&hooks_json(exe), &mcp_json(exe), &skills_blob(), &explorer_text())
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
pub(super) fn plugin_files(exe: &str) -> Vec<(String, String)> {
    let hooks = hooks_json(exe);
    let mcp = mcp_json(exe);
    let explorer = explorer_text();
    let version = plugin_json(&plugin_version(&hooks, &mcp, &skills_blob(), &explorer));
    let mut files = vec![
        (".claude-plugin/marketplace.json".to_string(), marketplace_json()),
        (format!("plugins/{PLUGIN}/.claude-plugin/plugin.json"), version),
        (format!("plugins/{PLUGIN}/hooks/hooks.json"), hooks),
        (format!("plugins/{PLUGIN}/.mcp.json"), mcp),
    ];
    files.extend(PRESETS.iter().map(|p| (skill_path(p), p.skill_text())));
    files.push((format!("plugins/{PLUGIN}/agents/{EXPLORER_AGENT}.md"), explorer));
    files
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
            ("Notification", "needs-input", Some("permission_prompt|idle_prompt|elicitation_dialog")),
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
    fn version_depends_on_hooks_mcp_skill_and_explorer() {
        let (hooks, mcp) = (hooks_json("C:/a.exe"), mcp_json("C:/a.exe"));
        let skill = skills_blob();
        let explorer = explorer_text();
        let v1 = plugin_version(&hooks, &mcp, &skill, &explorer);
        assert_eq!(v1, plugin_version(&hooks, &mcp, &skill, &explorer));
        assert_eq!(v1, plugin_version_for("C:/a.exe"));
        // Stessi hook, MCP diverso => versione diversa.
        assert_ne!(v1, plugin_version(&hooks, &mcp_json("C:/b.exe"), &skill, &explorer));
        assert_ne!(v1, plugin_version(&hooks_json("C:/b.exe"), &mcp, &skill, &explorer));
        assert_ne!(v1, plugin_version(&hooks, &mcp, "altra skill", &explorer));
        // Un altro testo dell'esploratore => versione diversa (altrimenti resta la cache vecchia).
        assert_ne!(v1, plugin_version(&hooks, &mcp, &skill, "altro esploratore"));
    }

    #[test]
    fn version_tracks_the_app_release() {
        let v = plugin_version_for("C:/a.exe");
        let (release, hash) = v.split_once("-h").unwrap();
        assert_eq!(release, env!("CARGO_PKG_VERSION"));
        assert_eq!(hash.len(), 8);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        let files = plugin_files("C:/a.exe");
        let (_, manifest) =
            files.iter().find(|(p, _)| p.ends_with(".claude-plugin/plugin.json") && p.starts_with("plugins/")).unwrap();
        let m: serde_json::Value = serde_json::from_str(manifest).unwrap();
        assert_eq!(m["version"], v.as_str());
    }

    #[test]
    fn plugin_ships_one_skill_per_preset_and_nothing_else() {
        let files = plugin_files("C:/a.exe");
        let skills: Vec<_> = files.iter().filter(|(p, _)| p.contains("/skills/")).map(|(p, _)| p.as_str()).collect();
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
    fn plugin_ships_the_read_only_explorer_subagent() {
        let files = plugin_files("C:/a.exe");
        let (_, text) = files
            .iter()
            .find(|(p, _)| p == "plugins/cli-companion-notify/agents/explorer.md")
            .expect("agents/explorer.md nel plugin");
        assert!(!text.contains('\r'));
        let front = text.strip_prefix("---\n").and_then(|t| t.split_once("\n---\n")).expect("frontmatter").0;
        let field = |name: &str| front.lines().find_map(|l| l.strip_prefix(name)).map(str::trim);
        assert_eq!(field("name:"), Some("explorer"));
        assert_eq!(field("model:"), Some("haiku"));
        // Niente Bash: un permesso richiesto fermerebbe un worker non presidiato.
        assert_eq!(field("tools:"), Some("Read, Grep, Glob"));
        assert!(field("description:").is_some_and(|d| !d.is_empty()));
        assert_eq!(files.iter().filter(|(p, _)| p.contains("/agents/")).count(), 1);
    }

    #[test]
    fn old_installs_are_flagged_outdated_by_their_marker() {
        // La versione dipende dal contenuto delle skill: il marcatore di un'installazione
        // con le vecchie skill (o senza marcatore) non coincide e `find_outdated` la segnala.
        let current = plugin_version_for("C:/a.exe");
        let old =
            plugin_version(&hooks_json("C:/a.exe"), &mcp_json("C:/a.exe"), "orchestrate\nworkflow", &explorer_text());
        assert_ne!(current, old);
    }
}
