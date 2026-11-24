// SPDX-License-Identifier: GPL-3.0-or-later

//! The app behind every shell: what the command line asks for, what a start puts back, the
//! menu and its commands, what the skin's buttons ask of the shell, and the Visualizer's
//! presets. No test opens a window or the audio output.

use std::path::{Path, PathBuf};

use andamp_core::app::{command, App, Ask, Cursor, Menu, Options, Request};
use andamp_core::host::HostAction;
use andamp_core::player::Player;
use andamp_core::vis::{by_name, Key, Presets};

fn resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/Resources")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("andamp-app-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

/// The app as `line` on the command line makes it, with its own folder at `config`, started.
fn start(line: &str, config: &Path) -> App {
    let mut app = App::with(
        Options::parse(&args(line)),
        &resources(),
        (1728, 1000),
        Some(config.to_path_buf()),
        Player::silent(),
    );
    app.start();
    // the skin shows at the first step, the playlist comes back at the second
    app.step();
    app.step();
    app
}

/// A second of silence as a WAV file.
fn song(dir: &Path, name: &str) -> String {
    let frames = 44100u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + frames * 4).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(44100u32.to_le_bytes());
    bytes.extend((44100u32 * 4).to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((frames * 4).to_le_bytes());
    bytes.resize(bytes.len() + (frames * 4) as usize, 0);
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn main_corner(app: &mut App) -> (i32, i32) {
    let zoom = app.zoom();
    let skin = app.skin().expect("a skin is worn");
    let main = skin.runtime.main_window().expect("a main window");
    let rect = skin.runtime.window_rect(main);
    (rect.left * zoom, rect.top * zoom)
}

#[test]
fn the_command_line_is_read_the_same_everywhere() {
    let options = Options::parse(&args(
        "Some.wsz a.mp3 https://radio.example/stream --zoom 2 --volume 0 --effect tone --effect width \
         -NSDocumentRevisionsDebugMode YES --click 10,20 --click 30,40 --bench 5 --show-vis --key-window 5 b.flac",
    ));
    // files are kept from the root, so the next run finds them from another folder
    let here = |name: &str| std::env::current_dir().unwrap().join(name);
    assert_eq!(options.skin, Some(here("Some.wsz")));
    let songs = [
        here("a.mp3"),
        PathBuf::from("https://radio.example/stream"),
        here("b.flac"),
    ];
    assert_eq!(
        options.songs,
        songs.map(|song| song.to_string_lossy().into_owned())
    );
    assert_eq!((options.zoom, options.volume), (Some(2), Some(0)));
    assert_eq!(options.effects, ["tone", "width"]);
    assert_eq!(options.get("click").as_deref(), Some("10,20\n30,40"));
    assert_eq!(options.get("bench").as_deref(), Some("5"));
    assert_eq!(options.get("key-window").as_deref(), Some("5"));
    assert_eq!(options.get("show-vis").as_deref(), Some("1"));
    assert_eq!(options.get("hidden"), None);
    assert_eq!(options.get("dump"), None);
}

#[test]
fn a_first_start_wears_the_default_skin_and_tells_the_shell() {
    let config = scratch("first");
    let mut app = App::with(
        Options::default(),
        &resources(),
        (1728, 1000),
        Some(config),
        Player::silent(),
    );
    assert!(app.skin().is_none());
    app.start();
    assert_eq!(app.skin_name(), "AndAmp Dark.wsz");
    assert_eq!(app.take_requests(), [Request::Skin, Request::Menu]);
    app.step();
    assert_eq!(
        app.skin().unwrap().windows().len(),
        3,
        "main, equalizer and playlist"
    );
    assert_eq!(main_corner(&mut app), (120, 80));
    assert_eq!((app.zoom(), app.on_top(), app.opacity()), (1, false, 100));
}

#[test]
fn the_menu_shows_the_skins_and_the_switches() {
    let mut app = start("", &scratch("menu"));
    let bar = app.menu(Menu::Bar);
    for line in [
        ">\tFile",
        "C\t1\t0\t0\tOpen Files…\to",
        "C\t4\t0\t0\tOpen Skin…\tshift+o",
        "C\t26\t0\t0\tQuit Andamp\tq",
        "C\t200\t1\t0\tAndAmp Dark\t",
        "C\t201\t0\t0\tAndAmp Light\t",
        "C\t20\t0\t0\tDouble Size\td",
        "C\t21\t0\t0\tAlways on Top\tt",
        "C\t28\t0\t0\tPreferences…\t,",
        ">\tPlaylist",
        "C\t45\t0\t0\tSelect All\ta",
        "C\t100\t1\t0\t100 %\t",
        "C\t23\t0\t0\tVisualizer\tshift+k",
    ] {
        assert!(
            bar.lines().any(|l| l == line),
            "the bar has {line:?}:\n{bar}"
        );
    }
    // every submenu that starts also ends
    assert_eq!(
        bar.lines().filter(|l| l.starts_with('>')).count(),
        bar.lines().filter(|l| *l == "<").count()
    );

    app.command(command::ON_TOP);
    app.command(command::OPACITY + 3);
    app.command(command::SKIN + 1);
    assert_eq!((app.on_top(), app.opacity()), (true, 70));
    assert_eq!(app.skin_name(), "AndAmp Light.wsz");
    let popup = app.menu(Menu::Popup);
    for line in [
        "C\t21\t1\t0\tAlways on Top\t",
        "C\t103\t1\t0\t70 %\t",
        "C\t201\t1\t0\tAndAmp Light\t",
    ] {
        assert!(
            popup.lines().any(|l| l == line),
            "the popup has {line:?}:\n{popup}"
        );
    }
    let requests = app.take_requests();
    assert!(requests.contains(&Request::Look) && requests.contains(&Request::Skin));

    let presets = app.menu(Menu::Equalizer);
    assert!(presets.starts_with("C\t300\t0\t0\tFlat\t\nS\n"));
    assert_eq!(
        presets.lines().count(),
        2 + andamp_core::player::EQ_PRESETS.len()
    );
}

#[test]
fn double_size_keeps_the_main_window_where_it_is() {
    let mut app = start("", &scratch("zoom"));
    app.skin().unwrap().move_windows(200, 100);
    app.step();
    let before = main_corner(&mut app);
    assert_eq!(before, (320, 180));
    // the engine has a window's new place after its next step
    app.command(command::DOUBLE_SIZE);
    app.step();
    assert_eq!(app.zoom(), 2);
    assert_eq!(main_corner(&mut app), before);
    app.command(command::DOUBLE_SIZE);
    app.step();
    assert_eq!(app.zoom(), 1);
    assert_eq!(main_corner(&mut app), before);
}

#[test]
fn the_skins_buttons_ask_the_shell() {
    let mut app = start("walkit-classic.wsz", &scratch("buttons"));
    // no file by that name: the default skin is not replaced, and the shell says why
    assert!(matches!(
        app.take_requests().first(),
        Some(Request::Error(_))
    ));

    let skin = resources().join("skins/walkit-classic.wsz");
    assert!(app.wear(&skin));
    app.step();
    app.take_requests();
    let (x, y) = main_corner(&mut app);
    let click = |app: &mut App, dx: i32, dy: i32| {
        let skin = app.skin().unwrap();
        skin.runtime.pointer_down(x + dx, y + dy, 1);
        skin.runtime.pointer_up(x + dx, y + dy);
        app.step();
        app.take_requests()
    };
    // a classic main window: close at its top right, minimize two buttons before it
    assert_eq!(click(&mut app, 268, 7), [Request::Quit]);
    assert_eq!(click(&mut app, 248, 7), [Request::Hide]);
    // the eject button asks for files to play
    assert_eq!(click(&mut app, 146, 96), [Request::Ask(Ask::OpenSongs)]);
    // the menu button at the top left asks for the app's menu, with a place under it
    let asked = click(&mut app, 10, 7);
    assert!(
        matches!(
            asked.as_slice(),
            [Request::Popup {
                menu: Menu::Popup,
                ..
            }]
        ),
        "the menu button asked {asked:?}"
    );
    // the clutter bar's D is double size
    let asked = click(&mut app, 14, 51);
    assert_eq!(app.zoom(), 2, "the D button asked {asked:?}");
    assert!(asked.contains(&Request::Look));
}

#[test]
fn what_the_listener_picks_is_done() {
    let dir = scratch("picked");
    let mut app = start("", &dir);
    let (a, b) = (song(&dir, "a.wav"), song(&dir, "b.wav"));
    app.picked(Ask::AddSongs, std::slice::from_ref(&a));
    assert_eq!(app.player.borrow().entries().len(), 1);
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Stopped,
        "adding does not play"
    );
    app.picked(Ask::OpenSongs, &[b.clone(), a.clone()]);
    assert_eq!(
        app.player.borrow().entries().len(),
        2,
        "opening takes the playlist's place"
    );
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    app.command(command::STOP);
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Stopped
    );

    app.picked(Ask::Url, &["ftp://not.a.stream/".into()]);
    assert_eq!(
        app.player.borrow().entries().len(),
        2,
        "not an address of a stream"
    );
    app.picked(Ask::OpenSongs, &[]);
    assert_eq!(
        app.player.borrow().entries().len(),
        2,
        "nothing picked changes nothing"
    );

    let list = dir.join("list.m3u8").to_string_lossy().into_owned();
    app.picked(Ask::SaveList, std::slice::from_ref(&list));
    app.picked(Ask::LoadList, &[list]);
    assert_eq!(app.player.borrow().entries().len(), 2);
    app.command(command::STOP);

    let light = resources()
        .join("skins/AndAmp Light.wsz")
        .to_string_lossy()
        .into_owned();
    app.picked(Ask::Skin, std::slice::from_ref(&light));
    assert_eq!(app.skin_name(), "AndAmp Light.wsz");
    // what the system opens: the skin is worn and the songs play
    app.open(&[
        a,
        resources()
            .join("skins/AndAmp Spot.wsz")
            .to_string_lossy()
            .into_owned(),
    ]);
    assert_eq!(app.skin_name(), "AndAmp Spot.wsz");
    assert_eq!(app.player.borrow().entries().len(), 1);
    app.command(command::STOP);

    app.command(command::EQ_PRESET + 1);
    let state = app.player.borrow().state();
    app.command(command::EQ_PRESET);
    assert_ne!(app.player.borrow().state(), state, "a preset is not flat");
}

