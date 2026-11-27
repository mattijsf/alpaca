// SPDX-License-Identifier: GPL-3.0-or-later

//! The preferences: what the listener can set, spelled out for a shell to draw as a window
//! of pages and rows, and what setting each one does.
//!
//! A shell draws the rows by their kind and knows nothing of what they mean: a `toggle` is a
//! switch, a `choice` a list of options, a `slider` a number in a range, an `action` a button,
//! a `text` something to read and a `heading` a title over the rows after it. A change goes
//! back through [`App::set_preference`], and the shell then asks for the pages again.

use std::path::PathBuf;

use walkit::classic::vis::{AnalyzerStyle, ScopeStyle, VisMode, VisStyle};

use super::{command, App, Ask, Request};
use crate::dsp::describe::json_text;
use crate::vis;

/// One row of a page, as JSON.
fn row(id: &str, kind: &str, label: &str, value: &str, more: &str) -> String {
    format!(
        "{{\"id\":{},\"kind\":\"{kind}\",\"label\":{},\"value\":{}{more}}}",
        json_text(id),
        json_text(label),
        json_text(value)
    )
}

fn heading(label: &str) -> String {
    row("", "heading", label, "", "")
}

fn toggle(id: &str, label: &str, on: bool) -> String {
    row(id, "toggle", label, if on { "1" } else { "0" }, "")
}

fn action(id: &str, label: &str) -> String {
    row(id, "action", label, "", "")
}

fn text(label: &str, value: &str) -> String {
    row("", "text", label, value, "")
}

/// A list of options, each a name and the value it stands for.
fn choice(id: &str, label: &str, value: &str, options: &[(&str, &str)]) -> String {
    let options: Vec<String> = options
        .iter()
        .map(|(name, value)| {
            format!(
                "{{\"name\":{},\"value\":{}}}",
                json_text(name),
                json_text(value)
            )
        })
        .collect();
    row(
        id,
        "choice",
        label,
        value,
        &format!(",\"options\":[{}]", options.join(",")),
    )
}

fn slider(id: &str, label: &str, value: i32, range: (i32, i32, i32), unit: &str) -> String {
    let (min, max, step) = range;
    let more = format!(
        ",\"min\":{min},\"max\":{max},\"step\":{step},\"unit\":{}",
        json_text(unit)
    );
    row(id, "slider", label, &value.to_string(), &more)
}

impl App {
    // what a classic skin's own switches are set to, kept in the settings

    /// What a classic skin's visualizer shows.
    pub fn vis_mode(&self) -> VisMode {
        match self.settings.get("vis.mode") {
            Some("scope") => VisMode::Scope,
            Some("off") => VisMode::Off,
            _ => VisMode::Analyzer,
        }
    }

    /// Sets what a classic skin's visualizer shows.
    pub fn set_vis_mode(&mut self, mode: VisMode) {
        let name = match mode {
            VisMode::Analyzer => "analyzer",
            VisMode::Scope => "scope",
            VisMode::Off => "off",
        };
        self.settings.set("vis.mode", Some(name));
        self.apply_switches();
        self.request(Request::Menu);
    }

    /// How a classic skin's analyzer and oscilloscope draw.
    fn vis_style(&self) -> VisStyle {
        let get = |key: &str| self.settings.get(key);
        VisStyle {
            analyzer: match get("vis.analyzer") {
                Some("fire") => AnalyzerStyle::Fire,
                Some("line") => AnalyzerStyle::Line,
                _ => AnalyzerStyle::Normal,
            },
            thin_bars: get("vis.thin") == Some("1"),
            peaks: get("vis.peaks") != Some("0"),
            scope: match get("vis.scope") {
                Some("dots") => ScopeStyle::Dots,
                Some("solid") => ScopeStyle::Solid,
                _ => ScopeStyle::Lines,
            },
        }
    }

    /// Whether a classic skin shows the time left of the song, not the time played.
    pub fn time_remaining(&self) -> bool {
        self.settings.get("time.remaining") == Some("1")
    }

    /// Shows the time left of the song, or the time played.
    pub fn set_time_remaining(&mut self, on: bool) {
        self.settings
            .set("time.remaining", Some(if on { "1" } else { "0" }));
        self.apply_switches();
        self.request(Request::Menu);
    }

