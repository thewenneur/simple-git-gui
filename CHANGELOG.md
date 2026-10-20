# Changelog

All notable changes to Simple Git are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - Unreleased

First release.

### Added

- Open, clone (with submodules) or create a repository; opening a plain folder offers to
  make it one.
- Changed files in plain words, readable diffs (unified or side by side, whitespace-only
  changes hidden, moved lines shown as moved, code colored for VHDL, Verilog, Tcl/XDC,
  C, Python and more), a large diff view and the whole file with its changes.
- Stage a file or a single block, commit, **Get** (fetch then rebase or merge) and a
  guided **Send** that gets first and shows what will go to the server.
- Ctrl+Z / Ctrl+Y on every action until the next send.
- Ignore a file, an extension or a folder from its menu, shared or only for you, and
  ready-made `.gitignore` rules for Vivado, Quartus, ModelSim/Questa, GHDL/NVC, KiCad,
  Office, VS Code, Python and Rust.
- An explained Git configuration screen and a servers (remotes) card.
- History and branches: a graph of every branch, commit details, comparing two commits,
  browsing the project at a commit, and drag and drop to merge, move (rebase),
  cherry-pick, reorder or squash commits, each confirmed with its git command.
- A conflict assistant that names each side by its branch instead of "ours" and
  "theirs", with fewer false conflicts and duplicated moved blocks.
- Submodules (state, update, open) and Git LFS file locks.
- Workspaces (git worktrees): create, open, lock and remove them.
- An interactive tutorial on a practice repository, offered on first launch.
- Interface in English and French, light and dark themes, and a Git log panel showing
  every command run, with a console in advanced mode.
- App icon, Windows installer (per user, no administrator rights, adds itself to the `PATH`) and portable `.exe`,
  macOS disk image (universal), Linux AppImage and `.tar.gz` with an install script.

[0.1.0]: https://github.com/thewenneur/simple-git-gui/releases/tag/v0.1.0