#[test]
fn the_next_run_starts_from_what_this_one_left() {
    let dir = scratch("kept");
    let (a, b) = (song(&dir, "a.wav"), song(&dir, "b.wav"));
    let mut app = start("", &dir);
    app.command(command::SKIN + 2);
    app.step();
    app.skin().unwrap().move_windows(300, 200);
    app.step();
    let corner = main_corner(&mut app);
    app.command(command::DOUBLE_SIZE);
    app.command(command::ON_TOP);
    app.command(command::OPACITY + 2);
    app.picked(Ask::AddSongs, &[a, b]);
    app.player.borrow_mut().select(1);
    app.player.borrow_mut().set_volume(77);
    app.step();
    app.save();
    let summary = app.summary();
    drop(app);

    let mut again = start("", &dir);
    assert_eq!(again.skin_name(), "AndAmp Spot.wsz");
    assert_eq!(
        (again.zoom(), again.on_top(), again.opacity()),
        (2, true, 80)
    );
    assert_eq!(main_corner(&mut again), corner);
    assert_eq!(again.player.borrow().entries().len(), 2);
    assert_eq!(again.player.borrow().index(), 1);
    assert_eq!(again.player.borrow().volume(), 77);
    assert_eq!(again.summary(), summary);

    // a run that only measures starts from nothing and leaves nothing
    let mut bench = start("--bench 5", &dir);
    assert_eq!(bench.skin_name(), "AndAmp Dark.wsz");
    assert_eq!(
        (bench.zoom(), bench.player.borrow().entries().len()),
        (1, 0)
    );
    bench.command(command::SKIN + 1);
    bench.save();
    assert_eq!(start("", &dir).skin_name(), "AndAmp Spot.wsz");
}

#[test]
fn what_the_system_opens_before_the_start_waits_for_it() {
    let dir = scratch("waiting");
    let a = song(&dir, "a.wav");
    let mut app = App::with(
        Options::default(),
        &resources(),
        (1728, 1000),
        Some(dir),
        Player::silent(),
    );
    app.open(&[a]);
    assert_eq!(app.player.borrow().entries().len(), 0);
    app.start();
    assert_eq!(app.next_due_ms(), 0);
    app.step();
    assert_eq!(
        app.player.borrow().entries().len(),
        0,
        "the skin shows before the songs are read"
    );
    assert_eq!(app.next_due_ms(), 0);
    app.step();
    assert_eq!(app.player.borrow().entries().len(), 1);
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    app.command(command::STOP);
}

#[test]
fn the_listeners_own_plugins_are_kept_and_can_go() {
    let dir = scratch("plugins");
    let mut app = start("", &dir);
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/plugins/gain.lua");
    let index = app.add_plugin(&file).expect("the plug-in loads");
    assert!(app.is_own(index));
    assert!(
        !app.is_own(0),
        "one that comes with the app is not the listener's"
    );
    let id = app.player.borrow().rack.effects()[index]
        .plugin
        .info
        .id
        .clone();
    let kept = dir.join("Plugins").join(format!("{id}.lua"));
    assert!(kept.exists());
    assert!(app
        .add_plugin(&resources().join("skins/AndAmp Dark.wsz"))
        .is_err());
    app.player.borrow_mut().rack.set_on(index, true);
    app.save();
    drop(app);

    // left on: loaded at the start, and on again
    let mut again = start("", &dir);
    let index = again
        .player
        .borrow()
        .rack
        .effects()
        .iter()
        .position(|e| e.plugin.info.id == id)
        .expect("still there");
    assert!(again.player.borrow().rack.effects()[index].on);
    assert!(again.is_own(index));
    again.remove_plugin(0);
    assert_eq!(
        again.player.borrow().rack.effects().len(),
        index + 1,
        "only the listener's own can go"
    );
    again.remove_plugin(index);
    assert!(!kept.exists());
}

