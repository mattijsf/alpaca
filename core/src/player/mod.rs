// SPDX-License-Identifier: GPL-3.0-or-later

//! The player: the playlist, the transport and the audio behind them.
//!
//! [`Player`] lives on the main thread. A song is decoded on a thread of its own
//! ([`decode`]) into a ring that the output's callback empties ([`output`]). The skin reads and
//! drives the player through [`SkinPlayer`], which is walkit's `Player` over the same object,
//! so the app's menus and the skin's buttons act on one player, and a skin can be changed
//! while a song plays.

pub mod decode;
pub mod output;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use walkit::player::{Status, Track};

use decode::{FileInfo, Stream};
use output::{Output, Shared, Source};

/// How many samples the spectrum is worked out from.
const FFT_SIZE: usize = 1024;
/// How many bands a skin's visualizer gets, as Winamp hands them out.
const SPECTRUM_BANDS: usize = 75;
/// The lowest and highest frequency the bands cover, in Hz.
const SPECTRUM_RANGE: (f32, f32) = (40.0, 16000.0);
/// How far below full scale a band still shows, in dB.
const SPECTRUM_FLOOR_DB: f32 = 66.0;
/// How many samples of the wave a skin's oscilloscope gets.
const WAVE_SAMPLES: usize = 576;

/// A request the player cannot answer by itself: the app does.
pub const REQUEST_OPEN_FILES: u32 = 1;

/// One entry of the playlist.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    /// The title shown: "artist - title" from the tags, else the file's name.
    pub title: String,
    pub info: FileInfo,
}

impl Entry {
    /// Reads the file at `path`: what it is and what it is called.
    pub fn read(path: &Path) -> Result<Entry, String> {
        let info = decode::probe(path)?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = match (info.tags.get("artist"), info.tags.get("title")) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => name,
        };
        Ok(Entry {
            path: path.to_path_buf(),
            title,
            info,
        })
    }
}

/// The song the output has, and where in the file it started.
struct Playing {
    stream: Arc<Stream>,
    start_ms: i32,
    /// The ring's reading end when there is no output to hand it to: the player empties it
    /// itself, as fast as the decoder fills it.
    unheard: Option<rtrb::Consumer<f32>>,
}

/// The player.
pub struct Player {
    shared: Arc<Shared>,
    output: Option<Output>,
    playlist: Vec<Entry>,
    index: usize,
    status: Status,
    playing: Option<Playing>,
    /// Where the song stands while nothing is loaded for it: after a stop, or a seek while stopped.
    resume_ms: i32,
    volume: i32,
    pan: i32,
    shuffle: bool,
    repeat: bool,
    crossfade: bool,
    eq_bands: [i32; 10],
    eq_preamp: i32,
    eq_on: bool,
    requests: u32,
    fft: Arc<dyn Fft<f32>>,
    /// The entries played since shuffle came on, so that previous goes back through them.
    shuffled: Vec<usize>,
    seed: u32,
    /// Whether the output may make sound: tests leave it shut.
    audible: bool,
}

impl Default for Player {
    fn default() -> Self {
        Player::new()
    }
}

impl Player {
    pub fn new() -> Player {
        Player {
            shared: Shared::new(),
            output: None,
            playlist: Vec::new(),
            index: 0,
            status: Status::Stopped,
            playing: None,
            resume_ms: 0,
            volume: 255,
            pan: 0,
            shuffle: false,
            repeat: false,
            crossfade: false,
            eq_bands: [128; 10],
            eq_preamp: 128,
            eq_on: false,
            requests: 0,
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
            shuffled: Vec::new(),
            seed: 0x2545_f491,
            audible: true,
        }
    }

    /// A player that decodes and keeps time but opens no audio output.
    pub fn silent() -> Player {
        let mut player = Player::new();
        player.audible = false;
        player
    }

    // the playlist

