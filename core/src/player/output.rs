// SPDX-License-Identifier: GPL-3.0-or-later

//! The way out: the Mac's default output unit, fed from the decoder's ring.
//!
//! The unit takes the song's own sample rate and converts it for the device. Its callback
//! runs on the audio thread: it takes no lock and allocates nothing. What the rest of the
//! player sets (volume, balance, the equalizer) and reads (the last samples, the levels)
//! crosses in atomics ([`Shared`]); a new stretch of audio crosses in a small ring of its own.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use coreaudio::audio_unit::audio_format::LinearPcmFlags;
use coreaudio::audio_unit::render_callback::{self, data};
use coreaudio::audio_unit::{AudioUnit, Element, IOType, SampleFormat, Scope, StreamFormat};

use super::decode::Stream;
use crate::dsp::rack::RackVoice;

/// The equalizer's bands, as Winamp has them, in Hz.
pub const EQ_BANDS: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
/// How far a band or the preamp goes either way, in dB.
const EQ_RANGE_DB: f32 = 12.0;
/// How wide a band is.
const EQ_Q: f32 = 1.2;
/// `kAudioDevicePropertyBufferFrameSize`: how many frames the device asks for at a time.
const DEVICE_BUFFER_FRAME_SIZE: u32 = 0x6673_697a;
/// How many frames the output asks for at a time.
const BLOCK_FRAMES: u32 = 2048;
/// How many of the last samples the visualizer can read.
pub const TAP_SAMPLES: usize = 2048;

/// A float in an atomic.
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        AtomicF32(AtomicU32::new(v.to_bits()))
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// The last samples heard, both channels mixed, for the visualizer. The audio thread writes,
/// the main thread reads; a read that overlaps a write gets a sample or two out of place,
/// which a spectrum does not show.
pub struct Tap {
    samples: Box<[AtomicU32]>,
    at: AtomicUsize,
}

impl Tap {
    fn new() -> Self {
        Tap {
            samples: (0..TAP_SAMPLES).map(|_| AtomicU32::new(0)).collect(),
            at: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn push(&self, at: &mut usize, v: f32) {
        self.samples[*at % TAP_SAMPLES].store(v.to_bits(), Ordering::Relaxed);
        *at = at.wrapping_add(1);
    }

    /// The last `n` samples, oldest first.
    pub fn last(&self, n: usize) -> Vec<f32> {
        let n = n.min(TAP_SAMPLES);
        let end = self.at.load(Ordering::Relaxed);
        (0..n)
            .map(|i| {
                let at = end.wrapping_sub(n).wrapping_add(i) % TAP_SAMPLES;
                f32::from_bits(self.samples[at].load(Ordering::Relaxed))
            })
            .collect()
    }

    /// The samples heard since `cursor` was last here, oldest first, at most `max` of them (the
    /// newest when there were more); `cursor` moves on to now. For a visualizer that wants
    /// every sample once.
    pub fn since(&self, cursor: &mut usize, max: usize) -> Vec<f32> {
        let end = self.at.load(Ordering::Relaxed);
        let fresh = end.wrapping_sub(*cursor).min(TAP_SAMPLES).min(max);
        *cursor = end;
        (0..fresh)
            .map(|i| {
                let at = end.wrapping_sub(fresh).wrapping_add(i) % TAP_SAMPLES;
                f32::from_bits(self.samples[at].load(Ordering::Relaxed))
            })
            .collect()
    }

    /// Forgets what was heard.
    pub fn clear(&self) {
        for s in self.samples.iter() {
            s.store(0, Ordering::Relaxed);
        }
    }
}

/// What the player and the audio thread share.
pub struct Shared {
    /// Whether the output plays; false leaves the ring alone and puts out silence.
    pub playing: AtomicBool,
    /// The volume, 0..1.
    pub volume: AtomicF32,
    /// The balance as a gain for each side, 0..1.
    pub left: AtomicF32,
    pub right: AtomicF32,
    /// The equalizer: on or off, ten bands and the preamp 0..255 with 128 in the middle, and a
    /// count that goes up when any of them changes.
    pub eq_on: AtomicBool,
    pub eq_bands: [AtomicI32; 10],
    pub eq_preamp: AtomicI32,
    pub eq_version: AtomicU32,
    /// The last samples heard.
    pub tap: Tap,
    /// The loudest sample of each side in the last block, 0..1.
    pub level_left: AtomicF32,
    pub level_right: AtomicF32,
    /// Blocks in which the ring ran empty while the song went on.
    pub underruns: AtomicU64,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            playing: AtomicBool::new(false),
            volume: AtomicF32::new(1.0),
            left: AtomicF32::new(1.0),
            right: AtomicF32::new(1.0),
            eq_on: AtomicBool::new(false),
            eq_bands: std::array::from_fn(|_| AtomicI32::new(128)),
            eq_preamp: AtomicI32::new(128),
            eq_version: AtomicU32::new(1),
            tap: Tap::new(),
            level_left: AtomicF32::new(0.0),
            level_right: AtomicF32::new(0.0),
            underruns: AtomicU64::new(0),
        })
    }
}