#[test]
fn presets_go_round_by_name() {
    assert_eq!(
        by_name("preset 9.milk", "preset 10.milk"),
        std::cmp::Ordering::Less
    );
    assert_eq!(by_name("Alpha", "beta"), std::cmp::Ordering::Less);
    assert_eq!(
        by_name("a007", "a7"),
        std::cmp::Ordering::Less,
        "the same number: by its spelling"
    );

    let dir = scratch("presets");
    std::fs::create_dir_all(dir.join("more")).unwrap();
    for name in [
        "b 10.milk",
        "b 9.milk",
        "more/c.MILK",
        "not a preset.txt",
        "A.milk",
    ] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    let mut presets = Presets::default();
    assert_eq!(
        presets.key("right"),
        Key::Taken,
        "no presets: the key is the window's, nothing changes"
    );
    assert_eq!(presets.title(), "Visualizer");
    presets.read(&dir);
    assert_eq!(presets.len(), 4);
    assert_eq!(presets.title(), "Visualizer: A");
    assert_eq!(presets.key("right"), Key::Changed);
    assert_eq!(presets.title(), "Visualizer: b 9");
    assert_eq!(presets.key("left"), Key::Changed);
    assert_eq!(presets.key("left"), Key::Changed);
    assert_eq!(
        presets.title(),
        "Visualizer: c",
        "round the start to the last"
    );
    assert_eq!(presets.key("q"), Key::Other);
    assert_eq!(presets.key("r"), Key::Changed);
    assert!(presets.index() < 4);

    presets.go(0);
    assert!(presets.pass());
    assert_eq!(presets.index(), 1);
    assert_eq!(presets.key("space"), Key::Taken);
    assert!(!presets.pass(), "held: time passing does not change it");
    assert_eq!(presets.index(), 1);

    // the app: the window opens, the first preset is asked for, and the folder is kept
    let config = scratch("presets-app");
    let mut app = start("", &config);
    app.take_requests();
    app.command(command::CHOOSE_PRESETS);
    assert_eq!(app.take_requests(), [Request::Ask(Ask::Presets)]);
    app.picked(Ask::Presets, &[dir.to_string_lossy().into_owned()]);
    assert_eq!(app.take_requests(), [Request::Vis(true)]);
    app.vis_shown(true);
    let requests = app.take_requests();
    let first = Request::Preset {
        file: dir.join("A.milk"),
        title: "Visualizer: A".into(),
        blend: false,
    };
    assert_eq!(requests, [Request::Menu, first]);
    assert_eq!(Request::Vis(true).line(), "vis\t1");
    assert!(app.vis_key("right"));
    assert_eq!(
        app.take_requests()[0].line(),
        format!(
            "preset\t1\tVisualizer: b 9\t{}",
            dir.join("b 9.milk").display()
        )
    );
    assert!(!app.vis_key("x"));
    app.vis_tick();
    assert_eq!(app.take_requests().len(), 1);
    app.command(command::VISUALIZER);
    assert_eq!(app.take_requests(), [Request::Vis(false)]);
}

/// Something a button of the skin asks, as the engine hands it over.
fn asks(name: &str, param: Option<&str>) -> HostAction {
    HostAction {
        name: name.into(),
        param: param.map(str::to_string),
        at: Some((10, 20)),
    }
}

/// The app with the songs `names` in its playlist, in a folder of their own.
fn with_songs(scratch_name: &str, names: &[&str]) -> (App, PathBuf) {
    let dir = scratch(scratch_name);
    let mut app = start("", &dir);
    let songs: Vec<String> = names.iter().map(|name| song(&dir, name)).collect();
    app.picked(Ask::AddSongs, &songs);
    app.step();
    app.take_requests();
    (app, dir)
}

fn titles(app: &App) -> Vec<String> {
    app.player
        .borrow()
        .entries()
        .iter()
        .map(|e| e.title.clone())
        .collect()
}

#[test]
fn the_playlists_selection_follows_its_commands() {
    let (mut app, _) = with_songs("select", &["a.wav", "b.wav", "c.wav", "d.wav"]);
    assert!(app.selection().is_empty());
    app.command(command::SELECT_ALL);
    assert_eq!(app.selection(), [0, 1, 2, 3]);
    app.select(&[2, 0]);
    assert_eq!(app.selection(), [0, 2], "lowest first");
    app.command(command::SELECT_INVERT);
    assert_eq!(app.selection(), [1, 3]);
    app.command(command::SELECT_NONE);
    assert!(app.selection().is_empty());

    // what a classic playlist's Sel button picks is those commands
    app.asked(&asks("PLAYLIST", Some("select_all")));
    assert_eq!(app.selection(), [0, 1, 2, 3]);
    app.asked(&asks("PLAYLIST", Some("select_invert")));
    assert!(app.selection().is_empty());
    app.asked(&asks("PLAYLIST", Some("select_invert")));
    app.asked(&asks("PLAYLIST", Some("select_none")));
    assert!(app.selection().is_empty());
}

#[test]
fn the_wheel_scrolls_the_playlist_under_it_and_turns_the_volume_elsewhere() {
    let dir = scratch("wheel");
    let mut app = start("", &dir);
    let one = song(&dir, "a.wav");
    app.picked(Ask::AddSongs, &vec![one; 30]);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let (list, main) = {
        let skin = app.skin().unwrap();
        let list = skin.runtime.window_of("pledit").expect("a playlist window");
        let main = skin.runtime.main_window().unwrap();
        (
            skin.runtime.window_rect(list),
            skin.runtime.window_rect(main),
        )
    };
    // the row under a press near the list's top says how far it scrolled
    let first_row = |app: &mut App| {
        let skin = app.skin().unwrap();
        skin.runtime.pointer_down(list.left + 60, list.top + 26, 1);
        skin.runtime.pointer_up(list.left + 60, list.top + 26);
        app.step();
        app.selection()
    };
    assert_eq!(first_row(&mut app), [0]);
    let volume = app.player.borrow().volume();

    // two notches down over the list: six rows
    app.wheel((list.left + 60, list.top + 40), -2.0, false);
    app.step();
    assert_eq!(first_row(&mut app), [6]);
    assert_eq!(app.player.borrow().volume(), volume, "the volume stays");
    // a trackpad moves the list as far as the fingers go: 26 points up are two rows back
    for _ in 0..13 {
        app.wheel((list.left + 60, list.top + 40), 2.0, true);
    }
    app.step();
    assert_eq!(first_row(&mut app), [4]);
    // past the top it stops
    app.wheel((list.left + 60, list.top + 40), 50.0, false);
    app.step();
    assert_eq!(first_row(&mut app), [0]);

    // over the volume slider the wheel is the volume: Winamp has it at 107, 58 of the main
    // window
    let slider = (main.left + 130, main.top + 62);
    app.wheel(slider, -3.0, false);
    assert_eq!(app.player.borrow().volume(), volume - 12);
    for _ in 0..16 {
        app.wheel(slider, 2.0, true);
    }
    assert_eq!(
        app.player.borrow().volume(),
        volume - 12 + 8,
        "32 points are two notches"
    );
    // anywhere else on the skin it does nothing
    let volume = app.player.borrow().volume();
    for at in [
        (main.left + 30, main.top + 30),
        (main.left + 200, main.top + 100),
        (5, 5),
    ] {
        app.wheel(at, -3.0, false);
    }
    assert_eq!(app.player.borrow().volume(), volume);
    app.step();
    assert_eq!(first_row(&mut app), [0], "the list stays");

    // a modern skin names its slider
    app.wear(&resources().join("skins/walkit-modern.wal"));
    app.step();
    let (window, slider) = {
        let skin = app.skin().unwrap();
        let slider = skin.runtime.object_info("volume").expect("a volume slider");
        (skin.runtime.window_rect(slider.window), slider.rect)
    };
    app.wheel(
        (window.left + slider.left + 10, window.top + slider.top + 4),
        2.0,
        false,
    );
    assert_eq!(app.player.borrow().volume(), volume + 8);
    app.wheel(
        (window.left + slider.left + 10, window.top + slider.top - 20),
        2.0,
        false,
    );
    assert_eq!(app.player.borrow().volume(), volume + 8);
}

