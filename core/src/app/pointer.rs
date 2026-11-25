// SPDX-License-Identifier: GPL-3.0-or-later

//! The mouse on the skin: presses, drags and clicks as a shell reports them.
//!
//! A shell hands over what the mouse does at once. A tap on a trackpad is over in a few
//! milliseconds, before the screen has shown the button pressed, so the app holds such a
//! release back until the press has shown for a moment: the button is seen to go down, and
//! what it does comes that moment later.

use std::time::{Duration, Instant};
use walkit::host::PointerShape;

use super::App;

/// How long a press shows at least.
const SHORTEST_PRESS: Duration = Duration::from_millis(90);

/// A release held back: where the mouse let go, and when the skin is told.
#[derive(Debug, Clone, Copy)]
pub(super) struct Release {
    at: (i32, i32),
    due: Instant,
}

/// The cursor a shell shows over a place of the skin: one of the system's, by what a press
/// there takes hold of, or a picture the skin brings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    /// The plain arrow.
    Arrow,
    /// Dragged left and right: a divider between panes side by side, a window's side.
    LeftRight,
    /// Dragged up and down: a divider between panes one over the other, a window's top or
    /// bottom.
    UpDown,
    /// A window's bottom right or top left corner.
    Diagonal,
    /// A window's bottom left or top right corner.
    OtherDiagonal,
    /// A classic skin's own cursor, by its number among the 29 a skin can bring;
    /// [`App::cursor_picture`] gives its pictures.
    Skin(u32),
}

impl Cursor {
    /// The first of the numbers that name a skin's own cursors in the C interface.
    pub const SKIN: u32 = 100;

    /// The cursor as the C interface numbers it: 0 to 4 for the system's, and
    /// [`Cursor::SKIN`] and up for the skin's own.
    pub fn number(self) -> u32 {
        match self {
            Cursor::Arrow => 0,
            Cursor::LeftRight => 1,
            Cursor::UpDown => 2,
            Cursor::Diagonal => 3,
            Cursor::OtherDiagonal => 4,
            Cursor::Skin(slot) => Cursor::SKIN + slot,
        }
    }
}

impl App {
    /// The cursor for the mouse at `x`, `y` on the viewport, in layout pixels. A classic skin
    /// that brings a cursor for the place shows it; without one, and on a modern skin, it is
    /// the system's for what a press there sizes, else the arrow. While a divider or a
    /// window's edge is dragged it stays that drag's, wherever the mouse is.
    pub fn cursor_at(&mut self, x: i32, y: i32) -> Cursor {
        let Some(skin) = self.skin.as_mut() else {
            return Cursor::Arrow;
        };
        if let Some(slot) = skin.runtime.classic_cursor_at(x, y) {
            if skin.runtime.classic_cursor(slot).is_some() {
                return Cursor::Skin(slot as u32);
            }
        }
        match skin.runtime.pointer_shape_at(x, y) {
            PointerShape::Arrow => Cursor::Arrow,
            PointerShape::LeftRight => Cursor::LeftRight,
            PointerShape::UpDown => Cursor::UpDown,
            PointerShape::Diagonal => Cursor::Diagonal,
            PointerShape::OtherDiagonal => Cursor::OtherDiagonal,
        }
    }

    /// The pictures of the skin's own cursor `slot`, as [`Cursor::Skin`] numbers it, in whole
    /// numbers: how many steps it has, then for each step the picture it shows and for how
    /// many milliseconds (one step with no time for a cursor that stands still); how many
    /// pictures, then for each its width, its height, the tip's place from the left and from
    /// the top, and its pixels row by row from the top, each `0xAARRGGBB` with the color not
    /// multiplied by the alpha. `None` when the skin worn has no such cursor.
    pub fn cursor_picture(&mut self, slot: u32) -> Option<Vec<u32>> {
        let cursor = self.skin.as_mut()?.runtime.classic_cursor(slot as usize)?;
        let mut words = vec![cursor.steps.len() as u32];
        for (frame, ms) in &cursor.steps {
            words.extend([*frame as u32, *ms]);
        }
        words.push(cursor.frames.len() as u32);
        for frame in &cursor.frames {
            words.extend([frame.width, frame.height, frame.hot_x, frame.hot_y]);
            words.extend(&frame.argb);
        }
        Some(words)
    }

