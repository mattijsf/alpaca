// SPDX-License-Identifier: GPL-3.0-or-later

//! The app behind every shell: what it starts with, its menu, what each command does, what
//! the skin's buttons ask for, and what it keeps for the next run.
//!
//! A shell is the part written for one system. It shows the skin's windows, hands over the
//! mouse and the keys, and draws the menu this module describes ([`App::menu`]); a pick comes
//! back as a number ([`App::command`]). What only the system can do, the app asks of the shell
//! as a [`Request`]: quit, hide, let the listener pick a file, show a menu at a place. The
//! answer to a pick comes back through [`App::picked`]. Nothing here knows its system.
//!
//! Places on the screen are in points from the top left corner of the part of the screen the
//! windows move on: a layout pixel of the skin is [`App::zoom`] points.

mod commands;
mod effects;
mod menu;
mod options;
mod playlist;
mod prefs;
mod request;
mod visualizer;
mod worn;

pub use options::Options;
pub use request::{command, Ask, Menu, Request};

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use walkit::player::Status;

use crate::player::{location_kind, LocationKind, Player};
use crate::settings::{self, Settings};
use crate::skin::Skin;
use crate::vis::Presets;

/// The skin shown when none is asked for, which also lies under every classic skin.
const DEFAULT_SKIN: &str = "AndAmp Dark.wsz";
/// The skin `--modern` shows.
const MODERN_SKIN: &str = "walkit-modern.wal";

/// Shows a menu given as text and gives the number picked, 0 for none.
pub type MenuHandler = Rc<dyn Fn(&str) -> i32>;

/// The app.
pub struct App {
    /// The player every skin shows and drives.
    pub player: Rc<RefCell<Player>>,
    /// What is kept between runs.
    pub settings: Settings,
    /// What the command line asked for.
    pub options: Options,
    /// The Visualizer's presets.
    pub presets: Presets,
    skin: Option<Box<Skin>>,
    /// The folder of what comes with the app: `skins`, `wasabi`, `fonts` and `plugins`.
    resources: PathBuf,
    /// The app's own folder, where the listener's plug-ins are.
    config: Option<PathBuf>,
    /// The part of the screen the windows move on, in points.
    screen: (i32, i32),
    zoom: i32,
    on_top: bool,
    /// How much of the windows shows over what is behind them, in percent.
    opacity: i32,
    vis_open: bool,
    /// The skin file worn.
    worn: Option<PathBuf>,
    /// Where the main window of the skin before this one was, for the next to open at.
    main_corner: Option<(i32, i32)>,
    requests: Vec<Request>,
    menu_handler: Option<MenuHandler>,
    effects_loaded: bool,
    /// The ids of the plug-ins the listener added, which can be removed again.
    own: BTreeSet<String>,
    started: bool,
    /// What the system asked to open before the app was up.
    waiting: Vec<String>,
    /// 2 from the start until the first step, 1 until the second, which puts the playlist
    /// back; then 0.
    playlist_due: u8,
    /// What is left of the wheel's turns over the playlist, in rows, short of a whole one.
    wheel_rows: f32,
    /// The same for the volume, in its steps.
    wheel_volume: f32,
    /// The first row a classic playlist shows, as far as the app has counted its scrolling.
    list_top: i32,
    /// The song that was current when the playlist was last brought to it.
    followed: Option<usize>,
}

impl App {
    /// The app as the command line `args` (without the program's name) and the settings of
    /// the last run make it, with what comes with it in the folder `resources`, on a screen
    /// of `screen` points. Nothing shows until [`App::start`].
    pub fn new(args: &[String], resources: &Path, screen: (i32, i32)) -> App {
        App::with(
            Options::parse(args),
            resources,
            screen,
            settings::config_dir(),
            Player::new(),
        )
    }

