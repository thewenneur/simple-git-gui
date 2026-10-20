//! Rewriting commits that are not sent yet (swap two neighbours, squash one
//! into the other, edit an older message) with an interactive rebase whose
//! to-do list the app writes itself: no editor ever opens, and the list is
//! shown to the user before it runs.

use std::path::Path;

use super::runner::{CommandRecord, GitCommand, run};

/// The to-do list and the message are written in the git directory.
const TODO_FILE: &str = "simple-git-gui-todo";
const MESSAGE_FILE: &str = "simple-git-gui-message";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// `newer` goes before `older` (they are neighbours).
    Swap { older: String, newer: String },
    /// `newer` is merged into `older`, keeping both messages.
    Squash { older: String, newer: String },
    /// The message of `commit` becomes `message`.
    Reword { commit: String, message: String },
}

impl Edit {
    /// The oldest commit the edit touches: the rebase starts at its parent.
    pub fn oldest(&self) -> &str {
        match self {
            Edit::Swap { older, .. } | Edit::Squash { older, .. } => older,
            Edit::Reword { commit, .. } => commit,
        }
    }
}

/// The to-do list for `edit`, given the commits replayed, oldest first
/// (from the oldest touched commit to HEAD). `None` if a commit of the edit
/// is not in the list.
pub fn todo(chain: &[String], edit: &Edit, message_file: &str) -> Option<String> {
    let position = |hash: &str| chain.iter().position(|h| h == hash);
    let mut lines: Vec<String> = Vec::new();
    match edit {
        Edit::Swap { older, newer } | Edit::Squash { older, newer } => {
            let (o, n) = (position(older)?, position(newer)?);
            if n != o + 1 {
                return None;
            }
            for (i, hash) in chain.iter().enumerate() {
                match edit {
                    Edit::Swap { .. } if i == o => lines.push(format!("pick {newer}")),
                    Edit::Swap { .. } if i == n => lines.push(format!("pick {older}")),
                    Edit::Squash { .. } if i == n => lines.push(format!("squash {hash}")),
                    _ => lines.push(format!("pick {hash}")),
                }
            }
        }
        Edit::Reword { commit, .. } => {
            position(commit)?;
            for hash in chain {
                lines.push(format!("pick {hash}"));
                if hash == commit {
                    lines.push(format!(
                        "exec git commit --amend --only --allow-empty -F {}",
                        sh_quote(message_file)
                    ));
                }
            }
        }
    }
    Some(lines.join("\n") + "\n")
}

/// Quotes a path for the shell git runs the editor and `exec` lines with.
/// Backslashes become slashes so Windows paths work in Git's own shell.
fn sh_quote(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:".contains(c))
    {
        path
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    }
}

/// The rebase command: its editor copies the prepared to-do list.
pub fn command(root: &Path, git_dir: &Path, base: Option<&str>) -> GitCommand {
    let todo = git_dir.join(TODO_FILE);
    let mut args = vec!["rebase", "-i", "--autostash"];
    match base {
        Some(base) => args.push(base),
        None => args.push("--root"),
    }
    GitCommand::new(args).in_dir(root).with_env(
        "GIT_SEQUENCE_EDITOR",
        format!("cp {}", sh_quote(&todo.to_string_lossy())),
    )
}

/// The rebase command as shown before it runs, without the editor variable
/// (the to-do list is shown instead).
pub fn shown(root: &Path, base: Option<&str>) -> String {
    let mut command = command(root, Path::new(""), base);
    command.env.clear();
    command.display()
}