    /// Adds the file at `path` to the end of the playlist; its place, or what is wrong with it.
    pub fn add(&mut self, path: &Path) -> Result<usize, String> {
        self.playlist.push(Entry::read(path)?);
        Ok(self.playlist.len() - 1)
    }

    /// Empties the playlist and stops.
    pub fn clear(&mut self) {
        self.stop();
        self.playlist.clear();
        self.index = 0;
    }

    pub fn entries(&self) -> &[Entry] {
        &self.playlist
    }

    pub fn index(&self) -> usize {
        self.index
    }

    fn entry(&self) -> Option<&Entry> {
        self.playlist.get(self.index)
    }

    // the transport

    pub fn status(&self) -> Status {
        self.status
    }

    /// Where the song is, in milliseconds.
    pub fn position_ms(&self) -> i32 {
        match &self.playing {
            Some(p) => {
                let frames = p.stream.frames_played.load(Ordering::Relaxed);
                let rate = u64::from(p.stream.sample_rate.max(1));
                let at = p.start_ms + (frames * 1000 / rate) as i32;
                match self.length_ms() {
                    0 => at,
                    length => at.min(length),
                }
            }
            None => self.resume_ms,
        }
    }

    pub fn length_ms(&self) -> i32 {
        self.entry().map_or(0, |e| e.info.length_ms)
    }

    /// Loads the current entry from `start_ms` and hands it to the output, playing or held.
    fn load(&mut self, start_ms: i32, play: bool) {
        self.unload();
        let Some(entry) = self.entry() else {
            self.status = Status::Stopped;
            return;
        };
        let rate = entry.info.sample_rate.max(8000);
        let (stream, ring) = decode::spawn(entry.path.clone(), start_ms, rate);
        if self.audible {
            // the unit takes one sample rate: a song at another gets a unit of its own
            if self.output.as_ref().is_some_and(|o| o.sample_rate != rate) {
                self.output = None;
            }
            if self.output.is_none() {
                match Output::new(rate, self.shared.clone()) {
                    Ok(output) => self.output = Some(output),
                    Err(message) => eprintln!("andamp: {message}"),
                }
            }
        }
        let unheard = match &mut self.output {
            Some(output) => {
                output.play(Source {
                    stream: stream.clone(),
                    ring,
                });
                None
            }
            None => Some(ring),
        };
        self.playing = Some(Playing {
            stream,
            start_ms,
            unheard,
        });
        self.resume_ms = start_ms;
        self.run(play);
    }

    /// Lets the output run or holds it.
    fn run(&mut self, play: bool) {
        self.shared.playing.store(play, Ordering::Relaxed);
        if let Some(output) = &mut self.output {
            if play {
                output.start();
            } else {
                output.stop();
            }
        }
        self.status = if play { Status::Playing } else { Status::Paused };
    }

    /// Lets go of the song loaded: its decoder stops.
    fn unload(&mut self) {
        if let Some(p) = self.playing.take() {
            p.stream.cancel.store(true, Ordering::Release);
        }
        self.shared.tap.clear();
    }

    pub fn play(&mut self) {
        match self.status {
            Status::Paused if self.playing.is_some() => self.run(true),
            Status::Playing => self.load(0, true),
            _ => {
                let from = std::mem::take(&mut self.resume_ms);
                self.load(from, true);
            }
        }
    }

    /// Pauses, or goes on when paused.
    pub fn pause(&mut self) {
        match self.status {
            Status::Playing => self.run(false),
            Status::Paused => self.play(),
            Status::Stopped => {}
        }
    }

    pub fn stop(&mut self) {
        self.unload();
        self.shared.playing.store(false, Ordering::Relaxed);
        if let Some(output) = &mut self.output {
            output.stop();
            output.collect();
        }
        self.status = Status::Stopped;
        self.resume_ms = 0;
    }

    /// Makes entry `index` current and plays it.
    pub fn play_index(&mut self, index: usize) {
        if index < self.playlist.len() {
            self.index = index;
            self.load(0, true);
        }
    }

