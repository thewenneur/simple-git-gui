//! Branch and commit actions of the history view: switching branch with the
//! local changes set aside per branch, creating and deleting branches and
//! tags, copying or undoing a commit, and what a commit changed.

use std::path::Path;

use super::runner::{CommandRecord, GitCommand, run};

/// Prefix of the stash entries the app makes when leaving a branch with
/// changes: `auto/<branch>`.
pub const AUTOSTASH_PREFIX: &str = "auto/";

/// A stash entry: `stash@{0}` and its message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    pub reference: String,
    pub message: String,
}

impl Stash {
    /// The branch whose changes were set aside here by the app.
    pub fn auto_branch(&self) -> Option<&str> {
        // `git stash push -m auto/x` records "On <branch>: auto/x".
        let message = self
            .message
            .split_once(": ")
            .map_or(self.message.as_str(), |(_, m)| m);
        message.strip_prefix(AUTOSTASH_PREFIX)
    }
}

pub fn stash_list_command(root: &Path) -> GitCommand {
    GitCommand::new(["stash", "list", "--format=%gd%x1f%s"]).in_dir(root)
}

pub fn parse_stash_list(output: &str) -> Vec<Stash> {
    output
        .lines()
        .filter_map(|line| {
            let (reference, message) = line.split_once('\u{1f}')?;
            Some(Stash {
                reference: reference.to_string(),
                message: message.to_string(),
            })
        })
        .collect()
}

/// Where to go: a local branch, or a commit (detached HEAD).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Branch(String),
    Commit(String),
}

/// Switches to `target`. When there are local changes and the current branch
/// is known, they are first set aside as `auto/<current>`; the changes set
/// aside earlier for the target branch come back afterwards.
pub async fn switch(
    root: &Path,
    current: Option<&str>,
    target: &Target,
    dirty: bool,
) -> Vec<CommandRecord> {
    let mut records = Vec::new();
    let stashed = match (current, dirty) {
        (Some(current), true) => {
            let message = format!("{AUTOSTASH_PREFIX}{current}");
            let record = run(GitCommand::new([
                "stash",
                "push",
                "--include-untracked",
                "-m",
                message.as_str(),
            ])
            .in_dir(root))
            .await;
            let ok = record.success();
            records.push(record);
            if !ok {
                return records;
            }
            true
        }
        _ => false,
    };

    let switch = match target {
        Target::Branch(name) => GitCommand::new(["switch", name.as_str()]),
        Target::Commit(hash) => GitCommand::new(["switch", "--detach", hash.as_str()]),
    };
    let record = run(switch.in_dir(root)).await;
    let switched = record.success();
    records.push(record);
    if !switched {
        if stashed {
            // Put the changes back where they were; the switch error stays last.
            let failed = records.pop();
            records.push(run(GitCommand::new(["stash", "pop", "--index"]).in_dir(root)).await);
            records.extend(failed);
        }
        return records;
    }

    if let Target::Branch(name) = target {
        let list = run(stash_list_command(root).background()).await;
        let saved = parse_stash_list(&list.stdout)
            .into_iter()
            .find(|s| s.auto_branch() == Some(name.as_str()));
        records.push(list);
        if let Some(saved) = saved {
            records.push(
                run(
                    GitCommand::new(["stash", "pop", "--index", saved.reference.as_str()])
                        .in_dir(root),
                )
                .await,
            );
        }
    }
    records
}

/// Creates `name` at `at`; with `switch`, also goes onto it (local changes
/// follow).
pub fn create_branch(root: &Path, name: &str, at: &str, switch: bool) -> GitCommand {
    let args = if switch {
        vec!["switch", "-c", name, at]
    } else {
        vec!["branch", name, at]
    };
    GitCommand::new(args).in_dir(root)
}

/// Deletes a local branch; without `force`, git refuses if some of its
/// commits are on no other branch.
pub fn delete_branch(root: &Path, name: &str, force: bool) -> GitCommand {
    GitCommand::new(["branch", if force { "-D" } else { "-d" }, name]).in_dir(root)
}

