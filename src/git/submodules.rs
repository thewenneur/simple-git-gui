//! Submodules: other repositories included in this one, each pinned to a
//! commit. What the app shows for each one and the commands that download,
//! update, reset and add them.

use std::path::{Path, PathBuf};

use super::runner::{CommandRecord, GitCommand, run};

/// Where a submodule stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Checked out on the commit this repository pins.
    UpToDate,
    /// Not downloaded yet (`git submodule update --init` never ran).
    NotDownloaded,
    /// Checked out on another commit than the pinned one.
    Moved,
    /// The pinned commit is in conflict (during a merge).
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    pub name: String,
    pub path: String,
    pub url: String,
    /// The branch followed by "update to the server" (`submodule.<n>.branch`).
    pub branch: Option<String>,
    /// The commit this repository pins.
    pub pinned: String,
    /// The commit checked out in the submodule, when downloaded.
    pub checked_out: Option<String>,
    pub state: State,
    /// Files changed inside the submodule and not saved there.
    pub dirty: bool,
    /// Commits on the server branch not in the pinned commit, as last fetched.
    pub behind: Option<usize>,
    /// The pinned commit's subject, and the server's latest one.
    pub pinned_subject: Option<String>,
    pub latest: Option<(String, String)>,
}

impl Submodule {
    pub fn downloaded(&self) -> bool {
        self.state != State::NotDownloaded
    }

    /// The branch on the server "update" follows: the configured branch, or
    /// the server's default branch.
    pub fn remote_ref(&self) -> String {
        match &self.branch {
            Some(b) if b != "." => format!("origin/{b}"),
            _ => "origin/HEAD".to_string(),
        }
    }

    pub fn needs_attention(&self) -> bool {
        self.state != State::UpToDate || self.dirty || self.behind.is_some_and(|n| n > 0)
    }
}

/// `.gitmodules` entries: name → (path, url, branch).
pub fn config_command(root: &Path) -> GitCommand {
    GitCommand::new([
        "config",
        "--file",
        ".gitmodules",
        "--get-regexp",
        r"^submodule\.",
    ])
    .in_dir(root)
    .background()
}

pub fn status_command(root: &Path) -> GitCommand {
    GitCommand::new(["submodule", "status"])
        .in_dir(root)
        .background()
}

/// Reads `.gitmodules` and `git submodule status` into the list shown.
pub fn parse(config: &str, status: &str) -> Vec<Submodule> {
    let mut list: Vec<Submodule> = Vec::new();
    for line in config.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let Some(rest) = key.strip_prefix("submodule.") else {
            continue;
        };
        let Some((name, field)) = rest.rsplit_once('.') else {
            continue;
        };
        let index = match list.iter().position(|s| s.name == name) {
            Some(i) => i,
            None => {
                list.push(Submodule {
                    name: name.to_string(),
                    path: name.to_string(),
                    url: String::new(),
                    branch: None,
                    pinned: String::new(),
                    checked_out: None,
                    state: State::NotDownloaded,
                    dirty: false,
                    behind: None,
                    pinned_subject: None,
                    latest: None,
                });
                list.len() - 1
            }
        };
        let entry = &mut list[index];
        match field {
            "path" => entry.path = value.to_string(),
            "url" => entry.url = value.to_string(),
            "branch" => entry.branch = Some(value.to_string()),
            _ => {}
        }
    }
    // " <sha> path (describe)", "-<sha> path", "+<sha> path (describe)", "U<sha> path"
    for line in status.lines() {
        let Some(flag) = line.chars().next() else {
            continue;
        };
        let mut words = line[flag.len_utf8()..].split_whitespace();
        let (Some(hash), Some(path)) = (words.next(), words.next()) else {
            continue;
        };
        let Some(entry) = list.iter_mut().find(|s| s.path == path) else {
            continue;
        };
        entry.state = match flag {
            '-' => State::NotDownloaded,
            '+' => State::Moved,
            'U' => State::Conflict,
            _ => State::UpToDate,
        };
        match entry.state {
            State::Moved => entry.checked_out = Some(hash.to_string()),
            State::UpToDate => {
                entry.pinned = hash.to_string();
                entry.checked_out = Some(hash.to_string());
            }
            _ => entry.pinned = hash.to_string(),
        }
    }
    list
}

