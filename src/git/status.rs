//! Parsing of `git status --porcelain=v2 --branch -z`.
//!
//! The porcelain v2 format is documented in `git help status` and is stable
//! across git versions, unlike the human-readable output.

/// What happened to a file on one side (index or working tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Unmodified,
    Modified,
    TypeChanged,
    Added,
    Deleted,
    Renamed,
    Copied,
    /// Conflict marker letter `U` (only in unmerged entries).
    Unmerged,
}

impl Change {
    fn from_letter(c: char) -> Self {
        match c {
            'M' => Change::Modified,
            'T' => Change::TypeChanged,
            'A' => Change::Added,
            'D' => Change::Deleted,
            'R' => Change::Renamed,
            'C' => Change::Copied,
            'U' => Change::Unmerged,
            _ => Change::Unmodified,
        }
    }
}

/// Which kind of line of the porcelain output an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Tracked,
    Untracked,
    Ignored,
    /// The file is in conflict (merge, rebase, cherry-pick...).
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    /// Previous path for renames and copies.
    pub orig_path: Option<String>,
    pub kind: EntryKind,
    /// The two status letters, kept so the UI can show the git notation.
    pub xy: String,
    /// Change between HEAD and the index (what is staged).
    pub staged: Change,
    /// Change between the index and the working tree (not staged yet).
    pub unstaged: Change,
    pub is_submodule: bool,
}

impl FileEntry {
    /// A plain-language description of the file's state.
    pub fn describe(&self) -> &'static str {
        match self.kind {
            EntryKind::Untracked => "Nouveau fichier (pas encore suivi)",
            EntryKind::Ignored => "Ignoré",
            EntryKind::Conflicted => match self.xy.as_str() {
                "DD" => "Conflit : supprimé des deux côtés",
                "AU" | "UA" => "Conflit : ajouté d'un seul côté",
                "DU" | "UD" => "Conflit : modifié d'un côté, supprimé de l'autre",
                "AA" => "Conflit : ajouté des deux côtés",
                _ => "Conflit : modifié des deux côtés",
            },
            EntryKind::Tracked => {
                // Prefer the most significant of the two sides.
                let change = if self.unstaged != Change::Unmodified {
                    self.unstaged
                } else {
                    self.staged
                };
                match change {
                    Change::Added => "Nouveau fichier",
                    Change::Deleted => "Supprimé",
                    Change::Renamed => "Renommé",
                    Change::Copied => "Copié",
                    Change::TypeChanged => "Type changé",
                    _ if self.is_submodule => "Sous-module modifié",
                    _ => "Modifié",
                }
            }
        }
    }

    /// Whether some of the change is already staged (`git add` done).
    pub fn has_staged(&self) -> bool {
        self.kind == EntryKind::Tracked && self.staged != Change::Unmodified
    }

    /// Whether some of the change is not staged yet.
    pub fn has_unstaged(&self) -> bool {
        match self.kind {
            EntryKind::Tracked => self.unstaged != Change::Unmodified,
            EntryKind::Untracked | EntryKind::Conflicted => true,
            EntryKind::Ignored => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// Current commit, `None` before the first commit.
    pub oid: Option<String>,
    /// Current branch, `None` when HEAD is detached.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: BranchInfo,
    pub entries: Vec<FileEntry>,
}

impl Status {
    pub fn conflicts(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Conflicted)
    }
}

/// The arguments that produce the output [`parse`] expects.
pub const STATUS_ARGS: &[&str] = &["status", "--porcelain=v2", "--branch", "-z"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sortie de git status inattendue : {}", self.0)
    }
}

