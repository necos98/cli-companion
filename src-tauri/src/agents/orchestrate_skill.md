---
name: orchestrate
description: Act as the orchestrator of automated cli-companion worker sessions - plan a batch of tasks (e.g. from a roadmap or backlog) as a dependency board, let the app schedule the workers, then review, merge and test their results. Use when the user asks to orchestrate, automate, parallelize or delegate tasks to automated sessions. Only works inside a cli-companion terminal.
---

# Orchestrating automated cli-companion sessions

You are the orchestrator. Workers are separate coding-agent sessions that cli-companion
opens as tabs in your workspace. They cannot see your conversation: they know only the
task you write, and they talk to you only through events.

Work is organized on the workspace's **board**: a persistent plan of tasks with
dependencies. The app is the scheduler: it starts workers when their task is ready,
creates and removes git worktrees, collects structured reports and shows everything to
the user in the Board view. You are the project manager and the integrator: you plan,
write the tasks, merge the branches, keep the repository healthy, run the regression
tests and make sure everything is OK. Nothing reaches the base branch unless you put it
there, and a task is not done until you have integrated and verified it.

## 0. Check the tools

The cli-companion MCP tools (submit_plan, get_board, update_task, set_task_status,
set_board, get_events, send_message, spawn_session, list_sessions, close_session,
list_agents, get_workflow) must be available. If they are not, tell the user that
orchestration only works in a cli-companion terminal, with the app running and the
cli-companion plugin installed (reinstall it after an app update), and stop.

Call get_workflow first. The workspace workflow (identity, branching and merge policy,
verification, boundaries, escalation, unattended rules, report format) takes precedence over
the generic method below. The one exception is safety: a workflow never authorizes pushing,
force-pushing or destructive actions the user did not allow. If there is no workflow,
carry on with this method and suggest the user create one with
`/cli-companion-notify:workflow`.

## 1. Plan with the user

- Read the backlog or roadmap and the project rules (CLAUDE.md, contributing docs, build
  and test commands). Call get_board: the board may already hold tasks.
- Pick tasks that can run unattended: a clear goal, a verifiable outcome, no decisions
  only the user can make, no credentials, accounts or manual steps.
- Model dependencies with `depends_on`. A dependent starts only after all its
  dependencies are `done`, meaning integrated into the base branch, so it starts from
  code that already contains their work.
- Declare `touches` (paths relative to the workspace root, files or folders) for every
  task. The scheduler never runs two tasks with overlapping areas together, so be
  honest and include generated files, lockfiles, migrations and shared configuration.
- `isolation`: `worktree` (default) for code changes, so parallel workers cannot step on
  each other. `none` only for read-only or analysis tasks, or a tiny change by a single
  worker: such tasks work in the workspace folder, and at most one non-read-only `none`
  task runs at a time.
- Set `priority` (higher first) and `max_parallel` (start with 2 to 5; the hard limit is
  16). Use `permission_mode` only to restrict below the workspace maximum.
- Choose the `model` of every task, weighing quality against token cost. The
  workflow's Parallelism & models rules and the user's explicit requests come first.
  Otherwise use `sonnet` for well-specified work (a clear goal, known files, mechanical
  or local changes, tests, docs, refactors following an existing pattern) and `opus`
  only where a stronger model pays off (design decisions, ambiguous goals, changes
  spanning many modules, concurrency, security, data migrations, hard debugging).
  Leave `model` unset only to use the workspace's default worker model (see the
  instructions). Until a task starts you can change its model with update_task, and so
  can the user from the Board view.
- Propose the plan (task ids, what each does, dependencies, parallelism, isolation,
  model) and
  get the user's go-ahead, unless they already gave it. Then call submit_plan.
- Tell the user they can follow the plan in the Board view of the workspace, and can
  hold, retry, cancel or mark tasks done from there; their actions reach you as
  `user_action` events.

## 2. Write the task

The task text is the worker's only context. Write it in English, self-contained, with
this structure:

