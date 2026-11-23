// SPDX-License-Identifier: GPL-3.0-or-later

//! The player behind Andamp for macOS, and the C interface the app calls
//! (`include/andamp_core.h`).
//!
//! The app hands over a skin archive's bytes; this library runs it in the walkit engine with
//! its windows set out apart ([`skin`]), decodes its pictures and fonts ([`host`]) and draws
//! each window into a buffer of pixels ([`render`]) that the app shows.
//!
//! Every call runs on the app's main thread: the engine is not shared across threads.

pub mod host;
pub mod player;
pub mod render;
pub mod skin;

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::rc::Rc;

use walkit::files::MemoryFiles;
use walkit::player::{FakePlayer, Player, Track};

use skin::Skin;

/// The app's player behind the C interface. Every skin opened with it shows and drives it.
pub struct AndampPlayer {
    player: Rc<RefCell<player::Player>>,
}

/// A loaded skin behind the C interface.
pub struct AndampSkin {
    skin: Skin,
    /// Set when the engine panicked: the skin does nothing more.
    broken: bool,
}

/// One window of a skin, as [`andamp_skin_window`] gives it: where it is on the viewport in
/// layout pixels, its `w * h` pixels (rows top first, each `0xAARRGGBB` with the color
/// multiplied by the alpha, which in memory is BGRA), and a count that goes up whenever the
/// pixels are drawn again. The pixels stay valid until the next step.
#[repr(C)]
pub struct AndampWindow {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pixels: *const u32,
    pub generation: u64,
}

/// The `len` bytes at `ptr`, or `None` for a null pointer.
unsafe fn bytes(ptr: *const u8, len: usize) -> Option<Vec<u8>> {
    (!ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, len).to_vec())
}

/// The C string at `ptr`, or an empty one for a null pointer.
unsafe fn string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// The text of a panic's payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the engine stopped".into())
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
    match catch_unwind(AssertUnwindSafe(|| f(&mut handle.skin))) {
        Ok(v) => v,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            handle.broken = true;
            fallback
        }
    }
}

/// The player a skin shows until the real one is in: it plays nothing, but answers the
/// transport, and its clock moves the time and the visualizer.
fn demo_player(clock: walkit::host::Clock) -> Box<dyn Player> {
    let mut player = FakePlayer::new(clock);
    player.playlist = (0..8)
        .map(|i| Track {
            title: format!("Andamp - track {}", i + 1),
            length_ms: 180_000 + i * 15_000,
        })
        .collect();
    player.play_track(0);
    Box::new(player)
}

/// Opens the skin archive (`.wsz` or `.wal`) of `skin_len` bytes at `skin`, named `name` (its
/// file name), on a screen of `viewport_w` x `viewport_h` layout pixels. `base` is a classic
/// skin laid under a classic skin, `wasabi` a zip of a wasabi folder laid under a modern one;
/// either may be null. `fonts_dir` is a folder of the stand-in fonts, or null. `player` is the
/// player the skin shows and drives; with null it shows a demo that plays nothing. On failure
/// it gives null and, when `error` is not null, a message in `*error` to free with
/// [`andamp_string_free`].
///
/// # Safety
/// The pointers are null or point at that many bytes, or at C strings.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_open(
    skin: *const u8,
    skin_len: usize,
    name: *const c_char,
    base: *const u8,
    base_len: usize,
    wasabi: *const u8,
    wasabi_len: usize,
    fonts_dir: *const c_char,
    viewport_w: i32,
    viewport_h: i32,
    player: *mut AndampPlayer,
    error: *mut *mut c_char,
) -> *mut AndampSkin {
    let player = player.as_ref().map(|p| p.player.clone());
    let result = catch_unwind(AssertUnwindSafe(|| {
        let skin = bytes(skin, skin_len).ok_or("no skin")?;
        let fonts = if fonts_dir.is_null() {
            MemoryFiles::new()
        } else {
            skin::read_fonts(Path::new(&string(fonts_dir)))
        };
        let shown = player.clone();
        let mut skin = Skin::open(
            skin,
            &string(name),
            bytes(base, base_len),
            bytes(wasabi, wasabi_len),
            fonts,
            (viewport_w, viewport_h),
            |clock| match shown {
                Some(player) => Box::new(player::SkinPlayer(player)),
                None => demo_player(clock),
            },
        )?;
        skin.player = player;
        Ok(skin)
    }))
    .unwrap_or_else(|payload| Err(panic_text(payload.as_ref())));
    match result {
        Ok(skin) => Box::into_raw(Box::new(AndampSkin {
            skin,
            broken: false,
        })),
        Err(message) => {
            if !error.is_null() {
                *error = CString::new(message.replace('\0', " "))
                    .unwrap_or_default()
                    .into_raw();
            }
            std::ptr::null_mut()
        }
    }
}

/// Frees a skin [`andamp_skin_open`] gave.
///
/// # Safety
/// `skin` is null or a skin not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_free(skin: *mut AndampSkin) {
    if !skin.is_null() {
        let skin = Box::from_raw(skin);
        let _ = catch_unwind(AssertUnwindSafe(move || drop(skin)));
    }
}

/// Frees a message [`andamp_skin_open`] gave.
///
/// # Safety
/// `text` is null or a message not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

