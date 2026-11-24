// SPDX-License-Identifier: GPL-3.0-or-later

//! The Visualizer window as the app sees it: open or shut, and which preset shows.

use std::path::PathBuf;

use super::{App, Request};
use crate::vis;

impl App {
    /// The shell opened the Visualizer window, or it closed. When it opens, the presets of
    /// the folder the listener picked are read and the first one shows.
    pub fn vis_shown(&mut self, open: bool) {
        self.vis_open = open;
        self.tell_switches();
        self.request(Request::Menu);
        if open {
            let kept = || {
                self.settings
                    .get("presets")
                    .map(PathBuf::from)
                    .filter(|folder| folder.exists())
            };
            if let Some(folder) = self.options.presets.clone().or_else(kept) {
                self.presets.read(&folder);
            }
            self.show_preset(false);
        }
    }

    /// Whether the Visualizer window is open.
    pub fn vis_open(&self) -> bool {
        self.vis_open
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

    /// A key in the Visualizer window, by its name ([`vis::Presets::key`]). False for a key that
    /// is not one of the window's.
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
}
