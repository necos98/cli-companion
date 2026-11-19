//! Testi del task di consegna di un goal (docs/GOALS.MD, "Consegna del goal"):
//! istruzioni proprie del kind, ambiente e passi di consegna. A differenza degli
//! altri task non partono da `EnvInfo` (un solo repository): da un input puro che
//! elenca tutti i repository da consegnare, ognuno col suo worktree sul branch
//! del goal. I passi dipendono dalla modalita' di consegna del workspace, che per
//! i goal si applica qui e una volta sola.

use super::{numbered, push_workspace_rules, ASK_IN_TEXT, ASK_WITH_TOOL, E2E_GATE, E2E_STEP, WORKER_GUARDRAILS};
use crate::workspaces::{Delivery, DeliveryMode};

#[cfg(test)]
mod tests;

/// Istruzioni proprie del kind `deliver` (niente skill: le esegue un Bot `work`).
const DELIVER_INSTRUCTIONS: &str = include_str!("../prompts/deliver_instructions.md");

/// Lunghezza massima del titolo della PR sanificato.
const PR_TITLE_MAX: usize = 80;
/// Titolo della PR se quello del goal non lascia nulla di utilizzabile.
const PR_TITLE_FALLBACK: &str = "Goal delivery";
/// Segni di punteggiatura ammessi nel titolo della PR, oltre a lettere, cifre e spazi.
const PR_TITLE_PUNCT: &str = "-_.,:/()#+";

/// Un repository da consegnare.
#[derive(Debug, Clone, Copy)]
pub struct DeliverRepo<'a> {
    /// Chiave del repository (`repo_key`: "." = la root del workspace).
    pub key: &'a str,
    /// Checkout dell'utente.
    pub repo_dir: &'a str,
    /// Worktree del task di consegna, sul branch del goal.
    pub workdir: &'a str,
    /// `goal/<id>`.
    pub branch: &'a str,
    /// Branch base (nome semplice, senza `origin/`).
    pub base: &'a str,
    /// Il repository ha gli E2E configurati.
    pub has_e2e: bool,
}

/// Cio' che serve a descrivere la consegna di un goal.
#[derive(Debug, Clone, Copy)]
pub struct DeliverInput<'a> {
    /// Consegna del workspace al momento dell'avvio (modalita' e metodo di merge).
    pub delivery: &'a Delivery,
    pub goal_title: &'a str,
    pub root: &'a str,
    /// Cartella di lavoro principale: il worktree del repository del task.
    pub workdir: &'a str,
    /// I repository da consegnare, il principale compreso.
    pub repos: &'a [DeliverRepo<'a>],
    /// Il task richiede gli E2E.
    pub e2e: bool,
}

/// Titolo della PR dal titolo del goal, sicuro da mettere fra virgolette in una
/// riga di comando: tiene lettere e cifre (anche unicode), spazi e `-_.,:/()#+`,
/// il resto diventa spazio; spazi compressi, al massimo `PR_TITLE_MAX` caratteri.
pub fn pr_title_safe(title: &str) -> String {
    let kept: String =
        title.chars().map(|c| if c.is_alphanumeric() || PR_TITLE_PUNCT.contains(c) { c } else { ' ' }).collect();
    let words: Vec<&str> = kept.split_whitespace().collect();
    let joined: String = words.join(" ").chars().take(PR_TITLE_MAX).collect();
    match joined.trim() {
        "" => PR_TITLE_FALLBACK.to_string(),
        title => title.to_string(),
    }
}

/// Come si chiama la consegna nell'ambiente e nei passi.
fn mode_label(d: &Delivery) -> String {
    match d.mode {
        DeliveryMode::Pr => format!("pull request, one per repository, merged with {}", d.merge_method.as_str()),
        DeliveryMode::LocalMerge => "local merge: fast-forward of the local base branch".to_string(),
        DeliveryMode::Push => "push: fast-forward of the local base branch, then push it".to_string(),
        DeliveryMode::Commit => "commit: fast-forward of the local base branch, nothing is pushed".to_string(),
    }
}

