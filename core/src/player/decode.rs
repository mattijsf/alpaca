// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading audio: what a file is ([`probe`]) and the samples of a file or an internet radio
//! stream ([`spawn`]), decoded on a thread of their own into a ring the output empties.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
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
    /// Samples a second; 0 for a radio stream until its first bytes say.
    pub sample_rate: AtomicU32,
    /// What a radio stream told since it was last asked: what it is, and the song it plays.
    pub news: Mutex<News>,
}

/// What a radio stream says of itself as it plays.
#[derive(Debug, Default)]
pub struct News {
    /// The stream's format, once its first bytes are read.
    pub info: Option<FileInfo>,
    /// The station's name, from the answer's `icy-name`.
    pub station: Option<String>,
    /// The song playing, from the stream's own titles.
    pub title: Option<String>,
}

/// Whether `location` is a stream on the internet and not a file.
pub fn is_url(location: &Path) -> bool {
    let text = location.to_string_lossy();
    text.starts_with("http://") || text.starts_with("https://")
}

/// A radio stream's bytes with its titles taken out. A server asked for them puts a block of
/// text after every `every` bytes of audio: one byte with its length in sixteens, then
/// `StreamTitle='...';`.
pub struct IcyReader<R: Read> {
    inner: R,
    every: usize,
    /// Bytes of audio until the next block.
    left: usize,
    news: Option<Arc<Stream>>,
    /// The last title, kept so that only a new one is told.
    last: String,
}

impl<R: Read> IcyReader<R> {
    pub fn new(inner: R, every: usize, news: Option<Arc<Stream>>) -> Self {
        IcyReader {
            inner,
            every,
            left: every,
            news,
            last: String::new(),
        }
    }

    /// The last title read.
    pub fn title(&self) -> &str {
        &self.last
    }

    fn read_block(&mut self) -> std::io::Result<()> {
        let mut length = [0u8; 1];
        self.inner.read_exact(&mut length)?;
        let mut block = vec![0u8; usize::from(length[0]) * 16];
        self.inner.read_exact(&mut block)?;
        let text = String::from_utf8_lossy(&block);
        if let Some(start) = text.find("StreamTitle='") {
            let rest = &text[start + 13..];
            let title = rest.split("';").next().unwrap_or("").trim_end_matches(['\0', '\'']).trim();
            if !title.is_empty() && title != self.last {
                self.last = title.to_string();
                if let Some(stream) = &self.news {
                    if let Ok(mut news) = stream.news.lock() {
                        news.title = Some(self.last.clone());
                    }
                }
            }
        }
        Ok(())
    }
}

impl<R: Read> Read for IcyReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.every == 0 {
            return self.inner.read(buf);
        }
        if self.left == 0 {
            self.read_block()?;
            self.left = self.every;
        }
        let want = buf.len().min(self.left);
        let got = self.inner.read(&mut buf[..want])?;
        self.left -= got;
        Ok(got)
    }
}

/// A reader other threads may hold but only one reads at a time.
struct Shared<R>(Mutex<R>);

impl<R: Read> Read for Shared<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.get_mut() {
            Ok(inner) => inner.read(buf),
            Err(_) => Ok(0),
        }
    }
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
    probed(source, &hint)
}

