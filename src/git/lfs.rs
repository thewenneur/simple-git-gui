//! Git LFS: large binary files (schematics, layouts, PDF, bitstreams) kept
//! outside the history, and the locks that stop two people editing the same
//! file, since binaries cannot be merged.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::runner::GitCommand;

/// One lock, as the server reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub id: String,
    pub path: String,
    pub owner: String,
    /// Seconds since the Unix epoch, when the server gave a valid date.
    pub locked_at: Option<i64>,
    /// Taken by this user (only known when the server can verify it).
    pub ours: Option<bool>,
}

pub fn version_command(root: &Path) -> GitCommand {
    GitCommand::new(["lfs", "version"])
        .in_dir(root)
        .background()
}

/// Lists the patterns stored in LFS (read from `.gitattributes`).
pub fn tracked_command(root: &Path) -> GitCommand {
    GitCommand::new(["lfs", "track"]).in_dir(root).background()
}

/// The files currently stored in LFS, by name.
pub fn files_command(root: &Path) -> GitCommand {
    GitCommand::new(["lfs", "ls-files", "--name-only"])
        .in_dir(root)
        .background()
}

/// The locks, telling apart ours from the others' (asks the server).
pub fn locks_command(root: &Path, verify: bool) -> GitCommand {
    let mut args = vec!["lfs", "locks", "--json"];
    if verify {
        args.push("--verify");
    }
    GitCommand::new(args).in_dir(root).network().background()
}

pub fn lock(root: &Path, path: &str) -> GitCommand {
    GitCommand::new(["lfs", "lock", "--", path])
        .in_dir(root)
        .network()
}

/// Releases a lock; `force` releases someone else's (needs the right to).
pub fn unlock(root: &Path, path: &str, force: bool) -> GitCommand {
    let mut args = vec!["lfs", "unlock"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);
    GitCommand::new(args).in_dir(root).network()
}

/// Stores every file matching `pattern` in LFS from now on (`*.pdf`), and
/// marks them lockable so they open read-only until locked.
pub fn track(root: &Path, pattern: &str) -> GitCommand {
    GitCommand::new(["lfs", "track", "--lockable", "--", pattern]).in_dir(root)
}

/// Installs the LFS hooks in this repository only.
pub fn install(root: &Path) -> GitCommand {
    GitCommand::new(["lfs", "install", "--local"]).in_dir(root)
}

/// The patterns in `git lfs track` output:
/// ```text
/// Listing tracked patterns
///     *.pdf [lockable] (.gitattributes)
/// Listing excluded patterns
/// ```
pub fn parse_tracked(output: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut tracked = false;
    for line in output.lines() {
        if !line.starts_with(' ') {
            tracked = line.contains("tracked");
            continue;
        }
        if !tracked {
            continue;
        }
        let line = line.trim();
        // The source file is the last parenthesised part.
        let pattern = match line.rfind(" (") {
            Some(i) => &line[..i],
            None => line,
        };
        let pattern = pattern.trim_end_matches(" [lockable]").trim();
        if !pattern.is_empty() && !patterns.iter().any(|p| p == pattern) {
            patterns.push(pattern.to_string());
        }
    }
    patterns
}

