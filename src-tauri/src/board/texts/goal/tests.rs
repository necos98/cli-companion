//! Testi dei task di un goal: ambiente, consegna e riferimento del revisore.

use super::*;
use crate::board::texts::{self, E2E_GATE, E2E_STEP};
use crate::board::{Task, TaskKind, TaskSpec, WorktreeInfo};
use crate::workspaces::{Delivery, DeliveryMode, MergeMethod};

fn delivery(mode: DeliveryMode, parallel: bool) -> Delivery {
    Delivery { mode, parallel, merge_method: MergeMethod::Squash }
}

fn task_from(id: &str, kind: TaskKind, e2e: bool, base: &str) -> Task {
    let spec = TaskSpec { id: id.into(), title: id.into(), task: "do".into(), kind, e2e, ..TaskSpec::default() };
    let mut t = Task::from_spec(spec, 0);
    t.worktree = Some(WorktreeInfo {
        path: format!("C:\\p\\.worktrees\\{id}"),
        branch: format!("task/{id}"),
        base: base.into(),
        setup_done: true,
    });
    t
}

/// Task di un goal: il suo worktree nasce da `goal/g`.
fn task(id: &str, kind: TaskKind, e2e: bool) -> Task {
    task_from(id, kind, e2e, "goal/g")
}

fn env<'a>(d: &'a Delivery, t: &'a Task, goal: Option<&'a str>) -> EnvInfo<'a> {
    EnvInfo {
        delivery: d,
        root: "C:\\p",
        repo_dir: "C:\\p",
        workdir: "C:\\p\\.worktrees\\a",
        task: t,
        branch: Some("task/a"),
        base: Some("main"),
        goal_branch: goal,
        repo_cfg: None,
        root_repos: None,
        free: false,
        handoff: false,
    }
}

#[test]
fn the_delivery_of_a_goal_task_is_one_variant_in_every_mode() {
    let t = task("a", TaskKind::Work, false);
    let expected = "Delivery: goal branch goal/g (the goal is delivered to main as a whole later)\n\
1. Commit your work on task/a (your worktree is already on it).\n\
2. git rebase goal/g. Resolve conflicts, then repeat the verification the task asks for.\n\
3. git fetch . task/a:goal/g (fast-forwards goal/g to your commit without checking it out).\n\
4. If it is rejected as non-fast-forward because goal/g moved, rebase on it again, repeat the verification and retry. \
If git refuses because goal/g is checked out in another folder, call complete_task with status \"failed\" and the reason: \
the user must switch that folder to another branch.\n\
Never push, never open a pull request, never touch main: the goal is delivered as a whole later. \
Do not check out other branches in the worktree.\n\
Then call complete_task with status \"completed\", `commit`.";
    for (mode, parallel) in [
        (DeliveryMode::Commit, false),
        (DeliveryMode::Push, false),
        (DeliveryMode::LocalMerge, false),
        (DeliveryMode::LocalMerge, true),
        (DeliveryMode::Pr, true),
    ] {
        let d = delivery(mode, parallel);
        assert_eq!(texts::delivery_steps(&env(&d, &t, Some("goal/g"))), expected, "{mode:?}");
    }
    assert!(!expected.contains("git push") && !expected.contains("merge --ff-only"));
}

#[test]
fn the_e2e_step_sits_between_the_rebase_and_the_fast_forward() {
    let t = task("a", TaskKind::Work, true);
    let d = delivery(DeliveryMode::Pr, true);
    let text = texts::delivery_steps(&env(&d, &t, Some("goal/g")));
    let pos = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} in {text}"));
    assert!(pos("git rebase goal/g") < pos(E2E_STEP) && pos(E2E_STEP) < pos("git fetch . task/a:goal/g"));
    assert!(text.contains("4. git fetch . task/a:goal/g") && text.contains(E2E_GATE));
}

#[test]
fn the_environment_of_a_goal_task_names_the_goal_branch() {
    let t = task("a", TaskKind::Work, false);
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::Pr, true)] {
        let d = delivery(mode, parallel);
        let text = texts::environment(&env(&d, &t, Some("goal/g")));
        for line in [
            "- Delivery mode: goal branch goal/g (the goal is delivered to main as a whole later)\n",
            "- Git worktree: branch task/a (created from goal/g). Work and commit here",
            "- Task branch: task/a; goal branch: goal/g; base branch: main\n",
        ] {
            assert!(text.contains(line), "{mode:?}: {line} in {text}");
        }
        assert!(!text.contains("No dedicated worktree"), "{text}");
    }
}

#[test]
fn the_environment_of_a_goal_review_points_at_the_integrated_state() {
    let t = task("rev", TaskKind::Review, false);
    let d = delivery(DeliveryMode::Commit, false);
    let text = texts::environment(&env(&d, &t, Some("goal/g")));
    assert!(text.contains("- Goal branch: goal/g (the integrated state to verify); base branch: main\n"), "{text}");
    assert!(text.contains("already containing the integrated work"), "{text}");
}

#[test]
fn the_loose_line_of_the_reviewer_instructions_is_in_the_prompt_exactly_once() {
    assert_eq!(REVIEWER_INSTRUCTIONS.matches(INTEGRATED_LOOSE).count(), 1);
}

#[test]
fn a_goal_reviewer_verifies_the_goal_branch_not_origin() {
    let t = task("rev", TaskKind::Review, false);
    let d = delivery(DeliveryMode::Pr, true);
    let goal = texts::reviewer_instructions(&env(&d, &t, Some("goal/g")), "", false);
    assert!(goal.contains(
        "- To verify the INTEGRATED state: it is the goal branch goal/g, and your worktree already contains it: do not fetch \
         or check out origin/main. The diff of the whole goal is git diff main...goal/g; nothing of the goal is on main yet."
    ));
    assert!(!goal.contains("in pull request mode run `git fetch origin`"), "{goal}");
    let loose = texts::reviewer_instructions(&env(&d, &t, None), "", false);
    assert!(loose.contains(INTEGRATED_LOOSE));
}

#[test]
fn the_instructions_of_a_goal_task_do_not_mention_pr_url() {
    let t = task("a", TaskKind::Work, false);
    let d = delivery(DeliveryMode::Pr, true);
    let goal = texts::instructions(&env(&d, &t, Some("goal/g")), "", false);
    assert!(goal.contains("(with `commit`)") && !goal.contains("pr_url"), "{goal}");
    assert!(texts::instructions(&env(&d, &t, None), "", false).contains("`pr_url` in PR mode"));
}

#[test]
fn the_texts_of_a_loose_task_never_mention_a_goal_branch() {
    let t = task_from("a", TaskKind::Work, false, "main");
    let r = task_from("rev", TaskKind::Review, false, "main");
    for (mode, parallel) in [(DeliveryMode::Commit, false), (DeliveryMode::LocalMerge, true), (DeliveryMode::Pr, true)]
    {
        let d = delivery(mode, parallel);
        for task in [&t, &r] {
            let e = env(&d, task, None);
            let all = [
                texts::environment(&e),
                texts::delivery_steps(&e),
                texts::instructions(&e, "", false),
                texts::reviewer_instructions(&e, "", false),
            ];
            assert!(all.iter().all(|s| !s.contains("goal/")), "{mode:?}");
        }
    }
}