#[test]
fn the_listeners_own_skins_are_offered_after_the_apps() {
    let dir = scratch("own-skins");
    // before the folder is there, and with what is no skin in it
    let mut app = start("", &dir);
    assert!(app.own_skins().is_empty());
    let folder = app.skins_folder().expect("a folder for skins");
    assert_eq!(folder, dir.join("Skins"));
    std::fs::create_dir_all(&folder).unwrap();
    let dark = resources().join("skins/AndAmp Dark.wsz");
    for name in [
        "Mine 10.wsz",
        "mine 9.wsz",
        "AndAmp Light.wsz",
        ".hidden.wsz",
    ] {
        std::fs::copy(&dark, folder.join(name)).unwrap();
    }
    std::fs::write(folder.join("notes.txt"), "not a skin").unwrap();
    // by name as a file browser has them; one named as a skin of the app's is the app's
    assert_eq!(app.own_skins(), ["mine 9.wsz", "Mine 10.wsz"]);
    let builtin = app.builtin_skins().len() as i32;
    let bar = app.menu(Menu::Bar);
    let lines: Vec<&str> = bar.lines().collect();
    let first = format!("C\t{}\t0\t0\tmine 9\t", command::SKIN + builtin);
    let at = lines
        .iter()
        .position(|l| *l == first)
        .unwrap_or_else(|| panic!("the bar has {first:?}:\n{bar}"));
    assert_eq!(lines[at - 1], "S", "under a line of their own");
    assert_eq!(
        lines[at + 1],
        format!("C\t{}\t0\t0\tMine 10\t", command::SKIN + builtin + 1)
    );

    // picked from the menu it is worn and checked, and the next run wears it again
    app.command(command::SKIN + builtin + 1);
    assert_eq!(app.skin_name(), "Mine 10.wsz");
    assert!(app.menu(Menu::Popup).contains(&format!(
        "C\t{}\t1\t0\tMine 10\t",
        command::SKIN + builtin + 1
    )));
    app.save();
    let mut next = start("", &dir);
    assert_eq!(next.skin_name(), "Mine 10.wsz");

    // the Preferences window offers it too, and shows the folder
    assert!(next.preferences().contains("mine 9.wsz"));
    next.set_preference("skin", "mine 9.wsz");
    assert_eq!(next.skin_name(), "mine 9.wsz");
    next.set_preference("skin", "notes.txt");
    assert_eq!(next.skin_name(), "mine 9.wsz");
    next.take_requests();
    next.set_preference("skins.reveal", "");
    assert_eq!(next.take_requests(), [Request::Reveal(folder)]);
}

#[test]
fn the_cursor_says_what_a_press_would_size() {
    let dir = scratch("cursor");
    let mut app = start("", &dir);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let (list, main) = {
        let skin = app.skin().unwrap();
        let list = skin.runtime.window_of("pledit").expect("a playlist window");
        let main = skin.runtime.main_window().unwrap();
        (
            skin.runtime.window_rect(list),
            skin.runtime.window_rect(main),
        )
    };
    // a classic playlist is sized by its bottom right corner; the rest is the arrow's
    assert_eq!(
        app.cursor_at(list.left + list.width - 4, list.top + list.height - 4),
        Cursor::Diagonal
    );
    assert_eq!(app.cursor_at(list.left + 60, list.top + 40), Cursor::Arrow);
    assert_eq!(app.cursor_at(main.left + 60, main.top + 40), Cursor::Arrow);
    assert_eq!(app.cursor_at(5, 5), Cursor::Arrow, "off the skin");
    // a mouse means the pixel it is on: nothing beside a control or a divider is the
    // control's
    assert_eq!(app.skin().unwrap().runtime.presentation().touch_slop, 0);
}

#[test]
fn a_skins_windows_stay_together_when_the_size_changed_while_another_was_worn() {
    let dir = scratch("places-zoom");
    let mut app = start("", &dir);
    let classic = resources().join("skins/walkit-classic.wsz");
    let modern = resources().join("skins/walkit-modern.wal");
    // how far under the main window the others sit, in layout pixels
    let under_main = |app: &mut App| -> Vec<(i32, i32)> {
        app.step();
        app.step();
        let zoom = app.zoom();
        let main = app.main_window().unwrap();
        let places = app.places();
        let (_, at) = *places.iter().find(|(id, _)| *id == main).unwrap();
        let mut offsets: Vec<(i32, i32)> = places
            .iter()
            .filter(|(id, _)| *id != main)
            .map(|(_, (x, y))| ((x - at.0) / zoom, (y - at.1) / zoom))
            .collect();
        offsets.sort_unstable();
        offsets
    };
    app.wear(&classic);
    let docked = under_main(&mut app);
    assert_eq!(docked, [(0, 116), (0, 232)]);
    // double size, another skin, back to the plain size there, and the first skin again: its
    // windows are as far apart in its own pixels as they were, not in points of the screen
    app.set_zoom(2);
    assert_eq!(under_main(&mut app), docked);
    let corner = app.places().last().copied();
    app.wear(&modern);
    app.set_zoom(1);
    app.wear(&classic);
    assert_eq!(under_main(&mut app), docked, "docked as it was left");
    assert_eq!(
        app.places().last().copied(),
        corner,
        "the main window's corner is where it was on the screen"
    );
    // and the other way: kept at the plain size, worn again at double size
    app.wear(&modern);
    app.set_zoom(2);
    app.wear(&classic);
    assert_eq!(under_main(&mut app), docked);
    // the next run finds them the same way
    app.wear(&modern);
    app.set_zoom(1);
    app.save();
    let mut next = start("", &dir);
    next.wear(&classic);
    assert_eq!(under_main(&mut next), docked);
}

/// A cursor file of 2 by 2 pixels: red and green over blue and one the mask turns black, its
/// tip at `tip`.
fn cursor_file(tip: (u16, u16)) -> Vec<u8> {
    let mut picture = Vec::new();
    picture.extend(40u32.to_le_bytes());
    picture.extend(2i32.to_le_bytes());
    picture.extend(4i32.to_le_bytes());
    picture.extend(1u16.to_le_bytes());
    picture.extend(24u16.to_le_bytes());
    picture.extend([0u8; 24]);
    // rows from the bottom, three bytes a pixel, filled up to four bytes
    picture.extend([0xff, 0, 0, 0xff, 0xff, 0xff, 0, 0]);
    picture.extend([0, 0, 0xff, 0, 0xff, 0, 0, 0]);
    // the mask, a bit a pixel: set for the bottom right one
    picture.extend([0x40, 0, 0, 0]);
    picture.extend([0, 0, 0, 0]);
    let mut file = vec![0, 0, 2, 0, 1, 0, 2, 2, 0, 0];
    file.extend(tip.0.to_le_bytes());
    file.extend(tip.1.to_le_bytes());
    file.extend((picture.len() as u32).to_le_bytes());
    file.extend(22u32.to_le_bytes());
    file.extend(picture);
    file
}