/// Opens the radio stream at `url`: asks for it with its titles, and reads on from the answer.
fn open_url(url: &str, stream: &Arc<Stream>) -> Result<Opened, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .build()
        .into();
    let answer = agent
        .get(url)
        .header("Icy-MetaData", "1")
        .header("User-Agent", "Andamp")
        .call()
        .map_err(|e| e.to_string())?;
    let header = |name: &str| {
        answer
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let every = header("icy-metaint").and_then(|v| v.parse().ok()).unwrap_or(0);
    let kbps = header("icy-br").and_then(|v| v.split(',').next()?.trim().parse::<i32>().ok());
    if let Ok(mut news) = stream.news.lock() {
        news.station = header("icy-name").filter(|n| !n.trim().is_empty());
    }
    let mut hint = Hint::new();
    match header("content-type").as_deref() {
        Some("audio/mpeg" | "audio/mp3") => hint.with_extension("mp3"),
        Some("audio/aac" | "audio/aacp") => hint.with_extension("aac"),
        Some("audio/ogg" | "application/ogg") => hint.with_extension("ogg"),
        Some("audio/flac") => hint.with_extension("flac"),
        Some("audio/wav" | "audio/x-wav") => hint.with_extension("wav"),
        _ => &mut hint,
    };
    let body = Shared(Mutex::new(answer.into_body().into_reader()));
    let reader = IcyReader::new(body, every, Some(stream.clone()));
    let source = MediaSourceStream::new(Box::new(ReadOnlySource::new(reader)), Default::default());
    let mut opened = probed(source, &hint)?;
    if let Some(kbps) = kbps {
        opened.tags.insert("icy-br".into(), kbps.to_string());
    }
    Ok(opened)
}

/// Finds out what `source` is and picks the track that plays.
fn probed(source: MediaSourceStream, hint: &Hint) -> Result<Opened, String> {
    let ProbeResult {
        mut format,
        mut metadata,
    } = symphonia::default::get_probe()
        .format(
            hint,
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

/// Starts decoding the file or radio stream at `location` from `start_ms` on a thread of its
/// own. Gives the stream and the end of the ring the output reads: stereo, 32-bit float, left
/// and right in turn. `sample_rate` is the file's, or 0 for a radio stream, whose rate the
/// stream says once it is known.
pub fn spawn(location: PathBuf, start_ms: i32, sample_rate: u32) -> (Arc<Stream>, rtrb::Consumer<f32>) {
    let (producer, consumer) = rtrb::RingBuffer::new(RING_SAMPLES);
    let stream = Arc::new(Stream {
        frames_played: AtomicU64::new(0),
        decoded_all: AtomicBool::new(false),
        ended: AtomicBool::new(false),
        cancel: AtomicBool::new(false),
        sample_rate: AtomicU32::new(sample_rate),
        news: Mutex::new(News::default()),
    });
    let state = stream.clone();
    let spawned = std::thread::Builder::new()
        .name("andamp-decode".into())
        .spawn(move || {
            if let Err(message) = decode(&location, start_ms, producer, &state) {
                eprintln!("andamp: {}: {message}", location.display());
            }
            state.decoded_all.store(true, Ordering::Release);
        });
    if spawned.is_err() {
        stream.decoded_all.store(true, Ordering::Release);
    }
    (stream, consumer)
}

/// Decodes the file or stream into `ring` until it ends or the stream is cancelled.
fn decode(
    location: &Path,
    start_ms: i32,
    mut ring: rtrb::Producer<f32>,
    stream: &Arc<Stream>,
) -> Result<(), String> {
    let radio = is_url(location);
    let Opened {
        mut format,
        track_id,
        params,
        tags,
    } = if radio {
        open_url(&location.to_string_lossy(), stream)?
    } else {
        open(location)?
    };
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    if radio {
        // what the stream is, for the player to show and to open the output for
        let rate = params.sample_rate.unwrap_or(44_100);
        if let Ok(mut news) = stream.news.lock() {
            news.info = Some(FileInfo {
                length_ms: 0,
                sample_rate: rate,
                channels: params.channels.map_or(2, |c| c.count() as u32),
                bitrate_kbps: tags.get("icy-br").and_then(|v| v.parse().ok()).unwrap_or(0),
                tags: BTreeMap::new(),
                codec: symphonia::default::get_codecs()
                    .get_codec(params.codec)
                    .map(|d| d.short_name.to_string())
                    .unwrap_or_default(),
            });
        }
        stream.sample_rate.store(rate, Ordering::Release);
    }
    if start_ms > 0 && !radio {
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
