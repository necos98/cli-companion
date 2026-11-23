//! Comandi Tauri di Headroom: impostazioni, stato del proxy e preparazione
//! dell'ambiente di un terminale aperto dal frontend.

use super::health;
use super::launch::launch_env;
use super::settings::{self, HeadroomSettings};
use crate::store::data_dir;
use crate::workspaces::WorkspaceStore;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use ts_rs::TS;

/// Stato del proxy per le Impostazioni.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct HeadroomStatus {
    /// Il proxy risponde sulla porta configurata.
    pub reachable: bool,
    /// Base URL che riceverebbe `ANTHROPIC_BASE_URL`.
    pub url: String,
    pub dashboard_url: String,
}

fn status_of(settings: &HeadroomSettings, reachable: bool) -> HeadroomStatus {
    let url = settings.base_url();
    HeadroomStatus { reachable, dashboard_url: format!("{url}/dashboard"), url }
}

#[tauri::command]
pub fn headroom_settings_get() -> HeadroomSettings {
    settings::current()
}

#[tauri::command]
pub async fn headroom_settings_set(app: AppHandle, settings: HeadroomSettings) -> Result<HeadroomSettings, String> {
    let saved = settings::save(&data_dir(&app)?, settings)?;
    health::invalidate();
    if !saved.enabled {
        close_dashboard(&app);
    }
    Ok(saved)
}

/// Etichetta della finestra della dashboard (nessuna capability: pagina remota, niente IPC).
const DASHBOARD_LABEL: &str = "headroom";

pub fn close_dashboard(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(DASHBOARD_LABEL) {
        let _ = win.destroy();
    }
}

/// Apre la dashboard di Headroom in una finestra propria (il proxy vieta
/// l'incorporamento con `X-Frame-Options: DENY`); se esiste già la porta in
/// primo piano. Asincrono: su Windows creare finestre da un comando sincrono
/// blocca WebView2.
#[tauri::command]
pub async fn headroom_open_dashboard(app: AppHandle) -> Result<(), String> {
    let current = settings::current();
    if !current.enabled {
        return Err("Headroom non è attivo: abilitalo in Impostazioni › Integrazioni.".to_string());
    }
    if let Some(win) = app.get_webview_window(DASHBOARD_LABEL) {
        let _ = win.unminimize();
        let _ = win.show();
        return win.set_focus().map_err(|e| format!("Impossibile mostrare la finestra di Headroom: {e}"));
    }
    let url = status_of(&current, false)
        .dashboard_url
        .parse()
        .map_err(|e| format!("Indirizzo della dashboard di Headroom non valido: {e}"))?;
    WebviewWindowBuilder::new(&app, DASHBOARD_LABEL, WebviewUrl::External(url))
        .title("Headroom")
        .inner_size(1200.0, 800.0)
        .center()
        .build()
        .map(|_| ())
        .map_err(|e| format!("Impossibile aprire la finestra di Headroom: {e}"))
}

#[tauri::command]
pub async fn headroom_status() -> Result<HeadroomStatus, String> {
    crate::blocking(|| {
        let current = settings::current();
        Ok(status_of(&current, health::is_reachable(current.port)))
    })
    .await
}

/// Prepara l'ambiente Headroom del terminale `tab_id` che il frontend sta per
/// aprire con l'avvio dell'agente `agent_id` nel workspace `workspace_id`
/// (`pty_spawn` lo applica una volta sola). Vale per gli avvii integrati e per
/// il loro ripristino; i Bot lo preparano da se'. Restituisce l'avviso, se c'e'.
#[tauri::command]
pub async fn headroom_stage_env(
    app: AppHandle,
    workspace_id: String,
    agent_id: String,
    tab_id: String,
) -> Result<Option<String>, String> {
    crate::blocking(move || {
        if !crate::ipc::is_valid_tab_id(&tab_id) {
            return Err("Identificativo del tab non valido.".to_string());
        }
        let ws = app.state::<WorkspaceStore>().find(&workspace_id).ok_or("Workspace non trovato.")?;
        let launch = launch_env(&agent_id, ws.kind);
        if !launch.env.is_empty() {
            crate::pty::stage_env(&app, &tab_id, launch.env);
        }
        Ok(launch.warning)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_urls_follow_the_port() {
        let s = status_of(&HeadroomSettings { enabled: true, port: 9000 }, true);
        assert_eq!(s.url, "http://127.0.0.1:9000");
        assert_eq!(s.dashboard_url, "http://127.0.0.1:9000/dashboard");
        assert!(s.reachable);
    }
}
