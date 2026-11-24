// SPDX-License-Identifier: GPL-3.0-or-later

//! The playlist as the app's commands see it: the rows selected in the skin's playlist
//! window, what is known of a file, and the list as a web page.

use std::path::PathBuf;

use super::App;
use crate::player::{decode, Player};
use crate::skin::Skin;

/// Rows a classic playlist scrolls for a notch of a mouse wheel, as Winamp's does.
const ROWS_A_NOTCH: f32 = 3.0;
/// The height of a classic playlist's row, in layout pixels.
const ROW_HEIGHT: f32 = 13.0;
/// Steps of the volume, of 255, for a notch of a mouse wheel.
const VOLUME_A_NOTCH: f32 = 4.0;

/// A row of a classic playlist is at most this high, in layout pixels: the engine does not
/// say how high a skin's rows are, and with this the app never counts more rows in view than
/// there are.
const TALLEST_ROW: i32 = 16;
/// A classic playlist's height less its list: the title bar and the buttons.
const LIST_FRAME: i32 = 60;
/// Where Winamp has the volume slider in a classic main window, with a little room over and
/// under it: left, top, width, height.
const CLASSIC_VOLUME: (i32, i32, i32, i32) = (107, 54, 69, 17);

impl App {
    /// The mouse wheel at `at`, a place on the viewport in layout pixels. Over a classic
    /// skin's playlist window it scrolls the list, and over the volume slider it turns the
    /// volume; anywhere else it does nothing. `delta` is more than 0 for up: in notches of a
    /// wheel, or with `precise` in points of a trackpad, which the list then follows as far
    /// as the fingers move.
    pub fn wheel(&mut self, at: (i32, i32), delta: f32, precise: bool) {
        let zoom = self.zoom as f32;
        if self.is_over(self.list_window(), at) {
            // up is towards the list's start
            let rows = if precise {
                delta / zoom / ROW_HEIGHT
            } else {
                delta * ROWS_A_NOTCH
            };
            let whole = whole_of(&mut self.wheel_rows, -rows);
            if whole != 0 {
                let last = self.player.borrow().entries().len() as i32 - 1;
                self.list_top = (self.list_top + whole).clamp(0, last.max(0));
                if let Some(skin) = self.skin.as_mut() {
                    skin.runtime.scroll_classic_playlist(whole);
                    skin.refresh();
                }
            }
        } else if self.volume_slider().is_some_and(|r| contains(r, at)) {
            // a trackpad's points, at the pace a wheel's notches have
            let steps = if precise { delta / 16.0 } else { delta } * VOLUME_A_NOTCH;
            let whole = whole_of(&mut self.wheel_volume, steps);
            if whole != 0 {
                let mut player = self.player.borrow_mut();
                let volume = (player.volume() + whole).clamp(0, 255);
                player.set_volume(volume);
            }
        }
    }

    /// The playlist's own window, by the engine's id; `None` when the skin has none, as one
    /// that shows its list inside the main window.
    fn list_window(&self) -> Option<u32> {
        let skin = self.skin.as_ref()?;
        skin.runtime.window_of("pledit").map(|id| id.0)
    }

    /// Whether `at`, a place on the viewport, is over window `id`.
    fn is_over(&mut self, id: Option<u32>, at: (i32, i32)) -> bool {
        let (Some(id), Some(skin)) = (id, self.skin.as_mut()) else {
            return false;
        };
        let r = skin.runtime.window_rect(Skin::window_id(id));
        contains((r.left, r.top, r.width, r.height), at)
    }

    /// Where the volume slider is on the viewport: left, top, width, height. A classic skin's
    /// is where Winamp has it in the main window; a modern skin's is the object it names
    /// `volume`. `None` when the skin shows none, as a main window rolled up to its bar.
    fn volume_slider(&mut self) -> Option<(i32, i32, i32, i32)> {
        let skin = self.skin.as_mut()?;
        if let Some(main) = skin.runtime.object_info("classic.main") {
            let window = skin.runtime.window_rect(main.window);
            let (x, y, w, h) = CLASSIC_VOLUME;
            return Some((
                window.left + main.rect.left + x,
                window.top + main.rect.top + y,
                w,
                h,
            ));
        }
        let slider = skin.runtime.object_info("volume").filter(|o| o.visible)?;
        let window = skin.runtime.window_rect(slider.window);
        let r = slider.rect;
        Some((window.left + r.left, window.top + r.top, r.width, r.height))
    }