/// Come un repository si chiama nei testi.
fn repo_name(r: &DeliverRepo) -> String {
    if r.key == "." {
        "workspace root".to_string()
    } else {
        r.key.to_string()
    }
}

/// Istruzioni del worker di consegna: cartelle di lavoro, regole del kind,
/// standard di ingegneria e regole del workspace. Con `ask_user` il worker chiede
/// le decisioni bloccanti con il tool invece che in testo semplice.
pub fn deliver_instructions(i: &DeliverInput, workspace_rules: &str, ask_user: bool) -> String {
    let dirs: Vec<&str> = i.repos.iter().map(|r| r.workdir).collect();
    let dirs = if dirs.is_empty() { i.workdir.to_string() } else { dirs.join(", ") };
    let ask = if ask_user { ASK_WITH_TOOL } else { ASK_IN_TEXT };
    let mut s = format!(
        "You are an automated cli-companion deliverer. Work only inside {dirs}; never touch other folders.\n\n{}",
        DELIVER_INSTRUCTIONS.trim_end()
    );
    s.push_str(&format!("\n- {ask}\n\n"));
    s.push_str(WORKER_GUARDRAILS.trim_end());
    push_workspace_rules(&mut s, workspace_rules);
    s
}

/// Testo `environment` di `get_task` per un task `deliver`.
pub fn deliver_environment(i: &DeliverInput) -> String {
    let mut s = String::from("Environment of this task\n");
    s.push_str(&format!("- Workspace root: {}\n", i.root));
    s.push_str(&format!("- Goal: {}\n", i.goal_title));
    s.push_str(&format!("- Your working folder: {}\n", i.workdir));
    s.push_str(&format!("- Delivery mode: {}\n", mode_label(i.delivery)));
    s.push_str(&format!("- Repositories to deliver ({}):\n", i.repos.len()));
    for r in i.repos {
        s.push_str(&format!(
            "  - {name}: repository checkout {dir} (the user's copy); worktree {wd} on goal branch {branch}; base branch {base}{e2e}\n",
            name = repo_name(r),
            dir = r.repo_dir,
            wd = r.workdir,
            branch = r.branch,
            base = r.base,
            e2e = if r.has_e2e { "; E2E tests configured" } else { "" },
        ));
    }
    s.push_str("- The goal branches already contain the integrated work of every task of the goal.\n");
    s
}

/// Passi di consegna (testo `delivery` di `get_task`): riallineo, verifica,
/// passo finale della modalita' e `complete_task`.
pub fn deliver_delivery_steps(i: &DeliverInput) -> String {
    let mut steps = vec![realign_step(i), verify_step()];
    if i.e2e {
        steps.push(E2E_STEP.to_string());
    }
    steps.push(final_step(i));
    steps.push(report_step(i));
    let mut out = format!("Delivery: {}\n{}", mode_label(i.delivery), numbered(&steps));
    out.push_str(
        "Never run git push --force on a base branch; --force-with-lease only on the goal branch, after the rebase. \
         Never delete the local goal branches. Do not check out other branches in the worktrees.\n",
    );
    if i.e2e {
        out.push_str(E2E_GATE);
    }
    out.trim_end().to_string()
}

/// Passo 1: il branch del goal sulla base aggiornata, repository per repository.
fn realign_step(i: &DeliverInput) -> String {
    let remote = matches!(i.delivery.mode, DeliveryMode::Pr | DeliveryMode::Push);
    let mut s = String::from("Realign each goal branch on its updated base. For every repository, in its worktree:");
    for r in i.repos {
        s.push_str(&format!("\n   - {} ({}):", repo_name(r), r.branch));
        if remote {
            s.push_str(&format!(
                " `git -C \"{}\" fetch origin`, then `git -C \"{}\" rebase origin/{}`.",
                r.workdir, r.workdir, r.base
            ));
        } else {
            s.push_str(&format!(" `git -C \"{}\" rebase {}`.", r.workdir, r.base));
        }
    }
    s.push_str(
        "\n   Resolve the conflicts. If you cannot, run `git rebase --abort` in that worktree and call complete_task with \
         status \"failed\", listing the files in conflict.",
    );
    s
}

