//! Esecuzione di un'azione validata sul browser.

use super::action::{resolve_url, Action};
use crate::browser::session::BrowserSession;
use crate::browser::BrowserError;

pub struct Executed {
    pub result: Result<(), BrowserError>,
    pub stale_ref_retry: bool,
    /// Errore non fatale dell'attesa di quiete della pagina.
    pub settle_warning: Option<String>,
}

/// Apre una pagina del sito e attende la quiete, come l'azione `open` dell'agente.
/// Non legge snapshot: il tracker resta intatto e la prima osservazione e' del passo.
/// L'errore dell'attesa di quiete e' non fatale e viene ignorato.
pub fn open_page(session: &mut BrowserSession, path: &str, base_url: &str) -> Result<(), String> {
    let action = Action::Open {
        path: path.to_string(),
    };
    execute(session, &action, base_url)
        .result
        .map_err(|e| e.to_string())
}

/// `done` non passa di qui: chiude il passo senza toccare il browser.
pub fn execute(session: &mut BrowserSession, action: &Action, base_url: &str) -> Executed {
    let (result, stale_ref_retry, settles) = match action {
        Action::Open { path } => (
            session.browser().open(&resolve_url(path, base_url)),
            false,
            true,
        ),
        Action::Click { element } => on_ref(session, element, true, |b, r| b.click(r)),
        Action::Fill { element, text } => on_ref(session, element, false, |b, r| b.fill(r, text)),
        Action::Select { element, value } => {
            on_ref(session, element, true, |b, r| b.select(r, value))
        }
        Action::Press { key } => (session.browser().press(key), false, true),
        Action::Done { .. } => (Ok(()), false, false),
    };
    let settle_warning = if settles && result.is_ok() {
        session
            .browser()
            .wait_settled()
            .err()
            .map(|e| e.to_string())
    } else {
        None
    };
    Executed {
        result,
        stale_ref_retry,
        settle_warning,
    }
}

fn on_ref<F>(
    session: &mut BrowserSession,
    element: &str,
    settles: bool,
    op: F,
) -> (Result<(), BrowserError>, bool, bool)
where
    F: Fn(&crate::browser::AgentBrowser, &str) -> Result<(), BrowserError>,
{
    let attempt = session.run_on_ref(element, op);
    (attempt.result, attempt.stale_ref_retry, settles)
}
