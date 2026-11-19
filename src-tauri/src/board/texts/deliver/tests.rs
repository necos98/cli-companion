//! Testi del task di consegna: istruzioni, ambiente, passi di ogni modalita' e
//! titolo sicuro della PR.

use super::*;
use crate::board::texts::{E2E_GATE, E2E_STEP};
use crate::workspaces::MergeMethod;

fn delivery(mode: DeliveryMode) -> Delivery {
    Delivery { mode, parallel: false, merge_method: MergeMethod::Squash }
}

fn repo<'a>(key: &'a str, dir: &'a str, wd: &'a str) -> DeliverRepo<'a> {
    DeliverRepo { key, repo_dir: dir, workdir: wd, branch: "goal/g", base: "main", has_e2e: false }
}

/// L'input di consegna sui `repos` dati.
fn input<'a>(d: &'a Delivery, repos: &'a [DeliverRepo<'a>], e2e: bool) -> DeliverInput<'a> {
    DeliverInput {
        delivery: d,
        goal_title: "Export orders",
        root: "C:\\p",
        workdir: "C:\\p\\.worktrees\\deliver-g",
        repos,
        e2e,
    }
}

const ROOT_REPO: DeliverRepo<'static> = DeliverRepo {
    key: ".",
    repo_dir: "C:\\p",
    workdir: "C:\\p\\.worktrees\\deliver-g",
    branch: "goal/g",
    base: "main",
    has_e2e: true,
};

fn steps(mode: DeliveryMode, e2e: bool) -> String {
    let d = delivery(mode);
    deliver_delivery_steps(&input(&d, &[ROOT_REPO], e2e))
}

#[test]
fn the_title_of_the_pull_request_is_safe_for_a_command_line() {
    assert_eq!(pr_title_safe("Export orders"), "Export orders");
    assert_eq!(
        pr_title_safe("Fix (api): add #12 + more_stuff-v1.2, ok/done"),
        "Fix (api): add #12 + more_stuff-v1.2, ok/done"
    );
    let hostile = pr_title_safe("a\"b$c`d;e!f%g\\h\ti\nj&k|l<m>n'o");
    assert_eq!(hostile, "a b c d e f g h i j k l m n o");
    assert!(!hostile.contains(['"', '$', '`', ';', '!', '%', '\\', '\'', '&', '|']), "{hostile}");
    assert_eq!(pr_title_safe("  \"$`;!%\\`  "), PR_TITLE_FALLBACK);
    assert_eq!(pr_title_safe(""), PR_TITLE_FALLBACK);
    assert_eq!(pr_title_safe("Consegna: ordini piu' veloci, perche'"), "Consegna: ordini piu veloci, perche");
    assert_eq!(pr_title_safe("Caffè è così"), "Caffè è così", "unicode letters stay");
    assert_eq!(pr_title_safe(&"x".repeat(200)).chars().count(), PR_TITLE_MAX);
    assert_eq!(pr_title_safe(&format!("{} y", "a".repeat(79))), "a".repeat(79), "cut, then trimmed");
}

#[test]
fn the_instructions_are_ascii_and_carry_the_kind_rules_the_standard_and_the_rules() {
    assert!(DELIVER_INSTRUCTIONS.is_ascii(), "model-facing text is ASCII");
    let d = delivery(DeliveryMode::Pr);
    let repos = [ROOT_REPO];
    let s = deliver_instructions(&input(&d, &repos, false), "Use tabs.", false);
    assert!(s.is_ascii(), "{s}");
    assert!(
        s.starts_with("You are an automated cli-companion deliverer. Work only inside C:\\p\\.worktrees\\deliver-g;")
    );
    for must in ["get_task_details", "goal_tasks", "--force-with-lease", "Never put text", "complete_task"] {
        assert!(s.contains(must), "missing {must}");
    }
    assert!(s.contains("Never delete local goal branches"), "{s}");
    assert!(s.contains(WORKER_GUARDRAILS.trim_end()), "the engineering standard");
    assert!(s.contains("ask the question in plain text"), "no ask_user");
    assert!(s.ends_with("Workspace rules (always apply; they add project specifics and never relax the security rules above):\nUse tabs."));
    let with_tool = deliver_instructions(&input(&d, &repos, false), "", true);
    assert!(with_tool.contains("call ask_user") && !with_tool.contains("Workspace rules"));
}

