// SPDX-License-Identifier: GPL-3.0-or-later

//! What goes between the app and its shell: the commands' numbers, the menus, what the
//! listener is asked to pick, and what the app asks the shell to do.

use std::path::PathBuf;

/// The numbers of the app's commands, as a menu item carries them.
pub mod command {
    /// Let the listener pick songs to play in place of the playlist.
    pub const OPEN_FILES: i32 = 1;
    /// Let the listener pick songs to add to the playlist.
    pub const ADD_FILES: i32 = 2;
    /// Ask for the address of a radio stream to add.
    pub const ADD_URL: i32 = 3;
    /// Let the listener pick a skin to wear.
    pub const OPEN_SKIN: i32 = 4;
    /// Let the listener pick folders of songs to add.
    pub const ADD_FOLDER: i32 = 5;
    /// Let the listener pick a playlist file to play.
    pub const LOAD_LIST: i32 = 6;
    /// Let the listener say where to write the playlist.
    pub const SAVE_LIST: i32 = 7;
    /// The song before.
    pub const PREVIOUS: i32 = 10;
    /// Play.
    pub const PLAY: i32 = 11;
    /// Pause, or go on.
    pub const PAUSE: i32 = 12;
    /// Stop.
    pub const STOP: i32 = 13;
    /// The next song.
    pub const NEXT: i32 = 14;
    /// Double size, or back to single.
    pub const DOUBLE_SIZE: i32 = 20;
    /// Stay above other apps' windows, or not.
    pub const ON_TOP: i32 = 21;
    /// Show the Effects window.
    pub const EFFECTS: i32 = 22;
    /// Open the Visualizer window, or close it.
    pub const VISUALIZER: i32 = 23;
    /// Let the listener pick the folder of Milkdrop presets.
    pub const CHOOSE_PRESETS: i32 = 24;
    /// Show who made the app.
    pub const ABOUT: i32 = 25;
    /// End the app.
    pub const QUIT: i32 = 26;
    /// Take the app's windows out of sight.
    pub const HIDE: i32 = 27;
    /// Show the Preferences window.
    pub const PREFERENCES: i32 = 28;
    /// Pick the next song at random, or not.
    pub const SHUFFLE: i32 = 30;
    /// Start the list over after its last song, or not.
    pub const REPEAT: i32 = 31;
    /// Show the time left of the song.
    pub const TIME_REMAINING: i32 = 32;
    /// Show the time played of the song.
    pub const TIME_ELAPSED: i32 = 33;
    /// Take the selected entries out of the playlist.
    pub const REMOVE_SELECTED: i32 = 40;
    /// Keep only the selected entries.
    pub const CROP: i32 = 41;
    /// Empty the playlist.
    pub const CLEAR: i32 = 42;
    /// Take out the entries whose file is gone.
    pub const REMOVE_DEAD: i32 = 43;
    /// Take out the entries that name a file an earlier entry names.
    pub const REMOVE_DUPLICATES: i32 = 44;
    /// Select every entry.
    pub const SELECT_ALL: i32 = 45;
    /// Select no entry.
    pub const SELECT_NONE: i32 = 46;
    /// Select what is not selected, and the other way round.
    pub const SELECT_INVERT: i32 = 47;
    /// Sort the playlist by title.
    pub const SORT_TITLE: i32 = 48;
    /// Sort the playlist by file name.
    pub const SORT_FILENAME: i32 = 49;
    /// Sort the playlist by path and file name.
    pub const SORT_PATH: i32 = 50;
    /// Turn the playlist round.
    pub const REVERSE: i32 = 51;
    /// Put the playlist in a random order.
    pub const RANDOMIZE: i32 = 52;
    /// Show what is known of a file: the first selected entry's, or the current song's.
    pub const FILE_INFO: i32 = 53;
    /// Let the listener say where to write the playlist as a web page.
    pub const HTML_LIST: i32 = 54;
    /// Read the selected files again: their tags and length as they are now.
    pub const REREAD: i32 = 55;
    /// Play the first selected entry.
    pub const PLAY_SELECTED: i32 = 56;
    /// Show the first selected entry's file in its folder.
    pub const REVEAL: i32 = 57;
    /// The classic visualizer as a spectrum analyzer.
    pub const VIS_ANALYZER: i32 = 60;
    /// The classic visualizer as an oscilloscope.
    pub const VIS_SCOPE: i32 = 61;
    /// The classic visualizer off.
    pub const VIS_OFF: i32 = 62;
    /// Opacity 100 %, and one more for every 10 % less, down to 40 %.
    pub const OPACITY: i32 = 100;
    /// The first skin the menu offers, and one more for each after it: those that come with
    /// the app by name, then the listener's own by name.
    pub const SKIN: i32 = 200;
    /// How many skins have a number.
    pub const SKINS: usize = 100;
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
    /// The options: preferences, skins, the time shown, the size, shuffle and repeat.
    Options = 3,
    /// What the visualizer shows.
    Visualization = 4,
    /// A playlist's Add button.
    PlaylistAdd = 5,
    /// A playlist's Remove button.
    PlaylistRemove = 6,
    /// A playlist's Select button.
    PlaylistSelect = 7,
    /// A playlist's Misc button.
    PlaylistMisc = 8,
    /// A playlist's List button.
    PlaylistList = 9,
    /// A right click on a row of the playlist.
    PlaylistRow = 10,
    /// The ways to sort the playlist.
    Sort = 11,
    /// The playlist's less common removals: dead files and duplicates.
    RemoveMisc = 12,
    /// The playlist's other options: as a web page, and reading files again.
    MiscOptions = 13,
}

