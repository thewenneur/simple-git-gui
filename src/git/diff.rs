//! Parsing of `git diff` output (unified format) for one file, detection of
//! moved lines, and extraction of single hunks as patches for `git apply`.

use std::path::Path;

use super::runner::GitCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// Line number in the old version (context and removed lines).
    pub old: Option<u32>,
    /// Line number in the new version (context and added lines).
    pub new: Option<u32>,
    pub text: String,
    /// Same content added or removed elsewhere in the file: a move, not a
    /// real change. Holds the line number at the other end of the move.
    pub moved_from: Option<u32>,
    pub moved_to: Option<u32>,
}

impl Line {
    pub fn is_moved(&self) -> bool {
        self.moved_from.is_some() || self.moved_to.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What git found around the change (function, process, entity...).
    pub section: String,
    pub lines: Vec<Line>,
    /// The hunk exactly as git printed it, for `git apply`.
    raw: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    /// `diff --git`, `index`, `---` and `+++` lines, needed to apply a hunk.
    header: String,
    pub hunks: Vec<Hunk>,
    pub binary: bool,
}

impl FileDiff {
    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty() && !self.binary
    }

    pub fn stats(&self) -> DiffStats {
        let mut stats = DiffStats::default();
        for line in self.hunks.iter().flat_map(|h| &h.lines) {
            match (line.kind, line.is_moved()) {
                (LineKind::Added, true) => stats.moved += 1,
                (LineKind::Added, false) => stats.added += 1,
                (LineKind::Removed, false) => stats.removed += 1,
                _ => {}
            }
        }
        stats
    }

    /// A patch containing only hunk `index`, for `git apply`.
    pub fn hunk_patch(&self, index: usize) -> Option<String> {
        let hunk = self.hunks.get(index)?;
        Some(format!("{}{}", self.header, hunk.raw))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub added: u32,
    pub removed: u32,
    /// Lines moved (counted once, on the added side).
    pub moved: u32,
}

/// Options that change how a diff is computed or shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    pub ignore_whitespace: bool,
    pub detect_moves: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            ignore_whitespace: true,
            detect_moves: true,
        }
    }
}

/// `git diff` for one file: staged (`--cached`) or not yet staged.
pub fn diff_command(root: &Path, path: &str, staged: bool, options: DiffOptions) -> GitCommand {
    let mut args = vec!["diff", "--no-ext-diff", "--histogram", "--no-color"];
    if staged {
        args.push("--cached");
    }
    if options.ignore_whitespace {
        args.push("-w");
    }
    args.extend(["--", path]);
    GitCommand::new(args).in_dir(root)
}

/// Parses the diff of a single file.
pub fn parse(output: &str, detect_moves: bool) -> FileDiff {
    let mut diff = FileDiff::default();
    let mut current: Option<Hunk> = None;
    let (mut old_no, mut new_no) = (0u32, 0u32);

    for line in output.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);

        if let Some(rest) = content.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                diff.hunks.push(hunk);
            }
            let Some(hunk) = parse_hunk_header(rest) else {
                continue;
            };
            old_no = hunk.old_start;
            new_no = hunk.new_start;
            let mut hunk = hunk;
            hunk.raw.push_str(line);
            current = Some(hunk);
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            if content.starts_with("Binary files ") || content == "GIT binary patch" {
                diff.binary = true;
            }
            diff.header.push_str(line);
            continue;
        };
        hunk.raw.push_str(line);

        let (kind, text) = match content.chars().next() {
            Some(' ') => (LineKind::Context, &content[1..]),
            Some('+') => (LineKind::Added, &content[1..]),
            Some('-') => (LineKind::Removed, &content[1..]),
            // "\ No newline at end of file"
            Some('\\') => continue,
            None => (LineKind::Context, ""),
            _ => continue,
        };
        let (old, new) = match kind {
            LineKind::Context => {
                old_no += 1;
                new_no += 1;
                (Some(old_no - 1), Some(new_no - 1))
            }
            LineKind::Added => {
                new_no += 1;
                (None, Some(new_no - 1))
            }
            LineKind::Removed => {
                old_no += 1;
                (Some(old_no - 1), None)
            }
        };
        hunk.lines.push(Line {
            kind,
            old,
            new,
            text: text.to_string(),
            moved_from: None,
            moved_to: None,
        });
    }
    if let Some(hunk) = current {
        diff.hunks.push(hunk);
    }
    if detect_moves {
        mark_moves(&mut diff);
    }
    diff
}

