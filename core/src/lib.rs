// SPDX-License-Identifier: GPL-3.0-or-later

//! The player behind Andamp for macOS, and the C interface the app calls
//! (`include/andamp_core.h`).
//!
//! The app hands over a skin archive's bytes; this library runs it in the walkit engine with
//! its windows set out apart ([`skin`]), decodes its pictures and fonts ([`host`]) and draws
//! each window into a buffer of pixels ([`render`]) that the app shows.
//!
//! Every call runs on the app's main thread: the engine is not shared across threads.

pub mod app;
pub mod dsp;
pub mod host;
pub mod player;
pub mod render;
pub mod settings;
pub mod skin;
pub mod vis;

use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
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
pub type AndampSkin = Skin;

/// One window of a skin, as [`andamp_skin_window`] gives it: where it is on the viewport in
/// layout pixels, its `w * h` pixels (rows top first, each `0xAARRGGBB` with the color
/// multiplied by the alpha, which in memory is BGRA), a count that goes up whenever the
/// pixels are drawn again, and the part of the window the last drawing changed: an app that
/// showed the drawing before needs to take over only that part. The pixels stay valid until
/// the next step.
#[repr(C)]
pub struct AndampWindow {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub pixels: *const u32,
    pub generation: u64,
    /// The part of the window the last drawing changed, in its own pixels.
    pub changed_x: i32,
    pub changed_y: i32,
    pub changed_w: i32,
    pub changed_h: i32,
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
    match catch_unwind(AssertUnwindSafe(|| f(handle))) {
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
/// skin laid under a classic skin. Under a modern one goes `wasabi`, a zip of a wasabi folder,
/// or without it the files of the folder `wasabi_dir`. Each may be null. `fonts_dir` is a
/// folder of the stand-in fonts, or null. `beside_dir` is the folder the skin's file is in, where a
/// skin it builds on is looked for, or null. `player` is the
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
    wasabi_dir: *const c_char,
    fonts_dir: *const c_char,
    beside_dir: *const c_char,
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
        let base = bytes(base, base_len).map(skin::unzip).transpose()?;
        let wasabi = match bytes(wasabi, wasabi_len) {
            Some(zip) => Some(skin::unzip(zip)?),
            None if !wasabi_dir.is_null() => Some(skin::read_folder(Path::new(&string(wasabi_dir)))),
            None => None,
        };
        let mut skin = Skin::open(
            skin,
            &string(name),
            base,
            wasabi,
            fonts,
            (!beside_dir.is_null()).then(|| string(beside_dir)).as_deref().map(Path::new),
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
        Ok(skin) => Box::into_raw(Box::new(skin)),
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

/// Puts window `id` with its top left corner at `x`, `y` on the viewport.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_place_window(skin: *mut AndampSkin, id: u32, x: i32, y: i32) {
    with_skin(skin, (), |s| s.place_window(id, x, y));
}

/// The id of the skin's main window, or 0 when it has none.
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_main_window(skin: *mut AndampSkin) -> u32 {
    with_skin(skin, 0, |s| s.runtime.main_window().map_or(0, |id| id.0))
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
            changed_x: w.changed.left,
            changed_y: w.changed.top,
            changed_w: w.changed.width,
            changed_h: w.changed.height,
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

/// What the skin's buttons asked of the app since the last call, or null for nothing: one line
/// for each, the action (`CLOSE`, `MINIMIZE`, `DOUBLESIZE`, `ALWAYS_ON_TOP`, `MENU` and the
/// like), its param, and the place on the viewport a menu for it opens at as `x` and `y`, with
/// tabs between; an action from nothing on screen has no place. Free it with
/// [`andamp_string_free`].
///
/// # Safety
/// `skin` is null or a live skin.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_take_actions(skin: *mut AndampSkin) -> *mut c_char {
    let actions = with_skin(skin, Vec::new(), Skin::take_actions);
    if actions.is_empty() {
        return std::ptr::null_mut();
    }
    let text: String = actions
        .iter()
        .map(|a| {
            let at = a.at.map(|(x, y)| format!("{x}\t{y}")).unwrap_or_default();
            let param = a.param.as_deref().unwrap_or("").replace(['\t', '\n'], " ");
            format!("{}\t{param}\t{at}\n", a.name)
        })
        .collect();
    CString::new(text).unwrap_or_default().into_raw()
}

/// Tells the skin whether one of the app's switches is on, so its button lights up:
/// "doublesize" and "ontop" for a classic skin's clutter bar.
///
/// # Safety
/// `skin` is null or a live skin; `name` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_set_switch(skin: *mut AndampSkin, name: *const c_char, on: bool) {
    let name = string(name);
    with_skin(skin, (), |s| s.set_switch(&name, on));
}

/// Shows a skin's popup menu: `menu` is its items as lines of text (`C`, the id, 1 when
/// checked, 1 when disabled and the text, with tabs between, for a command; `S` for a
/// separator; `>` and the text where a submenu starts, `<` where it ends). It gives the id of
/// the command picked, or 0. It is called from inside a mouse call, and must not call the skin.
pub type AndampMenuHandler = unsafe extern "C" fn(context: *mut c_void, menu: *const c_char) -> i32;

/// Sets how the skin's popup menus are shown; `context` is handed back to `handler`.
///
/// # Safety
/// `skin` is null or a live skin; `handler` stays callable, with `context`, for as long as the
/// skin lives.
#[no_mangle]
pub unsafe extern "C" fn andamp_skin_set_menu_handler(
    skin: *mut AndampSkin,
    handler: Option<AndampMenuHandler>,
    context: *mut c_void,
) {
    with_skin(skin, (), |s| {
        s.set_menu_handler(handler.map(|show| -> host::MenuHandler {
            Box::new(move |menu: &str| {
                let text = CString::new(menu.replace('\0', " ")).unwrap_or_default();
                // SAFETY: the caller promised the handler and its context for the skin's life
                unsafe { show(context, text.as_ptr()) }
            })
        }));
    });
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

/// Adds the songs the playlist file at `path` names (`.m3u`, `.m3u8` or `.pls`). Gives how
/// many joined, or -1 when the file cannot be read.
///
/// # Safety
/// `player` is null or a live player; `path` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_add_list(player: *mut AndampPlayer, path: *const c_char) -> i32 {
    let path = string(path);
    with_player(player, -1, |p| p.add_list(Path::new(&path)).map_or(-1, |n| n as i32))
}

/// Writes the playlist to `path` as an `.m3u8` file; false when it cannot be written.
///
/// # Safety
/// `player` is null or a live player; `path` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_save_list(player: *mut AndampPlayer, path: *const c_char) -> bool {
    let path = string(path);
    with_player(player, false, |p| p.save_list(Path::new(&path)).is_ok())
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

/// Makes entry `index` current without playing it, when nothing plays.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_select(player: *mut AndampPlayer, index: i32) {
    with_player(player, (), |p| {
        if let Ok(index) = usize::try_from(index) {
            p.select(index);
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

/// The volume, 0..255.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_volume(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| p.volume())
}

/// Moves the song by `ms` milliseconds, back for a negative number.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_seek_by(player: *mut AndampPlayer, ms: i32) {
    with_player(player, (), |p| {
        let to = p.position_ms().saturating_add(ms);
        p.seek_to(to);
    });
}

/// Moves the player on when its song has ended. A skin's step does this by itself; the app
/// calls it while no skin steps, so that the playlist goes on with every window out of sight.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_poll(player: *mut AndampPlayer) {
    with_player(player, (), player::Player::poll);
}

/// What the transport is doing: 0 stopped, 1 playing, 2 paused.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_status(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| match p.status() {
        walkit::player::Status::Stopped => 0,
        walkit::player::Status::Playing => 1,
        walkit::player::Status::Paused => 2,
    })
}

