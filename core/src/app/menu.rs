// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's menus as text a shell builds its own from.

use std::path::Path;

use walkit::classic::vis::VisMode;

use super::{command, App, Menu};
use crate::player::EQ_PRESETS;

/// A menu being spelled out, a line for each item.
struct Lines(String);

impl Lines {
    /// A command: its number, its text, its key and whether it is checked.
    fn item(&mut self, id: i32, text: &str, key: &str, checked: bool) {
        self.0.push_str(&format!(
            "C\t{id}\t{}\t0\t{text}\t{key}\n",
            u8::from(checked)
        ));
    }

    /// A command with no key and no check, grayed unless `enabled`.
    fn plain(&mut self, id: i32, text: &str, enabled: bool) {
        self.0
            .push_str(&format!("C\t{id}\t0\t{}\t{text}\t\n", u8::from(!enabled)));
    }

    fn separator(&mut self) {
        self.0.push_str("S\n");
    }

    /// A submenu starts; [`Lines::end`] ends it.
    fn sub(&mut self, text: &str) {
        self.0.push_str(&format!(">\t{text}\n"));
    }

    fn end(&mut self) {
        self.0.push_str("<\n");
    }
}

impl App {
    /// One of the app's menus as lines of text, for the shell to build its own from: `C`, the
    /// command's number, 1 when checked, 1 when disabled, the text and the key, with tabs
    /// between, for a command; `S` for a separator; `>` and the text where a submenu starts
    /// and `<` where it ends. A key is a letter or a sign, with `shift+` before it when Shift
    /// is held, and goes with the system's own command key; most items have none.
    pub fn menu(&self, menu: Menu) -> String {
        let mut m = Lines(String::new());
        // what the playlist's commands would act on
        let rows = self.player.borrow().entries().len();
        let some = rows > 0;
        let picked = !self.selection().is_empty();
        match menu {
            Menu::Bar => {
                m.sub("Alpaca");
                m.item(command::ABOUT, "About Alpaca", "", false);
                m.separator();
                m.item(command::PREFERENCES, "Preferences…", ",", false);
                m.separator();
                m.item(command::QUIT, "Quit Alpaca", "q", false);
                m.end();
                m.sub("File");
                m.item(command::OPEN_FILES, "Open Files…", "o", false);
                m.item(command::ADD_FILES, "Add Files…", "l", false);
                m.item(command::ADD_FOLDER, "Add Folder…", "shift+l", false);
                m.item(command::ADD_URL, "Add Radio Stream…", "u", false);
                m.separator();
                m.item(command::LOAD_LIST, "Load Playlist…", "", false);
                m.item(command::SAVE_LIST, "Save Playlist…", "s", false);
                m.separator();
                m.item(command::OPEN_SKIN, "Open Skin…", "shift+o", false);
                m.end();
                // Winamp's keys, with the command key: Z X C V B
                m.sub("Play");
                m.item(command::PREVIOUS, "Previous", "z", false);
                m.item(command::PLAY, "Play", "x", false);
                m.item(command::PAUSE, "Pause", "c", false);
                m.item(command::STOP, "Stop", "v", false);
                m.item(command::NEXT, "Next", "b", false);
                m.separator();
                self.modes(&mut m, "r", "shift+s");
                m.end();
                m.sub("Playlist");
                m.item(command::SELECT_ALL, "Select All", "a", false);
                m.item(command::SELECT_NONE, "Select None", "shift+a", false);
                m.item(command::SELECT_INVERT, "Invert Selection", "i", false);
                m.separator();
                self.removals(&mut m, picked, some);
                m.separator();
                m.sub("Sort");
                Self::sorts(&mut m, rows > 1);
                m.end();
                m.plain(command::FILE_INFO, "File Info…", some);
                m.end();
                m.sub("Skins");
                self.skin_items(&mut m);
                m.end();
                m.sub("View");
                m.item(command::DOUBLE_SIZE, "Double Size", "d", self.zoom > 1);
                m.item(command::ON_TOP, "Always on Top", "t", self.on_top);
                self.opacities(&mut m);
                m.separator();
                m.item(command::EFFECTS, "Effects…", "e", false);
                m.item(
                    command::VISUALIZER,
                    "Visualizer",
                    "shift+k",
                    self.vis_running(),
                );
                m.sub("Visualization");
                self.visualization(&mut m);
                m.end();
                m.end();
            }
            Menu::Popup => {
                m.item(command::OPEN_FILES, "Open Files…", "", false);
                m.item(command::ADD_FILES, "Add Files…", "", false);
                m.item(command::ADD_URL, "Add Radio Stream…", "", false);
                m.separator();
                m.item(command::OPEN_SKIN, "Open Skin…", "", false);
                m.sub("Skins");
                self.skin_items(&mut m);
                m.end();
                m.separator();
                m.item(command::DOUBLE_SIZE, "Double Size", "", self.zoom > 1);
                m.item(command::ON_TOP, "Always on Top", "", self.on_top);
                self.opacities(&mut m);
                m.item(command::EFFECTS, "Effects…", "", false);
                m.item(command::VISUALIZER, "Visualizer", "", self.vis_running());
                m.separator();
                m.item(command::PREFERENCES, "Preferences…", "", false);
                m.separator();
                m.item(command::QUIT, "Quit Alpaca", "", false);
            }
            Menu::Equalizer => {
                m.item(command::EQ_PRESET, "Flat", "", false);
                m.separator();
                for (n, (name, _, _)) in EQ_PRESETS.iter().enumerate() {
                    m.item(command::EQ_PRESET + 1 + n as i32, name, "", false);
                }
            }
            Menu::Options => {
                m.item(command::PREFERENCES, "Preferences…", "", false);
                m.separator();
                m.sub("Skins");
                self.skin_items(&mut m);
                m.end();
                m.separator();
                let left = self.time_remaining();
                m.item(command::TIME_ELAPSED, "Time Elapsed", "", !left);
                m.item(command::TIME_REMAINING, "Time Remaining", "", left);
                m.separator();
                m.item(command::DOUBLE_SIZE, "Double Size", "", self.zoom > 1);
                m.item(command::ON_TOP, "Always on Top", "", self.on_top);
                self.opacities(&mut m);
                m.separator();
                self.modes(&mut m, "", "");
            }
            Menu::Visualization => self.visualization(&mut m),
            Menu::PlaylistAdd => {
                m.item(command::ADD_FILES, "Add Files…", "", false);
                m.item(command::ADD_FOLDER, "Add Folder…", "", false);
                m.item(command::ADD_URL, "Add Radio Stream…", "", false);
            }
            Menu::PlaylistRemove => self.removals(&mut m, picked, some),
            Menu::PlaylistSelect => {
                m.plain(command::SELECT_ALL, "Select All", some);
                m.plain(command::SELECT_NONE, "Select None", picked);
                m.plain(command::SELECT_INVERT, "Invert Selection", some);
            }
            Menu::PlaylistMisc => {
                m.sub("Sort List");
                Self::sorts(&mut m, rows > 1);
                m.end();
                m.plain(command::FILE_INFO, "File Info…", some);
                m.sub("Misc Options");
                Self::misc_options(&mut m, some);
                m.end();
            }
            Menu::PlaylistList => {
                m.item(command::LOAD_LIST, "Load Playlist…", "", false);
                m.plain(command::SAVE_LIST, "Save Playlist…", some);
                m.plain(command::CLEAR, "New Playlist", some);
            }
            Menu::PlaylistRow => {
                m.plain(command::PLAY_SELECTED, "Play", picked);
                m.plain(command::FILE_INFO, "File Info…", picked);
                m.plain(
                    command::REVEAL,
                    "Show in Folder",
                    self.selected_file().is_some(),
                );
                m.separator();
                m.plain(command::REMOVE_SELECTED, "Remove", picked);
                m.plain(command::CROP, "Crop", picked);
            }
            Menu::Sort => Self::sorts(&mut m, rows > 1),
            Menu::RemoveMisc => {
                m.plain(command::REMOVE_DEAD, "Remove Dead Files", some);
                m.plain(command::REMOVE_DUPLICATES, "Remove Duplicates", rows > 1);
            }
            Menu::MiscOptions => Self::misc_options(&mut m, some),
        }
        m.0
    }

