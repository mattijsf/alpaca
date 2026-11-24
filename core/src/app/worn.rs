// SPDX-License-Identifier: GPL-3.0-or-later

//! The skin worn: wearing one, where its windows are, running it and answering its buttons.

use std::path::Path;

use super::{command, App, Ask, Menu, MenuHandler, Request, DEFAULT_SKIN};
use crate::host::HostAction;
use crate::player::{SkinPlayer, REQUEST_OPEN_FILES};
use crate::skin::{self, Skin};

impl App {
    /// The skin worn.
    pub fn skin(&mut self) -> Option<&mut Skin> {
        self.skin.as_deref_mut()
    }

    /// The file name of the skin worn.
    pub fn skin_name(&self) -> String {
        self.worn
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The screen in layout pixels.
    pub(super) fn viewport(&self) -> (i32, i32) {
        (self.screen.0 / self.zoom, self.screen.1 / self.zoom)
    }

    /// A place on the screen in points as one in layout pixels, kept on the screen.
    fn on_screen(&self, at: (i32, i32)) -> (i32, i32) {
        let (w, h) = self.viewport();
        let pixel = |points: i32| (f64::from(points) / f64::from(self.zoom)).round() as i32;
        (
            pixel(at.0).min(w - 40).max(0),
            pixel(at.1).min(h - 20).max(0),
        )
    }

    /// Every window's top left corner on the screen in points, by the engine's id, as the
    /// last step left them.
    pub fn places(&self) -> Vec<(u32, (i32, i32))> {
        let windows = self.skin.as_ref().map_or(&[][..], |skin| skin.windows());
        windows
            .iter()
            .map(|w| (w.id, (w.rect.left * self.zoom, w.rect.top * self.zoom)))
            .collect()
    }

    /// The main window's corner on the screen, in points.
    pub(super) fn main_place(&mut self) -> Option<(i32, i32)> {
        let zoom = self.zoom;
        let skin = self.skin.as_mut()?;
        let main = skin.runtime.main_window()?;
        let rect = skin.runtime.window_rect(main);
        Some((rect.left * zoom, rect.top * zoom))
    }

    /// Writes down where the windows of the skin worn are, for when it is worn again.
    pub(super) fn keep_places(&mut self) {
        let places = self.places();
        if places.is_empty() {
            return;
        }
        let prefix = format!("at.{}.", self.skin_name());
        self.settings.clear(&prefix);
        for (id, (x, y)) in places {
            self.settings
                .set(&format!("{prefix}{id}"), Some(&format!("{x},{y}")));
        }
    }

    /// Wears the skin at `file` in place of the one worn, with its windows where they were
    /// when it was last worn, or else with its main window where the last skin's was. When it
    /// does not open, the skin worn stays and the shell is asked to say why.
    pub fn wear(&mut self, file: &Path) -> bool {
        match self.open_skin(file) {
            Ok(()) => true,
            Err(message) => {
                self.request(Request::Error(message));
                false
            }
        }
    }

    fn open_skin(&mut self, file: &Path) -> Result<(), String> {
        let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // under a classic skin lies AndAmp Dark, which gives the pictures a skin leaves out;
        // under a modern one Andamp's element set; under both the stand-in fonts
        let base = self
            .options
            .base
            .clone()
            .unwrap_or_else(|| self.builtin(DEFAULT_SKIN));
        let base = std::fs::read(base).ok().map(skin::unzip).transpose()?;
        let wasabi = match &self.options.wasabi {
            Some(zip) => std::fs::read(zip).ok().map(skin::unzip).transpose()?,
            None => Some(skin::read_folder(&self.resources.join("wasabi"))),
        };
        let fonts = skin::read_fonts(&self.resources.join("fonts"));
        let fonts = &fonts;
        let player = self.player.clone();
        let mut skin = Skin::open(
            bytes,
            &name,
            base,
            wasabi,
            fonts,
            file.parent(),
            self.viewport(),
            |_| Box::new(SkinPlayer(player)),
        )?;
        skin.player = Some(self.player.clone());
        // the skin worn goes: its places are kept first
        if self.skin.is_some() {
            self.keep_places();
            self.main_corner = self.main_place();
        }
        self.skin = Some(Box::new(skin));
        self.worn = Some(file.to_path_buf());
        self.give_menu_handler();
        self.tell_switches();

        let prefix = format!("at.{name}.");
        let kept: Vec<(u32, (i32, i32))> = self
            .settings
            .keys(&prefix)
            .into_iter()
            .filter_map(|key| {
                let id = key[prefix.len()..].parse().ok()?;
                let (x, y) = self.settings.get(key)?.split_once(',')?;
                Some((id, (x.parse().ok()?, y.parse().ok()?)))
            })
            .collect();
        let zoom = self.zoom;
        let main = self
            .skin
            .as_ref()
            .and_then(|skin| skin.runtime.main_window());
        let corner = main
            .and_then(|main| kept.iter().find(|(id, _)| *id == main.0))
            .map(|(_, at)| *at)
            .or(self.main_corner.take());
        let corner = corner.map(|at| self.on_screen(at));
        let others: Vec<(u32, (i32, i32))> = kept
            .iter()
            .map(|(id, at)| (*id, self.on_screen(*at)))
            .collect();
        if let Some(skin) = self.skin.as_mut() {
            match (main, corner) {
                (Some(main), Some((x, y))) => {
                    // the main window goes where it was, and the others come along as they
                    // are docked; then every window that had a place of its own gets it back
                    let rect = skin.runtime.window_rect(main);
                    skin.move_windows(x - rect.left, y - rect.top);
                    for (id, (x, y)) in others {
                        if id != main.0 {
                            skin.place_window(id, x, y);
                        }
                    }
                }
                // a little in from the screen's top left corner
                _ => skin.move_windows(120 / zoom, 80 / zoom),
            }
        }
        self.request(Request::Skin);
        self.request(Request::Menu);
        Ok(())
    }

    /// Sets how the skin's own popup menus are shown, for this skin and every one after it.
    /// The handler is called from inside a mouse call and must not call the app.
    pub fn set_menu_handler(&mut self, handler: Option<MenuHandler>) {
        self.menu_handler = handler;
        self.give_menu_handler();
    }

    fn give_menu_handler(&mut self) {
        let handler = self.menu_handler.clone();
        if let Some(skin) = self.skin.as_mut() {
            skin.set_menu_handler(
                handler
                    .map(|show| -> crate::host::MenuHandler { Box::new(move |menu| show(menu)) }),
            );
        }
    }

    /// Tells the skin which of the app's switches are on, so its buttons show them.
    pub(super) fn tell_switches(&mut self) {
        let (double, on_top, vis) = (self.zoom > 1, self.on_top, self.vis_open);
        if let Some(skin) = self.skin.as_mut() {
            skin.set_switch("doublesize", double);
            skin.set_switch("ontop", on_top);
            skin.set_switch("vis", vis);
        }
    }

    /// Runs the skin and answers what its buttons asked for. Says what changed, as
    /// [`Skin::step`] does.
    pub fn step(&mut self) -> u32 {
        if self.playlist_due == 1 {
            self.playlist_due = 0;
            self.put_back_playlist();
        }
        let flags = self.skin.as_mut().map_or(0, |skin| skin.step());
        if self.playlist_due == 2 {
            self.playlist_due = 1;
        }
        if self.player.borrow_mut().take_requests() & REQUEST_OPEN_FILES != 0 {
            self.request(Request::Ask(Ask::OpenSongs));
        }
        let actions = self
            .skin
            .as_mut()
            .map(|skin| skin.take_actions())
            .unwrap_or_default();
        for action in actions {
            self.act(&action);
        }
        flags
    }

    /// Milliseconds until the app next has something to do by itself, and so until the next
    /// [`App::step`]: 0 for now, -1 for never. It is the skin's own clock, and now while the
    /// playlist waits to be put back.
    pub fn next_due_ms(&mut self) -> i64 {
        if self.playlist_due > 0 {
            return 0;
        }
        self.skin.as_mut().map_or(-1, |skin| skin.next_due_ms())
    }

    /// Answers something a button of the skin asked of the app.
    fn act(&mut self, action: &HostAction) {
        let at = action.at.map(|(x, y)| (x * self.zoom, y * self.zoom));
        match (action.name.as_str(), action.param.as_deref()) {
            ("CLOSE", _) => self.request(Request::Quit),
            ("MINIMIZE", _) => self.request(Request::Hide),
            ("DOUBLESIZE", _) => self.command(command::DOUBLE_SIZE),
            ("ALWAYS_ON_TOP", _) => self.command(command::ON_TOP),
            ("MENU" | "SYSMENU" | "CONTROLMENU", _) => self.request(Request::Popup {
                menu: Menu::Popup,
                at,
            }),
            ("ABOUT", _) => self.request(Request::About),
            ("TOGGLE" | "SHOW", Some("vis")) => self.command(command::VISUALIZER),
            ("EQ_PRESETS", _) => self.request(Request::Popup {
                menu: Menu::Equalizer,
                at,
            }),
            ("PE_ADD", _) | ("PLAYLIST", Some("add_file")) => {
                self.request(Request::Ask(Ask::AddSongs));
            }
            ("PLAYLIST", Some("add_url")) => self.request(Request::Ask(Ask::Url)),
            ("PLAYLIST", Some("add_dir")) => self.request(Request::Ask(Ask::AddFolders)),
            ("PLAYLIST", Some("list_load")) => self.request(Request::Ask(Ask::LoadList)),
            ("PLAYLIST", Some("list_save")) => self.request(Request::Ask(Ask::SaveList)),
            _ => {}
        }
    }
}
