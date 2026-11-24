// SPDX-License-Identifier: GPL-3.0-or-later

//! The windows of the skin worn and their pixels.

use std::panic::{catch_unwind, AssertUnwindSafe};

use super::panic_text;
use crate::skin::Skin;

/// A loaded skin behind the C interface.
pub type AndampSkin = Skin;

/// One window of a skin, as [`andamp_skin_window`] gives it: where it is on the viewport in
/// layout pixels, its `w * h` pixels (rows top first, each `0xAARRGGBB` with the color
/// multiplied by the alpha, which in memory is BGRA), a count that goes up whenever the
/// pixels are drawn again, and the part of the window the last drawing changed: an app that
/// showed the drawing before needs to take over only that part. The pixels stay valid until
/// the next step.
#[repr(C)]
pub struct AndampWindow {
    /// The engine's id of the window.
    pub id: u32,
    /// Its left edge on the viewport, in layout pixels.
    pub x: i32,
    /// Its top edge on the viewport, in layout pixels.
    pub y: i32,
    /// Its width in layout pixels, which is its width in pixels.
    pub w: i32,
    /// Its height.
    pub h: i32,
    /// Its `w * h` pixels, rows top first.
    pub pixels: *const u32,
    /// A count that goes up whenever the pixels are drawn again.
    pub generation: u64,
    /// The part of the window the last drawing changed, in its own pixels.
    pub changed_x: i32,
    /// The top edge of the part that changed.
    pub changed_y: i32,
    /// The width of the part that changed.
    pub changed_w: i32,
    /// The height of the part that changed.
    pub changed_h: i32,
}

/// Runs `f` on the skin at `skin` unless it broke before; a panic breaks it and gives
/// `fallback`.
unsafe fn with_skin<T>(skin: *mut AndampSkin, fallback: T, f: impl FnOnce(&mut Skin) -> T) -> T {
    let Some(handle) = skin.as_mut() else {
        return fallback;
    };
    if handle.broken {
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| f(handle))) {
        Ok(v) => v,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            handle.broken = true;
            fallback
        }
    }
}

/// How many windows the skin shows.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_window_count(skin: *mut AndampSkin) -> i32 {
    with_skin(skin, 0, |s| s.windows().len() as i32)
}

/// Window `index`, counted from the bottom, into `*out`; false when there is none.
///
/// # Safety
/// `skin` is null or a live skin; `out` points at an [`AndampWindow`].
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_window(
    skin: *mut AndampSkin,
    index: i32,
    out: *mut AndampWindow,
) -> bool {
    with_skin(skin, false, |s| {
        let Some(w) = usize::try_from(index).ok().and_then(|i| s.windows().get(i)) else {
            return false;
        };
        *out = AndampWindow {
            id: w.id,
            x: w.rect.left,
            y: w.rect.top,
            w: w.canvas.w,
            h: w.canvas.h,
            pixels: w.canvas.pixels.as_ptr(),
            generation: w.generation,
            changed_x: w.changed.left,
            changed_y: w.changed.top,
            changed_w: w.changed.width,
            changed_h: w.changed.height,
        };
        true
    })
}

/// What drawing has cost so far: steps in which something changed, windows drawn again, and
/// the nanoseconds spent in the engine and in drawing pixels. A null pointer is skipped.
///
/// # Safety
/// `skin` is null or a live skin; the others are null or point at a `u64`.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_stats(
    skin: *mut AndampSkin,
    steps: *mut u64,
    windows_drawn: *mut u64,
    engine_nanos: *mut u64,
    draw_nanos: *mut u64,
) {
    let stats = with_skin(skin, crate::skin::Stats::default(), |s| s.stats());
    for (out, value) in [
        (steps, stats.steps),
        (windows_drawn, stats.windows_drawn),
        (engine_nanos, stats.engine_nanos),
        (draw_nanos, stats.draw_nanos),
    ] {
        if !out.is_null() {
            *out = value;
        }
    }
}
