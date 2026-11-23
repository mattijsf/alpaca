// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading audio files: what a file is ([`probe`]) and its samples ([`spawn`]), decoded on a
//! thread of their own into a ring the output empties.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::{Hint, ProbeResult};
use symphonia::core::units::Time;

/// What a file is, read once when it joins the playlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInfo {
    /// Its length in milliseconds; 0 when the file does not say.
    pub length_ms: i32,
    /// Samples a second.
    pub sample_rate: u32,
    /// How many channels the file has.
    pub channels: u32,
    /// Its average bitrate in kbps, from its size and length.
    pub bitrate_kbps: i32,
    /// Its tags by lowercase name: artist, title, album and what else it carries.
    pub tags: BTreeMap<String, String>,
    /// The decoder's short name, such as "mp3" or "flac".
    pub codec: String,
}

/// One stretch of decoded audio on its way to the output: a file from some point on.
pub struct Stream {
    /// Frames the output has played from it.
    pub frames_played: AtomicU64,
    /// Set by the decoder when the file has no more.
    pub decoded_all: AtomicBool,
    /// Set by the output when the ring ran empty after that: the song is over.
    pub ended: AtomicBool,
    /// Set to make the decoder stop.
    pub cancel: AtomicBool,
    /// Samples a second.
    pub sample_rate: u32,
}

/// The ring's size in samples: two seconds of stereo at 48 kHz.
const RING_SAMPLES: usize = 2 * 2 * 48_000;

/// How much room the ring has before the decoder fills it again, in samples: a third of it.
const FILL_AT: usize = RING_SAMPLES / 3;

/// An open file: its container, the track that plays and that track's parameters.
struct Opened {
    format: Box<dyn FormatReader>,
    track_id: u32,
    params: symphonia::core::codecs::CodecParameters,
    tags: BTreeMap<String, String>,
}

fn open(path: &Path) -> Result<Opened, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let source = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let ProbeResult {
        mut format,
        mut metadata,
    } = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| e.to_string())?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio in the file")?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    // tags before the container (ID3v2) and tags inside it (Vorbis comments, MP4 atoms)
    let mut tags = BTreeMap::new();
    if let Some(found) = metadata.get() {
        if let Some(revision) = found.current() {
            collect(revision, &mut tags);
        }
    }
    if let Some(revision) = format.metadata().current() {
        collect(revision, &mut tags);
    }
    Ok(Opened {
        format,
        track_id,
        params,
        tags,
    })
}

/// Adds a revision's tags to `tags` by the names a skin asks for.
fn collect(revision: &MetadataRevision, tags: &mut BTreeMap<String, String>) {
    for tag in revision.tags() {
        let name = match tag.std_key {
            Some(StandardTagKey::TrackTitle) => "title".to_string(),
            Some(StandardTagKey::Artist) => "artist".to_string(),
            Some(StandardTagKey::Album) => "album".to_string(),
            Some(StandardTagKey::AlbumArtist) => "albumartist".to_string(),
            Some(StandardTagKey::Genre) => "genre".to_string(),
            Some(StandardTagKey::Date) => "year".to_string(),
            Some(StandardTagKey::TrackNumber) => "track".to_string(),
            Some(StandardTagKey::Comment) => "comment".to_string(),
            Some(StandardTagKey::Composer) => "composer".to_string(),
            _ => tag.key.to_lowercase(),
        };
        let value = tag.value.to_string();
        if !name.is_empty() && !value.is_empty() && value.len() < 4096 {
            tags.entry(name).or_insert(value);
        }
    }
}

/// What the file at `path` is: length, format and tags.
pub fn probe(path: &Path) -> Result<FileInfo, String> {
    let opened = open(path)?;
    let params = &opened.params;
    let sample_rate = params.sample_rate.ok_or("the file gives no sample rate")?;
    let length_ms = params
        .n_frames
        .map(|frames| (frames * 1000 / u64::from(sample_rate.max(1))) as i32)
        .unwrap_or(0);
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let bitrate_kbps = if length_ms > 0 {
        (size * 8 / length_ms as u64) as i32
    } else {
        0
    };
    let codec = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .map(|d| d.short_name.to_string())
        .unwrap_or_default();
    Ok(FileInfo {
        length_ms,
        sample_rate,
        channels: params.channels.map_or(2, |c| c.count() as u32),
        bitrate_kbps,
        tags: opened.tags,
        codec,
    })
}

/// Starts decoding the file at `path` from `start_ms` on a thread of its own. Gives the stream
/// and the end of the ring the output reads: stereo, 32-bit float, left and right in turn.
pub fn spawn(path: PathBuf, start_ms: i32, sample_rate: u32) -> (Arc<Stream>, rtrb::Consumer<f32>) {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_SAMPLES);
    let stream = Arc::new(Stream {
        frames_played: AtomicU64::new(0),
        decoded_all: AtomicBool::new(false),
        ended: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        sample_rate,
    });
    let state = stream.clone();
    let spawned = std::thread::Builder::new()
        .name("andamp-decode".into())
        .spawn(move || {
            if let Err(message) = decode(&path, start_ms, producer, &state) {
                eprintln!("andamp: {}: {message}", path.display());
            }
            state.decoded_all.store(true, Ordering::Release);
        });
    if spawned.is_err() {
        stream.decoded_all.store(true, Ordering::Release);
    }
    (stream, consumer)
}

/// Decodes the file into `ring` until it ends or the stream is cancelled.
fn decode(
    path: &Path,
    start_ms: i32,
    mut ring: rtrb::Producer<f32>,
    stream: &Stream,
) -> Result<(), String> {
    let Opened {
        mut format,
        track_id,
        params,
        ..
    } = open(path)?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    if start_ms > 0 {
        let time = Time::new(
            (start_ms / 1000) as u64,
            f64::from(start_ms % 1000) / 1000.0,
        );
        // a file that cannot seek plays from where it is
        let _ = format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(track_id),
            },
        );
        decoder.reset();
    }
    let mut samples: Option<SampleBuffer<f32>> = None;
    let mut stereo: Vec<f32> = Vec::new();
    loop {
        if stream.cancel.load(Ordering::Acquire) || ring.is_abandoned() {
            return Ok(());
        }
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // the end of the file
            Err(Error::IoError(_) | Error::ResetRequired) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // a damaged packet is skipped
            Err(Error::DecodeError(_)) => continue,
            Err(Error::IoError(_)) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let buffer = samples
            .get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if buffer.capacity() < decoded.capacity() * channels {
            *buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(decoded);
        // the ring is stereo: one channel goes to both sides, more than two keep the first two
        let interleaved = buffer.samples();
        let out: &[f32] = if channels == 2 {
            interleaved
        } else {
            stereo.clear();
            for frame in interleaved.chunks_exact(channels) {
                stereo.push(frame[0]);
                stereo.push(if channels == 1 { frame[0] } else { frame[1] });
            }
            &stereo
        };
        let mut rest = out;
        while !rest.is_empty() {
            if stream.cancel.load(Ordering::Acquire) {
                return Ok(());
            }
            // whole frames only, so left stays left
            let room = ring.slots() & !1;
            // the ring holds two seconds: once it is full the decoder sleeps until a good part
            // of it has been played, and so wakes a few times a second, not a hundred
            if room < FILL_AT.min(rest.len()) {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let take = room.min(rest.len());
            let (now, later) = rest.split_at(take);
            let (_, left) = ring.push_partial_slice(now);
            debug_assert!(left.is_empty(), "the ring had room for all of it");
            rest = later;
        }
    }
}