/// The commit a submodule is pinned to in the index (needed when it is
/// checked out elsewhere: `submodule status` then shows the checked out one).
fn pinned_command(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["ls-files", "--stage", "--", path])
        .in_dir(root)
        .background()
}

fn in_submodule(root: &Path, path: &str, args: &[&str]) -> GitCommand {
    GitCommand::new(args.iter().copied())
        .in_dir(root.join(path))
        .background()
}

/// Fills in what needs a look inside each downloaded submodule: local
/// changes, the pinned commit's subject, and how far the server is ahead.
pub async fn inspect(root: &Path, list: &mut [Submodule]) -> Vec<CommandRecord> {
    let mut records = Vec::new();
    for sub in list.iter_mut() {
        if sub.state == State::Moved {
            let record = run(pinned_command(root, &sub.path)).await;
            // "160000 <sha> 0\tpath"
            if let Some(hash) = record.stdout.split_whitespace().nth(1) {
                sub.pinned = hash.to_string();
            }
            records.push(record);
        }
        if !sub.downloaded() {
            continue;
        }
        let dirty = run(in_submodule(
            root,
            &sub.path,
            &["status", "--porcelain", "--untracked-files=no"],
        ))
        .await;
        sub.dirty = dirty.success() && !dirty.stdout.trim().is_empty();
        records.push(dirty);

        let subject = run(in_submodule(
            root,
            &sub.path,
            &["log", "-1", "--format=%s", &sub.pinned],
        ))
        .await;
        if subject.success() {
            sub.pinned_subject = Some(subject.stdout.trim().to_string());
        }
        records.push(subject);

        let remote = sub.remote_ref();
        let range = format!("{}..{remote}", sub.pinned);
        let behind = run(in_submodule(
            root,
            &sub.path,
            &["rev-list", "--count", &range],
        ))
        .await;
        sub.behind = behind.stdout.trim().parse().ok();
        records.push(behind);
        if sub.behind.is_some_and(|n| n > 0) {
            let latest = run(in_submodule(
                root,
                &sub.path,
                &["log", "-1", "--format=%h%x1f%s", &remote],
            ))
            .await;
            sub.latest = latest
                .stdout
                .trim()
                .split_once('\u{1f}')
                .map(|(h, s)| (h.to_string(), s.to_string()));
            records.push(latest);
        }
    }
    records
}

/// Asks each downloaded submodule's server what is new (network).
pub fn fetch(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["fetch", "--quiet", "origin"])
        .in_dir(root.join(path))
        .network()
}

/// Downloads a submodule (and the ones inside it) at the pinned commit.
pub fn download(root: &Path, path: Option<&str>) -> GitCommand {
    let mut args = vec!["submodule", "update", "--init", "--recursive"];
    if let Some(path) = path {
        args.extend(["--", path]);
    }
    GitCommand::new(args).in_dir(root).network()
}

/// Puts a moved submodule back on the pinned commit.
pub fn reset(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["submodule", "update", "--", path])
        .in_dir(root)
        .network()
}

/// Moves a submodule to the latest commit of its server branch, then pins
/// that commit in this repository (staged, saved with the next commit).
pub fn update_to_server(root: &Path, path: &str) -> Vec<GitCommand> {
    vec![
        GitCommand::new(["submodule", "update", "--remote", "--", path])
            .in_dir(root)
            .network(),
        GitCommand::new(["add", "--", path]).in_dir(root),
    ]
}

/// Updates the recorded server addresses, then downloads what is missing.
pub fn sync_all(root: &Path) -> Vec<GitCommand> {
    vec![
        GitCommand::new(["submodule", "sync", "--recursive"]).in_dir(root),
        download(root, None),
    ]
}

/// Adds a repository as a submodule in `path`.
pub fn add(root: &Path, url: &str, path: &str) -> GitCommand {
    GitCommand::new(["submodule", "add", "--", url, path])
        .in_dir(root)
        .network()
}

