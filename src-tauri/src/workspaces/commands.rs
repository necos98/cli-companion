//! Comandi Tauri sui workspace (elenco, creazione, impostazioni, ordine,
//! archiviazione, rimozione). Ognuno valida l'input, poi modifica e salva con
//! `store::edit`; i comandi sugli avvii sono in `launches`, il cambio di cartella
//! in `location`.

use super::delivery::Delivery;
use super::location::{ensure_free, Location};
use super::model::Workspace;
use super::repo_config::RepoConfig;
use super::store::{change, edit, now_millis, WorkspaceStore, WORKSPACE_ID_PREFIX, WORKSPACE_NOT_FOUND};
use super::validate::{
    normalize_agent_worker, normalize_rules, normalize_workspace_agent, parse_worker_permission, repo_config_key,
};
use super::worker_settings::AgentWorkerSettings;
use crate::automation::{AutomationState, TauriHost};
use crate::platform::TargetKind;
use crate::store::data_dir;
use std::collections::HashSet;
use tauri::{AppHandle, Manager, State};

/// Nome di un workspace senza nome e senza cartella riconoscibile.
const FALLBACK_NAME: &str = "workspace";

#[tauri::command]
pub fn list_workspaces(state: State<WorkspaceStore>) -> Vec<Workspace> {
    state.all()
}

/// Async: la verifica della cartella WSL avvia `wsl.exe`, che a VM spenta
/// puo' impiegare secondi (sul main thread congelerebbe la finestra).
#[tauri::command]
pub async fn add_workspace(
    app: AppHandle,
    name: String,
    kind: String,
    path: String,
    distro: Option<String>,
    shell: Option<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let location = Location::resolve(TargetKind::parse(&kind)?, path, distro)?;
        let dir = data_dir(&app)?;
        let store = app.state::<WorkspaceStore>();
        store.update(&dir, |list| {
            ensure_free(list, &location, None)?;
            let Location { kind, path, distro } = location;
            let name = if name.trim().is_empty() { default_name_for(&path) } else { name.trim().to_string() };
            let mut ws = Workspace::new(store.next_id(WORKSPACE_ID_PREFIX), name, kind, path);
            ws.distro = distro;
            ws.shell = shell.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            ws.created_at = now_millis();
            list.push(ws.clone());
            Ok(ws)
        })
    })
    .await
}

/// Nome predefinito: l'ultimo componente del percorso.
fn default_name_for(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(FALLBACK_NAME)
        .to_string()
}

#[tauri::command]
pub async fn rename_workspace(app: AppHandle, id: String, name: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("nome mancante".to_string());
        }
        change(&app, &id, |ws| ws.name = name)
    })
    .await
}

/// Imposta il tetto dei permessi dei worker automatici del workspace.
#[tauri::command]
pub async fn set_worker_permission(app: AppHandle, id: String, mode: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let mode = parse_worker_permission(&mode)?;
        change(&app, &id, |ws| ws.worker.permission = mode)
    })
    .await
}

/// Imposta l'agente dei worker e dei revisori del workspace (`None` o vuoto =
/// il predefinito globale).
#[tauri::command]
pub async fn set_workspace_agent(app: AppHandle, id: String, agent: Option<String>) -> Result<Workspace, String> {
    crate::blocking(move || {
        let agent = normalize_workspace_agent(agent)?;
        change(&app, &id, |ws| ws.agent = agent)
    })
    .await
}

/// Cambia le impostazioni dei worker dell'agente `agent`: `update` modifica la
/// voce attuale (vuota se non c'e'), il risultato si valida con le regole
/// dell'agente e si salva; una voce rimasta vuota viene tolta.
fn edit_agent_worker(
    app: &AppHandle,
    id: &str,
    agent: &str,
    update: impl FnOnce(&mut AgentWorkerSettings),
) -> Result<Workspace, String> {
    edit(app, id, |ws| {
        let mut settings = ws.worker.for_agent(agent).cloned().unwrap_or_default();
        update(&mut settings);
        ws.worker.set_agent(agent, normalize_agent_worker(agent, settings)?);
        Ok(())
    })
}

/// Imposta le regole di permesso pre-approvate (`allow`) e vietate (`deny`)
/// dei worker dell'agente `agent`, nella sintassi di quell'agente.
#[tauri::command]
pub async fn set_worker_permission_rules(
    app: AppHandle,
    id: String,
    agent: String,
    allow: Vec<String>,
    deny: Vec<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        edit_agent_worker(&app, &id, &agent, |settings| {
            settings.allow = allow;
            settings.deny = deny;
        })
    })
    .await
}

/// Imposta il modello di default dei worker automatici dell'agente `agent`
/// (vuoto = quello dell'agente).
#[tauri::command]
pub async fn set_worker_model(
    app: AppHandle,
    id: String,
    agent: String,
    model: Option<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || edit_agent_worker(&app, &id, &agent, |settings| settings.model = model)).await
}

/// Regole per i worker del workspace (vuoto = nessuna).
#[tauri::command]
pub async fn set_worker_rules(app: AppHandle, workspace_id: String, rules: String) -> Result<Workspace, String> {
    crate::blocking(move || {
        let rules = normalize_rules(&rules)?;
        change(&app, &workspace_id, |ws| ws.worker.rules = rules)
    })
    .await
}

/// Imposta la modalita' di consegna (vale per i task che partono da ora).
#[tauri::command]
pub async fn set_delivery(app: AppHandle, id: String, delivery: Delivery) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = change(&app, &id, |ws| ws.delivery = delivery.normalized())?;
        reconcile_automation(&app);
        Ok(out)
    })
    .await
}