    /// The mouse went down at `x`, `y` on the viewport, in layout pixels; `touch` numbers the
    /// press.
    pub fn pointer_down(&mut self, x: i32, y: i32, touch: i32) {
        self.let_go();
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_down(x, y, touch);
            self.pressed_at = Some(Instant::now());
        }
    }

    /// The keys held with the next presses: `add`, the system's key for picking rows one by
    /// one (Command on a Mac, Control elsewhere, which Winamp's playlist calls Ctrl), and
    /// `range`, Shift, for picking the rows from the last one picked. As in Winamp's playlist.
    pub fn set_modifiers(&mut self, add: bool, range: bool) {
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.set_pointer_modifiers(add, range);
        }
    }

    /// The mouse moved to `x`, `y` with no button down: what is under it hears it, as in
    /// Winamp, and a modern skin's button shows its hover picture.
    pub fn hover(&mut self, x: i32, y: i32) {
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_hover(x, y);
        }
    }

    /// The tooltip to show beside the mouse now, as Winamp shows it: 350 ms after the mouse came
    /// onto an object with one, for 3 s. Empty for none.
    pub fn tooltip(&mut self) -> String {
        self.skin
            .as_mut()
            .and_then(|skin| skin.runtime.tooltip())
            .unwrap_or_default()
    }

    /// The mouse left the skin's windows.
    pub fn pointer_leave(&mut self) {
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_leave();
        }
    }

    /// The mouse moved to `x`, `y` while down.
    pub fn pointer_move(&mut self, x: i32, y: i32) {
        // after the mouse let go there is nothing to drag, also while the skin does not know yet
        if self.release.is_some() {
            return;
        }
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_move(x, y);
        }
    }

    /// The mouse let go at `x`, `y`. After a press too short to have been seen, the skin is
    /// told when it has shown long enough: at the next [`App::step`] after that moment, which
    /// [`App::next_due_ms`] counts down to.
    pub fn pointer_up(&mut self, x: i32, y: i32) {
        // a window dragged is kept where a screen shows it once the skin has the release
        self.unsettled = true;
        let shown_until = self.pressed_at.take().map(|at| at + SHORTEST_PRESS);
        match shown_until {
            Some(due) if Instant::now() < due => self.release = Some(Release { at: (x, y), due }),
            _ => {
                if let Some(skin) = self.skin.as_mut() {
                    skin.runtime.pointer_up(x, y);
                }
            }
        }
    }

    /// The press `touch` at `x`, `y` was the second of a double click.
    pub fn double_click(&mut self, x: i32, y: i32, touch: i32) {
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.double_tap(x, y, touch);
        }
    }

    /// A click of the other mouse button at `x`, `y`: what is under it shows its menu.
    pub fn right_click(&mut self, x: i32, y: i32, touch: i32) {
        self.let_go();
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_down(x, y, touch);
            skin.runtime.right_click(touch);
            skin.runtime.pointer_up(x, y);
        }
    }

    /// Tells the skin of a release held back, whether or not its moment has come.
    fn let_go(&mut self) {
        if let (Some(release), Some(skin)) = (self.release.take(), self.skin.as_mut()) {
            skin.runtime.pointer_up(release.at.0, release.at.1);
        }
    }

    /// Tells the skin of a release held back once the press has shown long enough.
    pub(super) fn let_go_when_due(&mut self) {
        if self
            .release
            .is_some_and(|release| Instant::now() >= release.due)
        {
            self.let_go();
        }
    }

    /// Milliseconds until a release held back is due; `None` when none is.
    pub(super) fn release_due_ms(&self) -> Option<i64> {
        let release = self.release?;
        Some(
            release
                .due
                .saturating_duration_since(Instant::now())
                .as_millis() as i64,
        )
    }
}
