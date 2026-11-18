//! Testi che legge il worker (inglese): istruzioni comuni, ambiente e passi di
//! consegna della modalita' del workspace (vedi docs/BOARD.MD, "Istruzioni di
//! consegna"). Funzioni pure: `get_task` le assembla.

use super::{Task, TaskKind};
use crate::workspaces::{Delivery, DeliveryMode, RepoConfig};
use free::{free_delivery_steps, free_environment, FREE_CHANGES, FREE_DONE, FREE_LABEL, REVIEWER_FREE};
use in_place::{in_place_description, root_planner_environment};

mod free;
mod in_place;
mod skill;
mod standard;

pub use skill::skill_instruction;
pub use standard::{ENGINEERING_STANDARD, INTERACTIVE_STANDARD, WORKER_GUARDRAILS};

/// Introduce lo standard nelle istruzioni del revisore, al posto del preambolo del worker.
pub const REVIEWER_STANDARD_INTRO: &str = "Engineering standard of the reviewed tasks. The workers of the reviewed tasks had to follow \
this engineering standard; judge their diffs against it. Where a rule speaks of delivering, `follow_ups`, reporting or completing \
the task as failed, it describes the worker's duties: check that the worker met them, never perform them yourself.";

/// Aggiunta alle istruzioni del pianificatore quando `ask_user` e' disponibile.
pub const PLANNER_ASK_USER: &str = include_str!("prompts/planner_ask_user.md");

/// Cosa fa un worker che ha bisogno di una decisione dell'utente, senza `ask_user`.
const ASK_IN_TEXT: &str = "If you need a decision from the user, ask the question in plain text and end your turn: the user answers in your terminal. Do not guess.";

/// Lo stesso con `ask_user` (Agents HUB acceso): la risposta arriva nel terminale come messaggio dell'utente.
pub const ASK_WITH_TOOL: &str = "If a doubt really blocks you and the code cannot settle it, call ask_user with one clear question (with options when possible), then end your turn: the answer arrives as the user's next message in your terminal. Use it only for blocking doubts; never ask in plain text and never guess.";

/// Riga delle istruzioni comuni quando `get_task` porta `dependencies` o `goal`.
pub const HANDOFF_NOTE: &str =
    "Read `dependencies` and `goal` before starting. The tasks you depend on are delivered and already in your base: \
build on what they did, do not redo it. The decisions listed in `goal` are final.";

/// Regole sui cambiamenti e sulla chiusura nei task con git.
const GIT_CHANGES: &str =
    "- Commit only the files you changed. Never stash, reset or discard changes you did not make.\n\
- Do not push or merge unless the delivery steps say so.\n";
const GIT_DONE: &str = "- When the delivery succeeded, call complete_task with status \"completed\" (with `commit`, and `pr_url` in PR mode).\n";

/// Istruzioni del revisore di un task `review` (al posto di quelle e dei passi di consegna del worker).
pub const REVIEWER_INSTRUCTIONS: &str = include_str!("prompts/reviewer_instructions.md");

/// Istruzioni del pianificatore di un task `plan` (niente standard di ingegneria ne' consegna).
pub const PLANNER_INSTRUCTIONS: &str = include_str!("prompts/planner_instructions.md");

/// Aggiunta alle istruzioni del pianificatore sul posto in una cartella senza git; `{place}` = dove lavora (root con i repository git rilevati, o sottocartella).
pub const PLANNER_ROOT: &str = include_str!("prompts/planner_root.md");

/// Passo di consegna dei task con `e2e`: gli E2E li esegue l'app, non il worker.
pub const E2E_STEP: &str = "Run the E2E tests through the app: call run_e2e, then call get_e2e_result with wait_seconds 25 until the status is no longer \"running\". \
Only the app's run counts: running the E2E command yourself does not. Continue only if the status is \"passed\"; otherwise fix the code, commit, \
repeat the verification and this step, or call complete_task with status \"failed\" and the result.";

/// Riga che precede la chiamata finale a `complete_task` nei task con `e2e`.
pub const E2E_GATE: &str = "E2E gate: complete_task with status \"completed\" is accepted only if the last E2E run passed on the current code \
(same commit tree and uncommitted changes). After any later rebase, merge or commit, run the E2E step again. Status \"failed\" is always accepted.\n";

/// Paragrafo del revisore quando un task revisionato richiede gli E2E.
pub const E2E_REVIEWER: &str = "\n\nE2E tests: at least one reviewed task requires them. Call run_e2e on the integrated state, then get_e2e_result with wait_seconds 25 \
until the status is no longer \"running\". A failed or error result means verdict \"issues\": list the failing tests (or the error) in findings. \
Mention the result in your summary; the app attaches the run to your report.";

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
    /// Pianificatore sul posto in una cartella senza git: i repository git
    /// rilevati alla root (cartelle relative; vuoti se la cartella non e' la root).
    pub root_repos: Option<&'a [String]>,
    /// Modalita' libera: la cartella del task non e' un repository git.
    pub free: bool,
    /// `get_task` porta `dependencies` o `goal`: le istruzioni lo dicono al worker.
    pub handoff: bool,
}