/// A folder name from a repository address: `…/axi-uart-lite.git` →
/// `axi-uart-lite`.
pub fn folder_from_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let last = url.rsplit(['/', ':', '\\']).next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// The absolute folder of a submodule, to open it as a repository.
pub fn folder(root: &Path, path: &str) -> PathBuf {
    root.join(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\
submodule.ip/axi.path ip/axi
submodule.ip/axi.url https://example.com/axi.git
submodule.lib.path lib/common-vhdl
submodule.lib.url ../common-vhdl.git
submodule.lib.branch develop
submodule.osvvm.path sim/osvvm
submodule.osvvm.url https://github.com/OSVVM/OSVVM
";

    const STATUS: &str = " e72a4c0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ip/axi (heads/main)
+91fd0c3bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb lib/common-vhdl (v1.2-3-g91fd0c3)
-2023090ccccccccccccccccccccccccccccccccc sim/osvvm
";

    #[test]
    fn parses_config_and_status() {
        let list = parse(CONFIG, STATUS);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].path, "ip/axi");
        assert_eq!(list[0].state, State::UpToDate);
        assert_eq!(list[0].remote_ref(), "origin/HEAD");
        assert_eq!(list[1].name, "lib");
        assert_eq!(list[1].state, State::Moved);
        assert_eq!(list[1].remote_ref(), "origin/develop");
        assert!(
            list[1]
                .checked_out
                .as_deref()
                .unwrap()
                .starts_with("91fd0c3")
        );
        assert_eq!(list[2].state, State::NotDownloaded);
        assert!(!list[2].downloaded());
        assert!(list[2].needs_attention());
    }

    #[test]
    fn folder_names_from_urls() {
        assert_eq!(
            folder_from_url("https://github.com/labo/axi-uart-lite.git"),
            "axi-uart-lite"
        );
        assert_eq!(folder_from_url("git@github.com:labo/ddr.git/"), "ddr");
        assert_eq!(folder_from_url("../common-vhdl"), "common-vhdl");
    }

    async fn git(dir: &Path, args: &[&str]) -> String {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record.stdout
    }

    async fn init(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]).await;
        git(dir, &["config", "user.name", "Test"]).await;
        git(dir, &["config", "user.email", "t@example.com"]).await;
        git(dir, &["config", "core.autocrlf", "false"]).await;
    }

    /// Local paths as submodule URLs need the file protocol allowed.
    fn allow_file(command: GitCommand) -> GitCommand {
        command
            .with_env("GIT_CONFIG_COUNT", "1")
            .with_env("GIT_CONFIG_KEY_0", "protocol.file.allow")
            .with_env("GIT_CONFIG_VALUE_0", "always")
    }

    #[tokio::test]
    async fn adds_inspects_and_updates_a_submodule() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("lib");
        let main = tmp.path().join("main");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(&main).unwrap();
        init(&lib).await;
        std::fs::write(lib.join("a.vhd"), "a\n").unwrap();
        git(&lib, &["add", "."]).await;
        git(&lib, &["commit", "-q", "-m", "first"]).await;
        init(&main).await;
        git(&main, &["commit", "-q", "--allow-empty", "-m", "init"]).await;

        let url = lib.to_string_lossy().to_string();
        let record = run(allow_file(add(&main, &url, "ip/lib"))).await;
        assert!(record.success(), "{}", record.stderr);
        git(&main, &["commit", "-q", "-m", "add lib"]).await;

        let config = run(config_command(&main)).await.stdout;
        let status = run(status_command(&main)).await.stdout;
        let mut list = parse(&config, &status);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, State::UpToDate);

        // The library moves on: the submodule is one commit behind.
        std::fs::write(lib.join("a.vhd"), "b\n").unwrap();
        git(&lib, &["commit", "-q", "-am", "second"]).await;
        let sub = main.join("ip/lib");
        git(&sub, &["fetch", "-q", "origin"]).await;
        inspect(&main, &mut list).await;
        assert_eq!(list[0].behind, Some(1));
        assert_eq!(list[0].pinned_subject.as_deref(), Some("first"));
        assert_eq!(list[0].latest.as_ref().unwrap().1, "second");
        assert!(!list[0].dirty);

        for command in update_to_server(&main, "ip/lib") {
            let record = run(allow_file(command)).await;
            assert!(record.success(), "{}", record.stderr);
        }
        let staged = git(&main, &["diff", "--cached", "--name-only"]).await;
        assert_eq!(staged, "ip/lib\n");
    }
}