#[test]
fn a_classic_skins_own_cursor_shows_where_the_skin_has_one() {
    use std::io::Write;
    let dir = scratch("skin-cursor");
    // the app's classic skin with a cursor for the volume and balance sliders added
    let with_cursor = dir.join("Cursors.wsz");
    {
        let from = std::fs::File::open(resources().join("skins/walkit-classic.wsz")).unwrap();
        let mut from = zip::ZipArchive::new(from).unwrap();
        let mut to = zip::ZipWriter::new(std::fs::File::create(&with_cursor).unwrap());
        for n in 0..from.len() {
            to.raw_copy_file(from.by_index_raw(n).unwrap()).unwrap();
        }
        to.start_file("VOLBAL.CUR", zip::write::SimpleFileOptions::default())
            .unwrap();
        to.write_all(&cursor_file((1, 0))).unwrap();
        to.finish().unwrap();
    }
    let mut app = start("", &dir);
    assert!(app.wear(&with_cursor));
    app.step();
    let main = {
        let skin = app.skin().unwrap();
        let id = skin.runtime.main_window().unwrap();
        skin.runtime.window_rect(id)
    };
    // over the volume slider it is the skin's; the C interface numbers those from 100
    let over_volume = app.cursor_at(main.left + 130, main.top + 62);
    assert_eq!(over_volume, Cursor::Skin(0));
    assert_eq!(over_volume.number(), Cursor::SKIN);
    // one step that stands still, one picture of 2 by 2 with its tip at 1, 0
    assert_eq!(
        app.cursor_picture(0).expect("the cursor's pictures"),
        [
            1,
            0,
            0,
            1,
            2,
            2,
            1,
            0,
            0xffff_0000,
            0xff00_ff00,
            0xff00_00ff,
            0xff00_0000
        ]
    );
    // where the skin brings no cursor the arrow stays, and there are no pictures to give
    assert_eq!(app.cursor_at(main.left + 60, main.top + 40), Cursor::Arrow);
    assert_eq!(app.cursor_picture(8), None);
    // another skin, without cursors: the arrow over its volume slider too
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    assert_eq!(app.cursor_at(main.left + 130, main.top + 62), Cursor::Arrow);
    assert_eq!(app.cursor_picture(0), None);
}

#[test]
fn the_wheel_scrolls_a_modern_skins_playlist() {
    let dir = scratch("modern-wheel");
    let mut app = start("", &dir);
    let one = song(&dir, "a.wav");
    app.picked(Ask::AddSongs, &vec![one; 30]);
    app.wear(&resources().join("skins/walkit-modern.wal"));
    app.step();
    let list = {
        let skin = app.skin().unwrap();
        let id = skin.runtime.window_of("pledit").expect("a playlist window");
        skin.runtime.window_rect(id)
    };
    // the row under a press near the list's top says how far it scrolled
    let first_row = |app: &mut App| {
        let skin = app.skin().unwrap();
        skin.runtime.pointer_down(list.left + 60, list.top + 26, 1);
        skin.runtime.pointer_up(list.left + 60, list.top + 26);
        app.step();
        app.selection()
    };
    assert_eq!(first_row(&mut app), [0]);
    // two notches down: six rows
    app.wheel((list.left + 60, list.top + 40), -2.0, false);
    app.step();
    assert_eq!(first_row(&mut app), [6]);
    // a trackpad moves the list as far as the fingers go: 30 points up are two rows back
    for _ in 0..15 {
        app.wheel((list.left + 60, list.top + 40), 2.0, true);
    }
    app.step();
    assert_eq!(first_row(&mut app), [4]);
    // past the top it stops; outside the window, and over the window's title bar above the
    // list, the wheel is not the list's
    app.wheel((list.left + 60, list.top + 40), 50.0, false);
    app.wheel((list.left + 60, list.top - 40), -2.0, false);
    app.wheel((list.left + 60, list.top + 8), -2.0, false);
    app.step();
    assert_eq!(first_row(&mut app), [0]);
}

#[test]
fn the_window_the_keys_go_to_is_the_skins_one_in_front() {
    let dir = scratch("focus");
    let mut app = start("", &dir);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let (list, main) = {
        let skin = app.skin().unwrap();
        let list = skin.runtime.window_of("pledit").expect("a playlist window");
        (list, skin.runtime.main_window().unwrap())
    };
    let in_front = |app: &mut App| app.skin().unwrap().runtime.focused_window();
    assert_eq!(in_front(&mut app), Some(main));
    // the system gave the keys to the playlist's window, with no press on it: the title
    // bars are drawn anew
    app.focus(list.0);
    assert_eq!(in_front(&mut app), Some(list));
    assert!(app.step() > 0, "the skin shows it");
    app.focus(main.0);
    assert_eq!(in_front(&mut app), Some(main));
    // a window that is none of the skin's changes nothing
    app.focus(9999);
    assert_eq!(in_front(&mut app), Some(main));
}

#[test]
fn a_press_too_short_to_see_still_shows() {
    let (mut app, _) = with_songs("tap", &["a.wav"]);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    app.step();
    let main = app.main_window().expect("a main window");
    let pixels = |app: &mut App| {
        let window = app
            .skin()
            .unwrap()
            .windows()
            .iter()
            .find(|w| w.id == main)
            .unwrap();
        window.canvas.pixels.clone()
    };
    let (x, y) = main_corner(&mut app);
    // Winamp's play button: 23 by 18 at 39, 88 of the main window
    let (x, y) = (x + 45, y + 96);
    let idle = pixels(&mut app);

    // down and up at once, as a tap on a trackpad
    app.pointer_down(x, y, 1);
    app.pointer_up(x, y);
    app.step();
    let pressed = pixels(&mut app);
    assert!(pressed != idle, "the button shows pressed");
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Stopped,
        "and has not acted yet"
    );
    let due = app.next_due_ms();
    assert!((0..=90).contains(&due), "the release is due in {due} ms");
    // a drag after the mouse let go moves nothing
    app.pointer_move(x + 200, y);
    std::thread::sleep(std::time::Duration::from_millis(100));
    app.step();
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    app.step();
    assert!(pixels(&mut app) != pressed, "the button is up again");
    app.command(command::STOP);

    // a press held long enough acts when the mouse lets go
    app.pointer_down(x, y, 2);
    app.step();
    std::thread::sleep(std::time::Duration::from_millis(100));
    app.pointer_up(x, y);
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    app.command(command::STOP);

    // a second press before the first was let go lets it go first
    app.pointer_down(x, y, 3);
    app.pointer_up(x, y);
    app.pointer_down(x, y, 4);
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    app.pointer_up(x, y);
    app.command(command::STOP);
}

