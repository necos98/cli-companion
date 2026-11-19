//! Follow-up dei goal chiusi lato automazione (docs/GOALS.MD, "Follow-up"): la
//! board li raccoglie alla chiusura; qui la scelta dell'utente su ogni voce
//! (scartarla o mandarla all'Intake), mai di un tool.

use super::commands::workspace_of;
use crate::automation::*;
use crate::board::FollowUpState;

impl Registry {
    /// Porta il follow-up `index` di un goal chiuso a `state` (solo l'utente).
    pub fn set_follow_up_state(
        &mut self,
        ws: &str,
        goal_id: &str,
        index: usize,
        state: FollowUpState,
        now: i64,
    ) -> Result<(), String> {
        self.edit(ws, |b| b.set_follow_up_state(goal_id, index, state, now))
    }
}

/// "Scarta" o "Manda all'Intake" di un follow-up, dal pannello o dal dettaglio del goal.
pub(in crate::automation) fn goal_set_follow_up<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    goal_id: &str,
    index: usize,
    state: FollowUpState,
) -> Result<BoardView, String> {
    workspace_of(host, ws_id)?;
    st.run(host, |r| r.set_follow_up_state(ws_id, goal_id, index, state, now_millis()))?;
    st.view(host, ws_id)
}
