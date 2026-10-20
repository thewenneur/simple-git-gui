# Plan

Audience: a hardware team and a design team who don't know Git, plus experienced
users. General rule: guided choices never hide the matching git command. The interface is
in English and French.
Target visual: the approved wireframes (https://claude.ai/artifact/P4D9EvkYgPTQtZDBw82EjJ).

## 1. Foundation
Rust + iced app that calls the `git` CLI in the background with a timeout (no freeze
possible). Git log panel of every command, console in advanced mode. Open/clone a
repository, file status, history.

## 2. Everyday actions
- Readable diffs, moved lines shown as moved (`--color-moved`, histogram).
- Choose what to keep, "Save" (commit), "Send" (fetch → integrate → push).
- **Ctrl+Z / Ctrl+Y for every action**: snapshot of refs, index and files before each
  action. A push can only be undone by a revert or an explicit force push.
- **Explained "Git configuration" menu**: every setting described, git key visible,
  global or repository scope (identity, pull strategy, `core.autocrlf`, conflict style,
  `rerere`, credentials…).
- **Ignore any file**: right-click a file in the changes list → "Ignore this file",
  "Ignore all `*.ext`" or "Ignore this folder". The line is added to `.gitignore` (shared)
  or to `.git/info/exclude` (just for me), and the added line is shown. If the file is
  already tracked, also offers `git rm --cached` to stop tracking it without deleting it
  from disk. Undoable with Ctrl+Z.

## 3. Branches, conflicts and tree view
- Merge/rebase wizards. Never "ours/theirs": each side carries its branch name, the right
  way round for a merge as well as for a rebase.
- Fewer false conflicts: histogram, `zdiff3`, `rerere`, option to ignore whitespace,
  normalized line endings. Merge first with `--no-commit`, then detect duplicated blocks
  (a line moved on one side) before saving.
- Tree view with drag and drop, with a confirmation showing the exact command:
  - a branch's start commit dropped on another commit → rebase;
  - a commit dropped on the last commit of another branch → merge;
  - double-click a branch → checkout with per-branch autostash (`auto/<branch>`);
  - branch label dragged onto a commit → move the branch (backup first);
  - Ctrl + drag a commit onto a branch → cherry-pick;
  - drag a commit onto its neighbour → reorder / squash (driven interactive rebase);
  - local ↔ remote branch → push (with pull first) / pull;
  - right-click: branch or tag here, diff, revert, edit message, restore a file;
  - two commits selected → comparison;
  - safe branch deletion; detached HEAD banner with "create a branch here".

Status: done. Pushing the current branch by drag and drop opens the usual send wizard,
which gets colleagues' work first.

## 4. Submodules and trade-specific needs
Submodules (add, update, sync, status), Git LFS and file locking for design binaries,
`.gitignore` templates, portable Windows/macOS/Linux executables via
GitHub Actions.

Status: done. Locks need a server with the Git LFS locking API (GitHub, GitLab, Gitea);
lock owners are recognised by the server, or by `user.name` when it can't verify them.

Also (asked during step 4): create a new repository from the welcome screen (folder,
first branch, starting `.gitignore` rules, README and first commit, optional server),
also offered when opening a folder that is not a repository yet; and manage the
servers (remotes) in the Git configuration screen: add, rename, change the address
(with an optional separate send address), remove and test, each undoable with Ctrl+Z.

## 5. Workspaces (git worktrees)
Several branches checked out side by side, each in its own folder, sharing one
repository: build or simulate one branch while editing another, without
switching back and forth or setting work aside.
- A "Workspaces" screen listing every worktree (`git worktree list --porcelain`): folder,
  branch, state (clean, modified, locked, missing folder), with "Open" to switch the
  app to it.
- "New workspace" from a branch or a commit (`git worktree add <folder> <branch>`, or
  `-b <new-branch>`), with a suggested folder next to the repository.
- Drag a branch from the tree view onto the Workspaces item to open it in a new one.
- Remove (`git worktree remove`, refused while it has changes, `--force` in advanced
  mode), prune stale entries (`git worktree prune`), lock/unlock a worktree on a
  removable drive (`git worktree lock --reason`).
- Explain why a branch can't be checked out here when another workspace already has it,
  and offer to open that workspace instead.
- Submodules in a new workspace are downloaded on request (`git submodule update --init`).
- Undo with Ctrl+Z where possible (a removed workspace with no changes can be re-added).

Status: done. Creating, removing, locking and unlocking are undone with Ctrl+Z; a new
branch created with a workspace is deleted on undo only if no commit was added to it.
Not undoable: forced removal (the changes are gone), prune, and a workspace whose
submodules were downloaded (git refuses to remove it without `--force`). Switching to a
branch held by another workspace is explained before anything runs.

## 6. Interactive tutorial
A guided tour on first launch, replayable at any time from the help menu
("Replay the tutorial") and from the start screen.
- Runs on a practice repository created in a temporary folder, so nothing the
  user does in it can touch real work; the git commands it runs show in the Git log
  like everywhere else.
- Steps highlight the real interface (the rest dimmed) and wait for the user to do the
  action: open a repository, look at a change, stage and commit, get and send, create a
  branch, drag a commit in the tree, settle a conflict with the named-side assistant,
  undo with Ctrl+Z, lock a binary file.
- Each step can be skipped; the tour can be left and resumed where it stopped.
- Short tracks per audience (hardware, design, advanced) picked at the start.
- Shown once automatically (remembered in the app's settings), in English and French.
