//! Undo and redo of the app's actions.
//!
//! Before and after each action the app records where HEAD points and what
//! the index contains (as a tree written with `git write-tree`). Undoing puts
//! HEAD and the index back; redoing moves them forward again. Files on disk
//! are never overwritten, except ignore files the app wrote itself.
//!
//! An undo only runs if the repository is still exactly as the action left it,
//! so a change made meanwhile (in a terminal, another tool...) is never lost.

use std::path::{Path, PathBuf};

use super::config::{self, Scope};
use super::runner::{CommandRecord, GitCommand, Outcome, run};

/// Where HEAD points and what the index holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    /// The current commit, `None` before the first commit.
    pub head: Option<String>,
    /// The current branch (`refs/heads/…`), `None` when HEAD is detached.
    pub branch: Option<String>,
    /// The index as a tree, `None` while it has conflicts.
    pub tree: Option<String>,
}

/// A file the app rewrote (e.g. `.gitignore`), with both versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A git setting the app changed, with both values (`None`: not set).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEdit {
    pub scope: Scope,
    pub key: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// How HEAD is moved back or forth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moves {
    /// Only the index changed (stage, unstage).
    Index,
    /// HEAD moved by a commit: the files on disk stay as they are.
    Commit,
    /// HEAD moved by a rebase or merge: the files follow, local changes kept
    /// (`git reset --keep`).
    Integration,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// What the action was, in the user's words.
    pub label: String,
    pub moves: Moves,
    pub before: State,
    pub after: State,
    pub file: Option<FileChange>,
    pub config: Option<ConfigEdit>,
}

impl Entry {
    /// Whether the action changed anything worth undoing.
    pub fn changed_something(&self) -> bool {
        self.before != self.after
            || self.file.as_ref().is_some_and(|f| f.before != f.after)
            || self.config.as_ref().is_some_and(|c| c.before != c.after)
    }

    /// What to do to go back (`undo`) or forward again.
    pub fn step(&self, undo: bool) -> Step {
        let (expected, target) = if undo {
            (&self.after, &self.before)
        } else {
            (&self.before, &self.after)
        };
        Step {
            moves: self.moves,
            expected: expected.clone(),
            target: target.clone(),
            file: self.file.as_ref().map(|f| {
                let content = if undo { &f.before } else { &f.after };
                (f.path.clone(), content.clone())
            }),
            config: self.config.as_ref().map(|c| {
                let value = if undo { &c.before } else { &c.after };
                (c.scope, c.key.clone(), value.clone())
            }),
        }
    }
}

/// One move of the repository, checked against `expected` first.
#[derive(Debug, Clone)]
pub struct Step {
    pub moves: Moves,
    pub expected: State,
    pub target: State,
    /// A file to rewrite with this content (`None`: delete it).
    pub file: Option<(PathBuf, Option<String>)>,
    /// A setting to put back to this value (`None`: unset).
    pub config: Option<(Scope, String, Option<String>)>,
}

/// Reads the current [`State`]. The commands are background ones: they are
/// bookkeeping, not actions of the user.
pub async fn snapshot(root: &Path) -> (State, Vec<CommandRecord>) {
    let head = run(GitCommand::new(["rev-parse", "-q", "--verify", "HEAD"])
        .in_dir(root)
        .background());
    let branch = run(GitCommand::new(["symbolic-ref", "-q", "HEAD"])
        .in_dir(root)
        .background());
    let tree = run(GitCommand::new(["write-tree"]).in_dir(root).background());
    let (head, branch, tree) = tokio::join!(head, branch, tree);
    let value = |r: &CommandRecord| r.success().then(|| r.stdout.trim().to_string());
    let state = State {
        head: value(&head),
        branch: value(&branch),
        tree: value(&tree),
    };
    (state, vec![head, branch, tree])
}

