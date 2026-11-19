//! Passi di consegna in modalita' `pr` comuni a worktree e in place.

use super::{push_e2e_step, EnvInfo};

/// Passi PR comuni a worktree e in place: dal rebase al ramo remoto cancellato.
pub(super) fn pr_publish_steps(e: &EnvInfo, branch: &str, base: &str) -> Vec<String> {
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
