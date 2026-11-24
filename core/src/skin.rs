// SPDX-License-Identifier: GPL-3.0-or-later

//! A loaded skin on the desktop: the engine with its windows set out apart, and for every
//! window its place on the screen and its pixels.
//!
//! The engine's viewport is the screen, in layout pixels. The engine moves, docks and snaps
//! the windows in it; the app puts one of its windows where the engine has each one. A window
//! is drawn again only when the engine hands out a new draw list for it.

use std::cell::{Cell, RefCell};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use walkit::draw::{DrawCmd, Rect};
use walkit::files::{self, LayeredFiles, MemoryFiles, SkinFiles};
use walkit::host::{Clock, SkinSetup, WindowId};
use walkit::player::Player;
use walkit::SkinRuntime;

use crate::host::{Assets, Desk, Host, HostAction, MenuHandler};
use crate::render::{damage, draw_window, Canvas, ShapeCache};

/// The largest file read out of an archive, and the most read in all.
const MAX_ENTRY: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;

/// [`Skin::step`] says a window moved, opened, closed or changed its place in the order.
pub const MOVED: u32 = 1;
/// [`Skin::step`] says a window's pixels changed.
pub const DRAWN: u32 = 2;

/// One window of the skin as the app shows it.
pub struct Window {
    /// The engine's id for it, the same for as long as the skin is loaded.
    pub id: u32,
    /// Where it is in the viewport, in layout pixels.
    pub rect: Rect,
    /// Its pixels, `rect.width` x `rect.height`.
    pub canvas: Canvas,
    /// Counts up every time the pixels are drawn again.
    pub generation: u64,
    /// The part of the window the last drawing changed; the whole window when it drew it all.
    pub changed: Rect,
    /// The draw list the pixels were drawn from.
    cmds: Rc<Vec<DrawCmd>>,
    shapes: ShapeCache,
}

/// What drawing has cost so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Steps in which the engine had something new.
    pub steps: u64,
    /// Windows drawn again.
    pub windows_drawn: u64,
    /// Pixels those drawings covered.
    pub pixels_drawn: u64,
    /// Nanoseconds spent in the engine's tick and draw lists.
    pub engine_nanos: u64,
    /// Nanoseconds spent drawing pixels.
    pub draw_nanos: u64,
}

/// A loaded skin.
pub struct Skin {
    pub runtime: SkinRuntime,
    assets: Rc<RefCell<Assets>>,
    redraw: Rc<Cell<bool>>,
    /// The windows, bottom to top.
    windows: Vec<Window>,
    stats: Stats,
    desk: Rc<Desk>,
    /// The rows selected in the playlist window, as the skin last said.
    selected: Vec<usize>,
    /// The app's player, when the skin shows it: asked to move on before every step.
    pub player: Option<Rc<RefCell<crate::player::Player>>>,
    /// Set when the engine panicked behind the C interface: the skin does nothing more.
    pub broken: bool,
}

