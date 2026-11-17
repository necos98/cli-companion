//! La posizione di un workspace (ambiente, distro, percorso): la verifica di
//! quella scelta dall'utente, il controllo dei duplicati e il cambio della
//! cartella collegata. Il cambio tocca solo il collegamento: nessun file si
//! sposta o si copia.

use super::commands::reconcile_automation;
use super::model::Workspace;
use super::store::{WorkspaceStore, WORKSPACE_NOT_FOUND};
use crate::automation::AutomationState;
use crate::board::{relocation_blockers, relocation_refusal, Task};
use crate::platform::TargetKind;
use crate::store::data_dir;
use tauri::{AppHandle, Manager};

/// Errore di una posizione gia' collegata a un workspace.
pub(super) const LOCATION_TAKEN: &str = "workspace già presente";

/// Posizione verificata: la cartella esiste (su Windows) o si raggiunge con
/// `wsl.exe` (WSL, con la distro).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Location {
    pub kind: TargetKind,
    pub path: String,
    pub distro: Option<String>,
}

impl Location {
    /// Verifica la posizione scelta. Su WSL si accetta anche un percorso
    /// Windows o `\\wsl$\...`, convertito in percorso Linux. Bloccante: su WSL
    /// avvia `wsl.exe`.
    pub(super) fn resolve(kind: TargetKind, path: String, distro: Option<String>) -> Result<Self, String> {
        let mut path = path.trim().to_string();
        if path.is_empty() {
            return Err("percorso mancante".to_string());
        }
        match kind {
            TargetKind::Wsl => {
                let distro = distro.unwrap_or_default().trim().to_string();
                if distro.is_empty() {
                    return Err(super::target::WSL_DISTRO_MISSING.to_string());
                }
                if !path.starts_with('/') {
                    path = crate::platform::windows_to_wsl_path(&path)
                        .ok_or_else(|| "il percorso WSL deve essere assoluto (es. /home/user/proj)".to_string())?;
                }
                crate::wsl::ensure_wsl_dir(&distro, &path)?;
                Ok(Self { kind, path, distro: Some(distro) })
            }
            TargetKind::Windows => {
                let meta = std::fs::metadata(&path).map_err(|_| format!("cartella non trovata: {path}"))?;
                if !meta.is_dir() {
                    return Err(format!("non è una cartella: {path}"));
                }
                Ok(Self { kind, path, distro: None })
            }
        }
    }

    /// Il workspace e' collegato a questa posizione.
    fn is_of(&self, ws: &Workspace) -> bool {
        ws.kind == self.kind && ws.path == self.path && ws.distro == self.distro
    }

    /// Collega `ws` a questa posizione. Cambiando ambiente la shell torna
    /// quella predefinita: una shell Windows non vale in WSL e viceversa.
    fn apply_to(self, ws: &mut Workspace) {
        if ws.kind != self.kind {
            ws.shell = None;
        }
        ws.kind = self.kind;
        ws.path = self.path;
        ws.distro = self.distro;
    }
}

/// Rifiuta una posizione gia' collegata a un workspace diverso da `except`.
pub(super) fn ensure_free(list: &[Workspace], location: &Location, except: Option<&str>) -> Result<(), String> {
    let taken = list.iter().any(|w| Some(w.id.as_str()) != except && location.is_of(w));
    if taken {
        return Err(LOCATION_TAKEN.to_string());
    }
    Ok(())
}

/// Collega il workspace `id` a `location`. La posizione attuale si accetta
/// sempre (nessun cambio); un'altra si rifiuta se un task la blocca
/// (`blockers`, vedi `board::relocation_blockers`) o se e' di un altro workspace.
fn relocate(list: &mut [Workspace], id: &str, location: Location, blockers: &[&Task]) -> Result<Workspace, String> {
    let index = list.iter().position(|w| w.id == id).ok_or_else(|| WORKSPACE_NOT_FOUND.to_string())?;
    if location.is_of(&list[index]) {
        return Ok(list[index].clone());
    }
    if let Some(refusal) = relocation_refusal(blockers) {
        return Err(refusal);
    }
    ensure_free(list, &location, Some(id))?;
    location.apply_to(&mut list[index]);
    Ok(list[index].clone())
}

