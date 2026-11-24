//! Scheduler: sceglie i task da avviare. Funzione pura, il contesto
//! (consegna, fascia oraria) lo fornisce chi chiama.

use super::{repo_key, AgentGate, Board, TaskStatus};
use crate::workspaces::Delivery;
use std::collections::HashSet;

/// Contesto dello scheduler (lo fornisce chi chiama: il nucleo non conosce
/// i workspace).
#[derive(Debug, Clone, Copy)]
pub struct SchedCtx {
    pub delivery: Delivery,
    /// Falso quando il workspace e' archiviato o usa le fasce ed e' fuori fascia.
    pub gate_open: bool,
}

/// Id dei task da avviare ora, in ordine di coda. Funzione pura.
///
/// In modalita' parallela (worktree) fino a `max_parallel` running nella
/// board; altrimenti un solo running per repository, comunque entro
/// `max_parallel` (un task `isolated`, nel suo worktree, non occupa il
/// repository). Un task `no_git` lavora sul posto nella cartella: anche in
/// parallelo due task della stessa cartella non girano insieme. I task il cui
/// agente non e' pronto (`gate`) restano in coda e non occupano ne' il
/// repository ne' un posto: ne partono altri.
pub fn schedule(board: &Board, ctx: &SchedCtx, gate: &AgentGate) -> Vec<String> {
    if board.paused || !ctx.gate_open {
        return Vec::new();
    }
    let mut busy: HashSet<String> = HashSet::new();
    let mut running = 0usize;
    for t in board.tasks.iter().filter(|t| t.status() == TaskStatus::Running) {
        running += 1;
        if !t.kind.isolated() {
            busy.insert(repo_key(t.repo.as_deref()));
        }
    }
    let parallel = ctx.delivery.worktrees();
    let mut chosen = Vec::new();
    for t in board.tasks.iter().filter(|t| t.status() == TaskStatus::Queued && board.deps_done(t)) {
        if running + chosen.len() >= board.max_parallel as usize {
            break;
        }
        if gate.is_blocked(&t.agent) {
            continue;
        }
        let exclusive = !parallel || t.no_git;
        if exclusive && !t.kind.isolated() && !busy.insert(repo_key(t.repo.as_deref())) {
            continue;
        }
        chosen.push(t.id.clone());
    }
    chosen
}
