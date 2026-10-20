//! Higher-level queries on a repository, built on [`runner`](super::runner).

use std::path::{Path, PathBuf};

use super::runner::{CommandRecord, GitCommand, run};
use super::status::{self, Status};

/// An operation left in progress, which blocks most other actions until it
/// is finished or aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl Operation {
    /// Translation key of the operation's description.
    pub fn key(self) -> &'static str {
        match self {
            Operation::Merge => "op.merge",
            Operation::Rebase => "op.rebase",
            Operation::CherryPick => "op.cherry_pick",
            Operation::Revert => "op.revert",
        }
    }
}

/// Paths of an opened repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPaths {
    /// Top of the working tree.
    pub root: PathBuf,
    /// The `.git` directory (absolute).
    pub git_dir: PathBuf,
}

/// Finds the repository containing `dir`.
pub async fn discover(dir: &Path) -> (Result<RepoPaths, String>, CommandRecord) {
    let record =
        run(GitCommand::new(["rev-parse", "--show-toplevel", "--absolute-git-dir"]).in_dir(dir))
            .await;
    if !record.success() {
        let msg = if record.stderr.contains("not a git repository") {
            t!("repo.not_a_repo").into_owned()
        } else {
            record.error_summary()
        };
        return (Err(msg), record);
    }
    let mut lines = record.stdout.lines();
    let result = match (lines.next(), lines.next()) {
        (Some(root), Some(git_dir)) => Ok(RepoPaths {
            root: PathBuf::from(root),
            git_dir: PathBuf::from(git_dir),
        }),
        _ => Err(t!("repo.bad_rev_parse").into_owned()),
    };
    (result, record)
}

/// Detects a merge, rebase... left in progress.
pub fn operation_in_progress(git_dir: &Path) -> Option<Operation> {
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        Some(Operation::Rebase)
    } else if git_dir.join("MERGE_HEAD").exists() {
        Some(Operation::Merge)
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        Some(Operation::CherryPick)
    } else if git_dir.join("REVERT_HEAD").exists() {
        Some(Operation::Revert)
    } else {
        None
    }
}

pub fn status_command(root: &Path) -> GitCommand {
    GitCommand::new(status::STATUS_ARGS.iter().copied()).in_dir(root)
}

pub fn parse_status(record: &CommandRecord) -> Result<Status, String> {
    if !record.success() {
        return Err(record.error_summary());
    }
    status::parse(&record.stdout).map_err(|e| e.to_string())
}

/// One commit of the history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    /// Branch and tag names pointing at this commit.
    pub refs: Vec<String>,
    pub subject: String,
}

/// Field separator unlikely to appear in commit data (`%x1f` in the format).
const SEP: char = '\u{1f}';

pub fn log_command(root: &Path, limit: usize) -> GitCommand {
    GitCommand::new([
        "log".to_string(),
        "--all".to_string(),
        "--topo-order".to_string(),
        format!("--max-count={limit}"),
        "--date=format:%Y-%m-%d %H:%M".to_string(),
        "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ad%x1f%D%x1f%s".to_string(),
    ])
    .in_dir(root)
}

pub fn parse_log(output: &str) -> Vec<Commit> {
    output
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(7, SEP).collect();
            if f.len() != 7 {
                return None;
            }
            Some(Commit {
                hash: f[0].to_string(),
                short: f[1].to_string(),
                parents: f[2].split_whitespace().map(str::to_string).collect(),
                author: f[3].to_string(),
                date: f[4].to_string(),
                refs: f[5]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(|r| r.strip_prefix("HEAD -> ").unwrap_or(r).to_string())
                    .collect(),
                subject: f[6].to_string(),
            })
        })
        .collect()
}

pub fn clone_command(url: &str, dest: &Path) -> GitCommand {
    GitCommand::new([
        "clone".to_string(),
        "--recurse-submodules".to_string(),
        url.to_string(),
        dest.to_string_lossy().into_owned(),
    ])
    .network()
}

/// Guesses the folder name `git clone` would create for `url`.
pub fn clone_folder_name(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let last = trimmed.rsplit(['/', ':', '\\']).next().unwrap_or(trimmed);
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() {
        "depot".to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_name_from_urls() {
        assert_eq!(clone_folder_name("https://github.com/a/b.git"), "b");
        assert_eq!(
            clone_folder_name("git@github.com:a/fpga-top.git"),
            "fpga-top"
        );
        assert_eq!(clone_folder_name("https://example.com/repo/"), "repo");
        assert_eq!(clone_folder_name("C:\\depots\\proj"), "proj");
    }

    #[test]
    fn parses_log_lines() {
        let line = format!(
            "abc123{SEP}abc{SEP}p1 p2{SEP}Ada{SEP}2026-10-20 10:00{SEP}HEAD -> main, origin/main, tag: v1{SEP}Merge feature"
        );
        let commits = parse_log(&line);
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert_eq!(c.parents, vec!["p1", "p2"]);
        assert_eq!(c.refs, vec!["main", "origin/main", "tag: v1"]);
        assert_eq!(c.subject, "Merge feature");
    }

    /// Runs the real git against a temporary repository.
    #[tokio::test]
    async fn status_and_log_on_real_repo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git = |args: &[&str]| run(GitCommand::new(args.iter().copied()).in_dir(root));

        assert!(git(&["init", "-b", "main"]).await.success());
        assert!(git(&["config", "user.name", "Test"]).await.success());
        assert!(
            git(&["config", "user.email", "t@example.com"])
                .await
                .success()
        );
        std::fs::write(root.join("top.vhd"), "entity top is end;\n").unwrap();
        assert!(git(&["add", "."]).await.success());
        assert!(git(&["commit", "-m", "Premier commit"]).await.success());
        std::fs::write(root.join("top.vhd"), "entity top is\nend;\n").unwrap();
        std::fs::write(root.join("new.vhd"), "-- new\n").unwrap();

        let (paths, _) = discover(root).await;
        let paths = paths.unwrap();
        assert_eq!(
            paths.root.canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
        assert_eq!(operation_in_progress(&paths.git_dir), None);

        let status = parse_status(&run(status_command(root)).await).unwrap();
        assert_eq!(status.branch.head.as_deref(), Some("main"));
        let paths: Vec<_> = status.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["top.vhd", "new.vhd"]);

        let log = run(log_command(root, 10)).await;
        let commits = parse_log(&log.stdout);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "Premier commit");
        assert_eq!(commits[0].refs, vec!["main"]);
    }

    #[tokio::test]
    async fn discover_rejects_plain_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (result, record) = discover(dir.path()).await;
        assert!(result.is_err());
        assert!(record.stderr.contains("not a git repository"));
    }
}
