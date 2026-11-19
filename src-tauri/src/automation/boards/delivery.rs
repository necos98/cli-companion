//! Consegna dei goal lato automazione (docs/GOALS.MD, "Consegna del goal"): la
//! creazione del task di consegna (dal pulsante "Consegna" e dalla consegna
//! automatica, con la stessa funzione), le impostazioni di consegna del goal, cio'
//! che `Board::advance_goals` chiede al publish (notifica, consegna automatica) e
//! la chiusura di una consegna senza nulla da consegnare. Git gira altrove,
//! fuori dal lock (`worktrees`).

use super::commands::workspace_of;
use crate::automation::worktrees::DeliveryCleanup;
use crate::automation::*;
use crate::board::{build_report, DeliveredGoal, GoalAdvance, SpecContext};

impl Registry {
    /// Crea il task di consegna del goal (vedi `Board::create_deliver_task`).
    pub fn create_deliver_task(
        &mut self,
        ws: &str,
        goal_id: &str,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<String, String> {
        self.edit(ws, |b| b.create_deliver_task(goal_id, ctx, now))
    }

    /// Cambia consegna automatica e/o Bot di consegna di un goal aperto.
    pub fn set_goal_delivery(
        &mut self,
        ws: &str,
        goal_id: &str,
        auto_deliver: Option<bool>,
        bot: Option<String>,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        self.edit(ws, |b| b.set_goal_delivery(goal_id, auto_deliver, bot, ctx, now))
    }

    /// Cio' che l'avanzamento dei goal chiede dopo `publish_board`: la notifica di
    /// ogni goal consegnato e la consegna automatica dei goal pronti, una sola
    /// per goal finche' non e' finita (`auto_delivering`).
    pub(in crate::automation) fn apply_advance(&mut self, ws: &str, advance: GoalAdvance) {
        for delivered in advance.delivered {
            self.notify_delivered(ws, delivered);
        }
        for goal_id in advance.auto_deliver {
            if self.auto_delivering.insert((ws.to_string(), goal_id.clone())) {
                self.effects.push(Effect::AutoDeliver { ws_id: ws.to_string(), goal_id });
            }
        }
    }

    /// Notifica "Goal consegnato": il task e' quello di consegna, il titolo quello
    /// del goal, e nessun tab (il worker ha gia' finito).
    fn notify_delivered(&mut self, ws: &str, d: DeliveredGoal) {
        let notice = Notice {
            workspace_id: ws.to_string(),
            task_id: d.deliver_task_id,
            tab_id: None,
            kind: "delivered",
            body: format!("Goal consegnato: {}", d.title),
            title: d.title,
        };
        self.effects.push(Effect::Notify(Box::new(notice)));
    }

    /// La consegna automatica del goal e' finita (riuscita o no): se ne puo'
    /// chiedere un'altra.
    pub(in crate::automation) fn auto_deliver_done(&mut self, ws: &str, goal_id: &str) {
        self.auto_delivering.remove(&(ws.to_string(), goal_id.to_string()));
    }

    /// Il task di consegna non ha nulla da consegnare (nessun repository del goal
    /// ha commit oltre la base): `done` senza worker, con un rapporto dell'app, e
    /// subito la pulizia dei branch del goal. Come ogni esito di un avvio vale solo
    /// se l'avvio `attempt` e' ancora quello in corso; `false` se e' stato superato.
    pub fn finish_without_worker(&mut self, ws: &str, tid: &str, attempt: u32, summary: &str, now: i64) -> bool {
        if !self.start_is_current(ws, tid, attempt) {
            return false;
        }
        let report = build_report("completed", summary, ReportFields::default(), None);
        let done = self
            .boards
            .get_mut(ws)
            .is_some_and(|b| b.finish_running(tid, None, Outcome::Done, Some(report), None, now));
        if done {
            self.mark_dirty(ws);
            let board = self.boards.get(ws);
            let cleanup = board
                .and_then(|b| b.task(tid))
                .and_then(|t| DeliveryCleanup::of_task(ws, t, board, true, None))
                .map(|c| DeliveryCleanup { immediate: true, ..c });
            self.effects.extend(cleanup.map(|c| Effect::DeliveryCleanup(Box::new(c))));
        }
        done
    }

    /// Pulizia del task di consegna `done`, a goal consegnato: worktree e branch.
    pub(in crate::automation) fn queue_delivery_cleanup(&mut self, ws: &str, tid: &str) {
        let board = self.boards.get(ws);
        let cleanup = board
            .and_then(|b| b.task(tid))
            .filter(|t| t.status() == TaskStatus::Done)
            .and_then(|t| DeliveryCleanup::of_task(ws, t, board, true, None));
        self.effects.extend(cleanup.map(|c| Effect::DeliveryCleanup(Box::new(c))));
    }

    /// Esito della pulizia di una consegna: i branch cancellati si dimenticano, il
    /// worktree sparisce dal task se e' stato tolto ovunque, gli avvisi diventano
    /// la nota (mai bloccanti).
    pub fn delivery_cleanup_done(
        &mut self,
        cleanup: &DeliveryCleanup,
        removed: bool,
        forgotten: &[String],
        warnings: &[String],
        now: i64,
    ) {
        let ws = cleanup.ws_id.as_str();
        if !forgotten.is_empty() {
            self.forget_goal_branches(ws, &cleanup.goal_id, forgotten, now);
        }
        let Some(t) = self.boards.get_mut(ws).and_then(|b| b.task_mut(&cleanup.task_id)) else {
            return;
        };
        if removed {
            t.worktree = None;
        }
        if !warnings.is_empty() {
            t.replace_note(&format!("cleanup warnings: {}", warnings.join("; ")), now);
        }
        t.updated_at = now;
        self.mark_dirty(ws);
    }
}

/// Crea il task di consegna del goal con i Bot dell'host, letti fuori dal lock.
/// E' la funzione del pulsante "Consegna" e della consegna automatica.
fn create_deliver<H: Host>(st: &AutomationState, host: &H, ws: &Workspace, goal_id: &str) -> Result<String, String> {
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| r.create_deliver_task(&ws.id, goal_id, ctx, now_millis()))
}

