//! Contratto di avvio interattivo: aprire un terminale dell'agente con modello,
//! effort, skill e istruzioni di sistema scelti da un Bot (non da stringhe
//! statiche, come i preset).

use super::Effort;

/// La skill da avviare come primo prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchSkill<'a> {
    /// Una skill del plugin (id di `agents::presets::PRESETS`).
    Plugin(&'a str),
    /// Una skill globale dell'utente (nome in `user_skills`).
    User(&'a str),
}

/// Cosa chiede il Bot all'avvio. Il core consegna `effort` solo se
/// `EffortLevels` dell'agente lo ammette; un valore digitato nella riga di
/// comando (modello, nome della skill) passa da `is_safe_launch_token`.
#[derive(Debug, Clone, Copy)]
pub struct InteractiveRequest<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<Effort>,
    pub skill: Option<LaunchSkill<'a>>,
    /// File di istruzioni di sistema da aggiungere a quelle dell'agente per
    /// tutta la sessione, ripristino compreso. Lo scrive l'app nella sua
    /// cartella dati e arriva nel formato di path del target; l'integrazione lo
    /// rifiuta se non passa `is_shell_safe_path`. Il core lo consegna solo se
    /// `InteractiveLauncher::applies_instructions` e' vero.
    pub instructions_file: Option<&'a str>,
}

/// Come si avvia il terminale interattivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractiveLaunch {
    /// Riga digitata nella shell del tab (PowerShell, cmd o bash): contiene
    /// modello, effort e skill.
    pub command: String,
    /// Riga salvata nella sessione per il ripristino: tiene modello ed effort
    /// ma MAI la skill, perche' riprende la conversazione invece di rieseguirla.
    pub restore_command: String,
    /// Variabili impostate dal core sul processo del tab prima che la shell
    /// parta, con le stesse regole di `AutomationLaunch::env`.
    pub env: Vec<(String, String)>,
    /// Variabili che un tab ripristinato dopo un riavvio rimette sul processo,
    /// quando `restore_command` non basta a conservare le scelte del Bot (es. un
    /// modello che passa da una variabile). Mai la skill; stesse regole di `env`.
    pub restore_env: Vec<(String, String)>,
}

/// Avvio interattivo guidato dalla richiesta di un Bot.
pub trait InteractiveLauncher: Sync {
    /// Come avviare l'agente per `req`. `None` = richiesta non rappresentabile
    /// in modo sicuro (caratteri non ammessi nel modello, nella skill o nel
    /// file di istruzioni).
    fn launch(&self, req: &InteractiveRequest) -> Option<InteractiveLaunch>;

    /// L'agente applica `InteractiveRequest::instructions_file`, sia in
    /// `command`/`env` sia in `restore_command`/`restore_env`. Se e' falso il
    /// core non lo consegna e avvisa l'utente.
    fn applies_instructions(&self) -> bool;
}

/// Un valore digitato nella riga di comando (modello, nome della skill) e'
/// sicuro in PowerShell, cmd e bash: lo stesso insieme di caratteri di `--model`.
pub fn is_safe_launch_token(value: &str) -> bool {
    crate::validate::valid_model(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_tokens_are_plain_names() {
        for good in ["opus", "task-intake", "my_skill.v2", "claude-opus-4.1", &"a".repeat(64)] {
            assert!(is_safe_launch_token(good), "{good:?}");
        }
        for bad in ["", "a b", "a$b", "a\"b", "a%b", "a!b", "a`b", "a/b", "a;b", &"a".repeat(65)] {
            assert!(!is_safe_launch_token(bad), "{bad:?}");
        }
    }
}