    /// A key pressed in window `window` of the skin, by its name. In the playlist's window
    /// `up` and `down` move the selection a row, `pageup` and `pagedown` a screen, `home` and
    /// `end` to the list's ends, as in Winamp's playlist; every other key, and every key in
    /// another window, is [`App::key`]'s. False for a key that is none of these.
    pub fn key_in(&mut self, window: u32, key: &str) -> bool {
        if self.list_window() != Some(window) {
            return self.key(key);
        }
        let screen = self.rows_in_view();
        let last = self.player.borrow().entries().len() as i32 - 1;
        let selected = self.selection();
        // from the selection's end on the side the key goes to; with nothing selected the
        // first press selects the current song
        let (first, end) = (
            selected.first().map(|r| *r as i32),
            selected.last().map(|r| *r as i32),
        );
        let current = self.player.borrow().index() as i32;
        let to = match key {
            "up" => first.map_or(current, |row| row - 1),
            "down" => end.map_or(current, |row| row + 1),
            "pageup" => first.map_or(current, |row| row - screen),
            "pagedown" => end.map_or(current, |row| row + screen),
            "home" => 0,
            "end" => last,
            _ => return self.key(key),
        };
        if last >= 0 {
            let row = to.clamp(0, last) as usize;
            self.select(&[row]);
            self.show_row(row);
        }
        true
    }

    /// Brings the current song into view in a classic playlist when another song became
    /// current: the list follows what plays, as Winamp's does. The listener's own scrolling
    /// in between stays as it is.
    pub(super) fn follow_the_song(&mut self) {
        let current = {
            let player = self.player.borrow();
            (!player.entries().is_empty()).then(|| player.index())
        };
        if current != self.followed {
            self.followed = current;
            if let Some(row) = current {
                self.show_row(row);
            }
        }
    }

    /// Rows a classic playlist's window surely shows: one at least.
    fn rows_in_view(&mut self) -> i32 {
        let height = self.list_window().and_then(|id| {
            let skin = self.skin.as_mut()?;
            Some(skin.runtime.window_rect(Skin::window_id(id)).height)
        });
        ((height.unwrap_or(0) - LIST_FRAME) / TALLEST_ROW).max(1)
    }

