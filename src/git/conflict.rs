//! Conflicts in a file, as git writes them with `merge.conflictStyle=zdiff3`,
//! and the blocks found twice after a merge (a block moved on one side and
//! kept in place on the other).

/// A part of a conflicted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// Lines both sides agree on (each with its line ending).
    Text(String),
    Conflict(Conflict),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The version of the branch you are on (git's "ours").
    pub current: String,
    /// The version before both changes, when git wrote it.
    pub base: Option<String>,
    /// The version being brought in (git's "theirs").
    pub incoming: String,
}

/// How a conflict is settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Current,
    Incoming,
    /// The current version, then the incoming one.
    Both,
    /// Text written by the user.
    Custom(String),
}

impl Conflict {
    pub fn resolve(&self, choice: &Choice) -> String {
        match choice {
            Choice::Current => self.current.clone(),
            Choice::Incoming => self.incoming.clone(),
            Choice::Both => {
                let mut both = self.current.clone();
                if !both.is_empty() && !both.ends_with('\n') {
                    both.push('\n');
                }
                both.push_str(&self.incoming);
                both
            }
            Choice::Custom(text) => {
                let mut text = text.clone();
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text
            }
        }
    }
}

/// Splits a file with conflict markers into parts. Returns `None` when the
/// markers are not well formed (the user can still edit the file by hand).
pub fn parse(content: &str) -> Option<Vec<Part>> {
    #[derive(PartialEq)]
    enum In {
        Text,
        Current,
        Base,
        Incoming,
    }
    let mut parts = Vec::new();
    let mut state = In::Text;
    let mut text = String::new();
    let mut current = String::new();
    let mut base: Option<String> = None;
    let mut incoming = String::new();

    for line in content.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let marker =
            |m: &str| bare.starts_with(m) && (bare.len() == 7 || bare[7..].starts_with(' '));
        match state {
            In::Text if marker("<<<<<<<") => {
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                state = In::Current;
            }
            In::Text => text.push_str(line),
            In::Current if marker("|||||||") => {
                base = Some(String::new());
                state = In::Base;
            }
            In::Current | In::Base if marker("=======") && bare.len() == 7 => state = In::Incoming,
            In::Current => current.push_str(line),
            In::Base => base.get_or_insert_with(String::new).push_str(line),
            In::Incoming if marker(">>>>>>>") => {
                parts.push(Part::Conflict(Conflict {
                    current: std::mem::take(&mut current),
                    base: base.take(),
                    incoming: std::mem::take(&mut incoming),
                }));
                state = In::Text;
            }
            In::Incoming => incoming.push_str(line),
        }
    }
    if state != In::Text {
        return None;
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Some(parts)
}

pub fn conflict_count(parts: &[Part]) -> usize {
    parts
        .iter()
        .filter(|p| matches!(p, Part::Conflict(_)))
        .count()
}

/// The file once every conflict is settled; `None` if one is not.
pub fn resolve(parts: &[Part], choices: &[Option<Choice>]) -> Option<String> {
    let mut out = String::new();
    let mut index = 0;
    for part in parts {
        match part {
            Part::Text(t) => out.push_str(t),
            Part::Conflict(c) => {
                out.push_str(&c.resolve(choices.get(index)?.as_ref()?));
                index += 1;
            }
        }
    }
    Some(out)
}

/// A block of lines found twice in the merged file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    /// The lines of the block, as in the file.
    pub lines: Vec<String>,
    /// First line (0-based) of each copy in the merged file.
    pub first: usize,
    pub second: usize,
    /// The copy at the block's old place, the one to remove.
    pub stale: usize,
}

