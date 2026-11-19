//! Comandi Tauri dell'automazione, invocati dal frontend.

use super::*;

/// Il frontend ha chiuso un tab automatico (utente, app o shell uscita).
#[tauri::command]
pub async fn automation_tab_closed(app: AppHandle, tab_id: String) -> Result<(), String> {
    if !is_valid_tab_id(&tab_id) {
        return Err("id tab non valido".to_string());
    }
    // Il registro puo' essere occupato (callback dell'host): fuori dal main thread.
    crate::blocking(move || {
        let st = app.state::<AutomationState>();
        st.run(&TauriHost(app.clone()), |r| r.tab_closed(&tab_id, now_millis()));
        Ok(())
    })
    .await
}

/// Worker con tab aperto: il frontend ricostruisce lo stato (hot reload).
#[tauri::command]
pub async fn automation_sessions(app: AppHandle) -> Result<Vec<AutomationTabInfo>, String> {
    crate::blocking(move || Ok(app.state::<AutomationState>().open_tabs())).await
}

/// Board del workspace (vuota se non esiste).
#[tauri::command]
pub async fn board_get(app: AppHandle, workspace_id: String) -> Result<BoardView, String> {
    crate::blocking(move || app.state::<AutomationState>().view(&TauriHost(app.clone()), &workspace_id)).await
}

/// Imposta `max_parallel` e/o la pausa della coda.
#[tauri::command]
pub async fn board_set(
    app: AppHandle,
    workspace_id: String,
    max_parallel: Option<u8>,
    paused: Option<bool>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::board_set(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, max_parallel, paused)
    })
    .await
}

/// Nuovo task creato a mano (senza proposta): `NewTaskSpec`, con il goal aperto facoltativo.
#[tauri::command]
pub async fn task_create(app: AppHandle, workspace_id: String, spec: Value) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_create(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &spec)
    })
    .await
}

/// Modifica di un task `queued` o `failed` (campi di `spec`, gli assenti restano).
#[tauri::command]
pub async fn task_update(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    patch: Value,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_update(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &task_id, &patch)
    })
    .await
}

/// Azione dell'utente su un task (vedi `board::TaskAction`: `retry`, `mark_done`,
/// `stop`, `delete`, `delete_cascade`, `to_top`).
#[tauri::command]
pub async fn task_action(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    action: board::TaskAction,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_action(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &task_id, action)
    })
    .await
}

/// Riordina un task `queued`: prima di `before_id`, o in fondo.
#[tauri::command]
pub async fn task_move(
    app: AppHandle,
    workspace_id: String,
    task_id: String,
    before_id: Option<String>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::task_move(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &task_id,
            before_id.as_deref(),
        )
    })
    .await
}

/// "Chiudi goal": elimina i task `queued` e `failed` del goal, scarta le sue
/// proposte e lo chiude `abandoned` (errore se un suo task e' in corso).
#[tauri::command]
pub async fn goal_close(app: AppHandle, workspace_id: String, goal_id: String) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::goal_close(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &goal_id)
    })
    .await
}

/// "Elimina branch": cancella i branch `goal/<id>` di un goal abbandonato (errore
/// se e' aperto o consegnato, o se git ne rifiuta uno).
#[tauri::command]
pub async fn goal_delete_branches(app: AppHandle, workspace_id: String, goal_id: String) -> Result<BoardView, String> {
    crate::blocking(move || {
        let (st, host) = (app.state::<AutomationState>(), TauriHost(app.clone()));
        boards::goal_delete_branches(&st, &host, &workspace_id, &goal_id)
    })
    .await
}

/// Aggiunge una nota dell'utente al registro delle decisioni di un goal aperto.
#[tauri::command]
pub async fn goal_add_note(
    app: AppHandle,
    workspace_id: String,
    goal_id: String,
    text: String,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::goal_add_note(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &goal_id, &text)
    })
    .await
}

