// SPDX-License-Identifier: GPL-3.0-or-later

//! The default output device through the `cpal` crate: WASAPI on Windows, ALSA on Linux
//! (and Core Audio on a Mac, with the `portable-output` feature).
//!
//! The device runs at its own sample rate and channel count. A song at another rate is
//! converted ([`Resampler`]); its left and right go to the device's first two channels, or
//! mixed to its one.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::resample::Resampler;
use super::Fill;

/// The system's default output device through `cpal`, open for one sample rate.
pub struct Device {
    stream: cpal::Stream,
}

/// What the audio thread keeps: the filler, the converter when the rates differ, and a block
/// of stereo to convert the device's block from.
struct Feed {
    fill: Fill,
    resampler: Option<Resampler>,
    stereo: Vec<f32>,
    channels: usize,
}

impl Feed {
    /// Fills one block of the device, `channels` samples to a frame.
    fn run<T: SizedSample + FromSample<f32>>(&mut self, out: &mut [T]) {
        let frames = out.len() / self.channels.max(1);
        // sized once to the largest block the device asks for
        if self.stereo.len() < frames * 2 {
            self.stereo.resize(frames * 2, 0.0);
        }
        let stereo = &mut self.stereo[..frames * 2];
        match &mut self.resampler {
            Some(resampler) => resampler.run(stereo, &mut *self.fill),
            None => (self.fill)(stereo),
        }
        for (frame, pair) in out
            .chunks_exact_mut(self.channels)
            .zip(stereo.chunks_exact(2))
        {
            if frame.len() == 1 {
                frame[0] = T::from_sample(f32::midpoint(pair[0], pair[1]));
                continue;
            }
            frame[0] = T::from_sample(pair[0]);
            frame[1] = T::from_sample(pair[1]);
            for other in &mut frame[2..] {
                *other = T::from_sample(0.0);
            }
        }
    }
}

impl Device {
    /// Opens the default output, filled by `fill` with stereo at `sample_rate`.
    pub fn open(sample_rate: u32, fill: Fill) -> Result<Device, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("there is no audio output")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0;
        let mut feed = Feed {
            fill,
            resampler: (rate != sample_rate).then(|| Resampler::new(sample_rate, rate)),
            stereo: Vec::new(),
            channels: usize::from(config.channels).max(1),
        };
        let failed = |e: cpal::StreamError| eprintln!("andamp: audio output: {e}");
        let stream = match format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _: &cpal::OutputCallbackInfo| feed.run(out),
                failed,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |out: &mut [i16], _: &cpal::OutputCallbackInfo| feed.run(out),
                failed,
                None,
            ),
            SampleFormat::I32 => device.build_output_stream(
                &config,
                move |out: &mut [i32], _: &cpal::OutputCallbackInfo| feed.run(out),
                failed,
                None,
            ),
            other => {
                return Err(format!(
                    "the audio output takes {other} samples, which is not handled"
                ))
            }
        }
        .map_err(|e| e.to_string())?;
        // a stream may start by itself: it waits until it is asked
        let _ = stream.pause();
        Ok(Device { stream })
    }

    /// Starts the stream; false when it did not start.
    pub fn start(&mut self) -> bool {
        self.stream.play().is_ok()
    }

    /// Stops the stream: no callback, no wakeups.
    pub fn stop(&mut self) {
        let _ = self.stream.pause();
    }
}
