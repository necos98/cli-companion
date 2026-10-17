pub mod muse_notify;
mod pty;
mod settings;
pub mod updater;
mod workspaces;
mod wsl;

use pty::PtyState;
use settings::SettingsStore;
use tauri::Manager;
use workspaces::WorkspaceStore;

/// Identifier della vecchia app (muse-companion): la cartella dati dipende
/// dall'identifier, quindi al primo avvio se ne copiano i file.
const LEGACY_IDENTIFIER: &str = "com.jmagarelli.muse-companion";
const DATA_FILES: [&str; 2] = ["workspaces.json", "settings.json"];

/// Esegue lavoro bloccante (processi esterni, rete, filesystem) fuori dal
/// main thread. I comandi Tauri sincroni girano sul main thread: un comando
/// lento li' congela l'intera finestra (era il blocco all'avvio causato dal
/// controllo aggiornamenti via curl).
pub(crate) async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("operazione interrotta: {e}"))?
}

/// Scrittura atomica: file temporaneo + rename, cosi' un crash a meta'
/// salvataggio non lascia un JSON troncato (che al riavvio verrebbe scartato).
pub(crate) fn write_atomic(path: &std::path::Path, data: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)
}

/// Copia i dati della vecchia installazione (muse-companion) se la cartella
/// dati nuova non li ha ancora. I file legacy restano intatti.
fn migrate_legacy_data(app: &tauri::App) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let Some(legacy) = dir.parent().map(|p| p.join(LEGACY_IDENTIFIER)) else {
        return;
    };
    if legacy == dir || !legacy.is_dir() {
        return;
    }
    for name in DATA_FILES {
        let (from, to) = (legacy.join(name), dir.join(name));
        if from.is_file() && !to.exists() && std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::copy(&from, &to);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(WorkspaceStore::default())
        .manage(PtyState::default())
        .manage(SettingsStore::default())
        .setup(|app| {
            migrate_legacy_data(app);
            workspaces::load_from_disk(app);
            settings::load_from_disk(app);
            muse_notify::start_listener(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workspaces::list_workspaces,
            workspaces::add_workspace,
            workspaces::rename_workspace,
            workspaces::remove_workspace,
            wsl::list_wsl_distros,
            wsl::check_wsl_path,
            settings::get_settings,
            settings::set_default_command,
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            updater::get_update_config,
            updater::check_update,
            updater::download_update,
            updater::install_update,
            muse_notify::muse_env_status,
            muse_notify::install_muse_plugin,
            muse_notify::remove_muse_plugin,
            muse_notify::test_muse_notify,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