/// Passo 2: l'obiettivo si verifica sul branch riallineato.
fn verify_step() -> String {
    "Verify the goal on the realigned goal branches: read the specification of every task in `goal_tasks` with \
     get_task_details and run again every command of its \"Verify\" section, then check each criterion of `goal.done_when`. \
     If anything fails, do not deliver: call complete_task with status \"failed\", the failing command and its output."
        .to_string()
}

/// Passo finale: porta il lotto sulla base secondo la modalita' del workspace.
fn final_step(i: &DeliverInput) -> String {
    match i.delivery.mode {
        DeliveryMode::Pr => pr_step(i),
        DeliveryMode::LocalMerge | DeliveryMode::Commit => fast_forward_step(i, false),
        DeliveryMode::Push => fast_forward_step(i, true),
    }
}

/// Modalita' `pr`: una PR per repository, merge, cancellazione del branch remoto.
fn pr_step(i: &DeliverInput) -> String {
    let title = pr_title_safe(i.goal_title);
    let method = i.delivery.merge_method.as_str();
    let mut s = String::from(
        "Deliver every repository with its own pull request. For each one, run the commands in its worktree:",
    );
    for r in i.repos {
        s.push_str(&format!(
            "\n   - {name} (worktree {wd}):\n     a. `git push --force-with-lease -u origin {branch}`\n     \
             b. Write the pull request body (a short summary of what the tasks of the goal delivered, from `goal_tasks`) with your \
             file editing tool into a file OUTSIDE every repository, then run `gh pr create --base {base} --head {branch} --title \
             \"{title}\" --body-file <that file>` and delete the file. If a pull request for {branch} already exists, reuse it \
             (`gh pr view {branch} --json url`).\n     \
             c. `gh pr merge <pr-url> --{method}`. If the repository requires checks or reviews, use --auto and wait until \
             `gh pr view <pr-url> --json state` reports MERGED. If the merge is not possible, call complete_task with status \
             \"failed\", the reason and the pull request link.\n     \
             d. `git push origin --delete {branch}` (ignore the error if the branch is already deleted).",
            name = repo_name(r),
            wd = r.workdir,
            branch = r.branch,
            base = r.base,
        ));
    }
    s
}

/// Modalita' `local_merge`, `commit` e `push`: fast-forward del base locale,
/// nel checkout dell'utente se e' quello su cui e' il base, altrimenti dal
/// worktree senza metterlo in checkout; con `push` poi il push del base.
fn fast_forward_step(i: &DeliverInput, push: bool) -> String {
    let mut s = String::from(
        "Fast-forward the local base branch of every repository to its goal branch. For each one, run the commands in the worktree:",
    );
    for r in i.repos {
        s.push_str(&format!(
            "\n   - {name}: `git -C \"{dir}\" rev-parse --abbrev-ref HEAD`. If it prints {base}, the user's checkout is on the base: \
             run `git -C \"{dir}\" merge --ff-only {branch}`. Otherwise run `git -C \"{wd}\" fetch . {branch}:{base}` (fast-forwards \
             {base} without checking it out).",
            name = repo_name(r),
            dir = r.repo_dir,
            wd = r.workdir,
            branch = r.branch,
            base = r.base,
        ));
        if push {
            s.push_str(&format!(" Then `git -C \"{}\" push origin {}`.", r.repo_dir, r.base));
        }
    }
    s.push_str(
        "\n   If git refuses (the checkout has changes in the way, the base is checked out elsewhere, it is not a fast-forward, the \
         push is rejected), change nothing else: call complete_task with status \"failed\" and git's message.",
    );
    if !push {
        s.push_str(" Do not push.");
    }
    s
}

/// Ultimo passo: cosa mettere nel report.
fn report_step(i: &DeliverInput) -> String {
    let (hash, prs) = if i.delivery.mode == DeliveryMode::Pr {
        ("`git rev-parse <goal branch>` before the merge", ", and `pr_urls`: the pull request link of each repository")
    } else {
        ("`git rev-parse <base>` after the fast-forward", "")
    };
    format!(
        "Report: call complete_task with status \"completed\", `summary` (what was delivered, for the user), `commits` (the final \
         commit hash of each repository: {hash}){prs}."
    )
}
