//! Proposte pendenti di modifica delle impostazioni del workspace: l'utente le
//! approva o le scarta. Parte PURA come il resto della board; il confronto con
//! i valori attuali si calcola al momento della vista.

use super::{Board, MAX_PENDING_PROPOSALS};
use crate::workspaces::{SettingsPatch, Workspace};
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// Proposta pendente di impostazioni, con la motivazione dell'agente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsProposal {
    pub id: String,
    /// Tab che l'ha proposta.
    pub source_tab: String,
    pub created_at: i64,
    pub reason: String,
    pub patch: SettingsPatch,
}

/// Proposta esposta: la patch piu' cio' che serve per decidere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsProposalView {
    #[serde(flatten)]
    pub proposal: SettingsProposal,
    /// Valori attuali dei soli campi toccati (vuoto se il workspace manca).
    pub current: SettingsPatch,
    /// Campi che eseguono comandi o allargano i permessi.
    pub sensitive: Vec<String>,
}

impl SettingsProposal {
    pub fn view(&self, ws: Option<&Workspace>) -> SettingsProposalView {
        SettingsProposalView {
            proposal: self.clone(),
            current: ws.map(|w| self.patch.current(w)).unwrap_or_default(),
            sensitive: self.patch.sensitive(),
        }
    }
}

impl Board {
    /// Nuova proposta di impostazioni; il tetto delle pendenti e' separato da
    /// quello delle proposte di task.
    pub fn add_settings_proposal(
        &mut self,
        id: String,
        source_tab: &str,
        reason: String,
        patch: SettingsPatch,
        now: i64,
    ) -> Result<(), String> {
        if self.settings_proposals.len() >= MAX_PENDING_PROPOSALS {
            return Err(format!(
                "too many pending settings proposals (max {MAX_PENDING_PROPOSALS}): wait for the user to approve or discard some"
            ));
        }
        self.settings_proposals.push(SettingsProposal {
            id,
            source_tab: source_tab.to_string(),
            created_at: now,
            reason,
            patch,
        });
        self.updated_at = now;
        Ok(())
    }

    fn settings_position(&self, id: &str) -> Result<usize, String> {
        self.settings_proposals
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| format!("settings proposal not found: {id}"))
    }

    pub fn settings_proposal(&self, id: &str) -> Result<&SettingsProposal, String> {
        Ok(&self.settings_proposals[self.settings_position(id)?])
    }

    /// Toglie la proposta (approvata o scartata) e la ritorna.
    pub fn remove_settings_proposal(&mut self, id: &str, now: i64) -> Result<SettingsProposal, String> {
        let pos = self.settings_position(id)?;
        self.updated_at = now;
        Ok(self.settings_proposals.remove(pos))
    }
}