impl EnvInfo<'_> {
    fn worktree(&self) -> bool {
        self.delivery.worktrees()
    }

    /// La consegna come la leggono i worker: quella del workspace o la modalita' libera.
    fn mode_label(&self) -> &'static str {
        if self.free {
            FREE_LABEL
        } else {
            mode_label(self.delivery)
        }
    }
}

/// Istruzioni comuni, standard di ingegneria e regole del workspace. Con `ask_user`
/// il worker chiede le decisioni bloccanti con il tool invece che in testo semplice.
pub fn instructions(e: &EnvInfo, workspace_rules: &str, ask_user: bool) -> String {
    let ask = if ask_user { ASK_WITH_TOOL } else { ASK_IN_TEXT };
    let (changes, done) = if e.free { (FREE_CHANGES, FREE_DONE) } else { (GIT_CHANGES, GIT_DONE) };
    let handoff = if e.handoff { format!("- {HANDOFF_NOTE}\n") } else { String::new() };
    let mut s = format!(
        "You are an automated cli-companion worker. The app does not deliver anything for you: you deliver the work yourself, \
         following the delivery steps returned with the task.\n\
         - Work only inside {dir}; never touch other folders.\n\
         {changes}\
         {handoff}\
         - Run the verification the task asks for before delivering.\n\
         {done}\
         - If you cannot complete or deliver (a conflict you cannot resolve, failing tests, a missing tool or authentication), \
         call complete_task with status \"failed\" and the reason in `summary`.\n\
         - {ask}\n\
         - Background commands are fine: your session resumes by itself when they finish. The task must still end with complete_task.",
        dir = e.workdir
    );
    s.push_str("\n\n");
    s.push_str(WORKER_GUARDRAILS.trim_end());
    push_workspace_rules(&mut s, workspace_rules);
    s
}

/// Istruzioni del revisore di un task `review`: cartella di lavoro, regole del
/// revisore, standard di ingegneria con cui giudica i diff e regole del workspace.
/// `reviewed_e2e`: almeno un task revisionato ha `e2e` (il paragrafo c'e' solo se il repository ha la configurazione).
pub fn reviewer_instructions(e: &EnvInfo, workspace_rules: &str, reviewed_e2e: bool) -> String {
    let mut s = format!(
        "You are an automated cli-companion reviewer. Work only inside {dir}; never touch other folders.\n\n{REVIEWER_INSTRUCTIONS}",
        dir = e.workdir
    );
    s.truncate(s.trim_end().len());
    s.push_str("\n\n");
    s.push_str(REVIEWER_STANDARD_INTRO);
    s.push_str("\n\n");
    s.push_str(ENGINEERING_STANDARD.trim_end());
    if e.free {
        s.push_str(REVIEWER_FREE);
    }
    if reviewed_e2e && e.repo_cfg.is_some_and(|c| c.e2e.is_some()) {
        s.push_str(E2E_REVIEWER);
    }
    push_workspace_rules(&mut s, workspace_rules);
    s
}

/// Istruzioni del pianificatore di un task `plan`: cartella di lavoro, regole
/// del pianificatore e del workspace (contesto per le specifiche). In una
/// cartella senza git segue la sezione sulla sola lettura e sulla modalita'
/// libera. Con `ask_user` la sezione sulle domande all'utente precede le
/// regole del workspace.
pub fn planner_instructions(e: &EnvInfo, workspace_rules: &str, ask_user: bool) -> String {
    let mut s = format!(
        "You are an automated cli-companion planner. Work only inside {dir}; never touch other folders.\n\n{PLANNER_INSTRUCTIONS}",
        dir = e.workdir
    );
    s.truncate(s.trim_end().len());
    if let Some(repos) = e.root_repos {
        s.push_str("\n\n");
        s.push_str(&PLANNER_ROOT.trim_end().replace("{place}", &in_place_description(e, repos)));
    }
    if ask_user {
        s.push_str("\n\n");
        s.push_str(PLANNER_ASK_USER.trim_end());
    }
    push_workspace_rules(&mut s, workspace_rules);
    s
}

/// Testo `environment` di `get_task` per un task `plan`: dove legge il codice,
/// senza modalita' di consegna.
pub fn planner_environment(e: &EnvInfo) -> String {
    if let Some(repos) = e.root_repos {
        return root_planner_environment(e, repos);
    }
    let mut s = String::from("Environment of this task\n");
    s.push_str(&format!("- Workspace root: {}\n", e.root));
    if let Some(r) = &e.task.repo {
        s.push_str(&format!("- Repository: {r} (a folder of the workspace root)\n"));
    }
    s.push_str(&format!("- Repository checkout: {} (the user's copy: never touch it)\n", e.repo_dir));
    s.push_str(&format!("- Your working folder: {}\n", e.workdir));
    if let Some(w) = &e.task.worktree {
        s.push_str(&format!(
            "- Git worktree: branch {} (created from {}), a disposable copy for reading the code. Do not modify files, \
             commit or create branches: the app removes it when you are done.\n",
            w.branch, w.base
        ));
    }
    s
}