/// Cambia la cartella (e l'ambiente) del workspace, che tiene tutto il resto:
/// id, nome, avvii, impostazioni, consegna, repository e task. I terminali gia'
/// aperti restano dove sono; i nuovi partono dalla nuova posizione.
/// Async: la verifica della cartella WSL avvia `wsl.exe`.
#[tauri::command]
pub async fn set_workspace_location(
    app: AppHandle,
    id: String,
    kind: String,
    path: String,
    distro: Option<String>,
) -> Result<Workspace, String> {
    crate::blocking(move || {
        let location = Location::resolve(TargetKind::parse(&kind)?, path, distro)?;
        let dir = data_dir(&app)?;
        // Task controllati e workspace salvato sotto il registro: nel frattempo
        // lo scheduler non puo' avviare un task nella vecchia cartella.
        let out = app.state::<AutomationState>().with_board_tasks(&id, |tasks| {
            let blockers = relocation_blockers(tasks);
            app.state::<WorkspaceStore>().update(&dir, |list| relocate(list, &id, location, &blockers))
        })?;
        reconcile_automation(&app);
        Ok(out)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::WorktreeInfo;
    use crate::workspaces::Launch;

    const TASK_TITLE: &str = "Migrazione";

    fn ws(id: &str, kind: TargetKind, path: &str, distro: Option<&str>) -> Workspace {
        let mut w = Workspace::new(id.to_string(), format!("nome {id}"), kind, path.to_string());
        w.distro = distro.map(str::to_string);
        w
    }

    fn windows(path: &str) -> Location {
        Location { kind: TargetKind::Windows, path: path.to_string(), distro: None }
    }

    fn wsl(distro: &str, path: &str) -> Location {
        Location { kind: TargetKind::Wsl, path: path.to_string(), distro: Some(distro.to_string()) }
    }

    fn list() -> Vec<Workspace> {
        vec![ws("a", TargetKind::Windows, "C:\\a", None), ws("b", TargetKind::Wsl, "/home/me/b", Some("Ubuntu"))]
    }

    fn task_with_worktree() -> Task {
        let mut task = Task::default();
        task.id = "t1".to_string();
        task.title = TASK_TITLE.to_string();
        task.worktree = Some(WorktreeInfo {
            path: "C:\\a\\.worktrees\\t1".to_string(),
            branch: "task/t1".to_string(),
            base: "main".to_string(),
            setup_done: true,
        });
        task
    }

    #[test]
    fn a_location_needs_a_path_a_distro_for_wsl_and_an_existing_folder() {
        let err = |kind, path: &str, distro: Option<&str>| {
            Location::resolve(kind, path.to_string(), distro.map(str::to_string)).unwrap_err()
        };
        assert_eq!(err(TargetKind::Windows, "  ", None), "percorso mancante");
        let missing = err(TargetKind::Windows, "Z:\\non\\esiste\\davvero", None);
        assert!(missing.starts_with("cartella non trovata"), "{missing}");
        let tmp = std::env::temp_dir();
        let found = Location::resolve(TargetKind::Windows, format!(" {} ", tmp.display()), None).unwrap();
        assert_eq!(found, windows(tmp.to_str().unwrap()));
        let not_dir = err(TargetKind::Windows, &std::env::current_exe().unwrap().display().to_string(), None);
        assert!(not_dir.starts_with("non è una cartella"), "{not_dir}");
        assert_eq!(err(TargetKind::Wsl, "/home/me", Some("  ")), super::super::target::WSL_DISTRO_MISSING);
        assert_eq!(err(TargetKind::Wsl, "/home/me", None), super::super::target::WSL_DISTRO_MISSING);
    }

    #[test]
    fn a_location_of_another_workspace_is_taken_but_its_own_is_not() {
        let list = list();
        assert_eq!(ensure_free(&list, &windows("C:\\a"), None).unwrap_err(), LOCATION_TAKEN);
        assert_eq!(ensure_free(&list, &windows("C:\\a"), Some("b")).unwrap_err(), LOCATION_TAKEN);
        assert_eq!(ensure_free(&list, &windows("C:\\a"), Some("a")), Ok(()));
        assert_eq!(ensure_free(&list, &wsl("Ubuntu", "/home/me/b"), Some("a")).unwrap_err(), LOCATION_TAKEN);
        // Stesso percorso su un'altra distro o in un altro ambiente: posizione diversa.
        assert_eq!(ensure_free(&list, &wsl("Debian", "/home/me/b"), None), Ok(()));
        assert_eq!(ensure_free(&list, &windows("/home/me/b"), None), Ok(()));
    }

    #[test]
    fn relocating_changes_only_the_location_and_keeps_everything_else() {
        let mut list = list();
        let before = {
            let a = &mut list[0];
            a.shell = Some("pwsh".to_string());
            a.agent = Some("codex".to_string());
            a.use_schedule = true;
            a.launches.push(Launch { id: "ln-1".into(), name: "dev".into(), command: "npm run dev".into() });
            a.clone()
        };
        let out = relocate(&mut list, "a", windows("D:\\nuova"), &[]).unwrap();
        assert_eq!(out, list[0]);
        assert_eq!(out, Workspace { path: "D:\\nuova".to_string(), ..before });
    }

    #[test]
    fn changing_environment_resets_the_shell_and_keeping_it_does_not() {
        let mut list = list();
        list[0].shell = Some("pwsh".to_string());
        let same_kind = relocate(&mut list, "a", windows("D:\\x"), &[]).unwrap();
        assert_eq!(same_kind.shell.as_deref(), Some("pwsh"));
        let to_wsl = relocate(&mut list, "a", wsl("Debian", "/srv/x"), &[]).unwrap();
        assert_eq!(
            (to_wsl.kind, to_wsl.path.as_str(), to_wsl.distro.as_deref()),
            (TargetKind::Wsl, "/srv/x", Some("Debian"))
        );
        assert_eq!(to_wsl.shell, None);

        list[1].shell = Some("zsh".to_string());
        let to_windows = relocate(&mut list, "b", windows("C:\\b"), &[]).unwrap();
        assert_eq!((to_windows.kind, to_windows.distro), (TargetKind::Windows, None));
        assert_eq!(to_windows.shell, None);
    }

    #[test]
    fn a_taken_location_or_a_blocking_task_refuses_without_changes() {
        let mut list = list();
        let before = list.clone();
        assert_eq!(relocate(&mut list, "a", wsl("Ubuntu", "/home/me/b"), &[]).unwrap_err(), LOCATION_TAKEN);
        let task = task_with_worktree();
        let refusal = relocate(&mut list, "a", windows("D:\\x"), &[&task]).unwrap_err();
        assert!(refusal.contains(TASK_TITLE), "{refusal}");
        assert_eq!(relocate(&mut list, "x", windows("D:\\x"), &[]).unwrap_err(), WORKSPACE_NOT_FOUND);
        assert_eq!(list, before);
    }

    #[test]
    fn the_current_location_is_accepted_even_with_blocking_tasks() {
        let mut list = list();
        let task = task_with_worktree();
        let out = relocate(&mut list, "a", windows("C:\\a"), &[&task]).unwrap();
        assert_eq!(out, list[0]);
    }
}
