//! Tool MCP per le skill globali dell'utente (`save_skill`, `list_skills`,
//! `delete_skill`) e ripristino all'avvio. Le skill non appartengono a un
//! workspace: non passano da `boards::rpc_session`. Restano pero' dietro lo
//! stesso congelamento di `dispatch` (tab di un workspace archiviato) e sono
//! vietate ai worker. La logica su file e' in `crate::user_skills`.

use super::*;
use crate::user_skills::{self, SkillInput};
use serde_json::json;

pub const SKILL_METHODS: [&str; 3] = ["save_skill", "list_skills", "delete_skill"];

pub(super) fn rpc<H: Host>(host: &H, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "save_skill" => save_skill(host, params),
        "list_skills" => list_skills(host),
        "delete_skill" => delete_skill(host, params),
        _ => Err(format!("unknown method: {method}")),
    }
}

fn save_skill<H: Host>(host: &H, params: &Value) -> Result<Value, String> {
    let input: SkillInput =
        serde_json::from_value(params.clone()).map_err(|e| format!("invalid parameters: {e}"))?;
    user_skills::validate(&input)?;
    let targets = host.skill_targets();
    // Prima di scrivere qualunque cosa: una skill scritta a mano non si tocca.
    for dir in targets.iter().filter_map(|t| t.dir.as_ref().ok()) {
        user_skills::check_target_free(dir, &input.name)?;
    }
    let store = host.skills_store_dir()?;
    let saved = user_skills::save(&store, &input)?;
    let synced: Vec<Value> = targets
        .iter()
        .map(|t| outcome(&t.env, t.dir.clone().and_then(|dir| user_skills::sync_one(&store, &saved.name, &dir))))
        .collect();
    Ok(json!({
        "name": saved.name,
        "invocation": format!("/{}", saved.name),
        "store_path": store.join(&saved.name).display().to_string(),
        "synced": synced,
    }))
}

fn list_skills<H: Host>(host: &H) -> Result<Value, String> {
    let skills = user_skills::list(&host.skills_store_dir()?)?;
    serde_json::to_value(skills).map_err(|e| e.to_string())
}

fn delete_skill<H: Host>(host: &H, params: &Value) -> Result<Value, String> {
    let name = req_str(params, "name")?;
    user_skills::delete(&host.skills_store_dir()?, name)?;
    let removed: Vec<Value> = host
        .skill_targets()
        .iter()
        .map(|t| outcome(&t.env, t.dir.clone().and_then(|dir| user_skills::remove_from(&dir, name)).map(drop)))
        .collect();
    Ok(json!({ "name": name, "removed": removed }))
}

/// `{env, ok}` o `{env, ok: false, error}`.
fn outcome(env: &str, result: Result<(), String>) -> Value {
    match result {
        Ok(()) => json!({ "env": env, "ok": true }),
        Err(error) => json!({ "env": env, "ok": false, "error": error }),
    }
}

/// Ripristina in background le skill dell'archivio nelle cartelle target
/// (svuotate o cancellate dall'utente), anche quelle delle distro WSL in uso:
/// risolvere i target puo' essere lento, per questo tutto gira nel thread. Gli
/// errori si loggano soltanto: non devono impedire l'avvio dell'app.
pub fn restore_in_background<H: Host>(host: H) {
    std::thread::spawn(move || {
        let store = match host.skills_store_dir() {
            Ok(store) => store,
            Err(e) => return eprintln!("user skills: {e}"),
        };
        for target in host.skill_targets() {
            let reports = target.dir.and_then(|dir| user_skills::sync_all(&store, &dir));
            match reports {
                Ok(reports) => reports
                    .into_iter()
                    .filter_map(|r| r.result.err().map(|e| format!("user skills ({}): {}: {e}", target.env, r.name)))
                    .for_each(|line| eprintln!("{line}")),
                Err(e) => eprintln!("user skills ({}): {e}", target.env),
            }
        }
    });
}
