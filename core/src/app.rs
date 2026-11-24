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

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use walkit::player::Status;

use crate::host::HostAction;
use crate::player::{location_kind, LocationKind, Player, SkinPlayer, EQ_PRESETS, REQUEST_OPEN_FILES};
use crate::settings::{self, Settings};
use crate::skin::{self, Skin};
use crate::vis::{self, Presets};

/// The skin shown when none is asked for, which also lies under every classic skin.
const DEFAULT_SKIN: &str = "AndAmp Dark.wsz";
/// The skin `--modern` shows.
const MODERN_SKIN: &str = "walkit-modern.wal";

/// The numbers of the app's commands, as a menu item carries them.
pub mod command {
    pub const OPEN_FILES: i32 = 1;
    pub const ADD_FILES: i32 = 2;
    pub const ADD_URL: i32 = 3;
    pub const OPEN_SKIN: i32 = 4;
    pub const ADD_FOLDER: i32 = 5;
    pub const LOAD_LIST: i32 = 6;
    pub const SAVE_LIST: i32 = 7;
    pub const PREVIOUS: i32 = 10;
    pub const PLAY: i32 = 11;
    pub const PAUSE: i32 = 12;
    pub const STOP: i32 = 13;
    pub const NEXT: i32 = 14;
    pub const DOUBLE_SIZE: i32 = 20;
    pub const ON_TOP: i32 = 21;
    pub const EFFECTS: i32 = 22;
    pub const VISUALIZER: i32 = 23;
    pub const CHOOSE_PRESETS: i32 = 24;
    pub const ABOUT: i32 = 25;
    pub const QUIT: i32 = 26;
    pub const HIDE: i32 = 27;
    /// Opacity 100 %, and one more for every 10 % less, down to 40 %.
    pub const OPACITY: i32 = 100;
    /// The first skin that comes with the app, and one more for each after it, by name.
    pub const SKIN: i32 = 200;
    /// The equalizer set flat, and one more for each factory preset.
    pub const EQ_PRESET: i32 = 300;
}

/// One of the app's menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    /// The menu bar: its top level is the bar's menus.
    Bar = 0,
    /// The same commands as one popup list, for the skin's menu button.
    Popup = 1,
    /// The equalizer's factory presets, for the equalizer window's Presets button.
    Equalizer = 2,
}

impl Menu {
    pub fn from_number(kind: i32) -> Option<Menu> {
        [Menu::Bar, Menu::Popup, Menu::Equalizer].into_iter().find(|m| *m as i32 == kind)
    }
}

/// Something the listener is asked to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Songs and folders of songs, to play in place of the playlist.
    OpenSongs,
    /// Songs and folders of songs, to add to the playlist.
    AddSongs,
    /// Folders of songs, to add to the playlist.
    AddFolders,
    /// A playlist file, to play.
    LoadList,
    /// Where to write the playlist.
    SaveList,
    /// A skin, to wear.
    Skin,
    /// A folder of Milkdrop presets.
    Presets,
    /// The address of a radio stream, as text.
    Url,
}

impl Ask {
    const ALL: [(Ask, &'static str); 8] = [
        (Ask::OpenSongs, "open_songs"),
        (Ask::AddSongs, "add_songs"),
        (Ask::AddFolders, "add_folders"),
        (Ask::LoadList, "load_list"),
        (Ask::SaveList, "save_list"),
        (Ask::Skin, "skin"),
        (Ask::Presets, "presets"),
        (Ask::Url, "url"),
    ];

    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(ask, _)| *ask == self).map_or("", |(_, name)| name)
    }

    pub fn from_name(name: &str) -> Option<Ask> {
        Self::ALL.iter().find(|(_, n)| *n == name).map(|(ask, _)| *ask)
    }
}

/// What the app asks of its shell: the things only the system can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// End the app.
    Quit,
    /// Take the app's windows out of sight.
    Hide,
    /// Show who made the app.
    About,
    /// Let the listener pick; the answer goes to [`App::picked`].
    Ask(Ask),
    /// Show one of the app's menus as a popup at a place on the screen, or at the mouse
    /// without one; the pick goes to [`App::command`].
    Popup { menu: Menu, at: Option<(i32, i32)> },
    /// Another skin is worn: the windows of the one before are gone.
    Skin,
    /// The size, the opacity or staying on top changed: every window is to be shown again.
    Look,
    /// The menu bar changed.
    Menu,
    /// Show the Effects window.
    Effects,
    /// Open the Visualizer window, or close it; [`App::vis_shown`] says when it is done.
    Vis(bool),
    /// Show this preset in the Visualizer window, blending into it or cutting to it, with
    /// this title on the window.
    Preset { file: PathBuf, title: String, blend: bool },
    /// Tell the listener something went wrong.
    Error(String),
}