/// Whether `git branch -d` refused because the branch is not merged.
pub fn is_not_merged(stderr: &str) -> bool {
    stderr.contains("not fully merged")
}

pub fn create_tag(root: &Path, name: &str, at: &str) -> GitCommand {
    GitCommand::new(["tag", name, at]).in_dir(root)
}

/// Copies a commit onto the current branch.
pub fn cherry_pick(root: &Path, hash: &str, merge_commit: bool) -> GitCommand {
    let mut args = vec!["cherry-pick"];
    if merge_commit {
        args.extend(["-m", "1"]);
    }
    args.push(hash);
    GitCommand::new(args).in_dir(root)
}

/// Adds a commit that undoes `hash`.
pub fn revert(root: &Path, hash: &str, merge_commit: bool) -> GitCommand {
    let mut args = vec!["revert", "--no-edit"];
    if merge_commit {
        args.extend(["-m", "1"]);
    }
    args.push(hash);
    GitCommand::new(args).in_dir(root)
}

/// Changes the message of the last commit, leaving staged changes out.
pub fn reword_head(root: &Path, message: &str) -> GitCommand {
    GitCommand::new(["commit", "--amend", "--only", "-m", message]).in_dir(root)
}

/// Puts a file back as it was in commit `hash` (on disk and staged).
pub fn restore_file(root: &Path, hash: &str, path: &str) -> GitCommand {
    let source = format!("--source={hash}");
    GitCommand::new([
        "restore",
        source.as_str(),
        "--staged",
        "--worktree",
        "--",
        path,
    ])
    .in_dir(root)
}

/// The branch checked out before the current position (`@{-1}`).
pub fn previous_branch(root: &Path) -> GitCommand {
    GitCommand::new(["rev-parse", "--abbrev-ref", "@{-1}"])
        .in_dir(root)
        .background()
}

/// What to compare in the commit details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Comparison {
    /// What a commit changed (against its first parent).
    Commit {
        hash: String,
        parent: Option<String>,
    },
    /// From one commit to another.
    Between { from: String, to: String },
    /// From a commit to the files on disk.
    WithWorkingTree { hash: String },
}

impl Comparison {
    fn args(&self) -> Vec<String> {
        match self {
            Comparison::Commit {
                hash,
                parent: Some(parent),
            } => vec![parent.clone(), hash.clone()],
            // The first commit: compare with nothing.
            Comparison::Commit { hash, parent: None } => {
                vec!["--root".to_string(), hash.clone()]
            }
            Comparison::Between { from, to } => vec![from.clone(), to.clone()],
            Comparison::WithWorkingTree { hash } => vec![hash.clone()],
        }
    }

    fn is_root(&self) -> bool {
        matches!(self, Comparison::Commit { parent: None, .. })
    }
}

/// The files changed, one per line as `status\tpath` (renames: `R\told\tnew`).
pub fn changed_files_command(root: &Path, comparison: &Comparison) -> GitCommand {
    let mut args: Vec<String> = if comparison.is_root() {
        vec!["diff-tree".into(), "-r".into()]
    } else {
        vec!["diff".into()]
    };
    args.extend(["--name-status".into(), "-M".into()]);
    args.extend(comparison.args());
    GitCommand::new(args).in_dir(root)
}

/// The diff of one file of the comparison.
pub fn file_diff_command(
    root: &Path,
    comparison: &Comparison,
    path: &str,
    ignore_whitespace: bool,
) -> GitCommand {
    let mut args: Vec<String> = if comparison.is_root() {
        vec!["diff-tree".into(), "-p".into()]
    } else {
        vec!["diff".into(), "--no-ext-diff".into()]
    };
    args.extend(["--histogram".into(), "--no-color".into()]);
    if ignore_whitespace {
        args.push("-w".into());
    }
    args.extend(comparison.args());
    args.extend(["--".into(), path.to_string()]);
    GitCommand::new(args).in_dir(root)
}

