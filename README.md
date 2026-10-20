# Simple Git

A graphical interface for Git designed for people who don't know Git (design teams, non-dev people)
 without hiding anything from experienced users: every guided action shows
the exact `git` command it runs.

A portable desktop application written in Rust with [iced](https://iced.rs). It uses the
`git` installed on the machine (no libgit2).

## Requirements

- [Git](https://git-scm.com) installed and available in the `PATH`.
- To build: stable Rust (`rustup`).

## Running

```sh
cargo run --release                 # welcome screen: open or clone a repository
cargo run --release -- ~/project    # opens this repository directly
```

## What the current version does

- Interface in English and French (system language by default, FR/EN switch).
- Style of the approved wireframes: embedded IBM Plex fonts, light and dark themes that
  follow the system (can be forced with `SIMPLE_GIT_THEME=dark` or `light`).
- Open an existing repository or clone one (submodules included).
- See the current branch, how it stands against the server, and the changed files, in
  plain words, with the git notation in advanced mode.
- Commit history of all branches.
- **Git log panel**: every command run, its result and its duration. In advanced mode:
  full outputs and a console to type your own git commands.
- Git can never freeze the interface: every command runs in the background, without a
  terminal or an editor, and is stopped after a maximum delay.

The roadmap is in [docs/PLAN.md](docs/PLAN.md).

## Code layout

- `src/git/runner.rs`: the only place that runs `git` (timeout, non-interactive environment).
- `src/git/status.rs`: parsing of `git status --porcelain=v2`.
- `src/git/repo.rs`: queries on a repository (opening, history, clone, operation in progress).
- `src/app.rs`: application state and screens.
- `src/ui/style.rs`: light and dark palettes, fonts and widget styles; `src/ui/icons.rs`: icons.
- `assets/fonts/`: IBM Plex Sans and Mono (SIL OFL 1.1 license, see `OFL.txt`).
- `locales/app.yml`: every UI text, in English and French side by side. A test checks that
  every key exists in both languages and that every key used in the code exists.

## Tests

```sh
cargo test
```

Some tests create real temporary repositories and run `git`.