#[test]
fn the_instructions_list_the_working_folder_of_every_repository() {
    let d = delivery(DeliveryMode::Commit);
    let repos = [ROOT_REPO, repo("api", "C:\\p\\api", "C:\\p\\api\\.worktrees\\deliver-g")];
    let s = deliver_instructions(&input(&d, &repos, false), "", false);
    assert!(s.contains("inside C:\\p\\.worktrees\\deliver-g, C:\\p\\api\\.worktrees\\deliver-g;"), "{s}");
    let none = deliver_instructions(&input(&d, &[], false), "", false);
    assert!(none.contains("inside C:\\p\\.worktrees\\deliver-g;"), "{none}");
}

#[test]
fn the_environment_describes_every_repository() {
    let d = delivery(DeliveryMode::Pr);
    let repos = [ROOT_REPO, repo("api", "C:\\p\\api", "C:\\p\\api\\.worktrees\\deliver-g")];
    let env = deliver_environment(&input(&d, &repos, true));
    assert!(env.contains("- Goal: Export orders") && env.contains("- Workspace root: C:\\p"), "{env}");
    assert!(env.contains("- Delivery mode: pull request, one per repository, merged with squash"), "{env}");
    assert!(env.contains("Repositories to deliver (2):"), "{env}");
    assert!(env.contains("workspace root: repository checkout C:\\p (the user's copy); worktree C:\\p\\.worktrees\\deliver-g on goal branch goal/g; base branch main; E2E tests configured"), "{env}");
    assert!(env.contains("api: repository checkout C:\\p\\api (the user's copy); worktree C:\\p\\api\\.worktrees\\deliver-g on goal branch goal/g; base branch main\n"), "{env}");
}

#[test]
fn pr_mode_rebases_on_origin_and_opens_one_pull_request_per_repository() {
    let d = Delivery { mode: DeliveryMode::Pr, parallel: false, merge_method: MergeMethod::Rebase };
    let repos =
        [ROOT_REPO, DeliverRepo { base: "develop", ..repo("api", "C:\\p\\api", "C:\\p\\api\\.worktrees\\deliver-g") }];
    let s = deliver_delivery_steps(&input(&d, &repos, false));
    assert!(s.starts_with("Delivery: pull request, one per repository, merged with rebase\n1. Realign"), "{s}");
    assert!(s.contains("`git -C \"C:\\p\\.worktrees\\deliver-g\" fetch origin`, then `git -C \"C:\\p\\.worktrees\\deliver-g\" rebase origin/main`"), "{s}");
    assert!(s.contains("rebase origin/develop`"), "{s}");
    assert!(s.contains("git rebase --abort") && s.contains("listing the files in conflict"), "{s}");
    assert!(
        s.contains("get_task_details") && s.contains("\"Verify\" section") && s.contains("`goal.done_when`"),
        "{s}"
    );
    for base in ["main", "develop"] {
        assert!(
            s.contains(&format!("--base {base} --head goal/g --title \"Export orders\" --body-file <that file>")),
            "{s}"
        );
    }
    assert_eq!(s.matches("git push --force-with-lease -u origin goal/g").count(), 2, "one per repository");
    assert_eq!(s.matches("gh pr merge <pr-url> --rebase").count(), 2);
    assert_eq!(s.matches("git push origin --delete goal/g").count(), 2);
    assert!(
        s.contains("file editing tool") && s.contains("OUTSIDE every repository") && s.contains("delete the file"),
        "{s}"
    );
    assert!(!s.contains("--body \""), "the body never goes on the command line");
    assert!(s.contains("--auto") && s.contains("MERGED") && s.contains("reuse it"), "{s}");
    assert!(s.contains("and `pr_urls`: the pull request link of each repository"), "{s}");
    assert!(
        s.contains("Never run git push --force on a base branch; --force-with-lease only on the goal branch"),
        "{s}"
    );
    assert!(!s.contains("E2E"), "no e2e on the task");
}

#[test]
fn the_title_in_the_steps_is_the_sanitized_one() {
    let d = delivery(DeliveryMode::Pr);
    let mut i = input(&d, &[ROOT_REPO], false);
    i.goal_title = "Pay \"$(rm -rf)\" `now`";
    let s = deliver_delivery_steps(&i);
    assert!(s.contains("--title \"Pay (rm -rf) now\""), "{s}");
    assert!(!s.contains("$(") && !s.contains("`now`"), "{s}");
}

