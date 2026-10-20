//! Remotes (the servers a repository gets from and sends to) and creating a
//! new repository.

use std::path::Path;

use super::runner::GitCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub fetch: String,
    /// Where sends go, when it differs from `fetch`.
    pub push: Option<String>,
}

pub fn list_command(root: &Path) -> GitCommand {
    GitCommand::new(["remote", "-v"]).in_dir(root).background()
}

/// Parses `git remote -v`: `name\turl (fetch)` and `name\turl (push)`.
pub fn parse(output: &str) -> Vec<Remote> {
    let mut remotes: Vec<Remote> = Vec::new();
    for line in output.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let (url, kind) = match rest.rsplit_once(' ') {
            Some((url, kind)) if kind.starts_with('(') => (url, kind),
            _ => (rest, "(fetch)"),
        };
        let index = match remotes.iter().position(|r| r.name == name) {
            Some(i) => i,
            None => {
                remotes.push(Remote {
                    name: name.to_string(),
                    fetch: String::new(),
                    push: None,
                });
                remotes.len() - 1
            }
        };
        let remote = &mut remotes[index];
        if kind == "(push)" {
            remote.push = Some(url.to_string());
        } else {
            remote.fetch = url.to_string();
        }
    }
    for remote in &mut remotes {
        if remote.push.as_deref() == Some(remote.fetch.as_str()) {
            remote.push = None;
        }
    }
    remotes
}

/// Whether `name` can be a remote name (it becomes part of branch names such
/// as `origin/main`).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.', '/'])
        && !name.ends_with(['.', '/'])
        && !name.ends_with(".lock")
        && !name.contains("..")
        && !name.contains("//")
        && name
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !"~^:?*[\\@{".contains(c))
}

pub fn add(root: &Path, name: &str, url: &str) -> GitCommand {
    GitCommand::new(["remote", "add", "--", name, url]).in_dir(root)
}

pub fn rename(root: &Path, from: &str, to: &str) -> GitCommand {
    GitCommand::new(["remote", "rename", "--", from, to]).in_dir(root)
}

pub fn set_url(root: &Path, name: &str, url: &str, push: bool) -> GitCommand {
    let mut args = vec!["remote", "set-url"];
    if push {
        args.push("--push");
    }
    args.extend(["--", name, url]);
    GitCommand::new(args).in_dir(root)
}

/// Stops using a separate address for sending.
pub fn unset_push_url(root: &Path, name: &str) -> GitCommand {
    GitCommand::new([
        "config".to_string(),
        "--unset-all".to_string(),
        format!("remote.{name}.pushurl"),
    ])
    .in_dir(root)
}

pub fn remove(root: &Path, name: &str) -> GitCommand {
    GitCommand::new(["remote", "remove", "--", name]).in_dir(root)
}

/// Checks the server answers (lists its branches, changes nothing).
pub fn test(root: &Path, name: &str) -> GitCommand {
    GitCommand::new(["ls-remote", "--heads", "--", name])
        .in_dir(root)
        .network()
}

/// Creates an empty repository in `dir` (created if missing) with `branch`
/// as its first branch.
pub fn init(dir: &Path, branch: &str) -> GitCommand {
    GitCommand::new([
        "init".to_string(),
        format!("--initial-branch={branch}"),
        "--".to_string(),
        dir.to_string_lossy().into_owned(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::runner::run;

    #[test]
    fn parses_remotes() {
        let output = "origin\thttps://example.com/a.git (fetch)\norigin\thttps://example.com/a.git (push)\nlab\t/srv/a.git (fetch)\nlab\tssh://lab/a.git (push)\n";
        let remotes = parse(output);
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].push, None);
        assert_eq!(remotes[1].fetch, "/srv/a.git");
        assert_eq!(remotes[1].push.as_deref(), Some("ssh://lab/a.git"));
        assert!(parse("").is_empty());
    }

    #[test]
    fn validates_names() {
        for good in ["origin", "upstream", "lab-server", "team/main_2"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in ["", "my server", "-x", "a..b", "a:b", "x.lock", "a/", "@{x"] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn creates_and_manages_remotes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("new project");
        let record = run(init(&dir, "main").in_dir(tmp.path())).await;
        assert!(record.success(), "{}", record.stderr);
        let head = run(GitCommand::new(["symbolic-ref", "HEAD"]).in_dir(&dir)).await;
        assert_eq!(head.stdout.trim(), "refs/heads/main");

        for command in [
            add(&dir, "origin", "https://example.com/a.git"),
            rename(&dir, "origin", "lab"),
            set_url(&dir, "lab", "https://example.com/b.git", false),
            set_url(&dir, "lab", "ssh://example.com/b.git", true),
        ] {
            let record = run(command).await;
            assert!(record.success(), "{}", record.stderr);
        }
        let remotes = parse(&run(list_command(&dir)).await.stdout);
        assert_eq!(
            remotes,
            [Remote {
                name: "lab".into(),
                fetch: "https://example.com/b.git".into(),
                push: Some("ssh://example.com/b.git".into()),
            }]
        );
        assert!(run(unset_push_url(&dir, "lab")).await.success());
        assert!(run(remove(&dir, "lab")).await.success());
        assert!(parse(&run(list_command(&dir)).await.stdout).is_empty());
    }
}
