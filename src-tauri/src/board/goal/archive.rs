//! Goal chiusi da archiviare: la board tiene i `MAX_CLOSED_GOALS_KEPT` chiusi
//! piu' di recente, i piu' vecchi escono (li scrive su disco chi possiede il
//! file, vedi `persist::archive_goal_line`). Qui solo la scelta, pura.

use super::Goal;
use crate::board::Board;

/// Goal chiusi che restano sulla board.
pub const MAX_CLOSED_GOALS_KEPT: usize = 100;

impl Board {
    /// Toglie dalla board i goal chiusi piu' vecchi (per `closed_at`) oltre i
    /// `MAX_CLOSED_GOALS_KEPT` piu' recenti e li ritorna dal piu' vecchio. I
    /// task restano: hanno la loro regola di archiviazione.
    pub fn take_archivable_goals(&mut self) -> Vec<Goal> {
        let mut closed: Vec<(i64, usize)> =
            self.goals.iter().enumerate().filter_map(|(i, g)| Some((g.closed_at?, i))).collect();
        let Some(surplus) = closed.len().checked_sub(MAX_CLOSED_GOALS_KEPT).filter(|n| *n > 0) else {
            return Vec::new();
        };
        closed.sort_unstable();
        let mut slots: Vec<Option<Goal>> = std::mem::take(&mut self.goals).into_iter().map(Some).collect();
        let archived: Vec<Goal> = closed[..surplus].iter().filter_map(|&(_, i)| slots[i].take()).collect();
        self.goals = slots.into_iter().flatten().collect();
        archived
    }
}
