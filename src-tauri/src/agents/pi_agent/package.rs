//! Contratto `IntegrationPackage` di Pi: la cartella dell'estensione (generata
//! da `extension.rs`) copiata in `<agent dir>/extensions/cli-companion-notify/`,
//! dove Pi la scopre da solo.
//!
//! Non si tocca nessun altro file dell'utente (nemmeno `settings.json`). La
//! cartella di destinazione si sovrascrive o si toglie SOLO se il suo
//! `index.ts` porta la nostra intestazione. Una copia dell'estensione e la
//! destinazione scelta restano in `plugin_root` (`destination.txt`), cosi' la
//! rimozione cancella la stessa cartella.

use super::agent_dir::agent_dir;
use super::extension::{extension_files, extension_version_for, is_ours};
use super::version::is_supported;
use super::{PiAgent, AGENT_ID, EXTENSION, SUPPORTED_MAJOR};
use crate::agents::{Detection, InstallCtx, IntegrationPackage, PackageFile};
use crate::platform::Target;
use std::path::{Path, PathBuf};

/// File di `plugin_root` con la cartella in cui l'estensione e' stata installata.
const DESTINATION_RECORD: &str = "destination.txt";
/// Cartella delle estensioni dentro la cartella dell'agente Pi.
const EXTENSIONS_DIR: &str = "extensions";

/// Si procede solo se l'agente c'e' ed e' di una versione supportata.
fn ensure_usable(det: &Detection) -> Result<(), String> {
    if !det.found {
        return Err("Pi non trovato in questo env".to_string());
    }
    if !is_supported(det.version.as_deref()) {
        return Err(format!(
            "Pi {} non e' supportato: serve la versione {SUPPORTED_MAJOR}.x",
            det.version.as_deref().unwrap_or("sconosciuta")
        ));
    }
    Ok(())
}

/// Cartella dell'estensione dentro la cartella dell'agente `agent`.
fn destination_in(agent: &Path) -> PathBuf {
    agent.join(EXTENSIONS_DIR).join(EXTENSION)
}

/// Scrive `files` (path relativi) sotto `dir`.
fn write_files(dir: &Path, files: &[PackageFile]) -> Result<(), String> {
    for PackageFile { path, content } in files {
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("scrittura estensione: {e}"))?;
        }
        std::fs::write(&file, content).map_err(|e| format!("scrittura estensione: {e}"))?;
    }
    Ok(())
}

/// Rifiuta una cartella di destinazione che esiste ma non e' nostra.
fn ensure_not_foreign(destination: &Path) -> Result<(), String> {
    if destination.exists() && !is_ours(destination) {
        return Err(format!(
            "La cartella {} esiste ma non e' stata creata da cli-companion: rinominala o eliminala e riprova.",
            destination.display()
        ));
    }
    Ok(())
}

/// Cartella registrata dall'ultima installazione, se c'e'.
fn recorded_destination(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(DESTINATION_RECORD)).ok()?;
    Some(PathBuf::from(text.trim())).filter(|p| !p.as_os_str().is_empty())
}

/// Dove sta (o starebbe) l'estensione: la cartella registrata o quella derivata dall'agente.
fn known_destination(ctx: &InstallCtx, target: &Target) -> Result<PathBuf, String> {
    if let Some(recorded) = recorded_destination(&ctx.plugin_root(AGENT_ID, target)) {
        return Ok(recorded);
    }
    agent_dir(target).map(|agent| destination_in(&agent))
}

impl IntegrationPackage for PiAgent {
    fn version(&self, ctx: &InstallCtx, target: &Target) -> Option<String> {
        ctx.exe_for(target).ok().map(|exe| extension_version_for(&exe))
    }

    fn files(&self, ctx: &InstallCtx, target: &Target) -> Result<Vec<PackageFile>, String> {
        let exe = ctx.exe_for(target)?;
        Ok(extension_files(&exe).into_iter().map(|(path, content)| PackageFile { path, content }).collect())
    }

    fn is_installed(&self, target: &Target, det: &Detection) -> bool {
        ensure_usable(det).is_ok() && agent_dir(target).is_ok_and(|agent| is_ours(&destination_in(&agent)))
    }