/// `-12,4 +12,5 @@ architecture rtl of top`
fn parse_hunk_header(rest: &str) -> Option<Hunk> {
    let (ranges, section) = rest.split_once(" @@").unwrap_or((rest, ""));
    let mut parts = ranges.split_whitespace();
    let (old_start, old_len) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_len) = parse_range(parts.next()?.strip_prefix('+')?)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        section: section.trim().to_string(),
        lines: Vec::new(),
        raw: String::new(),
    })
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// Lines worth matching: blank lines and lone braces or `end;` move around
/// by themselves all the time and would create false moves.
fn significant(text: &str) -> bool {
    text.trim().chars().filter(|c| c.is_alphanumeric()).count() >= 4
}

/// Marks removed/added runs of identical lines (ignoring indentation) as a
/// move. A run must hold at least 2 significant lines, or 1 long line, so
/// that a common line like `begin` is not reported as moved.
fn mark_moves(diff: &mut FileDiff) {
    // Flat views of all removed and added lines, with their position.
    let mut removed: Vec<(usize, usize)> = Vec::new();
    let mut added: Vec<(usize, usize)> = Vec::new();
    for (h, hunk) in diff.hunks.iter().enumerate() {
        for (l, line) in hunk.lines.iter().enumerate() {
            match line.kind {
                LineKind::Removed => removed.push((h, l)),
                LineKind::Added => added.push((h, l)),
                LineKind::Context => {}
            }
        }
    }
    let text = |d: &FileDiff, (h, l): (usize, usize)| d.hunks[h].lines[l].text.trim().to_string();
    let adjacent = |d: &FileDiff, a: (usize, usize), b: (usize, usize)| {
        // Consecutive in the diff and same kind.
        a.0 == b.0 && a.1 + 1 == b.1 && d.hunks[a.0].lines[a.1].kind == d.hunks[b.0].lines[b.1].kind
    };

    let mut used_added = vec![false; added.len()];
    let mut i = 0;
    while i < removed.len() {
        let mut best: Option<(usize, usize)> = None; // (start in added, run length)
        for j in 0..added.len() {
            if used_added[j] || text(diff, removed[i]) != text(diff, added[j]) {
                continue;
            }
            let mut len = 1;
            while i + len < removed.len()
                && j + len < added.len()
                && !used_added[j + len]
                && adjacent(diff, removed[i + len - 1], removed[i + len])
                && adjacent(diff, added[j + len - 1], added[j + len])
                && text(diff, removed[i + len]) == text(diff, added[j + len])
            {
                len += 1;
            }
            if best.is_none_or(|(_, l)| len > l) {
                best = Some((j, len));
            }
        }
        let Some((j, len)) = best else {
            i += 1;
            continue;
        };
        let lines: Vec<String> = (0..len).map(|k| text(diff, removed[i + k])).collect();
        let worth_it = lines.iter().filter(|t| significant(t)).count() >= 2
            || lines.iter().any(|t| t.len() >= 24);
        if !worth_it {
            i += 1;
            continue;
        }
        for k in 0..len {
            let (rh, rl) = removed[i + k];
            let (ah, al) = added[j + k];
            let new_no = diff.hunks[ah].lines[al].new;
            let old_no = diff.hunks[rh].lines[rl].old;
            diff.hunks[rh].lines[rl].moved_to = new_no;
            diff.hunks[ah].lines[al].moved_from = old_no;
            used_added[j + k] = true;
        }
        i += len;
    }
}