impl Skin {
    /// Opens the skin archive `bytes` named `file_name`, over the files of `base` when it is a
    /// classic skin and over those of `wasabi` when it is a modern one, with `fonts` under
    /// both, on a screen of `viewport` layout pixels, driven by `player`. `beside` is the
    /// folder the skin's file is in, where a skin it builds on is looked for.
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        bytes: Vec<u8>,
        file_name: &str,
        base: Option<MemoryFiles>,
        wasabi: Option<MemoryFiles>,
        fonts: MemoryFiles,
        beside: Option<&Path>,
        viewport: (i32, i32),
        player: impl FnOnce(Clock) -> Box<dyn Player>,
    ) -> Result<Skin, String> {
        let archive = unzip(bytes)?;
        // a classic skin lies over the classic base skin, a modern one over the wasabi base
        let under = if walkit::classic::is_classic(&archive) {
            base
        } else {
            wasabi
        };
        let siblings = Rc::new(Siblings {
            folder: beside.map(Path::to_path_buf),
            opened: RefCell::new(Vec::new()),
        });
        // the engine and the host each read the files, so the host decodes wasabi-base/...
        // pictures and the stand-in fonts from under the skin too; the bytes are shared
        let layered = || -> Box<dyn SkinFiles> {
            let skin: Box<dyn SkinFiles> = match &under {
                Some(base) => Box::new(LayeredFiles::new(
                    Box::new(archive.clone()),
                    Box::new(base.clone()),
                )),
                None => Box::new(archive.clone()),
            };
            Box::new(WithFonts {
                skin,
                fonts: fonts.clone(),
                siblings: siblings.clone(),
            })
        };
        let assets = Rc::new(RefCell::new(Assets::new(layered())));
        let redraw = Rc::new(Cell::new(true));
        let desk = Rc::new(Desk::default());
        let host = Host {
            assets: assets.clone(),
            redraw: redraw.clone(),
            desk: desk.clone(),
        };
        let start = Instant::now();
        let clock: Clock = Rc::new(move || start.elapsed().as_millis() as i64);
        // the folder name scripts know the skin by
        let name = Path::new(file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut runtime = SkinRuntime::new(
            layered(),
            Box::new(host),
            player(clock.clone()),
            SkinSetup {
                skin_name: name,
                // the app answers close, minimize, the menus and the clutter bar
                host_actions: true,
                ..SkinSetup::default()
            },
            clock,
        );
        let viewport = (viewport.0.max(1), viewport.1.max(1));
        runtime.set_viewport(viewport.0, viewport.1);
        runtime.load();
        // a mouse lands where it points
        runtime.set_pointer_precise(true);
        // a classic skin's equalizer and playlist start shut: open them, so all three windows
        // show (a modern skin has no containers by these names, and nothing opens)
        runtime.open_window("equalizer");
        runtime.open_window("pledit");
        runtime.tick();
        // then the windows are set out apart, each moved on its own as on Winamp's desktop;
        // they keep the spots the stack gave them
        let mut presentation = runtime.presentation();
        presentation.free_windows = true;
        presentation.viewport = viewport;
        runtime.set_presentation(&presentation);
        runtime.tick();
        Ok(Skin {
            runtime,
            assets,
            redraw,
            windows: Vec::new(),
            stats: Stats::default(),
            desk,
            selected: Vec::new(),
            player: None,
            broken: false,
        })
    }

    /// Sets the screen the windows move on, in layout pixels.
    pub fn set_viewport(&mut self, w: i32, h: i32) {
        let viewport = (w.max(1), h.max(1));
        let mut presentation = self.runtime.presentation();
        if presentation.viewport != viewport {
            presentation.viewport = viewport;
            self.runtime.set_viewport(viewport.0, viewport.1);
            self.runtime.set_presentation(&presentation);
            self.redraw.set(true);
        }
    }

    /// Moves every window by `dx`, `dy`: the arrangement stays, somewhere else on the screen.
    pub fn move_windows(&mut self, dx: i32, dy: i32) {
        for id in self.runtime.window_ids() {
            let rect = self.runtime.window_rect(id);
            self.runtime.place_window(id, rect.left + dx, rect.top + dy);
        }
        self.redraw.set(true);
    }

    /// Puts window `id` with its top left corner at `x`, `y` on the viewport.
    pub fn place_window(&mut self, id: u32, x: i32, y: i32) {
        self.runtime.place_window(WindowId(id), x, y);
        self.redraw.set(true);
    }

    /// Runs the engine's timers and animations and brings the windows up to date: their
    /// places, and the pixels of those that changed. Says what changed, [`MOVED`] and [`DRAWN`].
    pub fn step(&mut self) -> u32 {
        if let Some(player) = &self.player {
            player.borrow_mut().poll();
        }
        let began = Instant::now();
        let ticked = self.runtime.tick();
        let asked = self.redraw.replace(false);
        if !ticked && !asked && !self.windows.is_empty() {
            self.stats.engine_nanos += began.elapsed().as_nanos() as u64;
            return 0;
        }
        self.stats.steps += 1;
        // windows that only moved keep their pixels
        if let Some(placed) = self.runtime.take_placed_only() {
            if placed.len() == self.windows.len()
                && placed
                    .iter()
                    .all(|(id, _)| self.windows.iter().any(|w| w.id == id.0))
            {
                let mut old = std::mem::take(&mut self.windows);
                for (id, rect) in placed {
                    let at = old.iter().position(|w| w.id == id.0).expect("checked above");
                    let mut window = old.swap_remove(at);
                    window.rect = rect;
                    self.windows.push(window);
                }
                self.stats.engine_nanos += began.elapsed().as_nanos() as u64;
                return MOVED;
            }
        }
        let ids = self.runtime.drawn_window_ids();
        let drawn = self.runtime.draw_windows_owned();
        self.stats.engine_nanos += began.elapsed().as_nanos() as u64;

        let began = Instant::now();
        let mut flags = 0;
        let mut old = std::mem::take(&mut self.windows);
        let before: Vec<u32> = old.iter().map(|w| w.id).collect();
        for (id, d) in ids.into_iter().zip(drawn) {
            // a skin's container for something it never shows comes out one pixel large
            if d.rect.width < 2 || d.rect.height < 2 {
                continue;
            }
            let mut window = match old.iter().position(|w| w.id == id.0) {
                Some(at) => old.swap_remove(at),
                None => Window {
                    id: id.0,
                    rect: d.rect,
                    canvas: Canvas::new(0, 0),
                    generation: 0,
                    changed: Rect::new(0, 0, 0, 0),
                    cmds: Rc::new(Vec::new()),
                    shapes: ShapeCache::default(),
                },
            };
            if window.rect != d.rect {
                flags |= MOVED;
                window.rect = d.rect;
            }
            let resized = window.canvas.w != d.rect.width || window.canvas.h != d.rect.height;
            if resized {
                window.canvas = Canvas::new(d.rect.width, d.rect.height);
            }
            // the engine hands out the same list for a window that did not change
            if resized || window.generation == 0 || !Rc::ptr_eq(&window.cmds, &d.cmds) {
                let whole = Rect::new(0, 0, d.rect.width, d.rect.height);
                // a list that differs from the last one in a few commands draws only their part
                let part = if resized || window.generation == 0 {
                    None
                } else {
                    damage(&window.cmds, &d.cmds)
                };
                let nothing = part.is_some_and(|p| p.width <= 0 || p.height <= 0);
                if !nothing {
                    draw_window(
                        &mut window.canvas,
                        &mut self.assets.borrow_mut(),
                        &mut window.shapes,
                        &d.cmds,
                        part,
                    );
                    window.changed = part.map_or(whole, |p| p.cut_to(&whole));
                    window.generation += 1;
                    self.stats.windows_drawn += 1;
                    self.stats.pixels_drawn += (window.changed.width * window.changed.height).max(0) as u64;
                    flags |= DRAWN;
                }
                window.cmds = d.cmds;
            }
            self.windows.push(window);
        }
        // another order, or a window more or less
        if !self.windows.iter().map(|w| w.id).eq(before) {
            flags |= MOVED;
        }
        self.stats.draw_nanos += began.elapsed().as_nanos() as u64;
        flags
    }

    /// The windows, bottom to top, as the last [`Skin::step`] left them.
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Window `index` drawn afresh from the draw list it shows, all of it: what its pixels
    /// should be, however many partial drawings made them. For tests.
    pub fn drawn_whole(&self, index: usize) -> Vec<u32> {
        let window = &self.windows[index];
        let mut canvas = Canvas::new(window.canvas.w, window.canvas.h);
        draw_window(
            &mut canvas,
            &mut self.assets.borrow_mut(),
            &mut ShapeCache::default(),
            &window.cmds,
            None,
        );
        canvas.pixels
    }

    /// Milliseconds until the skin next changes by itself: 0 for now, -1 for never.
    pub fn next_due_ms(&mut self) -> i64 {
        if self.redraw.get() {
            return 0;
        }
        match self.runtime.next_due_at() {
            Some(at) => (at - self.runtime.now()).max(0),
            None => -1,
        }
    }

    /// What the skin's buttons asked of the app since the last call. What the playlist
    /// window asks that the player can answer by itself (which rows are selected, remove,
    /// crop, sort, a new list) is answered here and not handed on.
    pub fn take_actions(&mut self) -> Vec<HostAction> {
        let actions = std::mem::take(&mut *self.desk.actions.borrow_mut());
        actions
            .into_iter()
            .filter(|action| !self.answer(action))
            .collect()
    }

    /// Answers `action` when the player can; false leaves it to the app.
    fn answer(&mut self, action: &HostAction) -> bool {
        let Some(player) = self.player.clone() else {
            return false;
        };
        let rows = |param: &Option<String>| -> Vec<usize> {
            param
                .as_deref()
                .unwrap_or("")
                .split(',')
                .filter_map(|row| row.trim().parse().ok())
                .collect()
        };
        match (action.name.as_str(), action.param.as_deref()) {
            ("PLAYLIST_SELECT" | "PLAYLIST_SELECTION", _) => self.selected = rows(&action.param),
            ("PLAYLIST", Some("remove_selected")) => player.borrow_mut().remove(&self.selected),
            ("PLAYLIST", Some("crop")) => player.borrow_mut().crop(&self.selected),
            ("PLAYLIST", Some("remove_all" | "list_new")) => player.borrow_mut().clear(),
            ("PLAYLIST", Some("sort")) => player.borrow_mut().sort_by_title(),
            _ => return false,
        }
        if action.name == "PLAYLIST" {
            // the list changed under the selection
            self.selected.clear();
            self.runtime.set_playlist_selection(None);
            self.runtime.invalidate();
            self.redraw.set(true);
        }
        true
    }

    /// Tells the skin whether one of the app's switches is on ("doublesize", "ontop"), so the
    /// button for it lights up.
    pub fn set_switch(&mut self, name: &str, on: bool) {
        self.desk.switches.borrow_mut().insert(name.to_string(), on);
        self.runtime.invalidate();
        self.redraw.set(true);
    }

    /// Sets how a skin's popup menu is shown.
    pub fn set_menu_handler(&mut self, handler: Option<MenuHandler>) {
        *self.desk.menu.borrow_mut() = handler;
    }

    /// The skin's pictures and fonts, as the renderer reads them.
    pub fn assets(&self) -> &Rc<RefCell<Assets>> {
        &self.assets
    }

    /// What drawing has cost so far.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// The engine's id for window `id`.
    pub fn window_id(id: u32) -> WindowId {
        WindowId(id)
    }
}