    /// The entry after this one: the next in the list, or any other with shuffle on. `None`
    /// at the end of the list with repeat off.
    fn following(&mut self) -> Option<usize> {
        let n = self.playlist.len();
        if n == 0 {
            return None;
        }
        if self.shuffle && n > 1 {
            // a small generator is enough to pick a song
            self.seed ^= self.seed << 13;
            self.seed ^= self.seed >> 17;
            self.seed ^= self.seed << 5;
            let step = 1 + self.seed as usize % (n - 1);
            return Some((self.index + step) % n);
        }
        match self.index + 1 {
            next if next < n => Some(next),
            _ if self.repeat => Some(0),
            _ => None,
        }
    }

    pub fn next(&mut self) {
        let was = self.status;
        if let Some(next) = self.following().or((!self.playlist.is_empty()).then_some(0)) {
            if self.shuffle {
                self.shuffled.push(self.index);
            }
            self.index = next;
            self.resume_ms = 0;
            match was {
                Status::Stopped => self.unload(),
                _ => self.load(0, was == Status::Playing),
            }
        }
    }

    pub fn previous(&mut self) {
        let was = self.status;
        let n = self.playlist.len();
        if n == 0 {
            return;
        }
        self.index = match self.shuffled.pop() {
            Some(before) if self.shuffle && before < n => before,
            _ => (self.index + n - 1) % n,
        };
        self.resume_ms = 0;
        match was {
            Status::Stopped => self.unload(),
            _ => self.load(0, was == Status::Playing),
        }
    }

    pub fn seek_to(&mut self, ms: i32) {
        let ms = ms.clamp(0, self.length_ms().max(0));
        match self.status {
            Status::Stopped => self.resume_ms = ms,
            status => self.load(ms, status == Status::Playing),
        }
    }

    /// Moves on when the song has ended. Called before the skin reads the player.
    pub fn poll(&mut self) {
        if let Some(output) = &mut self.output {
            output.collect();
        }
        // with no output the player takes what the decoder made, and the song is over when
        // the decoder is done and the ring is empty
        if let (Status::Playing, Some(p)) = (self.status, &mut self.playing) {
            if let Some(ring) = &mut p.unheard {
                let done = p.stream.decoded_all.load(Ordering::Acquire);
                let mut taken = 0;
                while let Ok(chunk) = ring.read_chunk(ring.slots()) {
                    if chunk.is_empty() {
                        break;
                    }
                    taken += chunk.len();
                    chunk.commit_all();
                }
                p.stream
                    .frames_played
                    .fetch_add((taken / 2) as u64, Ordering::Relaxed);
                if done && ring.is_empty() {
                    p.stream.ended.store(true, Ordering::Release);
                }
            }
        }
        let ended = self.status == Status::Playing
            && self
                .playing
                .as_ref()
                .is_some_and(|p| p.stream.ended.load(Ordering::Acquire));
        if !ended {
            return;
        }
        match self.following() {
            Some(next) => {
                if self.shuffle {
                    self.shuffled.push(self.index);
                }
                self.index = next;
                self.load(0, true);
            }
            None => {
                self.stop();
                self.index = 0;
            }
        }
    }

    // what is heard

    pub fn volume(&self) -> i32 {
        self.volume
    }

    pub fn set_volume(&mut self, v: i32) {
        self.volume = v.clamp(0, 255);
        self.shared.volume.set(self.volume as f32 / 255.0);
    }

    pub fn pan(&self) -> i32 {
        self.pan
    }

    pub fn set_pan(&mut self, v: i32) {
        self.pan = v.clamp(-127, 127);
        let p = self.pan as f32 / 127.0;
        self.shared.left.set(1.0 - p.max(0.0));
        self.shared.right.set(1.0 + p.min(0.0));
    }

    pub fn set_eq_enabled(&mut self, on: bool) {
        self.eq_on = on;
        self.shared.eq_on.store(on, Ordering::Relaxed);
    }

