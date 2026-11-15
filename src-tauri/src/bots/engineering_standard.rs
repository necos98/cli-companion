//! Lo standard di ingegneria dei Bot avviati in un tab. Un Bot con `work` fra
//! i suoi `kinds` riceve la variante interattiva (`INTERACTIVE_STANDARD`) come
//! istruzioni di sistema: l'app la scrive in un file della sua cartella dati e
//! l'agente la applica con il suo meccanismo (contratto `InteractiveLauncher`).
//! I Bot senza `work` (Intake, Setup, solo `review` o `plan`) non la ricevono.

use super::agent::AgentBot;
use crate::agents::{is_shell_safe_path, InteractiveLauncher};
use crate::board::texts::INTERACTIVE_STANDARD;
use crate::board::TaskKind;
use crate::platform::{windows_to_wsl_path, TargetKind};
use std::path::{Path, PathBuf};

/// Cartella, nella cartella dati dell'app, dei file di istruzioni dei Bot.
const INSTRUCTIONS_DIR: &str = "bot-instructions";

/// Il file con la variante interattiva dello standard.
const STANDARD_FILE_NAME: &str = "engineering-standard.md";

/// Il file dello standard pronto per il tab, nel formato di path del target, o
/// perche' non si e' potuto preparare (in inglese, come gli avvisi dei Bot).
pub type StandardFile = Result<String, String>;

/// Cosa riceve il Bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BotStandard<'a> {
    /// Il Bot non ha `work`: niente standard.
    NotNeeded,
    /// L'agente non sa applicare istruzioni di sistema.
    Unsupported,
    /// Il file non si e' potuto preparare, con il motivo.
    Unavailable(&'a str),
    /// Il file da consegnare all'agente.
    File(&'a str),
}

impl<'a> BotStandard<'a> {
    pub(super) fn of(bot: &AgentBot, launcher: &dyn InteractiveLauncher, file: &'a StandardFile) -> Self {
        if !bot.runs(TaskKind::Work) {
            return BotStandard::NotNeeded;
        }
        if !launcher.applies_instructions() {
            return BotStandard::Unsupported;
        }
        match file {
            Ok(path) => BotStandard::File(path),
            Err(reason) => BotStandard::Unavailable(reason),
        }
    }

    /// Il file per `InteractiveRequest::instructions_file`.
    pub(super) fn file(self) -> Option<&'a str> {
        match self {
            BotStandard::File(path) => Some(path),
            _ => None,
        }
    }

    /// L'avviso dell'avvio (in inglese) quando un Bot `work` parte senza lo standard.
    pub(super) fn launch_warning(self, agent_name: &str) -> Option<String> {
        match self {
            BotStandard::Unsupported => Some(format!(
                "{agent_name} does not support system instructions: the engineering standard is not applied"
            )),
            BotStandard::Unavailable(reason) => {
                Some(format!("the engineering standard could not be prepared ({reason}): not applied"))
            }
            BotStandard::NotNeeded | BotStandard::File(_) => None,
        }
    }
}

/// Scrive (se e' cambiato) il file dello standard interattivo in `data_dir` e
/// ne restituisce il path come lo vede `kind`. Il path viene solo dall'app ed
/// e' rifiutato se non e' sicuro fra doppi apici in PowerShell, cmd e bash.
pub fn prepare_standard_file(data_dir: &Path, kind: TargetKind) -> StandardFile {
    let file = standard_file_path(data_dir);
    write_if_changed(&file, &INTERACTIVE_STANDARD)?;
    let path = path_for_target(&file, kind)?;
    if !is_shell_safe_path(&path) {
        return Err(format!("the app data path contains characters that are unsafe in a shell command line: {path}"));
    }
    Ok(path)
}

fn standard_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INSTRUCTIONS_DIR).join(STANDARD_FILE_NAME)
}