    /// The same with the parts given: the app's own folder `config`, and the player.
    pub fn with(
        options: Options,
        resources: &Path,
        screen: (i32, i32),
        config: Option<PathBuf>,
        player: Player,
    ) -> App {
        let settings = match &config {
            Some(dir) if options.kept() => Settings::open(dir),
            _ => Settings::none(),
        };
        let number = |key: &str| settings.get(key).and_then(|v| v.parse::<f64>().ok());
        let zoom = options
            .zoom
            .or(number("zoom").map(|z| z as i32))
            .unwrap_or(1)
            .max(1);
        // kept as a part of one before it was kept in percent
        let opacity = match number("opacity") {
            Some(part) if part <= 1.0 => (part * 100.0).round() as i32,
            Some(percent) => percent as i32,
            None => 100,
        };
        let mut app = App {
            player: Rc::new(RefCell::new(player)),
            presets: Presets::default(),
            skin: None,
            resources: resources.to_path_buf(),
            config,
            screen: (screen.0.max(1), screen.1.max(1)),
            zoom,
            on_top: settings.get("ontop") == Some("1"),
            opacity: if opacity >= 20 { opacity.min(100) } else { 100 },
            vis_open: false,
            worn: None,
            main_corner: None,
            requests: Vec::new(),
            menu_handler: None,
            effects_loaded: false,
            own: BTreeSet::new(),
            started: false,
            waiting: Vec::new(),
            playlist_due: 0,
            wheel_rows: 0.0,
            wheel_volume: 0.0,
            list_top: 0,
            followed: None,
            settings,
            options,
        };
        {
            let mut player = app.player.borrow_mut();
            let volume = app.options.volume.or(number_of(&app.settings, "volume"));
            if let Some(volume) = volume {
                player.set_volume(volume.clamp(0, 255));
            }
            if let Some(state) = app.settings.get("player") {
                player.restore(state);
            }
        }
        // the plug-ins are loaded at the start only when one was left on, so it is heard at once
        let left_on = app.settings.get("effects").is_some_and(|state| {
            state
                .lines()
                .any(|line| line.split('\t').nth(1) == Some("1"))
        });
        if left_on {
            app.load_effects();
        }
        for id in app.options.effects.clone() {
            app.switch_on(&id);
        }
        app
    }

    /// Wears the first skin. The playlist comes back at the second [`App::step`] after it, so
    /// the skin shows before the songs are read.
    pub fn start(&mut self) {
        let builtin = self.builtin(if self.options.modern {
            MODERN_SKIN
        } else {
            DEFAULT_SKIN
        });
        let last = || {
            self.settings
                .get("skin")
                .map(PathBuf::from)
                .filter(|file| file.exists())
        };
        let skin = self
            .options
            .skin
            .clone()
            .or_else(|| if self.options.modern { None } else { last() })
            .unwrap_or_else(|| builtin.clone());
        if !self.wear(&skin) && skin != builtin {
            // the skin asked for does not open: the app still shows itself
            self.wear(&builtin);
        }
        self.started = true;
        // the songs are read after the first step, so the skin shows before them
        self.playlist_due = 2;
        if self.options.show_vis {
            self.request(Request::Vis(true));
        }
        if self.options.show_effects {
            self.request(Request::Effects);
        }
        if self.options.show_prefs {
            self.request(Request::Preferences);
        }
        self.request(Request::Menu);
    }

    /// Puts the playlist back: the songs the command line names play, else those the system
    /// asked to open, else the playlist is as it was left, at the song it was at, not playing.
    fn put_back_playlist(&mut self) {
        let waiting = std::mem::take(&mut self.waiting);
        if !self.options.songs.is_empty() {
            self.open(&self.options.songs.clone());
        } else if !waiting.is_empty() {
            self.open(&waiting);
        } else {
            let songs = self.settings.get("songs").unwrap_or("").to_string();
            let mut player = self.player.borrow_mut();
            for song in songs.lines() {
                let location = Path::new(song);
                if is_url(song) || location.exists() {
                    player.add_location(location);
                }
            }
            player.select(number_of(&self.settings, "song").unwrap_or(0).max(0) as usize);
        }
    }

    /// A file that comes with the app's skins, by its name.
    fn builtin(&self, name: &str) -> PathBuf {
        self.resources.join("skins").join(name)
    }