impl Request {
    /// The request as a line of text, its parts with tabs between, for a shell behind the C
    /// interface: `quit`, `hide`, `about`, `ask` and what, `popup` with the menu's number and
    /// the place when there is one, `skin`, `look`, `menu`, `effects`, `vis` with 1 or 0,
    /// `preset` with 1 for blending, the title and the file, `error` and the message.
    pub fn line(&self) -> String {
        let plain = |text: &str| text.replace(['\t', '\n'], " ");
        match self {
            Request::Quit => "quit".into(),
            Request::Hide => "hide".into(),
            Request::About => "about".into(),
            Request::Ask(ask) => format!("ask\t{}", ask.name()),
            Request::Popup { menu, at: Some((x, y)) } => format!("popup\t{}\t{x}\t{y}", *menu as i32),
            Request::Popup { menu, at: None } => format!("popup\t{}", *menu as i32),
            Request::Skin => "skin".into(),
            Request::Look => "look".into(),
            Request::Menu => "menu".into(),
            Request::Effects => "effects".into(),
            Request::Vis(open) => format!("vis\t{}", u8::from(*open)),
            Request::Preset { file, title, blend } => {
                format!("preset\t{}\t{}\t{}", u8::from(*blend), plain(title), file.to_string_lossy())
            }
            Request::Error(message) => format!("error\t{}", plain(message)),
        }
    }
}

/// What the command line asks for.
///
/// ```text
/// andamp [skin.wsz|skin.wal] [song ...] [--modern] [--zoom n] [--volume 0..255]
///        [--effect <id>]       switches on the effect with that id or name; may be given more
///                              than once
///        [--show-vis]          opens the Visualizer window
///        [--presets <folder>]  the Milkdrop presets it shows
///        [--show-effects]      opens the Effects window
///        [--base skin.wsz]     a classic skin laid under classic skins, in place of AndAmp Dark
///        [--wasabi base.zip]   a zip of a wasabi folder laid under modern skins, in place of
///                              Andamp's own element set
/// ```
///
/// Songs named on the command line play at once. The rest is for checking and measuring, and
/// the shell does it: `--click x,y` (may be given more than once), `--quit-after <seconds>`,
/// `--dump <folder>`, `--bench <seconds>`, `--then-skin <file>` and `--hidden`.
#[derive(Debug, Default, Clone)]
pub struct Options {
    pub skin: Option<PathBuf>,
    /// Songs, folders, playlist files and radio streams.
    pub songs: Vec<String>,
    pub volume: Option<i32>,
    pub modern: bool,
    pub zoom: Option<i32>,
    pub base: Option<PathBuf>,
    pub wasabi: Option<PathBuf>,
    pub effects: Vec<String>,
    pub show_effects: bool,
    pub show_vis: bool,
    pub presets: Option<PathBuf>,
    pub dump: Option<String>,
    pub bench: Option<String>,
    pub quit_after: Option<String>,
    pub hidden: bool,
    pub then_skin: Option<String>,
    pub clicks: Vec<String>,
}

impl Options {
    /// Reads the arguments after the program's name.
    pub fn parse(args: &[String]) -> Options {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let mut value = || args.next().cloned();
            match arg.as_str() {
                "--modern" => options.modern = true,
                "--zoom" => options.zoom = value().and_then(|v| v.parse().ok()),
                "--volume" => options.volume = value().and_then(|v| v.parse().ok()),
                "--dump" => options.dump = value(),
                "--show-effects" => options.show_effects = true,
                "--show-vis" => options.show_vis = true,
                "--presets" => options.presets = value().map(|v| whole(&v)),
                "--effect" => options.effects.extend(value()),
                "--base" => options.base = value().map(|v| whole(&v)),
                "--wasabi" => options.wasabi = value().map(|v| whole(&v)),
                "--click" => options.clicks.extend(value()),
                "--bench" => options.bench = value(),
                "--hidden" => options.hidden = true,
                "--then-skin" => options.then_skin = value(),
                "--quit-after" => options.quit_after = value(),
                // unknown, with its value: what a system adds when it starts the app, such as
                // -NSDocumentRevisionsDebugMode YES
                other if other.starts_with('-') && !other.starts_with("--") => {
                    value();
                }
                other if other.starts_with("--") => {}
                location if is_url(location) => options.songs.push(location.to_string()),
                location => {
                    // from the folder the app was started in: kept for the next run, which
                    // may start somewhere else
                    let file = whole(location);
                    if location_kind(&file) == LocationKind::Skin {
                        options.skin = Some(file);
                    } else {
                        options.songs.push(file.to_string_lossy().into_owned());
                    }
                }
            }
        }
        options
    }

    /// What the command line gave for `name`, as text: a flag's value, `1` for a switch that
    /// is on, and for `click` a line for each click. `None` for what was not given.
    pub fn get(&self, name: &str) -> Option<String> {
        let on = |flag: bool| flag.then(|| "1".to_string());
        let path = |path: &Option<PathBuf>| path.as_ref().map(|p| p.to_string_lossy().into_owned());
        match name {
            "dump" => self.dump.clone(),
            "bench" => self.bench.clone(),
            "quit-after" => self.quit_after.clone(),
            "then-skin" => self.then_skin.clone(),
            "hidden" => on(self.hidden),
            "modern" => on(self.modern),
            "show-vis" => on(self.show_vis),
            "show-effects" => on(self.show_effects),
            "presets" => path(&self.presets),
            "click" => (!self.clicks.is_empty()).then(|| self.clicks.join("\n")),
            _ => None,
        }
    }

    /// Whether this run reads the settings and leaves them: not one that only looks or
    /// measures, which starts from nothing.
    fn kept(&self) -> bool {
        self.dump.is_none() && self.bench.is_none() && self.clicks.is_empty()
    }
}