/// Which entry of the playlist is current.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_index(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| p.index() as i32)
}

/// Where the song is, in milliseconds.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_position_ms(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| p.position_ms())
}

/// Moves the song to `ms` milliseconds.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_seek_to(player: *mut AndampPlayer, ms: i32) {
    with_player(player, (), |p| p.seek_to(ms));
}

/// Entry `index` of the playlist: its length in milliseconds, or -1 when there is none. The
/// pointers that are not null get its title, its artist, its album and its file's path, each
/// to free with [`andamp_string_free`]; a tag the file does not carry comes out empty.
///
/// # Safety
/// `player` is null or a live player; the others are null or point at a pointer.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_entry(
    player: *mut AndampPlayer,
    index: i32,
    title: *mut *mut c_char,
    artist: *mut *mut c_char,
    album: *mut *mut c_char,
    path: *mut *mut c_char,
) -> i32 {
    with_player(player, -1, |p| {
        let Some(entry) = usize::try_from(index).ok().and_then(|i| p.entries().get(i)) else {
            return -1;
        };
        let tag = |name: &str| entry.info.tags.get(name).cloned().unwrap_or_default();
        // the title alone when the tags give one, else what the playlist shows
        let name = match entry.info.tags.get("title") {
            Some(title) => title.clone(),
            None => entry.title.clone(),
        };
        for (out, text) in [
            (title, name),
            (artist, tag("artist")),
            (album, tag("album")),
            (path, entry.path.to_string_lossy().into_owned()),
        ] {
            if !out.is_null() {
                *out = CString::new(text.replace('\0', " ")).unwrap_or_default().into_raw();
            }
        }
        entry.info.length_ms
    })
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

/// `text` as a JSON string.
fn json_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number as JSON, which has no word for one that is not finite.
fn json_number(v: f32) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "0".into()
    }
}

