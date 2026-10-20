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

Status: step 3a is done (wizards, false-conflict avoidance, duplicated blocks, graph,
double-click switch with `auto/<branch>`, right-click menus, comparison, safe deletion,
detached HEAD banner). Step 3b: the drag and drop gestures, and editing the message of
an older commit that is not sent yet.

## 4. Submodules and trade-specific needs
Submodules (add, update, sync, status), Git LFS and file locking for design binaries,
`.gitignore` templates, portable Windows/macOS/Linux executables via
GitHub Actions.