/// Writes the to-do list (and the new message) then runs the rebase.
pub async fn apply(
    root: &Path,
    git_dir: &Path,
    base: Option<&str>,
    chain: &[String],
    edit: &Edit,
) -> Vec<CommandRecord> {
    let message_file = git_dir.join(MESSAGE_FILE);
    let Some(list) = todo(chain, edit, &message_file.to_string_lossy()) else {
        return vec![CommandRecord::local(
            "# rebase -i".to_string(),
            root,
            Err("the commits are not in the expected order".to_string()),
        )];
    };
    let mut records = Vec::new();
    let mut write = |path: &Path, content: &str| {
        let result = std::fs::write(path, content).map_err(|e| e.to_string());
        let ok = result.is_ok();
        records.push(CommandRecord::local(
            format!("# write {}", path.display()),
            root,
            result,
        ));
        ok
    };
    if let Edit::Reword { message, .. } = edit
        && !write(&message_file, message)
    {
        return records;
    }
    if !write(&git_dir.join(TODO_FILE), &list) {
        return records;
    }
    records.push(run(command(root, git_dir, base)).await);
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain() -> Vec<String> {
        ["a", "b", "c"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn builds_the_todo_lists() {
        let swap = Edit::Swap {
            older: "a".into(),
            newer: "b".into(),
        };
        assert_eq!(
            todo(&chain(), &swap, "m").unwrap(),
            "pick b\npick a\npick c\n"
        );
        let squash = Edit::Squash {
            older: "b".into(),
            newer: "c".into(),
        };
        assert_eq!(
            todo(&chain(), &squash, "m").unwrap(),
            "pick a\npick b\nsquash c\n"
        );
        let reword = Edit::Reword {
            commit: "b".into(),
            message: "x".into(),
        };
        assert_eq!(
            todo(&chain(), &reword, "/g/m").unwrap(),
            "pick a\npick b\nexec git commit --amend --only --allow-empty -F /g/m\npick c\n"
        );
        // Not neighbours.
        let far = Edit::Swap {
            older: "a".into(),
            newer: "c".into(),
        };
        assert!(todo(&chain(), &far, "m").is_none());
    }

    async fn git(dir: &Path, args: &[&str]) -> String {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record.stdout
    }

    #[tokio::test]
    async fn swaps_squashes_and_rewords() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q", "-b", "main"]).await;
        git(root, &["config", "user.name", "Test"]).await;
        git(root, &["config", "user.email", "t@example.com"]).await;
        git(root, &["config", "core.autocrlf", "false"]).await;
        for name in ["a", "b", "c"] {
            std::fs::write(root.join(name), name).unwrap();
            git(root, &["add", name]).await;
            git(root, &["commit", "-q", "-m", name]).await;
        }
        let git_dir = root.join(".git");
        let hashes = |out: String| -> Vec<String> { out.lines().map(String::from).collect() };
        let subjects = || async { git(root, &["log", "--format=%s"]).await };

        // Swap b and c.
        let chain = hashes(git(root, &["rev-list", "--reverse", "HEAD~2..HEAD"]).await);
        let base = git(root, &["rev-parse", "HEAD~2"]).await;
        let edit = Edit::Swap {
            older: chain[0].clone(),
            newer: chain[1].clone(),
        };
        let records = apply(root, &git_dir, Some(base.trim()), &chain, &edit).await;
        assert!(records.iter().all(|r| r.success()), "{records:?}");
        assert_eq!(subjects().await, "b\nc\na\n");

        // Squash b (now on top) into c.
        let chain = hashes(git(root, &["rev-list", "--reverse", "HEAD~2..HEAD"]).await);
        let edit = Edit::Squash {
            older: chain[0].clone(),
            newer: chain[1].clone(),
        };
        let records = apply(root, &git_dir, Some(base.trim()), &chain, &edit).await;
        assert!(records.iter().all(|r| r.success()), "{records:?}");
        assert_eq!(
            git(root, &["log", "-1", "--format=%B"]).await.trim(),
            "c\n\nb"
        );
        assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).await, "2\n");

        // Reword the first commit (the root).
        let chain = hashes(git(root, &["rev-list", "--reverse", "HEAD"]).await);
        let edit = Edit::Reword {
            commit: chain[0].clone(),
            message: "first\n".into(),
        };
        let records = apply(root, &git_dir, None, &chain, &edit).await;
        assert!(records.iter().all(|r| r.success()), "{records:?}");
        assert_eq!(git(root, &["log", "--format=%s"]).await, "c\nfirst\n");
    }
}
