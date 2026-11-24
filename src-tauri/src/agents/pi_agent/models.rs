//! Contratto `ModelCatalog` di Pi: i modelli sono `provider/id` (o un pattern)
//! di uno dei tanti provider configurati dall'utente.

use super::PiAgent;
use crate::agents::{ModelCatalog, SuggestedModel};
use crate::validate::{valid_model_shape, MODEL_MAX};

/// Separa il suffisso di ragionamento dal modello (`modello:high`).
const THINKING_SEPARATOR: char = ':';

impl ModelCatalog for PiAgent {
    /// Pi e' multi-provider: i modelli disponibili dipendono dalle chiavi e dai
    /// provider dell'utente, quindi non esiste un catalogo fisso da proporre.
    fn suggested(&self) -> &'static [SuggestedModel] {
        &[]
    }

    /// Il modello lo decide la configurazione di Pi dell'utente.
    fn default_model(&self) -> Option<&'static str> {
        None
    }

    /// Forma generica sicura, al massimo `MODEL_MAX` byte (come gli altri
    /// nomi di modello dell'app) e senza `:`: il suffisso di ragionamento e' la
    /// impostazione separata dell'effort (`--thinking`) e qui andrebbe in conflitto.
    fn is_valid(&self, model: &str) -> bool {
        valid_model_shape(model) && model.len() <= MODEL_MAX && !model.contains(THINKING_SEPARATOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_no_fixed_catalog_and_leaves_the_default_to_pi() {
        assert!(PiAgent.suggested().is_empty());
        assert_eq!(PiAgent.default_model(), None);
    }

    #[test]
    fn accepts_provider_slash_model_forms() {
        for ok in ["deepseek/deepseek-v4-flash", "openrouter/vendor/x", "a/b", "model-1.0", "gpt_5.1"] {
            assert!(PiAgent.is_valid(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_the_thinking_suffix() {
        for bad in ["x:high", "p/m:low", "a:b/c", ":"] {
            assert!(!PiAgent.is_valid(bad), "{bad:?}");
        }
    }

    #[test]
    fn rejects_unsafe_names() {
        for bad in ["", "-a", "/x", "x/", "a//b", "a b", "a;ls", "a$b", "a\"b", "a`b", "a%b", "a!b", "a\nb", "a\\b"] {
            assert!(!PiAgent.is_valid(bad), "{bad:?}");
        }
        assert!(!PiAgent.is_valid(&"m".repeat(MODEL_MAX + 1)));
        assert!(PiAgent.is_valid(&"m".repeat(MODEL_MAX)));
    }
}
