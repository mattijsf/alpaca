// SPDX-License-Identifier: GPL-3.0-or-later

//! What each command does, and what the listener's picks lead to.

use std::path::Path;

use walkit::classic::vis::VisMode;

use super::{is_url, App, Ask, Request};
use crate::player::{location_kind, LocationKind, EQ_PRESETS};

impl App {
    /// Does the command with number `id`: what a pick from a menu, or a key that stands for
    /// one, asks for. A command that needs a selection or a song does nothing without one.
    pub fn command(&mut self, id: i32) {
        use super::command::{
            ABOUT, ADD_FILES, ADD_FOLDER, ADD_URL, CHOOSE_PRESETS, CLEAR, CROP, DOUBLE_SIZE,
            EFFECTS, EQ_PRESET, FILE_INFO, HIDE, HTML_LIST, LOAD_LIST, MILKDROP_WINDOW, NEXT,
            ON_TOP, OPACITY, OPEN_FILES, OPEN_SKIN, PAUSE, PLAY, PLAY_SELECTED, PREFERENCES,
            PREVIOUS, QUIT, RANDOMIZE, RANDOM_PRESETS, REMOVE_DEAD, REMOVE_DUPLICATES,
            REMOVE_SELECTED, REPEAT, REREAD, REVEAL, REVERSE, SAVE_LIST, SELECT_ALL, SELECT_INVERT,
            SELECT_NONE, SHUFFLE, SKIN, SORT_FILENAME, SORT_PATH, SORT_TITLE, STOP, TIME_ELAPSED,
            TIME_REMAINING, VISUALIZER, VIS_ANALYZER, VIS_OFF, VIS_SCOPE,
        };
        let skins = SKIN..SKIN + super::command::SKINS as i32;
        let presets = EQ_PRESET..=EQ_PRESET + EQ_PRESETS.len() as i32;
        match id {
            OPEN_FILES => self.request(Request::Ask(Ask::OpenSongs)),
            ADD_FILES => self.request(Request::Ask(Ask::AddSongs)),
            ADD_URL => self.request(Request::Ask(Ask::Url)),
            OPEN_SKIN => self.request(Request::Ask(Ask::Skin)),
            ADD_FOLDER => self.request(Request::Ask(Ask::AddFolders)),
            LOAD_LIST => self.request(Request::Ask(Ask::LoadList)),
            SAVE_LIST => self.request(Request::Ask(Ask::SaveList)),
            HTML_LIST => self.request(Request::Ask(Ask::SaveHtml)),
            PREVIOUS => self.player.borrow_mut().previous(),
            PLAY => self.player.borrow_mut().play(),
            PAUSE => self.player.borrow_mut().pause(),
            STOP => self.player.borrow_mut().stop(),
            NEXT => self.player.borrow_mut().next(),
            SHUFFLE => {
                let on = !self.player.borrow().shuffle();
                self.player.borrow_mut().set_shuffle(on);
                self.modes_changed();
            }
            REPEAT => {
                let on = !self.player.borrow().repeat();
                self.player.borrow_mut().set_repeat(on);
                self.modes_changed();
            }
            TIME_REMAINING => self.set_time_remaining(true),
            TIME_ELAPSED => self.set_time_remaining(false),
            DOUBLE_SIZE => self.set_zoom(if self.zoom == 1 { 2 } else { 1 }),
            ON_TOP => self.set_on_top(!self.on_top),
            EFFECTS => self.request(Request::Effects),
            PREFERENCES => self.request(Request::Preferences),
            VISUALIZER => self.toggle_visualizer(),
            MILKDROP_WINDOW => self.request(Request::Vis(!self.vis_open)),
            RANDOM_PRESETS => self.set_vis_random(!self.vis_random()),
            VIS_ANALYZER => self.set_vis_mode(VisMode::Analyzer),
            VIS_SCOPE => self.set_vis_mode(VisMode::Scope),
            VIS_OFF => self.set_vis_mode(VisMode::Off),
            CHOOSE_PRESETS => self.request(Request::Ask(Ask::Presets)),
            ABOUT => self.request(Request::About),
            QUIT => self.request(Request::Quit),
            HIDE => self.request(Request::Hide),
            REMOVE_SELECTED => {
                let rows = self.selection();
                self.change_list(|player| player.remove(&rows));
            }
            CROP => {
                let rows = self.selection();
                if !rows.is_empty() {
                    self.change_list(|player| player.crop(&rows));
                }
            }
            CLEAR => self.change_list(|player| player.clear()),
            REMOVE_DEAD => self.change_list(|player| {
                player.remove_dead();
            }),
            REMOVE_DUPLICATES => self.change_list(|player| {
                player.remove_duplicates();
            }),
            SORT_TITLE => self.change_list(|player| player.sort_by_title()),
            SORT_FILENAME => self.change_list(|player| player.sort_by_filename()),
            SORT_PATH => self.change_list(|player| player.sort_by_path()),
            REVERSE => self.change_list(|player| player.reverse()),
            RANDOMIZE => {
                // any number will do: the clock's is never the same twice
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64);
                self.change_list(|player| player.randomize(seed));
            }
            SELECT_ALL => {
                let rows: Vec<usize> = (0..self.player.borrow().entries().len()).collect();
                self.select(&rows);
            }
            SELECT_NONE => self.select(&[]),
            SELECT_INVERT => {
                let selected = self.selection();
                let rows: Vec<usize> = (0..self.player.borrow().entries().len())
                    .filter(|row| !selected.contains(row))
                    .collect();
                self.select(&rows);
            }
            FILE_INFO => {
                if let Some(row) = self.row_in_question() {
                    self.request(Request::Info(row));
                }
            }
            REREAD => {
                // the selection's files, or every file with nothing selected
                let mut rows = self.selection();
                if rows.is_empty() {
                    rows = (0..self.player.borrow().entries().len()).collect();
                }
                self.player.borrow_mut().reread(&rows);
                self.refresh_skin();
            }
            PLAY_SELECTED => {
                if let Some(&row) = self.selection().first() {
                    self.player.borrow_mut().play_index(row);
                }
            }
            REVEAL => {
                if let Some(file) = self.selected_file() {
                    self.request(Request::Reveal(file));
                }
            }
            id if (OPACITY..=OPACITY + 6).contains(&id) => {
                self.set_opacity(100 - (id - OPACITY) * 10);
            }
            id if skins.contains(&id) => {
                let name = self.skins().into_iter().nth((id - SKIN) as usize);
                if let Some(file) = name.and_then(|name| self.skin_file(&name)) {
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

    /// Answers a key, by its name. `delete` takes the selected entries out of the playlist and
    /// `return` plays the first of them, as in Winamp's playlist; the rest are Winamp's own
    /// keys, which the player knows ([`Player::key`](crate::player::Player::key)). False for
    /// a key that is none of these.
    pub fn key(&mut self, key: &str) -> bool {
        match key {
            "delete" => self.command(super::command::REMOVE_SELECTED),
            "return" => self.command(super::command::PLAY_SELECTED),
            _ => return self.player.borrow_mut().key(key),
        }
        true
    }

    /// Shuffle or repeat changed: the skin's buttons and the menu bar show it.
    fn modes_changed(&mut self) {
        self.refresh_skin();
        self.request(Request::Menu);
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
            Ask::SaveHtml => match std::fs::write(first, self.html_list()) {
                // the page opens in the browser, as Winamp's does
                Ok(()) => self.request(Request::Open(first.into())),
                Err(e) => self.request(Request::Error(format!("{first}: {e}"))),
            },
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
                self.presets_picked(Path::new(first));
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
        let added = {
            let mut player = self.player.borrow_mut();
            locations
                .iter()
                .map(|l| player.add_location(Path::new(l)))
                .sum()
        };
        self.refresh_skin();
        added
    }

    /// Empties the playlist, adds `locations` and plays the first.
    pub fn play(&mut self, locations: &[String]) {
        self.change_list(|player| player.clear());
        if self.add(locations) > 0 {
            self.player.borrow_mut().play_index(0);
        }
    }
}