/// A band's or the preamp's value 0..255 as dB.
pub fn eq_db(value: i32) -> f32 {
    ((value - 128) as f32 / 127.0 * EQ_RANGE_DB).clamp(-EQ_RANGE_DB, EQ_RANGE_DB)
}

/// One band: a peaking filter and what it remembers of each side.
#[derive(Clone, Copy, Default)]
struct Band {
    on: bool,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z: [[f32; 2]; 2],
}

impl Band {
    fn set(&mut self, hz: f32, db: f32, sample_rate: f32) {
        // a flat band, or one above what the sample rate carries, does nothing
        self.on = db.abs() > 0.05 && hz < sample_rate * 0.45;
        if !self.on {
            return;
        }
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * hz / sample_rate;
        let alpha = w.sin() / (2.0 * EQ_Q);
        let a0 = 1.0 + alpha / a;
        self.b0 = (1.0 + alpha * a) / a0;
        self.b1 = -2.0 * w.cos() / a0;
        self.b2 = (1.0 - alpha * a) / a0;
        self.a1 = self.b1;
        self.a2 = (1.0 - alpha / a) / a0;
    }

    #[inline(always)]
    fn run(&mut self, side: usize, x: f32) -> f32 {
        let z = &mut self.z[side];
        let y = self.b0 * x + z[0];
        z[0] = self.b1 * x - self.a1 * y + z[1];
        z[1] = self.b2 * x - self.a2 * y;
        y
    }
}

/// The ten bands and the preamp, as the audio thread runs them.
pub struct Equalizer {
    bands: [Band; 10],
    preamp: f32,
    version: u32,
    sample_rate: f32,
}

impl Equalizer {
    pub fn new(sample_rate: u32) -> Self {
        Equalizer {
            bands: [Band::default(); 10],
            preamp: 1.0,
            version: 0,
            sample_rate: sample_rate as f32,
        }
    }

    /// Takes the bands and the preamp from `shared` when they changed.
    fn follow(&mut self, shared: &Shared) {
        let version = shared.eq_version.load(Ordering::Relaxed);
        if version == self.version {
            return;
        }
        self.version = version;
        for (band, (hz, value)) in self.bands.iter_mut().zip(EQ_BANDS.iter().zip(&shared.eq_bands)) {
            band.set(*hz, eq_db(value.load(Ordering::Relaxed)), self.sample_rate);
        }
        self.preamp = 10f32.powf(eq_db(shared.eq_preamp.load(Ordering::Relaxed)) / 20.0);
    }

    /// Forgets what the filters remember: a new song starts clean.
    fn clear(&mut self) {
        for band in &mut self.bands {
            band.z = [[0.0; 2]; 2];
        }
    }

    /// Runs `samples` (left and right in turn) through the preamp and the bands.
    pub fn run(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_exact_mut(2) {
            for (side, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * self.preamp;
                for band in &mut self.bands {
                    if band.on {
                        x = band.run(side, x);
                    }
                }
                *sample = x;
            }
        }
    }

    /// An equalizer with these bands and this preamp, for tests and offline use.
    pub fn with(sample_rate: u32, bands: [i32; 10], preamp: i32) -> Self {
        let shared = Shared::new();
        for (slot, value) in shared.eq_bands.iter().zip(bands) {
            slot.store(value, Ordering::Relaxed);
        }
        shared.eq_preamp.store(preamp, Ordering::Relaxed);
        let mut eq = Equalizer::new(sample_rate);
        eq.follow(&shared);
        eq
    }
}

/// A stretch of audio handed to the output: the stream and the end of its ring to read.
pub struct Source {
    pub stream: Arc<Stream>,
    pub ring: rtrb::Consumer<f32>,
}