/// The skins beside the one opened, for a skin that builds on another: Winamp's Bento reads
/// its scripts and pictures from `../Big Bento/`, the skin installed next to it.
struct Siblings {
    /// The folder the opened skin's file is in.
    folder: Option<PathBuf>,
    /// The siblings asked for so far, by the name the skin used: their files, or `None` for
    /// one that is not there.
    opened: RefCell<Vec<(String, Option<MemoryFiles>)>>,
}

impl Siblings {
    /// The file a path like `../Big Bento/scripts/main.maki` names, from the archive or the
    /// folder of that name beside the opened skin.
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        // also as Winamp spells its skins folder: @SKINSPATH@\Big Bento\scripts\notifier.maki
        let path = path.replace('\\', "/");
        let rest = path.strip_prefix("../").or_else(|| {
            let lower = path.to_lowercase();
            lower.starts_with("@skinspath@/").then(|| &path["@skinspath@/".len()..])
        })?;
        let (name, inner) = rest.split_once('/')?;
        let key = name.to_lowercase();
        let mut opened = self.opened.borrow_mut();
        if !opened.iter().any(|(known, _)| *known == key) {
            let files = self.find(&key);
            opened.push((key.clone(), files));
        }
        let (_, files) = opened.iter().find(|(known, _)| *known == key)?;
        files.as_ref()?.read(inner)
    }

    /// The files of the skin called `name` beside the opened one: a folder, or a `.wal` or
    /// `.zip` archive, its name compared without case and without spaces.
    fn find(&self, name: &str) -> Option<MemoryFiles> {
        let plain = |s: &str| s.to_lowercase().replace([' ', '_', '-'], "");
        let wanted = plain(name);
        for entry in std::fs::read_dir(self.folder.as_ref()?).ok()?.flatten() {
            let path = entry.path();
            let stem = path.file_stem().map(|s| plain(&s.to_string_lossy()))?;
            if stem != wanted {
                continue;
            }
            if path.is_dir() {
                return Some(read_tree(&path));
            }
            let archive = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wal") || e.eq_ignore_ascii_case("zip"));
            if archive {
                if let Some(files) = std::fs::read(&path).ok().and_then(|b| unzip(b).ok()) {
                    return Some(files);
                }
            }
        }
        None
    }
}