/// Reads `git lfs locks --json`: a list of locks, or with `--verify` an
/// object `{"ours": [...], "theirs": [...]}`.
pub fn parse_locks(output: &str) -> Result<Vec<Lock>, String> {
    let value: serde_json::Value =
        serde_json::from_str(output.trim()).map_err(|e| e.to_string())?;
    let read = |list: &serde_json::Value, ours: Option<bool>| -> Vec<Lock> {
        list.as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        let text = |key: &str| item.get(key).and_then(|v| v.as_str());
                        Some(Lock {
                            id: match item.get("id") {
                                Some(serde_json::Value::String(s)) => s.clone(),
                                Some(other) => other.to_string(),
                                None => String::new(),
                            },
                            path: text("path")?.to_string(),
                            owner: item
                                .get("owner")
                                .and_then(|o| o.get("name"))
                                .and_then(|n| n.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            locked_at: text("locked_at").and_then(parse_time),
                            ours,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    if value.is_array() {
        return Ok(read(&value, None));
    }
    let mut locks = read(&value["ours"], Some(true));
    locks.extend(read(&value["theirs"], Some(false)));
    Ok(locks)
}

/// Seconds since the epoch from an RFC 3339 date
/// (`2026-10-20T09:30:00Z`, `2026-10-20T11:30:00.5+02:00`).
pub fn parse_time(text: &str) -> Option<i64> {
    let (date, time) = text.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let (clock, offset) = match time.find(['Z', 'z', '+', '-']) {
        Some(i) => time.split_at(i),
        None => (time, ""),
    };
    let mut c = clock.split(':');
    let h: i64 = c.next()?.parse().ok()?;
    let min: i64 = c.next()?.parse().ok()?;
    let sec: f64 = c.next().unwrap_or("0").parse().ok()?;
    let offset = match offset.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let (oh, om) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
            let minutes = oh.parse::<i64>().ok()? * 60 + om.parse::<i64>().ok()?;
            if sign == '+' {
                minutes * 60
            } else {
                -minutes * 60
            }
        }
        _ => 0,
    };
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + sec as i64 - offset)
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// How long ago, in the largest whole unit: (count, unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Age {
    Minutes(i64),
    Hours(i64),
    Days(i64),
}

pub fn age(since: i64, now: i64) -> Age {
    let seconds = (now - since).max(0);
    match seconds {
        s if s < 3600 => Age::Minutes(s / 60),
        s if s < 86_400 => Age::Hours(s / 3600),
        s => Age::Days(s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tracked_patterns() {
        let output = "Listing tracked patterns\n    *.pdf [lockable] (.gitattributes)\n    *.fig (.gitattributes)\n    *.pdf (doc/.gitattributes)\nListing excluded patterns\n    *.txt (.gitattributes)\n";
        assert_eq!(parse_tracked(output), ["*.pdf", "*.fig"]);
        assert!(parse_tracked("Listing tracked patterns\nListing excluded patterns\n").is_empty());
    }

    #[test]
    fn parses_locks() {
        let plain = r#"[{"id":"3","path":"pcb/carte.kicad_pcb","owner":{"name":"Camille"},"locked_at":"2026-10-18T09:30:00Z"}]"#;
        let locks = parse_locks(plain).unwrap();
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].owner, "Camille");
        assert_eq!(locks[0].ours, None);
        assert_eq!(locks[0].locked_at, Some(1_792_315_800));

        let verified = r#"{"ours":[{"id":"1","path":"a.fig","owner":{"name":"Thaumas"},"locked_at":"2026-10-20T11:30:00+02:00"}],"theirs":[{"id":2,"path":"b.pdf","owner":{"name":"Camille"}}]}"#;
        let locks = parse_locks(verified).unwrap();
        assert_eq!(locks.len(), 2);
        assert_eq!(locks[0].ours, Some(true));
        assert_eq!(locks[0].locked_at, parse_time("2026-10-20T09:30:00Z"));
        assert_eq!(locks[1].id, "2");
        assert_eq!(locks[1].ours, Some(false));
        assert_eq!(locks[1].locked_at, None);
        assert!(parse_locks("Remote \"origin\" does not support the LFS locking API").is_err());
    }

    #[test]
    fn dates_and_ages() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2000-03-01T00:00:00.123Z"), Some(951_868_800));
        assert_eq!(age(0, 120), Age::Minutes(2));
        assert_eq!(age(0, 7200), Age::Hours(2));
        assert_eq!(age(0, 3 * 86_400 + 5), Age::Days(3));
    }

    #[tokio::test]
    async fn tracks_patterns() {
        use super::super::runner::run;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            run(GitCommand::new(["init", "-q"]).in_dir(root))
                .await
                .success()
        );
        if !run(version_command(root)).await.success() {
            return; // Git LFS is not installed on this machine.
        }
        let record = run(track(root, "*.kicad_pcb")).await;
        assert!(record.success(), "{}", record.stderr);
        let listed = run(tracked_command(root)).await;
        assert_eq!(parse_tracked(&listed.stdout), ["*.kicad_pcb"]);
        let attributes = std::fs::read_to_string(root.join(".gitattributes")).unwrap();
        assert!(attributes.contains("lockable"), "{attributes}");
    }
}
