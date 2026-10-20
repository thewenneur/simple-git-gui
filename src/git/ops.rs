//! The git commands behind the guided actions (stage, commit, fetch,
//! integrate, push...). Each function only builds the command, so the UI can
//! show it before or while running it.

use std::collections::HashMap;
use std::path::Path;

use super::repo::Operation;
use super::runner::GitCommand;

/// How to integrate the server's commits into the local branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Integration {
    /// Put local commits after the server's (`pull.rebase = true`).
    #[default]
    Rebase,
    /// Create a merge commit (`pull.rebase = false`).
    Merge,
}

impl Integration {
    /// From the value of `git config pull.rebase` (rebase when unset).
    pub fn from_config(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("false") => Integration::Merge,
            _ => Integration::Rebase,
        }
    }
}

fn paths_args<'a>(base: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    let mut args = base.to_vec();
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    args
}

/// Includes files in the next commit (new, modified or deleted).
pub fn stage(root: &Path, paths: &[String]) -> GitCommand {
    GitCommand::new(paths_args(&["add", "-A"], paths)).in_dir(root)
}

pub fn stage_all(root: &Path) -> GitCommand {
    GitCommand::new(["add", "-A"]).in_dir(root)
}

/// Saves every tracked file again with the current line-ending rules
/// (`core.autocrlf`, `.gitattributes`): the fixed files are staged.
pub fn renormalize(root: &Path) -> GitCommand {
    GitCommand::new(["add", "--renormalize", "--", "."]).in_dir(root)
}

/// Takes files out of the next commit, keeping their changes on disk.
pub fn unstage(root: &Path, paths: &[String], has_head: bool) -> GitCommand {
    let base: &[&str] = if has_head {
        &["restore", "--staged"]
    } else {
        // Before the first commit there is nothing to restore from.
        &["rm", "--cached", "-r", "-q"]
    };
    GitCommand::new(paths_args(base, paths)).in_dir(root)
}

/// Includes (or with `reverse`, removes) a single hunk in the index.
pub fn apply_to_index(root: &Path, patch: String, reverse: bool) -> GitCommand {
    let mut args = vec!["apply", "--cached", "--whitespace=nowarn"];
    if reverse {
        args.push("--reverse");
    }
    args.push("-");
    GitCommand::new(args).in_dir(root).with_input(patch)
}

/// `git commit -m <summary> [-m <description>]`: Git joins the two with a
/// blank line.
pub fn commit(root: &Path, summary: &str, description: &str) -> GitCommand {
    let mut command = GitCommand::new(["commit", "-m", summary]);
    if !description.is_empty() {
        command
            .args
            .extend(["-m".to_string(), description.to_string()]);
    }
    command.in_dir(root)
}

pub fn remotes(root: &Path) -> GitCommand {
    GitCommand::new(["remote"]).in_dir(root)
}

pub fn fetch(root: &Path, remote: &str) -> GitCommand {
    GitCommand::new(["fetch", "--prune", remote])
        .in_dir(root)
        .network()
}

pub fn pull_rebase_setting(root: &Path) -> GitCommand {
    GitCommand::new(["config", "--get", "pull.rebase"]).in_dir(root)
}

/// Commits on the upstream that are not local yet: `author␟subject` lines.
pub fn incoming(root: &Path, upstream: &str) -> GitCommand {
    GitCommand::new([
        "log".to_string(),
        "--format=%an%x1f%s".to_string(),
        format!("HEAD..{upstream}"),
    ])
    .in_dir(root)
}

pub fn parse_incoming(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|l| l.split_once('\u{1f}'))
        .map(|(a, s)| (a.to_string(), s.to_string()))
        .collect()
}

/// Integrates `upstream` into the current branch. Local uncommitted changes
/// are put aside and restored automatically (`--autostash`).
pub fn integrate(root: &Path, upstream: &str, how: Integration) -> GitCommand {
    let args: Vec<&str> = match how {
        Integration::Rebase => vec!["rebase", "--autostash", upstream],
        Integration::Merge => vec!["merge", "--autostash", "--no-edit", upstream],
    };
    GitCommand::new(args).in_dir(root)
}