#[test]
fn the_playlist_follows_the_song_that_plays() {
    let dir = scratch("follow");
    let mut app = start("", &dir);
    let one = song(&dir, "a.wav");
    app.picked(Ask::AddSongs, &vec![one; 30]);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let list = {
        let skin = app.skin().unwrap();
        let id = skin.runtime.window_of("pledit").expect("a playlist window");
        skin.runtime.window_rect(id)
    };
    // the row under a press near the list's top says how far it scrolled
    let first_row = |app: &mut App| {
        let skin = app.skin().unwrap();
        skin.runtime.pointer_down(list.left + 60, list.top + 26, 1);
        skin.runtime.pointer_up(list.left + 60, list.top + 26);
        app.step();
        app.selection()[0]
    };
    assert_eq!(first_row(&mut app), 0);
    // the song after the next, and so on: past what the window shows the list comes along
    for _ in 0..14 {
        app.command(command::NEXT);
        app.step();
    }
    app.command(command::STOP);
    assert_eq!(app.player.borrow().index(), 14);
    let top = first_row(&mut app);
    assert!(
        (12..=14).contains(&top),
        "song 14 is in view: the first row is {top}"
    );
    // the listener scrolls away, and the list stays there while the same song is current
    app.wheel((list.left + 60, list.top + 40), 50.0, false);
    app.step();
    app.step();
    assert_eq!(first_row(&mut app), 0);
    // a key in the list brings back no more than it must: from the top, the row selected
    // comes in at the bottom
    let window = app.skin().unwrap().runtime.window_of("pledit").unwrap().0;
    app.select(&[16]);
    app.key_in(window, "down");
    app.step();
    assert_eq!(app.selection(), [17]);
    let top = first_row(&mut app);
    assert!(
        (1..=15).contains(&top),
        "row 17 came in from below: the first row is {top}"
    );
    // another song, and the list is back with it; another skin, and its list is too
    app.player.borrow_mut().select(20);
    app.step();
    let top = first_row(&mut app);
    assert!(
        (18..=20).contains(&top),
        "song 20 is in view: the first row is {top}"
    );
    app.wear(&resources().join("skins/AndAmp Dark.wsz"));
    app.step();
    app.step();
    let top = first_row(&mut app);
    assert!(
        (18..=20).contains(&top),
        "the new skin's list is at song 20 too: the first row is {top}"
    );
}

#[test]
fn the_arrow_keys_move_the_selection_in_the_playlists_window() {
    let dir = scratch("arrows");
    let mut app = start("", &dir);
    let one = song(&dir, "a.wav");
    app.picked(Ask::AddSongs, &vec![one; 30]);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let (list, main, at) = {
        let skin = app.skin().unwrap();
        let list = skin.runtime.window_of("pledit").expect("a playlist window");
        let rect = skin.runtime.window_rect(list);
        (list.0, skin.runtime.main_window().unwrap().0, rect)
    };
    let volume = app.player.borrow().volume();

    // the first press selects the current song, the next ones move from it
    assert!(app.key_in(list, "down"));
    assert_eq!(app.selection(), [0]);
    assert!(app.key_in(list, "down"));
    assert!(app.key_in(list, "down"));
    assert_eq!(app.selection(), [2]);
    assert!(app.key_in(list, "up"));
    assert_eq!(app.selection(), [1]);
    assert_eq!(
        app.player.borrow().volume(),
        volume,
        "not the volume, in this window"
    );
    assert!(app.key_in(list, "end"));
    assert_eq!(app.selection(), [29]);
    assert!(app.key_in(list, "down"));
    assert_eq!(app.selection(), [29], "the list ends there");
    assert!(app.key_in(list, "home"));
    assert!(app.key_in(list, "up"));
    assert_eq!(app.selection(), [0]);

    // the list follows the selection: after going down past what shows, the row under a
    // press near the list's bottom edge of rows is the selected one or one near it
    for _ in 0..12 {
        app.key_in(list, "down");
    }
    assert_eq!(app.selection(), [12]);
    app.step();
    let skin = app.skin().unwrap();
    skin.runtime.pointer_down(at.left + 60, at.top + 26, 1);
    skin.runtime.pointer_up(at.left + 60, at.top + 26);
    app.step();
    let top = app.selection()[0];
    assert!(
        (10..=12).contains(&top),
        "the list scrolled with the selection: its first row is {top}"
    );

    // return plays what is selected, delete removes it
    app.select(&[5]);
    assert!(app.key_in(list, "return"));
    assert_eq!(app.player.borrow().index(), 5);
    app.command(command::STOP);
    assert!(app.key_in(list, "delete"));
    assert_eq!(app.player.borrow().entries().len(), 29);

    // in the main window the arrows are Winamp's: the volume
    assert!(app.key_in(main, "up"));
    assert_eq!(app.player.borrow().volume(), volume + 5);
    assert!(app.selection().is_empty());
}

#[test]
fn the_delete_key_removes_the_selection_and_return_plays_it() {
    let (mut app, _) = with_songs("keys", &["a.wav", "b.wav", "c.wav"]);
    assert!(app.key("delete"), "the key is the playlist's");
    assert_eq!(titles(&app).len(), 3, "nothing selected: nothing goes");
    app.select(&[0, 2]);
    assert!(app.key("delete"));
    assert_eq!(titles(&app), ["b"]);
    assert!(app.selection().is_empty());
    app.picked(Ask::AddSongs, &[]);
    app.select(&[0]);
    assert!(app.key("return"));
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Playing
    );
    // Winamp's keys still go to the player
    assert!(app.key("v"));
    assert_eq!(
        app.player.borrow().status(),
        walkit::player::Status::Stopped
    );
    assert!(!app.key("q"), "not a key of the app's");
}

#[test]
fn removing_takes_the_selection_or_what_is_not_selected() {
    let (mut app, dir) = with_songs("remove", &["a.wav", "b.wav", "c.wav", "d.wav"]);
    app.command(command::REMOVE_SELECTED);
    assert_eq!(titles(&app).len(), 4, "nothing selected: nothing goes");
    app.command(command::CROP);
    assert_eq!(
        titles(&app).len(),
        4,
        "nothing selected: cropping keeps the list"
    );

    app.select(&[1]);
    app.asked(&asks("PLAYLIST", Some("remove_selected")));
    assert_eq!(titles(&app), ["a", "c", "d"]);
    assert!(app.selection().is_empty(), "the rows are other entries now");

    app.select(&[0, 2]);
    app.asked(&asks("PLAYLIST", Some("crop")));
    assert_eq!(titles(&app), ["a", "d"]);

    // a file that is gone, and one that is there twice
    let d = dir.join("d.wav").to_string_lossy().into_owned();
    app.picked(Ask::AddSongs, &[d.clone(), song(&dir, "e.wav"), d]);
    assert_eq!(titles(&app), ["a", "d", "d", "e", "d"]);
    app.command(command::REMOVE_DUPLICATES);
    assert_eq!(titles(&app), ["a", "d", "e"]);
    std::fs::remove_file(dir.join("a.wav")).unwrap();
    app.command(command::REMOVE_DEAD);
    assert_eq!(titles(&app), ["d", "e"]);

    app.asked(&asks("PLAYLIST", Some("remove_misc")));
    let asked = app.take_requests();
    assert!(
        matches!(
            asked.as_slice(),
            [Request::Popup {
                menu: Menu::RemoveMisc,
                at: Some((10, 20))
            }]
        ),
        "the Rem button's Misc asked {asked:?}"
    );
    app.asked(&asks("PLAYLIST", Some("list_new")));
    assert!(titles(&app).is_empty());
}

