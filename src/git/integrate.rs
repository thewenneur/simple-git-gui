//! Merging and rebasing branches, and what the conflict assistant needs to
//! know about an operation in progress: which branch is on which side, the
//! versions of a file, and how to carry on.
//!
//! The words "ours" and "theirs" are never shown: each side is named after
//! its branch, the right way round for a merge as well as for a rebase.

use std::path::Path;

use super::repo::Operation;
use super::runner::{CommandRecord, GitCommand, run};

/// Settings given to the integration commands so that conflicts are as few
/// and as readable as possible, whatever the user's configuration.
const CONFLICT_CONFIG: &[&str] = &[
    "-c",
    "merge.conflictStyle=zdiff3",
    "-c",
    "rerere.enabled=true",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Brings another branch into the current one with a merge commit.
    Merge,
    /// Replays the current branch's commits after another branch.
    Rebase,
}

/// The command that starts the integration of `other` into the current
/// branch. A merge stops before committing so the result can be checked.
/// A rebase given `after` only moves the commits after that one (`--onto`);
/// given `branch`, it rebases that branch instead of the current one (git
/// switches to it first).
pub fn start(
    root: &Path,
    kind: Kind,
    other: &str,
    after: Option<&str>,
    branch: Option<&str>,
    ignore_whitespace: bool,
) -> GitCommand {
    let mut args: Vec<&str> = CONFLICT_CONFIG.to_vec();
    match kind {
        Kind::Merge => args.extend(["merge", "--no-commit", "--no-ff"]),
        Kind::Rebase => args.push("rebase"),
    }
    args.extend(["--autostash", "-X", "diff-algorithm=histogram"]);
    if ignore_whitespace {
        args.extend(["-X", "ignore-space-change"]);
    }
    match (kind, after) {
        (Kind::Rebase, Some(after)) => args.extend(["--onto", other, after]),
        _ => args.push(other),
    }
    if let (Kind::Rebase, Some(branch)) = (kind, branch) {
        args.push(branch);
    }
    GitCommand::new(args).in_dir(root)
}

/// The command that goes on once the conflicts of the current step are
/// settled (or saves a merge that had none).
pub fn proceed(root: &Path, operation: Operation) -> GitCommand {
    let mut args: Vec<&str> = CONFLICT_CONFIG.to_vec();
    match operation {
        Operation::Merge => args.extend(["commit", "--no-edit"]),
        Operation::Rebase => args.extend(["rebase", "--continue"]),
        Operation::CherryPick => args.extend(["cherry-pick", "--continue"]),
        Operation::Revert => args.extend(["revert", "--continue"]),
    }
    GitCommand::new(args).in_dir(root)
}

/// Marks a file as settled.
pub fn mark_resolved(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["add", "--", path]).in_dir(root)
}

/// Puts a settled file back in conflict (undo of [`mark_resolved`]).
pub fn unresolve(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["update-index", "--unresolve", "--", path]).in_dir(root)
}

/// The two sides of the operation, named for the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sides {
    /// Where you are: the branch receiving the changes (git's "ours").
    pub current: String,
    /// What is brought in (git's "theirs").
    pub incoming: String,
    /// For a rebase: the commit being replayed, `short subject`.
    pub replaying: Option<String>,
}

