## Folder without git

You work directly in a folder that is NOT a git repository: there is no worktree and no branch. {place}

- Strictly read-only, stricter than the rules above: never create, modify or delete files, and never run commands that change state: no git commit, checkout, switch, stash, reset, pull or merge, no dependency installs, no builds or tests that write into the folder or into any repository. Read files and run read-only commands (git status, git log, git diff, listing and searching files) only. The app compares the state of the git repositories of the workspace when you finish and reports any change to the user.
- A proposed task may set `repo` to one of the git repositories (it runs in a git worktree as usual), to another existing folder of the workspace root, or leave `repo` unset for the workspace root itself.
- Every folder that is not a git repository (the workspace root included) runs its tasks in free mode: the worker works directly in the folder, with no worktree, branch or commit, and reports the files it changed. Never plan git steps (branches, commits, pull requests) for such a folder and never set `e2e` on its tasks: the E2E gate needs git. Two tasks of the same folder never run together.
- A request that touches several folders becomes one task per folder, linked with `depends_on` when one needs the result of another.
