// SPDX-License-Identifier: GPL-3.0-or-later

//! The player: what plays, the volume, the effect rack and what the visualizer reads.

use std::cell::RefCell;
use std::ffi::{c_char, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use super::{c_text, panic_text};
use crate::player::Player;

/// The app's player behind the C interface. Every skin opened with it shows and drives it.
pub struct AndampPlayer {
    pub(super) player: Rc<RefCell<Player>>,
}

/// Runs `f` on the player at `player`; a panic gives `fallback`.
unsafe fn with_player<T>(
    player: *mut AndampPlayer,
    fallback: T,
    f: impl FnOnce(&mut Player) -> T,
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

/// How many entries the playlist has.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_count(player: *mut AndampPlayer) -> i32 {
    with_player(player, 0, |p| p.entries().len() as i32)
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

/// Moves the player on when its song has ended. A skin's step does this by itself; the app
/// calls it while no skin steps, so that the playlist goes on with every window out of sight.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_poll(player: *mut AndampPlayer) {
    with_player(player, (), Player::poll);
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
/// to free with [`andamp_string_free`](super::andamp_string_free); a tag the file does not carry comes out empty.
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
                *out = CString::new(text.replace('\0', " "))
                    .unwrap_or_default()
                    .into_raw();
            }
        }
        entry.info.length_ms
    })
}

/// Blocks of audio in which the output ran dry while a song went on, so far.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_underruns(player: *mut AndampPlayer) -> u64 {
    with_player(player, 0, |p| p.underruns())
}

/// The rack as JSON, to free with [`andamp_string_free`](super::andamp_string_free): `{"effects":[...]}`, each effect
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
    let json = with_player(player, "{\"effects\":[]}".to_string(), |p| {
        crate::dsp::describe::json(&p.rack)
    });
    CString::new(json).unwrap_or_default().into_raw()
}

/// Switches effect `index`: it fades in or out, so the switch does not click.
///
/// # Safety
/// `player` is null or a live player.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_set_effect_on(
    player: *mut AndampPlayer,
    index: i32,
    on: bool,
) {
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
pub unsafe extern "C" fn andamp_player_apply_preset(
    player: *mut AndampPlayer,
    index: i32,
    preset: i32,
) {
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

/// The samples heard since the last call, both sides mixed, oldest first, into `out`, which
/// has room for `max`; gives how many there were. Nothing while no song plays. For a
/// visualizer that takes every sample once.
///
/// # Safety
/// `player` is null or a live player; `out` points at `max` floats.
#[no_mangle]
pub unsafe extern "C" fn andamp_player_take_wave(
    player: *mut AndampPlayer,
    out: *mut f32,
    max: i32,
) -> i32 {
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

/// Control `param` of effect `index` at `value` as the listener reads it ("50 %", "-3.0 dB"),
/// to free with [`andamp_string_free`](super::andamp_string_free).
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
            .and_then(|e| {
                usize::try_from(param)
                    .ok()
                    .and_then(|i| e.plugin.params.get(i))
            });
        spec.map(|s| s.readout(value)).unwrap_or_default()
    });
    c_text(&text)
}
