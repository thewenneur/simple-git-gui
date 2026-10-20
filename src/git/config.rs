//! Reading and writing the git settings shown in the "Git configuration"
//! screen.

use std::collections::HashMap;
use std::path::Path;

use super::runner::GitCommand;

/// Where a setting is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Scope {
    /// This repository only (`.git/config`).
    #[default]
    Local,
    /// Every repository of the user (`~/.gitconfig`).
    Global,
}

impl Scope {
    fn flag(self) -> &'static str {
        match self {
            Scope::Local => "--local",
            Scope::Global => "--global",
        }
    }
}

/// Where the effective value of a setting comes from, as git reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Local,
    Global,
    System,
    /// Worktree, command line…
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Bool,
    /// The values offered, each with the translation key of its label.
    Choice(&'static [(&'static str, &'static str)]),
}

/// A setting of the screen. Titles and hints are translation keys derived
/// from `id`: `config.<id>` and `config.<id>_hint`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    pub key: &'static str,
    pub id: &'static str,
    pub kind: Kind,
    /// The value advised by Simple Git, if any.
    pub recommended: Option<&'static str>,
}

/// The settings grouped in sections (section id, settings).
pub const SECTIONS: &[(&str, &[Setting])] = &[
    (
        "identity",
        &[
            Setting {
                key: "user.name",
                id: "name",
                kind: Kind::Text,
                recommended: None,
            },
            Setting {
                key: "user.email",
                id: "email",
                kind: Kind::Text,
                recommended: None,
            },
        ],
    ),
    (
        "sync",
        &[Setting {
            key: "pull.rebase",
            id: "pull",
            kind: Kind::Choice(&[
                ("true", "config.pull_rebase"),
                ("false", "config.pull_merge"),
            ]),
            recommended: Some("true"),
        }],
    ),
    (
        "conflicts",
        &[
            Setting {
                key: "rerere.enabled",
                id: "rerere",
                kind: Kind::Bool,
                recommended: Some("true"),
            },
            Setting {
                key: "merge.conflictStyle",
                id: "conflict_style",
                kind: Kind::Choice(&[
                    ("zdiff3", "config.style_zdiff3"),
                    ("diff3", "config.style_diff3"),
                    ("merge", "config.style_merge"),
                ]),
                recommended: Some("zdiff3"),
            },
            Setting {
                key: "diff.algorithm",
                id: "algorithm",
                kind: Kind::Choice(&[
                    ("histogram", "config.algo_histogram"),
                    ("patience", "config.algo_patience"),
                    ("myers", "config.algo_myers"),
                ]),
                recommended: Some("histogram"),
            },
        ],
    ),
    (
        "files",
        &[Setting {
            key: "core.autocrlf",
            id: "autocrlf",
            kind: Kind::Choice(&[
                ("input", "config.crlf_input"),
                ("true", "config.crlf_true"),
                ("false", "config.crlf_false"),
            ]),
            recommended: None,
        }],
    ),
];

/// Every value set, per lowercase key, from the lowest priority (system) to
/// the highest (local).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values(HashMap<String, Vec<(Origin, String)>>);

impl Values {
    /// The value git uses, and where it comes from.
    pub fn effective(&self, key: &str) -> Option<(Origin, &str)> {
        self.0
            .get(&key.to_lowercase())?
            .last()
            .map(|(o, v)| (*o, v.as_str()))
    }

    /// The value set in `scope`, if any.
    pub fn in_scope(&self, key: &str, scope: Scope) -> Option<&str> {
        let wanted = match scope {
            Scope::Local => Origin::Local,
            Scope::Global => Origin::Global,
        };
        self.0
            .get(&key.to_lowercase())?
            .iter()
            .rev()
            .find(|(o, _)| *o == wanted)
            .map(|(_, v)| v.as_str())
    }
}

/// Lists every setting with its scope.
pub fn list_command(root: Option<&Path>) -> GitCommand {
    let command = GitCommand::new(["config", "--list", "--show-scope", "-z"]);
    match root {
        Some(root) => command.in_dir(root),
        None => command,
    }
}

/// Parses `git config --list --show-scope -z`: `scope\0key\nvalue\0`.
pub fn parse(output: &str) -> Values {
    let mut values: HashMap<String, Vec<(Origin, String)>> = HashMap::new();
    let mut tokens = output.split('\0');
    while let (Some(scope), Some(entry)) = (tokens.next(), tokens.next()) {
        let origin = match scope {
            "local" => Origin::Local,
            "global" => Origin::Global,
            "system" => Origin::System,
            _ => Origin::Other,
        };
        let (key, value) = entry.split_once('\n').unwrap_or((entry, "true"));
        values
            .entry(key.to_lowercase())
            .or_default()
            .push((origin, value.to_string()));
    }
    Values(values)
}

/// Sets `key` in `scope`; `None` removes it from that scope.
pub fn set_command(root: &Path, scope: Scope, key: &str, value: Option<&str>) -> GitCommand {
    let args = match value {
        Some(value) => vec!["config", scope.flag(), key, value],
        None => vec!["config", scope.flag(), "--unset-all", key],
    };
    GitCommand::new(args).in_dir(root)
}

/// Whether a set or unset failed only because the key was already unset
/// (`git config --unset` exits with 5 then).
pub fn nothing_to_unset(code: Option<i32>) -> bool {
    code == Some(5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::runner::run;

    #[test]
    fn parses_scopes_and_priority() {
        let values = parse(
            "system\0core.autocrlf\ntrue\0global\0user.name\nThaumas\0local\0user.name\nLéa\0local\0core.bare\nfalse\0local\0flag\0",
        );
        assert_eq!(values.effective("user.name"), Some((Origin::Local, "Léa")));
        assert_eq!(values.in_scope("user.name", Scope::Global), Some("Thaumas"));
        assert_eq!(
            values.effective("core.autocrlf"),
            Some((Origin::System, "true"))
        );
        assert_eq!(values.effective("Flag"), Some((Origin::Local, "true")));
        assert_eq!(values.effective("user.email"), None);
    }

    #[test]
    fn every_setting_has_texts() {
        // The checkout may use CRLF line endings (Windows).
        let locales = include_str!("../../locales/app.yml").replace("\r\n", "\n");
        for (section, settings) in SECTIONS {
            assert!(locales.contains(&format!("\nconfig.section_{section}:\n")));
            for s in *settings {
                for key in [format!("config.{}", s.id), format!("config.{}_hint", s.id)] {
                    assert!(locales.contains(&format!("\n{key}:\n")), "missing {key}");
                }
                if let Kind::Choice(choices) = s.kind {
                    for (_, label) in choices {
                        assert!(
                            locales.contains(&format!("\n{label}:\n")),
                            "missing {label}"
                        );
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn sets_reads_and_unsets_in_the_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert!(
            run(GitCommand::new(["init", "-q"]).in_dir(root))
                .await
                .success()
        );
        let set = run(set_command(root, Scope::Local, "pull.rebase", Some("true"))).await;
        assert!(set.success(), "{}", set.stderr);
        let list = run(list_command(Some(root))).await;
        let values = parse(&list.stdout);
        assert_eq!(values.in_scope("pull.rebase", Scope::Local), Some("true"));
        assert!(
            run(set_command(root, Scope::Local, "pull.rebase", None))
                .await
                .success()
        );
        let again = run(set_command(root, Scope::Local, "pull.rebase", None)).await;
        let code = match again.outcome {
            crate::git::runner::Outcome::Exited(c) => Some(c),
            _ => None,
        };
        assert!(nothing_to_unset(code));
    }
}
