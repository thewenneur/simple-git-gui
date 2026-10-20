//! Workspaces (git worktrees): several branches checked out side by side,
//! each in its own folder, all sharing one repository.

use std::path::{Path, PathBuf};

use super::runner::{CommandRecord, GitCommand, run};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// The branch checked out (short name), `None` when detached.
    pub branch: Option<String>,
    /// The repository's own folder (listed first by git).
    pub main: bool,
    pub bare: bool,
    /// Locked, with the reason given (may be empty).
    pub locked: Option<String>,
    /// Git thinks the folder is gone (`prunable`), or it is missing.
    pub missing: bool,
    /// Files changed and not committed there (when the folder exists).
    pub changes: usize,
    /// The folder has submodules (a `.gitmodules` file).
    pub submodules: bool,
}

pub fn list_command(root: &Path) -> GitCommand {
    GitCommand::new(["worktree", "list", "--porcelain"])
        .in_dir(root)
        .background()
}

/// Parses `git worktree list --porcelain`: blocks of lines separated by an
/// empty line, the first one being the main worktree.
pub fn parse(output: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    for block in output.split("\n\n") {
        let mut entry: Option<Worktree> = None;
        for line in block.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            if key == "worktree" {
                entry = Some(Worktree {
                    path: PathBuf::from(value),
                    head: None,
                    branch: None,
                    main: list.is_empty(),
                    bare: false,
                    locked: None,
                    missing: false,
                    changes: 0,
                    submodules: false,
                });
                continue;
            }
            let Some(entry) = entry.as_mut() else {
                continue;
            };
            match key {
                "HEAD" => entry.head = Some(value.to_string()),
                "branch" => {
                    entry.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_string(),
                    )
                }
                "bare" => entry.bare = true,
                "locked" => entry.locked = Some(unquote(value)),
                "prunable" => entry.missing = true,
                _ => {}
            }
        }
        list.extend(entry);
    }
    list
}

/// Undoes git's C-style quoting (`"cl\303\251"` → `clé`), used for lock
/// reasons with unusual characters.
fn unquote(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match chars.next() {
            Some(d @ b'0'..=b'7') => {
                let mut n = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&d @ b'0'..=b'7') = chars.peek() {
                        n = n * 8 + u32::from(d - b'0');
                        chars.next();
                    }
                }
                bytes.push(n as u8);
            }
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(other) => bytes.push(other),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads the workspaces, with the uncommitted changes of each one.
pub async fn read(root: &Path) -> (Vec<Worktree>, Vec<CommandRecord>) {
    let listed = run(list_command(root)).await;
    let mut list = parse(&listed.stdout);
    let mut records = vec![listed];
    for tree in list.iter_mut().filter(|t| !t.bare) {
        if !tree.path.is_dir() {
            tree.missing = true;
            continue;
        }
        tree.submodules = tree.path.join(".gitmodules").is_file();
        let status = run(GitCommand::new(["status", "--porcelain"])
            .in_dir(&tree.path)
            .background())
        .await;
        tree.changes = status.stdout.lines().count();
        records.push(status);
    }
    (list, records)
}

/// Where a new workspace goes by default: next to the repository's folder,
/// named after it and the branch (`carte-fpga-uart-fix-baud`).
pub fn suggested_folder(main: &Path, branch: &str) -> PathBuf {
    let name = main
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let branch: String = branch
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .collect();
    let folder = format!("{name}-{}", branch.trim_matches('-'));
    main.parent().unwrap_or(main).join(folder)
}

/// What a new workspace starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// An existing local branch.
    Branch(String),
    /// A new branch created at a commit (or another branch).
    NewBranch { name: String, at: String },
    /// A commit, outside any branch.
    Detached(String),
}

pub fn add(root: &Path, folder: &Path, start: &Start) -> GitCommand {
    let folder = folder.to_string_lossy().into_owned();
    let args: Vec<String> = match start {
        Start::Branch(branch) => vec![
            "worktree".into(),
            "add".into(),
            "--".into(),
            folder,
            branch.clone(),
        ],
        Start::NewBranch { name, at } => vec![
            "worktree".into(),
            "add".into(),
            "-b".into(),
            name.clone(),
            "--".into(),
            folder,
            at.clone(),
        ],
        Start::Detached(commit) => vec![
            "worktree".into(),
            "add".into(),
            "--detach".into(),
            "--".into(),
            folder,
            commit.clone(),
        ],
    };
    GitCommand::new(args).in_dir(root)
}

/// Removes a workspace's folder (git refuses if it has changes, unless
/// `force`). Its branch stays.
pub fn remove(root: &Path, folder: &Path, force: bool) -> GitCommand {
    let mut args = vec!["worktree".to_string(), "remove".to_string()];
    if force {
        args.push("--force".into());
    }
    args.extend(["--".into(), folder.to_string_lossy().into_owned()]);
    GitCommand::new(args).in_dir(root)
}

/// Forgets the workspaces whose folder was deleted.
pub fn prune(root: &Path) -> GitCommand {
    GitCommand::new(["worktree", "prune", "--verbose"]).in_dir(root)
}

