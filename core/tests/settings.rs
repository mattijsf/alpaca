// SPDX-License-Identifier: GPL-3.0-or-later

//! What is the same on every system: the settings file, and what a file the listener opens is.

use andamp_core::player::{location_kind, LocationKind, Player};
use andamp_core::settings::Settings;
use std::path::Path;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("andamp-settings-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn settings_come_back_as_they_were_left() {
    let dir = scratch("back");
    let mut settings = Settings::open(&dir);
    assert_eq!(settings.get("zoom"), None);
    settings.set("zoom", Some("2"));
    settings.set("skin", Some("/Skins/A tab\there and a\nline.wsz"));
    settings.set("places.Dark.1", Some("120,859"));
    settings.set("places.Dark.3", Some("120,627"));
    settings.set("gone", Some("x"));
    settings.set("gone", None);
    settings.save().unwrap();

    let mut again = Settings::open(&dir);
    assert_eq!(again.get("zoom"), Some("2"));
    assert_eq!(again.get("skin"), Some("/Skins/A tab\there and a\nline.wsz"));
    assert_eq!(again.get("gone"), None);
    assert_eq!(again.keys("places.Dark."), ["places.Dark.1", "places.Dark.3"]);
    again.clear("places.");
    assert!(again.keys("places.").is_empty());
    again.save().unwrap();
    assert_eq!(Settings::open(&dir).keys(""), ["skin", "zoom"]);

    // settings that are never written take values and leave no file
    let mut none = Settings::none();
    none.set("zoom", Some("3"));
    assert_eq!(none.get("zoom"), Some("3"));
    none.save().unwrap();
}

#[test]
fn a_location_is_known_by_its_name_and_a_folder_gives_its_songs() {
    assert_eq!(location_kind(Path::new("/a/Skin.WSZ")), LocationKind::Skin);
    assert_eq!(location_kind(Path::new("b.wal")), LocationKind::Skin);
    assert_eq!(location_kind(Path::new("list.m3u8")), LocationKind::List);
    assert_eq!(location_kind(Path::new("song.flac")), LocationKind::Audio);
    assert_eq!(location_kind(Path::new("https://example.org/stream.pls")), LocationKind::Audio);

    // a folder with songs, a folder under it, a picture and a hidden file
    let dir = scratch("folder");
    let wav = |path: &Path| {
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend(40u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend([1, 0, 2, 0]);
        bytes.extend(44_100u32.to_le_bytes());
        bytes.extend(176_400u32.to_le_bytes());
        bytes.extend([4, 0, 16, 0]);
        bytes.extend(b"data");
        bytes.extend(4u32.to_le_bytes());
        bytes.extend([0, 0, 0, 0]);
        std::fs::write(path, bytes).unwrap();
    };
    std::fs::create_dir_all(dir.join("Disc 2")).unwrap();
    wav(&dir.join("b.wav"));
    wav(&dir.join("A.wav"));
    wav(&dir.join("Disc 2/c.wav"));
    wav(&dir.join(".hidden.wav"));
    std::fs::write(dir.join("cover.jpg"), b"not audio").unwrap();
    let mut player = Player::silent();
    assert_eq!(player.add_location(&dir), 3);
    let titles: Vec<&str> = player.entries().iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, ["A", "b", "c"], "by name, the folder under it after its own songs");
    // Winamp's keys
    assert!(player.key("x") && player.key("up") && !player.key("q"));
    assert_eq!(player.status(), walkit::player::Status::Playing);
}
