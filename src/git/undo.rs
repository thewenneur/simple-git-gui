//! Undo and redo of the app's actions.
//!
//! Before and after each action the app records where HEAD points and what
//! the index contains (as a tree written with `git write-tree`), plus the
//! branches and tags the action touches. Undoing puts them back; redoing
//! moves them forward again. Files on disk are never overwritten, except
//! files the app wrote itself (ignore files, a restored file).
//!
//! An undo only runs if the repository is still exactly as the action left it,
//! so a change made meanwhile (in a terminal, another tool...) is never lost.

use std::path::{Path, PathBuf};

use super::branches::{self, Target};
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
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// A branch or tag the action created, moved or deleted (`None`: absent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefChange {
    /// Full name: `refs/heads/x`, `refs/tags/v1`.
    pub name: String,
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
    /// Another branch (or commit) was checked out: going back switches again.
    /// With `set_aside`, local changes are set aside per branch, as the
    /// action did; otherwise they come along.
    Switch { set_aside: bool },
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
    pub refs: Vec<RefChange>,
    /// A conflicted file the action marked as settled (`git add`): undoing
    /// puts the conflict back.
    pub resolved: Option<String>,
}

impl Entry {
    /// Whether the action changed anything worth undoing.
    pub fn changed_something(&self) -> bool {
        self.before != self.after
            || self.file.as_ref().is_some_and(|f| f.before != f.after)
            || self.config.as_ref().is_some_and(|c| c.before != c.after)
            || self.refs.iter().any(|r| r.before != r.after)
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
            refs: self
                .refs
                .iter()
                .map(|r| {
                    let (expected, target) = if undo {
                        (&r.after, &r.before)
                    } else {
                        (&r.before, &r.after)
                    };
                    (r.name.clone(), expected.clone(), target.clone())
                })
                .collect(),
            resolved: self.resolved.clone(),
            undo,
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
    pub file: Option<(PathBuf, Option<Vec<u8>>)>,
    /// A setting to put back to this value (`None`: unset).
    pub config: Option<(Scope, String, Option<String>)>,
    /// Branches and tags: name, expected value, value to set.
    pub refs: Vec<(String, Option<String>, Option<String>)>,
    /// A file settled in a conflict: put back in conflict (undo) or marked
    /// settled again (redo).
    pub resolved: Option<String>,
    /// Going back (refs are restored after switching) or forward (before).
    pub undo: bool,
}

/// The commits `names` point to (`None`: the ref does not exist).
pub async fn snapshot_refs(
    root: &Path,
    names: &[String],
) -> (Vec<Option<String>>, Vec<CommandRecord>) {
    let mut values = Vec::new();
    let mut records = Vec::new();
    for name in names {
        let record = run(
            GitCommand::new(["rev-parse", "-q", "--verify", name.as_str()])
                .in_dir(root)
                .background(),
        )
        .await;
        values.push(record.success().then(|| record.stdout.trim().to_string()));
        records.push(record);
    }
    (values, records)
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
        refs,
        resolved,
        undo,
    } = step;
    let (current, mut records) = snapshot(&root).await;
    let names: Vec<String> = refs.iter().map(|(n, _, _)| n.clone()).collect();
    let (values, ref_records) = snapshot_refs(&root, &names).await;
    records.extend(ref_records);
    let refs_as_expected = refs
        .iter()
        .zip(&values)
        .all(|((_, expected, _), value)| expected == value);
    if current != expected || !refs_as_expected {
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
    // A branch is created before switching onto it, and deleted after
    // switching away from it.
    let ref_commands: Vec<GitCommand> = refs
        .iter()
        .map(|(name, _, value)| {
            match value {
                Some(value) => GitCommand::new(["update-ref", name.as_str(), value.as_str()]),
                None => GitCommand::new(["update-ref", "-d", name.as_str()]),
            }
            .in_dir(&root)
        })
        .collect();
    let mut moves_commands = Vec::new();
    if !matches!(moves, Moves::Switch { .. }) {
        moves_commands = commands(&root, moves, &current, &target);
    }
    let (first, second) = if undo {
        (moves_commands, ref_commands)
    } else {
        (ref_commands, moves_commands)
    };
    let mut steps: Vec<Option<GitCommand>> = first.into_iter().map(Some).collect();
    if matches!(moves, Moves::Switch { .. })
        && (current.branch != target.branch || current.head != target.head)
    {
        // The switch itself, between the two groups of commands.
        steps.push(None);
    }
    steps.extend(second.into_iter().map(Some));
    for step in steps {
        match step {
            Some(command) => {
                let record = run(command).await;
                let ok = record.success();
                let summary = record.error_summary();
                records.push(record);
                if !ok {
                    return (Err(Refusal::Failed(summary)), records);
                }
            }
            None => {
                let status = run(GitCommand::new(["status", "--porcelain"])
                    .in_dir(&root)
                    .background())
                .await;
                let set_aside = matches!(moves, Moves::Switch { set_aside: true });
                let dirty = set_aside && !status.stdout.trim().is_empty();
                records.push(status);
                let to = match (&target.branch, &target.head) {
                    (Some(branch), _) => Target::Branch(
                        branch
                            .strip_prefix("refs/heads/")
                            .unwrap_or(branch)
                            .to_string(),
                    ),
                    (None, Some(head)) => Target::Commit(head.clone()),
                    (None, None) => continue,
                };
                let from = current
                    .branch
                    .as_deref()
                    .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b));
                let switched = branches::switch(&root, from, &to, dirty).await;
                let failed = switched
                    .iter()
                    .find(|r| !r.success())
                    .map(CommandRecord::error_summary);
                records.extend(switched);
                if let Some(summary) = failed {
                    return (Err(Refusal::Failed(summary)), records);
                }
            }
        }
    }
    if let Some(path) = resolved.as_deref().filter(|_| undo) {
        let record = run(super::integrate::unresolve(&root, path)).await;
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
    if let Some(path) = resolved.as_deref().filter(|_| !undo) {
        let record = run(super::integrate::mark_resolved(&root, path)).await;
        let ok = record.success();
        let summary = record.error_summary();
        records.push(record);
        if !ok {
            return (Err(Refusal::Failed(summary)), records);
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
        // Windows runners set core.autocrlf, which rewrites line endings on checkout.
        git(root, &["config", "core.autocrlf", "false"]).await;
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
            refs: Vec::new(),
            resolved: None,
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
            refs: Vec::new(),
            resolved: None,
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
                after: Some(b"*.jou\n".to_vec()),
            }),
            config: None,
            refs: Vec::new(),
            resolved: None,
        };
        assert!(entry.changed_something());
        undo(root, &entry).await.unwrap();
        assert!(!ignore.exists());
        redo(root, &entry).await.unwrap();
        assert_eq!(std::fs::read_to_string(&ignore).unwrap(), "*.jou\n");
    }

    async fn ref_entry(root: &Path, moves: Moves, names: &[&str], action: &[&[&str]]) -> Entry {
        let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        let (refs_before, _) = snapshot_refs(root, &names).await;
        let mut entry = entry_for(root, moves, action).await;
        let (refs_after, _) = snapshot_refs(root, &names).await;
        entry.refs = names
            .into_iter()
            .zip(refs_before.into_iter().zip(refs_after))
            .map(|(name, (before, after))| RefChange {
                name,
                before,
                after,
            })
            .collect();
        entry
    }

    #[tokio::test]
    async fn undo_and_redo_branch_creation_with_switch() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        std::fs::write(root.join("a.vhd"), "local\n").unwrap();

        let entry = ref_entry(
            root,
            Moves::Switch { set_aside: false },
            &["refs/heads/feature/x"],
            &[&["switch", "-c", "feature/x"]],
        )
        .await;
        assert!(entry.changed_something());
        undo(root, &entry).await.unwrap();
        assert_eq!(git(root, &["branch", "--show-current"]).await, "main\n");
        assert!(
            git(root, &["branch", "--list", "feature/x"])
                .await
                .is_empty()
        );
        // Local changes came along.
        assert_eq!(
            std::fs::read_to_string(root.join("a.vhd")).unwrap(),
            "local\n"
        );
        redo(root, &entry).await.unwrap();
        assert_eq!(
            git(root, &["branch", "--show-current"]).await,
            "feature/x\n"
        );
    }

    #[tokio::test]
    async fn undo_brings_a_deleted_branch_back() {
        let tmp = repo().await;
        let root = tmp.path();
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        git(root, &["switch", "-q", "-c", "fix/x"]).await;
        std::fs::write(root.join("a.vhd"), "fix\n").unwrap();
        git(root, &["commit", "-q", "-am", "fix"]).await;
        let tip = git(root, &["rev-parse", "HEAD"]).await;
        git(root, &["switch", "-q", "main"]).await;

        let entry = ref_entry(
            root,
            Moves::Index,
            &["refs/heads/fix/x"],
            &[&["branch", "-D", "fix/x"]],
        )
        .await;
        undo(root, &entry).await.unwrap();
        assert_eq!(git(root, &["rev-parse", "fix/x"]).await, tip);
        redo(root, &entry).await.unwrap();
        assert!(git(root, &["branch", "--list", "fix/x"]).await.is_empty());
    }

    #[tokio::test]
    async fn undo_puts_a_settled_conflict_back() {
        let tmp = repo().await;
        let root = tmp.path();
        let file = root.join("a.vhd");
        std::fs::write(&file, "base\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        git(root, &["switch", "-q", "-c", "fix/x"]).await;
        std::fs::write(&file, "fix\n").unwrap();
        git(root, &["commit", "-q", "-am", "fix"]).await;
        git(root, &["switch", "-q", "main"]).await;
        std::fs::write(&file, "main\n").unwrap();
        git(root, &["commit", "-q", "-am", "main"]).await;
        let merge = run(GitCommand::new(["merge", "--no-commit", "fix/x"]).in_dir(root)).await;
        assert!(!merge.success());
        let conflicted = std::fs::read(&file).unwrap();

        let before = state(root).await;
        std::fs::write(&file, "main\nfix\n").unwrap();
        git(root, &["add", "a.vhd"]).await;
        let entry = Entry {
            label: String::new(),
            moves: Moves::Index,
            before,
            after: state(root).await,
            file: Some(FileChange {
                path: file.clone(),
                before: Some(conflicted.clone()),
                after: Some(b"main\nfix\n".to_vec()),
            }),
            config: None,
            refs: Vec::new(),
            resolved: Some("a.vhd".into()),
        };
        let unmerged = || async { git(root, &["diff", "--name-only", "--diff-filter=U"]).await };

        undo(root, &entry).await.unwrap();
        assert_eq!(unmerged().await, "a.vhd\n");
        assert_eq!(std::fs::read(&file).unwrap(), conflicted);
        redo(root, &entry).await.unwrap();
        assert!(unmerged().await.is_empty());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "main\nfix\n");
    }
}
