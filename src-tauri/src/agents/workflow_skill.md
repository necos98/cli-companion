---
name: workflow
description: Create or update the cli-companion orchestration workflow of the current workspace - a wizard that interviews the user about the project (branching, merge policy, verification, task size, parallelism, boundaries, unattended runs, reporting) and saves a tailored workflow.md that the workspace's orchestrator follows. Use when the user wants to create, review or update the cli-companion workflow, or set up the automatic orchestrator of a workspace. Only works inside a cli-companion terminal.
---

# Creating the workspace workflow

The workflow is a short markdown document stored by cli-companion for this workspace. The
orchestrator (the session that plans tasks on the board and integrates the workers' results)
reads it with `get_workflow` before planning and follows it over its generic method. Your job
is to interview the user and write it, so that an orchestrator running unattended at night
behaves the way this user and this project want.

## 0. Check the tools

The cli-companion MCP tools `get_workflow` and `save_workflow` must be available. If they are
not, tell the user that this wizard only works in a terminal opened by cli-companion, with the
app running and the cli-companion plugin installed (reinstall it after an app update), and
stop. Do not write the workflow to a file instead.

## 1. Load what exists

Call `get_workflow`. If a workflow exists, show a short summary of it and ask whether to
update it (change only what the user names, keep the rest) or start over. Never discard an
existing workflow without being told to.

## 2. Investigate the repository first

Before asking anything, learn the project so that your questions come with informed
defaults. Read what exists: README, CLAUDE.md or AGENTS.md, contributing docs, package
manifests (package.json, Cargo.toml, pyproject.toml, go.mod, ...), lockfiles, CI
configuration, test and lint scripts, Makefile or task runner files. Look at git: current
and base branch (`git branch -a`, default branch), recent history (`git log --oneline -30`)
for the commit message convention, whether the repository is pushed to a remote, and whether
`.worktrees/` is ignored. Note the exact build, test and lint commands, how long they take,
and what setup a fresh checkout needs (for example `npm ci`). Do not modify anything.

## 3. Interview the user

Talk in the user's language. Ask a few questions at a time, never a long form. Use your
multiple-choice question tool when you have one (in Claude Code, AskUserQuestion), with the
recommended option first and derived from what you found in step 2; otherwise ask in plain
text and propose a default for each question. Skip what the repository already answers
unambiguously, but confirm it in the summary. Cover all these topics:

1. Identity and goals: what the project is, who it is for, what the orchestrator is working
   toward, current priorities, the roadmap or backlog file if any.
2. Branching: base branch, whether task branches are merged locally only, and whether the
   orchestrator may ever push (default: never).
3. Merge policy: merge commit, squash or rebase; whether the orchestrator merges by itself
   once the checks pass or waits for the user.
4. Verification: exact build, test and lint commands; which regression suite must pass after
   every merge; what to do on a failure (fix, revert the merge, send the task back).
5. Commit conventions: message format, language, sign-off rules.
6. Task design: how big a task should be, what makes a good task, what is never delegated,
   worktree setup (the command a fresh worktree needs).
7. Parallelism and models: how many workers at once, which model or effort for which kind of
   task (for example a stronger model for design or risky changes, a faster one for
   mechanical work), cost or rate-limit concerns.
8. Boundaries: areas, files and operations that must not be touched (secrets, migrations,
   generated files, vendored code, production configuration, dependency upgrades, public
   API changes).
9. Escalation: what the orchestrator decides alone and what needs the user, and how to ask.
10. Unattended runs: what to do when the app starts the orchestrator in an execution window
    and nobody is there: may it add follow-up tasks, retry failed tasks (how many times),
    keep going after a failed regression, or must it stop and report; where questions
    wait for the user (held tasks, final report).
11. Reporting: format, language and level of detail of the final report, and where it goes
    (the terminal, a file in the repository, the backlog).

Do not ask about things that make no sense for the project; say so in the workflow instead.

## 4. Write workflow.md

Write the workflow in English, since the orchestrator reads it. Use exactly these sections,
in this order:

```
# Workflow - <project name>
## Identity
## Goals
## Branching & merge
## Verification
## Task design
## Parallelism & models
## Boundaries
## Escalation
## Unattended runs
## Reporting
```

Rules for the content:

- Concrete, imperative rules the orchestrator can follow without guessing: "Merge with
  `git merge --no-ff task/<id>`", "Run `npm test` and `npm run lint` after every merge".
- Exact commands, branch names and paths from the repository. No placeholders, no "TBD", no
  vague advice, no restating of the generic orchestrate method. Only what is specific to
  this project and this user.
- State defaults explicitly: never push, never force-push, never delete branches that are
  not task branches, never touch what the Boundaries section lists.
- Keep it short enough to read in one go (usually 40 to 120 lines). If the user changed
  nothing in a section, keep the existing text.
- In Reporting, include a short template of the final report, for example:

```
## Report <date>
- Merged: <task ids and one line each>
- Failed or held: <task ids, reason>
- Verification: <commands run, result>
- Follow-ups and questions for the user: <list>
- Manual steps: <list or none>
```

## 5. Confirm and save

Show the whole document to the user and ask for confirmation or edits, in the user's
language. Apply the edits and show the changed parts again. Only when the user approves, call
`save_workflow` with the full markdown. Then tell the user:

- the workflow is saved and the orchestrator will read it at the start of every run;
- it can be viewed and edited by hand in the workspace settings of cli-companion;
- they can run this wizard again at any time with `/cli-companion-notify:workflow` to
  change it.