/// I task del workspace partono solo nelle fasce orarie.
#[tauri::command]
pub async fn set_use_schedule(app: AppHandle, id: String, value: bool) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = change(&app, &id, |ws| ws.use_schedule = value)?;
        reconcile_automation(&app);
        Ok(out)
    })
    .await
}

/// Configurazione di un repository del workspace (`None` la rimuove).
#[tauri::command]
pub async fn set_repo_config(
    app: AppHandle,
    workspace_id: String,
    repo: String,
    config: Option<RepoConfig>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let key = repo_config_key(&repo)?;
        let config = config.map(RepoConfig::normalized).transpose()?;
        edit(&app, &workspace_id, |ws| {
            match config {
                Some(c) => ws.repos.insert(key, c),
                None => ws.repos.remove(&key),
            };
            Ok(())
        })
    })
    .await
}

/// Nuovo ordine dei workspace: `ids` elenca tutti gli id o solo un
/// sottoinsieme (i non elencati seguono, nel loro ordine relativo).
fn apply_order(list: &mut Vec<Workspace>, ids: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        if !list.iter().any(|w| &w.id == id) {
            return Err(WORKSPACE_NOT_FOUND.to_string());
        }
        if !seen.insert(id.as_str()) {
            return Err("id duplicato".to_string());
        }
    }
    let mut rest = std::mem::take(list);
    for id in ids {
        if let Some(i) = rest.iter().position(|w| &w.id == id) {
            list.push(rest.remove(i));
        }
    }
    list.extend(rest);
    Ok(())
}

/// Riordina i workspace e salva; ritorna l'elenco completo.
#[tauri::command]
pub async fn reorder_workspaces(app: AppHandle, ids: Vec<String>) -> Result<Vec<Workspace>, String> {
    crate::blocking(move || {
        let dir = data_dir(&app)?;
        app.state::<WorkspaceStore>().update(&dir, |list| {
            apply_order(list, &ids)?;
            Ok(list.clone())
        })
    })
    .await
}

/// Archivia / riattiva un workspace. Archiviato = congelato: la board non
/// avvia task; i task in corso tornano in coda
/// (i tab li chiude il frontend).
#[tauri::command]
pub async fn set_workspace_archived(app: AppHandle, workspace_id: String, archived: bool) -> Result<Workspace, String> {
    crate::blocking(move || {
        let out = change(&app, &workspace_id, |ws| ws.archived = archived)?;
        // Fuori dal lock dei workspace: il registro li rilegge.
        let automation = app.state::<AutomationState>();
        automation.set_archived(&TauriHost(app.clone()), &workspace_id, archived);
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn remove_workspace(app: AppHandle, id: String) -> Result<(), String> {
    crate::blocking(move || {
        let dir = data_dir(&app)?;
        let mut removed = None;
        app.state::<WorkspaceStore>().update(&dir, |list| {
            let position = list.iter().position(|w| w.id == id).ok_or_else(|| WORKSPACE_NOT_FOUND.to_string())?;
            removed = Some(list.remove(position));
            Ok(())
        })?;
        // Fuori dal lock dei workspace: la board lo rilegge per schedulare. La
        // copia serve a ripulire i worktree dei suoi task: lo store non lo ha piu'.
        if let Some(ws) = removed {
            app.state::<AutomationState>().remove_board(&TauriHost(app.clone()), &ws);
            crate::hub::forget_workspace(&app, &ws.id);
        }
        Ok(())
    })
    .await
}

/// Dopo il salvataggio di consegna, fasce orarie o posizione lo scheduler va
/// riallineato. Da chiamare fuori dal lock dei workspace: lo scheduler li rilegge.
pub(super) fn reconcile_automation(app: &AppHandle) {
    let automation = app.state::<AutomationState>();
    automation.reconcile(&TauriHost(app.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws_id(id: &str) -> Workspace {
        Workspace::new(id.to_string(), id.to_string(), TargetKind::Windows, "C:\\p".to_string())
    }

    fn order(list: &[Workspace]) -> Vec<&str> {
        list.iter().map(|w| w.id.as_str()).collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reorder_applies_a_permutation_or_a_visible_subset() {
        let mut list = vec![ws_id("a"), ws_id("b"), ws_id("c"), ws_id("d")];
        apply_order(&mut list, &ids(&["d", "c", "b", "a"])).unwrap();
        assert_eq!(order(&list), ["d", "c", "b", "a"]);
        // Sottoinsieme: i non elencati seguono nel loro ordine relativo.
        apply_order(&mut list, &ids(&["a", "c"])).unwrap();
        assert_eq!(order(&list), ["a", "c", "d", "b"]);
        apply_order(&mut list, &[]).unwrap();
        assert_eq!(order(&list), ["a", "c", "d", "b"]);
    }

    #[test]
    fn reorder_rejects_unknown_and_duplicate_ids_without_changes() {
        let mut list = vec![ws_id("a"), ws_id("b")];
        assert_eq!(apply_order(&mut list, &ids(&["b", "x"])).unwrap_err(), WORKSPACE_NOT_FOUND);
        assert_eq!(apply_order(&mut list, &ids(&["b", "b"])).unwrap_err(), "id duplicato");
        assert_eq!(order(&list), ["a", "b"]);
    }

    #[test]
    fn default_name_is_the_last_path_component() {
        assert_eq!(default_name_for("C:\\work\\proj"), "proj");
        assert_eq!(default_name_for("/home/me/proj/"), "proj");
        assert_eq!(default_name_for("C:\\work\\proj\\\\"), "proj");
        assert_eq!(default_name_for("/"), FALLBACK_NAME);
        assert_eq!(default_name_for(""), FALLBACK_NAME);
    }
}
