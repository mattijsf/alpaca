// SPDX-License-Identifier: GPL-3.0-or-later

//! The skin worn: wearing one, where its windows are, running it and answering its buttons.

use std::path::Path;

use super::screens::{self, Area};
use super::{command, number_of, pixel, App, Ask, Menu, MenuHandler, Request, DEFAULT_SKIN};
use crate::host::HostAction;
use crate::player::{SkinPlayer, REQUEST_OPEN_FILES};
use crate::skin::{self, Skin};

impl App {
    /// The skin worn.
    pub fn skin(&mut self) -> Option<&mut Skin> {
        self.skin.as_deref_mut()
    }

    /// The engine's id of the skin's main window; `None` when no skin is worn or it has none.
    pub fn main_window(&self) -> Option<u32> {
        self.skin.as_ref()?.runtime.main_window().map(|id| id.0)
    }

    /// Window `window` of the skin, by the engine's id, is the one the keys go to now, by
    /// another way than a press on it: the skin shows it as the one in front, and the window
    /// that was as one behind.
    pub fn focus(&mut self, window: u32) {
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.focus_window(Skin::window_id(window));
        }
    }

    /// The file name of the skin worn.
    pub fn skin_name(&self) -> String {
        self.worn
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The viewport in layout pixels: the part of the screen the main window is on, from
    /// [`App::origin`].
    pub(super) fn viewport(&self) -> (i32, i32) {
        let (x, y) = self.origin();
        (
            (self.view.x + self.view.w - x).div_euclid(self.zoom).max(1),
            (self.view.y + self.view.h - y).div_euclid(self.zoom).max(1),
        )
    }

    /// Every window's top left corner on the desktop in points, by the engine's id, as the
    /// last step left them.
    pub fn places(&self) -> Vec<(u32, (i32, i32))> {
        let (ox, oy) = self.origin();
        let windows = self.skin.as_ref().map_or(&[][..], |skin| skin.windows());
        windows
            .iter()
            .map(|w| {
                (
                    w.id,
                    (ox + w.rect.left * self.zoom, oy + w.rect.top * self.zoom),
                )
            })
            .collect()
    }

    /// Every window on the desktop, in points, by the engine's id, as the engine has them
    /// now, and which of them is the main window.
    fn on_desktop(&mut self) -> (Vec<(u32, Area)>, usize) {
        let (zoom, (ox, oy)) = (self.zoom, self.origin());
        let Some(skin) = self.skin.as_mut() else {
            return (Vec::new(), 0);
        };
        let main = skin.runtime.main_window();
        let ids = skin.runtime.window_ids();
        let windows: Vec<(u32, Area)> = ids
            .iter()
            .map(|id| {
                let r = skin.runtime.window_rect(*id);
                let area = Area::new(
                    ox + r.left * zoom,
                    oy + r.top * zoom,
                    r.width * zoom,
                    r.height * zoom,
                );
                (id.0, area)
            })
            .collect();
        let main = main
            .and_then(|main| windows.iter().position(|(id, _)| *id == main.0))
            .unwrap_or(0);
        (windows, main)
    }

    /// Puts the windows `at` places on the desktop, and the viewport on the screen of `view`.
    /// `was` is where they are now.
    fn put(&mut self, was: &[(u32, Area)], at: &[Area], view: Area) {
        let moved = view != self.view;
        self.view = view;
        let (zoom, (ox, oy)) = (self.zoom, self.origin());
        let (w, h) = self.viewport();
        let Some(skin) = self.skin.as_mut() else {
            return;
        };
        skin.set_viewport(w, h);
        for ((id, before), area) in was.iter().zip(at) {
            // a new viewport counts every window from another corner
            if moved || before != area {
                skin.place_window(*id, pixel(area.x - ox, zoom), pixel(area.y - oy, zoom));
            }
        }
        // the engine has a window's new place after a step: the places are asked for next
        skin.step();
    }

    /// Keeps every window where a screen shows it, and the viewport on the screen the main
    /// window is on, so that it snaps to that screen's edges and a window opened later opens
    /// there. Nothing is done while the mouse is down. Says whether a window was put
    /// somewhere else.
    pub(super) fn settle(&mut self) -> bool {
        if self.pressed_at.is_some() || self.release.is_some() {
            return false;
        }
        self.unsettled = false;
        let before = std::mem::take(&mut self.before);
        let (windows, main) = self.on_desktop();
        if windows.is_empty() {
            self.view = self.screens[0];
            return false;
        }
        let now: Vec<Area> = windows.iter().map(|(_, area)| *area).collect();
        let at = screens::into_view(&now, main, &self.screens, &before);
        let view = screens::screen_of(at[main], &self.screens)
            .map_or(self.screens[0], |i| self.screens[i]);
        if at == now && view == self.view {
            return false;
        }
        self.put(&windows, &at, view);
        true
    }

    /// The main window's corner on the desktop, in points.
    pub(super) fn main_place(&mut self) -> Option<(i32, i32)> {
        let (zoom, (ox, oy)) = (self.zoom, self.origin());
        let skin = self.skin.as_mut()?;
        let main = skin.runtime.main_window()?;
        let rect = skin.runtime.window_rect(main);
        Some((ox + rect.left * zoom, oy + rect.top * zoom))
    }

    /// Writes down where the player is, for every skin: the main window's corner on the
    /// desktop in points, and the screen it is on, so that a screen gone or moved is seen as
    /// such. And how the skin worn sets its windows out around its main window, for when it is
    /// worn again: each one's corner, and the size they were kept at, since the windows are
    /// that many points apart for each of the skin's pixels.
    pub(super) fn keep_places(&mut self) {
        let places = self.places();
        if places.is_empty() {
            return;
        }
        let prefix = format!("at.{}.", self.skin_name());
        self.settings.clear(&prefix);
        for &(id, (x, y)) in &places {
            self.settings
                .set(&format!("{prefix}{id}"), Some(&format!("{x},{y}")));
        }
        self.settings
            .set(&format!("{prefix}zoom"), Some(&self.zoom.to_string()));
        let main = self.main_window();
        if let Some((_, (x, y))) = places.iter().find(|(id, _)| Some(*id) == main) {
            self.settings.set("corner", Some(&format!("{x},{y}")));
            let Area { x, y, w, h } = self.view;
            self.settings
                .set("screen", Some(&format!("{x},{y},{w},{h}")));
        }
    }

    /// Writes down what the skin worn keeps for itself: what its scripts wrote down (a
    /// drawer left open, a pane's width, the tab shown) and its color theme, for when it is
    /// worn again.
    pub(super) fn keep_skin_state(&mut self) {
        let Some(skin) = self.skin.as_ref() else {
            return;
        };
        let kept = skin.kept();
        let prefix = format!("kept.{}.", self.skin_name());
        self.settings.clear(&prefix);
        self.settings
            .set(&format!("{prefix}theme"), kept.theme.as_deref());
        for (name, value) in &kept.settings.attributes {
            self.settings.set(&format!("{prefix}a.{name}"), Some(value));
        }
        for (name, value) in &kept.settings.strings {
            self.settings.set(&format!("{prefix}s.{name}"), Some(value));
        }
        for (name, value) in &kept.settings.ints {
            self.settings
                .set(&format!("{prefix}i.{name}"), Some(&value.to_string()));
        }
    }

    /// What the skin with the file name `name` kept when it was last worn; nothing for one
    /// never worn.
    fn kept_skin_state(&self, name: &str) -> skin::Kept {
        let prefix = format!("kept.{name}.");
        let mut kept = skin::Kept::default();
        for key in self.settings.keys(&prefix) {
            let Some(value) = self.settings.get(key) else {
                continue;
            };
            let rest = &key[prefix.len()..];
            if rest == "theme" {
                kept.theme = Some(value.to_string());
            } else if let Some(name) = rest.strip_prefix("a.") {
                kept.settings
                    .attributes
                    .insert(name.to_string(), value.to_string());
            } else if let Some(name) = rest.strip_prefix("s.") {
                kept.settings
                    .strings
                    .insert(name.to_string(), value.to_string());
            } else if let Some(name) = rest.strip_prefix("i.") {
                if let Ok(value) = value.parse() {
                    kept.settings.ints.insert(name.to_string(), value);
                }
            }
        }
        kept
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
        // under a modern one Andamp's element set; under both Arial and Tahoma
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
        let fonts = self.fonts.clone();
        let player = self.player.clone();
        // the skin worn goes: what it kept is written down first, also for when the skin
        // opened is the same one again
        if self.skin.is_some() {
            self.keep_skin_state();
        }
        let kept = self.kept_skin_state(&name);
        let mut skin = Skin::open(
            bytes,
            &name,
            base,
            wasabi,
            &fonts,
            file.parent(),
            self.viewport(),
            Some(&kept),
            |_| Box::new(SkinPlayer(player)),
        )?;
        skin.player = Some(self.player.clone());
        // its places are kept too, as the engine has them now: a window moved since the last
        // step shows at its new place after the engine's next one
        if let Some(old) = self.skin.as_mut() {
            old.step();
            self.keep_places();
            self.keep_switches();
            self.main_corner = self.main_place();
        }
        self.skin = Some(Box::new(skin));
        self.worn = Some(file.to_path_buf());
        // the new skin's list starts at its top, and is brought to the current song again
        self.followed = None;
        self.give_menu_handler();
        self.tell_switches();
        self.apply_switches();

        self.put_back_places(&name);
        self.request(Request::Skin);
        self.request(Request::Menu);
        Ok(())
    }

    /// Puts the windows of the skin just worn, the one with the file name `name`: its main
    /// window where the player was, whatever skin it was then, in this run or the last; the
    /// others as far from it in the skin's own pixels as they were when this skin was last
    /// worn, whatever the size was then (in points they would be twice as far apart, or half,
    /// after a change of the size), else as the skin sets them out. The first start opens a
    /// little in from the main screen's top left corner. Then every window is kept where a
    /// screen shows it.
    fn put_back_places(&mut self, name: &str) {
        let prefix = format!("at.{name}.");
        let kept: Vec<(u32, (i32, i32))> = self
            .settings
            .keys(&prefix)
            .into_iter()
            .filter_map(|key| {
                let id = key[prefix.len()..].parse().ok()?;
                Some((id, parse_point(self.settings.get(key)?)?))
            })
            .collect();
        let kept_zoom = number_of(&self.settings, &format!("{prefix}zoom"))
            .filter(|zoom| *zoom >= 1)
            .unwrap_or(1);
        let zoom = self.zoom;
        let main_screen = self.screens[0];
        let (windows, main) = self.on_desktop();
        let Some(&(main_id, anchor)) = windows.get(main) else {
            return;
        };
        let kept_main = kept
            .iter()
            .find(|(id, _)| *id == main_id)
            .map(|(_, at)| *at);
        let (corner, before) = match self.main_corner.take() {
            Some(corner) => (Some(corner), Vec::new()),
            None => self.kept_corner(name, main_id),
        };
        let corner = corner.unwrap_or((main_screen.x + 120, main_screen.y + 80));
        let at: Vec<Area> = windows
            .iter()
            .map(|(id, area)| {
                let kept = kept.iter().find(|(kept, _)| kept == id).map(|(_, at)| *at);
                let (x, y) = match (kept_main, kept) {
                    (Some(from), Some(at)) => (
                        corner.0 + pixel(at.0 - from.0, kept_zoom) * zoom,
                        corner.1 + pixel(at.1 - from.1, kept_zoom) * zoom,
                    ),
                    // where the skin sets it out from the main window
                    _ => (corner.0 + area.x - anchor.x, corner.1 + area.y - anchor.y),
                };
                Area::new(x, y, area.w, area.h)
            })
            .collect();
        let at = screens::into_view(&at, main, &self.screens, &before);
        let view =
            screens::screen_of(at[main], &self.screens).map_or(main_screen, |i| self.screens[i]);
        self.put(&windows, &at, view);
        self.unsettled = false;
        self.before.clear();
    }

    /// Where the last run left the player: the main window's corner on the desktop, and the
    /// screen it was on. Settings from before the place was one for every skin have it by
    /// skin, under the skin `name` with the main window `main`; those from before the screens
    /// were known count from the main screen's top left corner.
    fn kept_corner(&self, name: &str, main: u32) -> (Option<(i32, i32)>, Vec<Area>) {
        let screen = |key: &str| self.settings.get(key).and_then(parse_area);
        if let Some(corner) = self.settings.get("corner").and_then(parse_point) {
            return (Some(corner), screen("screen").into_iter().collect());
        }
        let prefix = format!("at.{name}.");
        let kept = screen(&format!("{prefix}screen"));
        let (sx, sy) = match kept {
            Some(_) => (0, 0),
            None => (self.screens[0].x, self.screens[0].y),
        };
        let corner = self
            .settings
            .get(&format!("{prefix}{main}"))
            .and_then(parse_point)
            .map(|(x, y)| (x + sx, y + sy));
        (corner, kept.into_iter().collect())
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
        self.let_go_when_due();
        let mut flags = self.skin.as_mut().map_or(0, |skin| skin.step());
        // after a drag, or when the screens changed: the windows are kept in view, stepped
        // again where they were put elsewhere, so the shell has their places now
        if (flags & skin::MOVED != 0 || self.unsettled) && self.settle() {
            flags |= skin::MOVED;
        }
        if self.playlist_due == 2 {
            self.playlist_due = 1;
        }
        self.follow_the_song();
        if self.player.borrow_mut().take_requests() & REQUEST_OPEN_FILES != 0 {
            self.request(Request::Ask(Ask::OpenSongs));
        }
        let actions = self
            .skin
            .as_mut()
            .map(|skin| skin.take_actions())
            .unwrap_or_default();
        for action in actions {
            self.asked(&action);
        }
        flags
    }

    /// Milliseconds until the app next has something to do by itself, and so until the next
    /// [`App::step`]: 0 for now, -1 for never. It is the skin's own clock, now while the
    /// playlist waits to be put back, and the moment a release held back is told.
    pub fn next_due_ms(&mut self) -> i64 {
        if self.playlist_due > 0 {
            return 0;
        }
        let skin = self.skin.as_mut().map_or(-1, |skin| skin.next_due_ms());
        // a release held back is told when its press has shown long enough
        match self.release_due_ms() {
            Some(release) if skin < 0 || release < skin => release,
            _ => skin,
        }
    }

    /// Answers something a button of the skin asked of the app: `action` is what the engine
    /// handed its host, by Winamp's names.
    pub fn asked(&mut self, action: &HostAction) {
        let (ox, oy) = self.origin();
        let at = action
            .at
            .map(|(x, y)| (ox + x * self.zoom, oy + y * self.zoom));
        let popup = |menu: Menu| Request::Popup { menu, at };
        match (action.name.as_str(), action.param.as_deref()) {
            ("CLOSE", _) => self.request(Request::Quit),
            ("MINIMIZE", _) => self.request(Request::Hide),
            ("DOUBLESIZE", _) => self.command(command::DOUBLE_SIZE),
            ("ALWAYS_ON_TOP", _) => self.command(command::ON_TOP),
            // the clutter bar's O, I and V
            ("MENU", Some("wa5:options")) => self.request(popup(Menu::Options)),
            ("TRACKINFO", _) => {
                let player = self.player.borrow();
                let row = player.index();
                let any = !player.entries().is_empty();
                drop(player);
                if any {
                    self.request(Request::Info(row));
                }
            }
            ("VIS_MENU", _) => self.request(popup(Menu::Visualization)),
            // a double click on the visualizer asks for the full one
            ("VIS_FS", _) => {
                if !self.vis_open {
                    self.request(Request::Vis(true));
                }
            }
            ("MENU" | "SYSMENU" | "CONTROLMENU", _) => self.request(popup(Menu::Popup)),
            ("ABOUT", _) => self.request(Request::About),
            ("TOGGLE" | "SHOW", Some("vis")) => self.command(command::VISUALIZER),
            ("EQ_PRESETS", _) => self.request(popup(Menu::Equalizer)),
            // a modern skin's playlist buttons: the app shows the menu for each
            ("PE_ADD", _) => self.request(popup(Menu::PlaylistAdd)),
            ("PE_REM", _) => self.request(popup(Menu::PlaylistRemove)),
            ("PE_SEL", _) => self.request(popup(Menu::PlaylistSelect)),
            ("PE_MISC", _) => self.request(popup(Menu::PlaylistMisc)),
            ("PE_LIST", _) => self.request(popup(Menu::PlaylistList)),
            // a right click on a row: the menu is about that row, unless it is one of several
            // selected
            ("PLAYLIST_MENU", row) => {
                if let Some(row) = row.and_then(|row| row.trim().parse::<usize>().ok()) {
                    if !self.selection().contains(&row) {
                        self.select(&[row]);
                    }
                }
                self.request(popup(Menu::PlaylistRow));
            }
            // a classic playlist's bottom buttons: the skin drew the menu, and this is the pick
            ("PLAYLIST", Some(pick)) => match pick {
                "add_file" => self.command(command::ADD_FILES),
                "add_dir" => self.command(command::ADD_FOLDER),
                "add_url" => self.command(command::ADD_URL),
                "remove_selected" => self.command(command::REMOVE_SELECTED),
                "crop" => self.command(command::CROP),
                "remove_all" | "list_new" => self.command(command::CLEAR),
                "remove_misc" => self.request(popup(Menu::RemoveMisc)),
                "select_all" => self.command(command::SELECT_ALL),
                "select_none" => self.command(command::SELECT_NONE),
                "select_invert" => self.command(command::SELECT_INVERT),
                "sort" => self.request(popup(Menu::Sort)),
                "file_info" => self.command(command::FILE_INFO),
                "misc_options" => self.request(popup(Menu::MiscOptions)),
                "list_load" => self.command(command::LOAD_LIST),
                "list_save" => self.command(command::SAVE_LIST),
                _ => {}
            },
            _ => {}
        }
    }
}

/// A place kept as `x,y`.
fn parse_point(text: &str) -> Option<(i32, i32)> {
    let (x, y) = text.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// A rectangle kept as `x,y,w,h`.
fn parse_area(text: &str) -> Option<Area> {
    let mut parts = text.split(',').map(|part| part.trim().parse::<i32>().ok());
    let mut next = || parts.next().flatten();
    Some(Area::new(next()?, next()?, next()?, next()?))
}
