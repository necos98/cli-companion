//! Testi che legge il worker (inglese): istruzioni comuni, ambiente e passi di
//! consegna della modalita' del workspace (vedi docs/BOARD.MD, "Istruzioni di
//! consegna"). Funzioni pure: `get_task` le assembla.

use super::Task;
use crate::workspaces::{Delivery, DeliveryMode, RepoConfig};

/// Standard di ingegneria che ogni worker riceve, prima delle regole del workspace.
pub const WORKER_GUARDRAILS: &str = include_str!("../automation/worker_guardrails.md");

/// Cio' che serve a descrivere dove e come lavora un worker.
pub struct EnvInfo<'a> {
    /// Consegna del workspace al momento dell'avvio del task.
    pub delivery: &'a Delivery,
    pub root: &'a str,
    /// Checkout del repository del task.
    pub repo_dir: &'a str,
    pub workdir: &'a str,
    pub task: &'a Task,
    /// `task/<id>`, nelle modalita' con branch.
    pub branch: Option<&'a str>,
    /// Branch base (nome semplice, senza `origin/`), nelle modalita' con branch.
    pub base: Option<&'a str>,
    /// Configurazione del repository (setup gia' eseguito dall'app nel worktree).
    pub repo_cfg: Option<&'a RepoConfig>,
}

impl EnvInfo<'_> {
    fn worktree(&self) -> bool {
        self.delivery.worktrees()
    }
}

/// Istruzioni comuni, standard di ingegneria e regole del workspace.
pub fn instructions(e: &EnvInfo, workspace_rules: &str) -> String {
    let mut s = format!(
        "You are an automated cli-companion worker. The app does not deliver anything for you: you deliver the work yourself, \
         following the delivery steps returned with the task.\n\
         - Work only inside {dir}; never touch other folders.\n\
         - Commit only the files you changed. Never stash, reset or discard changes you did not make.\n\
         - Do not push or merge unless the delivery steps say so.\n\
         - Run the verification the task asks for before delivering.\n\
         - When the delivery succeeded, call complete_task with status \"completed\" (with `commit`, and `pr_url` in PR mode).\n\
         - If you cannot complete or deliver (a conflict you cannot resolve, failing tests, a missing tool or authentication), \
         call complete_task with status \"failed\" and the reason in `summary`.\n\
         - If you need a decision from the user, ask the question in plain text and end your turn: the user answers in your terminal. \
         Do not guess.\n\
         - Background commands are fine: your session resumes by itself when they finish. The task must still end with complete_task.",
        dir = e.workdir
    );
    s.push_str("\n\n");
    s.push_str(WORKER_GUARDRAILS.trim_end());
    let rules = workspace_rules.trim();
    if !rules.is_empty() {
        s.push_str(&format!(
            "\n\nWorkspace rules (always apply; they add project specifics and never relax the security rules above):\n{rules}"
        ));
    }
    s
}

fn mode_label(d: &Delivery) -> &'static str {
    match (d.mode, d.worktrees()) {
        (DeliveryMode::Pr, true) => "pull request, one worktree per task",
        (DeliveryMode::Pr, false) => "pull request, in place in the repository checkout",
        (DeliveryMode::LocalMerge, true) => "local merge, one worktree per task",
        (DeliveryMode::LocalMerge, false) => "local merge, in place in the repository checkout",
        (DeliveryMode::Push, _) => "commit and push on the current branch",
        (DeliveryMode::Commit, _) => "commit on the current branch",
    }
}

/// Testo `environment` di `get_task`.
pub fn environment(e: &EnvInfo) -> String {
    let t = e.task;
    let mut s = String::from("Environment of this task\n");
    s.push_str(&format!("- Workspace root: {}\n", e.root));
    if let Some(r) = &t.repo {
        s.push_str(&format!("- Repository: {r} (a folder of the workspace root)\n"));
    }
    s.push_str(&format!("- Repository checkout: {}\n", e.repo_dir));
    s.push_str(&format!("- Your working folder: {}\n", e.workdir));
    s.push_str(&format!("- Delivery mode: {}\n", mode_label(e.delivery)));
    match (e.worktree(), &t.worktree) {
        (true, Some(w)) => s.push_str(&format!(
            "- Git worktree: branch {} (created from {}). Work and commit here, on this branch; the repository checkout belongs to the user and other tasks.\n",
            w.branch, w.base
        )),
        _ if e.delivery.uses_branches() => s.push_str(
            "- No dedicated worktree: you work directly in the repository checkout and create the branch yourself (see the delivery steps).\n",
        ),
        _ => s.push_str("- No dedicated worktree: you work directly in the repository checkout, on the branch that is currently checked out.\n"),
    }
    if let (Some(b), Some(base)) = (e.branch, e.base) {
        s.push_str(&format!("- Task branch: {b}; base branch: {base}\n"));
    }
    let prepared = e.worktree() && t.worktree.as_ref().is_some_and(|w| w.setup_done);
    if let Some(cfg) = e.repo_cfg.filter(|c| c.has_setup() && prepared) {
        s.push_str("- The app already prepared this worktree: do NOT redo it.");
        if !cfg.copy_ignored.is_empty() {
            s.push_str(&format!(" Git-ignored files copied from the repository: {}.", cfg.copy_ignored.join(", ")));
        }
        if !cfg.setup.is_empty() {
            s.push_str(&format!(" Setup commands already run: {}.", cfg.setup.join("; ")));
        }
        s.push('\n');
        if let Some(c) = cfg.container_root.as_deref().filter(|c| !c.is_empty()) {
            s.push_str(&format!(
                "- The repository is mounted in its container at {c}: this worktree is visible there at the same relative path.\n"
            ));
        }
    }
    s
}