    /// Sets the skin worn as the settings say: its visualizer and the time it shows.
    pub(super) fn apply_switches(&mut self) {
        let (mode, style, left) = (self.vis_mode(), self.vis_style(), self.time_remaining());
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.set_classic_vis_mode(mode);
            skin.runtime.set_classic_vis_style(style);
            skin.runtime.set_classic_time_remaining(left);
            skin.refresh();
        }
    }

    /// Writes down what the listener set on the skin itself: a click on the visualizer goes
    /// to its next mode, a click on the time turns it round.
    pub(super) fn keep_switches(&mut self) {
        let Some(skin) = self.skin.as_ref() else {
            return;
        };
        let mode = match skin.runtime.classic_vis_mode() {
            VisMode::Analyzer => "analyzer",
            VisMode::Scope => "scope",
            VisMode::Off => "off",
        };
        let left = if skin.runtime.classic_time_remaining() {
            "1"
        } else {
            "0"
        };
        self.settings.set("vis.mode", Some(mode));
        self.settings.set("time.remaining", Some(left));
    }

    // the Milkdrop window's settings

    /// Seconds a Milkdrop preset stays before the next one blends in.
    pub fn vis_stay(&self) -> i32 {
        let kept = self.settings.get("milk.stay").and_then(|v| v.parse().ok());
        kept.unwrap_or(vis::STAY_SECONDS as i32).clamp(5, 600)
    }

    /// Frames a second the Milkdrop window draws.
    pub fn vis_fps(&self) -> i32 {
        match self.settings.get("milk.fps") {
            Some("60") => 60,
            _ => 30,
        }
    }

    /// The folder of Milkdrop presets the listener picked.
    fn presets_folder(&self) -> Option<PathBuf> {
        self.options
            .presets
            .clone()
            .or_else(|| self.settings.get("presets").map(PathBuf::from))
    }

    // the window

    /// The preferences as JSON: `{"pages": [...]}`, each page a `title` and its `rows`. A row
    /// has an `id`, a `kind` (`toggle`, `choice`, `slider`, `action`, `text` or `heading`), a
    /// `label` and a `value` as text: `1` or `0` for a toggle, an option's value for a
    /// choice, a number for a slider. A choice also has its `options`, each a `name` and a
    /// `value`; a slider its `min`, `max`, `step` and `unit`.
    pub fn preferences(&self) -> String {
        let player = self.player.borrow();
        let worn = self.skin_name();
        let skins = self.skins();
        let mut skin_options: Vec<(String, String)> = skins
            .iter()
            .map(|file| {
                let name = std::path::Path::new(file)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned());
                (name.unwrap_or_default(), file.clone())
            })
            .collect();
        if !worn.is_empty() && !skins.contains(&worn) {
            // a skin of the listener's own, picked from a file
            skin_options.push((worn.clone(), worn.clone()));
        }
        let skin_options: Vec<(&str, &str)> = skin_options
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let style = self.vis_style();
        let folder = |path: Option<PathBuf>| path.map(|p| p.to_string_lossy().into_owned());

        let look = [
            heading("Skin"),
            choice("skin", "Skin", &worn, &skin_options),
            action("skin.open", "Open Skin File…"),
            action("skins.reveal", "Show Skins Folder"),
            heading("Windows"),
            toggle("double", "Double size", self.zoom > 1),
            toggle("ontop", "Always on top", self.on_top),
            slider("opacity", "Opacity", self.opacity, (40, 100, 10), "%"),
        ];
        let playback = [
            heading("Playlist"),
            toggle("shuffle", "Shuffle", player.shuffle()),
            toggle("repeat", "Repeat", player.repeat()),
            heading("Time"),
            choice(
                "time",
                "Show (classic skins)",
                if self.time_remaining() {
                    "remaining"
                } else {
                    "elapsed"
                },
                &[("Time elapsed", "elapsed"), ("Time remaining", "remaining")],
            ),
        ];
        let mode = match self.vis_mode() {
            VisMode::Analyzer => "analyzer",
            VisMode::Scope => "scope",
            VisMode::Off => "off",
        };
        let visualizer = [
            heading("In a classic skin"),
            choice(
                "vis.mode",
                "Shows",
                mode,
                &[
                    ("Spectrum analyzer", "analyzer"),
                    ("Oscilloscope", "scope"),
                    ("Nothing", "off"),
                ],
            ),
            choice(
                "vis.analyzer",
                "Analyzer colors",
                match style.analyzer {
                    AnalyzerStyle::Normal => "normal",
                    AnalyzerStyle::Fire => "fire",
                    AnalyzerStyle::Line => "line",
                },
                &[("Normal", "normal"), ("Fire", "fire"), ("Line", "line")],
            ),
            choice(
                "vis.thin",
                "Analyzer bars",
                if style.thin_bars { "1" } else { "0" },
                &[("Thick", "0"), ("Thin", "1")],
            ),
            toggle("vis.peaks", "Analyzer peaks", style.peaks),
            choice(
                "vis.scope",
                "Oscilloscope",
                match style.scope {
                    ScopeStyle::Dots => "dots",
                    ScopeStyle::Lines => "lines",
                    ScopeStyle::Solid => "solid",
                },
                &[("Dots", "dots"), ("Lines", "lines"), ("Solid", "solid")],
            ),
            heading("Milkdrop window"),
            toggle("milk.open", "Show the Visualizer window", self.vis_open),
            text(
                "Presets folder",
                &folder(self.presets_folder()).unwrap_or_else(|| "None chosen".into()),
            ),
            action("milk.choose", "Choose Presets Folder…"),
            toggle("milk.random", "Pick presets at random", self.vis_random()),
            slider(
                "milk.stay",
                "A preset stays",
                self.vis_stay(),
                (5, 120, 5),
                "s",
            ),
            choice(
                "milk.fps",
                "Frames a second",
                &self.vis_fps().to_string(),
                &[("30", "30"), ("60", "60")],
            ),
        ];
        let effects = [
            action("effects.open", "Show Effects…"),
            text(
                "Your plug-ins",
                &folder(self.plugins_folder()).unwrap_or_else(|| "No folder on this system".into()),
            ),
            action("plugins.reveal", "Show Plug-ins Folder"),
        ];
        let about = [
            text("Alpaca", env!("CARGO_PKG_VERSION")),
            text(
                "Settings",
                &folder(self.config.clone()).unwrap_or_else(|| "Not kept on this system".into()),
            ),
            action("settings.reveal", "Show Settings Folder"),
        ];
        let page = |title: &str, rows: &[String]| {
            format!(
                "{{\"title\":{},\"rows\":[{}]}}",
                json_text(title),
                rows.join(",")
            )
        };
        format!(
            "{{\"pages\":[{},{},{},{},{}]}}",
            page("Look", &look),
            page("Playback", &playback),
            page("Visualizer", &visualizer),
            page("Effects", &effects),
            page("About", &about)
        )
    }

    /// Sets the preference `id` to `value` as [`App::preferences`] spells values out, or does
    /// the action `id`. What it does not know it leaves alone.
    pub fn set_preference(&mut self, id: &str, value: &str) {
        let on = value == "1";
        match id {
            "skin" => {
                if let Some(file) = self.skin_file(value) {
                    self.wear(&file);
                }
            }
            "skin.open" => self.command(command::OPEN_SKIN),
            "double" => self.set_zoom(if on { 2 } else { 1 }),
            "ontop" => self.set_on_top(on),
            "opacity" => {
                if let Ok(percent) = value.parse::<f64>() {
                    self.set_opacity(percent.round() as i32);
                }
            }
            "shuffle" => {
                if self.player.borrow().shuffle() != on {
                    self.command(command::SHUFFLE);
                }
            }
            "repeat" => {
                if self.player.borrow().repeat() != on {
                    self.command(command::REPEAT);
                }
            }
            "time" => self.set_time_remaining(value == "remaining"),
            "vis.mode" => self.set_vis_mode(match value {
                "scope" => VisMode::Scope,
                "off" => VisMode::Off,
                _ => VisMode::Analyzer,
            }),
            "vis.analyzer" | "vis.thin" | "vis.peaks" | "vis.scope" => {
                self.settings.set(id, Some(value));
                self.apply_switches();
            }
            "milk.random" => self.set_vis_random(on),
            "milk.open" => {
                if self.vis_open != on {
                    self.command(command::MILKDROP_WINDOW);
                }
            }
            "milk.choose" => self.request(Request::Ask(Ask::Presets)),
            "milk.stay" => {
                if let Ok(seconds) = value.parse::<f64>() {
                    let seconds = (seconds.round() as i32).clamp(5, 600);
                    self.settings.set(id, Some(&seconds.to_string()));
                }
            }
            "milk.fps" => self
                .settings
                .set(id, Some(if value == "60" { "60" } else { "30" })),
            "effects.open" => self.request(Request::Effects),
            "plugins.reveal" => {
                if let Some(folder) = self.plugins_folder() {
                    // the folder is made when the first plug-in is added: make it now
                    let _ = std::fs::create_dir_all(&folder);
                    self.request(Request::Reveal(folder));
                }
            }
            "skins.reveal" => {
                if let Some(folder) = self.skins_folder() {
                    // the folder is the listener's to fill: it is there from the first look
                    let _ = std::fs::create_dir_all(&folder);
                    self.request(Request::Reveal(folder));
                }
            }
            "settings.reveal" => {
                if let Some(folder) = self.config.clone() {
                    self.request(Request::Reveal(folder));
                }
            }
            _ => {}
        }
    }
}