/// Scrittura atomica, saltata se il file ha gia' il contenuto: piu' tab che
/// partono insieme scrivono lo stesso testo senza pestarsi.
fn write_if_changed(file: &Path, content: &str) -> Result<(), String> {
    if std::fs::read_to_string(file).is_ok_and(|current| current == content) {
        return Ok(());
    }
    let fail = |e: std::io::Error| format!("writing {}: {e}", file.display());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    crate::store::write_atomic(file, content).map_err(fail)
}

fn path_for_target(file: &Path, kind: TargetKind) -> Result<String, String> {
    let win = file.to_string_lossy().to_string();
    match kind {
        TargetKind::Windows => Ok(win),
        TargetKind::Wsl => windows_to_wsl_path(&win).ok_or_else(|| format!("path not reachable from WSL: {win}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::testing::FakeIntegration;
    use crate::bots::presets::{self, GENERAL_ID};

    fn preset(id: &str) -> AgentBot {
        presets::find(id).unwrap().bot()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cli-companion-standard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_bots_with_work_need_the_standard() {
        let full = FakeIntegration::full("fake");
        let file: StandardFile = Ok("C:/data/std.md".into());
        assert_eq!(BotStandard::of(&preset(GENERAL_ID), &full, &file), BotStandard::File("C:/data/std.md"));
        for id in ["intake", "setup", "skill-wizard"] {
            assert_eq!(BotStandard::of(&preset(id), &full, &file), BotStandard::NotNeeded, "{id}");
        }
        for kinds in [vec![TaskKind::Review], vec![TaskKind::Plan], vec![]] {
            let bot = AgentBot { kinds: kinds.clone(), ..preset(GENERAL_ID) };
            assert_eq!(BotStandard::of(&bot, &full, &file), BotStandard::NotNeeded, "{kinds:?}");
        }
    }

    #[test]
    fn a_bot_with_work_that_cannot_get_the_standard_is_warned() {
        let general = preset(GENERAL_ID);
        let ok: StandardFile = Ok("C:/data/std.md".into());
        let unsupported = BotStandard::of(&general, &FakeIntegration::full("fake").without_instructions(), &ok);
        assert_eq!(unsupported, BotStandard::Unsupported);
        assert_eq!(unsupported.file(), None);
        assert_eq!(
            unsupported.launch_warning("Fake").as_deref(),
            Some("Fake does not support system instructions: the engineering standard is not applied")
        );
        let failed: StandardFile = Err("disk full".into());
        let unavailable = BotStandard::of(&general, &FakeIntegration::full("fake"), &failed);
        assert_eq!(unavailable, BotStandard::Unavailable("disk full"));
        assert_eq!(
            unavailable.launch_warning("Fake").as_deref(),
            Some("the engineering standard could not be prepared (disk full): not applied")
        );
        assert_eq!(BotStandard::File("x").launch_warning("Fake"), None);
        assert_eq!(BotStandard::NotNeeded.launch_warning("Fake"), None);
    }

    #[test]
    fn the_file_holds_the_interactive_standard_and_is_rewritten_when_stale() {
        let dir = temp_dir("write");
        let path = prepare_standard_file(&dir, TargetKind::Windows).unwrap();
        let file = standard_file_path(&dir);
        assert_eq!(path, file.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), *INTERACTIVE_STANDARD);
        std::fs::write(&file, "stale").unwrap();
        assert_eq!(prepare_standard_file(&dir, TargetKind::Windows).unwrap(), path);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), *INTERACTIVE_STANDARD);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_path_follows_the_target() {
        let data = Path::new(r"C:\Users\Mario Rossi\AppData\Roaming\cli-companion");
        assert_eq!(
            path_for_target(&standard_file_path(data), TargetKind::Wsl).unwrap(),
            "/mnt/c/Users/Mario Rossi/AppData/Roaming/cli-companion/bot-instructions/engineering-standard.md"
        );
        assert!(path_for_target(Path::new("relative/std.md"), TargetKind::Wsl).is_err());
    }

    #[test]
    fn an_unsafe_data_path_is_refused() {
        let dir = temp_dir("unsafe!");
        let error = prepare_standard_file(&dir, TargetKind::Windows).unwrap_err();
        assert!(error.contains("unsafe in a shell command line"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