/// The rack as JSON: for every effect what it is, whether it is on, why it is left out of the
/// stream playing when it is, its controls with where each stands, and its presets.
fn effects_json(player: &player::Player) -> String {
    use dsp::lua::Kind;
    let effects: Vec<String> = player
        .rack
        .effects()
        .iter()
        .map(|effect| {
            let info = &effect.plugin.info;
            let params: Vec<String> = effect
                .plugin
                .params
                .iter()
                .zip(&effect.values)
                .map(|(p, value)| {
                    let (kind, options) = match &p.kind {
                        Kind::Number => ("number", Vec::new()),
                        Kind::Toggle => ("toggle", Vec::new()),
                        Kind::Choice(options) => (
                            "choice",
                            options
                                .iter()
                                .map(|(name, v)| format!("{{\"name\":{},\"value\":{}}}", json_text(name), json_number(*v)))
                                .collect(),
                        ),
                    };
                    format!(
                        "{{\"id\":{},\"name\":{},\"kind\":\"{kind}\",\"min\":{},\"max\":{},\"default\":{},\"value\":{},\
                         \"unit\":{},\"help\":{},\"scale\":{},\"decimals\":{},\"zero\":{},\"readout\":{},\"options\":[{}]}}",
                        json_text(&p.id),
                        json_text(&p.name),
                        json_number(p.min),
                        json_number(p.max),
                        json_number(p.default),
                        json_number(*value),
                        json_text(&p.unit),
                        json_text(&p.help),
                        json_number(p.scale),
                        p.decimals.map_or(-1, |d| d as i32),
                        json_text(&p.zero),
                        json_text(&p.readout(*value)),
                        options.join(",")
                    )
                })
                .collect();
            let presets: Vec<String> = effect
                .plugin
                .presets
                .iter()
                .map(|preset| {
                    let values: Vec<String> = preset
                        .values
                        .iter()
                        .map(|(index, v)| format!("[{index},{}]", json_number(*v)))
                        .collect();
                    format!("{{\"name\":{},\"values\":[{}]}}", json_text(&preset.name), values.join(","))
                })
                .collect();
            format!(
                "{{\"id\":{},\"name\":{},\"version\":{},\"author\":{},\"about\":{},\"on\":{},\"error\":{},\
                 \"params\":[{}],\"presets\":[{}]}}",
                json_text(&info.id),
                json_text(&info.name),
                json_text(&info.version),
                json_text(&info.author),
                json_text(&info.about),
                effect.on,
                json_text(&effect.error),
                params.join(","),
                presets.join(",")
            )
        })
        .collect();
    format!("{{\"effects\":[{}]}}", effects.join(","))
}

/// Adds an effect plug-in from the Lua source at `source`, off and at its defaults; a
/// plug-in with the id of one in the rack takes its place. Gives its place in the rack, or -1
/// with the reason in `*error` (to free with [`andamp_string_free`]) when it is refused.
///
/// # Safety
/// `player` is null or a live player; `source` is a C string; `error` is null or points at a pointer.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_add_effect(
    player: *mut AndampPlayer,
    source: *const c_char,
    error: *mut *mut c_char,
) -> i32 {
    let source = string(source);
    let added = with_player(player, Err("the player is gone".to_string()), |p| p.rack.add(&source));
    match added {
        Ok(index) => index as i32,
        Err(message) => {
            if !error.is_null() {
                *error = CString::new(message.replace('\0', " ")).unwrap_or_default().into_raw();
            }
            -1
        }
    }
}

/// Takes effect `index` out of the rack.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_remove_effect(player: *mut AndampPlayer, index: i32) {
    with_player(player, (), |p| {
        if let Ok(index) = usize::try_from(index) {
            p.rack.remove(index);
        }
    });
}

/// The rack as JSON, to free with [`andamp_string_free`]: `{"effects":[...]}`, each effect
/// with `id`, `name`, `version`, `author`, `about`, `on`, `error` (why it is left out of the
/// stream playing, or empty), `params` (each `id`, `name`, `kind` "number", "toggle" or
/// "choice", `min`, `max`, `default`, `value`, `unit`, `help`, `scale`, `decimals` or -1,
/// `zero`, `options` of `name` and `value`) and `presets` (each `name` and `values` as pairs
/// of a control's place and a number).
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_effects(player: *mut AndampPlayer) -> *mut c_char {
    let json = with_player(player, "{\"effects\":[]}".to_string(), |p| effects_json(p));
    CString::new(json).unwrap_or_default().into_raw()
}

