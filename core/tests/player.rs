// SPDX-License-Identifier: GPL-3.0-or-later

//! The player on files made here: what it reads of a file, that a song plays to its end and the
//! next one starts, and what the equalizer and the spectrum make of a tone. No test opens the
//! audio output, so none makes a sound.

use std::f32::consts::PI;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use andamp_core::player::output::Equalizer;
use andamp_core::player::{spectrum, Player};
use rustfft::FftPlanner;
use walkit::player::Status;

/// Writes `seconds` of a `hz` tone as a 16-bit stereo WAV file and gives its path.
fn tone_file(name: &str, hz: f32, seconds: f32, sample_rate: u32) -> PathBuf {
    let frames = (seconds * sample_rate as f32) as u32;
    let mut bytes = Vec::new();
    let data_len = frames * 4;
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_len).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes()); // PCM
    bytes.extend(2u16.to_le_bytes()); // stereo
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * 4).to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_len.to_le_bytes());
    for i in 0..frames {
        let v = (2.0 * PI * hz * i as f32 / sample_rate as f32).sin() * 0.5;
        let s = (v * 32767.0) as i16;
        bytes.extend(s.to_le_bytes());
        bytes.extend(s.to_le_bytes());
    }
    let dir = std::env::temp_dir().join(format!("andamp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Polls `player` until `done` says so, for at most five seconds.
fn wait(player: &mut Player, done: impl Fn(&Player) -> bool) {
    let began = Instant::now();
    while !done(player) {
        assert!(began.elapsed() < Duration::from_secs(5), "the player did not get there");
        std::thread::sleep(Duration::from_millis(2));
        player.poll();
    }
}

#[test]
fn a_file_is_read_for_what_it_is() {
    let path = tone_file("probe.wav", 440.0, 1.5, 44100);
    let mut player = Player::silent();
    assert_eq!(player.add(&path), Ok(0));
    let entry = &player.entries()[0];
    assert_eq!(entry.title, "probe");
    assert_eq!(entry.info.length_ms, 1500);
    assert_eq!(entry.info.sample_rate, 44100);
    assert_eq!(entry.info.channels, 2);
    assert_eq!(entry.info.bitrate_kbps, 1411);
    assert!(player.add(std::path::Path::new("/nonexistent/song.mp3")).is_err());
}

#[test]
fn a_song_plays_to_its_end_and_the_next_one_starts() {
    let first = tone_file("first.wav", 440.0, 0.5, 44100);
    let second = tone_file("second.wav", 880.0, 0.25, 48000);
    let mut player = Player::silent();
    player.add(&first).unwrap();
    player.add(&second).unwrap();
    assert_eq!(player.status(), Status::Stopped);
    player.play();
    assert_eq!(player.status(), Status::Playing);
    assert_eq!(player.index(), 0);
    wait(&mut player, |p| p.index() == 1);
    assert_eq!(player.status(), Status::Playing, "the second song follows the first");
    // the list ends: the player stops at its top
    wait(&mut player, |p| p.status() == Status::Stopped);
    assert_eq!(player.index(), 0);
    assert_eq!(player.position_ms(), 0);
}

#[test]
fn the_position_follows_what_was_played_and_a_seek_starts_from_there() {
    let path = tone_file("seek.wav", 440.0, 2.0, 44100);
    let mut player = Player::silent();
    player.add(&path).unwrap();
    player.play();
    player.pause();
    assert_eq!(player.status(), Status::Paused);
    player.seek_to(1500);
    assert_eq!(player.status(), Status::Paused, "a seek while paused stays paused");
    assert_eq!(player.position_ms(), 1500);
    player.pause();
    wait(&mut player, |p| p.status() == Status::Stopped);
    // stopped: a seek is kept for the next play
    player.seek_to(1900);
    assert_eq!(player.position_ms(), 1900);
    player.play();
    wait(&mut player, |p| p.status() == Status::Stopped);
}

#[test]
fn a_tone_shows_in_its_own_band_of_the_spectrum() {
    let fft = FftPlanner::new().plan_fft_forward(1024);
    let tone = |hz: f32| -> Vec<f32> {
        (0..1024).map(|i| (2.0 * PI * hz * i as f32 / 44100.0).sin()).collect()
    };
    let low = spectrum(&tone(200.0), 44100, &*fft);
    let high = spectrum(&tone(5000.0), 44100, &*fft);
    assert_eq!(low.len(), 75);
    let peak = |bands: &[f32]| {
        bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap()
    };
    assert!(peak(&low) < peak(&high), "a higher tone peaks further right");
    assert!(low[peak(&low)] > 0.9, "a full-scale tone fills its band: {}", low[peak(&low)]);
    assert!(high[peak(&high)] > 0.9);
    assert!(low[70] < 0.2, "a low tone leaves the top bands empty: {}", low[70]);
    let silence = spectrum(&vec![0.0; 1024], 44100, &*fft);
    assert!(silence.iter().all(|b| *b == 0.0));
}

#[test]
fn the_equalizer_lifts_its_band_and_leaves_the_others() {
    /// How loud `hz` comes out of `eq`, against going in.
    fn gain_db(eq: &mut Equalizer, hz: f32) -> f32 {
        let mut samples: Vec<f32> = (0..44100)
            .flat_map(|i| {
                let v = (2.0 * PI * hz * i as f32 / 44100.0).sin() * 0.1;
                [v, v]
            })
            .collect();
        eq.run(&mut samples);
        // the second half, after the filter settled
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        20.0 * (rms(&samples[44100..]) / (0.1 / 2f32.sqrt())).log10()
    }
    let mut bands = [128; 10];
    bands[4] = 255; // 1 kHz all the way up
    let mut eq = Equalizer::with(44100, bands, 128);
    let at_band = gain_db(&mut eq, 1000.0);
    assert!((at_band - 12.0).abs() < 0.5, "12 dB at the band: {at_band}");
    let far = gain_db(&mut Equalizer::with(44100, bands, 128), 60.0);
    assert!(far.abs() < 0.5, "nothing far from it: {far}");
    let flat = gain_db(&mut Equalizer::with(44100, [128; 10], 128), 1000.0);
    assert!(flat.abs() < 0.01, "flat is flat: {flat}");
    let preamp = gain_db(&mut Equalizer::with(44100, [128; 10], 0), 1000.0);
    assert!((preamp + 12.0).abs() < 0.2, "the preamp all the way down takes 12 dB: {preamp}");
}

#[test]
fn rows_are_removed_cropped_and_sorted_around_the_song_playing() {
    let mut player = Player::silent();
    for name in ["d.wav", "b.wav", "a.wav", "c.wav"] {
        player.add(&tone_file(name, 440.0, 5.0, 44100)).unwrap();
    }
    let titles = |p: &Player| p.entries().iter().map(|e| e.title.clone()).collect::<Vec<_>>();
    player.play_index(2);
    assert_eq!(titles(&player)[player.index()], "a");
    // a row before the song goes: the song stays current and keeps playing
    player.remove(&[0]);
    assert_eq!(titles(&player), ["b", "a", "c"]);
    assert_eq!((player.index(), player.status()), (1, Status::Playing));
    player.sort_by_title();
    assert_eq!(titles(&player), ["a", "b", "c"]);
    assert_eq!((player.index(), player.status()), (0, Status::Playing));
    // the song itself goes: the player stops
    player.crop(&[1, 2]);
    assert_eq!(titles(&player), ["b", "c"]);
    assert_eq!(player.status(), Status::Stopped);
    assert!(player.index() < 2);
}

#[test]
fn a_list_is_saved_and_read_back() {
    let mut player = Player::silent();
    let first = tone_file("one.wav", 440.0, 1.0, 44100);
    let second = tone_file("two.wav", 440.0, 2.0, 44100);
    player.add(&first).unwrap();
    player.add(&second).unwrap();
    let list = first.with_file_name("list.m3u8");
    player.save_list(&list).unwrap();
    let text = std::fs::read_to_string(&list).unwrap();
    assert!(text.starts_with("#EXTM3U\n#EXTINF:1,one\n"), "{text}");
    let mut other = Player::silent();
    assert_eq!(other.add_list(&list), Ok(2));
    assert_eq!(other.entries()[1].title, "two");
    // a .pls, with names beside the list
    let pls = first.with_file_name("list.pls");
    std::fs::write(&pls, "[playlist]\nFile1=one.wav\nTitle1=One\nFile2=missing.wav\nNumberOfEntries=2\n").unwrap();
    let mut third = Player::silent();
    assert_eq!(third.add_list(&pls), Ok(1));
}

#[test]
fn a_factory_preset_sets_the_bands_and_switches_the_equalizer_on() {
    use walkit::player::Player as _;
    let player = std::rc::Rc::new(std::cell::RefCell::new(Player::silent()));
    let skin = andamp_core::player::SkinPlayer(player.clone());
    assert!(!skin.eq_enabled());
    // Full Bass: the low bands up, the high ones down
    let full_bass = andamp_core::player::EQ_PRESETS.iter().position(|p| p.0 == "Full Bass").unwrap();
    player.borrow_mut().apply_eq_preset(Some(full_bass));
    let bands = skin.eq_bands();
    assert!(skin.eq_enabled());
    assert!(bands[0] > 170 && bands[9] < 70, "{bands:?}");
    assert!((skin.eq_preamp() - 128).abs() <= 2, "the preamp in the middle: {}", skin.eq_preamp());
    player.borrow_mut().apply_eq_preset(None);
    assert_eq!(skin.eq_bands(), vec![128; 10]);
}

#[test]
fn what_a_run_leaves_is_put_back_in_the_next() {
    use walkit::player::Player as _;
    let first = std::rc::Rc::new(std::cell::RefCell::new(Player::silent()));
    let mut skin = andamp_core::player::SkinPlayer(first.clone());
    skin.set_eq_enabled(true);
    skin.set_eq_band(3, 200);
    skin.set_eq_preamp(90);
    skin.set_pan(-40);
    skin.set_shuffle(true);
    let left = first.borrow().state();
    let second = std::rc::Rc::new(std::cell::RefCell::new(Player::silent()));
    second.borrow_mut().restore(&left);
    let skin = andamp_core::player::SkinPlayer(second.clone());
    assert!(skin.eq_enabled() && skin.shuffle() && !skin.repeat());
    assert_eq!((skin.eq_bands()[3], skin.eq_preamp(), skin.pan()), (200, 90, -40));
    // text from somewhere else changes nothing it does not name, and breaks nothing
    second.borrow_mut().restore("nonsense;eq=x;bands=1,2,three;pan=900");
    assert_eq!(skin.pan(), 127);
}

/// `audio` as a radio stream sends it: after every `every` bytes a block with the song's title.
fn with_titles(audio: &[u8], every: usize, title: &str) -> Vec<u8> {
    let mut text = format!("StreamTitle='{title}';").into_bytes();
    text.resize(text.len().div_ceil(16) * 16, 0);
    let mut out = Vec::new();
    for chunk in audio.chunks(every) {
        out.extend(chunk);
        if chunk.len() == every {
            out.push((text.len() / 16) as u8);
            out.extend(&text);
        }
    }
    out
}

#[test]
fn a_radio_streams_titles_are_taken_out_of_its_audio() {
    use andamp_core::player::decode::IcyReader;
    use std::io::Read;
    let audio: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let sent = with_titles(&audio, 64, "Artist - Song");
    let mut reader = IcyReader::new(&sent[..], 64, None);
    let mut heard = Vec::new();
    reader.read_to_end(&mut heard).unwrap();
    assert_eq!(heard, audio, "what is left is the audio, byte for byte");
    assert_eq!(reader.title(), "Artist - Song");
}

#[test]
fn a_radio_stream_plays_and_says_what_it_is() {
    use std::io::{Read, Write};
    let wav = std::fs::read(tone_file("radio.wav", 440.0, 1.0, 48000)).unwrap();
    let body = with_titles(&wav, 8192, "Artist - Song");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0u8; 2048];
        let _ = socket.read(&mut request);
        let asked = String::from_utf8_lossy(&request).to_lowercase();
        let head = "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nicy-name: Test FM\r\nicy-br: 128\r\nicy-metaint: 8192\r\n\r\n";
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(&body);
        asked.contains("icy-metadata: 1")
    });
    let mut player = Player::silent();
    let url = format!("http://{address}/stream");
    assert_eq!(player.add(std::path::Path::new(&url)), Ok(0));
    assert_eq!(player.entries()[0].title, url, "before it plays a stream is known by its address");
    player.play();
    assert_eq!(player.status(), Status::Playing);
    wait(&mut player, |p| p.entries()[0].title.contains("Artist - Song"));
    let entry = &player.entries()[0];
    assert_eq!(entry.title, "Artist - Song (Test FM)");
    assert_eq!((entry.info.sample_rate, entry.info.channels, entry.info.bitrate_kbps), (48000, 2, 128));
    // a stream has no other place to seek to: asking changes nothing
    player.seek_to(500);
    assert_eq!(player.entries()[0].title, "Artist - Song (Test FM)");
    // the server hangs up: the stream is over, and the list with it
    wait(&mut player, |p| p.status() == Status::Stopped);
    assert!(server.join().unwrap(), "the stream was asked for with its titles");
    // an address nothing answers at stops the player and does not hang it
    let mut player = Player::silent();
    player.add(std::path::Path::new("http://127.0.0.1:1/none")).unwrap();
    player.play();
    wait(&mut player, |p| p.status() == Status::Stopped);
}

