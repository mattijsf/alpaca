// SPDX-License-Identifier: GPL-3.0-or-later

//! What the app keeps between runs, in a file of its own: the same file and the same code on
//! every system, so a shell stores nothing itself.
//!
//! The file is text, a key and its value to a line with a tab between, sorted by key. A key
//! is a path of words with dots (`skin`, `places.AndAmp Dark.wsz.1`); a value is any text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file's name in the app's folder.
const FILE: &str = "settings.txt";

/// The folder the app keeps its settings and the listener's plug-ins in: Application Support
/// on a Mac, the roaming `AppData` on Windows, the XDG config folder elsewhere. `None` when the
/// system names no home.
pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(target_os = "macos") {
        // inside a sandbox HOME is the app's container, which is where this belongs
        var("HOME").map(|home| home.join("Library/Application Support/Andamp"))
    } else if cfg!(target_os = "windows") {
        var("APPDATA").map(|data| data.join("Andamp"))
    } else {
        var("XDG_CONFIG_HOME")
            .or_else(|| var("HOME").map(|home| home.join(".config")))
            .map(|config| config.join("andamp"))
    }
}

/// The settings: read once, changed in memory, written when asked.
#[derive(Debug, Default)]
pub struct Settings {
    /// The file, or `None` for settings that are never written: a run that only looks.
    file: Option<PathBuf>,
    values: BTreeMap<String, String>,
    changed: bool,
}

impl Settings {
    /// The settings in the folder `dir`; a folder with none starts empty.
    pub fn open(dir: &Path) -> Settings {
        let file = dir.join(FILE);
        let values = std::fs::read_to_string(&file)
            .map(|text| {
                text.lines()
                    .filter_map(|line| line.split_once('\t'))
                    .map(|(key, value)| (key.to_string(), unescape(value)))
                    .collect()
            })
            .unwrap_or_default();
        Settings {
            file: Some(file),
            values,
            changed: false,
        }
    }

    /// Settings that start empty and are never written.
    pub fn none() -> Settings {
        Settings::default()
    }

    /// The value kept under `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Sets `key` to `value`, or forgets it with `None`.
    pub fn set(&mut self, key: &str, value: Option<&str>) {
        // a key is one word to a line: no tab, no line end
        let key = key.replace(['\t', '\n', '\r'], " ");
        let before = self.values.get(&key).map(String::as_str);
        if before == value {
            return;
        }
        match value {
            Some(value) => self.values.insert(key, value.to_string()),
            None => self.values.remove(&key),
        };
        self.changed = true;
    }

    /// The keys that start with `prefix`, in order.
    pub fn keys(&self, prefix: &str) -> Vec<&str> {
        self.values
            .range(prefix.to_string()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.as_str())
            .collect()
    }

    /// Forgets every key that starts with `prefix`.
    pub fn clear(&mut self, prefix: &str) {
        let gone: Vec<String> = self.keys(prefix).into_iter().map(str::to_string).collect();
        for key in gone {
            self.values.remove(&key);
            self.changed = true;
        }
    }

    /// Writes the file when something changed since it was read or last written. The new file
    /// takes the old one's place in one step, so a crash leaves one of the two whole.
    pub fn save(&mut self) -> Result<(), String> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if !self.changed {
            return Ok(());
        }
        let text: String = self
            .values
            .iter()
            .map(|(key, value)| format!("{key}\t{}\n", escape(value)))
            .collect();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let fresh = file.with_extension("new");
        std::fs::write(&fresh, text).map_err(|e| e.to_string())?;
        std::fs::rename(&fresh, file).map_err(|e| e.to_string())?;
        self.changed = false;
        Ok(())
    }
}

/// A value on one line: a line end as `\n`, a tab as `\t`, a backslash twice.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}
