//! Interruttore di disponibilita' dell'Agents HUB. L'HUB era un'implementazione
//! sperimentale e non e' piu' usato: finche' `HUB_AVAILABLE` e' `false` resta
//! spento ovunque (avvio, feed, segretari, notifiche, connettori), anche se il
//! file delle impostazioni dice `enabled: true`. Il file non viene mai toccato.

/// TEMPORANEO: `false` disabilita l'integrazione dell'Agents HUB. Per
/// riattivarla basta metterlo a `true` (nessun'altra modifica: il codice
/// dell'HUB e le impostazioni salvate restano intatti).
pub const HUB_AVAILABLE: bool = false;

/// Il messaggio mostrato all'utente quando prova ad accendere l'HUB.
pub const HUB_UNAVAILABLE: &str =
    "L'integrazione dell'Agents HUB è momentaneamente disabilitata: era un'implementazione sperimentale.";

/// Lo stato effettivo dell'interruttore: salvato e disponibile.
pub fn effective(available: bool, saved: bool) -> bool {
    available && saved
}

/// Esito della richiesta di accendere (`enabled`) o spegnere l'HUB: accenderlo
/// con l'HUB non disponibile e' un errore, spegnerlo e' sempre innocuo.
pub fn check_request(available: bool, enabled: bool) -> Result<(), String> {
    if enabled && !available {
        Err(HUB_UNAVAILABLE.to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_enabled_flag_does_not_turn_on_an_unavailable_hub() {
        assert!(!effective(false, true));
        assert!(!effective(false, false));
        assert!(!effective(true, false));
        assert!(effective(true, true));
    }

    #[test]
    fn enabling_is_refused_only_when_unavailable() {
        assert_eq!(check_request(false, true), Err(HUB_UNAVAILABLE.to_string()));
        assert_eq!(check_request(false, false), Ok(()));
        assert_eq!(check_request(true, true), Ok(()));
        assert_eq!(check_request(true, false), Ok(()));
    }

    #[test]
    fn the_notice_says_the_integration_is_disabled() {
        assert!(HUB_UNAVAILABLE.contains("momentaneamente disabilitata"));
    }
}
