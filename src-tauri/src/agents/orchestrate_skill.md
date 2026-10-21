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

The cli-companion MCP tools (submit_plan, get_board, get_task_details, update_task, set_task_status,
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
  honest and include generated files, lockfiles, migrations and shared configuration. Too
  broad serializes tasks needlessly; too narrow lets conflicting tasks run in parallel.
- `isolation`: `worktree` (default) for code changes, so parallel workers cannot step on
  each other. `none` only for read-only or analysis tasks, or a tiny change by a single
  worker: such tasks work in the workspace folder, and at most one non-read-only `none`
  task runs at a time.
- Multi-repository workspaces: if the workspace folder is not a git repo but holds several
  (get_board lists them in `repos`), set `repo` on every task (folder name relative to the
  workspace root; `touches` are then relative to that repo, and tasks of different repos
  never conflict). Set each repo's base branch with `base_branches` (e.g.
  `{"freud-manager": "master", "DDB": "main"}`; otherwise the default branch of its origin,
  after a fetch). Consider `integration_branch` (e.g. `integration/{date}`): the app creates
  that branch (next free suffix -2/-3 if the name exists) and a worktree
  `<repo>/.worktrees/integration` per repo, and every task of the repo starts from it.
  - `integrations` in get_board: `{branch, path, base, setup}` per repo, present after the
    repo's first worktree task starts. setup: `done`; `none` = no setup configured, nothing
    prepared; `pending` = not completed. `missing: true` = the worktree folder is gone.
  - Merge reviewed `task/<id>` branches into the integration branch from that worktree and
    run the tests there. The app never pushes or switches branches in the main checkout, and
    neither do you unless the user or the workflow says so.
  - To start over on a repo: set_board `close_integration` (repo key or array; `.` = root)
    only forgets the registration, leaving worktree and branch to you; the next task creates
    a fresh integration branch from the updated base. With `close_integration_cleanup: true`
    the app also runs the cleanup hooks, removes the worktree and deletes the local branch
    only if contained in its upstream; otherwise remove them yourself first.
- Setup and cleanup hooks (per repo, edited only by the user in the workspace settings; read
  them in get_workflow `repo_settings`): the app copies git-ignored config files and runs the
  setup commands in every new task and integration worktree, and runs the cleanup commands
  (then removes the worktree) when a task is done. A worker starts as soon as its worktree
  and setup are done, so you cannot prepare a worktree beforehand: put preparation (e.g.
  composer install) in the setup hooks, never in task texts. A failed setup fails the start
  (`start_failed`, failing step in the task note); a retry runs it again.
- Set `priority` (higher first) and `max_parallel` (start with 2 to 5; the hard limit is
  16; also 16 active workers per orchestrator). Use `permission_mode` only to restrict below
  the workspace maximum: `plan` (read-only), `default` (asks on every action, so the worker
  sticks waiting for the user), `accept-edits`, `bypass` (no prompts; widening is refused).
- Limits: 50 tasks per submit_plan call, 200 per board. submit_plan validates everything
  first (unique ids, existing dependencies, no cycles, permissions, git repo for worktree
  tasks): on error nothing is added. You become the board owner.
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
- The workspace rules set by the user are already sent to every worker (board and
  spawn_session): do not repeat them; read them with get_workflow (`workspace_rules`).
- Do inline every project rule the worker must follow and the exact verify commands.
  Never refer to "the rules above", to this conversation or to files only you have seen.
- Re-read the text before submitting: no placeholders, no truncated sections.
- Keep tasks small enough to review: one clear outcome each.

## 3. Run: end your turn

After submit_plan, tell the user briefly what is going to run and end your turn. Never
wait, sleep, poll, or call get_events in a loop. When something needs you, cli-companion
types "New events from your automated sessions" into your prompt: call get_events once,
handle every event, then end your turn again. The board owner is the last orchestrator
tab that called a board tool; the events go to its queue. Only help_request, completed,
needs_input, stopped_without_report and exited wake you while idle; started and fact stay
queued until the next get_events. Workers talk to you only through their tools
(report_fact, request_help, complete_task): never read their terminals.

Derived columns in get_board: `backlog` (waiting on dependencies), `ready` (startable,
waiting for a slot or a conflicting task), `running`, `review` (worker reported completed,
yours to integrate), `done`, `failed`, `blocked`, `cancelled`.