#[test]
fn sorting_keeps_the_song_that_is_current() {
    let dir = scratch("sort");
    let mut app = start("", &dir);
    std::fs::create_dir_all(dir.join("x")).unwrap();
    std::fs::create_dir_all(dir.join("y")).unwrap();
    let songs = [
        song(&dir.join("y"), "Alpha.wav"),
        song(&dir.join("x"), "beta.wav"),
        song(&dir.join("x"), "Gamma.wav"),
    ];
    app.picked(Ask::AddSongs, &songs);
    app.player.borrow_mut().select(1);
    app.select(&[0]);

    app.command(command::SORT_PATH);
    assert_eq!(
        titles(&app),
        ["beta", "Gamma", "Alpha"],
        "by folder, then name"
    );
    assert_eq!(
        app.player.borrow().index(),
        0,
        "beta is still the current song"
    );
    assert!(
        app.selection().is_empty(),
        "the rows moved under the selection"
    );
    app.command(command::SORT_FILENAME);
    assert_eq!(
        titles(&app),
        ["Alpha", "beta", "Gamma"],
        "without regard to case"
    );
    assert_eq!(app.player.borrow().index(), 1);
    app.command(command::REVERSE);
    assert_eq!(titles(&app), ["Gamma", "beta", "Alpha"]);
    app.command(command::SORT_TITLE);
    assert_eq!(titles(&app), ["Alpha", "beta", "Gamma"]);
    app.command(command::RANDOMIZE);
    let mut shuffled = titles(&app);
    shuffled.sort_by_key(|title| title.to_lowercase());
    assert_eq!(
        shuffled,
        ["Alpha", "beta", "Gamma"],
        "the same songs in some order"
    );
    let current = app.player.borrow().index();
    assert_eq!(titles(&app)[current], "beta");

    // the Misc button's Sort is a menu of these
    app.take_requests();
    app.asked(&asks("PLAYLIST", Some("sort")));
    assert!(matches!(
        app.take_requests().as_slice(),
        [Request::Popup {
            menu: Menu::Sort,
            ..
        }]
    ));
    let sort = app.menu(Menu::Sort);
    assert!(
        sort.lines()
            .any(|l| l == "C\t49\t0\t0\tSort by File Name\t"),
        "{sort}"
    );
}

#[test]
fn a_file_tells_what_it_is_and_the_list_becomes_a_page() {
    let (mut app, dir) = with_songs("info", &["first.wav", "second & <third>.wav"]);
    // with nothing selected a question is about the current song
    app.asked(&asks("PLAYLIST", Some("file_info")));
    assert_eq!(app.take_requests(), [Request::Info(0)]);
    app.asked(&asks("TRACKINFO", None));
    assert_eq!(app.take_requests(), [Request::Info(0)]);
    // a right click on a row is about that row
    app.asked(&asks("PLAYLIST_MENU", Some("1")));
    assert_eq!(app.selection(), [1]);
    assert!(matches!(
        app.take_requests().as_slice(),
        [Request::Popup {
            menu: Menu::PlaylistRow,
            ..
        }]
    ));
    app.command(command::FILE_INFO);
    assert_eq!(app.take_requests(), [Request::Info(1)]);
    app.command(command::REVEAL);
    assert_eq!(
        app.take_requests(),
        [Request::Reveal(dir.join("second & <third>.wav"))]
    );
    app.command(command::PLAY_SELECTED);
    assert_eq!(app.player.borrow().index(), 1);
    app.command(command::STOP);

    let info = app.file_info(0).expect("the first song");
    let lines: Vec<&str> = info.lines().collect();
    assert_eq!(lines[0], "first");
    assert!(lines.contains(&"Length: 0:01"), "{info}");
    assert!(
        lines.contains(&"Format: pcm_s16le, 44100 Hz, stereo, 1411 kbps"),
        "{info}"
    );
    assert_eq!(
        *lines.last().unwrap(),
        format!("File: {}", dir.join("first.wav").display())
    );
    assert_eq!(app.file_info(7), None);

    app.asked(&asks("PLAYLIST", Some("misc_options")));
    assert!(matches!(
        app.take_requests().as_slice(),
        [Request::Popup {
            menu: Menu::MiscOptions,
            ..
        }]
    ));
    app.command(command::HTML_LIST);
    assert_eq!(app.take_requests(), [Request::Ask(Ask::SaveHtml)]);
    let page = dir.join("list.html");
    app.picked(Ask::SaveHtml, &[page.to_string_lossy().into_owned()]);
    assert_eq!(app.take_requests(), [Request::Open(page.clone())]);
    let html = std::fs::read_to_string(page).unwrap();
    assert!(
        html.contains("<li>first <small>(0:01)</small></li>"),
        "{html}"
    );
    assert!(
        html.contains("<li>second &amp; &lt;third&gt; <small>(0:01)</small></li>"),
        "{html}"
    );
    assert!(html.contains("<p>2 songs, 0:02</p>"));
}

#[test]
fn the_skin_itself_selects_a_row_and_its_button_removes_it() {
    // through the engine: a press on a row of a classic playlist selects it, and a double
    // click on the Rem button picks its lowest item, Remove Selected. The engine clears what
    // it shows as selected right after the pick; the app still removes the row that was
    let (mut app, _) = with_songs("engine", &["a.wav", "b.wav", "c.wav"]);
    app.wear(&resources().join("skins/walkit-classic.wsz"));
    app.step();
    let playlist = {
        let windows = app.skin().unwrap().windows();
        // the playlist is the lowest of the three on the screen
        windows
            .iter()
            .map(|w| (w.rect.left, w.rect.top, w.rect.height))
            .max_by_key(|w| w.1)
            .unwrap()
    };
    let (left, top, height) = playlist;
    let skin = app.skin().unwrap();
    // the second row: rows are 13 high from 20 down
    skin.runtime.pointer_down(left + 60, top + 20 + 13 + 6, 1);
    skin.runtime.pointer_up(left + 60, top + 20 + 13 + 6);
    app.step();
    assert_eq!(app.selection(), [1]);
    // the Rem button: 43 from the left, 30 from the bottom
    let (x, y) = (left + 43 + 10, top + height - 30 + 8);
    let skin = app.skin().unwrap();
    skin.runtime.pointer_down(x, y, 2);
    skin.runtime.double_tap(x, y, 2);
    skin.runtime.pointer_up(x, y);
    app.step();
    assert_eq!(titles(&app), ["a", "c"]);
    assert!(app.selection().is_empty());
}