    /// The skins that come with the app: their file names, in order.
    pub fn builtin_skins(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.resources.join("skins"))
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.retain(|name| {
            !name.starts_with('.') && location_kind(Path::new(name)) == LocationKind::Skin
        });
        names.sort();
        names
    }

    fn request(&mut self, request: Request) {
        // one is enough of those that only say something changed
        let again = matches!(request, Request::Skin | Request::Look | Request::Menu);
        if !(again && self.requests.contains(&request)) {
            self.requests.push(request);
        }
    }

    /// What the app asked of the shell since the last call, in order.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    // the look

    /// Points to a layout pixel: 2 is Winamp's double size.
    pub fn zoom(&self) -> i32 {
        self.zoom
    }

    /// Whether the windows stay above other apps' windows.
    pub fn on_top(&self) -> bool {
        self.on_top
    }

    /// How much of the windows shows over what is behind them, in percent: 20 to 100.
    pub fn opacity(&self) -> i32 {
        self.opacity
    }

    /// Sets how many points a layout pixel is. The main window's corner stays where it is.
    pub fn set_zoom(&mut self, zoom: i32) {
        if zoom == self.zoom || zoom < 1 {
            return;
        }
        let corner = self.main_place();
        self.zoom = zoom;
        let (w, h) = self.viewport();
        if let Some(skin) = self.skin.as_mut() {
            skin.set_viewport(w, h);
            // the engine first puts its windows where they fit the screen now
            skin.step();
            if let (Some(main), Some((x, y))) = (skin.runtime.main_window(), corner) {
                let rect = skin.runtime.window_rect(main);
                let pixel = |points: i32| (f64::from(points) / f64::from(zoom)).round() as i32;
                skin.move_windows(pixel(x) - rect.left, pixel(y) - rect.top);
            }
        }
        self.tell_switches();
        self.request(Request::Look);
        self.request(Request::Menu);
    }

    /// Keeps the windows above other apps' windows, or not.
    pub fn set_on_top(&mut self, on_top: bool) {
        self.on_top = on_top;
        self.tell_switches();
        self.request(Request::Look);
        self.request(Request::Menu);
    }

    /// Sets how much of the windows shows over what is behind them, in percent: 20 to 100.
    pub fn set_opacity(&mut self, percent: i32) {
        self.opacity = percent.clamp(20, 100);
        self.request(Request::Look);
        self.request(Request::Menu);
    }

    /// The part of the screen the windows move on changed, to `screen` points.
    pub fn set_screen(&mut self, screen: (i32, i32)) {
        self.screen = (screen.0.max(1), screen.1.max(1));
        let (w, h) = self.viewport();
        if let Some(skin) = self.skin.as_mut() {
            skin.set_viewport(w, h);
        }
    }

    // between runs

    /// Keeps what the next run starts from, and writes the settings file.
    pub fn save(&mut self) {
        let worn = self
            .worn
            .as_ref()
            .map(|file| file.to_string_lossy().into_owned());
        self.settings.set("skin", worn.as_deref());
        self.settings.set("zoom", Some(&self.zoom.to_string()));
        self.settings
            .set("ontop", Some(if self.on_top { "1" } else { "0" }));
        self.settings
            .set("opacity", Some(&self.opacity.to_string()));
        {
            let player = self.player.borrow();
            let songs: Vec<String> = player
                .entries()
                .iter()
                .map(|e| e.path.to_string_lossy().into_owned())
                .collect();
            self.settings
                .set("volume", Some(&player.volume().to_string()));
            self.settings.set("songs", Some(&songs.join("\n")));
            self.settings.set("song", Some(&player.index().to_string()));
            self.settings.set("player", Some(&player.state()));
        }
        self.keep_places();
        self.keep_switches();
        self.keep_effects();
        if let Err(message) = self.settings.save() {
            eprintln!("andamp: {message}");
        }
    }

    /// What the app would keep for the next run, in a line: for `--quit-after`.
    pub fn summary(&self) -> String {
        let player = self.player.borrow();
        let status = match player.status() {
            Status::Stopped => 0,
            Status::Playing => 1,
            Status::Paused => 2,
        };
        let mut windows: Vec<String> = self
            .places()
            .iter()
            .map(|(_, (x, y))| format!("{x},{y}"))
            .collect();
        windows.sort();
        format!(
            "skin {} zoom {} on top {} opacity {} volume {} songs {} at {} status {status} position {} ms windows {windows:?}",
            self.skin_name(),
            self.zoom,
            self.on_top,
            self.opacity,
            player.volume(),
            player.entries().len(),
            player.index(),
            player.position_ms(),
        )
    }
}

/// The whole number kept under `key`.
fn number_of(settings: &Settings, key: &str) -> Option<i32> {
    settings.get(key).and_then(|v| v.parse().ok())
}

/// Whether `location` is the address of a radio stream.
fn is_url(location: &str) -> bool {
    let lower = location.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}
