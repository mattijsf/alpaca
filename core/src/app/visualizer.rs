// SPDX-License-Identifier: GPL-3.0-or-later

//! The visualizer as the app sees it: where it shows, which preset, and its keys.
//!
//! It shows in the skin where the skin has a place for it: a classic skin's visualizer window,
//! framed by `gen.bmp`, which the Visualizer command opens and shuts, or wherever a modern skin
//! holds Winamp's visualizer. Otherwise it shows in the Milkdrop window, the shell's own,
//! which a double click on the skin's visualizer also opens. While that window is open the
//! visualizer shows there and not in the skin.

use std::path::{Path, PathBuf};

use walkit::draw::Rect;

use super::{App, Request};
use crate::skin::VIS_CONTAINER;
use crate::vis;

/// Steps a skin's visualizer window has to show a place for the visualizer once it opens.
const OPENING_STEPS: u8 = 3;
/// The visualizer's area in a modern skin's visualizer window just opened is made at least
/// this large, in layout pixels: what a classic skin's shows at its smallest.
const SMALLEST_AREA: (i32, i32) = (256, 198);
/// The size a classic skin's visualizer window opens at the first time, in layout pixels: as
/// Winamp 5.666 opens its Milkdrop plug-in's with the classic base skin.
const WINAMP_VIS_WINDOW: (i32, i32) = (586, 475);

impl App {
    /// The shell opened the Milkdrop window, or it closed. When the visualizer starts showing,
    /// the presets of the folder the listener picked are read and the first one shows.
    pub fn vis_shown(&mut self, open: bool) {
        let was = self.vis_running();
        self.vis_open = open;
        self.tell_switches();
        self.request(Request::Menu);
        if self.vis_running() && !was {
            self.start_vis();
        }
        // the skin shows it again
        if !open && self.vis_area.is_some() {
            self.request(Request::VisArea);
        }
    }

    /// Whether the Milkdrop window is open.
    pub fn vis_open(&self) -> bool {
        self.vis_open
    }

    /// Whether the visualizer shows anywhere: in the skin or in the Milkdrop window.
    pub fn vis_running(&self) -> bool {
        self.vis_open || self.vis_area.is_some()
    }

    /// Where the visualizer shows in the skin: the window, by the engine's id, and the part of
    /// it in layout pixels from its top left corner. `None` while it shows in the Milkdrop
    /// window, or the skin shows none. The shell hears of a change by [`Request::VisArea`].
    pub fn vis_area(&self) -> Option<(u32, Rect)> {
        self.vis_area.filter(|_| !self.vis_open)
    }

    /// The Visualizer command: the skin's visualizer window opens or shuts, or, for a skin
    /// with none, the Milkdrop window.
    pub(super) fn toggle_visualizer(&mut self) {
        if self.vis_open {
            self.request(Request::Vis(false));
            return;
        }
        let showing = self.vis_area.is_some();
        let skin_has_one = self
            .skin
            .as_mut()
            .is_some_and(|skin| skin.show_vis_window(!showing));
        self.vis_window_opening = if skin_has_one && !showing {
            OPENING_STEPS
        } else {
            0
        };
        if !skin_has_one {
            // a modern skin that shows it where it shows it, or one with no place for it
            self.request(Request::Vis(true));
        }
    }

    /// Brings the visualizer's place in the skin up to date after a step that changed the
    /// windows. A skin that starts showing it takes it from the Milkdrop window, which closes.
    pub(super) fn follow_vis_area(&mut self) {
        let area = self.skin.as_mut().and_then(|skin| skin.vis_area());
        if area.is_none() && self.vis_window_opening > 0 {
            // a skin whose visualizer window shows no place for it, as Bento's at first: the
            // window shuts again, and the Milkdrop window opens
            self.vis_window_opening -= 1;
            if self.vis_window_opening == 0 {
                if let Some(skin) = self.skin.as_mut() {
                    skin.show_vis_window(false);
                }
                self.request(Request::Vis(true));
            }
        }
        if area == self.vis_area {
            // it may have moved, with its window
            self.keep_vis_window();
            return;
        }
        let was = self.vis_running();
        let appeared = area.is_some() && self.vis_area.is_none();
        self.vis_area = area;
        if appeared && std::mem::take(&mut self.vis_window_opening) > 0 {
            self.put_vis_window();
        } else {
            self.keep_vis_window();
        }
        if appeared && self.vis_open {
            self.request(Request::Vis(false));
        }
        if self.vis_running() != was {
            self.tell_switches();
            self.request(Request::Menu);
            if !was {
                self.start_vis();
            }
        }
        self.request(Request::VisArea);
    }

    /// The key that keeps where the skin's visualizer window is, from its main window's corner.
    fn vis_place_key(&self) -> String {
        format!("visat.{}", self.skin_name())
    }

