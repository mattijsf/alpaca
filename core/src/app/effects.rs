// SPDX-License-Identifier: GPL-3.0-or-later

//! The effect plug-ins: those that come with the app and the listener's own.

use std::path::{Path, PathBuf};

use super::App;

impl App {
    /// Where the listener's own plug-ins are kept: in the app's folder, beside its settings.
    pub(super) fn plugins_folder(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|config| config.join("Plugins"))
    }

    /// Loads every plug-in once, those that come with the app and the listener's own, and
    /// puts each back as the listener left it. A file that is not a plug-in is named on the
    /// standard error, with the reason.
    pub fn load_effects(&mut self) {
        if self.effects_loaded {
            return;
        }
        self.effects_loaded = true;
        let mut player = self.player.borrow_mut();
        for (folder, mine) in [
            (Some(self.resources.join("plugins")), false),
            (self.plugins_folder(), true),
        ] {
            let Some(folder) = folder else {
                continue;
            };
            for (name, added) in player.rack.add_folder(&folder) {
                match added {
                    Ok(index) if mine => {
                        self.own
                            .insert(player.rack.effects()[index].plugin.info.id.clone());
                    }
                    Ok(_) => {}
                    Err(reason) => eprintln!("alpaca: {name}: {reason}"),
                }
            }
        }
        if let Some(left) = self.settings.get("effects") {
            player.rack.restore(left);
        }
    }

    /// Switches on the effect with the id or the name `id`, for the command line.
    pub(super) fn switch_on(&mut self, id: &str) {
        self.load_effects();
        let mut player = self.player.borrow_mut();
        let found = player.rack.effects().iter().position(|e| {
            // the whole id, its last word, or the name as the rack shows it
            let info = &e.plugin.info;
            info.id == id
                || info.id.rsplit('.').next() == Some(id)
                || info.name.eq_ignore_ascii_case(id)
        });
        match found {
            Some(index) => player.rack.set_on(index, true),
            None => eprintln!("alpaca: no effect {id}"),
        }
    }

    /// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder
    /// under its id, so a newer version takes the older one's place. Gives its place in the
    /// rack, or why it is not a plug-in the app can run.
    pub fn add_plugin(&mut self, file: &Path) -> Result<usize, String> {
        self.load_effects();
        let source = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        let (index, id) = {
            let mut player = self.player.borrow_mut();
            let index = player.rack.add(&source)?;
            (index, player.rack.effects()[index].plugin.info.id.clone())
        };
        if let Some(folder) = self.plugins_folder() {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join(format!("{id}.lua")), source);
        }
        self.own.insert(id);
        self.keep_effects();
        Ok(index)
    }

    /// Whether effect `index` of the rack is a plug-in the listener added.
    pub fn is_own(&self, index: usize) -> bool {
        let player = self.player.borrow();
        player
            .rack
            .effects()
            .get(index)
            .is_some_and(|e| self.own.contains(&e.plugin.info.id))
    }

    /// Takes a plug-in the listener added out of the rack and out of the app's folder.
    pub fn remove_plugin(&mut self, index: usize) {
        if !self.is_own(index) {
            return;
        }
        let id = {
            let mut player = self.player.borrow_mut();
            let id = player.rack.effects()[index].plugin.info.id.clone();
            player.rack.remove(index);
            id
        };
        self.own.remove(&id);
        if let Some(folder) = self.plugins_folder() {
            let _ = std::fs::remove_file(folder.join(format!("{id}.lua")));
        }
    }

    /// Writes down which effects are on and where their controls stand.
    pub(super) fn keep_effects(&mut self) {
        if self.effects_loaded {
            let state = self.player.borrow().rack.state();
            self.settings.set("effects", Some(&state));
        }
    }
}