    fn install(&self, ctx: &InstallCtx, target: &Target, det: &Detection) -> Result<String, String> {
        ensure_usable(det)?;
        let files = self.files(ctx, target)?;
        let destination = destination_in(&agent_dir(target)?);
        ensure_not_foreign(&destination)?;

        // Copia in `plugin_root` (la sola cosa che il core conosce) e destinazione registrata.
        let root = ctx.plugin_root(AGENT_ID, target);
        let _ = std::fs::remove_dir_all(&root);
        write_files(&root, &files)?;
        std::fs::write(root.join(DESTINATION_RECORD), destination.to_string_lossy().as_bytes())
            .map_err(|e| format!("scrittura estensione: {e}"))?;

        // Niente avanzi: la cartella (nostra) si rifa da zero.
        if destination.exists() {
            std::fs::remove_dir_all(&destination).map_err(|e| format!("pulizia estensione precedente: {e}"))?;
        }
        write_files(&destination, &files)?;
        let log = [
            format!("estensione scritta in {}", root.display()),
            format!("estensione installata in {}", destination.display()),
            "stato: installato".to_string(),
        ];
        Ok(log.join("\n"))
    }

    fn uninstall(&self, ctx: &InstallCtx, target: &Target, _det: &Detection) -> Result<String, String> {
        let mut log = Vec::new();
        match known_destination(ctx, target) {
            Ok(destination) if !destination.exists() => log.push("estensione gia' assente".to_string()),
            Ok(destination) if is_ours(&destination) => match std::fs::remove_dir_all(&destination) {
                Ok(()) => log.push(format!("estensione rimossa da {}", destination.display())),
                Err(e) => log.push(format!("rimozione estensione: {e}")),
            },
            Ok(destination) => {
                log.push(format!("{} non e' stata creata da cli-companion: lasciata dov'e'", destination.display()))
            }
            Err(e) => {
                log.push(format!("Pi: cartella dell'estensione non determinata ({e}): rimossi solo i file generati"))
            }
        }
        let root = ctx.plugin_root(AGENT_ID, target);
        match std::fs::remove_dir_all(&root) {
            Ok(()) => log.push("file generati eliminati".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log.push(format!("file generati: {e}")),
        }
        Ok(log.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &Path) -> InstallCtx {
        InstallCtx { exe: "C:/a/cli-companion.exe".to_string(), data_dir: dir.to_path_buf() }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mc-pi-package-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn det(version: Option<&str>) -> Detection {
        Detection { found: true, version: version.map(str::to_string), path: Some("pi".to_string()) }
    }

    #[test]
    fn files_are_the_generated_extension_for_the_target_exe() {
        let ctx = ctx(Path::new("unused"));
        let win = PiAgent.files(&ctx, &Target::Windows).unwrap();
        assert!(win.iter().any(|f| f.path == "index.ts" && f.content.contains("C:/a/cli-companion.exe")));
        assert!(win.iter().any(|f| f.path == "skills/task-intake/SKILL.md"));
        let wsl = PiAgent.files(&ctx, &Target::Wsl { distro: "U".into() }).unwrap();
        assert!(wsl.iter().any(|f| f.path == "index.ts" && f.content.contains("/mnt/c/a/cli-companion.exe")));
        let version = |t: &Target| PiAgent.version(&ctx, t).unwrap();
        assert_ne!(version(&Target::Windows), version(&Target::Wsl { distro: "U".into() }));
        // Exe non traducibile per WSL: nessuna versione, nessun file.
        let odd = InstallCtx { exe: "relativo/cc.exe".to_string(), data_dir: "unused".into() };
        let wsl = Target::Wsl { distro: "U".into() };
        assert!(PiAgent.version(&odd, &wsl).is_none() && PiAgent.files(&odd, &wsl).is_err());
    }

    #[test]
    fn only_a_found_and_supported_version_is_usable() {
        assert_eq!(ensure_usable(&det(Some("1.6.0"))), Ok(()));
        let err = ensure_usable(&det(Some("2.0.1"))).unwrap_err();
        assert!(err.contains("2.0.1") && err.contains("1.x"), "{err}");
        assert!(ensure_usable(&det(None)).unwrap_err().contains("sconosciuta"));
        assert!(ensure_usable(&Detection::default()).is_err());
        let not_found = Detection { found: false, ..det(Some("1.6.0")) };
        assert!(ensure_usable(&not_found).is_err());
    }

    #[test]
    fn an_unsupported_version_is_neither_installed_nor_installable() {
        let dir = temp_dir("unsupported");
        let old = det(Some("0.9.0"));
        assert!(!PiAgent.is_installed(&Target::Windows, &old));
        assert!(PiAgent.install(&ctx(&dir), &Target::Windows, &old).unwrap_err().contains("1.x"));
        assert!(!dir.exists(), "nulla scritto");
    }

    #[test]
    fn the_destination_is_the_extension_folder_inside_the_agent_dir() {
        assert_eq!(
            destination_in(Path::new("agent")),
            Path::new("agent").join("extensions").join("cli-companion-notify")
        );
    }

    #[test]
    fn a_foreign_folder_is_refused_and_ours_is_accepted() {
        let dir = temp_dir("foreign");
        let dest = dir.join("dest");
        assert!(ensure_not_foreign(&dest).is_ok(), "assente: va bene");
        std::fs::create_dir_all(&dest).unwrap();
        assert!(ensure_not_foreign(&dest).unwrap_err().contains("non e' stata creata da cli-companion"));
        std::fs::write(dest.join("index.ts"), "export default {}").unwrap();
        assert!(ensure_not_foreign(&dest).is_err());
        let files = PiAgent.files(&ctx(&dir), &Target::Windows).unwrap();
        write_files(&dest, &files).unwrap();
        assert!(ensure_not_foreign(&dest).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_replaces_the_folder_without_leftovers() {
        let dir = temp_dir("leftovers");
        let dest = dir.join("dest");
        let files = PiAgent.files(&ctx(&dir), &Target::Windows).unwrap();
        write_files(&dest, &files).unwrap();
        for stale in ["skills/orchestrate/SKILL.md", "old.ts"] {
            std::fs::create_dir_all(dest.join(stale).parent().unwrap()).unwrap();
            std::fs::write(dest.join(stale), "x").unwrap();
        }
        // Come in `install`: cartella nostra, si rifa da zero.
        assert!(is_ours(&dest));
        std::fs::remove_dir_all(&dest).unwrap();
        write_files(&dest, &files).unwrap();
        assert!(!dest.join("old.ts").exists() && !dest.join("skills/orchestrate").exists());
        assert!(dest.join("skills/task-intake/SKILL.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_recorded_destination_is_read_back() {
        let dir = temp_dir("record");
        assert_eq!(recorded_destination(&dir), None);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(DESTINATION_RECORD), "  \n").unwrap();
        assert_eq!(recorded_destination(&dir), None);
        std::fs::write(dir.join(DESTINATION_RECORD), "C:\\Users\\me\\.pi\\agent\\extensions\\cli-companion-notify\n")
            .unwrap();
        assert_eq!(
            recorded_destination(&dir),
            Some(PathBuf::from("C:\\Users\\me\\.pi\\agent\\extensions\\cli-companion-notify"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uninstall_removes_our_folder_by_the_record_and_the_generated_files() {
        let dir = temp_dir("uninstall");
        let ctx = ctx(&dir);
        let target = Target::Windows;
        let dest = dir.join("agent").join("extensions").join(EXTENSION);
        write_files(&dest, &PiAgent.files(&ctx, &target).unwrap()).unwrap();
        let root = ctx.plugin_root(AGENT_ID, &target);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(DESTINATION_RECORD), dest.to_string_lossy().as_bytes()).unwrap();
        let log = PiAgent.uninstall(&ctx, &target, &Detection::default()).unwrap();
        assert!(!dest.exists() && !root.exists(), "{log}");
        assert!(log.contains("estensione rimossa") && log.contains("file generati eliminati"), "{log}");
        assert!(dir.join("agent").join("extensions").exists(), "solo la nostra cartella");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uninstall_never_removes_a_folder_that_is_not_ours() {
        let dir = temp_dir("uninstall-foreign");
        let ctx = ctx(&dir);
        let target = Target::Windows;
        let dest = dir.join("extensions").join(EXTENSION);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("index.ts"), "// mine").unwrap();
        let root = ctx.plugin_root(AGENT_ID, &target);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(DESTINATION_RECORD), dest.to_string_lossy().as_bytes()).unwrap();
        let log = PiAgent.uninstall(&ctx, &target, &det(Some("1.6.0"))).unwrap();
        assert!(dest.join("index.ts").exists(), "{log}");
        assert!(log.contains("lasciata dov'e'"), "{log}");
        assert!(!root.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