    /// Where the skin's visualizer window is from the main window's top left corner, and its
    /// size, in layout pixels, when the visualizer shows in it; `None` when it shows elsewhere
    /// or not at all.
    fn vis_window_place(&mut self) -> Option<[i32; 4]> {
        let (window, _) = self.vis_area?;
        let skin = self.skin.as_mut()?;
        if skin.runtime.window_of(VIS_CONTAINER)?.0 != window {
            return None;
        }
        let main = skin.runtime.window_rect(skin.runtime.main_window()?);
        let at = skin
            .runtime
            .window_rect(crate::skin::Skin::window_id(window));
        Some([at.left - main.left, at.top - main.top, at.width, at.height])
    }

    /// Writes down where the visualizer window is and how large, for when it opens again.
    fn keep_vis_window(&mut self) {
        if let Some([dx, dy, w, h]) = self.vis_window_place() {
            let key = self.vis_place_key();
            self.settings.set(&key, Some(&format!("{dx},{dy},{w},{h}")));
        }
    }

    /// Puts the visualizer window just opened where it was last and at the size it had, or,
    /// the first time, against the main window's right edge, its tops level. A classic skin's
    /// opens at the size Winamp's opens at; a window that opens at its smallest, as Winamp3's
    /// does, is made large enough to see the visualizer in.
    fn put_vis_window(&mut self) {
        let kept: Option<Vec<i32>> = self.settings.get(&self.vis_place_key()).and_then(|text| {
            let numbers: Vec<i32> = text
                .split(',')
                .filter_map(|n| n.trim().parse().ok())
                .collect();
            (numbers.len() == 4).then_some(numbers)
        });
        let Some((window, area)) = self.vis_area else {
            return;
        };
        let Some(skin) = self.skin.as_mut() else {
            return;
        };
        let Some(main) = skin.runtime.main_window() else {
            return;
        };
        let main = skin.runtime.window_rect(main);
        let id = crate::skin::Skin::window_id(window);
        let (w, h) = skin.runtime.window_size(id);
        let (dx, dy, size) = match kept.as_deref() {
            Some(&[dx, dy, w, h]) => (dx, dy, (w, h)),
            _ if skin.classic => (main.width, 0, WINAMP_VIS_WINDOW),
            _ => (
                main.width,
                0,
                (
                    w + (SMALLEST_AREA.0 - area.width).max(0),
                    h + (SMALLEST_AREA.1 - area.height).max(0),
                ),
            ),
        };
        if size != (w, h) {
            skin.runtime.resize_window(id, size.0, size.1);
        }
        skin.place_window(window, main.left + dx, main.top + dy);
        // kept in view at the next step, as after a drag
        self.unsettled = true;
    }

    /// The visualizer starts showing: the presets are read and the first one shows.
    fn start_vis(&mut self) {
        let kept = || {
            self.settings
                .get("presets")
                .map(PathBuf::from)
                .filter(|folder| folder.exists())
        };
        if let Some(folder) = self.options.presets.clone().or_else(kept) {
            self.presets.read(&folder);
        }
        self.presets.random = self.vis_random();
        self.show_preset(false);
    }

    pub(super) fn show_preset(&mut self, blend: bool) {
        if let Some(file) = self.presets.current() {
            let request = Request::Preset {
                file: file.to_path_buf(),
                title: self.presets.title(),
                blend,
            };
            self.request(request);
        }
    }

    /// Whether the preset after a stay is one at random, as the listener set it.
    pub fn vis_random(&self) -> bool {
        self.settings.get("milk.random") == Some("1")
    }

    /// Picks the preset after a stay at random, or the next by name; kept for the next run.
    pub(super) fn set_vis_random(&mut self, on: bool) {
        self.settings
            .set("milk.random", Some(if on { "1" } else { "0" }));
        self.presets.random = on;
        self.tell_switches();
        self.request(Request::Menu);
    }

    /// The skin's window the visualizer has to itself, by the engine's id: a classic skin's
    /// visualizer window, or a modern skin's (Winamp Modern's); not one the visualizer shares
    /// with the player, as Winamp Modern's drawer.
    pub(super) fn vis_window(&mut self) -> Option<u32> {
        let (window, _) = self.vis_area()?;
        let own = self.skin.as_mut()?.runtime.window_of(VIS_CONTAINER)?.0;
        (own == window).then_some(window)
    }

    /// A key for the visualizer, by its name ([`vis::Presets::key`]). False for a key that is
    /// not one of its own.
    pub fn vis_key(&mut self, key: &str) -> bool {
        match self.presets.key(key) {
            vis::Key::Changed => self.show_preset(true),
            vis::Key::Taken => {}
            vis::Key::Other => return false,
        }
        true
    }

    /// A preset's stay ([`vis::STAY_SECONDS`]) is over: the next one blends in, unless the
    /// listener holds the one shown.
    pub fn vis_tick(&mut self) {
        if self.presets.pass() {
            self.show_preset(true);
        }
    }

    /// The listener picked a folder of presets: they show, the first one now.
    pub(super) fn presets_picked(&mut self, folder: &Path) {
        self.presets.read(folder);
        if self.vis_running() {
            self.show_preset(false);
        } else {
            self.toggle_visualizer();
        }
    }
}
