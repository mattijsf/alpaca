// SPDX-License-Identifier: GPL-3.0-or-later

//! The mouse on the skin: presses, drags and clicks as a shell reports them.
//!
//! A shell hands over what the mouse does at once. A tap on a trackpad is over in a few
//! milliseconds, before the screen has shown the button pressed, so the app holds such a
//! release back until the press has shown for a moment: the button is seen to go down, and
//! what it does comes that moment later.

use std::time::{Duration, Instant};

use super::App;

/// How long a press shows at least.
const SHORTEST_PRESS: Duration = Duration::from_millis(90);

/// A release held back: where the mouse let go, and when the skin is told.
#[derive(Debug, Clone, Copy)]
pub(super) struct Release {
    at: (i32, i32),
    due: Instant,
}

impl App {
    /// The mouse went down at `x`, `y` on the viewport, in layout pixels; `touch` numbers the
    /// press.
    pub fn pointer_down(&mut self, x: i32, y: i32, touch: i32) {
        self.let_go();
        if let Some(skin) = self.skin.as_mut() {
            skin.runtime.pointer_down(x, y, touch);
            self.pressed_at = Some(Instant::now());
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