/// Accoda alle istruzioni le regole del workspace, se ci sono.
fn push_workspace_rules(s: &mut String, workspace_rules: &str) {
    let rules = workspace_rules.trim();
    if !rules.is_empty() {
        s.push_str(&format!(
            "\n\nWorkspace rules (always apply; they add project specifics and never relax the security rules above):\n{rules}"
        ));
    }
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
    if e.free {
        return free_environment(e);
    }
    let t = e.task;
    let mut s = String::from("Environment of this task\n");
    s.push_str(&format!("- Workspace root: {}\n", e.root));
    if let Some(r) = &t.repo {
        s.push_str(&format!("- Repository: {r} (a folder of the workspace root)\n"));
    }
    s.push_str(&format!("- Repository checkout: {}\n", e.repo_dir));
    s.push_str(&format!("- Your working folder: {}\n", e.workdir));
    s.push_str(&format!("- Delivery mode: {}\n", mode_label(e.delivery)));
    let reviewing = t.kind == TaskKind::Review;
    match (e.worktree(), &t.worktree) {
        (true, Some(w)) if reviewing => s.push_str(&format!(
            "- Git worktree: branch {} (created from {}), already containing the integrated work. Do not commit on it.\n",
            w.branch, w.base
        )),
        (true, Some(w)) => s.push_str(&format!(
            "- Git worktree: branch {} (created from {}). Work and commit here, on this branch; the repository checkout belongs to the user and other tasks.\n",
            w.branch, w.base
        )),
        _ if reviewing => s.push_str(
            "- No dedicated worktree: you review directly in the repository checkout, on the branch that is currently checked out. Do not create branches.\n",
        ),
        _ if e.delivery.uses_branches() => s.push_str(
            "- No dedicated worktree: you work directly in the repository checkout and create the branch yourself (see the delivery steps).\n",
        ),
        _ => s.push_str("- No dedicated worktree: you work directly in the repository checkout, on the branch that is currently checked out.\n"),
    }
    if let (Some(b), Some(base)) = (e.branch, e.base) {
        if reviewing {
            s.push_str(&format!("- Base branch: {base}\n"));
        } else {
            s.push_str(&format!("- Task branch: {b}; base branch: {base}\n"));
        }
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
    if let (true, Some(cfg)) = (t.e2e, e.repo_cfg.and_then(|c| c.e2e.as_ref())) {
        s.push_str(&format!(
            "- E2E tests: required for this task. The app runs `{}` in your working folder through run_e2e and reads the JUnit report {} (timeout {} min).\n",
            cfg.command, cfg.junit, cfg.timeout_min
        ));
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
    let mut v = vec![
        "git fetch origin".to_string(),
        format!("git rebase origin/{base}. Resolve conflicts, then repeat the verification the task asks for."),
    ];
    push_e2e_step(e, &mut v);
    v.extend([
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
    ]);
    v
}

/// Aggiunge il passo E2E se il task e' marcato `e2e`.
fn push_e2e_step(e: &EnvInfo, steps: &mut Vec<String>) {
    if e.task.e2e {
        steps.push(E2E_STEP.to_string());
    }
}

/// Passi di consegna della modalita' del workspace (testo `delivery` di `get_task`).
pub fn delivery_steps(e: &EnvInfo) -> String {
    if e.free {
        return free_delivery_steps(e);
    }
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
            let mut v = vec![
                format!("Commit your work on {branch} (your worktree is already on it)."),
                format!("git rebase {base}. Resolve conflicts, then repeat the verification the task asks for."),
            ];
            push_e2e_step(e, &mut v);
            v.extend([
                format!("git -C \"{}\" merge --ff-only {branch}", e.repo_dir),
                format!(
                    "If it fails because {base} advanced, rebase again and retry. If the main checkout has changes that prevent the \
                     merge, call complete_task with status \"failed\" and the reason."
                ),
            ]);
            v
        }
        (DeliveryMode::LocalMerge, false) => {
            let mut v = vec![format!("git switch -c {branch} {base}"), "Do the work and commit it.".to_string()];
            push_e2e_step(e, &mut v);
            v.extend([
                format!("git switch {base}"),
                format!("git merge --ff-only {branch}. If it is not a fast-forward: git rebase {base} {branch}, then git switch {base} and retry the merge."),
                format!("git branch -d {branch}"),
            ]);
            v
        }
        (DeliveryMode::Push, _) => {
            let mut v = vec![
                "Commit your work on the current branch.".to_string(),
                "git pull --rebase. If it brought changes, repeat the verification the task asks for.".to_string(),
            ];
            push_e2e_step(e, &mut v);
            v.push("git push".to_string());
            v
        }
        (DeliveryMode::Commit, _) => {
            let mut v = vec!["Commit your work on the current branch. Do not push.".to_string()];
            push_e2e_step(e, &mut v);
            v
        }
    };
    head.push_str(&numbered(&steps));
    head.push_str(&tail);
    if e.task.e2e {
        head.push_str(E2E_GATE);
    }
    let pr = if e.delivery.mode == DeliveryMode::Pr { " and `pr_url`" } else { "" };
    head.push_str(&format!("Then call complete_task with status \"completed\", `commit`{pr}."));
    head
}