/// The commands that move the repository from `from` (the current state) to
/// `to`.
fn commands(root: &Path, moves: Moves, from: &State, to: &State) -> Vec<GitCommand> {
    let mut commands = Vec::new();
    if from.head != to.head {
        let command = match (&to.head, &to.branch) {
            (Some(target), _) => {
                let mode = if moves == Moves::Integration {
                    "--keep"
                } else {
                    "--soft"
                };
                GitCommand::new(["reset", mode, target.as_str()])
            }
            // Back before the first commit: the branch did not exist yet.
            (None, Some(branch)) => GitCommand::new(["update-ref", "-d", branch.as_str()]),
            (None, None) => return Vec::new(),
        };
        commands.push(command.in_dir(root));
    }
    if let Some(tree) = &to.tree
        && (from.tree.as_ref() != Some(tree) || from.head != to.head)
    {
        commands.push(GitCommand::new(["read-tree", tree.as_str()]).in_dir(root));
    }
    commands
}

/// Why an undo or redo did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The repository changed since the action: undoing could lose work.
    ChangedMeanwhile,
    /// A command failed; its error summary.
    Failed(String),
}

/// Moves the repository as `step` says, if it is still in the expected state.
pub async fn apply(root: PathBuf, step: Step) -> (Result<(), Refusal>, Vec<CommandRecord>) {
    let Step {
        moves,
        expected,
        target,
        file,
        config,
    } = step;
    let (current, mut records) = snapshot(&root).await;
    if current != expected {
        return (Err(Refusal::ChangedMeanwhile), records);
    }
    if let Some((scope, key, value)) = config {
        let record = run(config::set_command(&root, scope, &key, value.as_deref())).await;
        let code = match record.outcome {
            Outcome::Exited(c) => Some(c),
            _ => None,
        };
        let ok = record.success() || (value.is_none() && config::nothing_to_unset(code));
        let summary = record.error_summary();
        records.push(record);
        if !ok {
            return (Err(Refusal::Failed(summary)), records);
        }
    }
    for command in commands(&root, moves, &current, &target) {
        let record = run(command).await;
        let ok = record.success();
        let summary = record.error_summary();
        records.push(record);
        if !ok {
            return (Err(Refusal::Failed(summary)), records);
        }
    }
    if let Some((path, content)) = file {
        let result = match &content {
            Some(c) => std::fs::write(&path, c),
            None => std::fs::remove_file(&path).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            }),
        };
        let shown = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let shell = match &content {
            Some(_) => format!("# restore {shown}"),
            None => format!("rm {shown}"),
        };
        let result = result.map_err(|e| e.to_string());
        let failed = result.clone().err();
        records.push(CommandRecord::local(shell, &root, result));
        if let Some(e) = failed {
            return (Err(Refusal::Failed(e)), records);
        }
    }
    (Ok(()), records)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn git(dir: &Path, args: &[&str]) -> String {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record.stdout
    }

    async fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q", "-b", "main"]).await;
        git(root, &["config", "user.name", "Test"]).await;
        git(root, &["config", "user.email", "t@example.com"]).await;
        tmp
    }

    async fn state(root: &Path) -> State {
        snapshot(root).await.0
    }

    async fn undo(root: &Path, entry: &Entry) -> Result<(), Refusal> {
        apply(root.to_path_buf(), entry.step(true)).await.0
    }

    async fn redo(root: &Path, entry: &Entry) -> Result<(), Refusal> {
        apply(root.to_path_buf(), entry.step(false)).await.0
    }

    async fn entry_for(root: &Path, moves: Moves, action: &[&[&str]]) -> Entry {
        let before = state(root).await;
        for args in action {
            git(root, args).await;
        }
        Entry {
            label: String::new(),
            moves,
            before,
            after: state(root).await,
            file: None,
            config: None,
        }
    }

    #[tokio::test]
    async fn undo_and_redo_staging() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        std::fs::write(root.join("a.vhd"), "b\n").unwrap();

        let entry = entry_for(root, Moves::Index, &[&["add", "a.vhd"]]).await;
        assert!(entry.changed_something());
        assert!(!git(root, &["diff", "--cached"]).await.is_empty());
        undo(root, &entry).await.unwrap();
        assert!(git(root, &["diff", "--cached"]).await.is_empty());
        // The file on disk is untouched.
        assert_eq!(std::fs::read_to_string(root.join("a.vhd")).unwrap(), "b\n");
        redo(root, &entry).await.unwrap();
        assert!(!git(root, &["diff", "--cached"]).await.is_empty());
    }

    #[tokio::test]
    async fn undo_and_redo_commits_including_the_first() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        let first = entry_for(root, Moves::Commit, &[&["commit", "-q", "-m", "first"]]).await;
        assert_eq!(first.before.head, None);
        std::fs::write(root.join("a.vhd"), "b\n").unwrap();
        let stage = entry_for(root, Moves::Index, &[&["add", "."]]).await;
        let second = entry_for(root, Moves::Commit, &[&["commit", "-q", "-m", "second"]]).await;

        undo(root, &second).await.unwrap();
        assert_eq!(git(root, &["log", "--format=%s"]).await, "first\n");
        // The change is staged again, ready to be saved differently.
        assert!(!git(root, &["diff", "--cached"]).await.is_empty());
        undo(root, &stage).await.unwrap();
        undo(root, &first).await.unwrap();
        assert_eq!(state(root).await.head, None);
        redo(root, &first).await.unwrap();
        redo(root, &stage).await.unwrap();
        redo(root, &second).await.unwrap();
        assert_eq!(git(root, &["log", "--format=%s"]).await, "second\nfirst\n");
    }

    #[tokio::test]
    async fn undo_integration_keeps_local_changes() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        std::fs::write(root.join("b.vhd"), "b\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        git(root, &["checkout", "-q", "-b", "other"]).await;
        std::fs::write(root.join("c.vhd"), "c\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "theirs"]).await;
        git(root, &["checkout", "-q", "main"]).await;
        std::fs::write(root.join("b.vhd"), "b local\n").unwrap();

        let entry = entry_for(
            root,
            Moves::Integration,
            &[&["merge", "-q", "--autostash", "--no-edit", "other"]],
        )
        .await;
        assert!(root.join("c.vhd").exists());
        undo(root, &entry).await.unwrap();
        assert!(!root.join("c.vhd").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("b.vhd")).unwrap(),
            "b local\n"
        );
        redo(root, &entry).await.unwrap();
        assert!(root.join("c.vhd").exists());
    }

    #[tokio::test]
    async fn refuses_when_something_changed_meanwhile() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        let entry = entry_for(root, Moves::Commit, &[&["commit", "-q", "-m", "first"]]).await;
        // Someone commits from a terminal.
        std::fs::write(root.join("a.vhd"), "b\n").unwrap();
        git(root, &["commit", "-q", "-am", "outside"]).await;
        assert_eq!(undo(root, &entry).await, Err(Refusal::ChangedMeanwhile));
        assert_eq!(git(root, &["log", "--format=%s", "-1"]).await, "outside\n");
    }

    #[tokio::test]
    async fn undo_and_redo_a_setting() {
        let tmp = repo().await;
        let root = tmp.path();
        let before = state(root).await;
        git(root, &["config", "--local", "pull.rebase", "false"]).await;
        let entry = Entry {
            label: String::new(),
            moves: Moves::Index,
            before: before.clone(),
            after: before,
            file: None,
            config: Some(ConfigEdit {
                scope: Scope::Local,
                key: "pull.rebase".into(),
                before: None,
                after: Some("false".into()),
            }),
        };
        undo(root, &entry).await.unwrap();
        let get = run(GitCommand::new(["config", "--local", "pull.rebase"]).in_dir(root)).await;
        assert!(!get.success());
        redo(root, &entry).await.unwrap();
        assert_eq!(
            git(root, &["config", "--local", "pull.rebase"]).await,
            "false\n"
        );
    }

    #[tokio::test]
    async fn restores_rewritten_files() {
        let tmp = repo().await;
        let root = tmp.path();
        let ignore = root.join(".gitignore");
        let before = state(root).await;
        std::fs::write(&ignore, "*.jou\n").unwrap();
        let entry = Entry {
            label: String::new(),
            moves: Moves::Index,
            before: before.clone(),
            after: before,
            file: Some(FileChange {
                path: ignore.clone(),
                before: None,
                after: Some("*.jou\n".into()),
            }),
            config: None,
        };
        assert!(entry.changed_something());
        undo(root, &entry).await.unwrap();
        assert!(!ignore.exists());
        redo(root, &entry).await.unwrap();
        assert_eq!(std::fs::read_to_string(&ignore).unwrap(), "*.jou\n");
    }
}