    /// The skins that come with the app, then the listener's own under a line, the one
    /// worn checked.
    fn skin_items(&self, m: &mut Lines) {
        let worn = self.skin_name();
        let builtin = self.builtin_skins().len();
        for (n, file) in self.skins().iter().enumerate() {
            if n == builtin {
                m.separator();
            }
            let name = Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned());
            m.item(
                command::SKIN + n as i32,
                &name.unwrap_or_default(),
                "",
                *file == worn,
            );
        }
    }

    /// The Opacity submenu: 100 % down to 40 %.
    fn opacities(&self, m: &mut Lines) {
        m.sub("Opacity");
        for step in 0..=6 {
            let percent = 100 - step * 10;
            m.item(
                command::OPACITY + step,
                &format!("{percent} %"),
                "",
                self.opacity == percent,
            );
        }
        m.end();
    }

    /// Repeat and shuffle, with the keys given.
    fn modes(&self, m: &mut Lines, repeat: &str, shuffle: &str) {
        let player = self.player.borrow();
        m.item(command::REPEAT, "Repeat", repeat, player.repeat());
        m.item(command::SHUFFLE, "Shuffle", shuffle, player.shuffle());
    }

    /// What takes entries out of the playlist.
    fn removals(&self, m: &mut Lines, picked: bool, some: bool) {
        let rows = self.player.borrow().entries().len();
        m.plain(command::REMOVE_SELECTED, "Remove Selected", picked);
        m.plain(command::CROP, "Crop Selected", picked);
        m.plain(command::CLEAR, "Clear Playlist", some);
        m.separator();
        m.plain(command::REMOVE_DEAD, "Remove Dead Files", some);
        m.plain(command::REMOVE_DUPLICATES, "Remove Duplicates", rows > 1);
    }

    /// The ways to sort the playlist.
    fn sorts(m: &mut Lines, enabled: bool) {
        m.plain(command::SORT_TITLE, "Sort by Title", enabled);
        m.plain(command::SORT_FILENAME, "Sort by File Name", enabled);
        m.plain(command::SORT_PATH, "Sort by Path and File Name", enabled);
        m.separator();
        m.plain(command::REVERSE, "Reverse List", enabled);
        m.plain(command::RANDOMIZE, "Randomize List", enabled);
    }

    /// The playlist's other options.
    fn misc_options(m: &mut Lines, some: bool) {
        m.plain(command::HTML_LIST, "Generate HTML Playlist…", some);
        m.plain(command::REREAD, "Read File Info Again", some);
    }

    /// What a classic skin's visualizer shows, and the Milkdrop window.
    fn visualization(&self, m: &mut Lines) {
        let mode = self.vis_mode();
        m.item(
            command::VIS_ANALYZER,
            "Spectrum Analyzer",
            "",
            mode == VisMode::Analyzer,
        );
        m.item(
            command::VIS_SCOPE,
            "Oscilloscope",
            "",
            mode == VisMode::Scope,
        );
        m.item(command::VIS_OFF, "Off", "", mode == VisMode::Off);
        m.separator();
        m.item(
            command::MILKDROP_WINDOW,
            "Milkdrop Window",
            "",
            self.vis_open,
        );
        m.item(
            command::RANDOM_PRESETS,
            "Random Presets",
            "",
            self.vis_random(),
        );
        m.item(
            command::CHOOSE_PRESETS,
            "Choose Milkdrop Presets…",
            "",
            false,
        );
    }
}