/// Shows a menu given as text and gives the number picked, 0 for none.
pub type MenuHandler = Rc<dyn Fn(&str) -> i32>;

/// The app.
pub struct App {
    /// The player every skin shows and drives.
    pub player: Rc<RefCell<Player>>,
    pub settings: Settings,
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
}

impl App {
    /// The app as the command line `args` (without the program's name) and the settings of
    /// the last run make it, with what comes with it in the folder `resources`, on a screen
    /// of `screen` points. Nothing shows until [`App::start`].
    pub fn new(args: &[String], resources: &Path, screen: (i32, i32)) -> App {
        App::with(Options::parse(args), resources, screen, settings::config_dir(), Player::new())
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
        let zoom = options.zoom.or(number("zoom").map(|z| z as i32)).unwrap_or(1).max(1);
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
        let left_on = app
            .settings
            .get("effects")
            .is_some_and(|state| state.lines().any(|line| line.split('\t').nth(1) == Some("1")));
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
        let builtin = self.builtin(if self.options.modern { MODERN_SKIN } else { DEFAULT_SKIN });
        let last = || self.settings.get("skin").map(PathBuf::from).filter(|file| file.exists());
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
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.retain(|name| !name.starts_with('.') && location_kind(Path::new(name)) == LocationKind::Skin);
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

    // the skin

    /// The skin worn.
    pub fn skin(&mut self) -> Option<&mut Skin> {
        self.skin.as_deref_mut()
    }

    /// The file name of the skin worn.
    pub fn skin_name(&self) -> String {
        self.worn
            .as_ref()
            .and_then(|file| file.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The screen in layout pixels.
    fn viewport(&self) -> (i32, i32) {
        (self.screen.0 / self.zoom, self.screen.1 / self.zoom)
    }

    /// A place on the screen in points as one in layout pixels, kept on the screen.
    fn on_screen(&self, at: (i32, i32)) -> (i32, i32) {
        let (w, h) = self.viewport();
        let pixel = |points: i32| (f64::from(points) / f64::from(self.zoom)).round() as i32;
        (pixel(at.0).min(w - 40).max(0), pixel(at.1).min(h - 20).max(0))
    }

    /// Every window's top left corner on the screen in points, by the engine's id, as the
    /// last step left them.
    pub fn places(&self) -> Vec<(u32, (i32, i32))> {
        let windows = self.skin.as_ref().map_or(&[][..], |skin| skin.windows());
        windows
            .iter()
            .map(|w| (w.id, (w.rect.left * self.zoom, w.rect.top * self.zoom)))
            .collect()
    }

    /// The main window's corner on the screen, in points.
    fn main_place(&mut self) -> Option<(i32, i32)> {
        let zoom = self.zoom;
        let skin = self.skin.as_mut()?;
        let main = skin.runtime.main_window()?;
        let rect = skin.runtime.window_rect(main);
        Some((rect.left * zoom, rect.top * zoom))
    }

    /// Writes down where the windows of the skin worn are, for when it is worn again.
    fn keep_places(&mut self) {
        let places = self.places();
        if places.is_empty() {
            return;
        }
        let prefix = format!("at.{}.", self.skin_name());
        self.settings.clear(&prefix);
        for (id, (x, y)) in places {
            self.settings.set(&format!("{prefix}{id}"), Some(&format!("{x},{y}")));
        }
    }

    /// Wears the skin at `file` in place of the one worn, with its windows where they were
    /// when it was last worn, or else with its main window where the last skin's was. When it
    /// does not open, the skin worn stays and the shell is asked to say why.
    pub fn wear(&mut self, file: &Path) -> bool {
        match self.open_skin(file) {
            Ok(()) => true,
            Err(message) => {
                self.request(Request::Error(message));
                false
            }
        }
    }

    fn open_skin(&mut self, file: &Path) -> Result<(), String> {
        let bytes = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // under a classic skin lies AndAmp Dark, which gives the pictures a skin leaves out;
        // under a modern one Andamp's element set; under both the stand-in fonts
        let base = self.options.base.clone().unwrap_or_else(|| self.builtin(DEFAULT_SKIN));
        let base = std::fs::read(base).ok().map(skin::unzip).transpose()?;
        let wasabi = match &self.options.wasabi {
            Some(zip) => std::fs::read(zip).ok().map(skin::unzip).transpose()?,
            None => Some(skin::read_folder(&self.resources.join("wasabi"))),
        };
        let fonts = skin::read_fonts(&self.resources.join("fonts"));
        let player = self.player.clone();
        let mut skin = Skin::open(bytes, &name, base, wasabi, fonts, file.parent(), self.viewport(), |_| {
            Box::new(SkinPlayer(player))
        })?;
        skin.player = Some(self.player.clone());
        // the skin worn goes: its places are kept first
        if self.skin.is_some() {
            self.keep_places();
            self.main_corner = self.main_place();
        }
        self.skin = Some(Box::new(skin));
        self.worn = Some(file.to_path_buf());
        self.give_menu_handler();
        self.tell_switches();

        let prefix = format!("at.{name}.");
        let kept: Vec<(u32, (i32, i32))> = self
            .settings
            .keys(&prefix)
            .into_iter()
            .filter_map(|key| {
                let id = key[prefix.len()..].parse().ok()?;
                let (x, y) = self.settings.get(key)?.split_once(',')?;
                Some((id, (x.parse().ok()?, y.parse().ok()?)))
            })
            .collect();
        let zoom = self.zoom;
        let main = self.skin.as_ref().and_then(|skin| skin.runtime.main_window());
        let corner = main
            .and_then(|main| kept.iter().find(|(id, _)| *id == main.0))
            .map(|(_, at)| *at)
            .or(self.main_corner.take());
        let corner = corner.map(|at| self.on_screen(at));
        let others: Vec<(u32, (i32, i32))> = kept.iter().map(|(id, at)| (*id, self.on_screen(*at))).collect();
        if let Some(skin) = self.skin.as_mut() {
            match (main, corner) {
                (Some(main), Some((x, y))) => {
                    // the main window goes where it was, and the others come along as they
                    // are docked; then every window that had a place of its own gets it back
                    let rect = skin.runtime.window_rect(main);
                    skin.move_windows(x - rect.left, y - rect.top);
                    for (id, (x, y)) in others {
                        if id != main.0 {
                            skin.place_window(id, x, y);
                        }
                    }
                }
                // a little in from the screen's top left corner
                _ => skin.move_windows(120 / zoom, 80 / zoom),
            }
        }
        self.request(Request::Skin);
        self.request(Request::Menu);
        Ok(())
    }

    /// Sets how the skin's own popup menus are shown, for this skin and every one after it.
    /// The handler is called from inside a mouse call and must not call the app.
    pub fn set_menu_handler(&mut self, handler: Option<MenuHandler>) {
        self.menu_handler = handler;
        self.give_menu_handler();
    }

    fn give_menu_handler(&mut self) {
        let handler = self.menu_handler.clone();
        if let Some(skin) = self.skin.as_mut() {
            skin.set_menu_handler(handler.map(|show| -> crate::host::MenuHandler { Box::new(move |menu| show(menu)) }));
        }
    }

    /// Tells the skin which of the app's switches are on, so its buttons show them.
    fn tell_switches(&mut self) {
        let (double, on_top, vis) = (self.zoom > 1, self.on_top, self.vis_open);
        if let Some(skin) = self.skin.as_mut() {
            skin.set_switch("doublesize", double);
            skin.set_switch("ontop", on_top);
            skin.set_switch("vis", vis);
        }
    }

    /// Runs the skin and answers what its buttons asked for. Says what changed, as
    /// [`Skin::step`] does.
    pub fn step(&mut self) -> u32 {
        if self.playlist_due == 1 {
            self.playlist_due = 0;
            self.put_back_playlist();
        }
        let flags = self.skin.as_mut().map_or(0, |skin| skin.step());
        if self.playlist_due == 2 {
            self.playlist_due = 1;
        }
        if self.player.borrow_mut().take_requests() & REQUEST_OPEN_FILES != 0 {
            self.request(Request::Ask(Ask::OpenSongs));
        }
        let actions = self.skin.as_mut().map(|skin| skin.take_actions()).unwrap_or_default();
        for action in actions {
            self.act(&action);
        }
        flags
    }

    /// Milliseconds until the app next has something to do by itself, and so until the next
    /// [`App::step`]: 0 for now, -1 for never. It is the skin's own clock, and now while the
    /// playlist waits to be put back.
    pub fn next_due_ms(&mut self) -> i64 {
        if self.playlist_due > 0 {
            return 0;
        }
        self.skin.as_mut().map_or(-1, |skin| skin.next_due_ms())
    }

    /// Answers something a button of the skin asked of the app.
    fn act(&mut self, action: &HostAction) {
        let at = action.at.map(|(x, y)| (x * self.zoom, y * self.zoom));
        match (action.name.as_str(), action.param.as_deref()) {
            ("CLOSE", _) => self.request(Request::Quit),
            ("MINIMIZE", _) => self.request(Request::Hide),
            ("DOUBLESIZE", _) => self.command(command::DOUBLE_SIZE),
            ("ALWAYS_ON_TOP", _) => self.command(command::ON_TOP),
            ("MENU" | "SYSMENU" | "CONTROLMENU", _) => self.request(Request::Popup { menu: Menu::Popup, at }),
            ("ABOUT", _) => self.request(Request::About),
            ("TOGGLE" | "SHOW", Some("vis")) => self.command(command::VISUALIZER),
            ("EQ_PRESETS", _) => self.request(Request::Popup { menu: Menu::Equalizer, at }),
            ("PE_ADD", _) | ("PLAYLIST", Some("add_file")) => self.request(Request::Ask(Ask::AddSongs)),
            ("PLAYLIST", Some("add_url")) => self.request(Request::Ask(Ask::Url)),
            ("PLAYLIST", Some("add_dir")) => self.request(Request::Ask(Ask::AddFolders)),
            ("PLAYLIST", Some("list_load")) => self.request(Request::Ask(Ask::LoadList)),
            ("PLAYLIST", Some("list_save")) => self.request(Request::Ask(Ask::SaveList)),
            _ => {}
        }
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

    pub fn set_on_top(&mut self, on_top: bool) {
        self.on_top = on_top;
        self.tell_switches();
        self.request(Request::Look);
        self.request(Request::Menu);
    }

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

    // the menu and its commands

    /// One of the app's menus as lines of text, for the shell to build its own from: `C`, the
    /// command's number, 1 when checked, 1 when disabled, the text and the key, with tabs
    /// between, for a command; `S` for a separator; `>` and the text where a submenu starts
    /// and `<` where it ends. A key is a letter, with `shift+` before it when Shift is held,
    /// and goes with the system's own command key; most items have none.
    pub fn menu(&self, menu: Menu) -> String {
        let mut out = String::new();
        let item = |out: &mut String, id: i32, text: &str, key: &str, checked: bool| {
            out.push_str(&format!("C\t{id}\t{}\t0\t{text}\t{key}\n", u8::from(checked)));
        };
        let worn = self.skin_name();
        let skins = |out: &mut String| {
            for (n, file) in self.builtin_skins().iter().enumerate() {
                let name = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned());
                let id = command::SKIN + n as i32;
                out.push_str(&format!(
                    "C\t{id}\t{}\t0\t{}\t\n",
                    u8::from(*file == worn),
                    name.unwrap_or_default()
                ));
            }
        };
        let opacity = |out: &mut String| {
            out.push_str(">\tOpacity\n");
            for step in 0..=6 {
                let percent = 100 - step * 10;
                out.push_str(&format!(
                    "C\t{}\t{}\t0\t{percent} %\t\n",
                    command::OPACITY + step,
                    u8::from(self.opacity == percent)
                ));
            }
            out.push_str("<\n");
        };
        match menu {
            Menu::Bar => {
                out.push_str(">\tAndamp\n");
                item(&mut out, command::ABOUT, "About Andamp", "", false);
                out.push_str("S\n");
                item(&mut out, command::QUIT, "Quit Andamp", "q", false);
                out.push_str("<\n>\tFile\n");
                item(&mut out, command::OPEN_FILES, "Open Files…", "o", false);
                item(&mut out, command::ADD_FILES, "Add Files…", "l", false);
                item(&mut out, command::ADD_URL, "Add Radio Stream…", "u", false);
                out.push_str("S\n");
                item(&mut out, command::OPEN_SKIN, "Open Skin…", "shift+o", false);
                // Winamp's keys, with the command key: Z X C V B
                out.push_str("<\n>\tPlay\n");
                item(&mut out, command::PREVIOUS, "Previous", "z", false);
                item(&mut out, command::PLAY, "Play", "x", false);
                item(&mut out, command::PAUSE, "Pause", "c", false);
                item(&mut out, command::STOP, "Stop", "v", false);
                item(&mut out, command::NEXT, "Next", "b", false);
                out.push_str("<\n>\tSkins\n");
                skins(&mut out);
                out.push_str("<\n>\tView\n");
                item(&mut out, command::DOUBLE_SIZE, "Double Size", "d", self.zoom > 1);
                item(&mut out, command::ON_TOP, "Always on Top", "a", self.on_top);
                opacity(&mut out);
                out.push_str("S\n");
                item(&mut out, command::EFFECTS, "Effects…", "e", false);
                item(&mut out, command::VISUALIZER, "Visualizer", "shift+k", self.vis_open);
                item(&mut out, command::CHOOSE_PRESETS, "Choose Milkdrop Presets…", "", false);
                out.push_str("<\n");
            }
            Menu::Popup => {
                item(&mut out, command::OPEN_FILES, "Open Files…", "", false);
                item(&mut out, command::ADD_FILES, "Add Files…", "", false);
                item(&mut out, command::ADD_URL, "Add Radio Stream…", "", false);
                out.push_str("S\n");
                item(&mut out, command::OPEN_SKIN, "Open Skin…", "", false);
                out.push_str(">\tSkins\n");
                skins(&mut out);
                out.push_str("<\nS\n");
                item(&mut out, command::DOUBLE_SIZE, "Double Size", "", self.zoom > 1);
                item(&mut out, command::ON_TOP, "Always on Top", "", self.on_top);
                opacity(&mut out);
                item(&mut out, command::EFFECTS, "Effects…", "", false);
                item(&mut out, command::VISUALIZER, "Visualizer", "", self.vis_open);
                out.push_str("S\n");
                item(&mut out, command::QUIT, "Quit Andamp", "", false);
            }
            Menu::Equalizer => {
                item(&mut out, command::EQ_PRESET, "Flat", "", false);
                out.push_str("S\n");
                for (n, (name, _, _)) in EQ_PRESETS.iter().enumerate() {
                    item(&mut out, command::EQ_PRESET + 1 + n as i32, name, "", false);
                }
            }
        }
        out
    }

    /// Does the command with number `id`: what a pick from the menu, or a key that stands
    /// for one, asks for.
    pub fn command(&mut self, id: i32) {
        use command::*;
        let skins = SKIN..SKIN + 100;
        let presets = EQ_PRESET..=EQ_PRESET + EQ_PRESETS.len() as i32;
        match id {
            OPEN_FILES => self.request(Request::Ask(Ask::OpenSongs)),
            ADD_FILES => self.request(Request::Ask(Ask::AddSongs)),
            ADD_URL => self.request(Request::Ask(Ask::Url)),
            OPEN_SKIN => self.request(Request::Ask(Ask::Skin)),
            ADD_FOLDER => self.request(Request::Ask(Ask::AddFolders)),
            LOAD_LIST => self.request(Request::Ask(Ask::LoadList)),
            SAVE_LIST => self.request(Request::Ask(Ask::SaveList)),
            PREVIOUS => self.player.borrow_mut().previous(),
            PLAY => self.player.borrow_mut().play(),
            PAUSE => self.player.borrow_mut().pause(),
            STOP => self.player.borrow_mut().stop(),
            NEXT => self.player.borrow_mut().next(),
            DOUBLE_SIZE => self.set_zoom(if self.zoom == 1 { 2 } else { 1 }),
            ON_TOP => self.set_on_top(!self.on_top),
            EFFECTS => self.request(Request::Effects),
            VISUALIZER => self.request(Request::Vis(!self.vis_open)),
            CHOOSE_PRESETS => self.request(Request::Ask(Ask::Presets)),
            ABOUT => self.request(Request::About),
            QUIT => self.request(Request::Quit),
            HIDE => self.request(Request::Hide),
            id if (OPACITY..=OPACITY + 6).contains(&id) => self.set_opacity(100 - (id - OPACITY) * 10),
            id if skins.contains(&id) => {
                if let Some(name) = self.builtin_skins().get((id - SKIN) as usize) {
                    let file = self.builtin(name);
                    self.wear(&file);
                }
            }
            id if presets.contains(&id) => {
                // the first is flat, the rest the factory presets in order
                let preset = usize::try_from(id - EQ_PRESET - 1).ok();
                self.player.borrow_mut().apply_eq_preset(preset);
            }
            _ => {}
        }
    }

    /// What the listener picked when asked: files and folders by their paths, the address of
    /// a radio stream as text. Nothing picked is no call, or an empty `answer`.
    pub fn picked(&mut self, ask: Ask, answer: &[String]) {
        let Some(first) = answer.first() else {
            return;
        };
        match ask {
            Ask::OpenSongs | Ask::LoadList => self.play(answer),
            Ask::AddSongs | Ask::AddFolders => {
                self.add(answer);
            }
            Ask::SaveList => {
                let saved = self.player.borrow().save_list(Path::new(first));
                if let Err(message) = saved {
                    self.request(Request::Error(message));
                }
            }
            Ask::Skin => {
                self.wear(Path::new(first));
            }
            Ask::Url => {
                let address = first.trim();
                if !is_url(address) {
                    return;
                }
                let was_empty = self.player.borrow().entries().is_empty();
                if self.add(&[address.to_string()]) > 0 && was_empty {
                    self.player.borrow_mut().play_index(0);
                }
            }
            Ask::Presets => {
                self.settings.set("presets", Some(first));
                self.presets.read(Path::new(first));
                if self.vis_open {
                    self.show_preset(false);
                } else {
                    self.request(Request::Vis(true));
                }
            }
        }
    }

    /// What the system asks the app to open, or what was dropped on a window: a skin is
    /// worn; songs, folders of songs, playlist files and radio streams take the playlist's
    /// place and play. Before [`App::start`] it waits for the start.
    pub fn open(&mut self, locations: &[String]) {
        if !self.started || self.playlist_due > 0 {
            self.waiting.extend_from_slice(locations);
            return;
        }
        let is_skin = |location: &String| !is_url(location) && location_kind(Path::new(location)) == LocationKind::Skin;
        if let Some(skin) = locations.iter().find(|l| is_skin(l)) {
            self.wear(Path::new(skin));
        }
        let rest: Vec<String> = locations.iter().filter(|l| !is_skin(l)).cloned().collect();
        if !rest.is_empty() {
            self.play(&rest);
        }
    }

    /// Adds songs, folders of songs, playlist files and radio streams to the playlist; how
    /// many entries joined.
    pub fn add(&mut self, locations: &[String]) -> usize {
        let mut player = self.player.borrow_mut();
        locations.iter().map(|l| player.add_location(Path::new(l))).sum()
    }

    /// Empties the playlist, adds `locations` and plays the first.
    pub fn play(&mut self, locations: &[String]) {
        self.player.borrow_mut().clear();
        if self.add(locations) > 0 {
            self.player.borrow_mut().play_index(0);
        }
    }

    // the Visualizer

    /// The shell opened the Visualizer window, or it closed. When it opens, the presets of
    /// the folder the listener picked are read and the first one shows.
    pub fn vis_shown(&mut self, open: bool) {
        self.vis_open = open;
        self.tell_switches();
        self.request(Request::Menu);
        if open {
            let kept = || self.settings.get("presets").map(PathBuf::from).filter(|folder| folder.exists());
            if let Some(folder) = self.options.presets.clone().or_else(kept) {
                self.presets.read(&folder);
            }
            self.show_preset(false);
        }
    }

    pub fn vis_open(&self) -> bool {
        self.vis_open
    }

    fn show_preset(&mut self, blend: bool) {
        if let Some(file) = self.presets.current() {
            let request = Request::Preset {
                file: file.to_path_buf(),
                title: self.presets.title(),
                blend,
            };
            self.request(request);
        }
    }

    /// A key in the Visualizer window, by its name ([`Presets::key`]). False for a key that
    /// is not one of the window's.
    pub fn vis_key(&mut self, key: &str) -> bool {
        match self.presets.key(key) {
            vis::Key::Changed => self.show_preset(true),
            vis::Key::Taken => {}
            vis::Key::Other => return false,
        }
        true
    }

    /// A preset's stay ([`vis::STAY_SECONDS`]) is over: the next one blends in, unless the
    /// listener holds the one shown.
    pub fn vis_tick(&mut self) {
        if self.presets.pass() {
            self.show_preset(true);
        }
    }

    // the effects

    /// Where the listener's own plug-ins are kept: in the app's folder, beside its settings.
    fn plugins_folder(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|config| config.join("Plugins"))
    }

    /// Loads every plug-in once, those that come with the app and the listener's own, and
    /// puts each back as the listener left it. A file that is not a plug-in is named on the
    /// standard error, with the reason.
    pub fn load_effects(&mut self) {
        if self.effects_loaded {
            return;
        }
        self.effects_loaded = true;
        let mut player = self.player.borrow_mut();
        for (folder, mine) in [(Some(self.resources.join("plugins")), false), (self.plugins_folder(), true)] {
            let Some(folder) = folder else {
                continue;
            };
            for (name, added) in player.rack.add_folder(&folder) {
                match added {
                    Ok(index) if mine => {
                        self.own.insert(player.rack.effects()[index].plugin.info.id.clone());
                    }
                    Ok(_) => {}
                    Err(reason) => eprintln!("andamp: {name}: {reason}"),
                }
            }
        }
        if let Some(left) = self.settings.get("effects") {
            player.rack.restore(left);
        }
    }

    /// Switches on the effect with the id or the name `id`, for the command line.
    fn switch_on(&mut self, id: &str) {
        self.load_effects();
        let mut player = self.player.borrow_mut();
        let found = player
            .rack
            .effects()
            .iter()
            .position(|e| {
                // the whole id, its last word, or the name as the rack shows it
                let info = &e.plugin.info;
                info.id == id || info.id.rsplit('.').next() == Some(id) || info.name.eq_ignore_ascii_case(id)
            });
        match found {
            Some(index) => player.rack.set_on(index, true),
            None => eprintln!("andamp: no effect {id}"),
        }
    }

    /// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder
    /// under its id, so a newer version takes the older one's place. Gives its place in the
    /// rack, or why it is not a plug-in the app can run.
    pub fn add_plugin(&mut self, file: &Path) -> Result<usize, String> {
        self.load_effects();
        let source = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        let (index, id) = {
            let mut player = self.player.borrow_mut();
            let index = player.rack.add(&source)?;
            (index, player.rack.effects()[index].plugin.info.id.clone())
        };
        if let Some(folder) = self.plugins_folder() {
            let _ = std::fs::create_dir_all(&folder);
            let _ = std::fs::write(folder.join(format!("{id}.lua")), source);
        }
        self.own.insert(id);
        self.keep_effects();
        Ok(index)
    }

    /// Whether effect `index` of the rack is a plug-in the listener added.
    pub fn is_own(&self, index: usize) -> bool {
        let player = self.player.borrow();
        player.rack.effects().get(index).is_some_and(|e| self.own.contains(&e.plugin.info.id))
    }

    /// Takes a plug-in the listener added out of the rack and out of the app's folder.
    pub fn remove_plugin(&mut self, index: usize) {
        if !self.is_own(index) {
            return;
        }
        let id = {
            let mut player = self.player.borrow_mut();
            let id = player.rack.effects()[index].plugin.info.id.clone();
            player.rack.remove(index);
            id
        };
        self.own.remove(&id);
        if let Some(folder) = self.plugins_folder() {
            let _ = std::fs::remove_file(folder.join(format!("{id}.lua")));
        }
    }

    /// Writes down which effects are on and where their controls stand.
    fn keep_effects(&mut self) {
        if self.effects_loaded {
            let state = self.player.borrow().rack.state();
            self.settings.set("effects", Some(&state));
        }
    }

    // between runs

    /// Keeps what the next run starts from, and writes the settings file.
    pub fn save(&mut self) {
        let worn = self.worn.as_ref().map(|file| file.to_string_lossy().into_owned());
        self.settings.set("skin", worn.as_deref());
        self.settings.set("zoom", Some(&self.zoom.to_string()));
        self.settings.set("ontop", Some(if self.on_top { "1" } else { "0" }));
        self.settings.set("opacity", Some(&self.opacity.to_string()));
        {
            let player = self.player.borrow();
            let songs: Vec<String> =
                player.entries().iter().map(|e| e.path.to_string_lossy().into_owned()).collect();
            self.settings.set("volume", Some(&player.volume().to_string()));
            self.settings.set("songs", Some(&songs.join("\n")));
            self.settings.set("song", Some(&player.index().to_string()));
            self.settings.set("player", Some(&player.state()));
        }
        self.keep_places();
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
        let mut windows: Vec<String> = self.places().iter().map(|(_, (x, y))| format!("{x},{y}")).collect();
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

/// The path `file` from the root: one given from the folder the app was started in has that
/// folder before it.
fn whole(file: &str) -> PathBuf {
    std::path::absolute(file).unwrap_or_else(|_| PathBuf::from(file))
}

/// Whether `location` is the address of a radio stream.
fn is_url(location: &str) -> bool {
    let lower = location.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}
