//! Adding patterns to `.gitignore` (shared with the team) or to
//! `.git/info/exclude` (this computer only).

use std::path::{Path, PathBuf};

/// Where an ignore rule is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IgnoreTarget {
    /// `.gitignore` at the root: committed, so everyone gets the rule.
    #[default]
    Shared,
    /// `.git/info/exclude`: only this clone.
    Local,
}

impl IgnoreTarget {
    pub fn file(self, root: &Path, git_dir: &Path) -> PathBuf {
        match self {
            IgnoreTarget::Shared => root.join(".gitignore"),
            IgnoreTarget::Local => git_dir.join("info").join("exclude"),
        }
    }

    /// The file as shown to the user, relative to the repository.
    pub fn display(self) -> &'static str {
        match self {
            IgnoreTarget::Shared => ".gitignore",
            IgnoreTarget::Local => ".git/info/exclude",
        }
    }
}

/// What to ignore, starting from one file of the status list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IgnoreRule {
    /// This exact file.
    File(String),
    /// Every file with this extension, anywhere (`*.jou`).
    Extension(String),
    /// The folder containing the file.
    Folder(String),
}

impl IgnoreRule {
    /// The rules offered for `path` (a path relative to the root, `/`-separated).
    pub fn choices(path: &str) -> Vec<IgnoreRule> {
        let path = path.trim_end_matches('/');
        let mut rules = vec![IgnoreRule::File(path.to_string())];
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some((stem, ext)) = name.rsplit_once('.')
            && !stem.is_empty()
            && !ext.is_empty()
        {
            rules.push(IgnoreRule::Extension(ext.to_string()));
        }
        if let Some((dir, _)) = path.rsplit_once('/') {
            rules.push(IgnoreRule::Folder(dir.to_string()));
        }
        rules
    }

    /// The line written in the ignore file.
    pub fn pattern(&self) -> String {
        match self {
            // A leading `/` anchors the rule to the root: only this file.
            IgnoreRule::File(path) => format!("/{}", escape(path)),
            IgnoreRule::Extension(ext) => format!("*.{}", escape(ext)),
            IgnoreRule::Folder(dir) => format!("/{}/", escape(dir)),
        }
    }
}

/// Escapes the characters that have a meaning in ignore patterns.
fn escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for (i, c) in path.chars().enumerate() {
        let special = matches!(c, '*' | '?' | '[' | '\\') || (i == 0 && matches!(c, '#' | '!'));
        if special {
            out.push('\\');
        }
        out.push(c);
    }
    // Trailing spaces are dropped by git unless escaped.
    let trimmed = out.trim_end_matches(' ').len();
    if trimmed < out.len() {
        let spaces = out.len() - trimmed;
        out.truncate(trimmed);
        out.push_str(&"\\ ".repeat(spaces));
    }
    out
}

/// Appends `pattern` to `file` unless it is already there. Returns whether a
/// line was added.
pub fn append(file: &Path, pattern: &str) -> std::io::Result<bool> {
    let existing = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if existing.lines().any(|l| l.trim_end() == pattern) {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(pattern);
    content.push('\n');
    std::fs::write(file, content)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_file_extension_and_folder() {
        assert_eq!(
            IgnoreRule::choices("build/top.jou"),
            vec![
                IgnoreRule::File("build/top.jou".into()),
                IgnoreRule::Extension("jou".into()),
                IgnoreRule::Folder("build".into()),
            ]
        );
        assert_eq!(
            IgnoreRule::choices(".env"),
            vec![IgnoreRule::File(".env".into())]
        );
    }

    #[test]
    fn patterns_are_anchored_and_escaped() {
        assert_eq!(IgnoreRule::File("a/b.log".into()).pattern(), "/a/b.log");
        assert_eq!(IgnoreRule::Extension("jou".into()).pattern(), "*.jou");
        assert_eq!(IgnoreRule::Folder("ip/gen".into()).pattern(), "/ip/gen/");
        assert_eq!(IgnoreRule::File("x[1].v".into()).pattern(), "/x\\[1].v");
        assert_eq!(escape("#notes"), "\\#notes");
        assert_eq!(escape("a "), "a\\ ");
    }

    #[test]
    fn appends_once_with_newline() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("info").join("exclude");
        assert!(append(&file, "*.jou").unwrap());
        assert!(!append(&file, "*.jou").unwrap());
        std::fs::write(&file, "*.jou").unwrap();
        assert!(append(&file, "/build/").unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "*.jou\n/build/\n");
    }

    /// The written rules really make git ignore the files.
    #[tokio::test]
    async fn git_honours_the_rules() {
        use crate::git::runner::{GitCommand, run};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            run(GitCommand::new(["init", "-q"]).in_dir(root))
                .await
                .success()
        );
        std::fs::create_dir_all(root.join("build")).unwrap();
        std::fs::write(root.join("build/top.jou"), "").unwrap();
        std::fs::write(root.join("x[1].v"), "").unwrap();
        std::fs::write(root.join("keep.v"), "").unwrap();
        append(
            &root.join(".gitignore"),
            &IgnoreRule::File("x[1].v".into()).pattern(),
        )
        .unwrap();
        append(
            &IgnoreTarget::Local.file(root, &root.join(".git")),
            &IgnoreRule::Folder("build".into()).pattern(),
        )
        .unwrap();
        let status = run(GitCommand::new(["status", "--porcelain", "-uall"]).in_dir(root)).await;
        assert!(!status.stdout.contains("x[1].v"), "{}", status.stdout);
        assert!(!status.stdout.contains("build/"), "{}", status.stdout);
        assert!(status.stdout.contains("keep.v"));
    }
}
