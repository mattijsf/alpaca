// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio device: the one part of the player that differs from system to system.
//!
//! A [`Device`] is opened for a sample rate with something to fill its blocks: stereo, 32-bit
//! float, left and right in turn, at that rate. It calls it on the audio thread while it is
//! started, and not at all while it is stopped.
//!
//! - On macOS it is the default output unit ([`coreaudio`]), which converts the song's
//!   sample rate for the device itself.
//! - Everywhere else it is the `cpal` crate ([`portable`]): WASAPI on Windows, ALSA on Linux.
//!   A song at another rate than the device's is converted here.
//!
//! The feature `portable-output` takes the portable one on macOS too, so that it can be built,
//! run and measured on a Mac.

/// Fills a block: as many samples as it has room for, left and right in turn.
pub type Fill = Box<dyn FnMut(&mut [f32]) + Send>;

#[cfg(all(target_os = "macos", not(feature = "portable-output")))]
mod coreaudio;
#[cfg(all(target_os = "macos", not(feature = "portable-output")))]
pub use coreaudio::Device;

#[cfg(any(not(target_os = "macos"), feature = "portable-output"))]
mod portable;
#[cfg(any(not(target_os = "macos"), feature = "portable-output"))]
pub use portable::Device;

pub mod resample;
