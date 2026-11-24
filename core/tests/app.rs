// SPDX-License-Identifier: GPL-3.0-or-later

//! The app behind every shell: what the command line asks for, what a start puts back, the
//! menu and its commands, what the skin's buttons ask of the shell, and the Visualizer's
//! presets. No test opens a window or the audio output.

use std::path::{Path, PathBuf};

use andamp_core::app::{command, App, Ask, Menu, Options, Request};
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
         -NSDocumentRevisionsDebugMode YES --click 10,20 --click 30,40 --bench 5 --show-vis b.flac",
    ));
    // files are kept from the root, so the next run finds them from another folder
    let here = |name: &str| std::env::current_dir().unwrap().join(name);
    assert_eq!(options.skin, Some(here("Some.wsz")));
    let songs = [here("a.mp3"), PathBuf::from("https://radio.example/stream"), here("b.flac")];
    assert_eq!(options.songs, songs.map(|song| song.to_string_lossy().into_owned()));
    assert_eq!((options.zoom, options.volume), (Some(2), Some(0)));
    assert_eq!(options.effects, ["tone", "width"]);
    assert_eq!(options.get("click").as_deref(), Some("10,20\n30,40"));
    assert_eq!(options.get("bench").as_deref(), Some("5"));
    assert_eq!(options.get("show-vis").as_deref(), Some("1"));
    assert_eq!(options.get("hidden"), None);
    assert_eq!(options.get("dump"), None);
}

#[test]
fn a_first_start_wears_the_default_skin_and_tells_the_shell() {
    let config = scratch("first");
    let mut app = App::with(Options::default(), &resources(), (1728, 1000), Some(config), Player::silent());
    assert!(app.skin().is_none());
    app.start();
    assert_eq!(app.skin_name(), "AndAmp Dark.wsz");
    assert_eq!(app.take_requests(), [Request::Skin, Request::Menu]);
    app.step();
    assert_eq!(app.skin().unwrap().windows().len(), 3, "main, equalizer and playlist");
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
        "C\t100\t1\t0\t100 %\t",
        "C\t23\t0\t0\tVisualizer\tshift+k",
    ] {
        assert!(bar.lines().any(|l| l == line), "the bar has {line:?}:\n{bar}");
    }
    // every submenu that starts also ends
    assert_eq!(bar.lines().filter(|l| l.starts_with('>')).count(), bar.lines().filter(|l| *l == "<").count());

    app.command(command::ON_TOP);
    app.command(command::OPACITY + 3);
    app.command(command::SKIN + 1);
    assert_eq!((app.on_top(), app.opacity()), (true, 70));
    assert_eq!(app.skin_name(), "AndAmp Light.wsz");
    let popup = app.menu(Menu::Popup);
    for line in ["C\t21\t1\t0\tAlways on Top\t", "C\t103\t1\t0\t70 %\t", "C\t201\t1\t0\tAndAmp Light\t"] {
        assert!(popup.lines().any(|l| l == line), "the popup has {line:?}:\n{popup}");
    }
    let requests = app.take_requests();
    assert!(requests.contains(&Request::Look) && requests.contains(&Request::Skin));

    let presets = app.menu(Menu::Equalizer);
    assert!(presets.starts_with("C\t300\t0\t0\tFlat\t\nS\n"));
    assert_eq!(presets.lines().count(), 2 + andamp_core::player::EQ_PRESETS.len());
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
    assert!(matches!(app.take_requests().first(), Some(Request::Error(_))));

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
        matches!(asked.as_slice(), [Request::Popup { menu: Menu::Popup, .. }]),
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
    assert_eq!(app.player.borrow().status(), walkit::player::Status::Stopped, "adding does not play");
    app.picked(Ask::OpenSongs, &[b.clone(), a.clone()]);
    assert_eq!(app.player.borrow().entries().len(), 2, "opening takes the playlist's place");
    assert_eq!(app.player.borrow().status(), walkit::player::Status::Playing);
    app.command(command::STOP);
    assert_eq!(app.player.borrow().status(), walkit::player::Status::Stopped);

    app.picked(Ask::Url, &["ftp://not.a.stream/".into()]);
    assert_eq!(app.player.borrow().entries().len(), 2, "not an address of a stream");
    app.picked(Ask::OpenSongs, &[]);
    assert_eq!(app.player.borrow().entries().len(), 2, "nothing picked changes nothing");

    let list = dir.join("list.m3u8").to_string_lossy().into_owned();
    app.picked(Ask::SaveList, std::slice::from_ref(&list));
    app.picked(Ask::LoadList, &[list]);
    assert_eq!(app.player.borrow().entries().len(), 2);
    app.command(command::STOP);

    let light = resources().join("skins/AndAmp Light.wsz").to_string_lossy().into_owned();
    app.picked(Ask::Skin, std::slice::from_ref(&light));
    assert_eq!(app.skin_name(), "AndAmp Light.wsz");
    // what the system opens: the skin is worn and the songs play
    app.open(&[a, resources().join("skins/AndAmp Spot.wsz").to_string_lossy().into_owned()]);
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
    assert_eq!((again.zoom(), again.on_top(), again.opacity()), (2, true, 80));
    assert_eq!(main_corner(&mut again), corner);
    assert_eq!(again.player.borrow().entries().len(), 2);
    assert_eq!(again.player.borrow().index(), 1);
    assert_eq!(again.player.borrow().volume(), 77);
    assert_eq!(again.summary(), summary);

    // a run that only measures starts from nothing and leaves nothing
    let mut bench = start("--bench 5", &dir);
    assert_eq!(bench.skin_name(), "AndAmp Dark.wsz");
    assert_eq!((bench.zoom(), bench.player.borrow().entries().len()), (1, 0));
    bench.command(command::SKIN + 1);
    bench.save();
    assert_eq!(start("", &dir).skin_name(), "AndAmp Spot.wsz");
}