#[test]
fn a_skins_other_buttons_reach_their_menus() {
    let (mut app, _) = with_songs("others", &["a.wav"]);
    let popup = |app: &mut App, name: &str, param: Option<&str>| {
        app.asked(&asks(name, param));
        match app.take_requests().as_slice() {
            [Request::Popup { menu, .. }] => Some(*menu),
            _ => None,
        }
    };
    assert_eq!(
        popup(&mut app, "MENU", Some("wa5:options")),
        Some(Menu::Options)
    );
    assert_eq!(popup(&mut app, "MENU", None), Some(Menu::Popup));
    assert_eq!(popup(&mut app, "VIS_MENU", None), Some(Menu::Visualization));
    assert_eq!(popup(&mut app, "PE_ADD", None), Some(Menu::PlaylistAdd));
    assert_eq!(popup(&mut app, "PE_REM", None), Some(Menu::PlaylistRemove));
    assert_eq!(popup(&mut app, "PE_SEL", None), Some(Menu::PlaylistSelect));
    assert_eq!(popup(&mut app, "PE_MISC", None), Some(Menu::PlaylistMisc));
    assert_eq!(popup(&mut app, "PE_LIST", None), Some(Menu::PlaylistList));
    app.asked(&asks("VIS_FS", None));
    assert_eq!(app.take_requests(), [Request::Vis(true)]);
    // every menu spells itself out, each submenu closed, each command with a number
    for menu in Menu::ALL {
        let text = app.menu(menu);
        assert!(!text.is_empty(), "{menu:?}");
        let (open, close) = (
            text.lines().filter(|l| l.starts_with('>')).count(),
            text.lines().filter(|l| *l == "<").count(),
        );
        assert_eq!(open, close, "{menu:?}");
        for line in text.lines().filter(|l| l.starts_with('C')) {
            let parts: Vec<&str> = line.split('\t').collect();
            assert_eq!(parts.len(), 6, "{menu:?}: {line:?}");
            assert!(
                parts[1].parse::<i32>().is_ok_and(|id| id > 0),
                "{menu:?}: {line:?}"
            );
        }
    }
    // with nothing selected, what needs a selection is grayed
    let remove = app.menu(Menu::PlaylistRemove);
    assert!(
        remove
            .lines()
            .any(|l| l == "C\t40\t0\t1\tRemove Selected\t"),
        "{remove}"
    );
    app.select(&[0]);
    let remove = app.menu(Menu::PlaylistRemove);
    assert!(
        remove
            .lines()
            .any(|l| l == "C\t40\t0\t0\tRemove Selected\t"),
        "{remove}"
    );
    // shuffle and repeat show in the options, and turn with their commands
    app.command(command::SHUFFLE);
    assert!(app.player.borrow().shuffle());
    let options = app.menu(Menu::Options);
    assert!(
        options.lines().any(|l| l == "C\t30\t1\t0\tShuffle\t"),
        "{options}"
    );
    assert!(
        options.lines().any(|l| l == "C\t31\t0\t0\tRepeat\t"),
        "{options}"
    );
}

#[test]
fn the_preferences_spell_themselves_out_and_take_changes() {
    use walkit::classic::vis::VisMode;
    let dir = scratch("prefs");
    let mut app = start("", &dir);
    app.take_requests();
    let prefs = app.preferences();
    assert!(
        prefs.starts_with("{\"pages\":[{\"title\":\"Look\","),
        "{prefs}"
    );
    for part in [
        "{\"id\":\"double\",\"kind\":\"toggle\",\"label\":\"Double size\",\"value\":\"0\"}",
        "{\"id\":\"opacity\",\"kind\":\"slider\",\"label\":\"Opacity\",\"value\":\"100\",\"min\":40,\"max\":100,\"step\":10,\"unit\":\"%\"}",
        "{\"name\":\"AndAmp Light\",\"value\":\"AndAmp Light.wsz\"}",
        "\"id\":\"skin\",\"kind\":\"choice\",\"label\":\"Skin\",\"value\":\"AndAmp Dark.wsz\"",
        "\"id\":\"vis.mode\",\"kind\":\"choice\",\"label\":\"Shows\",\"value\":\"analyzer\"",
        "{\"id\":\"milk.stay\",\"kind\":\"slider\",\"label\":\"A preset stays\",\"value\":\"30\",\"min\":5,\"max\":120,\"step\":5,\"unit\":\"s\"}",
        "{\"title\":\"About\",",
    ] {
        assert!(prefs.contains(part), "the preferences have {part}:\n{prefs}");
    }
    // braces and brackets pair up
    assert_eq!(prefs.matches('{').count(), prefs.matches('}').count());
    assert_eq!(prefs.matches('[').count(), prefs.matches(']').count());

    app.set_preference("double", "1");
    app.set_preference("ontop", "1");
    app.set_preference("opacity", "70");
    app.set_preference("shuffle", "1");
    app.set_preference("repeat", "1");
    app.set_preference("time", "remaining");
    app.set_preference("vis.mode", "scope");
    app.set_preference("vis.peaks", "0");
    app.set_preference("milk.stay", "45");
    app.set_preference("milk.fps", "60");
    app.set_preference("skin", "AndAmp Spot.wsz");
    app.set_preference("skin", "../../somewhere/else.wsz");
    app.set_preference("no such preference", "1");
    assert_eq!((app.zoom(), app.on_top(), app.opacity()), (2, true, 70));
    assert!(app.player.borrow().shuffle() && app.player.borrow().repeat());
    assert_eq!((app.vis_stay(), app.vis_fps()), (45, 60));
    assert_eq!(
        app.skin_name(),
        "AndAmp Spot.wsz",
        "only a skin that comes with the app is worn by name"
    );
    // the skin just worn is set as the one before it was
    assert_eq!(
        app.skin().unwrap().runtime.classic_vis_mode(),
        VisMode::Scope
    );
    assert!(app.skin().unwrap().runtime.classic_time_remaining());
    let prefs = app.preferences();
    assert!(prefs.contains(
        "\"id\":\"vis.peaks\",\"kind\":\"toggle\",\"label\":\"Analyzer peaks\",\"value\":\"0\""
    ));
    assert!(prefs.contains("\"label\":\"Show (classic skins)\",\"value\":\"remaining\""));

    // an action asks the shell
    app.take_requests();
    app.set_preference("settings.reveal", "");
    assert_eq!(app.take_requests(), [Request::Reveal(dir.clone())]);
    app.set_preference("plugins.reveal", "");
    assert_eq!(app.take_requests(), [Request::Reveal(dir.join("Plugins"))]);
    assert!(dir.join("Plugins").is_dir());
    app.set_preference("skin.open", "");
    assert_eq!(app.take_requests(), [Request::Ask(Ask::Skin)]);
    app.command(command::PREFERENCES);
    assert_eq!(app.take_requests(), [Request::Preferences]);

    // what the listener sets on the skin itself is kept too: a click on the visualizer
    // goes to its next mode
    app.skin()
        .unwrap()
        .runtime
        .set_classic_vis_mode(VisMode::Off);
    app.save();
    drop(app);
    let mut again = start("", &dir);
    assert_eq!(again.vis_mode(), VisMode::Off);
    assert_eq!(
        again.skin().unwrap().runtime.classic_vis_mode(),
        VisMode::Off
    );
    assert!(again.time_remaining());
    assert!(again.player.borrow().shuffle() && again.player.borrow().repeat());
    assert_eq!((again.vis_stay(), again.vis_fps()), (45, 60));
    assert!(again
        .preferences()
        .contains("\"id\":\"vis.mode\",\"kind\":\"choice\",\"label\":\"Shows\",\"value\":\"off\""));
}