/// A file of a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// `A`, `M`, `D`, `R`, `C` or `T`.
    pub status: char,
    pub path: String,
    pub old_path: Option<String>,
}

pub fn parse_changed_files(output: &str) -> Vec<ChangedFile> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let status = fields.next()?.chars().next()?;
            let first = fields.next()?.to_string();
            Some(match fields.next() {
                Some(second) => ChangedFile {
                    status,
                    path: second.to_string(),
                    old_path: Some(first),
                },
                None => ChangedFile {
                    status,
                    path: first,
                    old_path: None,
                },
            })
        })
        // `diff-tree --root` starts with the commit's own hash.
        .filter(|f| !f.path.is_empty())
        .collect()
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
        git(root, &["config", "core.autocrlf", "false"]).await;
        std::fs::write(root.join("a.vhd"), "a\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        tmp
    }

    #[test]
    fn reads_auto_stashes() {
        let list = parse_stash_list(
            "stash@{0}\u{1f}On main: auto/main\nstash@{1}\u{1f}WIP on x: 123 msg\n",
        );
        assert_eq!(list[0].auto_branch(), Some("main"));
        assert_eq!(list[1].auto_branch(), None);
    }

    #[test]
    fn parses_name_status() {
        let files = parse_changed_files("M\trtl/a.vhd\nR087\told.vhd\tnew.vhd\n");
        assert_eq!(files[0].status, 'M');
        assert_eq!(files[1].path, "new.vhd");
        assert_eq!(files[1].old_path.as_deref(), Some("old.vhd"));
    }

    #[tokio::test]
    async fn switching_sets_changes_aside_per_branch() {
        let tmp = repo().await;
        let root = tmp.path();
        git(root, &["branch", "feature/x"]).await;
        std::fs::write(root.join("a.vhd"), "on main\n").unwrap();

        let target = Target::Branch("feature/x".into());
        let records = switch(root, Some("main"), &target, true).await;
        assert!(records.iter().all(CommandRecord::success));
        assert_eq!(std::fs::read_to_string(root.join("a.vhd")).unwrap(), "a\n");
        std::fs::write(root.join("b.vhd"), "on feature\n").unwrap();

        let back = Target::Branch("main".into());
        let records = switch(root, Some("feature/x"), &back, true).await;
        assert!(records.iter().all(CommandRecord::success));
        assert_eq!(
            std::fs::read_to_string(root.join("a.vhd")).unwrap(),
            "on main\n"
        );
        assert!(!root.join("b.vhd").exists());

        let records = switch(root, Some("main"), &target, true).await;
        assert!(records.iter().all(CommandRecord::success));
        assert_eq!(
            std::fs::read_to_string(root.join("b.vhd")).unwrap(),
            "on feature\n"
        );
        let stashes = parse_stash_list(&git(root, &["stash", "list", "--format=%gd%x1f%s"]).await);
        assert_eq!(stashes.len(), 1);
        assert_eq!(stashes[0].auto_branch(), Some("main"));
    }

    #[tokio::test]
    async fn lists_what_a_commit_changed() {
        let tmp = repo().await;
        let root = tmp.path();
        let first = git(root, &["rev-parse", "HEAD"]).await.trim().to_string();
        std::fs::write(root.join("b.vhd"), "b\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "second"]).await;
        let second = git(root, &["rev-parse", "HEAD"]).await.trim().to_string();

        let root_commit = Comparison::Commit {
            hash: first.clone(),
            parent: None,
        };
        let out = run(changed_files_command(root, &root_commit)).await;
        let files = parse_changed_files(&out.stdout);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "a.vhd");
        let diff = run(file_diff_command(root, &root_commit, "a.vhd", false)).await;
        assert!(diff.stdout.contains("+a"), "{}", diff.stdout);

        let commit = Comparison::Commit {
            hash: second,
            parent: Some(first),
        };
        let out = run(changed_files_command(root, &commit)).await;
        assert_eq!(parse_changed_files(&out.stdout)[0].path, "b.vhd");
    }
}