impl Menu {
    /// Every menu, in the order of their numbers.
    pub const ALL: [Menu; 14] = [
        Menu::Bar,
        Menu::Popup,
        Menu::Equalizer,
        Menu::Options,
        Menu::Visualization,
        Menu::PlaylistAdd,
        Menu::PlaylistRemove,
        Menu::PlaylistSelect,
        Menu::PlaylistMisc,
        Menu::PlaylistList,
        Menu::PlaylistRow,
        Menu::Sort,
        Menu::RemoveMisc,
        Menu::MiscOptions,
    ];

    /// The menu with the number `kind`, as the C interface names it.
    pub fn from_number(kind: i32) -> Option<Menu> {
        Menu::ALL.into_iter().find(|m| *m as i32 == kind)
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
    /// Where to write the playlist as a web page.
    SaveHtml,
}

impl Ask {
    const ALL: [(Ask, &'static str); 9] = [
        (Ask::OpenSongs, "open_songs"),
        (Ask::AddSongs, "add_songs"),
        (Ask::AddFolders, "add_folders"),
        (Ask::LoadList, "load_list"),
        (Ask::SaveList, "save_list"),
        (Ask::Skin, "skin"),
        (Ask::Presets, "presets"),
        (Ask::Url, "url"),
        (Ask::SaveHtml, "save_html"),
    ];

    /// The word the C interface names it by.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(ask, _)| *ask == self)
            .map_or("", |(_, name)| name)
    }

    /// What the word `name` asks for, as [`Ask::name`] gives it.
    pub fn from_name(name: &str) -> Option<Ask> {
        Self::ALL
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(ask, _)| *ask)
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
    /// Let the listener pick; the answer goes to [`App::picked`](super::App::picked).
    Ask(Ask),
    /// Show one of the app's menus as a popup at a place on the screen, or at the mouse
    /// without one; the pick goes to [`App::command`](super::App::command).
    Popup {
        /// Which menu.
        menu: Menu,
        /// Where its top left corner goes, in points; `None` for at the mouse.
        at: Option<(i32, i32)>,
    },
    /// Another skin is worn: the windows of the one before are gone.
    Skin,
    /// The size, the opacity or staying on top changed: every window is to be shown again.
    Look,
    /// The menu bar changed.
    Menu,
    /// Show the Effects window.
    Effects,
    /// Show the Preferences window ([`App::preferences`](super::App::preferences)).
    Preferences,
    /// Show what is known of the playlist's entry at this row
    /// ([`App::file_info`](super::App::file_info)).
    Info(usize),
    /// Open this file with whatever the system opens it with.
    Open(PathBuf),
    /// Show this file in its folder.
    Reveal(PathBuf),
    /// Open the Visualizer window, or close it; [`App::vis_shown`](super::App::vis_shown) says when it is done.
    Vis(bool),
    /// Show this preset in the Visualizer window, blending into it or cutting to it, with
    /// this title on the window.
    Preset {
        /// The preset's file.
        file: PathBuf,
        /// The window's title while it shows.
        title: String,
        /// Whether to blend into it; false cuts to it.
        blend: bool,
    },
    /// Tell the listener something went wrong.
    Error(String),
}

impl Request {
    /// The request as a line of text, its parts with tabs between, for a shell behind the C
    /// interface: `quit`, `hide`, `about`, `ask` and what, `popup` with the menu's number and
    /// the place when there is one, `skin`, `look`, `menu`, `effects`, `preferences`, `info` with the playlist's row, `open`
    /// and `reveal` with a file, `vis` with 1 or 0,
    /// `preset` with 1 for blending, the title and the file, `error` and the message.
    pub fn line(&self) -> String {
        let plain = |text: &str| text.replace(['\t', '\n'], " ");
        match self {
            Request::Quit => "quit".into(),
            Request::Hide => "hide".into(),
            Request::About => "about".into(),
            Request::Ask(ask) => format!("ask\t{}", ask.name()),
            Request::Popup {
                menu,
                at: Some((x, y)),
            } => format!("popup\t{}\t{x}\t{y}", *menu as i32),
            Request::Popup { menu, at: None } => format!("popup\t{}", *menu as i32),
            Request::Skin => "skin".into(),
            Request::Look => "look".into(),
            Request::Menu => "menu".into(),
            Request::Effects => "effects".into(),
            Request::Preferences => "preferences".into(),
            Request::Info(row) => format!("info\t{row}"),
            Request::Open(file) => format!("open\t{}", file.to_string_lossy()),
            Request::Reveal(file) => format!("reveal\t{}", file.to_string_lossy()),
            Request::Vis(open) => format!("vis\t{}", u8::from(*open)),
            Request::Preset { file, title, blend } => {
                format!(
                    "preset\t{}\t{}\t{}",
                    u8::from(*blend),
                    plain(title),
                    file.to_string_lossy()
                )
            }
            Request::Error(message) => format!("error\t{}", plain(message)),
        }
    }
}
