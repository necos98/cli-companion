pub mod agent_notify;
mod agents;
pub mod automation;
mod awake;
pub mod board;
mod launch_settings;
pub mod mcp;
mod pty;
pub mod schedule;
mod session;
mod toast;
pub mod updater;
mod workspaces;
mod wsl;

use automation::AutomationState;
use pty::PtyState;
use tauri::Manager;
use workspaces::WorkspaceStore;

/// Identifier della vecchia app (muse-companion): la cartella dati dipende
/// dall'identifier, quindi al primo avvio se ne copiano i file.
const LEGACY_IDENTIFIER: &str = "com.jmagarelli.muse-companion";
const DATA_FILES: [&str; 1] = ["workspaces.json"];

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
    // Nome unico per scrittura: due salvataggi concorrenti dello stesso file
    // (debounce + chiusura) non devono pestarsi il temporaneo.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{n}.tmp"));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(WorkspaceStore::default())
        .manage(PtyState::default())
        .manage(launch_settings::LaunchSettingsStore::default())
        .manage(AutomationState::default())
        .setup(|app| {
            migrate_legacy_data(app);
            workspaces::load_from_disk(app);
            launch_settings::load_from_disk(app);
            automation::load_boards(app);
            automation::auto::init(app);
            agent_notify::start_listener(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workspaces::list_workspaces,
            workspaces::add_workspace,
            workspaces::rename_workspace,
            workspaces::set_worker_permission,
            workspaces::set_worker_model,
            workspaces::set_worktree_setup,
            workspaces::set_auto_orchestrate,
            workspaces::remove_workspace,
            workspaces::add_launch,
            workspaces::update_launch,
            workspaces::remove_launch,
            workspaces::set_default_launch,
            launch_settings::get_launch_settings,
            launch_settings::set_global_default_launch,
            agents::builtin_launches,
            workspaces::open_workspace_folder,
            wsl::list_wsl_distros,
            wsl::check_wsl_path,
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_close,
            agent_notify::test_agent_notify,
            automation::automation_tab_closed,
            automation::automation_orchestrator_open,
            automation::automation_sessions,
            automation::board_get,
            automation::board_set,
            automation::board_task_action,
            automation::board_task_set_model,
            automation::auto::schedule_get,
            automation::auto::schedule_set,
            automation::auto::workflow_get,
            automation::auto::workflow_save,
            toast::show_agent_notification,
            agents::agent_env_status,
            agents::agent_profiles,
            agents::install_agent_plugin,
            agents::remove_agent_plugin,
            session::load_session,
            session::save_session,
            session::save_scrollback,
            session::load_scrollback,
            session::delete_scrollback,
            session::prune_scrollback,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Salvataggi di board ancora in coda (coalescenza).
            if let tauri::RunEvent::Exit = event {
                automation::flush_on_exit(app);
            }
        });
}