/// What the audio thread keeps between blocks.
struct Voice {
    shared: Arc<Shared>,
    /// New sources arrive here; the ones they replace go back through `done`, so that a ring
    /// is never freed on the audio thread.
    incoming: rtrb::Consumer<Source>,
    done: rtrb::Producer<Source>,
    current: Option<Source>,
    eq: Equalizer,
    /// The effects, after the equalizer.
    rack: RackVoice,
    tap_at: usize,
}

impl Voice {
    /// Fills `out` (left and right in turn) with the next block.
    fn render(&mut self, out: &mut [f32]) {
        while let Ok(next) = self.incoming.pop() {
            if let Some(old) = self.current.replace(next) {
                // full only if the player stopped collecting: then the ring is freed here
                let _ = self.done.push(old);
            }
            self.eq.clear();
        }
        let shared = &*self.shared;
        let source = match &mut self.current {
            Some(source) if shared.playing.load(Ordering::Relaxed) => source,
            _ => {
                out.fill(0.0);
                shared.level_left.set(0.0);
                shared.level_right.set(0.0);
                return;
            }
        };
        let wanted = out.len();
        let (filled, rest) = source.ring.pop_partial_slice(out);
        let got = filled.len() & !1;
        rest.fill(0.0);
        if got < wanted {
            if source.stream.decoded_all.load(Ordering::Acquire) && source.ring.is_empty() {
                source.stream.ended.store(true, Ordering::Release);
            } else {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        source
            .stream
            .frames_played
            .fetch_add((got / 2) as u64, Ordering::Relaxed);
        let heard = &mut out[..got];
        if shared.eq_on.load(Ordering::Relaxed) {
            self.eq.follow(shared);
            self.eq.run(heard);
        }
        self.rack.run(heard);
        // the visualizer and the levels read what is heard before the volume
        let (mut peak_left, mut peak_right) = (0f32, 0f32);
        let mut at = self.tap_at;
        for frame in heard.chunks_exact(2) {
            shared.tap.push(&mut at, (frame[0] + frame[1]) * 0.5);
            peak_left = peak_left.max(frame[0].abs());
            peak_right = peak_right.max(frame[1].abs());
        }
        self.tap_at = at;
        shared.tap.at.store(at, Ordering::Relaxed);
        shared.level_left.set(peak_left);
        shared.level_right.set(peak_right);
        let volume = shared.volume.get();
        let (left, right) = (volume * shared.left.get(), volume * shared.right.get());
        if left != 1.0 || right != 1.0 {
            for frame in heard.chunks_exact_mut(2) {
                frame[0] *= left;
                frame[1] *= right;
            }
        }
    }
}

/// The output unit, set to one sample rate.
pub struct Output {
    unit: AudioUnit,
    sources: rtrb::Producer<Source>,
    replaced: rtrb::Consumer<Source>,
    /// The sample rate the unit takes.
    pub sample_rate: u32,
    running: bool,
}

impl Output {
    /// Opens the default output for stereo float at `sample_rate`, with `rack` as its effects.
    pub fn new(sample_rate: u32, shared: Arc<Shared>, rack: RackVoice) -> Result<Output, String> {
        let error = |e: coreaudio::Error| format!("the audio output did not open: {e}");
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
        let (sources, incoming) = rtrb::RingBuffer::new(8);
        let (done, replaced) = rtrb::RingBuffer::new(8);
        let mut voice = Voice {
            shared,
            incoming,
            done,
            current: None,
            eq: Equalizer::new(sample_rate),
            rack,
            tap_at: 0,
        };
        type Args = render_callback::Args<data::Interleaved<f32>>;
        unit.set_render_callback(move |args: Args| {
            voice.render(args.data.buffer);
            Ok(())
        })
        .map_err(error)?;
        unit.initialize().map_err(error)?;
        Ok(Output {
            unit,
            sources,
            replaced,
            sample_rate,
            running: false,
        })
    }

    /// Hands the output a new stretch of audio, in place of the one it plays.
    pub fn play(&mut self, source: Source) {
        self.collect();
        // full only when the unit does not run and many seeks piled up: the newest wins later
        let _ = self.sources.push(source);
    }

    /// Frees the stretches the audio thread is done with.
    pub fn collect(&mut self) {
        while self.replaced.pop().is_ok() {}
    }

    /// Starts the unit; its callback runs from now on.
    pub fn start(&mut self) {
        if !self.running {
            self.running = self.unit.start().is_ok();
        }
    }

    /// Stops the unit: no callback, no wakeups.
    pub fn stop(&mut self) {
        if self.running {
            let _ = self.unit.stop();
            self.running = false;
        }
    }
}