/// Accetta o scarta una proposta dell'Intake. Con una proposta accettata che crea
/// un goal nuovo, `auto_deliver` e' la sua consegna automatica e `auto_fix` i suoi
/// giri di correzione automatica (0..=3): errore se attivi e la proposta non crea
/// un goal.
#[tauri::command]
pub async fn proposal_decide(
    app: AppHandle,
    workspace_id: String,
    proposal_id: String,
    accept: bool,
    auto_deliver: Option<bool>,
    auto_fix: Option<u8>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        let acceptance = board::Acceptance { auto_deliver, auto_fix, automatic: false };
        boards::proposal_decide_with(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &proposal_id,
            accept,
            acceptance,
        )
    })
    .await
}

/// Giri di correzione automatica di un goal aperto (0..=3), dal dettaglio del goal.
#[tauri::command]
pub async fn goal_set_auto_fix(
    app: AppHandle,
    workspace_id: String,
    goal_id: String,
    max_rounds: u8,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::goal_set_auto_fix(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &goal_id,
            max_rounds,
        )
    })
    .await
}

/// "Consegna" / "Consegna comunque": crea il task di consegna del goal pronto
/// (anche se l'ultima review ha trovato problemi: e' la scelta dell'utente).
#[tauri::command]
pub async fn goal_deliver(app: AppHandle, workspace_id: String, goal_id: String) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::goal_deliver(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, &goal_id)
    })
    .await
}

/// Impostazioni di consegna di un goal aperto: consegna automatica e/o Bot di
/// consegna (acceso, con `work` fra i tipi di task).
#[tauri::command]
pub async fn goal_set_delivery(
    app: AppHandle,
    workspace_id: String,
    goal_id: String,
    auto_deliver: Option<bool>,
    bot: Option<String>,
) -> Result<BoardView, String> {
    crate::blocking(move || {
        boards::goal_set_delivery(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &goal_id,
            auto_deliver,
            bot,
        )
    })
    .await
}

/// Approva o scarta una proposta di impostazioni dell'agente.
#[tauri::command]
pub async fn settings_proposal_decide(
    app: AppHandle,
    workspace_id: String,
    proposal_id: String,
    accept: bool,
) -> Result<settings::SettingsDecision, String> {
    crate::blocking(move || {
        settings::settings_proposal_decide(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            &proposal_id,
            accept,
        )
    })
    .await
}

/// Repository del workspace (root se e' un repo, sottocartelle con `.git`):
/// per la scelta del repository dei task. Git/disco fuori dal thread principale.
#[tauri::command]
pub async fn workspace_detect_repos(app: AppHandle, workspace_id: String) -> Result<Vec<String>, String> {
    crate::blocking(move || {
        let host = TauriHost(app);
        let ws = host.workspace(&workspace_id).ok_or_else(|| "workspace non trovato".to_string())?;
        host.list_repos(&ws)
    })
    .await
}

/// Lancia a mano gli E2E di un repository (`None` = la root del workspace):
/// ritorna subito l'esecuzione `running`; l'esito arriva con l'evento `e2e-update`.
#[tauri::command]
pub async fn e2e_run_repo(app: AppHandle, workspace_id: String, repo: Option<String>) -> Result<board::E2eRun, String> {
    crate::blocking(move || {
        e2e::manual::run_repo(&app.state::<AutomationState>(), &TauriHost(app.clone()), &workspace_id, repo.as_deref())
    })
    .await
}

/// Ultimo esito degli E2E lanciati a mano per il repository (solo in memoria).
#[tauri::command]
pub async fn e2e_last_result(
    app: AppHandle,
    workspace_id: String,
    repo: Option<String>,
) -> Result<Option<board::E2eRun>, String> {
    crate::blocking(move || {
        e2e::manual::last_result(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            repo.as_deref(),
        )
    })
    .await
}

/// Apre il report JUnit dell'ultima esecuzione E2E del task `task_id` o, senza,
/// del repository `repo`, con l'applicazione predefinita di Windows.
#[tauri::command]
pub async fn e2e_open_report(
    app: AppHandle,
    workspace_id: String,
    task_id: Option<String>,
    repo: Option<String>,
) -> Result<(), String> {
    crate::blocking(move || {
        let file = e2e::report::report_file(
            &app.state::<AutomationState>(),
            &TauriHost(app.clone()),
            &workspace_id,
            task_id.as_deref(),
            repo.as_deref(),
        )?;
        crate::workspaces::open_in_explorer(file.as_os_str())
    })
    .await
}