/// Switches effect `index`: it fades in or out, so the switch does not click.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_set_effect_on(player: *mut AndampPlayer, index: i32, on: bool) {
    with_player(player, (), |p| {
        if let Ok(index) = usize::try_from(index) {
            p.rack.set_on(index, on);
        }
    });
}

/// Moves control `param` of effect `index` to `value`, in the control's own units.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_set_effect_value(
    player: *mut AndampPlayer,
    index: i32,
    param: i32,
    value: f32,
) {
    with_player(player, (), |p| {
        if let (Ok(index), Ok(param)) = (usize::try_from(index), usize::try_from(param)) {
            p.rack.set_value(index, param, value);
        }
    });
}

/// Applies preset `preset` of effect `index`, or with -1 puts its controls back at their
/// defaults; either way in one step, so nothing sweeps.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_apply_preset(player: *mut AndampPlayer, index: i32, preset: i32) {
    with_player(player, (), |p| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        match usize::try_from(preset) {
            Ok(preset) => p.rack.apply_preset(index, preset),
            Err(_) => p.rack.reset(index),
        }
    });
}

/// The names of the equalizer's factory presets, one to a line, to free with
/// [`andamp_string_free`].
#[no_mangle]
pub extern "C" fn andamp_eq_presets() -> *mut c_char {
    let names: Vec<&str> = player::EQ_PRESETS.iter().map(|p| p.0).collect();
    CString::new(names.join("\n")).unwrap_or_default().into_raw()
}

/// Sets the equalizer to factory preset `index` and switches it on; -1 sets it flat.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_apply_eq_preset(player: *mut AndampPlayer, index: i32) {
    with_player(player, (), |p| p.apply_eq_preset(usize::try_from(index).ok()));
}

/// The switches and sliders a run leaves for the next (the equalizer, the balance, shuffle and
/// repeat) as one line of text, to free with [`andamp_string_free`] and hand to
/// [`andamp_player_restore`] in the next run.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_state(player: *mut AndampPlayer) -> *mut c_char {
    let state = with_player(player, String::new(), |p| p.state());
    CString::new(state).unwrap_or_default().into_raw()
}

/// Puts back what [`andamp_player_state`] gave.
///
/// # Safety
/// `player` is null or a live player; `state` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_restore(player: *mut AndampPlayer, state: *const c_char) {
    let state = string(state);
    with_player(player, (), |p| p.restore(&state));
}

/// The samples heard since the last call, both sides mixed, oldest first, into `out`, which
/// has room for `max`; gives how many there were. Nothing while no song plays. For a
/// visualizer that takes every sample once.
///
/// # Safety
/// `player` is null or a live player; `out` points at `max` floats.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_take_wave(player: *mut AndampPlayer, out: *mut f32, max: i32) -> i32 {
    let Ok(max) = usize::try_from(max) else {
        return 0;
    };
    if out.is_null() {
        return 0;
    }
    let wave = with_player(player, Vec::new(), |p| p.take_wave(max));
    std::ptr::copy_nonoverlapping(wave.as_ptr(), out, wave.len());
    wave.len() as i32
}

/// A text handed to the caller, to free with [`andamp_string_free`].
fn c_text(text: &str) -> *mut c_char {
    CString::new(text.replace('\0', " ")).unwrap_or_default().into_raw()
}

/// What the file or address `location` is, by its name: 0 a song, a folder or a radio stream,
/// 1 a skin, 2 a playlist file.
///
/// # Safety
/// `location` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_location_kind(location: *const c_char) -> i32 {
    match player::location_kind(Path::new(&string(location))) {
        player::LocationKind::Audio => 0,
        player::LocationKind::Skin => 1,
        player::LocationKind::List => 2,
    }
}

/// Adds whatever `location` is to the end of the playlist: a song, a radio stream by its
/// address, the songs a playlist file names, or the songs of a folder and the folders under
/// it. Gives how many entries joined.
///
/// # Safety
/// `player` is null or a live player; `location` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_add_location(player: *mut AndampPlayer, location: *const c_char) -> i32 {
    let location = string(location);
    with_player(player, 0, |p| p.add_location(Path::new(&location)) as i32)
}