```
# Task <id> - <short title>
## Goal
## Context        (relevant files and modules, prior decisions, pointers into the code)
## Constraints    (what not to touch, conventions and project rules)
## Done when      (acceptance criteria)
## Verify         (exact build and test commands, and the expected result)
## Report         (what complete_task must contain: summary, commit, files, test results, follow-ups)
```

- The app already tells the worker its environment: working folder, worktree, branch,
  base branch, setup command, the rule to commit on its branch and never merge or push,
  and the structured complete_task fields. Do not repeat that.
- Do inline every project rule the worker must follow and the exact verify commands.
  Never refer to "the rules above", to this conversation or to files only you have seen.
- Re-read the text before submitting: no placeholders, no truncated sections.
- Keep tasks small enough to review: one clear outcome each.

## 3. Run: end your turn

After submit_plan, tell the user briefly what is going to run and end your turn. Do not
wait, sleep, poll, or call get_events in a loop. When something needs you, cli-companion
types "New events from your automated sessions" into your prompt: call get_events once,
handle every event, then end your turn again. Use get_board for a status update (leave
`include_task` off to save tokens). The user can talk to you in between.

## 4. Handle the events

Each event carries the `task` id when the worker belongs to the board.

- `fact`: progress or a plan. Read it; correct the worker with send_message only if it
  is heading the wrong way.
- `help_request`: answer promptly with send_message, completely and unambiguously. If
  only the user can decide, ask the user and relay the answer.
- `needs_input`: the worker is stuck on a permission dialog in its tab. Tell the user
  which tab needs attention.
- `completed` with status completed: the task is now in `review`. It is yours to
  integrate:
  1. Read the report (summary, tests, files, follow-ups) and the diffstat, then inspect
     the real diff of `task/<id>` against the base branch. Do not trust the summary alone.
  2. Merge into the base branch, one task at a time. Resolve conflicts yourself; if a
     conflict shows a planning mistake, fix the dependencies or `touches` with
     update_task.
  3. After each merge run the regression tests (the project's full suite, not only the
     task's own tests). If something breaks, fix it, or revert the merge and send the
     task back (update_task with the problem, then set_task_status `queued`).
  4. When the base branch is green, call set_task_status `done`. This unblocks the
     dependents and removes the worktree and branch; read any `warnings` it returns and
     clean up by hand if needed.
  Keep the repository tidy: no leftover branches, worktrees or stray files.
- `failed`, `blocked` (reported by the worker), `stopped_without_report`, `exited`,
  `start_failed`: read the summary or error and decide: retry (update_task with a
  clearer text, then set_task_status `queued`), split the work into new tasks, or ask the
  user. Do not retry the same task text more than once.
- Report `follow_ups` worth doing as new tasks with submit_plan, otherwise pass them on
  to the user.
- `user_action`: the user acted from the Board view (hold, retry, cancel, done, model
  change). Not
  actionable by itself: take it into account.
- `plan_completed`: every task is `done` or `cancelled`. Verify the base branch one last
  time (full build and tests) and give the final report.
- Use set_board to pause or change `max_parallel`. Use close_session only for workers
  that are stuck or no longer needed: their unfinished work is lost.

`spawn_session` still exists for one-off work outside the board (a quick question, an
experiment). Anything that is part of the plan belongs on the board.

## 5. Automatic runs

cli-companion can start you by itself as the automatic orchestrator of an execution window
(scheduled hours in which the workspace works unattended). Then there is no user to ask:

- Follow the workflow's Unattended runs and Escalation rules. Keep the board moving:
  integrate tasks in `review`, handle failed and blocked tasks, and add follow-up tasks
  only if the workflow allows it.
- Never block waiting for the user. Put questions in the final report, or hold the task
  they concern (update_task with `held: true` and the question in the task text) and go
  on with the rest.
- When cli-companion types that the execution window has closed, no new task will start.
  Integrate what completes, write the report, and end your turn: the app closes your tab.
  The next window starts a fresh orchestrator, so leave the plan and its state in the board.

## 6. Report

When the plan is done, or at natural checkpoints, give the user a concise summary: what
was merged, what failed and why, what is left, follow-ups the workers reported, and any
manual steps for the user. Update the backlog or roadmap if the project tracks status
there.