    pub fn set_eq_band(&mut self, band: usize, value: i32) {
        if let Some(slot) = self.eq_bands.get_mut(band) {
            *slot = value.clamp(0, 255);
            self.shared.eq_bands[band].store(*slot, Ordering::Relaxed);
            self.shared.eq_version.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_eq_preamp(&mut self, value: i32) {
        self.eq_preamp = value.clamp(0, 255);
        self.shared.eq_preamp.store(self.eq_preamp, Ordering::Relaxed);
        self.shared.eq_version.fetch_add(1, Ordering::Relaxed);
    }

    /// The spectrum of what is heard: 75 bands, 0..1; empty when nothing plays.
    pub fn spectrum(&self) -> Vec<f32> {
        if self.status != Status::Playing {
            return Vec::new();
        }
        let rate = self.playing.as_ref().map_or(44100, |p| p.stream.sample_rate);
        spectrum(&self.shared.tap.last(FFT_SIZE), rate, &*self.fft)
    }

    /// The wave being heard: 576 samples, -1..1; empty when nothing plays.
    pub fn waveform(&self) -> Vec<f32> {
        if self.status != Status::Playing {
            return Vec::new();
        }
        self.shared.tap.last(WAVE_SAMPLES)
    }

    /// The level of each side, 0..255.
    pub fn vu(&self) -> (i32, i32) {
        if self.status != Status::Playing {
            return (0, 0);
        }
        let level = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as i32;
        (
            level(self.shared.level_left.get()),
            level(self.shared.level_right.get()),
        )
    }

    /// Blocks in which the output ran dry while a song went on.
    pub fn underruns(&self) -> u64 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// What the player asked the app for since the last call, as `REQUEST_` bits.
    pub fn take_requests(&mut self) -> u32 {
        std::mem::take(&mut self.requests)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The spectrum of `samples` at `sample_rate`: [`SPECTRUM_BANDS`] bands spaced evenly in
/// pitch across [`SPECTRUM_RANGE`], each the loudest frequency in it, 0 at
/// [`SPECTRUM_FLOOR_DB`] below full scale and 1 at full scale.
pub fn spectrum(samples: &[f32], sample_rate: u32, fft: &dyn Fft<f32>) -> Vec<f32> {
    let n = fft.len();
    if samples.len() < n {
        return vec![0.0; SPECTRUM_BANDS];
    }
    let mut bins: Vec<Complex32> = samples[samples.len() - n..]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            // a Hann window keeps a steady tone in its own band
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos();
            Complex32::new(s * w, 0.0)
        })
        .collect();
    fft.process(&mut bins);
    // a full-scale sine comes out of the windowed transform at a quarter of its length
    let full = n as f32 / 4.0;
    let hz_per_bin = sample_rate as f32 / n as f32;
    let (low, high) = SPECTRUM_RANGE;
    let edge = |band: usize| low * (high / low).powf(band as f32 / SPECTRUM_BANDS as f32);
    (0..SPECTRUM_BANDS)
        .map(|band| {
            let first = ((edge(band) / hz_per_bin).floor() as usize).clamp(1, n / 2 - 1);
            let last = ((edge(band + 1) / hz_per_bin).ceil() as usize).clamp(first + 1, n / 2);
            let peak = bins[first..last]
                .iter()
                .map(|c| c.norm())
                .fold(0.0f32, f32::max);
            let db = 20.0 * (peak / full).max(1e-9).log10();
            ((db + SPECTRUM_FLOOR_DB) / SPECTRUM_FLOOR_DB).clamp(0.0, 1.0)
        })
        .collect()
}

/// The player as a skin sees it: walkit's `Player` over the app's one [`Player`].
pub struct SkinPlayer(pub Rc<RefCell<Player>>);

impl walkit::player::Player for SkinPlayer {
    fn status(&self) -> Status {
        self.0.borrow().status()
    }
    fn position_ms(&self) -> i32 {
        self.0.borrow().position_ms()
    }
    fn length_ms(&self) -> i32 {
        self.0.borrow().length_ms()
    }
    fn title(&self) -> String {
        self.0.borrow().entry().map(|e| e.title.clone()).unwrap_or_default()
    }
    fn bitrate_kbps(&self) -> i32 {
        self.0.borrow().entry().map_or(0, |e| e.info.bitrate_kbps)
    }
    fn sample_rate_khz(&self) -> i32 {
        self.0.borrow().entry().map_or(0, |e| (e.info.sample_rate / 1000) as i32)
    }
    fn channels(&self) -> i32 {
        self.0.borrow().entry().map_or(0, |e| e.info.channels as i32)
    }
    fn tags(&self) -> BTreeMap<String, String> {
        self.0.borrow().entry().map(|e| e.info.tags.clone()).unwrap_or_default()
    }
    fn decoder_name(&self) -> String {
        self.0.borrow().entry().map(|e| e.info.codec.clone()).unwrap_or_default()
    }
    fn volume(&self) -> i32 {
        self.0.borrow().volume()
    }
    fn set_volume(&mut self, v: i32) {
        self.0.borrow_mut().set_volume(v);
    }
    fn shuffle(&self) -> bool {
        self.0.borrow().shuffle
    }
    fn set_shuffle(&mut self, on: bool) {
        let mut p = self.0.borrow_mut();
        p.shuffle = on;
        p.shuffled.clear();
    }
    fn repeat(&self) -> bool {
        self.0.borrow().repeat
    }
    fn set_repeat(&mut self, on: bool) {
        self.0.borrow_mut().repeat = on;
    }
    fn crossfade(&self) -> bool {
        self.0.borrow().crossfade
    }
    fn set_crossfade(&mut self, on: bool) {
        self.0.borrow_mut().crossfade = on;
    }
    fn eq_enabled(&self) -> bool {
        self.0.borrow().eq_on
    }
    fn set_eq_enabled(&mut self, on: bool) {
        self.0.borrow_mut().set_eq_enabled(on);
    }
    fn eq_bands(&self) -> Vec<i32> {
        self.0.borrow().eq_bands.to_vec()
    }
    fn eq_preamp(&self) -> i32 {
        self.0.borrow().eq_preamp
    }
    fn set_eq_preamp(&mut self, v: i32) {
        self.0.borrow_mut().set_eq_preamp(v);
    }
    fn pan(&self) -> i32 {
        self.0.borrow().pan()
    }
    fn set_pan(&mut self, v: i32) {
        self.0.borrow_mut().set_pan(v);
    }
    fn play(&mut self) {
        self.0.borrow_mut().play();
    }
    fn pause(&mut self) {
        self.0.borrow_mut().pause();
    }
    fn stop(&mut self) {
        self.0.borrow_mut().stop();
    }
    fn next(&mut self) {
        self.0.borrow_mut().next();
    }
    fn previous(&mut self) {
        self.0.borrow_mut().previous();
    }
    fn eject(&mut self) {
        self.0.borrow_mut().requests |= REQUEST_OPEN_FILES;
    }
    fn seek_to(&mut self, ms: i32) {
        self.0.borrow_mut().seek_to(ms);
    }
    fn set_eq_band(&mut self, band: i32, value: i32) {
        if let Ok(band) = usize::try_from(band) {
            self.0.borrow_mut().set_eq_band(band, value);
        }
    }
    fn tracks(&self) -> Vec<Track> {
        self.0
            .borrow()
            .entries()
            .iter()
            .map(|e| Track {
                title: e.title.clone(),
                length_ms: e.info.length_ms,
            })
            .collect()
    }
    fn track_index(&self) -> i32 {
        self.0.borrow().index() as i32
    }
    fn play_track(&mut self, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.0.borrow_mut().play_index(index);
        }
    }
    fn spectrum(&self) -> Vec<f32> {
        self.0.borrow().spectrum()
    }
    fn waveform(&self) -> Vec<f32> {
        self.0.borrow().waveform()
    }
    fn vu(&self) -> (i32, i32) {
        self.0.borrow().vu()
    }
}
