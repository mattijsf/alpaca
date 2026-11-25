// SPDX-License-Identifier: GPL-3.0-or-later

//! The screens a shell shows the windows on, and the rules that keep every window where a
//! screen shows it.
//!
//! The screens are rectangles in one desktop space, in points from a corner the shell picks,
//! x to the right and y down, the same on every system: a shell reports the part of each
//! screen the windows move on (on the Mac, less the menu bar and the Dock). The first screen
//! is the main one. The engine has one of them as its viewport at a time, the one the main
//! window is on, so a window snaps to that screen's edges; a window can be dragged onto any
//! other.

/// A rectangle on the desktop, in points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Area {
    /// The left edge.
    pub x: i32,
    /// The top edge.
    pub y: i32,
    /// How wide.
    pub w: i32,
    /// How high.
    pub h: i32,
}

/// How much of a window a screen shows at least for it to count as seen, in points: as much
/// as a pointer needs to take it by its title bar, or all of a smaller window.
const SEEN: (i32, i32) = (40, 20);

impl Area {
    /// The rectangle `w` by `h` with its top left corner at `x`, `y`.
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Area {
        Area { x, y, w, h }
    }

    fn right(self) -> i32 {
        self.x + self.w
    }

    fn bottom(self) -> i32 {
        self.y + self.h
    }

    /// The rectangle moved by `dx`, `dy`.
    #[must_use]
    pub fn moved(self, dx: i32, dy: i32) -> Area {
        Area::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// How wide and how high the part is that this one and `other` share; 0 by 0 for none.
    fn overlap(self, other: Area) -> (i32, i32) {
        let w = self.right().min(other.right()) - self.x.max(other.x);
        let h = self.bottom().min(other.bottom()) - self.y.max(other.y);
        (w.max(0), h.max(0))
    }

    /// Whether this one and `other` overlap or touch: windows docked to each other touch.
    fn near(self, other: Area) -> bool {
        self.x <= other.right()
            && other.x <= self.right()
            && self.y <= other.bottom()
            && other.y <= self.bottom()
    }

    /// The smallest rectangle around both.
    fn around(self, other: Area) -> Area {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Area::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }
}

/// The screen of `screens` that shows the most of `area`; `None` when none shows any of it.
pub fn screen_of(area: Area, screens: &[Area]) -> Option<usize> {
    screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let (w, h) = area.overlap(*screen);
            (i, i64::from(w) * i64::from(h))
        })
        .filter(|(_, shown)| *shown > 0)
        .max_by_key(|(i, shown)| (*shown, std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
}

/// Whether one of `screens` shows enough of the window at `area` to see it and take it.
pub fn seen(area: Area, screens: &[Area]) -> bool {
    let least = (area.w.clamp(1, SEEN.0), area.h.clamp(1, SEEN.1));
    screens.iter().any(|screen| {
        let (w, h) = area.overlap(*screen);
        w >= least.0 && h >= least.1
    })
}

/// The windows at `windows` that overlap or touch, by their places in it, each group as one:
/// a group moves as one so that docked windows stay docked. The group of window `main` comes
/// first.
fn groups(windows: &[Area], main: usize) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut taken = vec![false; windows.len()];
    let order = std::iter::once(main).chain((0..windows.len()).filter(|i| *i != main));
    for first in order {
        if first >= windows.len() || taken[first] {
            continue;
        }
        taken[first] = true;
        let mut group = vec![first];
        let mut next = 0;
        while next < group.len() {
            let at = windows[group[next]];
            for (i, other) in windows.iter().enumerate() {
                if !taken[i] && at.near(*other) {
                    taken[i] = true;
                    group.push(i);
                }
            }
            next += 1;
        }
        groups.push(group);
    }
    groups
}

/// How far a stretch at `at`, `size` long, moves to lie inside the stretch at `inside`,
/// `room` long, as little as it can; one longer than the room starts where the room does.
fn into(at: i32, size: i32, inside: i32, room: i32) -> i32 {
    if size >= room || at < inside {
        inside - at
    } else if at + size > inside + room {
        inside + room - (at + size)
    } else {
        0
    }
}

/// Where the windows at `windows` go so that a screen of `screens` shows every one of them;
/// window `main` is the main one. A group of windows that are all seen stays where it is.
/// Any other group moves as one, onto the screen that shows the most of it, or, when no
/// screen shows any of it, onto one of the size of the screen it was on in `before`, else
/// the main window's group onto the main screen and the others onto the main window's
/// screen: at the place it had on its screen before, then moved as little as it takes to
/// show whole.
pub fn into_view(windows: &[Area], main: usize, screens: &[Area], before: &[Area]) -> Vec<Area> {
    let mut out = windows.to_vec();
    if screens.is_empty() || windows.is_empty() {
        return out;
    }
    let main = main.min(windows.len() - 1);
    for group in groups(windows, main) {
        if group.iter().all(|i| seen(out[*i], screens)) {
            continue;
        }
        let Some(bounds) = group.iter().map(|i| out[*i]).reduce(Area::around) else {
            continue;
        };
        let (target, from) = if let Some(screen) = screen_of(bounds, screens) {
            (screens[screen], None)
        } else {
            let from = screen_of(bounds, before).map(|i| before[i]);
            let same_size = from.and_then(|from| {
                screens
                    .iter()
                    .filter(|s| (s.w, s.h) == (from.w, from.h))
                    .min_by_key(|s| (s.x - from.x).abs() + (s.y - from.y).abs())
            });
            let fallback = if group.contains(&main) {
                screens[0]
            } else {
                screens[screen_of(out[main], screens).unwrap_or(0)]
            };
            (*same_size.unwrap_or(&fallback), from)
        };
        // first the same place on the new screen as on the old one, then whole on it
        let (dx, dy) = from.map_or((0, 0), |from| (target.x - from.x, target.y - from.y));
        let at = bounds.moved(dx, dy);
        let dx = dx + into(at.x, at.w, target.x, target.w);
        let dy = dy + into(at.y, at.h, target.y, target.h);
        for i in group {
            out[i] = out[i].moved(dx, dy);
        }
    }
    out
}