Use get_board for a status update; never poll it. It returns settings (max_parallel,
paused, base_branch, base_branches, integration_branch, `integrations`, `repos`),
`automation`, `counts` per column and one short row per open task: id, title, column,
attempts, and when they apply held (only queued tasks; the top-level `held` counts them),
external, worker_status, waiting_on (backlog), priority, model, note, orchestrator_note,
summary (note, orchestrator_note and summary are truncated: get_task_details has the full
text, report, diffstat, worktree and note of one task). done and cancelled tasks are only
counted; pass `columns` (e.g. ["done"]) to list them. `automation` is null for a normal
workspace; otherwise it says whether execution windows are enabled, whether the window is
open now and when it changes (`next_change_at`, unix ms): while closed, no new task starts.
Tool results are already structured: read them directly and never parse them with shell
scripts (python, jq, PowerShell). The user can talk to you in between.

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
  1. Call get_task_details for the report (summary, tests, files, follow-ups) and the
     diffstat, then inspect
     the real diff of `task/<id>` against the base branch. Do not trust the summary alone.
  2. Merge into the base branch, one task at a time. Resolve conflicts yourself; if a
     conflict shows a planning mistake, fix the dependencies or `touches` with
     update_task.
  3. After each merge run the regression tests (the project's full suite, not only the
     task's own tests). If something breaks, fix it, or revert the merge and send the
     task back (update_task with the problem, then set_task_status `queued`).
  4. When the base branch (or the integration worktree) is green, call set_task_status `done`. This unblocks the
     dependents and removes the worktree and branch (`git branch -D` when merged into the
     repo's integration branch or the worktree base, else `-d` with a warning; no force).
     Problems come back as `warnings` and in the task note, never blocking the change:
     clean up by hand if needed. `cleanup: false` keeps worktree and branch. update_task,
     set_task_status and submit_plan also return `warnings` when the board is paused.
     done works from review, failed or blocked, never from running.
  A task that never started and has no dependents can be removed with set_task_status
  `deleted`.
  If a queued (even held) or cancelled task was done outside the board, call set_task_status `done` on
  it with a `note` saying how (required); it is shown as completed outside the board and
  unblocks its dependents (done also clears `held`).
  `cancelled` works from any status but done: a running task's tab is closed and its work
  lost; dependents stay in backlog until you update their dependencies. `queued` retries a
  failed, blocked, cancelled or review task and reuses its worktree; the old note is kept
  unless you pass a new `note`.
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
- `plan_completed`: every task is `done` or `cancelled` (the text gives the counts; it is not
  sent if you cancelled everything before any task started). Verify the base branch one last
  time (full build and tests) and give the final report.
- Use set_board to pause (running workers go on) or change `max_parallel`. Use
  close_session only for workers that are stuck or no longer needed: their unfinished work
  is lost.
- update_task changes any submit_plan field of a task that is queued, failed, blocked or
  cancelled (not running, review or done); `isolation` is fixed once it has a worktree,
  dependencies are re-validated, omitted fields stay, `null` clears base_branch, model and
  permission_mode.
- `start_failed` text holds the reason (e.g. a git error).

Held tasks are not started: release them with update_task `held: false`. `held` stays on
cancelled tasks, so a retry (set_task_status `queued`, which keeps the task note) stays held
until released. Use `orchestrator_note` on a task for private reminders to
yourself (merge order, what to check): it is never sent to the worker. For news prefer
get_events over list_sessions. list_sessions statuses: starting (task not read yet; see
`starting_secs`: seconds is normal, minutes means stuck), running, background (waiting on a
background command, resumes by itself), needs_input, idle (stopped without complete_task),
completed, failed, blocked, closed; it includes the last report.

`spawn_session` is for one ad-hoc session outside the board (a quick question, an
experiment); a dependent batch belongs on the board. It returns at once with the session id:
end your turn. Its `cwd` must be an existing absolute path equal to or inside the workspace
folder (no `..`), in the workspace's environment; for git worktrees use a subfolder such as
`.worktrees/<name>`, which you create and clean up yourself. Other options: `agent` (see
list_agents; default claude-code), `permission_mode`, `model`, `keep_open` (tab stays open
after complete_task). Write the `task` and any send_message text in English.
get_workflow also returns, read-only, `workspace_rules` and `repo_settings` (base_branch,
container_root, copy_ignored, setup, cleanup, remove_force); only the user edits them.
Never call save_workflow yourself: it belongs to the workflow wizard.

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

## 6. Always available

The app starts a new task only while an orchestrator is alive in the workspace (the board
owner tab, one the user opened, or one the app opened). Running workers are never stopped.
If the board needs an orchestrator (startable tasks, tasks in review, pending help requests
or events) and none is alive, the app opens one by itself, with a prompt telling it to load
this skill, call get_workflow, get_board and get_events and carry the plan forward. So:

- Ending your turn is always safe. If your tab is closed, the app reopens an orchestrator
  when work appears; when you are idle and the board needs nothing more, the app closes
  your tab.
- A new orchestrator must start with get_workflow, get_board and get_events to pick up
  pending events and the state of the plan; everything is kept in the board.
- If the user closes the orchestrator the app opened, or it fails to start 3 times, the
  app does not reopen it: the Board view shows a button to start one.

## 7. Report

When the plan is done, or at natural checkpoints, give the user a concise summary: what
was merged, what failed and why, what is left, follow-ups the workers reported, and any
manual steps for the user. Update the backlog or roadmap if the project tracks status
there.