/// Parses NUL-separated porcelain v2 output.
pub fn parse(output: &str) -> Result<Status, ParseError> {
    let mut status = Status::default();
    let mut records = output.split('\0').filter(|r| !r.is_empty());

    while let Some(record) = records.next() {
        let bad = || ParseError(record.to_string());
        let (tag, rest) = record.split_once(' ').ok_or_else(bad)?;
        match tag {
            "#" => parse_header(rest, &mut status.branch),
            "1" => {
                // XY sub mH mI mW hH hI path
                let fields: Vec<&str> = rest.splitn(8, ' ').collect();
                if fields.len() != 8 {
                    return Err(bad());
                }
                status
                    .entries
                    .push(tracked(fields[0], fields[1], fields[7], None, bad)?);
            }
            "2" => {
                // XY sub mH mI mW hH hI Xscore path, then origPath as the next record.
                let fields: Vec<&str> = rest.splitn(9, ' ').collect();
                if fields.len() != 9 {
                    return Err(bad());
                }
                let orig = records.next().ok_or_else(bad)?;
                status.entries.push(tracked(
                    fields[0],
                    fields[1],
                    fields[8],
                    Some(orig.to_string()),
                    bad,
                )?);
            }
            "u" => {
                // XY sub m1 m2 m3 mW h1 h2 h3 path
                let fields: Vec<&str> = rest.splitn(10, ' ').collect();
                if fields.len() != 10 {
                    return Err(bad());
                }
                let mut entry = tracked(fields[0], fields[1], fields[9], None, bad)?;
                entry.kind = EntryKind::Conflicted;
                status.entries.push(entry);
            }
            "?" | "!" => status.entries.push(FileEntry {
                path: rest.to_string(),
                orig_path: None,
                kind: if tag == "?" {
                    EntryKind::Untracked
                } else {
                    EntryKind::Ignored
                },
                xy: format!("{tag}{tag}"),
                staged: Change::Unmodified,
                unstaged: Change::Unmodified,
                is_submodule: false,
            }),
            _ => return Err(bad()),
        }
    }
    Ok(status)
}

fn parse_header(rest: &str, branch: &mut BranchInfo) {
    let Some((key, value)) = rest.split_once(' ') else {
        return;
    };
    match key {
        "branch.oid" if value != "(initial)" => branch.oid = Some(value.to_string()),
        "branch.head" if value != "(detached)" => branch.head = Some(value.to_string()),
        "branch.upstream" => branch.upstream = Some(value.to_string()),
        "branch.ab" => {
            for part in value.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    branch.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    branch.behind = n.parse().unwrap_or(0);
                }
            }
        }
        _ => {}
    }
}

fn tracked(
    xy: &str,
    sub: &str,
    path: &str,
    orig_path: Option<String>,
    bad: impl Fn() -> ParseError,
) -> Result<FileEntry, ParseError> {
    let mut letters = xy.chars();
    let (x, y) = match (letters.next(), letters.next()) {
        (Some(x), Some(y)) => (x, y),
        _ => return Err(bad()),
    };
    Ok(FileEntry {
        path: path.to_string(),
        orig_path,
        kind: EntryKind::Tracked,
        xy: xy.to_string(),
        staged: Change::from_letter(x),
        unstaged: Change::from_letter(y),
        is_submodule: sub.starts_with('S'),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_headers() {
        let out = "# branch.oid 1234abcd\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\0";
        let status = parse(out).unwrap();
        assert_eq!(
            status.branch,
            BranchInfo {
                oid: Some("1234abcd".into()),
                head: Some("main".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 3,
            }
        );
        assert!(status.entries.is_empty());
    }

    #[test]
    fn parses_initial_and_detached() {
        let status = parse("# branch.oid (initial)\0# branch.head (detached)\0").unwrap();
        assert_eq!(status.branch.oid, None);
        assert_eq!(status.branch.head, None);
    }

    #[test]
    fn parses_entries() {
        let out = concat!(
            "1 .M N... 100644 100644 100644 aaa bbb src/top level.vhd\0",
            "1 A. N... 000000 100644 100644 000 ccc new.vhd\0",
            "2 R. N... 100644 100644 100644 ddd ddd R100 renamed.vhd\0old.vhd\0",
            "1 .M SC.. 160000 160000 160000 eee eee libs/ip\0",
            "u UU N... 100644 100644 100644 100644 f1 f2 f3 conflict.vhd\0",
            "? notes.txt\0",
        );
        let status = parse(out).unwrap();
        let e = &status.entries;
        assert_eq!(e.len(), 6);

        assert_eq!(e[0].path, "src/top level.vhd");
        assert_eq!(e[0].unstaged, Change::Modified);
        assert!(e[0].has_unstaged() && !e[0].has_staged());

        assert_eq!(e[1].staged, Change::Added);
        assert_eq!(e[1].describe(), "Nouveau fichier");

        assert_eq!(e[2].path, "renamed.vhd");
        assert_eq!(e[2].orig_path.as_deref(), Some("old.vhd"));
        assert_eq!(e[2].describe(), "Renommé");

        assert!(e[3].is_submodule);
        assert_eq!(e[3].describe(), "Sous-module modifié");

        assert_eq!(e[4].kind, EntryKind::Conflicted);
        assert_eq!(e[4].describe(), "Conflit : modifié des deux côtés");
        assert_eq!(status.conflicts().count(), 1);

        assert_eq!(e[5].kind, EntryKind::Untracked);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("hello world\0").is_err());
    }
}