/// Answers one of Winamp's keys, by its name: "z" previous, "x" play, "c" or "space" pause,
/// "v" stop, "b" next, "up" and "down" for the volume, "left" and "right" for five seconds
/// back and on. False for any other key.
///
/// # Safety
/// `player` is null or a live player; `key` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_key(player: *mut AndampPlayer, key: *const c_char) -> bool {
    let key = string(key);
    with_player(player, false, |p| p.key(&key))
}

/// Adds every effect plug-in (`.lua`) of the folder `dir` to the rack. Gives a line for each
/// file, to free with [`andamp_string_free`]: its name, its place in the rack or -1, and why
/// it was refused, with tabs between.
///
/// # Safety
/// `player` is null or a live player; `dir` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_add_effects(player: *mut AndampPlayer, dir: *const c_char) -> *mut c_char {
    let dir = string(dir);
    let added = with_player(player, Vec::new(), |p| p.rack.add_folder(Path::new(&dir)));
    let text: String = added
        .iter()
        .map(|(name, result)| match result {
            Ok(index) => format!("{name}\t{index}\t\n"),
            Err(reason) => format!("{name}\t-1\t{}\n", reason.replace(['\t', '\n'], " ")),
        })
        .collect();
    c_text(&text)
}

/// Which effects are on and where their controls stand, as text for the next run, to free
/// with [`andamp_string_free`] and hand to [`andamp_player_restore_effects`].
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_effects_state(player: *mut AndampPlayer) -> *mut c_char {
    c_text(&with_player(player, String::new(), |p| p.rack.state()))
}

/// Puts the effects back as [`andamp_player_effects_state`] left them.
///
/// # Safety
/// `player` is null or a live player; `state` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_restore_effects(player: *mut AndampPlayer, state: *const c_char) {
    let state = string(state);
    with_player(player, (), |p| p.rack.restore(&state));
}

/// Control `param` of effect `index` at `value` as the listener reads it ("50 %", "-3.0 dB"),
/// to free with [`andamp_string_free`].
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_effect_readout(
    player: *mut AndampPlayer,
    index: i32,
    param: i32,
    value: f32,
) -> *mut c_char {
    let text = with_player(player, String::new(), |p| {
        let spec = usize::try_from(index)
            .ok()
            .and_then(|i| p.rack.effects().get(i))
            .and_then(|e| usize::try_from(param).ok().and_then(|i| e.plugin.params.get(i)));
        spec.map(|s| s.readout(value)).unwrap_or_default()
    });
    c_text(&text)
}

/// The settings the app keeps between runs, behind the C interface.
pub struct AndampSettings {
    settings: std::cell::RefCell<settings::Settings>,
}

/// The folder the app keeps its settings and the listener's plug-ins in, to free with
/// [`andamp_string_free`]: Application Support on a Mac, the roaming AppData on Windows, the
/// XDG config folder elsewhere. Null when the system names no home.
#[no_mangle]
pub extern "C" fn andamp_config_dir() -> *mut c_char {
    match settings::config_dir() {
        Some(dir) => c_text(&dir.to_string_lossy()),
        None => std::ptr::null_mut(),
    }
}

/// Opens the settings in the folder `dir`, or with null settings that start empty and are
/// never written, for a run that only looks. Free them with [`andamp_settings_free`].
///
/// # Safety
/// `dir` is null or a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_open(dir: *const c_char) -> *mut AndampSettings {
    let settings = if dir.is_null() {
        settings::Settings::none()
    } else {
        settings::Settings::open(Path::new(&string(dir)))
    };
    Box::into_raw(Box::new(AndampSettings {
        settings: std::cell::RefCell::new(settings),
    }))
}

/// Writes the settings when they changed, and frees them.
///
/// # Safety
/// `settings` is null or settings not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_free(settings: *mut AndampSettings) {
    if !settings.is_null() {
        let settings = Box::from_raw(settings);
        let _ = settings.settings.borrow_mut().save();
    }
}

/// The value of `key`, to free with [`andamp_string_free`]; null when it is not set.
///
/// # Safety
/// `settings` is null or live settings; `key` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_get(settings: *mut AndampSettings, key: *const c_char) -> *mut c_char {
    let Some(handle) = settings.as_ref() else {
        return std::ptr::null_mut();
    };
    match handle.settings.borrow().get(&string(key)) {
        Some(value) => c_text(value),
        None => std::ptr::null_mut(),
    }
}

/// Sets `key` to `value`, or forgets it when `value` is null.
///
/// # Safety
/// `settings` is null or live settings; `key` is a C string, `value` null or one.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_set(
    settings: *mut AndampSettings,
    key: *const c_char,
    value: *const c_char,
) {
    if let Some(handle) = settings.as_ref() {
        let value = (!value.is_null()).then(|| string(value));
        handle.settings.borrow_mut().set(&string(key), value.as_deref());
    }
}

