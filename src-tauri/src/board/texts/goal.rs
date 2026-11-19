//! Testi dei task di un goal in una cartella git (docs/GOALS.MD, "Branch del
//! goal"): ambiente, consegna (una sola variante, indipendente dalla modalita'
//! del workspace) e il riferimento dello stato integrato per il revisore. Le
//! sceglie `EnvInfo::goal_branch`.

use super::{numbered, push_e2e_step, EnvInfo, E2E_GATE, REVIEWER_INSTRUCTIONS};

/// Chiusura dei task di un goal: la consegna e' l'integrazione in `goal/<id>`.
pub(super) const GOAL_DONE: &str =
    "- When the delivery succeeded, call complete_task with status \"completed\" (with `commit`).\n";

/// Riga delle istruzioni del revisore sullo stato integrato, per un task sciolto.
const INTEGRATED_LOOSE: &str = "- To verify the INTEGRATED state: in pull request mode run `git fetch origin` and check `origin/<base>` (the base branch is in the environment). In the other modes your checkout or worktree already contains the integrated work.";

/// Il goal non e' nel base: si consegna per intero piu' tardi.
pub(super) fn mode_label(goal: &str, base: &str) -> String {
    format!("goal branch {goal} (the goal is delivered to {base} as a whole later)")
}

/// Riga dei branch dell'ambiente: quelli del task, o (revisione) lo stato da verificare.
pub(super) fn branch_line(reviewing: bool, branch: &str, goal: &str, base: &str) -> String {
    if reviewing {
        format!("- Goal branch: {goal} (the integrated state to verify); base branch: {base}\n")
    } else {
        format!("- Task branch: {branch}; goal branch: {goal}; base branch: {base}\n")
    }
}

/// Istruzioni del revisore: per un task di un goal lo stato integrato e' il
/// branch del goal, che il worktree contiene gia'; niente e' ancora sul base.
pub(super) fn reviewer_body(e: &EnvInfo) -> String {
    let Some(goal) = e.goal_branch else {
        return REVIEWER_INSTRUCTIONS.to_string();
    };
    let base = e.base.unwrap_or("<base>");
    let line = format!(
        "- To verify the INTEGRATED state: it is the goal branch {goal}, and your worktree already contains it: do not fetch \
         or check out origin/{base}. The diff of the whole goal is git diff {base}...{goal}; nothing of the goal is on {base} yet."
    );
    REVIEWER_INSTRUCTIONS.replace(INTEGRATED_LOOSE, &line)
}

/// Passi di consegna di un task di un goal: rebase su `goal/<id>` e
/// avanzamento (solo fast-forward) del branch del goal senza metterlo in
/// checkout. `git fetch . task:goal` lo rifiuta se non e' un fast-forward o se
/// il branch del goal e' in checkout in una cartella, senza cambiare nulla.
pub(super) fn delivery_steps(e: &EnvInfo, goal: &str) -> String {
    let (branch, base) = (e.branch.unwrap_or("task/<id>"), e.base.unwrap_or("<base>"));
    let mut steps = vec![
        format!("Commit your work on {branch} (your worktree is already on it)."),
        format!("git rebase {goal}. Resolve conflicts, then repeat the verification the task asks for."),
    ];
    push_e2e_step(e, &mut steps);
    steps.extend([
        format!("git fetch . {branch}:{goal} (fast-forwards {goal} to your commit without checking it out)."),
        format!(
            "If it is rejected as non-fast-forward because {goal} moved, rebase on it again, repeat the verification and retry. \
             If git refuses because {goal} is checked out in another folder, call complete_task with status \"failed\" and \
             the reason: the user must switch that folder to another branch."
        ),
    ]);
    let mut out = format!("Delivery: {}\n{}", mode_label(goal, base), numbered(&steps));
    out.push_str(&format!(
        "Never push, never open a pull request, never touch {base}: the goal is delivered as a whole later. \
         Do not check out other branches in the worktree.\n"
    ));
    if e.task.e2e {
        out.push_str(E2E_GATE);
    }
    out.push_str("Then call complete_task with status \"completed\", `commit`.");
    out
}

#[cfg(test)]
mod tests;