#[test]
fn local_merge_and_commit_fast_forward_the_local_base() {
    for mode in [DeliveryMode::LocalMerge, DeliveryMode::Commit] {
        let s = steps(mode, false);
        assert!(
            s.contains("`git -C \"C:\\p\\.worktrees\\deliver-g\" rebase main`") && !s.contains("fetch origin"),
            "{mode:?}: {s}"
        );
        assert!(s.contains("git -C \"C:\\p\" rev-parse --abbrev-ref HEAD"), "{s}");
        assert!(s.contains("git -C \"C:\\p\" merge --ff-only goal/g"), "{s}");
        assert!(s.contains("git -C \"C:\\p\\.worktrees\\deliver-g\" fetch . goal/g:main"), "{s}");
        assert!(s.contains("Do not push.") && !s.contains("git push origin main") && !s.contains("gh pr"), "{s}");
        assert!(s.contains("complete_task with status \"failed\" and git's message"), "{s}");
        assert!(s.contains("`summary` (what was delivered, for the user), `commits`"), "{s}");
        assert!(s.contains("`git rev-parse <base>` after the fast-forward") && !s.contains("pr_urls"), "{s}");
    }
    assert!(steps(DeliveryMode::LocalMerge, false).starts_with("Delivery: local merge"));
    assert!(steps(DeliveryMode::Commit, false).starts_with("Delivery: commit"));
}

#[test]
fn push_mode_rebases_on_origin_fast_forwards_and_pushes_the_base() {
    let s = steps(DeliveryMode::Push, false);
    assert!(s.starts_with("Delivery: push"), "{s}");
    assert!(s.contains("fetch origin`, then `git -C \"C:\\p\\.worktrees\\deliver-g\" rebase origin/main`"), "{s}");
    assert!(s.contains("merge --ff-only goal/g") && s.contains("fetch . goal/g:main"), "{s}");
    assert!(s.contains("Then `git -C \"C:\\p\" push origin main`."), "{s}");
    assert!(!s.contains("Do not push.") && !s.contains("--force-with-lease -u"), "{s}");
}

#[test]
fn multiple_repositories_are_each_fast_forwarded() {
    let d = delivery(DeliveryMode::LocalMerge);
    let repos = [ROOT_REPO, repo("api", "C:\\p\\api", "C:\\p\\api\\.worktrees\\deliver-g")];
    let s = deliver_delivery_steps(&input(&d, &repos, false));
    assert_eq!(s.matches("--abbrev-ref HEAD").count(), 2, "{s}");
    assert!(s.contains("git -C \"C:\\p\\api\" merge --ff-only goal/g") && s.contains("\n   - api: "), "{s}");
}

#[test]
fn an_e2e_task_runs_the_e2e_step_and_the_gate() {
    for mode in [DeliveryMode::Pr, DeliveryMode::LocalMerge, DeliveryMode::Push, DeliveryMode::Commit] {
        let with = steps(mode, true);
        assert!(with.contains(E2E_STEP), "{mode:?}");
        assert!(with.contains(E2E_GATE.trim_end()), "{mode:?}");
        let step = |n: usize| with.lines().find(|l| l.starts_with(&format!("{n}. "))).unwrap_or_default().to_string();
        assert!(step(3).starts_with("3. Run the E2E tests through the app"), "after the verification: {with}");
        let without = steps(mode, false);
        assert!(!without.contains("run_e2e") && !without.contains("E2E gate"), "{mode:?}");
    }
}

#[test]
fn every_text_is_ascii() {
    for mode in [DeliveryMode::Pr, DeliveryMode::LocalMerge, DeliveryMode::Push, DeliveryMode::Commit] {
        let d = delivery(mode);
        let repos = [ROOT_REPO, repo("api", "C:\\p\\api", "C:\\p\\api\\.worktrees\\deliver-g")];
        let i = input(&d, &repos, true);
        for text in [deliver_instructions(&i, "", true), deliver_environment(&i), deliver_delivery_steps(&i)] {
            assert!(text.is_ascii(), "{mode:?}: {}", text.chars().find(|c| !c.is_ascii()).unwrap());
        }
    }
}