    /// Scrolls a classic playlist so that `row` is in view. The engine does not say how far
    /// its list is scrolled, so the app keeps count (`list_top`) and sets the list to its
    /// count: all the way up, then down to it. A drag of the list's own scroll bar is not
    /// counted, and the next key then brings the list back to the selection.
    fn show_row(&mut self, row: usize) {
        let (row, screen) = (row as i32, self.rows_in_view());
        let last = self.player.borrow().entries().len() as i32 - 1;
        let mut top = self.list_top.clamp(0, last.max(0));
        if row < top {
            top = row;
        } else if row >= top + screen {
            top = row - screen + 1;
        }
        self.list_top = top;
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.scroll_classic_playlist(-1_000_000);
            skin.runtime.scroll_classic_playlist(top);
            skin.refresh();
        }
    }

    /// The rows selected in the playlist window, lowest first.
    pub fn selection(&self) -> Vec<usize> {
        let mut rows = self
            .skin
            .as_ref()
            .map(|skin| skin.selection().to_vec())
            .unwrap_or_default();
        let len = self.player.borrow().entries().len();
        rows.retain(|row| *row < len);
        rows.sort_unstable();
        rows.dedup();
        rows
    }

    /// Selects `rows` in the playlist window, and no other.
    pub fn select(&mut self, rows: &[usize]) {
        if let Some(skin) = self.skin.as_mut() {
            skin.set_selection(rows);
        }
    }

    /// Changes the playlist's entries or their order. The rows the selection named are other
    /// entries afterwards, so nothing stays selected.
    pub(super) fn change_list(&mut self, change: impl FnOnce(&mut Player)) {
        change(&mut self.player.borrow_mut());
        self.select(&[]);
    }

    /// Has the skin show the player again: its list, its titles, its buttons.
    pub(super) fn refresh_skin(&mut self) {
        if let Some(skin) = self.skin.as_mut() {
            skin.refresh();
        }
    }

    /// The row a question about a file is about: the first one selected, else the current
    /// song's. `None` with an empty playlist.
    pub(super) fn row_in_question(&self) -> Option<usize> {
        let player = self.player.borrow();
        let row = self.selection().first().copied().unwrap_or(player.index());
        (row < player.entries().len()).then_some(row)
    }

    /// The first selected entry's file; `None` with nothing selected, or a radio stream.
    pub(super) fn selected_file(&self) -> Option<PathBuf> {
        let row = *self.selection().first()?;
        let player = self.player.borrow();
        let path = &player.entries().get(row)?.path;
        (!decode::is_url(path)).then(|| path.clone())
    }

    /// What is known of the playlist's entry at `row`, for a shell to show: its title on the
    /// first line, then a line for each thing known, a name and a value with a colon between.
    pub fn file_info(&self, row: usize) -> Option<String> {
        let player = self.player.borrow();
        let entry = player.entries().get(row)?;
        let info = &entry.info;
        let mut lines = vec![entry.title.clone()];
        // the tags a listener looks for first, then whatever else the file carries
        let first = ["title", "artist", "album", "date", "genre", "track"];
        let named = first
            .iter()
            .filter_map(|name| Some((*name, info.tags.get(*name)?)));
        let others = info
            .tags
            .iter()
            .filter(|(name, _)| !first.contains(&name.as_str()));
        for (name, value) in named.chain(others.map(|(name, value)| (name.as_str(), value))) {
            let mut label = name.to_string();
            if let Some(initial) = label.get_mut(0..1) {
                initial.make_ascii_uppercase();
            }
            lines.push(format!("{label}: {}", value.replace('\n', " ")));
        }
        if info.length_ms > 0 {
            lines.push(format!("Length: {}", time(info.length_ms)));
        }
        if decode::is_url(&entry.path) {
            lines.push(format!("Stream: {}", entry.path.to_string_lossy()));
            return Some(lines.join("\n"));
        }
        let mut format = Vec::new();
        if !info.codec.is_empty() {
            format.push(info.codec.clone());
        }
        if info.sample_rate > 0 {
            format.push(format!("{} Hz", info.sample_rate));
        }
        match info.channels {
            0 => {}
            1 => format.push("mono".into()),
            2 => format.push("stereo".into()),
            n => format.push(format!("{n} channels")),
        }
        if info.bitrate_kbps > 0 {
            format.push(format!("{} kbps", info.bitrate_kbps));
        }
        if !format.is_empty() {
            lines.push(format!("Format: {}", format.join(", ")));
        }
        lines.push(format!("File: {}", entry.path.to_string_lossy()));
        Some(lines.join("\n"))
    }

    /// The playlist as a web page: its entries in order, each with its length.
    pub fn html_list(&self) -> String {
        let escape = |text: &str| {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let player = self.player.borrow();
        let total: i64 = player
            .entries()
            .iter()
            .map(|e| i64::from(e.info.length_ms.max(0)))
            .sum();
        let rows: String = player
            .entries()
            .iter()
            .map(|entry| {
                let length = if entry.info.length_ms > 0 {
                    format!(" <small>({})</small>", time(entry.info.length_ms))
                } else {
                    String::new()
                };
                format!("<li>{}{length}</li>\n", escape(&entry.title))
            })
            .collect();
        format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Andamp playlist</title>\n\
             <style>body {{ font: 14px sans-serif; margin: 2em; }} small {{ color: gray; }}</style>\n\
             </head>\n<body>\n<h1>Andamp playlist</h1>\n<p>{} songs, {}</p>\n<ol>\n{rows}</ol>\n</body>\n</html>\n",
            player.entries().len(),
            time(total.min(i64::from(i32::MAX)) as i32),
        )
    }
}

/// Whether `at` is in the box `r`: left, top, width, height.
fn contains(r: (i32, i32, i32, i32), at: (i32, i32)) -> bool {
    (r.0..r.0 + r.2).contains(&at.0) && (r.1..r.1 + r.3).contains(&at.1)
}

/// Adds `more` to what is `kept` of a turn of the wheel, and takes the whole steps out of it:
/// a trackpad turns by fractions, and what is left over counts towards the next step.
fn whole_of(kept: &mut f32, more: f32) -> i32 {
    *kept += more;
    let whole = kept.trunc();
    *kept -= whole;
    whole as i32
}

/// Milliseconds as a listener reads them: `3:07`, or `1:02:03` from an hour on.
fn time(ms: i32) -> String {
    let seconds = ms / 1000;
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