/// Keeps git from forgetting a workspace while its folder is unreachable
/// (e.g. on a removable drive).
pub fn lock(root: &Path, folder: &Path, reason: &str) -> GitCommand {
    let mut args = vec!["worktree".to_string(), "lock".to_string()];
    if !reason.is_empty() {
        args.extend(["--reason".into(), reason.to_string()]);
    }
    args.extend(["--".into(), folder.to_string_lossy().into_owned()]);
    GitCommand::new(args).in_dir(root)
}

pub fn unlock(root: &Path, folder: &Path) -> GitCommand {
    GitCommand::new([
        "worktree".to_string(),
        "unlock".to_string(),
        "--".to_string(),
        folder.to_string_lossy().into_owned(),
    ])
    .in_dir(root)
}

/// Downloads the submodules of a new workspace.
pub fn submodules(folder: &Path) -> GitCommand {
    GitCommand::new(["submodule", "update", "--init", "--recursive"])
        .in_dir(folder)
        .network()
}

/// The folder of the workspace that already has a branch, from git's refusal:
/// `fatal: 'x' is already checked out at '/path'` (older git) or
/// `fatal: 'x' is already used by worktree at '/path'`.
pub fn already_checked_out(stderr: &str) -> Option<PathBuf> {
    let at = [
        "is already checked out at '",
        "is already used by worktree at '",
    ]
    .iter()
    .find_map(|marker| stderr.split_once(marker).map(|(_, rest)| rest))?;
    let path = at.split_once('\'').map(|(p, _)| p)?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_list() {
        let output = "worktree /work/carte\nHEAD aaa\nbranch refs/heads/main\n\nworktree /work/carte-fix\nHEAD bbb\nbranch refs/heads/fix/baud\nlocked usb drive\n\nworktree /work/old\nHEAD ccc\ndetached\nprunable gitdir file points to non-existent location\n\n";
        let list = parse(output);
        assert_eq!(list.len(), 3);
        assert!(list[0].main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].branch.as_deref(), Some("fix/baud"));
        assert_eq!(list[1].locked.as_deref(), Some("usb drive"));
        assert!(!list[1].main);
        assert_eq!(list[2].branch, None);
        assert_eq!(
            parse("worktree /a\nHEAD a\nlocked \"sur la cl\\303\\251 \\\"USB\\\"\"\n")[0]
                .locked
                .as_deref(),
            Some("sur la clé \"USB\"")
        );
        assert!(list[2].missing);
    }

    #[test]
    fn suggests_folders_and_reads_refusals() {
        assert_eq!(
            suggested_folder(Path::new("/work/carte"), "fix/baud"),
            Path::new("/work/carte-fix-baud")
        );
        assert_eq!(
            already_checked_out("fatal: 'main' is already checked out at '/work/carte'\n"),
            Some(PathBuf::from("/work/carte"))
        );
        assert_eq!(
            already_checked_out("fatal: 'main' is already used by worktree at 'C:/w/carte'"),
            Some(PathBuf::from("C:/w/carte"))
        );
        assert_eq!(already_checked_out("fatal: other"), None);
    }

    async fn git(dir: &Path, args: &[&str]) -> String {
        let record = run(GitCommand::new(args.iter().copied()).in_dir(dir)).await;
        assert!(record.success(), "git {args:?}: {}", record.stderr);
        record.stdout
    }

    #[tokio::test]
    async fn adds_locks_and_removes_workspaces() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("carte");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]).await;
        git(&root, &["config", "user.name", "Test"]).await;
        git(&root, &["config", "user.email", "t@example.com"]).await;
        git(&root, &["commit", "-q", "--allow-empty", "-m", "init"]).await;
        git(&root, &["branch", "fix"]).await;

        let fix = suggested_folder(&root, "fix");
        assert!(
            run(add(&root, &fix, &Start::Branch("fix".into())))
                .await
                .success()
        );
        let new = tmp.path().join("new");
        let start = Start::NewBranch {
            name: "feature".into(),
            at: "main".into(),
        };
        assert!(run(add(&root, &new, &start)).await.success());

        // A branch can't be in two workspaces.
        let refused = run(add(
            &root,
            &tmp.path().join("again"),
            &Start::Branch("fix".into()),
        ))
        .await;
        assert!(!refused.success());
        let other = already_checked_out(&refused.stderr).expect("path in refusal");
        assert_eq!(other.file_name(), fix.file_name());

        std::fs::write(new.join("a.txt"), "a").unwrap();
        let (list, _) = read(&root).await;
        assert_eq!(list.len(), 3);
        let by = |b: &str| {
            list.iter()
                .find(|t| t.branch.as_deref() == Some(b))
                .unwrap()
                .clone()
        };
        assert_eq!(by("feature").changes, 1);

        assert!(run(lock(&root, &fix, "usb drive")).await.success());
        let (list, _) = read(&root).await;
        let fix_tree = list
            .iter()
            .find(|t| t.branch.as_deref() == Some("fix"))
            .unwrap();
        assert_eq!(fix_tree.locked.as_deref(), Some("usb drive"));
        assert!(run(unlock(&root, &fix)).await.success());

        // Changes stop a removal unless forced.
        assert!(!run(remove(&root, &new, false)).await.success());
        assert!(run(remove(&root, &new, true)).await.success());
        assert!(run(remove(&root, &fix, false)).await.success());
        std::fs::create_dir_all(tmp.path().join("gone")).unwrap();
        let (list, _) = read(&root).await;
        assert_eq!(list.len(), 1);
        assert!(run(prune(&root)).await.success());
    }
}
