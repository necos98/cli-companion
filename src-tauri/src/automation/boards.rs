//! Board dentro l'automazione: la parte del `Registry` che tiene le board
//! (stato puro + `Effect`) e lo scheduler che le fa avanzare. Il resto sta nei
//! sottomoduli: `lifecycle` (avvio dei task, guardiano, pulizia), `commands`
//! (comandi Tauri della UI) e `session` (tool MCP delle sessioni). Git e
//! worktree sono in `worktrees`; tipi, validazione e scheduler puri in
//! `crate::board`. Il contratto e' `docs/BOARD.MD`.

use super::worktrees::{CleanupTarget, DeliveryCleanup, FolderKinds, StartJob};
use super::*;
use crate::board::{self, Acceptance, GoalView, ProposalInput, SchedCtx, SpecContext, Task, TaskSpec, TaskView};
use serde_json::json;

mod agents;
mod auto_fix;
mod commands;
mod delivery;
mod follow_up;
mod goal_branches;
mod lifecycle;
mod session;

pub(super) use agents::ReadinessLookup;
pub(super) use auto_fix::{goal_set_auto_fix, run_auto_fix};
pub(super) use commands::{
    board_set, goal_add_note, goal_close, proposal_decide, proposal_decide_with, task_action, task_create, task_move,
    task_update, task_update_check, workspace_of,
};
pub(super) use delivery::{goal_deliver, goal_set_delivery, run_auto_deliver};
pub(super) use follow_up::goal_set_follow_up;
pub(super) use goal_branches::{delete_branch, goal_delete_branches};
#[cfg(test)]
pub(super) use lifecycle::START_TIMEOUT_MS;
pub(super) use session::{
    get_goal, list_goals, list_tasks, propose_tasks, rpc_session, session_workspace, task_details,
};

// ---------------------------------------------------------------------------
// Registry: board
// ---------------------------------------------------------------------------

impl Registry {
    pub fn load_board(&mut self, mut board: Board) {
        self.next_rev += 1;
        board.rev = self.next_rev;
        self.boards.insert(board.workspace_id.clone(), board);
    }

    /// Vista della board del workspace (vuota se non esiste).
    pub fn board_view(&self, ws: &Workspace, minute: u16) -> BoardView {
        let blocked = self.schedule_blocked(ws, minute);
        let gate = self.gate_of(&ws.id);
        match self.boards.get(&ws.id) {
            Some(b) => b.view_with(Some(ws), blocked, &gate),
            None => Board::new(&ws.id).view_with(Some(ws), blocked, &gate),
        }
    }

    pub fn task_view(&self, ws: &str, id: &str) -> Option<TaskView> {
        self.boards.get(ws).and_then(|b| b.task_view(id, &self.gate_of(ws)))
    }

    /// I goal della board del workspace con fase e attesa (vuoto se non esiste).
    pub fn goal_views(&self, ws: &str) -> Vec<GoalView> {
        self.boards.get(ws).map(Board::goal_views).unwrap_or_default()
    }

    /// Il goal `id` per `get_goal`, con i suoi task.
    pub fn goal_detail(&self, ws: &str, id: &str) -> Option<Value> {
        self.boards.get(ws).and_then(|b| b.goal_detail_json(id))
    }

    pub(super) fn mark_dirty(&mut self, ws: &str) {
        if !self.dirty.iter().any(|d| d == ws) {
            self.dirty.push(ws.to_string());
        }
    }

    fn board_or_new(&mut self, ws: &str) -> &mut Board {
        self.boards.entry(ws.to_string()).or_insert_with(|| Board::new(ws))
    }

    /// Esegue `f` sulla board e la segna da schedulare/pubblicare se riesce.
    pub(super) fn edit<T>(&mut self, ws: &str, f: impl FnOnce(&mut Board) -> Result<T, String>) -> Result<T, String> {
        let out = f(self.board_or_new(ws))?;
        self.mark_dirty(ws);
        Ok(out)
    }

    /// Id libero per un task creato a mano dal titolo.
    pub fn free_id(&self, ws: &str, title: &str) -> String {
        let board = self.boards.get(ws);
        board::id_from_title(title, &|id| board.is_some_and(|b| b.id_taken(id)))
    }

