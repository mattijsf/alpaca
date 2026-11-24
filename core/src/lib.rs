// SPDX-License-Identifier: GPL-3.0-or-later

//! Andamp for the desktop, less what only a system can do: the app, the player and the skin
//! engine's host, the same on macOS, Windows and Linux.
//!
//! - [`app`] is the app itself: its command line, its menu, what each command does, what it
//!   keeps between runs. A shell written in Rust starts here.
//! - [`skin`] runs a skin in the walkit engine with its windows set out apart, [`host`]
//!   decodes its pictures and fonts, and [`render`] draws each window into a buffer of pixels.
//! - [`player`] is the playlist, decoding and the audio output; [`dsp`] the effects.
//! - [`settings`] is the file kept between runs, [`vis`] the Visualizer's presets.
//! - [`ffi`] is the same as a C interface (`include/andamp_core.h`), for a shell in another
//!   language. It is the only module with unsafe code.
//!
//! The engine is not shared across threads: everything but the audio runs on the thread the
//! shell calls from. ENGINEERING.md in the repository has the rules.

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub mod app;
pub mod dsp;
#[allow(unsafe_code)]
pub mod ffi;
pub mod host;
pub mod player;
pub mod render;
pub mod settings;
pub mod skin;
pub mod vis;
