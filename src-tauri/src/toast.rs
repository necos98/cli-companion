//! Toast Windows cliccabile per gli eventi degli agenti. Il plugin
//! notification non espone il click su desktop: qui si usa direttamente
//! `tauri-winrt-notification` (lo stesso crate sotto notify-rust) con
//! `on_activated`, che porta in primo piano la finestra e avvisa il frontend.

use crate::agent_notify::is_valid_tab_id;

/// Evento Tauri emesso al click: `{ tab_id }`.
#[cfg(windows)]
const ACTIVATED_EVENT: &str = "notification-activated";

/// AUMID della toast. Da installata e' l'identifier dell'app (registrato dal
/// bundler NSIS/MSI); da `target\debug|release` nessuna app e' registrata e si
/// ripiega su quello di PowerShell. Stessa logica di tauri-plugin-notification.
#[cfg(windows)]
fn app_id(identifier: &str) -> String {
    use tauri_winrt_notification::Toast;
    let portable = tauri::utils::platform::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.display().to_string()))
        .is_some_and(|dir| {
            let sep = std::path::MAIN_SEPARATOR;
            dir.ends_with(&format!("{sep}target{sep}debug"))
                || dir.ends_with(&format!("{sep}target{sep}release"))
        });
    if portable {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        identifier.to_string()
    }
}

/// Porta la finestra principale in primo piano.
#[cfg(windows)]
fn bring_to_front(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Mostra la toast per il tab `tab_id`; al click focus + `notification-activated`.
/// Solo Windows: altrove errore, e il frontend ripiega sul plugin.
#[tauri::command]
pub async fn show_agent_notification(
    app: tauri::AppHandle,
    tab_id: String,
    title: String,
    body: String,
) -> Result<(), String> {
    if !is_valid_tab_id(&tab_id) {
        return Err("id tab non valido".to_string());
    }
    #[cfg(windows)]
    {
        // WinRT fuori dal main thread (async + thread bloccante).
        crate::blocking(move || {
            use tauri::Emitter;
            use tauri_winrt_notification::Toast;
            let id = app_id(&app.config().identifier);
            let handle = app.clone();
            Toast::new(&id)
                .title(&title)
                .text1(&body)
                // Il suono lo riproduce gia' l'app.
                .sound(None)
                .on_activated(move |_| {
                    bring_to_front(&handle);
                    let _ = handle.emit(ACTIVATED_EVENT, serde_json::json!({ "tab_id": tab_id }));
                    Ok(())
                })
                .show()
                .map_err(|e| format!("toast non mostrata: {e}"))
        })
        .await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, title, body);
        Err("toast cliccabile solo su Windows".to_string())
    }
}
