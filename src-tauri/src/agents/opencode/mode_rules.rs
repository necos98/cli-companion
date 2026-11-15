//! Regole di permesso (sintassi v2: `{action, resource, effect}`) e agente di
//! partenza di ogni `PermissionMode`, nel JSON di `OPENCODE_CONFIG_CONTENT`.
//!
//! L'ultima regola che corrisponde vince, e le regole di un agente
//! (`agents.<agent>.permissions`, dove stanno quelle del workspace) vengono
//! DOPO tutte quelle di primo livello: le regole del workspace battono
//! sempre quelle di modalita'.

use super::MCP_SERVER;
use crate::agents::PermissionMode;
use serde_json::{json, Value};

/// Agenti incorporati di OpenCode.
pub(super) const AGENT_BUILD: &str = "build";
pub(super) const AGENT_PLAN: &str = "plan";

pub(super) const EFFECT_ALLOW: &str = "allow";
const EFFECT_ASK: &str = "ask";
pub(super) const EFFECT_DENY: &str = "deny";

/// Risorsa che corrisponde a tutto.
pub(super) const ANY_RESOURCE: &str = "*";
const ACTION_READ: &str = "read";
const ACTION_EDIT: &str = "edit";
/// Con il "Code Mode" predefinito il modello chiama i tool MCP tramite `execute`.
const ACTIONS_ALLOWED_BY_DEFAULT: [&str; 6] = [ACTION_READ, "glob", "grep", "skill", "question", ACTION_EXECUTE];
const ACTION_EXECUTE: &str = "execute";
/// File di ambiente: si chiede sempre prima di leggerli, tranne gli esempi.
const ENV_FILES_ASK: [&str; 2] = ["*.env", "*.env.*"];
const ENV_EXAMPLE_FILES: &str = "*.env.example";

/// Chiavi del JSON di configurazione.
const KEY_MODEL: &str = "model";
const KEY_DEFAULT_AGENT: &str = "default_agent";
const KEY_PERMISSIONS: &str = "permissions";
/// File di istruzioni aggiuntive (path assoluti o glob), in coda agli `AGENTS.md`.
const KEY_INSTRUCTIONS: &str = "instructions";

/// Una regola nel formato JSON di OpenCode.
pub(super) fn rule(action: &str, resource: &str, effect: &str) -> Value {
    json!({ "action": action, "resource": resource, "effect": effect })
}

/// Azione di permesso che copre tutti i tool del server MCP del plugin.
fn mcp_action() -> String {
    format!("{MCP_SERVER}_*")
}

fn allow(action: &str) -> Value {
    rule(action, ANY_RESOURCE, EFFECT_ALLOW)
}

/// Chiede tutto tranne le azioni di lettura/ricerca, le skill, le domande, il
/// tool `execute` e il server MCP; i file `.env` restano da confermare.
fn ask_base_rules() -> Vec<Value> {
    let mut rules = vec![rule(ANY_RESOURCE, ANY_RESOURCE, EFFECT_ASK)];
    rules.extend(ACTIONS_ALLOWED_BY_DEFAULT.iter().map(|action| allow(action)));
    rules.push(allow(&mcp_action()));
    rules.extend(ENV_FILES_ASK.iter().map(|file| rule(ACTION_READ, file, EFFECT_ASK)));
    rules.push(rule(ACTION_READ, ENV_EXAMPLE_FILES, EFFECT_ALLOW));
    rules
}

/// Agente di partenza e regole di primo livello di una modalita'.
pub(super) fn mode_rules(mode: PermissionMode) -> (&'static str, Vec<Value>) {
    match mode {
        PermissionMode::Plan => (AGENT_PLAN, ask_base_rules()),
        PermissionMode::Default => (AGENT_BUILD, ask_base_rules()),
        PermissionMode::AcceptEdits => {
            let mut rules = ask_base_rules();
            rules.push(allow(ACTION_EDIT));
            (AGENT_BUILD, rules)
        }
        // Con `--auto` OpenCode approva da solo quel che non e' negato: restano
        // solo il server MCP e `execute` (Code Mode) espliciti.
        PermissionMode::Bypass => (AGENT_BUILD, vec![allow(ACTION_EXECUTE), allow(&mcp_action())]),
    }
}

/// Contenuto compatto di `OPENCODE_CONFIG_CONTENT`.
pub(super) fn config_content(mode: PermissionMode, model: Option<&str>) -> String {
    let (agent, rules) = mode_rules(mode);
    let mut config = json!({ KEY_DEFAULT_AGENT: agent, KEY_PERMISSIONS: rules });
    if let Some(model) = model {
        config[KEY_MODEL] = json!(model);
    }
    config.to_string()
}

