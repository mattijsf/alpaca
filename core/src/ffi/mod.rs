// SPDX-License-Identifier: GPL-3.0-or-later

//! The C interface (`include/andamp_core.h`): the app, its player and the skin worn, for a
//! shell written in another language than Rust.
//!
//! This is the only module with unsafe code, and it holds no logic: every function turns its
//! pointers into Rust values, calls one method and turns the answer back. A panic stops at
//! this boundary: the call gives its fallback answer, and a skin whose engine panicked does
//! nothing more. Text goes out as C strings the caller frees with [`andamp_string_free`], and
//! comes in as UTF-8.

mod app;
mod player;
mod skin;

pub use app::*;
pub use player::*;
pub use skin::*;

use std::ffi::{c_char, CStr, CString};

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

/// Frees text a call gave.
///
/// # Safety
/// `text` is null or a message not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(CString::from_raw(text));
    }
}

/// Frees whole numbers a call gave, with the count it gave.
///
/// # Safety
/// `words` is null, or `count` numbers a call gave that were not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_words_free(words: *mut u32, count: usize) {
    if !words.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            words, count,
        )));
    }
}

/// Whole numbers handed to the caller with how many there are, to free with
/// [`andamp_words_free`]; null and 0 for none.
unsafe fn give_words(words: Option<Vec<u32>>, count: *mut usize) -> *mut u32 {
    let words = words.filter(|w| !w.is_empty());
    if !count.is_null() {
        *count = words.as_ref().map_or(0, Vec::len);
    }
    match words {
        Some(words) => Box::into_raw(words.into_boxed_slice()).cast::<u32>(),
        None => std::ptr::null_mut(),
    }
}

/// A text handed to the caller, to free with [`andamp_string_free`].
fn c_text(text: &str) -> *mut c_char {
    CString::new(text.replace('\0', " "))
        .unwrap_or_default()
        .into_raw()
}

/// The lines of the C string at `text`, without the empty ones.
unsafe fn lines(text: *const c_char) -> Vec<String> {
    string(text)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}