/// Sets the screen the windows move on, `w` x `h` layout pixels.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_set_viewport(skin: *mut AndampSkin, w: i32, h: i32) {
    with_skin(skin, (), |s| s.set_viewport(w, h));
}

/// Moves every window by `dx`, `dy` layout pixels.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_move_windows(skin: *mut AndampSkin, dx: i32, dy: i32) {
    with_skin(skin, (), |s| s.move_windows(dx, dy));
}

/// Runs the engine's timers and animations and brings the windows up to date. Bit 1 of the
/// answer is set when a window moved, opened, closed or changed its place in the order, bit 2
/// when a window's pixels changed.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_step(skin: *mut AndampSkin) -> u32 {
    with_skin(skin, 0, Skin::step)
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
        };
        true
    })
}

/// Milliseconds until the skin next changes by itself: 0 for now, -1 for never.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_next_due_ms(skin: *mut AndampSkin) -> i64 {
    with_skin(skin, -1, Skin::next_due_ms)
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
    let stats = with_skin(skin, skin::Stats::default(), |s| s.stats());
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

/// The mouse went down at `x`, `y` on the viewport (layout pixels); `touch` numbers the press.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_pointer_down(
    skin: *mut AndampSkin,
    x: i32,
    y: i32,
    touch: i32,
) {
    with_skin(skin, (), |s| s.runtime.pointer_down(x, y, touch));
}

/// The mouse moved to `x`, `y` while down.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_pointer_move(skin: *mut AndampSkin, x: i32, y: i32) {
    with_skin(skin, (), |s| s.runtime.pointer_move(x, y));
}

/// The mouse went up at `x`, `y`.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_pointer_up(skin: *mut AndampSkin, x: i32, y: i32) {
    with_skin(skin, (), |s| s.runtime.pointer_up(x, y));
}

/// A double click at `x`, `y`; `touch` numbers its press.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_double_click(
    skin: *mut AndampSkin,
    x: i32,
    y: i32,
    touch: i32,
) {
    with_skin(skin, (), |s| s.runtime.double_tap(x, y, touch));
}

/// A right click on what press `touch` landed on.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_right_click(skin: *mut AndampSkin, touch: i32) {
    with_skin(skin, (), |s| s.runtime.right_click(touch));
}

/// Runs `f` on the player at `player`; a panic gives `fallback`.
unsafe fn with_player<T>(
    player: *mut AndampPlayer,
    fallback: T,
    f: impl FnOnce(&mut player::Player) -> T,
) -> T {
    let Some(handle) = player.as_ref() else {
        return fallback;
    };
    match catch_unwind(AssertUnwindSafe(|| f(&mut handle.player.borrow_mut()))) {
        Ok(v) => v,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            fallback
        }
    }
}

/// Makes the app's player. Free it with [`andamp_player_free`], after every skin opened with it.
#[no_mangle]
pub extern "C" fn andamp_player_new() -> *mut AndampPlayer {
    Box::into_raw(Box::new(AndampPlayer {
        player: Rc::new(RefCell::new(player::Player::new())),
    }))
}

/// Frees a player [`andamp_player_new`] gave.
///
/// # Safety
/// `player` is null or a player not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_free(player: *mut AndampPlayer) {
    if !player.is_null() {
        let player = Box::from_raw(player);
        let _ = catch_unwind(AssertUnwindSafe(move || drop(player)));
    }
}

/// Adds the audio file at `path` to the end of the playlist. Gives its place in the list, or
/// -1 when the file cannot be read as audio.
///
/// # Safety
/// `player` is null or a live player; `path` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_add(player: *mut AndampPlayer, path: *const c_char) -> i32 {
    let path = string(path);
    with_player(player, -1, |p| match p.add(Path::new(&path)) {
        Ok(index) => index as i32,
        Err(message) => {
            eprintln!("andamp: {path}: {message}");
            -1
        }
    })
}

/// Empties the playlist and stops.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_clear(player: *mut AndampPlayer) {
    with_player(player, (), player::Player::clear);
}

/// How many entries the playlist has.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_count(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| p.entries().len() as i32)
}

/// Makes entry `index` current and plays it.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_play_index(player: *mut AndampPlayer, index: i32) {
    with_player(player, (), |p| {
        if let Ok(index) = usize::try_from(index) {
            p.play_index(index);
        }
    });
}

/// The transport: 1 play, 2 pause or go on, 3 stop, 4 next, 5 previous.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_command(player: *mut AndampPlayer, command: i32) {
    with_player(player, (), |p| match command {
        1 => p.play(),
        2 => p.pause(),
        3 => p.stop(),
        4 => p.next(),
        5 => p.previous(),
        _ => {}
    });
}

/// Sets the volume, 0..255.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_set_volume(player: *mut AndampPlayer, volume: i32) {
    with_player(player, (), |p| p.set_volume(volume));
}

/// What the player asked the app for since the last call: bit 1, to let the listener pick
/// files to play.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_take_requests(player: *mut AndampPlayer) -> u32 {
    with_player(player, 0, player::Player::take_requests)
}

/// Blocks of audio in which the output ran dry while a song went on, so far.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_underruns(player: *mut AndampPlayer) -> u64 {
    with_player(player, 0, |p| p.underruns())
}