fn read(git_dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(git_dir.join(name))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The branch merged, from the first line of `MERGE_MSG`.
pub fn merged_name(merge_msg: &str) -> Option<String> {
    let first = merge_msg.lines().next()?;
    let start = first.find('\'')? + 1;
    let end = start + first[start..].find('\'')?;
    Some(first[start..end].to_string())
}

/// Names the sides of the operation in progress. `current_branch` is the
/// branch checked out (absent during a rebase, where HEAD is detached).
pub async fn sides(
    root: &Path,
    git_dir: &Path,
    operation: Operation,
    current_branch: Option<&str>,
) -> (Sides, Vec<CommandRecord>) {
    let mut records = Vec::new();
    let describe = |rev: &str| {
        GitCommand::new(["log", "-1", "--format=%h %s", rev])
            .in_dir(root)
            .background()
    };
    let here = current_branch.unwrap_or("HEAD").to_string();
    let sides = match operation {
        Operation::Merge => Sides {
            current: here,
            incoming: read(git_dir, "MERGE_MSG")
                .as_deref()
                .and_then(merged_name)
                .unwrap_or_else(|| "MERGE_HEAD".to_string()),
            replaying: None,
        },
        Operation::Rebase => {
            let dir = if git_dir.join("rebase-merge").exists() {
                "rebase-merge"
            } else {
                "rebase-apply"
            };
            let branch = read(git_dir, &format!("{dir}/head-name"))
                .map(|h| h.strip_prefix("refs/heads/").unwrap_or(&h).to_string())
                .unwrap_or_else(|| "HEAD".to_string());
            let onto = read(git_dir, &format!("{dir}/onto")).unwrap_or_default();
            // Name the base after a branch pointing at it, if any.
            let names = run(GitCommand::new([
                "for-each-ref",
                "--format=%(refname:short)",
                "--points-at",
                onto.as_str(),
                "refs/heads",
                "refs/remotes",
            ])
            .in_dir(root)
            .background())
            .await;
            let onto_name = names
                .stdout
                .lines()
                .find(|n| !n.ends_with("/HEAD"))
                .map(str::to_string)
                .unwrap_or_else(|| onto.chars().take(7).collect());
            records.push(names);
            let replaying = if git_dir.join("REBASE_HEAD").exists() {
                let record = run(describe("REBASE_HEAD")).await;
                let line = record.stdout.trim().to_string();
                records.push(record);
                Some(line).filter(|l| !l.is_empty())
            } else {
                None
            };
            // In a rebase, git's "ours" is the base being built on and
            // "theirs" is your own commit being replayed.
            Sides {
                current: onto_name,
                incoming: branch,
                replaying,
            }
        }
        Operation::CherryPick | Operation::Revert => {
            let head = if operation == Operation::CherryPick {
                "CHERRY_PICK_HEAD"
            } else {
                "REVERT_HEAD"
            };
            let record = run(describe(head)).await;
            let line = record.stdout.trim().to_string();
            records.push(record);
            Sides {
                current: here,
                incoming: line.split(' ').next().unwrap_or(head).to_string(),
                replaying: Some(line).filter(|l| !l.is_empty()),
            }
        }
    };
    (sides, records)
}

/// The base, current and incoming versions of a conflicted file, from the
/// index stages 1, 2 and 3 (empty when a side has no such file).
pub async fn stage_versions(root: &Path, path: &str) -> ([String; 3], Vec<CommandRecord>) {
    let mut versions: [String; 3] = Default::default();
    let mut records = Vec::new();
    for (i, stage) in ["1", "2", "3"].iter().enumerate() {
        let spec = format!(":{stage}:{path}");
        let record = run(GitCommand::new(["show", spec.as_str()])
            .in_dir(root)
            .background())
        .await;
        if record.success() {
            versions[i] = record.stdout.clone();
        }
        records.push(record);
    }
    (versions, records)
}

/// The versions of `path` at three commits (base, current, incoming).
pub async fn commit_versions(
    root: &Path,
    revs: [&str; 3],
    path: &str,
) -> ([String; 3], Vec<CommandRecord>) {
    let mut versions: [String; 3] = Default::default();
    let mut records = Vec::new();
    for (i, rev) in revs.iter().enumerate() {
        let spec = format!("{rev}:{path}");
        let record = run(GitCommand::new(["show", spec.as_str()])
            .in_dir(root)
            .background())
        .await;
        if record.success() {
            versions[i] = record.stdout.clone();
        }
        records.push(record);
    }
    (versions, records)
}

/// Files changed on both sides since `base`: the ones where a merge can
/// silently keep a block twice.
pub async fn touched_by_both(
    root: &Path,
    base: &str,
    current: &str,
    incoming: &str,
) -> (Vec<String>, Vec<CommandRecord>) {
    let names = |to: &str| {
        GitCommand::new(["diff", "--name-only", base, to])
            .in_dir(root)
            .background()
    };
    let (a, b) = tokio::join!(run(names(current)), run(names(incoming)));
    let theirs: Vec<&str> = b.stdout.lines().collect();
    let both = a
        .stdout
        .lines()
        .filter(|l| theirs.contains(l))
        .map(str::to_string)
        .collect();
    (both, vec![a, b])
}

pub fn merge_base(root: &Path, a: &str, b: &str) -> GitCommand {
    GitCommand::new(["merge-base", a, b])
        .in_dir(root)
        .background()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::repo::operation_in_progress;

    async fn git(dir: &Path, args: &[&str]) -> CommandRecord {
        run(GitCommand::new(args.iter().copied()).in_dir(dir)).await
    }

    async fn ok(dir: &Path, args: &[&str]) {
        let r = git(dir, args).await;
        assert!(r.success(), "git {args:?}: {}", r.stderr);
    }

    async fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        ok(root, &["init", "-q", "-b", "main"]).await;
        ok(root, &["config", "user.name", "Test"]).await;
        ok(root, &["config", "user.email", "t@example.com"]).await;
        ok(root, &["config", "core.autocrlf", "false"]).await;
        std::fs::write(root.join("a.vhd"), "one\ntwo\nthree\n").unwrap();
        ok(root, &["add", "."]).await;
        ok(root, &["commit", "-q", "-m", "init"]).await;
        ok(root, &["switch", "-q", "-c", "feature/x"]).await;
        std::fs::write(root.join("a.vhd"), "one\nTWO feature\nthree\n").unwrap();
        ok(root, &["commit", "-q", "-am", "feature change"]).await;
        ok(root, &["switch", "-q", "main"]).await;
        std::fs::write(root.join("a.vhd"), "one\nTWO main\nthree\n").unwrap();
        ok(root, &["commit", "-q", "-am", "main change"]).await;
        tmp
    }

    #[test]
    fn reads_the_merged_branch() {
        assert_eq!(
            merged_name("Merge branch 'feature/x'\n\n# Conflicts:").as_deref(),
            Some("feature/x")
        );
        assert_eq!(
            merged_name("Merge remote-tracking branch 'origin/main'").as_deref(),
            Some("origin/main")
        );
    }

    #[tokio::test]
    async fn merge_names_sides_after_branches() {
        let tmp = repo().await;
        let root = tmp.path();
        let git_dir = root.join(".git");
        let r = run(super::start(
            root,
            Kind::Merge,
            "feature/x",
            None,
            None,
            true,
        ))
        .await;
        assert!(!r.success());
        assert_eq!(operation_in_progress(&git_dir), Some(Operation::Merge));
        let (sides, _) = sides(root, &git_dir, Operation::Merge, Some("main")).await;
        assert_eq!(sides.current, "main");
        assert_eq!(sides.incoming, "feature/x");
        let (versions, _) = stage_versions(root, "a.vhd").await;
        assert!(versions[1].contains("TWO main"));
        assert!(versions[2].contains("TWO feature"));
        // zdiff3 markers include the base.
        let content = std::fs::read_to_string(root.join("a.vhd")).unwrap();
        assert!(content.contains("|||||||"), "{content}");
    }

    #[tokio::test]
    async fn rebase_names_sides_the_right_way_round() {
        let tmp = repo().await;
        let root = tmp.path();
        let git_dir = root.join(".git");
        ok(root, &["switch", "-q", "feature/x"]).await;
        let r = run(super::start(root, Kind::Rebase, "main", None, None, true)).await;
        assert!(!r.success());
        assert_eq!(operation_in_progress(&git_dir), Some(Operation::Rebase));
        let (sides, _) = sides(root, &git_dir, Operation::Rebase, None).await;
        // The base being built on is main; the commit replayed is the feature's.
        assert_eq!(sides.current, "main");
        assert_eq!(sides.incoming, "feature/x");
        assert!(sides.replaying.unwrap().ends_with("feature change"));
        let (versions, _) = stage_versions(root, "a.vhd").await;
        assert!(versions[1].contains("TWO main"));
        assert!(versions[2].contains("TWO feature"));

        // Settle with the feature's version and carry on.
        std::fs::write(root.join("a.vhd"), "one\nTWO feature\nthree\n").unwrap();
        ok(root, &["add", "a.vhd"]).await;
        let go = run(proceed(root, Operation::Rebase)).await;
        assert!(go.success(), "{}", go.stderr);
        assert_eq!(operation_in_progress(&git_dir), None);
    }
}