/// Builds the diff of a new, untracked file from its content, all lines added.
pub fn untracked_file(content: &[u8]) -> FileDiff {
    if content.contains(&0) {
        return FileDiff {
            binary: true,
            ..FileDiff::default()
        };
    }
    let text = String::from_utf8_lossy(content);
    let lines: Vec<Line> = text
        .lines()
        .enumerate()
        .map(|(i, l)| Line {
            kind: LineKind::Added,
            old: None,
            new: Some(i as u32 + 1),
            text: l.to_string(),
            moved_from: None,
            moved_to: None,
        })
        .collect();
    let new_len = lines.len() as u32;
    FileDiff {
        header: String::new(),
        hunks: vec![Hunk {
            old_start: 0,
            old_len: 0,
            new_start: 1,
            new_len,
            section: String::new(),
            lines,
            raw: String::new(),
        }],
        binary: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "diff --git a/rtl/uart_rx.vhd b/rtl/uart_rx.vhd
index 1111111..2222222 100644
--- a/rtl/uart_rx.vhd
+++ b/rtl/uart_rx.vhd
@@ -18,4 +18,6 @@ architecture rtl of uart_rx is
 architecture rtl of uart_rx is
+  constant BAUD_DIV : integer := CLK_FREQ / BAUD;
+  constant HALF_BIT : integer := BAUD_DIV / 2;
   signal rx_sync  : std_logic_vector(1 downto 0);
-  signal bit_cnt  : integer range 0 to 9;
+  signal bit_cnt  : unsigned(3 downto 0);
 begin
@@ -40,4 +42,2 @@ begin
-  constant BAUD_DIV : integer := CLK_FREQ / BAUD;
-  constant HALF_BIT : integer := BAUD_DIV / 2;
 end architecture;
\\ No newline at end of file
";

    #[test]
    fn parses_hunks_and_line_numbers() {
        let diff = parse(SAMPLE, false);
        assert_eq!(diff.hunks.len(), 2);
        let h = &diff.hunks[0];
        assert_eq!(h.section, "architecture rtl of uart_rx is");
        assert_eq!((h.old_start, h.new_start), (18, 18));
        assert_eq!(h.lines.len(), 7);
        assert_eq!(h.lines[1].kind, LineKind::Added);
        assert_eq!(h.lines[1].new, Some(19));
        assert_eq!(h.lines[4].kind, LineKind::Removed);
        assert_eq!(h.lines[4].old, Some(20));
        assert_eq!(h.lines[5].new, Some(22));
        assert_eq!(diff.hunks[1].lines.len(), 3);
        assert!(!diff.binary);
        assert_eq!(
            diff.stats(),
            DiffStats {
                added: 3,
                removed: 3,
                moved: 0
            }
        );
    }

    #[test]
    fn detects_moved_block() {
        let diff = parse(SAMPLE, true);
        let added = &diff.hunks[0].lines[1];
        assert_eq!(added.moved_from, Some(40));
        let removed = &diff.hunks[1].lines[1];
        assert_eq!(removed.moved_to, Some(20));
        // The real change is not a move.
        assert!(!diff.hunks[0].lines[4].is_moved());
        assert_eq!(
            diff.stats(),
            DiffStats {
                added: 1,
                removed: 1,
                moved: 2
            }
        );
    }

    #[test]
    fn short_common_lines_are_not_moves() {
        let out = "@@ -1,3 +1,3 @@
-end if;
 x <= y;
+end if;
";
        let diff = parse(out, true);
        assert!(diff.hunks[0].lines.iter().all(|l| !l.is_moved()));
    }

    #[test]
    fn extracts_one_hunk_as_patch() {
        let diff = parse(SAMPLE, true);
        let patch = diff.hunk_patch(1).unwrap();
        assert!(patch.starts_with("diff --git a/rtl/uart_rx.vhd"));
        assert!(patch.contains("+++ b/rtl/uart_rx.vhd\n@@ -40,4 +42,2 @@ begin\n"));
        assert!(!patch.contains("@@ -18,4"));
        assert!(patch.ends_with("\\ No newline at end of file\n"));
    }

    #[test]
    fn detects_binary() {
        let out = "diff --git a/logo.png b/logo.png\nindex 1..2 100644\nBinary files a/logo.png and b/logo.png differ\n";
        assert!(parse(out, true).binary);
        assert!(untracked_file(b"PNG\0\x01").binary);
    }

    #[test]
    fn untracked_file_is_all_added() {
        let diff = untracked_file(b"-- tb\nentity tb is\nend;\n");
        assert_eq!(diff.hunks[0].lines.len(), 3);
        assert_eq!(diff.stats().added, 3);
        assert!(diff.hunk_patch(0).is_some());
    }
}