/// The files under the folder `dir`, by their paths from it.
fn read_tree(dir: &Path) -> MemoryFiles {
    fn walk(root: &Path, dir: &Path, files: &mut MemoryFiles) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, files);
            } else if let (Ok(inner), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
                files.insert(&inner.to_string_lossy(), bytes);
            }
        }
    }
    let mut files = MemoryFiles::new();
    walk(dir, dir, &mut files);
    files
}

/// A skin's files with the stand-in fonts where the engine looks for them, at `fonts/`, and
/// the skins beside it at `../<name>/`. A file of the skin's own by the same path comes first.
struct WithFonts {
    skin: Box<dyn SkinFiles>,
    fonts: MemoryFiles,
    siblings: Rc<Siblings>,
}

impl SkinFiles for WithFonts {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        self.skin
            .read(path)
            .or_else(|| self.fonts.read(path))
            .or_else(|| self.siblings.read(path))
    }

    fn names(&self) -> Vec<String> {
        let mut names = self.skin.names();
        names.extend(self.fonts.names());
        names
    }
}

/// The files of the folder `dir`, by their names: a set of files to lay under a skin.
pub fn read_folder(dir: &Path) -> MemoryFiles {
    let mut files = MemoryFiles::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let (true, Some(name), Ok(bytes)) = (path.is_file(), path.file_name(), std::fs::read(&path)) {
            files.insert(&name.to_string_lossy(), bytes);
        }
    }
    files
}

