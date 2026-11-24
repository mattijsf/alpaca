// SPDX-License-Identifier: GPL-3.0-or-later

//! The fonts under every skin, at the paths the engine reads them from: `fonts/LiberationSans.ttf`
//! for Arial, `fonts/DejaVuSansCondensed.ttf` for Tahoma, and their bold cuts. Skins name
//! these two Windows fonts without a file, and text that names no font is drawn in Arial.
//!
//! Each path gives the real Windows font where the system has it, and else the free font the
//! app brings that stands in for it: Liberation Sans has Arial's widths but not its letters,
//! and Tahoma's stand-in only looks like it. Only the real fonts draw text as Winamp draws
//! it, which is how walkit-cli comes to match Winamp when it is given them (`--fonts`): the
//! names here are the ones it maps. macOS and Windows have both fonts; Linux has Arial where
//! Microsoft's core fonts are installed. Nothing of them is in the app: they are read from the
//! system's folders when a skin first draws with them, and kept for every skin after.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use walkit::files::SkinFiles;

/// The fonts skins name without a file: the stand-in's path among the skin's files, and the
/// names the real font's file has on the systems: macOS's and Linux's, then Windows'. A disk
/// that ignores case finds the first.
const WINDOWS_FONTS: [(&str, &[&str]); 4] = [
    ("fonts/LiberationSans.ttf", &["Arial.ttf", "arial.ttf"]),
    (
        "fonts/LiberationSans-Bold.ttf",
        &["Arial Bold.ttf", "Arial_Bold.ttf", "arialbd.ttf"],
    ),
    (
        "fonts/DejaVuSansCondensed.ttf",
        &["Tahoma.ttf", "tahoma.ttf"],
    ),
    (
        "fonts/DejaVuSansCondensed-Bold.ttf",
        &["Tahoma Bold.ttf", "Tahoma_Bold.ttf", "tahomabd.ttf"],
    ),
];

/// The folders the system keeps Arial and Tahoma in, the first that has a font giving it.
pub fn system_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut folders = Vec::new();
    if cfg!(target_os = "macos") {
        folders.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        folders.push(PathBuf::from("/Library/Fonts"));
        folders.extend(home.map(|h| h.join("Library/Fonts")));
    } else if cfg!(target_os = "windows") {
        let windows =
            std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), PathBuf::from);
        folders.push(windows.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            folders.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    } else {
        for dir in [
            "/usr/share/fonts/truetype/msttcorefonts",
            "/usr/share/fonts/msttcore",
            "/usr/share/fonts/corefonts",
            "/usr/share/fonts/TTF",
        ] {
            folders.push(PathBuf::from(dir));
        }
        folders.extend(home.map(|h| h.join(".fonts")));
    }
    folders
}

/// What a path among the skin's files gave: the bytes and the file they came from.
type Found = Option<(Rc<[u8]>, PathBuf)>;

/// The fonts under every skin: the real ones from `system`, else the stand-ins in `own`. A
/// file is read the first time it is asked for.
pub struct Fonts {
    /// The folder of the stand-ins the app brings.
    own: PathBuf,
    /// The folders searched for the real fonts, in order.
    system: Vec<PathBuf>,
    /// What each path gave, once asked: the bytes and where they came from.
    read: RefCell<HashMap<String, Found>>,
}

impl Fonts {
    /// The real fonts from the system's folders, else the stand-ins in `own`.
    pub fn new(own: &Path) -> Fonts {
        Fonts::with_system(own, system_folders())
    }

    /// The stand-ins in `own` only, as text is drawn on a system without the real fonts.
    pub fn stand_ins(own: &Path) -> Fonts {
        Fonts::with_system(own, Vec::new())
    }

    /// The real fonts from `system`, searched in order, else the stand-ins in `own`.
    pub fn with_system(own: &Path, system: Vec<PathBuf>) -> Fonts {
        Fonts {
            own: own.to_path_buf(),
            system,
            read: RefCell::new(HashMap::new()),
        }
    }

    /// The file `path` among the skin's files is read from: the real font, the stand-in, or
    /// `None` when neither is there.
    pub fn source(&self, path: &str) -> Option<PathBuf> {
        self.load(path).map(|(_, from)| from)
    }

    /// The bytes at `path` and the file they came from, read once.
    fn load(&self, path: &str) -> Found {
        let key = path.to_lowercase();
        if let Some(found) = self.read.borrow().get(&key) {
            return found.clone();
        }
        let found = self
            .candidates(&key)
            .into_iter()
            .find_map(|file| std::fs::read(&file).ok().map(|b| (Rc::from(b), file)));
        self.read.borrow_mut().insert(key, found.clone());
        found
    }

    /// The files that may give `path` (lowercase), best first.
    fn candidates(&self, path: &str) -> Vec<PathBuf> {
        let Some(name) = path.strip_prefix("fonts/") else {
            return Vec::new();
        };
        let ttf = Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ttf"));
        if name.contains(['/', '\\']) || !ttf {
            return Vec::new();
        }
        let mut files = Vec::new();
        if let Some((stand_in, real)) = WINDOWS_FONTS
            .iter()
            .find(|(stand_in, _)| stand_in.eq_ignore_ascii_case(path))
        {
            for dir in &self.system {
                files.extend(real.iter().map(|n| dir.join(n)));
            }
            files.push(self.own.join(&stand_in["fonts/".len()..]));
        } else {
            // another font the app brings, by its own name
            if let Ok(entries) = std::fs::read_dir(&self.own) {
                files.extend(entries.flatten().map(|e| e.path()).filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
                }));
            }
        }
        files
    }
}

impl SkinFiles for Fonts {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        self.load(path).map(|(bytes, _)| bytes)
    }

    fn names(&self) -> Vec<String> {
        WINDOWS_FONTS
            .iter()
            .filter(|(stand_in, _)| {
                self.candidates(&stand_in.to_lowercase())
                    .iter()
                    .any(|f| f.is_file())
            })
            .map(|(stand_in, _)| (*stand_in).to_string())
            .collect()
    }
}