#[test]
fn a_song_at_one_rate_keeps_its_pitch_and_its_time_on_a_device_at_another() {
    use andamp_core::player::device::resample::Resampler;
    // a 1 kHz tone at 44.1 kHz, asked for by a device at 48 kHz in blocks of 512 frames
    let mut resampler = Resampler::new(44_100, 48_000);
    let mut source_frame = 0u64;
    let mut fill = |block: &mut [f32]| {
        for frame in block.chunks_exact_mut(2) {
            let v = (2.0 * PI * 1000.0 * source_frame as f32 / 44_100.0).sin();
            frame[0] = v;
            frame[1] = -v;
            source_frame += 1;
        }
    };
    let mut out = Vec::new();
    for _ in 0..94 {
        let mut block = vec![0f32; 1024];
        resampler.run(&mut block, &mut fill);
        out.extend(block);
    }
    let frames = out.len() / 2;
    // a second of the device's time took a second of the song's, give or take a block
    let taken = source_frame as f64 / 44_100.0;
    let played = frames as f64 / 48_000.0;
    assert!((taken - played).abs() < 0.02, "took {taken} s of song for {played} s of output");
    // the tone is still at 1 kHz: it crosses zero going up a thousand times a second
    let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
    let crossings = left.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    let hz = crossings as f64 / played;
    assert!((hz - 1000.0).abs() < 3.0, "{hz} Hz");
    // and the sides stay their own
    assert!(out.chunks_exact(2).skip(8).all(|f| (f[0] + f[1]).abs() < 1e-5));
    let loudest = left.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!((0.95..=1.0).contains(&loudest), "{loudest}");
}