    /// Task creato a mano, nel goal aperto `goal` o sciolto.
    pub fn create_task(
        &mut self,
        ws: &str,
        spec: TaskSpec,
        goal: Option<&str>,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<String, String> {
        self.edit(ws, |b| b.create_task_in(spec, goal, ctx, now))
    }

    /// Segna `no_git` sui task `queued` la cui cartella e' stata appena
    /// controllata (`ensure_repos`): lo scheduler, che gira dopo, lo legge.
    pub(in crate::automation) fn note_no_git(&mut self, ws: &str, kinds: &FolderKinds) {
        let Some(b) = self.boards.get_mut(ws) else {
            return;
        };
        let mut changed = false;
        for t in b.tasks.iter_mut().filter(|t| t.status() == board::TaskStatus::Queued) {
            let key = board::repo_key(t.repo.as_deref());
            if let Some((_, no_git)) = kinds.iter().find(|(k, _)| *k == key) {
                changed |= std::mem::replace(&mut t.no_git, *no_git) != *no_git;
            }
        }
        if changed {
            self.mark_dirty(ws);
        }
    }

    /// Nuova proposta (dell'Intake, o del revisore o del pianificatore di
    /// `source_task`): ritorna l'id. Un pianificatore propone una volta sola.
    pub fn propose(
        &mut self,
        ws: &str,
        source_tab: &str,
        source_task: Option<String>,
        input: ProposalInput,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<String, String> {
        self.check_planner_can_propose(source_tab)?;
        self.next_proposal += 1;
        let id = format!("p-{now}-{}", self.next_proposal);
        self.edit(ws, |b| b.add_proposal(id.clone(), source_tab, source_task, input, ctx, now))?;
        self.note_planner_proposal(source_tab);
        Ok(id)
    }

    /// Task di una proposta pendente (per verificarne i repository).
    pub fn proposal_specs(&self, ws: &str, id: &str) -> Result<Vec<TaskSpec>, String> {
        self.boards.get(ws).ok_or_else(|| format!("proposal not found: {id}"))?.proposal(id).map(|p| p.tasks.clone())
    }

    pub fn decide_proposal(
        &mut self,
        ws: &str,
        id: &str,
        accept: bool,
        acceptance: Acceptance,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        self.edit(ws, |b| {
            if accept {
                b.accept_proposal_with(id, acceptance, ctx, now).map(|_| ())
            } else {
                b.discard_proposal(id, now)
            }
        })
    }

    pub fn update_task(
        &mut self,
        ws: &str,
        id: &str,
        patch: &Value,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        self.edit(ws, |b| b.update_task(id, patch, ctx, now))
    }

    /// Prova `update_task` su una copia della board: l'errore che darebbe, senza cambiare nulla.
    pub fn check_task_update(
        &self,
        ws: &str,
        id: &str,
        patch: &Value,
        ctx: SpecContext<'_>,
        now: i64,
    ) -> Result<(), String> {
        let board = self.boards.get(ws).ok_or_else(|| format!("task not found: {id}"))?;
        board.clone().update_task(id, patch, ctx, now)
    }

    /// Toglie un task; se aveva un worktree lo fa ripulire.
    pub fn delete_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let removed = self.edit(ws, |b| b.delete_task(id, now))?;
        self.queue_cleanups(ws, vec![removed], None, None);
        Ok(())
    }

    /// "Chiudi goal": toglie i task non conclusi del goal (tutto o niente),
    /// scarta le sue proposte e lo chiude; fa ripulire i worktree.
    pub fn close_goal(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let removed = self.edit(ws, |b| b.close_goal(id, now))?;
        self.queue_cleanups(ws, removed, None, None);
        Ok(())
    }

    /// Nota dell'utente nel registro delle decisioni di un goal aperto.
    pub fn add_goal_note(&mut self, ws: &str, id: &str, text: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.add_goal_note(id, text, now))
    }

    /// Toglie un task e quelli che ne dipendono (tutto o niente); fa ripulire i worktree.
    pub fn delete_task_cascade(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let removed = self.edit(ws, |b| b.delete_with_dependents(id, now))?;
        self.queue_cleanups(ws, removed, None, None);
        Ok(())
    }