/// The keys that start with `prefix`, one to a line, to free with [`andamp_string_free`].
///
/// # Safety
/// `settings` is null or live settings; `prefix` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_keys(settings: *mut AndampSettings, prefix: *const c_char) -> *mut c_char {
    let keys = settings
        .as_ref()
        .map(|handle| handle.settings.borrow().keys(&string(prefix)).join("\n"))
        .unwrap_or_default();
    c_text(&keys)
}

/// Forgets every key that starts with `prefix`.
///
/// # Safety
/// `settings` is null or live settings; `prefix` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_clear(settings: *mut AndampSettings, prefix: *const c_char) {
    if let Some(handle) = settings.as_ref() {
        handle.settings.borrow_mut().clear(&string(prefix));
    }
}

/// Writes the settings to their file when they changed; false when the file cannot be written.
///
/// # Safety
/// `settings` is null or live settings.
#[no_mangle]
pub unsafe extern "C" fn andamp_settings_save(settings: *mut AndampSettings) -> bool {
    settings
        .as_ref()
        .is_some_and(|handle| handle.settings.borrow_mut().save().is_ok())
}

// the app

/// The app behind the C interface: its player, its settings, the skin worn, its menu and
/// what it asks of the shell ([`app::App`]).
pub struct AndampApp {
    app: app::App,
    /// The app's player as the `andamp_player_` calls take it.
    player: AndampPlayer,
}

/// Runs `f` on the app at `app`; a panic gives `fallback`.
unsafe fn with_app<T>(app: *mut AndampApp, fallback: T, f: impl FnOnce(&mut app::App) -> T) -> T {
    let Some(handle) = app.as_mut() else {
        return fallback;
    };
    match catch_unwind(AssertUnwindSafe(|| f(&mut handle.app))) {
        Ok(v) => v,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            fallback
        }
    }
}

/// The lines of the C string at `text`, without the empty ones.
unsafe fn lines(text: *const c_char) -> Vec<String> {
    string(text).lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Makes the app from the `count` arguments of the command line at `args` (without the
/// program's name), with what comes with it in the folder `resources` (`skins`, `wasabi`,
/// `fonts`, `plugins`), on a screen of `screen_w` x `screen_h` points: the part the windows
/// move on. It reads the settings of the last run. Nothing shows until [`andamp_app_start`].
/// Free it with [`andamp_app_free`].
///
/// # Safety
/// `args` points at `count` C strings; `resources` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_new(
    args: *const *const c_char,
    count: i32,
    resources: *const c_char,
    screen_w: i32,
    screen_h: i32,
) -> *mut AndampApp {
    let args: Vec<String> = if args.is_null() {
        Vec::new()
    } else {
        (0..usize::try_from(count).unwrap_or(0)).map(|i| string(*args.add(i))).collect()
    };
    let resources = string(resources);
    let made = catch_unwind(AssertUnwindSafe(|| app::App::new(&args, Path::new(&resources), (screen_w, screen_h))));
    match made {
        Ok(app) => {
            let player = AndampPlayer {
                player: app.player.clone(),
            };
            Box::into_raw(Box::new(AndampApp { app, player }))
        }
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            std::ptr::null_mut()
        }
    }
}

/// Frees an app [`andamp_app_new`] gave, with its skin and its player.
///
/// # Safety
/// `app` is null or an app not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_free(app: *mut AndampApp) {
    if !app.is_null() {
        let app = Box::from_raw(app);
        let _ = catch_unwind(AssertUnwindSafe(move || drop(app)));
    }
}

/// Wears the first skin and puts the playlist back, or plays what the command line names.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_start(app: *mut AndampApp) {
    with_app(app, (), app::App::start);
}

/// The app's player, for the `andamp_player_` calls. It lives as long as the app and is not
/// to be freed.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_player(app: *mut AndampApp) -> *mut AndampPlayer {
    match app.as_mut() {
        Some(handle) => &mut handle.player,
        None => std::ptr::null_mut(),
    }
}

/// The skin worn, for the `andamp_skin_` calls, or null when none is. It is the app's: not to
/// be freed, and gone when another skin is worn, which the request `skin` says.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_skin(app: *mut AndampApp) -> *mut AndampSkin {
    match app.as_mut().and_then(|handle| handle.app.skin()) {
        Some(skin) => skin,
        None => std::ptr::null_mut(),
    }
}

/// The file name of the skin worn, to free with [`andamp_string_free`].
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_skin_name(app: *mut AndampApp) -> *mut c_char {
    c_text(&with_app(app, String::new(), |a| a.skin_name()))
}

