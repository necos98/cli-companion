//! Sessione di browser: wrapper + tracker degli snapshot + gestione dei ref scaduti.

use super::snapshot::{SnapshotTracker, SnapshotView};
use super::{AgentBrowser, BrowserError};

/// Stato della pagina osservato dopo (o prima di) un'azione.
#[derive(Clone, Debug)]
pub struct PageObservation {
    pub url: String,
    pub title: String,
    pub snapshot: SnapshotView,
    pub page_text: String,
}

/// Esito di un'azione su un ref, con l'eventuale ripetizione per ref scaduto.
#[derive(Debug)]
pub struct RefAttempt {
    pub result: Result<(), BrowserError>,
    pub stale_ref_retry: bool,
}

pub struct BrowserSession {
    browser: AgentBrowser,
    tracker: SnapshotTracker,
    delta: bool,
}

impl BrowserSession {
    pub fn new(browser: AgentBrowser, delta: bool) -> Self {
        BrowserSession {
            browser,
            tracker: SnapshotTracker::new(),
            delta,
        }
    }

    pub fn browser(&self) -> &AgentBrowser {
        &self.browser
    }

    pub fn observe(&mut self) -> Result<PageObservation, BrowserError> {
        Ok(PageObservation {
            url: self.browser.url()?,
            title: self.browser.title()?,
            snapshot: self.take_snapshot()?,
            page_text: self.browser.page_text()?,
        })
    }

    /// Esegue `op` sul ref; se e' scaduto rilegge lo snapshot e, solo se il ref
    /// esiste ancora, riprova una volta.
    pub fn run_on_ref<F>(&mut self, element: &str, op: F) -> RefAttempt
    where
        F: Fn(&AgentBrowser, &str) -> Result<(), BrowserError>,
    {
        let first = op(&self.browser, element);
        let Err(BrowserError::StaleRef(message)) = first else {
            return RefAttempt {
                result: first,
                stale_ref_retry: false,
            };
        };
        let still_present = self
            .take_snapshot()
            .map(|view| view.refs.contains(element))
            .unwrap_or(false);
        self.tracker.mark_unreported();
        if still_present {
            RefAttempt {
                result: op(&self.browser, element),
                stale_ref_retry: true,
            }
        } else {
            RefAttempt {
                result: Err(BrowserError::StaleRef(message)),
                stale_ref_retry: false,
            }
        }
    }

    fn take_snapshot(&mut self) -> Result<SnapshotView, BrowserError> {
        let data = self.browser.snapshot(self.delta)?;
        self.tracker.apply(&data).map_err(BrowserError::BadOutput)
    }
}
