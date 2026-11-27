// SPDX-License-Identifier: GPL-3.0-or-later

//! The Visualizer's Milkdrop presets: which files there are, which one shows, and what the
//! window's keys do.
//!
//! The engine that draws a preset is the shell's (libprojectM, in a window with OpenGL); this
//! is the part every shell would otherwise write again. Presets are `.milk` files in a folder
//! the listener picks. One stays for [`STAY_SECONDS`], then the next blends in, unless the
//! listener holds the one shown.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Seconds a preset stays before the next one blends in.
pub const STAY_SECONDS: u32 = 30;

/// What a key did in the Visualizer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Another preset is current: the shell loads it.
    Changed,
    /// The key was one of the window's, and the preset shown stays.
    Taken,
    /// Not one of the window's keys.
    Other,
}

/// The presets of a folder, and the one that shows.
#[derive(Debug, Default)]
pub struct Presets {
    files: Vec<PathBuf>,
    index: usize,
    /// Set while the listener holds the preset shown: time passing does not change it.
    held: bool,
    /// The state of the random numbers; 0 before the first one.
    seed: u64,
    /// Whether the preset after a stay is one at random, not the next by name.
    pub random: bool,
}

impl Presets {
    /// Reads the presets of `folder` and the folders under it, by name, and makes the first
    /// current.
    pub fn read(&mut self, folder: &Path) {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for path in entries.flatten().map(|e| e.path()) {
                if path.is_dir() {
                    walk(&path, out);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("milk"))
                {
                    out.push(path);
                }
            }
        }
        self.files.clear();
        walk(folder, &mut self.files);
        self.files
            .sort_by(|a, b| by_name(&a.to_string_lossy(), &b.to_string_lossy()));
        self.index = 0;
    }

    /// How many presets there are.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the folder had no presets.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The preset that shows, or `None` when the folder had none: the engine then shows its
    /// own idle picture.
    pub fn current(&self) -> Option<&Path> {
        self.files.get(self.index).map(PathBuf::as_path)
    }

    /// Which preset is current, counted from 0.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Whether the listener holds the preset shown.
    pub fn held(&self) -> bool {
        self.held
    }

    /// The window's title: the preset's name after "Visualizer".
    pub fn title(&self) -> String {
        match self.current().and_then(Path::file_stem) {
            Some(name) => format!("Visualizer: {}", name.to_string_lossy()),
            None => "Visualizer".into(),
        }
    }

    /// Makes preset `to` current, going round at both ends. False when there are none.
    pub fn go(&mut self, to: i64) -> bool {
        if self.files.is_empty() {
            return false;
        }
        self.index = to.rem_euclid(self.files.len() as i64) as usize;
        true
    }

    /// Answers a key of the window, by its name: `right` the next preset, `left` the one
    /// before, `r` one at random, `space` holds the one shown or lets it go.
    pub fn key(&mut self, key: &str) -> Key {
        let changed = match key {
            "right" => self.go(self.index as i64 + 1),
            "left" => self.go(self.index as i64 - 1),
            "r" => {
                let pick = self.random();
                self.go(pick)
            }
            "space" => {
                self.held = !self.held;
                false
            }
            _ => return Key::Other,
        };
        if changed {
            Key::Changed
        } else {
            Key::Taken
        }
    }

    /// The stay is over: the next preset is current, or one at random other than this one
    /// when `random` is set, unless the listener holds this one. True when another
    /// preset is current.
    pub fn pass(&mut self) -> bool {
        if self.held || self.files.len() < 2 {
            return false;
        }
        let step = if self.random {
            // any but the one shown
            1 + self.random() % (self.files.len() as i64 - 1)
        } else {
            1
        };
        self.go(self.index as i64 + step)
    }

    /// A number to pick a preset by: an xorshift generator, started from the clock.
    fn random(&mut self) -> i64 {
        if self.seed == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64);
            self.seed = now | 1;
        }
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed >> 1) as i64
    }
}

/// Orders two names as a file browser does: without regard to case, and a run of digits by
/// the number it spells, so "preset 9" comes before "preset 10".
pub fn by_name(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let run = |text: &[char], from: usize| {
                let end = from
                    + text[from..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit())
                        .count();
                let digits: String = text[from..end].iter().skip_while(|c| **c == '0').collect();
                (digits, end)
            };
            let ((x, i_end), (y, j_end)) = (run(&a, i), run(&b, j));
            // without leading zeros the longer number is the larger
            let order = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
            if order != Ordering::Equal {
                return order;
            }
            (i, j) = (i_end, j_end);
        } else {
            let order = a[i].to_lowercase().cmp(b[j].to_lowercase());
            if order != Ordering::Equal {
                return order;
            }
            (i, j) = (i + 1, j + 1);
        }
    }
    (a.len() - i).cmp(&(b.len() - j)).then_with(|| a.cmp(&b))
}