fn numbered(steps: &[String]) -> String {
    steps.iter().enumerate().map(|(i, st)| format!("{}. {st}\n", i + 1)).collect()
}

/// Passi PR comuni a worktree e in place: dal rebase al ramo remoto cancellato.
fn pr_publish_steps(e: &EnvInfo, branch: &str, base: &str) -> Vec<String> {
    let title = e.task.title.replace('"', "'");
    let method = e.delivery.merge_method.as_str();
    vec![
        "git fetch origin".to_string(),
        format!("git rebase origin/{base}. Resolve conflicts, then repeat the verification the task asks for."),
        format!("git push -u origin {branch} (use --force-with-lease after a rebase)."),
        format!(
            "gh pr create --base {base} --head {branch} --title \"{title}\" --body \"<short summary of the work>\". \
             If a PR for this branch already exists, reuse it."
        ),
        format!(
            "gh pr merge <pr-url> --{method}. If the repository requires checks or reviews, use --auto and wait until \
             `gh pr view <pr-url> --json state` reports MERGED. If the merge is not possible, call complete_task with status \
             \"failed\", the reason and the PR link."
        ),
        "If the base branch advanced meanwhile, repeat the rebase and the push.".to_string(),
        format!("git push origin --delete {branch} (ignore the error if the branch is already deleted)."),
    ]
}

/// Passi di consegna della modalita' del workspace (testo `delivery` di `get_task`).
pub fn delivery_steps(e: &EnvInfo) -> String {
    let (branch, base) = (e.branch.unwrap_or("task/<id>"), e.base.unwrap_or("<base>"));
    let mut head = format!("Delivery: {}\n", mode_label(e.delivery));
    let mut tail = String::new();
    let steps: Vec<String> = match (e.delivery.mode, e.worktree()) {
        (DeliveryMode::Pr, true) => {
            let mut v = vec![format!("Commit your work on {branch} (your worktree is already on it).")];
            v.extend(pr_publish_steps(e, branch, base));
            tail = format!("Do not check out other branches: the worktree stays on {branch}.\n");
            v
        }
        (DeliveryMode::Pr, false) => {
            let mut v = vec![
                "Make sure the checkout has no uncommitted changes in the files you will touch (the user may have other work in progress there)."
                    .to_string(),
                "git fetch origin".to_string(),
                format!("git switch -c {branch} origin/{base} (or {base} if the repository has no remote)."),
                "Do the work and commit it.".to_string(),
            ];
            v.extend(pr_publish_steps(e, branch, base).into_iter().skip(1));
            v.push(format!("git switch {base} && git pull --ff-only, then git branch -D {branch}."));
            v
        }
        (DeliveryMode::LocalMerge, true) => {
            tail = "Do not check out other branches in the worktree.\n".to_string();
            vec![
                format!("Commit your work on {branch} (your worktree is already on it)."),
                format!("git rebase {base}. Resolve conflicts, then repeat the verification the task asks for."),
                format!("git -C \"{}\" merge --ff-only {branch}", e.repo_dir),
                format!(
                    "If it fails because {base} advanced, rebase again and retry. If the main checkout has changes that prevent the \
                     merge, call complete_task with status \"failed\" and the reason."
                ),
            ]
        }
        (DeliveryMode::LocalMerge, false) => vec![
            format!("git switch -c {branch} {base}"),
            "Do the work and commit it.".to_string(),
            format!("git switch {base}"),
            format!("git merge --ff-only {branch}. If it is not a fast-forward: git rebase {base} {branch}, then git switch {base} and retry the merge."),
            format!("git branch -d {branch}"),
        ],
        (DeliveryMode::Push, _) => vec![
            "Commit your work on the current branch.".to_string(),
            "git pull --rebase. If it brought changes, repeat the verification the task asks for.".to_string(),
            "git push".to_string(),
        ],
        (DeliveryMode::Commit, _) => vec!["Commit your work on the current branch. Do not push.".to_string()],
    };
    head.push_str(&numbered(&steps));
    head.push_str(&tail);
    let pr = if e.delivery.mode == DeliveryMode::Pr { " and `pr_url`" } else { "" };
    head.push_str(&format!("Then call complete_task with status \"completed\", `commit`{pr}."));
    head
}