/// Blocks of at least `MIN_LINES` meaningful lines that appear twice in
/// `merged` while each side has them once: one side moved the block, the
/// other kept it where it was, and the merge kept both copies.
pub fn duplicated_blocks(
    base: &str,
    current: &str,
    incoming: &str,
    merged: &str,
) -> Vec<Duplicate> {
    const MIN_LINES: usize = 2;
    let lines: Vec<&str> = merged.lines().collect();
    let meaningful = |l: &str| {
        let t = l.trim();
        t.len() >= 3 && !matches!(t, "begin" | "end;" | "end" | "{" | "}" | "else" | "then")
    };
    let count = |hay: &str, block: &[&str]| -> usize {
        let hay: Vec<&str> = hay.lines().collect();
        if hay.len() < block.len() {
            return 0;
        }
        (0..=hay.len() - block.len())
            .filter(|&i| {
                hay[i..i + block.len()]
                    .iter()
                    .map(|l| l.trim())
                    .eq(block.iter().map(|l| l.trim()))
            })
            .count()
    };
    let mut found: Vec<Duplicate> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !meaningful(lines[i]) {
            i += 1;
            continue;
        }
        // Longest block starting at i that appears again later.
        let mut best: Option<(usize, usize)> = None; // (second start, length)
        for j in i + 1..lines.len() {
            let mut len = 0;
            while j + len < lines.len()
                && i + len < j
                && lines[i + len].trim() == lines[j + len].trim()
                && !lines[i + len].trim().is_empty()
            {
                len += 1;
            }
            let strong = lines[i..i + len].iter().filter(|l| meaningful(l)).count();
            if strong >= MIN_LINES && best.is_none_or(|(_, l)| len > l) {
                best = Some((j, len));
            }
        }
        let Some((j, len)) = best else {
            i += 1;
            continue;
        };
        let block = &lines[i..i + len];
        if count(current, block) == 1 && count(incoming, block) == 1 && count(merged, block) == 2 {
            // The stale copy is the one whose previous line is the same as
            // before the block in the base version.
            let before_in_base = {
                let hay: Vec<&str> = base.lines().collect();
                (0..hay.len().saturating_sub(len - 1))
                    .find(|&k| {
                        hay[k..k + len]
                            .iter()
                            .map(|l| l.trim())
                            .eq(block.iter().map(|l| l.trim()))
                    })
                    .map(|k| {
                        if k == 0 {
                            None
                        } else {
                            Some(hay[k - 1].trim())
                        }
                    })
            };
            let previous = |start: usize| (start > 0).then(|| lines[start - 1].trim());
            let stale = match before_in_base {
                Some(context) if previous(j) == context && previous(i) != context => j,
                _ => i,
            };
            found.push(Duplicate {
                lines: block.iter().map(|l| l.to_string()).collect(),
                first: i,
                second: j,
                stale,
            });
            i += len;
        } else {
            i += 1;
        }
    }
    found
}

/// `merged` without the stale copy of `duplicate`.
pub fn remove_copy(merged: &str, duplicate: &Duplicate) -> String {
    let mut out = String::new();
    let range = duplicate.stale..duplicate.stale + duplicate.lines.len();
    for (n, line) in merged.split_inclusive('\n').enumerate() {
        if !range.contains(&n) {
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
entity a is
<<<<<<< HEAD
  cnt <= HALF_BIT - 1;
||||||| base
  cnt <= 0;
=======
  cnt <= to_unsigned(HALF_BIT, 16);
>>>>>>> feature/uart-rx
end;
";

    #[test]
    fn parses_zdiff3_markers() {
        let parts = parse(FILE).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(conflict_count(&parts), 1);
        let Part::Conflict(c) = &parts[1] else {
            panic!("expected a conflict")
        };
        assert_eq!(c.current, "  cnt <= HALF_BIT - 1;\n");
        assert_eq!(c.base.as_deref(), Some("  cnt <= 0;\n"));
        assert_eq!(c.incoming, "  cnt <= to_unsigned(HALF_BIT, 16);\n");
    }

    #[test]
    fn resolves_with_each_choice() {
        let parts = parse(FILE).unwrap();
        assert_eq!(resolve(&parts, &[None]), None);
        let both = resolve(&parts, &[Some(Choice::Both)]).unwrap();
        assert_eq!(
            both,
            "entity a is\n  cnt <= HALF_BIT - 1;\n  cnt <= to_unsigned(HALF_BIT, 16);\nend;\n"
        );
        let custom = resolve(&parts, &[Some(Choice::Custom("  x".into()))]).unwrap();
        assert_eq!(custom, "entity a is\n  x\nend;\n");
    }

    #[test]
    fn rejects_broken_markers() {
        assert!(parse("<<<<<<< HEAD\na\n").is_none());
        // A line of '=' outside a conflict is ordinary text.
        assert_eq!(parse("=======\n").unwrap().len(), 1);
    }

    #[test]
    fn finds_a_block_moved_on_one_side() {
        let base =
            "a1;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nb1;\nb2;\nc1;\n";
        // Current moved the constants down; incoming edited c1 and kept them.
        let current =
            "a1;\nb1;\nb2;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nc1;\n";
        let incoming =
            "a1;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nb1;\nb2;\nc2;\n";
        let merged = "a1;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nb1;\nb2;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nc2;\n";
        let found = duplicated_blocks(base, current, incoming, merged);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].first, 1);
        assert_eq!(found[0].second, 5);
        // The copy at the old place (after a1) is the stale one.
        assert_eq!(found[0].stale, 1);
        assert_eq!(
            remove_copy(merged, &found[0]),
            "a1;\nb1;\nb2;\nconstant BAUD : integer := 10;\nconstant HALF : integer := 5;\nc2;\n"
        );
    }

    #[test]
    fn real_repetitions_are_not_duplicates() {
        let text = "x <= a;\ny <= b;\nz;\nx <= a;\ny <= b;\n";
        // The same lines were already twice on each side.
        assert!(duplicated_blocks(text, text, text, text).is_empty());
    }
}
