# Simple Git

A graphical interface for Git designed for people who don't know Git (design teams, non-dev people)
 without hiding anything from experienced users: every guided action shows
the exact `git` command it runs.

A portable desktop application written in Rust with [iced](https://iced.rs). It uses the
`git` installed on the machine (no libgit2).

## Requirements

- [Git](https://git-scm.com) installed and available in the `PATH`.
- Optional: [Git LFS](https://git-lfs.com) for large binary files and file locking.
- To build: stable Rust (`rustup`).

## Download

Installers and portable executables are attached to each
[release](https://github.com/thewenneur/simple-git-gui/releases):

- **Windows**: a setup `.exe` that installs for the current user without administrator
  rights, or a single portable `.exe` to run from anywhere.
- **macOS** (Apple Silicon and Intel): a `.dmg`; drag Simple Git onto Applications. It is
  not notarized, so open it the first time with right-click → Open.
- **Linux**: an AppImage, or a `.tar.gz` whose `install.sh` adds Simple Git to the
  applications menu of the current user.

They are built by `.github/workflows/release.yml` when a `v*` tag is pushed, or on demand
from the Actions tab. See [docs/RELEASING.md](docs/RELEASING.md) and the
[changelog](CHANGELOG.md).

## Running

```sh
cargo run --release                 # welcome screen: open or clone a repository
cargo run --release -- ~/project    # opens this repository directly
```

## What the current version does

- Interface in English and French (system language by default, FR/EN switch).
- Style of the approved wireframes: embedded IBM Plex fonts, light and dark themes that
  follow the system (can be forced with `SIMPLE_GIT_THEME=dark` or `light`).
- Open an existing repository, clone one (submodules included) or create a new one
  (first branch, starting `.gitignore` rules, README and first commit, server address).
  Opening a plain folder offers to make it a repository.
- See the current branch, how it stands against the server, and the changed files, in
  plain words, with the git notation in advanced mode.
- Readable diffs: unified or side by side, whitespace-only changes hidden, moved lines
  shown as moved instead of deleted and re-added, code colored by language (VHDL,
  Verilog, Tcl/XDC, C, Python...). **Open large** shows a diff over the whole window,
  optionally the whole file with its changes.
- Stage a whole file or a single block, write the message and commit.
- **Get** and **Send**: fetch then integrate (rebase by default, or merge), and a guided
  send that shows what will go to the server before pushing. A conflict can be aborted
  in one click.
- **Ctrl+Z / Ctrl+Y** on every action (stage, commit, get, ignore, settings) until the
  next send.
- Right-click a file to ignore it (the file, its extension or its folder), shared in
  `.gitignore` or only for you in `.git/info/exclude`.
- An explained **Git configuration** screen (identity, sync, conflicts, line endings),
  per repository or for every repository, showing the exact `git config` command.
- **Servers (remotes)**: add, rename, change the address (and a separate send address),
  remove and test a server, each with its command shown and undoable with Ctrl+Z.
- **History and branches**: a branch graph of every branch, the details of a commit
  (files and diffs), and the comparison of two commits (Ctrl+click) or of a commit with
  your files. **Browse the project at this commit** shows its folders and the content of
  any file as it was then.
- Right-click a commit or a branch: create a branch or a tag, copy a commit onto your
  branch (cherry-pick), undo a commit with a new one (revert), edit the last message,
  restore a file as it was, merge, rebase, switch, delete a branch safely (a branch
  whose work is not merged anywhere asks first). Each action shows its git command.
- Double-click a branch to go onto it: your unsaved changes are set aside for that
  branch (`auto/<branch>` stash) and come back when you return to it.
- A banner when you are on an old commit outside any branch (detached HEAD), with
  "create a branch here" and "go back".
- **Drag and drop** in the history, always confirmed with the exact command: a commit of
  your branch dropped on another commit or branch rebases from there; another commit
  dropped on your branch merges it; Ctrl+drag copies it (cherry-pick); a branch label
  dropped on a commit moves the branch (its old position is kept in `refs/backup/`);
  a label dropped on its server counterpart sends or gets; two neighbouring commits not
  sent yet are swapped or squashed. The message of an older commit not sent yet can be
  edited too (the app writes the interactive rebase's to-do list itself).
- **Merge and rebase assistant** that never says ours/theirs: each side carries its
  branch name, the right way round for a merge and for a rebase. Fewer false conflicts
  (`zdiff3`, `rerere`, histogram diff, whitespace ignored), and a merge is checked
  before it is saved: blocks found twice because one side moved them are pointed out
  and can be removed in one click.
- **Submodules**: every submodule with its state in plain words (up to date, new commits
  on its server, changed here, on another commit than the recorded one, not downloaded),
  the pinned and latest commits, and one-click download, update (then recorded in this
  repository), go back, record, sync all and add. "Open" switches to the submodule, with
  a banner to come back.
- **Locked files** (Git LFS): who holds which binary file and since when, lock and
  release in one click (Ctrl+Z gives it back), copy a colleague's name, choose which file
  types go to LFS (`git lfs track --lockable`) and set LFS up in the repository.
- **Workspaces** (git worktrees): several branches checked out side by side, each in its
  own folder sharing the repository. See each one's branch and state (changed files,
  locked, folder missing), open it, create one from a branch, a new branch or a commit
  (also by dropping a branch or a commit on the Workspaces item), with its submodules
  if wanted, remove it (forcing needs advanced mode and a confirmation), lock it with a
  reason, unlock it and forget missing folders. Ctrl+Z undoes them where possible. Going
  onto a branch already open in another workspace says so and offers to open it.
- An **interactive tutorial**, offered on first launch and replayable from the **?** button
  or the start screen: pick a track (hardware, design, advanced) and do each step for
  real on a practice repository in a temporary folder, with the right area of the window
  highlighted. Steps can be skipped, and the tour left and resumed where it stopped.
- Ready-made **`.gitignore` rules** for Vivado, Quartus, ModelSim/Questa, GHDL/NVC, KiCad,
  Office, the three systems, VS Code, Python and Rust, added without duplicates and undoable.
- **Git log panel**: every command run by your actions, its result and its duration.
  Read-only commands (`status`, `diff`, `log`...) and automatic refreshes are hidden
  by default. In advanced mode: a switch to show them, full outputs and a console to
  type your own git commands.
- Light on memory: on Windows and Linux the interface is drawn by the CPU (iced's
  tiny-skia renderer, about 25 MB) since the GPU driver alone would add 100+ MB; on
  macOS it is drawn by the GPU (Metal), which is cheaper there than keeping CPU-drawn
  Retina frames. Force either with `SIMPLE_GIT_GPU=1` (GPU) or `SIMPLE_GIT_GPU=0` (CPU).
- Git can never freeze the interface: every command runs in the background, without a
  terminal or an editor, and is stopped after a maximum delay.

The roadmap is in [docs/PLAN.md](docs/PLAN.md).

## Code layout

- `src/git/runner.rs`: the only place that runs `git` (timeout, non-interactive environment).
- `src/git/status.rs`: parsing of `git status --porcelain=v2`.
- `src/git/repo.rs`: queries on a repository (opening, history, clone, operation in progress).
- `src/git/ops.rs`: the commands of everyday actions (stage, commit, fetch, integrate, push, abort).
- `src/git/diff.rs`: diff parsing and moved-line detection; `src/git/ignore.rs`: ignore rules.
- `src/git/undo.rs`: snapshots of HEAD, the index and branches, used to undo and redo actions.
- `src/git/graph.rs`: lanes of the branch graph; `src/git/branches.rs`: switching with
  per-branch set-aside, branches, tags, cherry-pick, revert and commit contents.
- `src/git/integrate.rs`: merge and rebase with fewer false conflicts, and the branch
  name of each side; `src/git/conflict.rs`: conflict parsing and duplicated blocks;
  `src/git/rewrite.rs`: swap, squash and reword through a prepared interactive rebase.
- `src/git/config.rs`: the settings of the Git configuration screen; `src/git/remotes.rs`:
  remotes and `git init`.
- `src/git/worktrees.rs`: workspaces (`git worktree`) listing and commands.
- `src/git/practice.rs`: the tutorial's practice repository, its server and colleague.
- `src/git/submodules.rs`: submodule state and commands; `src/git/lfs.rs`: Git LFS
  patterns and locks; `src/git/templates.rs`: the `.gitignore` templates.
- `src/app/mod.rs`: application state, messages and the main layout; `src/app/changes.rs`
  (changed files, diff, commit), `src/app/sync.rs` (get and send), `src/app/undo.rs`
  (Ctrl+Z / Ctrl+Y), `src/app/config.rs` (Git configuration screen), `src/app/history.rs`
  (graph, commit details, menus and dialogs), `src/app/integrate.rs` (merge and rebase
  assistant), `src/app/dragdrop.rs` (drag and drop gestures and their confirmation),
  `src/app/modules.rs` (submodules, locked files and `.gitignore` templates),
  `src/app/remotes.rs` (servers card and new repository form), `src/app/worktrees.rs`
  (workspaces screen), `src/app/viewer.rs` (large diff view, project at a commit),
  `src/app/tutorial.rs` (tracks, lessons, guide card, highlights).
- `src/settings.rs`: the app's own settings (`settings.txt` in the configuration folder,
  or in `SIMPLE_GIT_CONFIG_DIR`).
- `src/ui/style.rs`: light and dark palettes, fonts and widget styles; `src/ui/icons.rs`: icons;
  `src/ui/syntax.rs`: syntax highlighting.
- `assets/fonts/`: IBM Plex Sans and Mono (SIL OFL 1.1 license, see `OFL.txt`).
- `locales/app.yml`: every UI text, in English and French side by side. A test checks that
  every key exists in both languages and that every key used in the code exists.
- `assets/icon/`: the app icon (`simple-git.svg`) and the files rendered from it.
- `packaging/`: the Windows installer script, the macOS `Info.plist`, the Linux menu
  entry and install script; `build.rs` embeds the icon in the Windows executable.

## Tests

```sh
cargo test
```

Some tests create real temporary repositories and run `git`.