pub fn push(root: &Path, remote: &str, branch: &str, set_upstream: bool) -> GitCommand {
    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
    }
    args.extend([remote, branch]);
    GitCommand::new(args).in_dir(root).network()
}

/// Stops the operation in progress and puts everything back as before it.
pub fn abort(root: &Path, operation: Operation) -> GitCommand {
    let verb = match operation {
        Operation::Merge => "merge",
        Operation::Rebase => "rebase",
        Operation::CherryPick => "cherry-pick",
        Operation::Revert => "revert",
    };
    GitCommand::new([verb, "--abort"]).in_dir(root)
}

/// Commits that `push` would send: those not on `upstream`, or not on any
/// branch of `remote` when the branch was never published.
pub fn outgoing_count(root: &Path, upstream: Option<&str>, remote: &str) -> GitCommand {
    let args = match upstream {
        Some(u) => vec![
            "rev-list".to_string(),
            "--count".to_string(),
            format!("{u}..HEAD"),
        ],
        None => vec![
            "rev-list".to_string(),
            "--count".to_string(),
            "HEAD".to_string(),
            "--not".to_string(),
            format!("--remotes={remote}"),
        ],
    };
    GitCommand::new(args).in_dir(root)
}

/// Stops tracking `path` (a file or a folder) without deleting it from disk.
pub fn untrack(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["rm", "--cached", "-r", "-q", "--ignore-unmatch", "--", path]).in_dir(root)
}

/// The remote of an upstream like `origin/feature/x`, among known remotes.
pub fn remote_of<'a>(upstream: &str, remotes: &'a [String]) -> Option<&'a str> {
    remotes
        .iter()
        .filter(|r| upstream.starts_with(&format!("{r}/")))
        .max_by_key(|r| r.len())
        .map(String::as_str)
}

/// The remote to publish a branch on: `origin` if present, else the first.
pub fn default_remote(remotes: &[String]) -> Option<&str> {
    remotes
        .iter()
        .find(|r| *r == "origin")
        .or_else(|| remotes.first())
        .map(String::as_str)
}

/// Lines added and removed per file.
pub fn numstat(root: &Path, staged: bool) -> GitCommand {
    let mut args = vec!["diff", "--numstat", "--no-renames", "-z"];
    if staged {
        args.push("--cached");
    }
    GitCommand::new(args).in_dir(root)
}

/// `added\tremoved\tpath\0` entries; binary files report `-`.
pub fn parse_numstat(output: &str) -> HashMap<String, (u32, u32)> {
    output
        .split('\0')
        .filter_map(|entry| {
            let mut parts = entry.splitn(3, '\t');
            let added = parts.next()?.parse().unwrap_or(0);
            let removed = parts.next()?.parse().unwrap_or(0);
            let path = parts.next()?;
            Some((path.to_string(), (added, removed)))
        })
        .collect()
}

