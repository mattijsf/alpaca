// SPDX-License-Identifier: GPL-3.0-or-later

//! The playlist as the app's commands see it: the rows selected in the skin's playlist
//! window, what is known of a file, and the list as a web page.

use std::path::PathBuf;

use super::App;
use crate::player::{decode, Player};

impl App {
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
