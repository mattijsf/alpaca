// SPDX-License-Identifier: GPL-3.0-or-later

//! The Mac's default output unit. It takes the song's own sample rate and converts it for the
//! device, and follows the listener when they pick another output.

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::render_callback::{self, data};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat};

use super::Fill;

/// `kAudioDevicePropertyBufferFrameSize`: how many frames the device asks for at a time.
const DEVICE_BUFFER_FRAME_SIZE: u32 = 0x6673_697a;
/// How many frames the output asks for at a time.
const BLOCK_FRAMES: u32 = 2048;

/// The Mac's default output unit, open for one sample rate.
pub struct Device {
    unit: AudioUnit,
}

impl Device {
    /// Opens the default output for stereo float at `sample_rate`, filled by `fill`.
    pub fn open(sample_rate: u32, mut fill: Fill) -> Result<Device, String> {
        let error = |e: coreaudio::Error| e.to_string();
        let mut unit = AudioUnit::new(IOType::DefaultOutput).map_err(error)?;
        unit.uninitialize().map_err(error)?;
        let format = StreamFormat {
            sample_rate: f64::from(sample_rate),
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED,
            channels: 2,
        };
        unit.set_stream_format(format, Scope::Input, Element::Output)
            .map_err(error)?;
        // a block of 2048 frames wakes the audio thread about 22 times a second, where the
        // default of 512 wakes it 86 times; a volume change is heard 46 ms later at most
        let _ = unit.set_property(
            DEVICE_BUFFER_FRAME_SIZE,
            Scope::Global,
            Element::Output,
            Some(&BLOCK_FRAMES),
        );
        unit.set_render_callback(move |args: render_callback::Args<data::Interleaved<f32>>| {
            fill(args.data.buffer);
            Ok(())
        })
        .map_err(error)?;
        unit.initialize().map_err(error)?;
        Ok(Device { unit })
    }

    /// Starts the unit; false when it did not start.
    pub fn start(&mut self) -> bool {
        self.unit.start().is_ok()
    }

    /// Stops the unit: no callback, no wakeups.
    pub fn stop(&mut self) {
        let _ = self.unit.stop();
    }
}