/// `OPENCODE_CONFIG_CONTENT` di un tab interattivo: il modello e il file di
/// istruzioni, se dati, e nient'altro (niente permessi ne' agente di partenza,
/// cosi' il tab conserva la configurazione dell'utente). `None` se non c'e'
/// niente da impostare. Le istruzioni si aggiungono a quelle delle altre
/// configurazioni e agli `AGENTS.md` (documentazione di `instructions`).
pub(super) fn interactive_content(model: Option<&str>, instructions_file: Option<&str>) -> Option<String> {
    let mut config = serde_json::Map::new();
    if let Some(model) = model {
        config.insert(KEY_MODEL.to_string(), json!(model));
    }
    if let Some(file) = instructions_file {
        config.insert(KEY_INSTRUCTIONS.to_string(), json!([file]));
    }
    (!config.is_empty()).then(|| Value::Object(config).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(mode: PermissionMode) -> Vec<Value> {
        mode_rules(mode).1
    }

    fn triples(rules: &[Value]) -> Vec<(String, String, String)> {
        let text = |r: &Value, k: &str| r[k].as_str().unwrap().to_string();
        rules.iter().map(|r| (text(r, "action"), text(r, "resource"), text(r, "effect"))).collect()
    }

    #[test]
    fn default_and_plan_share_the_ask_base_with_a_different_agent() {
        let base = triples(&rules(PermissionMode::Default));
        let t = |a: &str, r: &str, e: &str| (a.to_string(), r.to_string(), e.to_string());
        assert_eq!(
            base,
            [
                t("*", "*", "ask"),
                t("read", "*", "allow"),
                t("glob", "*", "allow"),
                t("grep", "*", "allow"),
                t("skill", "*", "allow"),
                t("question", "*", "allow"),
                t("execute", "*", "allow"),
                t("cli-companion_*", "*", "allow"),
                t("read", "*.env", "ask"),
                t("read", "*.env.*", "ask"),
                t("read", "*.env.example", "allow"),
            ]
        );
        assert_eq!(mode_rules(PermissionMode::Default).0, "build");
        assert_eq!(mode_rules(PermissionMode::Plan).0, "plan");
        assert_eq!(triples(&rules(PermissionMode::Plan)), base);
    }

    #[test]
    fn accept_edits_adds_the_edit_allowance_last() {
        let base = rules(PermissionMode::Default);
        let accept = rules(PermissionMode::AcceptEdits);
        assert_eq!(&accept[..base.len()], &base[..]);
        assert_eq!(triples(&accept[base.len()..]), [("edit".into(), "*".into(), "allow".into())]);
        assert_eq!(mode_rules(PermissionMode::AcceptEdits).0, "build");
    }

    #[test]
    fn bypass_allows_only_execute_and_the_mcp_server() {
        let (agent, rules) = mode_rules(PermissionMode::Bypass);
        assert_eq!(agent, "build");
        let t = |a: &str| (a.to_string(), "*".to_string(), "allow".to_string());
        assert_eq!(triples(&rules), [t("execute"), t("cli-companion_*")]);
    }

    #[test]
    fn the_mcp_server_is_allowed_without_confirmation_in_every_mode() {
        for mode in PermissionMode::ALL {
            let all = triples(&rules(mode));
            let mcp = all.iter().rposition(|(a, _, _)| a == "cli-companion_*").expect("regola MCP");
            let catch_all = all.iter().rposition(|(a, r, e)| a == "*" && r == "*" && e == "ask");
            assert!(catch_all.is_none_or(|c| c < mcp), "{mode:?}: la regola MCP deve seguire il catch-all");
            assert_eq!(all[mcp].2, "allow");
        }
    }

    #[test]
    fn config_content_is_compact_json_with_the_model_only_when_given() {
        let with = config_content(PermissionMode::Plan, Some("opencode/gpt-5.5"));
        assert!(!with.contains('\n') && !with.contains(' '), "{with}");
        let v: Value = serde_json::from_str(&with).unwrap();
        assert_eq!(v["model"], "opencode/gpt-5.5");
        assert_eq!(v["default_agent"], "plan");
        assert_eq!(v["permissions"].as_array().unwrap(), &rules(PermissionMode::Plan));
        let none: Value = serde_json::from_str(&config_content(PermissionMode::Default, None)).unwrap();
        assert!(none.get("model").is_none());
        assert_eq!(none["default_agent"], "build");
    }

    #[test]
    fn interactive_content_carries_only_the_model_and_the_instructions() {
        assert_eq!(
            interactive_content(Some("opencode/gpt-5.5"), None).as_deref(),
            Some(r#"{"model":"opencode/gpt-5.5"}"#)
        );
        let file = r"C:\Users\Mario Rossi\std.md";
        let both: Value = serde_json::from_str(&interactive_content(Some("p/m"), Some(file)).unwrap()).unwrap();
        assert_eq!(both, json!({ "model": "p/m", "instructions": [file] }));
        assert_eq!(
            interactive_content(None, Some("/mnt/c/x.md")).as_deref(),
            Some(r#"{"instructions":["/mnt/c/x.md"]}"#)
        );
        assert_eq!(interactive_content(None, None), None);
    }
}