/// Runs the skin and answers what its buttons asked for, in place of [`andamp_skin_step`].
/// The answer is the same.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_step(app: *mut AndampApp) -> u32 {
    let broken = app.as_mut().and_then(|handle| handle.app.skin()).is_some_and(|skin| skin.broken);
    if broken {
        return 0;
    }
    let Some(handle) = app.as_mut() else {
        return 0;
    };
    match catch_unwind(AssertUnwindSafe(|| handle.app.step())) {
        Ok(flags) => flags,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            if let Some(skin) = handle.app.skin() {
                skin.broken = true;
            }
            0
        }
    }
}

/// Milliseconds until the app next has something to do by itself, and so until the next
/// [`andamp_app_step`]: 0 for now, -1 for never. In place of [`andamp_skin_next_due_ms`].
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_next_due_ms(app: *mut AndampApp) -> i64 {
    with_app(app, -1, app::App::next_due_ms)
}

/// What the app asked of the shell since the last call, or null for nothing: a line for each
/// request, its parts with tabs between. Free it with [`andamp_string_free`].
///
/// - `quit`, `hide`, `about`
/// - `ask` and what to let the listener pick: `open_songs`, `add_songs`, `add_folders`,
///   `load_list`, `save_list`, `skin`, `presets`, or `url` for the address of a radio stream.
///   The answer goes to [`andamp_app_picked`].
/// - `popup`, the number of one of the app's menus ([`andamp_app_menu`]) and, when there is
///   one, the place for it in points from the screen's top left corner; without a place it
///   opens at the mouse. The pick goes to [`andamp_app_command`].
/// - `skin`: another skin is worn, and the windows of the one before are gone.
/// - `look`: the size, the opacity or staying on top changed.
/// - `menu`: the menu bar changed.
/// - `effects`: show the Effects window.
/// - `vis` and 1 to open the Visualizer window, 0 to close it; [`andamp_app_vis_shown`] says
///   when it is done.
/// - `preset`, 1 for blending into it, the window's title and the preset's file.
/// - `error` and a message for the listener.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_take_requests(app: *mut AndampApp) -> *mut c_char {
    let requests = with_app(app, Vec::new(), app::App::take_requests);
    if requests.is_empty() {
        return std::ptr::null_mut();
    }
    let text: String = requests.iter().map(|r| r.line() + "\n").collect();
    c_text(&text)
}

/// One of the app's menus as lines of text, to free with [`andamp_string_free`]: 0 the menu
/// bar, 1 the same commands as one popup list, 2 the equalizer's presets. A line is as a
/// skin's menu gives it ([`AndampMenuHandler`]), with the command's key after the text: a
/// letter, with `shift+` before it when Shift is held, to go with the system's command key.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_menu(app: *mut AndampApp, menu: i32) -> *mut c_char {
    let Some(menu) = app::Menu::from_number(menu) else {
        return std::ptr::null_mut();
    };
    c_text(&with_app(app, String::new(), |a| a.menu(menu)))
}

/// Does the command with the number `id`, which an item of [`andamp_app_menu`] carries.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_command(app: *mut AndampApp, id: i32) {
    with_app(app, (), |a| a.command(id));
}

/// What the listener picked when a request `ask` asked for `what`: a line for each file or
/// folder, or the text they typed. Nothing picked needs no call.
///
/// # Safety
/// `app` is null or a live app; `what` and `answer` are C strings.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_picked(app: *mut AndampApp, what: *const c_char, answer: *const c_char) {
    let (what, answer) = (string(what), lines(answer));
    if let Some(ask) = app::Ask::from_name(&what) {
        with_app(app, (), |a| a.picked(ask, &answer));
    }
}

/// What the system asks the app to open, or what was dropped on a window, a line for each:
/// a skin is worn; songs, folders, playlist files and radio streams play. Before
/// [`andamp_app_start`] it waits for the start.
///
/// # Safety
/// `app` is null or a live app; `locations` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_open(app: *mut AndampApp, locations: *const c_char) {
    let locations = lines(locations);
    with_app(app, (), |a| a.open(&locations));
}

/// Sets how the skin's own popup menus are shown, for the skin worn and every one after it;
/// `context` is handed back to `handler`.
///
/// # Safety
/// `app` is null or a live app; `handler` stays callable, with `context`, for as long as the
/// app lives.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_set_menu_handler(
    app: *mut AndampApp,
    handler: Option<AndampMenuHandler>,
    context: *mut c_void,
) {
    with_app(app, (), |a| {
        a.set_menu_handler(handler.map(|show| -> app::MenuHandler {
            Rc::new(move |menu: &str| {
                let text = CString::new(menu.replace('\0', " ")).unwrap_or_default();
                // SAFETY: the caller promised the handler and its context for the app's life
                unsafe { show(context, text.as_ptr()) }
            })
        }));
    });
}

