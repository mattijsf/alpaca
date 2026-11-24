// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's menus, what each command does, and what the listener's picks lead to.

use std::path::Path;

use super::{command, is_url, App, Ask, Menu, Request};
use crate::player::{location_kind, LocationKind, EQ_PRESETS};

impl App {
    /// One of the app's menus as lines of text, for the shell to build its own from: `C`, the
    /// command's number, 1 when checked, 1 when disabled, the text and the key, with tabs
    /// between, for a command; `S` for a separator; `>` and the text where a submenu starts
    /// and `<` where it ends. A key is a letter, with `shift+` before it when Shift is held,
    /// and goes with the system's own command key; most items have none.
    pub fn menu(&self, menu: Menu) -> String {
        let mut out = String::new();
        let item = |out: &mut String, id: i32, text: &str, key: &str, checked: bool| {
            out.push_str(&format!(
                "C\t{id}\t{}\t0\t{text}\t{key}\n",
                u8::from(checked)
            ));
        };
        let worn = self.skin_name();
        let skins = |out: &mut String| {
            for (n, file) in self.builtin_skins().iter().enumerate() {
                let name = Path::new(file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned());
                let id = command::SKIN + n as i32;
                out.push_str(&format!(
                    "C\t{id}\t{}\t0\t{}\t\n",
                    u8::from(*file == worn),
                    name.unwrap_or_default()
                ));
            }
        };
        let opacity = |out: &mut String| {
            out.push_str(">\tOpacity\n");
            for step in 0..=6 {
                let percent = 100 - step * 10;
                out.push_str(&format!(
                    "C\t{}\t{}\t0\t{percent} %\t\n",
                    command::OPACITY + step,
                    u8::from(self.opacity == percent)
                ));
            }
            out.push_str("<\n");
        };
        match menu {
            Menu::Bar => {
                out.push_str(">\tAndamp\n");
                item(&mut out, command::ABOUT, "About Andamp", "", false);
                out.push_str("S\n");
                item(&mut out, command::QUIT, "Quit Andamp", "q", false);
                out.push_str("<\n>\tFile\n");
                item(&mut out, command::OPEN_FILES, "Open Files…", "o", false);
                item(&mut out, command::ADD_FILES, "Add Files…", "l", false);
                item(&mut out, command::ADD_URL, "Add Radio Stream…", "u", false);
                out.push_str("S\n");
                item(&mut out, command::OPEN_SKIN, "Open Skin…", "shift+o", false);
                // Winamp's keys, with the command key: Z X C V B
                out.push_str("<\n>\tPlay\n");
                item(&mut out, command::PREVIOUS, "Previous", "z", false);
                item(&mut out, command::PLAY, "Play", "x", false);
                item(&mut out, command::PAUSE, "Pause", "c", false);
                item(&mut out, command::STOP, "Stop", "v", false);
                item(&mut out, command::NEXT, "Next", "b", false);
                out.push_str("<\n>\tSkins\n");
                skins(&mut out);
                out.push_str("<\n>\tView\n");
                item(
                    &mut out,
                    command::DOUBLE_SIZE,
                    "Double Size",
                    "d",
                    self.zoom > 1,
                );
                item(&mut out, command::ON_TOP, "Always on Top", "a", self.on_top);
                opacity(&mut out);
                out.push_str("S\n");
                item(&mut out, command::EFFECTS, "Effects…", "e", false);
                item(
                    &mut out,
                    command::VISUALIZER,
                    "Visualizer",
                    "shift+k",
                    self.vis_open,
                );
                item(
                    &mut out,
                    command::CHOOSE_PRESETS,
                    "Choose Milkdrop Presets…",
                    "",
                    false,
                );
                out.push_str("<\n");
            }
            Menu::Popup => {
                item(&mut out, command::OPEN_FILES, "Open Files…", "", false);
                item(&mut out, command::ADD_FILES, "Add Files…", "", false);
                item(&mut out, command::ADD_URL, "Add Radio Stream…", "", false);
                out.push_str("S\n");
                item(&mut out, command::OPEN_SKIN, "Open Skin…", "", false);
                out.push_str(">\tSkins\n");
                skins(&mut out);
                out.push_str("<\nS\n");
                item(
                    &mut out,
                    command::DOUBLE_SIZE,
                    "Double Size",
                    "",
                    self.zoom > 1,
                );
                item(&mut out, command::ON_TOP, "Always on Top", "", self.on_top);
                opacity(&mut out);
                item(&mut out, command::EFFECTS, "Effects…", "", false);
                item(
                    &mut out,
                    command::VISUALIZER,
                    "Visualizer",
                    "",
                    self.vis_open,
                );
                out.push_str("S\n");
                item(&mut out, command::QUIT, "Quit Andamp", "", false);
            }
            Menu::Equalizer => {
                item(&mut out, command::EQ_PRESET, "Flat", "", false);
                out.push_str("S\n");
                for (n, (name, _, _)) in EQ_PRESETS.iter().enumerate() {
                    item(&mut out, command::EQ_PRESET + 1 + n as i32, name, "", false);
                }
            }
        }
        out
    }

