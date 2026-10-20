//! Interface language. Translations live in `locales/app.yml`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// Short label for the language switch.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Fr => "FR",
        }
    }

    /// French when the system is in French, English otherwise.
    pub fn from_system() -> Self {
        match sys_locale::get_locale() {
            Some(locale) if locale.to_lowercase().starts_with("fr") => Lang::Fr,
            _ => Lang::En,
        }
    }
}

pub fn set(lang: Lang) {
    rust_i18n::set_locale(lang.code());
}

/// Translates a key chosen at runtime (e.g. a file state).
pub fn tr(key: &str) -> String {
    t!(key).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: &str = include_str!("../locales/app.yml");

    /// Top-level keys of the translation file, with the languages each one has.
    fn keys() -> Vec<(String, Vec<String>)> {
        let mut keys: Vec<(String, Vec<String>)> = Vec::new();
        for line in LOCALES.lines() {
            if line.starts_with('#') || line.starts_with("_version") || line.trim().is_empty() {
                continue;
            }
            if let Some(key) = line.strip_suffix(':').filter(|_| !line.starts_with(' ')) {
                keys.push((key.to_string(), Vec::new()));
            } else if let Some((lang, _)) = line.trim().split_once(':') {
                keys.last_mut().unwrap().1.push(lang.to_string());
            }
        }
        keys
    }

    #[test]
    fn every_key_has_every_language() {
        let keys = keys();
        assert!(keys.len() > 50);
        for (key, langs) in keys {
            for lang in Lang::ALL {
                assert!(
                    langs.iter().any(|l| l == lang.code()),
                    "{key} has no {} translation",
                    lang.code()
                );
            }
        }
    }

    /// Every key used in the source must exist in the translation file.
    /// Keys appear as `t!("…")`, as match arm results `=> "…"` (state keys)
    /// and as `("changes.…", …)` tuples.
    #[test]
    fn every_used_key_exists() {
        let known: Vec<String> = keys().into_iter().map(|(k, _)| k).collect();
        let sources = [
            include_str!("app/mod.rs"),
            include_str!("app/changes.rs"),
            include_str!("app/sync.rs"),
            include_str!("git/diff.rs"),
            include_str!("git/ops.rs"),
            include_str!("git/repo.rs"),
            include_str!("git/runner.rs"),
            include_str!("git/status.rs"),
        ];
        let mut checked = 0;
        for source in sources {
            for (i, _) in source.match_indices('"') {
                let before = &source[..i];
                let rest = &source[i + 1..];
                let Some(end) = rest.find('"') else { continue };
                let piece = &rest[..end];
                let is_key_context = before.ends_with("t!(")
                    || before.ends_with("=> ")
                    || (before.ends_with('(') && piece.starts_with("changes."));
                let looks_like_key = piece.contains('.')
                    && piece
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c == '.');
                if is_key_context && looks_like_key {
                    checked += 1;
                    assert!(known.iter().any(|k| k == piece), "missing key {piece}");
                }
            }
        }
        assert!(checked > 50, "only {checked} keys found in the sources");
    }

    #[test]
    fn translates_with_arguments() {
        assert_eq!(
            t!("branch.current", name = "main", locale = "fr"),
            "Branche : main"
        );
        assert_eq!(
            t!("branch.current", name = "main", locale = "en"),
            "Branch: main"
        );
    }
}