/// The files of a zip archive in memory. A skin zipped inside a folder has that folder
/// stripped.
pub fn unzip(bytes: Vec<u8>) -> Result<MemoryFiles, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut all = MemoryFiles::new();
    let mut names = Vec::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // the sizes an archive claims can lie: count what comes out
        let mut bytes = Vec::new();
        let read = entry
            .by_ref()
            .take(MAX_ENTRY + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())? as u64;
        if read > MAX_ENTRY {
            continue;
        }
        total += read;
        if total > MAX_TOTAL {
            return Err("the archive is too large".into());
        }
        all.insert(entry.name(), bytes);
        names.push(entry.name().to_string());
    }
    let folder = files::skin_folder(names.iter().map(String::as_str)).unwrap_or_default();
    Ok(all.in_folder(&folder))
}

/// The `.ttf` files in `dir`, as `fonts/<name>`: where the engine looks for the free fonts
/// that stand in for Arial and Tahoma.
pub fn read_fonts(dir: &Path) -> MemoryFiles {
    let mut fonts = MemoryFiles::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return fonts;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ttf"))
        {
            if let (Some(name), Ok(bytes)) = (path.file_name(), std::fs::read(&path)) {
                fonts.insert(&format!("fonts/{}", name.to_string_lossy()), bytes);
            }
        }
    }
    fonts
}