    /// Mette in coda la pulizia del worktree di ogni task tolto. `workspace`:
    /// copia del workspace quando non e' piu' nello store. `gone`: la board, se
    /// non e' piu' nel registro (serve ai repository del goal di una consegna).
    /// Di un task di consegna si rimuovono i worktree e si conservano i branch del
    /// goal: non e' stato consegnato.
    fn queue_cleanups(&mut self, ws: &str, removed: Vec<Task>, workspace: Option<&Workspace>, gone: Option<&Board>) {
        let board = gone.or_else(|| self.boards.get(ws));
        let mut effects = Vec::new();
        for task in removed {
            if task.worktree.is_some() {
                if let Some(cleanup) = DeliveryCleanup::of_task(ws, &task, board, false, workspace) {
                    effects.push(Effect::DeliveryCleanup(Box::new(cleanup)));
                    continue;
                }
            }
            if let Some(target) = CleanupTarget::for_task(ws, task) {
                let target = match workspace {
                    Some(w) => target.with_workspace(w),
                    None => target,
                };
                effects.push(Effect::Cleanup(Box::new(target)));
            }
        }
        self.effects.extend(effects);
    }

    pub fn retry_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.retry_task(id, now))
    }

    pub fn mark_task_done(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.mark_done(id, now))
    }

    pub fn move_task(&mut self, ws: &str, id: &str, before: Option<&str>, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.move_task(id, before, now))
    }

    pub fn task_to_top(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        self.edit(ws, |b| b.task_to_top(id, now))
    }

    /// "Ferma": il task `running` fallisce e il tab del worker si chiude.
    pub fn stop_task(&mut self, ws: &str, id: &str, now: i64) -> Result<(), String> {
        let b = self.boards.get(ws).ok_or_else(|| "task not found: the board is empty".to_string())?;
        b.check_stop(id)?;
        let session = b.task(id).and_then(|t| t.session().map(str::to_string));
        if let Some(w) = session.as_deref().and_then(|s| self.workers.get_mut(s)).filter(|w| w.tab_open) {
            w.ended.get_or_insert(WorkerEnd::Closed);
            let tab = w.session.clone();
            self.touch(&tab, now);
            self.effects.push(Effect::Close { tab_id: tab, delay: Duration::ZERO });
        }
        self.fail_task(
            ws,
            id,
            None,
            board::STOPPED_NOTE,
            Failure { cause: FailCause::Other, reason: FailReason::Stopped },
            now,
        );
        Ok(())
    }

    pub fn set_board(
        &mut self,
        ws: &str,
        max_parallel: Option<u8>,
        paused: Option<bool>,
        now: i64,
    ) -> Result<(), String> {
        if max_parallel.is_none() && paused.is_none() {
            return Err("nothing to change: pass max_parallel and/or paused".to_string());
        }
        if let Some(m) = max_parallel {
            board::parse_max_parallel(&json!(m))?;
        }
        self.edit(ws, |b| {
            if let Some(m) = max_parallel {
                b.max_parallel = m;
            }
            if let Some(p) = paused {
                b.paused = p;
            }
            b.updated_at = now;
            Ok(())
        })
    }

    /// Il workspace `ws` e' stato rimosso: via la sua board, e i worktree dei
    /// suoi task vanno ripuliti (con la copia del workspace: lo store non lo
    /// ha piu').
    pub fn remove_board(&mut self, ws: &Workspace) {
        self.cancel_e2e(&ws.id, None);
        self.gates.remove(&ws.id);
        if let Some(board) = self.boards.remove(&ws.id) {
            self.dirty.retain(|d| *d != ws.id);
            self.next_rev += 1;
            self.effects.push(Effect::DeleteBoard(ws.id.clone(), self.next_rev));
            self.queue_cleanups(&ws.id, board.tasks.clone(), Some(ws), Some(&board));
        }
    }

    // --- scheduler ---

    /// Schedula e pubblica le board modificate. `lookup` risolve i workspace
    /// (senza, nulla parte); `minute`: ora locale di `now`, per il gate delle
    /// fasce calcolato adesso e non al giro del timer (fino a 30 s fa);
    /// `readiness`: prontezza nota degli agenti (non bloccante), per il gate degli
    /// agenti.
    pub fn flush_boards(
        &mut self,
        now: i64,
        minute: u16,
        lookup: &dyn Fn(&str) -> Option<Workspace>,
        readiness: &ReadinessLookup,
    ) {
        for ws_id in std::mem::take(&mut self.dirty) {
            self.flush_one(&ws_id, now, minute, lookup, readiness);
        }
        self.update_awake();
    }

    fn flush_one(
        &mut self,
        ws_id: &str,
        now: i64,
        minute: u16,
        lookup: &dyn Fn(&str) -> Option<Workspace>,
        readiness: &ReadinessLookup,
    ) {
        let Some(ws) = lookup(ws_id) else {
            self.gates.remove(ws_id);
            return self.publish_board(ws_id, None, now, minute);
        };
        // Archiviato: congelato. Fuori fascia (se il workspace le usa): nessun avvio.
        let ctx = SchedCtx { delivery: ws.delivery, gate_open: !ws.archived && !self.schedule_blocked(&ws, minute) };
        let gate = self.refresh_gate(&ws, readiness);
        let Some(b) = self.boards.get_mut(ws_id) else {
            return;
        };
        for id in board::schedule(b, &ctx, &gate) {
            // Subito `running`: un'altra esecuzione non lo sceglie due volte.
            if b.start_task(&id, now).is_err() {
                continue;
            }
            if let Some(task) = b.task(&id).cloned() {
                let key = board::repo_key(task.repo.as_deref());
                let goal_branch = task.goal().and_then(|g| b.goal_branch(g, &key)).cloned();
                let goal_branches = match task.goal().and_then(|g| b.goal(g)) {
                    Some(goal) if task.kind == TaskKind::Deliver => goal.branches.clone(),
                    _ => Default::default(),
                };
                let goal_in_place = task.goal().and_then(|g| b.goal(g)).is_some_and(|g| g.in_place);
                let job = StartJob {
                    ws: ws.clone(),
                    attempt: task.attempts(),
                    task,
                    goal_branch,
                    goal_branches,
                    goal_in_place,
                };
                self.effects.push(Effect::StartTask(Box::new(job)));
            }
        }
        self.publish_board(ws_id, Some(&ws), now, minute);
    }

    /// Ogni pubblicazione e' una modifica della vista: nuova revisione.
    fn publish_board(&mut self, ws_id: &str, ws: Option<&Workspace>, now: i64, minute: u16) {
        let blocked = ws.is_some_and(|w| self.schedule_blocked(w, minute));
        let gate = self.gate_of(ws_id);
        self.next_rev += 1;
        let Some(b) = self.boards.get_mut(ws_id) else {
            return;
        };
        b.rev = self.next_rev;
        b.updated_at = now;
        // Chiusura dei goal finiti, prima della vista e dell'archivio; le consegne da
        // annunciare e da avviare sono effetti (vedi `apply_advance`).
        let advance = b.advance_goals(now);
        // L'archivio prima della board salvata senza quei task e goal: se l'app
        // cade in mezzo restano nel file della board (e nell'archivio, due volte).
        let archived = b.take_archivable();
        let archived_goals = b.take_archivable_goals();
        let view = b.view_with(ws, blocked, &gate);
        let published = Effect::Publish(Box::new(b.clone()), Box::new(view));
        if !archived.is_empty() || !archived_goals.is_empty() {
            self.effects.push(Effect::Archive { ws_id: ws_id.to_string(), tasks: archived, goals: archived_goals });
        }
        self.effects.push(published);
        self.apply_advance(ws_id, advance);
    }

    /// L'archivio ha rifiutato questi task e goal, gia' tolti dalla board: li
    /// rimette in fondo, se la board esiste ancora e non li ha gia'. Non segna
    /// la board modificata (niente giro di ripubblicazione): ci riprova la
    /// prossima modifica vera, che li archivia di nuovo.
    pub(super) fn restore_unarchived(&mut self, ws: &str, tasks: Vec<Task>, goals: Vec<Goal>) {
        let Some(b) = self.boards.get_mut(ws) else {
            return;
        };
        b.restore_archived(tasks, goals);
    }
}
