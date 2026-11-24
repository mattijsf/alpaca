// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line.

use std::path::PathBuf;

use super::is_url;
use crate::player::{location_kind, LocationKind};

/// What the command line asks for.
///
/// ```text
/// andamp [skin.wsz|skin.wal] [song ...] [--modern] [--zoom n] [--volume 0..255]
///        [--effect <id>]       switches on the effect with that id or name; may be given more
///                              than once
///        [--show-vis]          opens the Visualizer window
///        [--presets <folder>]  the Milkdrop presets it shows
///        [--show-effects]      opens the Effects window
///        [--show-prefs]        opens the Preferences window
///        [--base skin.wsz]     a classic skin laid under classic skins, in place of AndAmp Dark
///        [--wasabi base.zip]   a zip of a wasabi folder laid under modern skins, in place of
///                              Andamp's own element set
/// ```
///
/// Songs named on the command line play at once. The rest is for checking and measuring, and
/// the shell does it: `--click x,y` and `--press-pref <id>` (each may be given more than once),
/// `--quit-after <seconds>`, `--dump <folder>`, `--bench <seconds>`, `--then-skin <file>` and
/// `--hidden`.
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// The skin to wear in place of the one last worn.
    pub skin: Option<PathBuf>,
    /// Songs, folders, playlist files and radio streams.
    pub songs: Vec<String>,
    /// `--volume`: the volume to start at, 0..255.
    pub volume: Option<i32>,
    /// `--modern`: start with the built-in modern skin.
    pub modern: bool,
    /// `--zoom`: points to a layout pixel.
    pub zoom: Option<i32>,
    /// `--base`: a classic skin laid under classic skins.
    pub base: Option<PathBuf>,
    /// `--wasabi`: a zip of a wasabi folder laid under modern skins.
    pub wasabi: Option<PathBuf>,
    /// `--effect`: the effects to switch on, by id or name.
    pub effects: Vec<String>,
    /// `--show-effects`: open the Effects window.
    pub show_effects: bool,
    /// `--show-vis`: open the Visualizer window.
    pub show_vis: bool,
    /// `--show-prefs`: open the Preferences window.
    pub show_prefs: bool,
    /// `--presets`: the folder of Milkdrop presets to show.
    pub presets: Option<PathBuf>,
    /// `--dump`: the folder the shell writes every window into as a picture, before it quits.
    pub dump: Option<String>,
    /// `--bench`: the seconds the shell measures before it quits.
    pub bench: Option<String>,
    /// `--quit-after`: the seconds after which the shell quits as the menu's Quit does.
    pub quit_after: Option<String>,
    /// `--hidden`: the shell hides the app once it is up.
    pub hidden: bool,
    /// `--then-skin`: a skin the shell opens a second after the start.
    pub then_skin: Option<String>,
    /// `--click`: places the shell clicks at, each `x,y` in layout pixels.
    pub clicks: Vec<String>,
    /// `--press-pref`: rows of the Preferences window the shell presses, by their id.
    pub pref_presses: Vec<String>,
}

impl Options {
    /// Reads the arguments after the program's name.
    pub fn parse(args: &[String]) -> Options {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = || args.next().cloned();
            match arg.as_str() {
                "--modern" => options.modern = true,
                "--zoom" => options.zoom = value().and_then(|v| v.parse().ok()),
                "--volume" => options.volume = value().and_then(|v| v.parse().ok()),
                "--dump" => options.dump = value(),
                "--show-effects" => options.show_effects = true,
                "--show-vis" => options.show_vis = true,
                "--show-prefs" => options.show_prefs = true,
                "--presets" => options.presets = value().map(|v| whole(&v)),
                "--effect" => options.effects.extend(value()),
                "--base" => options.base = value().map(|v| whole(&v)),
                "--wasabi" => options.wasabi = value().map(|v| whole(&v)),
                "--click" => options.clicks.extend(value()),
                "--press-pref" => options.pref_presses.extend(value()),
                "--bench" => options.bench = value(),
                "--hidden" => options.hidden = true,
                "--then-skin" => options.then_skin = value(),
                "--quit-after" => options.quit_after = value(),
                // unknown, with its value: what a system adds when it starts the app, such as
                // -NSDocumentRevisionsDebugMode YES
                other if other.starts_with('-') && !other.starts_with("--") => {
                    value();
                }
                other if other.starts_with("--") => {}
                location if is_url(location) => options.songs.push(location.to_string()),
                location => {
                    // from the folder the app was started in: kept for the next run, which
                    // may start somewhere else
                    let file = whole(location);
                    if location_kind(&file) == LocationKind::Skin {
                        options.skin = Some(file);
                    } else {
                        options.songs.push(file.to_string_lossy().into_owned());
                    }
                }
            }
        }
        options
    }

    /// What the command line gave for `name`, as text: a flag's value, `1` for a switch that
    /// is on, and for `click` a line for each click. `None` for what was not given.
    pub fn get(&self, name: &str) -> Option<String> {
        let on = |flag: bool| flag.then(|| "1".to_string());
        let path = |path: &Option<PathBuf>| path.as_ref().map(|p| p.to_string_lossy().into_owned());
        match name {
            "dump" => self.dump.clone(),
            "bench" => self.bench.clone(),
            "quit-after" => self.quit_after.clone(),
            "then-skin" => self.then_skin.clone(),
            "hidden" => on(self.hidden),
            "modern" => on(self.modern),
            "show-vis" => on(self.show_vis),
            "show-effects" => on(self.show_effects),
            "show-prefs" => on(self.show_prefs),
            "presets" => path(&self.presets),
            "click" => (!self.clicks.is_empty()).then(|| self.clicks.join("\n")),
            "press-pref" => (!self.pref_presses.is_empty()).then(|| self.pref_presses.join("\n")),
            _ => None,
        }
    }

    /// Whether this run reads the settings and leaves them: not one that only looks or
    /// measures, which starts from nothing.
    pub(super) fn kept(&self) -> bool {
        self.dump.is_none()
            && self.bench.is_none()
            && self.clicks.is_empty()
            && self.pref_presses.is_empty()
    }
}

/// The path `file` from the root: one given from the folder the app was started in has that
/// folder before it.
fn whole(file: &str) -> PathBuf {
    std::path::absolute(file).unwrap_or_else(|_| PathBuf::from(file))
}