/// Points to a layout pixel of the skin: 2 is Winamp's double size.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_zoom(app: *mut AndampApp) -> i32 {
    with_app(app, 1, |a| a.zoom())
}

/// Whether the windows stay above other apps' windows.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_on_top(app: *mut AndampApp) -> bool {
    with_app(app, false, |a| a.on_top())
}

/// How much of the windows shows over what is behind them, in percent: 20 to 100.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_opacity(app: *mut AndampApp) -> i32 {
    with_app(app, 100, |a| a.opacity())
}

/// The part of the screen the windows move on changed, to `w` x `h` points.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_set_screen(app: *mut AndampApp, w: i32, h: i32) {
    with_app(app, (), |a| a.set_screen((w, h)));
}

/// The shell opened the Visualizer window (`open`) or it closed. When it opens, the app asks
/// for the first preset with a request `preset`.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_shown(app: *mut AndampApp, open: bool) {
    with_app(app, (), |a| a.vis_shown(open));
}

/// A key in the Visualizer window, by its name: `right` the next preset, `left` the one
/// before, `r` one at random, `space` holds the one shown or lets it go. False for any other.
///
/// # Safety
/// `app` is null or a live app; `key` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_key(app: *mut AndampApp, key: *const c_char) -> bool {
    let key = string(key);
    with_app(app, false, |a| a.vis_key(&key))
}

/// Seconds a preset stays: the shell calls [`andamp_app_vis_tick`] that often while the
/// Visualizer window can be seen.
#[no_mangle]
pub extern "C" fn andamp_app_vis_stay() -> i32 {
    vis::STAY_SECONDS as i32
}

/// A preset's stay is over: the next one blends in, unless the listener holds the one shown.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_tick(app: *mut AndampApp) {
    with_app(app, (), app::App::vis_tick);
}

/// Loads every effect plug-in once, those that come with the app and the listener's own, and
/// puts each back as it was left: before the rack is shown.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_load_effects(app: *mut AndampApp) {
    with_app(app, (), app::App::load_effects);
}

/// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder. Gives
/// its place in the rack, or -1 and, when `error` is not null, why in `*error`, to free with
/// [`andamp_string_free`].
///
/// # Safety
/// `app` is null or a live app; `file` is a C string; `error` is null or a place for a pointer.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_add_plugin(
    app: *mut AndampApp,
    file: *const c_char,
    error: *mut *mut c_char,
) -> i32 {
    let file = string(file);
    match with_app(app, Err("the app stopped".to_string()), |a| a.add_plugin(Path::new(&file))) {
        Ok(index) => index as i32,
        Err(message) => {
            if !error.is_null() {
                *error = c_text(&message);
            }
            -1
        }
    }
}

/// Whether effect `index` of the rack is a plug-in the listener added, which
/// [`andamp_app_remove_plugin`] can take out again.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_effect_own(app: *mut AndampApp, index: i32) -> bool {
    with_app(app, false, |a| usize::try_from(index).is_ok_and(|i| a.is_own(i)))
}

/// Takes a plug-in the listener added out of the rack and out of the app's folder.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_remove_plugin(app: *mut AndampApp, index: i32) {
    with_app(app, (), |a| {
        if let Ok(index) = usize::try_from(index) {
            a.remove_plugin(index);
        }
    });
}

/// Keeps what the next run starts from: the skin and where its windows are, the size, the
/// playlist, the volume, the equalizer, the effects. The shell calls it when the app goes to
/// the background and when it ends.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_save(app: *mut AndampApp) {
    with_app(app, (), app::App::save);
}

/// What the command line gave for one of the options the shell answers itself, to free with
/// [`andamp_string_free`], or null when it was not given: `dump`, `bench`, `quit-after`,
/// `then-skin`, `presets` with their value, `hidden`, `show-vis`, `show-effects` with `1`,
/// and `click` with a line for each.
///
/// # Safety
/// `app` is null or a live app; `name` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_option(app: *mut AndampApp, name: *const c_char) -> *mut c_char {
    let name = string(name);
    match with_app(app, None, |a| a.options.get(&name)) {
        Some(value) => c_text(&value),
        None => std::ptr::null_mut(),
    }
}

/// What the app would keep for the next run, in a line, to free with [`andamp_string_free`].
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_summary(app: *mut AndampApp) -> *mut c_char {
    c_text(&with_app(app, String::new(), |a| a.summary()))
}