    /// Does the command with number `id`: what a pick from the menu, or a key that stands
    /// for one, asks for.
    pub fn command(&mut self, id: i32) {
        use super::command::{
            ABOUT, ADD_FILES, ADD_FOLDER, ADD_URL, CHOOSE_PRESETS, DOUBLE_SIZE, EFFECTS, EQ_PRESET,
            HIDE, LOAD_LIST, NEXT, ON_TOP, OPACITY, OPEN_FILES, OPEN_SKIN, PAUSE, PLAY, PREVIOUS,
            QUIT, SAVE_LIST, SKIN, STOP, VISUALIZER,
        };
        let skins = SKIN..SKIN + 100;
        let presets = EQ_PRESET..=EQ_PRESET + EQ_PRESETS.len() as i32;
        match id {
            OPEN_FILES => self.request(Request::Ask(Ask::OpenSongs)),
            ADD_FILES => self.request(Request::Ask(Ask::AddSongs)),
            ADD_URL => self.request(Request::Ask(Ask::Url)),
            OPEN_SKIN => self.request(Request::Ask(Ask::Skin)),
            ADD_FOLDER => self.request(Request::Ask(Ask::AddFolders)),
            LOAD_LIST => self.request(Request::Ask(Ask::LoadList)),
            SAVE_LIST => self.request(Request::Ask(Ask::SaveList)),
            PREVIOUS => self.player.borrow_mut().previous(),
            PLAY => self.player.borrow_mut().play(),
            PAUSE => self.player.borrow_mut().pause(),
            STOP => self.player.borrow_mut().stop(),
            NEXT => self.player.borrow_mut().next(),
            DOUBLE_SIZE => self.set_zoom(if self.zoom == 1 { 2 } else { 1 }),
            ON_TOP => self.set_on_top(!self.on_top),
            EFFECTS => self.request(Request::Effects),
            VISUALIZER => self.request(Request::Vis(!self.vis_open)),
            CHOOSE_PRESETS => self.request(Request::Ask(Ask::Presets)),
            ABOUT => self.request(Request::About),
            QUIT => self.request(Request::Quit),
            HIDE => self.request(Request::Hide),
            id if (OPACITY..=OPACITY + 6).contains(&id) => {
                self.set_opacity(100 - (id - OPACITY) * 10);
            }
            id if skins.contains(&id) => {
                if let Some(name) = self.builtin_skins().get((id - SKIN) as usize) {
                    let file = self.builtin(name);
                    self.wear(&file);
                }
            }
            id if presets.contains(&id) => {
                // the first is flat, the rest the factory presets in order
                let preset = usize::try_from(id - EQ_PRESET - 1).ok();
                self.player.borrow_mut().apply_eq_preset(preset);
            }
            _ => {}
        }
    }

    /// What the listener picked when asked: files and folders by their paths, the address of
    /// a radio stream as text. Nothing picked is no call, or an empty `answer`.
    pub fn picked(&mut self, ask: Ask, answer: &[String]) {
        let Some(first) = answer.first() else {
            return;
        };
        match ask {
            Ask::OpenSongs | Ask::LoadList => self.play(answer),
            Ask::AddSongs | Ask::AddFolders => {
                self.add(answer);
            }
            Ask::SaveList => {
                let saved = self.player.borrow().save_list(Path::new(first));
                if let Err(message) = saved {
                    self.request(Request::Error(message));
                }
            }
            Ask::Skin => {
                self.wear(Path::new(first));
            }
            Ask::Url => {
                let address = first.trim();
                if !is_url(address) {
                    return;
                }
                let was_empty = self.player.borrow().entries().is_empty();
                if self.add(&[address.to_string()]) > 0 && was_empty {
                    self.player.borrow_mut().play_index(0);
                }
            }
            Ask::Presets => {
                self.settings.set("presets", Some(first));
                self.presets.read(Path::new(first));
                if self.vis_open {
                    self.show_preset(false);
                } else {
                    self.request(Request::Vis(true));
                }
            }
        }
    }

    /// What the system asks the app to open, or what was dropped on a window: a skin is
    /// worn; songs, folders of songs, playlist files and radio streams take the playlist's
    /// place and play. Before [`App::start`] it waits for the start.
    pub fn open(&mut self, locations: &[String]) {
        if !self.started || self.playlist_due > 0 {
            self.waiting.extend_from_slice(locations);
            return;
        }
        let is_skin = |location: &String| {
            !is_url(location) && location_kind(Path::new(location)) == LocationKind::Skin
        };
        if let Some(skin) = locations.iter().find(|l| is_skin(l)) {
            self.wear(Path::new(skin));
        }
        let rest: Vec<String> = locations.iter().filter(|l| !is_skin(l)).cloned().collect();
        if !rest.is_empty() {
            self.play(&rest);
        }
    }

    /// Adds songs, folders of songs, playlist files and radio streams to the playlist; how
    /// many entries joined.
    pub fn add(&mut self, locations: &[String]) -> usize {
        let mut player = self.player.borrow_mut();
        locations
            .iter()
            .map(|l| player.add_location(Path::new(l)))
            .sum()
    }

    /// Empties the playlist, adds `locations` and plays the first.
    pub fn play(&mut self, locations: &[String]) {
        self.player.borrow_mut().clear();
        if self.add(locations) > 0 {
            self.player.borrow_mut().play_index(0);
        }
    }
}