/// "Consegna" / "Consegna comunque" dell'utente: crea il task di consegna del
/// goal pronto, anche se l'ultima review ha trovato problemi.
pub(in crate::automation) fn goal_deliver<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    goal_id: &str,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    create_deliver(st, host, &ws, goal_id)?;
    st.view(host, ws_id)
}

/// Consegna automatica di un goal pronto (effetto `AutoDeliver`): se la creazione
/// fallisce (Bot spento, per esempio) il goal resta com'e' e lo si registra.
pub(in crate::automation) fn run_auto_deliver<H: Host>(st: &AutomationState, host: &H, ws_id: &str, goal_id: &str) {
    let created = workspace_of(host, ws_id).and_then(|ws| create_deliver(st, host, &ws, goal_id));
    if let Err(e) = created {
        log::warn!("workspace {ws_id}: automatic delivery of goal {goal_id} not started: {e}");
    }
    st.run(host, |r| r.auto_deliver_done(ws_id, goal_id));
}

/// Impostazioni di consegna di un goal aperto (solo dell'utente): consegna
/// automatica e/o Bot, che deve esistere, essere acceso ed eseguire i `work`.
pub(in crate::automation) fn goal_set_delivery<H: Host>(
    st: &AutomationState,
    host: &H,
    ws_id: &str,
    goal_id: &str,
    auto_deliver: Option<bool>,
    bot: Option<String>,
) -> Result<BoardView, String> {
    let ws = workspace_of(host, ws_id)?;
    let bots = host.agent_bots();
    let ctx = SpecContext { workspace_agent: ws.agent.as_deref(), bots: &bots };
    st.run(host, |r| r.set_goal_delivery(ws_id, goal_id, auto_deliver, bot, ctx, now_millis()))?;
    st.view(host, ws_id)
}
