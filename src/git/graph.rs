//! Layout of the history graph: which column each commit sits in and the
//! lines joining it to its parents, row by row.
//!
//! Each commit belongs to a branch (its "owner"), found by following first
//! parents from each branch tip, the main branch first. Lines take the color
//! of the branch of the commit they come from.

use std::collections::HashMap;

use super::repo::Commit;

/// Which half of a row a segment crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    /// From the top of the row to the commit's height.
    Top,
    /// From the commit's height to the bottom of the row.
    Bottom,
}

/// A line crossing half a row, from column `from` to column `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub half: Half,
    pub from: usize,
    pub to: usize,
    /// The branch the line belongs to, for its color.
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The column of the commit's dot.
    pub lane: usize,
    pub owner: Option<String>,
    pub segments: Vec<Segment>,
    /// How many columns this row uses.
    pub width: usize,
}

/// A branch name without `tag: ` and without the remote for remote branches
/// that also exist locally.
fn branch_refs(commit: &Commit) -> impl Iterator<Item = &str> {
    commit
        .refs
        .iter()
        .map(String::as_str)
        .filter(|r| !r.starts_with("tag: ") && *r != "HEAD" && !r.ends_with("/HEAD"))
}

fn is_main(name: &str) -> bool {
    let local = name.rsplit_once('/').map_or(name, |(_, n)| n);
    matches!(local, "main" | "master")
}

/// The branch each commit belongs to.
pub fn owners(commits: &[Commit]) -> HashMap<&str, &str> {
    let by_hash: HashMap<&str, &Commit> = commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    // Tips: main branches first, then local ones, then remote ones.
    let mut tips: Vec<(u8, &str, &str)> = Vec::new();
    for commit in commits {
        for name in branch_refs(commit) {
            let remote = name.contains('/') && !commit.refs.iter().any(|r| r == name);
            let rank = match (is_main(name), name.starts_with("origin/") || remote) {
                (true, false) => 0,
                (true, true) => 1,
                (false, false) => 2,
                (false, true) => 3,
            };
            tips.push((rank, name, commit.hash.as_str()));
        }
    }
    tips.sort_by_key(|(rank, _, _)| *rank);
    let mut owners: HashMap<&str, &str> = HashMap::new();
    for (_, name, tip) in tips {
        let mut current = Some(tip);
        while let Some(hash) = current {
            if owners.contains_key(hash) {
                break;
            }
            owners.insert(hash, name);
            current = by_hash
                .get(hash)
                .and_then(|c| c.parents.first())
                .map(String::as_str)
                .filter(|p| by_hash.contains_key(p));
        }
    }
    owners
}

/// Lays out `commits`, given newest first in topological order.
pub fn layout(commits: &[Commit]) -> Vec<Row> {
    let owners = owners(commits);
    // What each column is waiting for: a commit hash and the line's owner.
    let mut lanes: Vec<Option<(String, Option<String>)>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());

    for commit in commits {
        let owner = owners.get(commit.hash.as_str()).map(|s| s.to_string());
        let lane = lanes
            .iter()
            .position(|l| l.as_ref().is_some_and(|(h, _)| *h == commit.hash))
            .or_else(|| lanes.iter().position(Option::is_none))
            .unwrap_or(lanes.len());
        if lane == lanes.len() {
            lanes.push(None);
        }
        let before = lanes.clone();
        let mut segments = Vec::new();

        for (i, slot) in before.iter().enumerate() {
            if let Some((hash, line_owner)) = slot {
                let to = if *hash == commit.hash {
                    lanes[i] = None;
                    lane
                } else {
                    i
                };
                segments.push(Segment {
                    half: Half::Top,
                    from: i,
                    to,
                    owner: line_owner.clone(),
                });
            }
        }
        lanes[lane] = None;

        let mut parent_lanes = Vec::new();
        for (k, parent) in commit.parents.iter().enumerate() {
            let j = match lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|(h, _)| h == parent))
            {
                Some(j) => j,
                None => {
                    let j = if k == 0 && lanes[lane].is_none() {
                        lane
                    } else {
                        match lanes.iter().position(Option::is_none) {
                            Some(free) => free,
                            None => {
                                lanes.push(None);
                                lanes.len() - 1
                            }
                        }
                    };
                    lanes[j] = Some((parent.clone(), owner.clone()));
                    j
                }
            };
            parent_lanes.push(j);
        }

        // Lines going on below the row: the ones passing by, then the
        // commit's own lines to its parents.
        for (i, slot) in lanes.iter().enumerate() {
            if let Some(slot) = slot
                && before.get(i).and_then(Option::as_ref) == Some(slot)
            {
                segments.push(Segment {
                    half: Half::Bottom,
                    from: i,
                    to: i,
                    owner: slot.1.clone(),
                });
            }
        }
        for j in parent_lanes {
            segments.push(Segment {
                half: Half::Bottom,
                from: lane,
                to: j,
                owner: owner.clone(),
            });
        }

        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        let width = before.len().max(lanes.len()).max(lane + 1);
        rows.push(Row {
            lane,
            owner,
            segments,
            width,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str, parents: &[&str], refs: &[&str]) -> Commit {
        Commit {
            hash: hash.to_string(),
            short: hash.to_string(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            date: String::new(),
            refs: refs.iter().map(|r| r.to_string()).collect(),
            subject: String::new(),
        }
    }

    #[test]
    fn straight_history_stays_in_one_column() {
        let commits = [
            commit("c", &["b"], &["main"]),
            commit("b", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        let rows = layout(&commits);
        assert!(rows.iter().all(|r| r.lane == 0 && r.width == 1));
        assert_eq!(rows[0].segments.len(), 1); // only the line down to b
        assert_eq!(rows[2].segments.len(), 1); // only the line up from b
        assert!(rows.iter().all(|r| r.owner.as_deref() == Some("main")));
    }

    #[test]
    fn branch_and_merge_use_a_second_column() {
        // m merges f2 (feature) into main; feature started from a.
        let commits = [
            commit("m", &["b", "f2"], &["main"]),
            commit("f2", &["f1"], &["feature/x"]),
            commit("b", &["a"], &[]),
            commit("f1", &["a"], &[]),
            commit("a", &[], &[]),
        ];
        let rows = layout(&commits);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].lane, 1);
        assert_eq!(rows[4].lane, 0);
        // The merge sends a line to each parent.
        let down: Vec<_> = rows[0]
            .segments
            .iter()
            .filter(|s| s.half == Half::Bottom)
            .map(|s| s.to)
            .collect();
        assert_eq!(down, vec![0, 1]);
        // The feature's first commit curves back to its fork point.
        let down: Vec<_> = rows[3]
            .segments
            .iter()
            .filter(|s| s.half == Half::Bottom)
            .map(|s| (s.from, s.to))
            .collect();
        assert!(down.contains(&(1, 0)));
        // Commits of the feature belong to it, not to main.
        assert_eq!(rows[1].owner.as_deref(), Some("feature/x"));
        assert_eq!(rows[3].owner.as_deref(), Some("feature/x"));
        assert_eq!(rows[2].owner.as_deref(), Some("main"));
        assert_eq!(rows[3].width, 2);
        assert_eq!(rows[4].width, 1);
    }

    #[test]
    fn main_owns_shared_history_whatever_the_order() {
        let commits = [
            commit("f", &["a"], &["feature/x"]),
            commit("a", &[], &["main", "origin/main"]),
        ];
        let owners = owners(&commits);
        assert_eq!(owners["a"], "main");
        assert_eq!(owners["f"], "feature/x");
    }
}