/// Recognizes git's "Please tell me who you are" error.
pub fn is_missing_identity(stderr: &str) -> bool {
    stderr.contains("Please tell me who you are")
        || stderr.contains("unable to auto-detect email address")
        || stderr.contains("empty ident name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff;
    use crate::git::runner::{CommandRecord, run};

    #[test]
    fn integration_follows_pull_rebase() {
        assert_eq!(Integration::from_config(None), Integration::Rebase);
        assert_eq!(
            Integration::from_config(Some("true\n")),
            Integration::Rebase
        );
        assert_eq!(Integration::from_config(Some("false")), Integration::Merge);
    }

    #[test]
    fn finds_remote_of_upstream() {
        let remotes = vec!["origin".to_string(), "origin-lab".to_string()];
        assert_eq!(remote_of("origin/feature/x", &remotes), Some("origin"));
        assert_eq!(remote_of("origin-lab/main", &remotes), Some("origin-lab"));
        assert_eq!(remote_of("other/main", &remotes), None);
        assert_eq!(default_remote(&remotes), Some("origin"));
        assert_eq!(default_remote(&[]), None);
    }

    #[test]
    fn parses_numstat() {
        let stats = parse_numstat("14\t3\trtl/uart_rx.vhd\0-\t-\tlogo.png\0");
        assert_eq!(stats["rtl/uart_rx.vhd"], (14, 3));
        assert_eq!(stats["logo.png"], (0, 0));
    }

    async fn git(dir: &Path, args: &[&str]) -> CommandRecord {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record
    }

    async fn new_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]).await;
        git(dir, &["config", "user.name", "Test"]).await;
        git(dir, &["config", "user.email", "t@example.com"]).await;
    }

    /// Stage one hunk only, then commit it: the other hunk stays unstaged.
    #[tokio::test]
    async fn stages_a_single_hunk_and_commits() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        new_repo(root).await;
        let original: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        std::fs::write(root.join("a.vhd"), &original).unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;

        let changed = original
            .replace("line 2\n", "line 2 changed\n")
            .replace("line 28\n", "line 28 changed\n");
        std::fs::write(root.join("a.vhd"), changed).unwrap();

        let d = run(diff::diff_command(root, "a.vhd", false, Default::default())).await;
        let parsed = diff::parse(&d.stdout, true);
        assert_eq!(parsed.hunks.len(), 2);

        let r = run(apply_to_index(root, parsed.hunk_patch(0).unwrap(), false)).await;
        assert!(r.success(), "{}", r.stderr);
        let staged = run(diff::diff_command(root, "a.vhd", true, Default::default())).await;
        assert!(staged.stdout.contains("+line 2 changed"));
        assert!(!staged.stdout.contains("line 28 changed"));

        // Removing the hunk from the index again.
        let parsed_staged = diff::parse(&staged.stdout, true);
        let r = run(apply_to_index(
            root,
            parsed_staged.hunk_patch(0).unwrap(),
            true,
        ))
        .await;
        assert!(r.success(), "{}", r.stderr);
        let staged = run(diff::diff_command(root, "a.vhd", true, Default::default())).await;
        assert!(staged.stdout.is_empty());

        let r = run(apply_to_index(root, parsed.hunk_patch(0).unwrap(), false)).await;
        assert!(r.success());
        let r = run(commit(
            root,
            "Change line 2",
            "Only the first hunk.\n\nThe rest waits.",
        ))
        .await;
        assert!(r.success(), "{}", r.stderr);
        let message = run(GitCommand::new(["log", "-1", "--format=%B"]).in_dir(root)).await;
        assert_eq!(
            message.stdout.trim_end(),
            "Change line 2\n\nOnly the first hunk.\n\nThe rest waits."
        );
        let unstaged = run(diff::diff_command(root, "a.vhd", false, Default::default())).await;
        assert!(unstaged.stdout.contains("+line 28 changed"));
    }

    #[tokio::test]
    async fn stage_and_unstage_files_before_first_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        new_repo(root).await;
        std::fs::write(root.join("new.vhd"), "x\n").unwrap();
        assert!(run(stage(root, &["new.vhd".into()])).await.success());
        let s = run(numstat(root, true)).await;
        assert_eq!(parse_numstat(&s.stdout)["new.vhd"], (1, 0));
        assert!(
            run(unstage(root, &["new.vhd".into()], false))
                .await
                .success()
        );
        let s = run(numstat(root, true)).await;
        assert!(parse_numstat(&s.stdout).is_empty());
    }

    /// Two clones of one server: the second integrates the first's work
    /// before pushing, as the "Envoyer" wizard does.
    #[tokio::test]
    async fn fetch_integrate_then_push() {
        let tmp = tempfile::tempdir().unwrap();
        let server = tmp.path().join("server.git");
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        git(
            tmp.path(),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                server.to_str().unwrap(),
            ],
        )
        .await;
        std::fs::create_dir(&a).unwrap();
        new_repo(&a).await;
        std::fs::write(a.join("top.vhd"), "entity top is end;\n").unwrap();
        git(&a, &["add", "."]).await;
        git(&a, &["commit", "-q", "-m", "init"]).await;
        git(&a, &["remote", "add", "origin", server.to_str().unwrap()]).await;
        let remotes_out = git(&a, &["remote"]).await.stdout;
        let remotes: Vec<String> = remotes_out.lines().map(str::to_string).collect();
        let remote = default_remote(&remotes).unwrap();
        assert!(run(push(&a, remote, "main", true)).await.success());

        git(
            tmp.path(),
            &["clone", "-q", server.to_str().unwrap(), b.to_str().unwrap()],
        )
        .await;
        git(&b, &["config", "user.name", "Lea"]).await;
        git(&b, &["config", "user.email", "l@example.com"]).await;
        std::fs::write(b.join("tb.vhd"), "-- tb\n").unwrap();
        git(&b, &["add", "."]).await;
        git(&b, &["commit", "-q", "-m", "Testbench"]).await;
        git(&b, &["push", "-q"]).await;

        // `a` has a local commit and an uncommitted change.
        std::fs::write(a.join("uart.vhd"), "-- uart\n").unwrap();
        git(&a, &["add", "."]).await;
        git(&a, &["commit", "-q", "-m", "UART"]).await;
        std::fs::write(a.join("top.vhd"), "entity top is end; -- wip\n").unwrap();

        assert!(run(fetch(&a, "origin")).await.success());
        let inc = run(incoming(&a, "origin/main")).await;
        assert_eq!(
            parse_incoming(&inc.stdout),
            vec![("Lea".to_string(), "Testbench".to_string())]
        );
        let r = run(integrate(&a, "origin/main", Integration::Rebase)).await;
        assert!(r.success(), "{}", r.stderr);
        // The uncommitted change survived the rebase.
        let top = std::fs::read_to_string(a.join("top.vhd")).unwrap();
        assert!(top.contains("-- wip"));
        assert!(run(push(&a, "origin", "main", false)).await.success());
        let log = git(&b, &["ls-remote", "origin", "main"]).await;
        let head = git(&a, &["rev-parse", "HEAD"]).await;
        assert!(log.stdout.starts_with(head.stdout.trim()));
    }

    /// A conflicting integration stops; `abort` puts everything back.
    #[tokio::test]
    async fn abort_restores_state_after_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        new_repo(root).await;
        std::fs::write(root.join("a.vhd"), "BAUD := 115200;\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        git(root, &["checkout", "-q", "-b", "other"]).await;
        std::fs::write(root.join("a.vhd"), "BAUD := 9600;\n").unwrap();
        git(root, &["commit", "-q", "-am", "theirs"]).await;
        git(root, &["checkout", "-q", "main"]).await;
        std::fs::write(root.join("a.vhd"), "BAUD := 230400;\n").unwrap();
        git(root, &["commit", "-q", "-am", "mine"]).await;
        let before = git(root, &["rev-parse", "HEAD"]).await.stdout;

        let outgoing = run(outgoing_count(root, Some("other"), "origin")).await;
        assert_eq!(outgoing.stdout.trim(), "1");

        let r = run(integrate(root, "other", Integration::Rebase)).await;
        assert!(!r.success());
        let git_dir = root.join(".git");
        let op = crate::git::repo::operation_in_progress(&git_dir).unwrap();
        assert_eq!(op, Operation::Rebase);
        assert!(run(abort(root, op)).await.success());
        assert_eq!(git(root, &["rev-parse", "HEAD"]).await.stdout, before);
        assert!(crate::git::repo::operation_in_progress(&git_dir).is_none());
    }

    #[test]
    fn recognizes_missing_identity() {
        assert!(is_missing_identity(
            "Author identity unknown\n\n*** Please tell me who you are."
        ));
        assert!(!is_missing_identity("nothing to commit"));
    }

    /// A file stored with Windows line endings is staged again with Linux
    /// ones once `.gitattributes` asks for normalized text.
    #[tokio::test]
    async fn renormalize_stages_files_with_other_line_endings() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        new_repo(root).await;
        git(root, &["config", "core.autocrlf", "false"]).await;
        std::fs::write(root.join("a.vhd"), "one\r\ntwo\r\n").unwrap();
        std::fs::write(root.join("b.vhd"), "one\ntwo\n").unwrap();
        git(root, &["add", "."]).await;
        git(root, &["commit", "-q", "-m", "init"]).await;
        std::fs::write(root.join(".gitattributes"), "* text=auto\n").unwrap();

        assert!(run(renormalize(root)).await.success());
        let staged = git(root, &["diff", "--cached", "--name-only"]).await.stdout;
        assert_eq!(staged.lines().collect::<Vec<_>>(), ["a.vhd"]);
        let stored = git(root, &["show", ":a.vhd"]).await.stdout;
        assert_eq!(stored, "one\ntwo\n");
    }
}
