//! Correzione automatica lato automazione (docs/GOALS.MD, "Giri e correzione
//! automatica"): la board decide quali proposte di correzione si accettano da
//! sole (`board::auto_fix_decision`, al publish); qui l'effetto, che le accetta
//! con lo stesso percorso e la stessa validazione dei pulsanti
//! (`proposal_decide_with`), e l'impostazione dell'utente dal dettaglio del goal.

use super::commands::{proposal_decide_with, workspace_of};
use crate::automation::*;
use crate::board::Acceptance;

impl Registry {
    /// Chiede l'accettazione automatica delle proposte `ids`, una sola volta per
    /// proposta finche' quella in corso non e' finita (`auto_fixing`).
    pub(in crate::automation) fn queue_auto_fix(&mut self, ws: &str, ids: Vec<String>) {
        for proposal_id in ids {
            if self.auto_fixing.insert((ws.to_string(), proposal_id.clone())) {
                self.effects.push(Effect::AutoFix { ws_id: ws.to_string(), proposal_id });
            }
        }
    }

    /// L'accettazione automatica della proposta e' finita (riuscita o no).
    pub(in crate::automation) fn auto_fix_done(&mut self, ws: &str, proposal_id: &str) {
        self.auto_fixing.remove(&(ws.to_string(), proposal_id.to_string()));
    }

    /// L'accettazione automatica e' fallita: la proposta resta pendente con il
    /// motivo (vedi `Board::hold_auto_fix`).
    pub fn hold_auto_fix(&mut self, ws: &str, proposal_id: &str, error: &str, now: i64) {
        if self.boards.get_mut(ws).is_some_and(|b| b.hold_auto_fix(proposal_id, error, now)) {
            self.mark_dirty(ws);
        }
    }

    /// Giri di correzione automatica di un goal aperto (solo l'utente).
    pub fn set_goal_auto_fix(&mut self, ws: &str, goal_id: &str, max_rounds: u8, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.set_goal_auto_fix(goal_id, max_rounds, now))
    }
}

/// Accettazione automatica di una proposta di correzione (effetto `AutoFix`). Se
/// fallisce la proposta resta pendente con l'errore; se nel frattempo l'ha decisa
/// l'utente (o l'HUB) non c'e' piu' nulla da fare.
pub(in crate::automation) fn run_auto_fix<H: Host>(st: &AutomationState, host: &H, ws_id: &str, proposal_id: &str) {
    if let Err(e) = proposal_decide_with(st, host, ws_id, proposal_id, true, Acceptance::AUTOMATIC) {
        log::warn!("workspace {ws_id}: proposal {proposal_id} not accepted automatically: {e}");
        st.run(host, |r| r.hold_auto_fix(ws_id, proposal_id, &e, now_millis()));
    }
    st.run(host, |r| r.auto_fix_done(ws_id, proposal_id));
}

/// Giri di correzione automatica di un goal aperto, dal dettaglio del goal: solo
/// l'utente, mai un tool.
pub(in crate::automation) fn goal_set_auto_fix<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    goal_id: &str,
    max_rounds: u8,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    st.run(host, |r| r.set_goal_auto_fix(ws_id, goal_id, max_rounds, now_millis()))?;
    st.view(host, ws_id)
}