#[test]
fn what_the_system_opens_before_the_start_waits_for_it() {
    let dir = scratch("waiting");
    let a = song(&dir, "a.wav");
    let mut app = App::with(Options::default(), &resources(), (1728, 1000), Some(dir), Player::silent());
    app.open(&[a]);
    assert_eq!(app.player.borrow().entries().len(), 0);
    app.start();
    assert_eq!(app.next_due_ms(), 0);
    app.step();
    assert_eq!(app.player.borrow().entries().len(), 0, "the skin shows before the songs are read");
    assert_eq!(app.next_due_ms(), 0);
    app.step();
    assert_eq!(app.player.borrow().entries().len(), 1);
    assert_eq!(app.player.borrow().status(), walkit::player::Status::Playing);
    app.command(command::STOP);
}

#[test]
fn the_listeners_own_plugins_are_kept_and_can_go() {
    let dir = scratch("plugins");
    let mut app = start("", &dir);
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/plugins/gain.lua");
    let index = app.add_plugin(&file).expect("the plug-in loads");
    assert!(app.is_own(index));
    assert!(!app.is_own(0), "one that comes with the app is not the listener's");
    let id = app.player.borrow().rack.effects()[index].plugin.info.id.clone();
    let kept = dir.join("Plugins").join(format!("{id}.lua"));
    assert!(kept.exists());
    assert!(app.add_plugin(&resources().join("skins/AndAmp Dark.wsz")).is_err());
    app.player.borrow_mut().rack.set_on(index, true);
    app.save();
    drop(app);

    // left on: loaded at the start, and on again
    let mut again = start("", &dir);
    let index = again.player.borrow().rack.effects().iter().position(|e| e.plugin.info.id == id).expect("still there");
    assert!(again.player.borrow().rack.effects()[index].on);
    assert!(again.is_own(index));
    again.remove_plugin(0);
    assert_eq!(again.player.borrow().rack.effects().len(), index + 1, "only the listener's own can go");
    again.remove_plugin(index);
    assert!(!kept.exists());
}

#[test]
fn presets_go_round_by_name() {
    assert_eq!(by_name("preset 9.milk", "preset 10.milk"), std::cmp::Ordering::Less);
    assert_eq!(by_name("Alpha", "beta"), std::cmp::Ordering::Less);
    assert_eq!(by_name("a007", "a7"), std::cmp::Ordering::Less, "the same number: by its spelling");

    let dir = scratch("presets");
    std::fs::create_dir_all(dir.join("more")).unwrap();
    for name in ["b 10.milk", "b 9.milk", "more/c.MILK", "not a preset.txt", "A.milk"] {
        std::fs::write(dir.join(name), "").unwrap();
    }
    let mut presets = Presets::default();
    assert_eq!(presets.key("right"), Key::Taken, "no presets: the key is the window's, nothing changes");
    assert_eq!(presets.title(), "Visualizer");
    presets.read(&dir);
    assert_eq!(presets.len(), 4);
    assert_eq!(presets.title(), "Visualizer: A");
    assert_eq!(presets.key("right"), Key::Changed);
    assert_eq!(presets.title(), "Visualizer: b 9");
    assert_eq!(presets.key("left"), Key::Changed);
    assert_eq!(presets.key("left"), Key::Changed);
    assert_eq!(presets.title(), "Visualizer: c", "round the start to the last");
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
        format!("preset\t1\tVisualizer: b 9\t{}", dir.join("b 9.milk").display())
    );
    assert!(!app.vis_key("x"));
    app.vis_tick();
    assert_eq!(app.take_requests().len(), 1);
    app.command(command::VISUALIZER);
    assert_eq!(app.take_requests(), [Request::Vis(false)]);
}
