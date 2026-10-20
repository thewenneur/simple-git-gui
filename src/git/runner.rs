//! Runs the `git` command-line tool.
//!
//! Every call goes through [`run`], which:
//! - never inherits a terminal (stdin is closed, prompts are disabled), so git
//!   cannot block forever waiting for input;
//! - is bounded by a timeout and killed if it exceeds it;
//! - returns a [`CommandRecord`] describing exactly what ran, so the UI can show
//!   the real git command next to every guided action.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Command;

/// Timeout for local, fast operations (status, log, diff...).
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Timeout for operations that talk to a remote (clone, fetch, pull, push).
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Options that make git's output stable and its behavior non-interactive.
/// They are passed with `-c` so the user's own configuration is untouched.
const FIXED_CONFIG: &[&str] = &["core.quotepath=false", "color.ui=false"];

/// A git invocation to perform.
#[derive(Debug, Clone)]
pub struct GitCommand {
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
    /// Background commands (periodic refresh) are hidden from the journal by default.
    pub background: bool,
}

impl GitCommand {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            timeout: LOCAL_TIMEOUT,
            background: false,
        }
    }

    pub fn in_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn network(mut self) -> Self {
        self.timeout = NETWORK_TIMEOUT;
        self
    }

    pub fn background(mut self) -> Self {
        self.background = true;
        self
    }

    /// The command as a user would type it in a terminal.
    pub fn display(&self) -> String {
        std::iter::once("git")
            .chain(self.args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Quotes an argument only when a shell would need it, so the journal stays
/// readable (`--porcelain=v2` rather than `'--porcelain=v2'`).
fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_alphanumeric() || "-_=./:,@+%^~".contains(c));
    if plain {
        arg.to_string()
    } else {
        shell_words::quote(arg).into_owned()
    }
}

/// How a git invocation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// git exited; the code is 0 on success.
    Exited(i32),
    /// git was killed by a signal (Unix) or ended without an exit code.
    Killed,
    /// git took longer than its timeout and was stopped.
    TimedOut,
    /// git could not be started at all (not installed, bad directory...).
    SpawnFailed(String),
}

/// Everything that is known about one git invocation, for the journal.
#[derive(Debug, Clone)]
pub struct CommandRecord {
    pub command: GitCommand,
    pub outcome: Outcome,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

impl CommandRecord {
    pub fn success(&self) -> bool {
        self.outcome == Outcome::Exited(0)
    }

    /// A short explanation of a failure, for the status bar.
    pub fn error_summary(&self) -> String {
        match &self.outcome {
            Outcome::Exited(0) => String::new(),
            Outcome::Exited(code) => {
                let msg = self
                    .stderr
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .trim();
                if msg.is_empty() {
                    t!("runner.exit_code", code = code).into_owned()
                } else {
                    msg.to_string()
                }
            }
            Outcome::Killed => t!("runner.killed").into_owned(),
            Outcome::TimedOut => {
                t!("runner.timed_out", secs = self.command.timeout.as_secs()).into_owned()
            }
            Outcome::SpawnFailed(e) => t!("runner.spawn_failed", detail = e).into_owned(),
        }
    }
}

/// Runs a git command and collects its output.
pub async fn run(command: GitCommand) -> CommandRecord {
    let start = Instant::now();

    let mut cmd = Command::new("git");
    for config in FIXED_CONFIG {
        cmd.arg("-c").arg(config);
    }
    cmd.args(&command.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Never ask anything on a terminal: fail instead of hanging.
        .env("GIT_TERMINAL_PROMPT", "0")
        // An editor would wait for the user forever (e.g. merge messages).
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_PAGER", "cat")
        // Messages must be parseable regardless of the system language.
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C")
        .kill_on_drop(true);
    if let Some(dir) = &command.cwd {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: do not flash a console window for each command.
        cmd.creation_flags(0x0800_0000);
    }

    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return CommandRecord {
                command,
                outcome: Outcome::SpawnFailed(e.to_string()),
                stdout: String::new(),
                stderr: String::new(),
                duration: start.elapsed(),
            };
        }
    };

    // On timeout the future is dropped, which kills the child (kill_on_drop).
    match tokio::time::timeout(command.timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => CommandRecord {
            outcome: match output.status.code() {
                Some(code) => Outcome::Exited(code),
                None => Outcome::Killed,
            },
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            duration: start.elapsed(),
            command,
        },
        Ok(Err(e)) => CommandRecord {
            outcome: Outcome::SpawnFailed(e.to_string()),
            stdout: String::new(),
            stderr: String::new(),
            duration: start.elapsed(),
            command,
        },
        Err(_) => CommandRecord {
            outcome: Outcome::TimedOut,
            stdout: String::new(),
            stderr: String::new(),
            duration: start.elapsed(),
            command,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_arguments() {
        let cmd = GitCommand::new(["commit", "-m", "mon message"]);
        assert_eq!(cmd.display(), "git commit -m 'mon message'");
        let cmd = GitCommand::new(["status", "--porcelain=v2", "--format=%H %s"]);
        assert_eq!(cmd.display(), "git status --porcelain=v2 '--format=%H %s'");
    }

    #[tokio::test]
    async fn runs_git_version() {
        let record = run(GitCommand::new(["--version"])).await;
        assert!(record.success(), "{:?}", record);
        assert!(record.stdout.starts_with("git version"));
    }

    #[tokio::test]
    async fn reports_failure_message() {
        let dir = tempfile::tempdir().unwrap();
        let record = run(GitCommand::new(["status"]).in_dir(dir.path())).await;
        assert!(!record.success());
        assert!(record.error_summary().contains("not a git repository"));
    }

    #[tokio::test]
    async fn times_out_instead_of_hanging() {
        let dir = tempfile::tempdir().unwrap();
        // A shell alias that sleeps stands in for a stuck git process.
        let mut cmd = GitCommand::new(["-c", "alias.wait=!sleep 5", "wait"]).in_dir(dir.path());
        cmd.timeout = Duration::from_millis(200);
        let record = run(cmd).await;
        assert_eq!(record.outcome, Outcome::TimedOut);
    }
}
