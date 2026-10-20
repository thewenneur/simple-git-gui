//! The app's own settings (not git's): a small `key=value` file in the
//! user's configuration folder, e.g. whether the tutorial was shown.

use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Settings {
    values: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

/// `<config folder>/simple-git/settings.txt`, or the folder named by
/// `SIMPLE_GIT_CONFIG_DIR`.
fn file() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SIMPLE_GIT_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("settings.txt"));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home().map(|h| h.join(".config")))
    }?;
    Some(dir.join("simple-git").join("settings.txt"))
}

fn parse(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect()
}

fn format(values: &BTreeMap<String, String>) -> String {
    let mut out = String::from("# Simple Git settings\n");
    for (key, value) in values {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

impl Settings {
    pub fn load() -> Self {
        let path = file();
        let values = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|c| parse(&c))
            .unwrap_or_default();
        Self { values, path }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Changes a setting (`None` removes it) and saves the file. A failure to
    /// save is ignored: the setting still holds until the app closes.
    pub fn set(&mut self, key: &str, value: Option<String>) {
        match value {
            Some(v) => self.values.insert(key.to_string(), v.replace('\n', " ")),
            None => self.values.remove(key),
        };
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, format(&self.values));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let mut values = BTreeMap::new();
        values.insert("tutorial.seen".to_string(), "yes".to_string());
        values.insert("tutorial.repo".to_string(), "/tmp/a b=c".to_string());
        let text = format(&values);
        assert_eq!(parse(&text), values);
        assert!(parse("# comment\nbroken line\n").is_empty());
    }
}
