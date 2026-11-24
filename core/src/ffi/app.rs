// SPDX-License-Identifier: GPL-3.0-or-later

//! The app: its start, its menu and commands, what it asks of the shell, what it keeps.

use std::ffi::{c_char, c_void, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::rc::Rc;

use super::{c_text, lines, panic_text, string, AndampPlayer, AndampSkin};
use crate::app::{App, Ask, Menu, MenuHandler};
use crate::vis;

/// Shows a skin's popup menu: `menu` is its items as lines of text (`C`, the id, 1 when
/// checked, 1 when disabled and the text, with tabs between, for a command; `S` for a
/// separator; `>` and the text where a submenu starts, `<` where it ends). It gives the id of
/// the command picked, or 0. It is called from inside a mouse call, and must not call the skin.
pub type AndampMenuHandler = unsafe extern "C" fn(context: *mut c_void, menu: *const c_char) -> i32;

/// The app behind the C interface: its player, its settings, the skin worn, its menu and
/// what it asks of the shell ([`App`]).
pub struct AndampApp {
    app: App,
    /// The app's player as the `andamp_player_` calls take it.
    player: AndampPlayer,
}

/// Runs `f` on the app at `app`; a panic gives `fallback`.
unsafe fn with_app<T>(app: *mut AndampApp, fallback: T, f: impl FnOnce(&mut App) -> T) -> T {
    let Some(handle) = app.as_mut() else {
        return fallback;
    };
    match catch_unwind(AssertUnwindSafe(|| f(&mut handle.app))) {
        Ok(v) => v,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            fallback
        }
    }
}

/// Makes the app from the `count` arguments of the command line at `args` (without the
/// program's name), with what comes with it in the folder `resources` (`skins`, `wasabi`,
/// `fonts`, `plugins`), on a screen of `screen_w` x `screen_h` points: the part the windows
/// move on. It reads the settings of the last run. Nothing shows until [`andamp_app_start`].
/// Free it with [`andamp_app_free`].
///
/// # Safety
/// `args` points at `count` C strings; `resources` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_new(
    args: *const *const c_char,
    count: i32,
    resources: *const c_char,
    screen_w: i32,
    screen_h: i32,
) -> *mut AndampApp {
    let args: Vec<String> = if args.is_null() {
        Vec::new()
    } else {
        (0..usize::try_from(count).unwrap_or(0))
            .map(|i| string(*args.add(i)))
            .collect()
    };
    let resources = string(resources);
    let made = catch_unwind(AssertUnwindSafe(|| {
        App::new(&args, Path::new(&resources), (screen_w, screen_h))
    }));
    match made {
        Ok(app) => {
            let player = AndampPlayer {
                player: app.player.clone(),
            };
            Box::into_raw(Box::new(AndampApp { app, player }))
        }
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            std::ptr::null_mut()
        }
    }
}

/// Frees an app [`andamp_app_new`] gave, with its skin and its player.
///
/// # Safety
/// `app` is null or an app not freed before.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_free(app: *mut AndampApp) {
    if !app.is_null() {
        let app = Box::from_raw(app);
        let _ = catch_unwind(AssertUnwindSafe(move || drop(app)));
    }
}

/// Wears the first skin and puts the playlist back, or plays what the command line names.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_start(app: *mut AndampApp) {
    with_app(app, (), App::start);
}

/// The app's player, for the `andamp_player_` calls. It lives as long as the app and is not
/// to be freed.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_player(app: *mut AndampApp) -> *mut AndampPlayer {
    match app.as_mut() {
        Some(handle) => &raw mut handle.player,
        None => std::ptr::null_mut(),
    }
}

/// The skin worn, for the `andamp_skin_` calls, or null when none is. It is the app's: not to
/// be freed, and gone when another skin is worn, which the request `skin` says.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_skin(app: *mut AndampApp) -> *mut AndampSkin {
    match app.as_mut().and_then(|handle| handle.app.skin()) {
        Some(skin) => skin,
        None => std::ptr::null_mut(),
    }
}

/// The file name of the skin worn, to free with [`andamp_string_free`](super::andamp_string_free).
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_skin_name(app: *mut AndampApp) -> *mut c_char {
    c_text(&with_app(app, String::new(), |a| a.skin_name()))
}

/// Runs the skin and answers what its buttons asked for. Bit 1 of the answer is set when a
/// window moved, opened, closed or changed its place in the order, bit 2 when a window's
/// pixels changed.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_step(app: *mut AndampApp) -> u32 {
    let broken = app
        .as_mut()
        .and_then(|handle| handle.app.skin())
        .is_some_and(|skin| skin.broken);
    if broken {
        return 0;
    }
    let Some(handle) = app.as_mut() else {
        return 0;
    };
    match catch_unwind(AssertUnwindSafe(|| handle.app.step())) {
        Ok(flags) => flags,
        Err(payload) => {
            eprintln!("andamp: {}", panic_text(payload.as_ref()));
            if let Some(skin) = handle.app.skin() {
                skin.broken = true;
            }
            0
        }
    }
}

/// Milliseconds until the app next has something to do by itself, and so until the next
/// [`andamp_app_step`]: 0 for now, -1 for never.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_next_due_ms(app: *mut AndampApp) -> i64 {
    with_app(app, -1, App::next_due_ms)
}

/// What the app asked of the shell since the last call, or null for nothing: a line for each
/// request, its parts with tabs between. Free it with [`andamp_string_free`](super::andamp_string_free).
///
/// - `quit`, `hide`, `about`
/// - `ask` and what to let the listener pick: `open_songs`, `add_songs`, `add_folders`,
///   `load_list`, `save_list`, `save_html`, `skin`, `presets`, or `url` for the address of a
///   radio stream.
///   The answer goes to [`andamp_app_picked`].
/// - `popup`, the number of one of the app's menus ([`andamp_app_menu`]) and, when there is
///   one, the place for it in points from the screen's top left corner; without a place it
///   opens at the mouse. The pick goes to [`andamp_app_command`].
/// - `skin`: another skin is worn, and the windows of the one before are gone.
/// - `look`: the size, the opacity or staying on top changed.
/// - `menu`: the menu bar changed.
/// - `effects`: show the Effects window.
/// - `preferences`: show the Preferences window ([`andamp_app_preferences`]).
/// - `info` and a row of the playlist: show what is known of it ([`andamp_app_file_info`]).
/// - `open` and a file: open it with whatever the system opens it with.
/// - `reveal` and a file or folder: show it in its folder.
/// - `vis` and 1 to open the Visualizer window, 0 to close it; [`andamp_app_vis_shown`] says
///   when it is done.
/// - `preset`, 1 for blending into it, the window's title and the preset's file.
/// - `error` and a message for the listener.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_take_requests(app: *mut AndampApp) -> *mut c_char {
    let requests = with_app(app, Vec::new(), App::take_requests);
    if requests.is_empty() {
        return std::ptr::null_mut();
    }
    let text: String = requests.iter().map(|r| r.line() + "\n").collect();
    c_text(&text)
}

/// One of the app's menus as lines of text, to free with [`andamp_string_free`](super::andamp_string_free): 0 the menu
/// bar, 1 the same commands as one popup list, and from 2 on the popups a request `popup`
/// names by their number. A line is as a
/// skin's menu gives it ([`AndampMenuHandler`]), with the command's key after the text: a
/// letter, with `shift+` before it when Shift is held, to go with the system's command key.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_menu(app: *mut AndampApp, menu: i32) -> *mut c_char {
    let Some(menu) = Menu::from_number(menu) else {
        return std::ptr::null_mut();
    };
    c_text(&with_app(app, String::new(), |a| a.menu(menu)))
}

/// Does the command with the number `id`, which an item of [`andamp_app_menu`] carries.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_command(app: *mut AndampApp, id: i32) {
    with_app(app, (), |a| a.command(id));
}

/// What the listener picked when a request `ask` asked for `what`: a line for each file or
/// folder, or the text they typed. Nothing picked needs no call.
///
/// # Safety
/// `app` is null or a live app; `what` and `answer` are C strings.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_picked(
    app: *mut AndampApp,
    what: *const c_char,
    answer: *const c_char,
) {
    let (what, answer) = (string(what), lines(answer));
    if let Some(ask) = Ask::from_name(&what) {
        with_app(app, (), |a| a.picked(ask, &answer));
    }
}

/// What the system asks the app to open, or what was dropped on a window, a line for each:
/// a skin is worn; songs, folders, playlist files and radio streams play. Before
/// [`andamp_app_start`] it waits for the start.
///
/// # Safety
/// `app` is null or a live app; `locations` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_open(app: *mut AndampApp, locations: *const c_char) {
    let locations = lines(locations);
    with_app(app, (), |a| a.open(&locations));
}

/// The engine's id of the skin's main window, or 0 when there is none: the window a shell
/// may group the others around.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_main_window(app: *mut AndampApp) -> u32 {
    with_app(app, 0, |a| a.main_window().unwrap_or(0))
}

/// The mouse went down at `x`, `y` on the viewport, in layout pixels; `touch` numbers the
/// press.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_pointer_down(app: *mut AndampApp, x: i32, y: i32, touch: i32) {
    with_app(app, (), |a| a.pointer_down(x, y, touch));
}

/// The keys held with the next presses: `add` is the system's key for picking playlist rows one
/// by one (Command on a Mac, Control elsewhere), `range` is Shift, for picking the rows from
/// the last one picked. Call it before [`andamp_app_pointer_down`].
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_modifiers(app: *mut AndampApp, add: bool, range: bool) {
    with_app(app, (), |a| a.set_modifiers(add, range));
}

/// The mouse moved to `x`, `y` while down.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_pointer_move(app: *mut AndampApp, x: i32, y: i32) {
    with_app(app, (), |a| a.pointer_move(x, y));
}

/// The mouse let go at `x`, `y`. After a press too short to have been seen, as a tap on a
/// trackpad, the skin shows the press for a moment first and acts then:
/// [`andamp_app_next_due_ms`] counts down to it.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_pointer_up(app: *mut AndampApp, x: i32, y: i32) {
    with_app(app, (), |a| a.pointer_up(x, y));
}

/// The press `touch` at `x`, `y` was the second of a double click; call it after
/// [`andamp_app_pointer_down`] for that press.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_double_click(app: *mut AndampApp, x: i32, y: i32, touch: i32) {
    with_app(app, (), |a| a.double_click(x, y, touch));
}

/// A click of the other mouse button at `x`, `y`, the whole click: what is under it shows
/// its menu. `touch` numbers it as a press.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_right_click(app: *mut AndampApp, x: i32, y: i32, touch: i32) {
    with_app(app, (), |a| a.right_click(x, y, touch));
}

/// The cursor for the mouse at `x`, `y` on the viewport, in layout pixels: 0 the arrow, 1 left
/// and right, 2 up and down, 3 the diagonal of a window's bottom right corner, 4 the other
/// diagonal, and from 100 on a cursor the skin brings, whose pictures
/// [`andamp_app_cursor_picture`] gives. While a divider or a window's edge is dragged it stays
/// that drag's.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_cursor(app: *mut AndampApp, x: i32, y: i32) -> u32 {
    with_app(app, 0, |a| a.cursor_at(x, y).number())
}

/// The pictures of the cursor [`andamp_app_cursor`] gave as `cursor` (100 or more), as whole
/// numbers, `count` of them, to free with [`andamp_words_free`](super::andamp_words_free):
/// how many steps, then for each step the picture it shows and for how many milliseconds; how
/// many pictures, then for each its width, its height, the tip's place from the left and from
/// the top, and its pixels row by row from the top, each `0xAARRGGBB` with the color not
/// multiplied by the alpha. Null, with a `count` of 0, when the skin worn has no such cursor.
///
/// # Safety
/// `app` is null or a live app; `count` points to a number to write.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_cursor_picture(
    app: *mut AndampApp,
    cursor: u32,
    count: *mut usize,
) -> *mut u32 {
    let slot = cursor.checked_sub(crate::app::Cursor::SKIN);
    let words = with_app(app, None, |a| a.cursor_picture(slot?));
    super::give_words(words, count)
}

/// Answers a key pressed in window `window` of the skin (its id, as
/// [`AndampWindow`](super::AndampWindow) has it), by the key's name. In the playlist's window "up" and "down" move the selection a row,
/// "pageup" and "pagedown" a screen, "home" and "end" to the list's ends. Everywhere: "delete"
/// takes the selected entries out of the playlist and "return" plays the first of them; "z"
/// previous, "x" play, "c" or "space" pause, "v" stop, "b" next, "up" and "down" for the
/// volume, "left" and "right" for five seconds back and on. False for any other key.
///
/// # Safety
/// `app` is null or a live app; `key` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_key(
    app: *mut AndampApp,
    window: u32,
    key: *const c_char,
) -> bool {
    let key = string(key);
    with_app(app, false, |a| a.key_in(window, &key))
}

/// Window `window` of the skin (its id, as [`AndampWindow`](super::AndampWindow) has it) is
/// the one the keys go to now, by another way than a press on it: the skin shows it as the
/// one in front on the next step.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_focus(app: *mut AndampApp, window: u32) {
    with_app(app, (), |a| a.focus(window));
}

/// The mouse wheel at `x`, `y` on the viewport, in layout pixels: over a list of the skin,
/// wherever it shows one, it scrolls the list, over the volume slider it turns the volume,
/// and anywhere else it does nothing. `delta` is more than 0 for up, in notches of a wheel, or with `precise`
/// in points of a trackpad.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_wheel(
    app: *mut AndampApp,
    x: i32,
    y: i32,
    delta: f32,
    precise: bool,
) {
    with_app(app, (), |a| a.wheel((x, y), delta, precise));
}

/// Sets how the skin's own popup menus are shown, for the skin worn and every one after it;
/// `context` is handed back to `handler`.
///
/// # Safety
/// `app` is null or a live app; `handler` stays callable, with `context`, for as long as the
/// app lives.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_set_menu_handler(
    app: *mut AndampApp,
    handler: Option<AndampMenuHandler>,
    context: *mut c_void,
) {
    with_app(app, (), |a| {
        a.set_menu_handler(handler.map(|show| -> MenuHandler {
            Rc::new(move |menu: &str| {
                let text = CString::new(menu.replace('\0', " ")).unwrap_or_default();
                // SAFETY: the caller promised the handler and its context for the app's life
                unsafe { show(context, text.as_ptr()) }
            })
        }));
    });
}

/// Points to a layout pixel of the skin: 2 is Winamp's double size.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_zoom(app: *mut AndampApp) -> i32 {
    with_app(app, 1, |a| a.zoom())
}

/// Whether the windows stay above other apps' windows.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_on_top(app: *mut AndampApp) -> bool {
    with_app(app, false, |a| a.on_top())
}

/// How much of the windows shows over what is behind them, in percent: 20 to 100.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_opacity(app: *mut AndampApp) -> i32 {
    with_app(app, 100, |a| a.opacity())
}

/// The part of the screen the windows move on changed, to `w` x `h` points.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_set_screen(app: *mut AndampApp, w: i32, h: i32) {
    with_app(app, (), |a| a.set_screen((w, h)));
}

/// The shell opened the Visualizer window (`open`) or it closed. When it opens, the app asks
/// for the first preset with a request `preset`.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_shown(app: *mut AndampApp, open: bool) {
    with_app(app, (), |a| a.vis_shown(open));
}

/// A key in the Visualizer window, by its name: `right` the next preset, `left` the one
/// before, `r` one at random, `space` holds the one shown or lets it go. False for any other.
///
/// # Safety
/// `app` is null or a live app; `key` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_key(app: *mut AndampApp, key: *const c_char) -> bool {
    let key = string(key);
    with_app(app, false, |a| a.vis_key(&key))
}

/// Seconds a preset stays, as the listener set it: the shell calls [`andamp_app_vis_tick`] that
/// often while the Visualizer window can be seen.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_stay(app: *mut AndampApp) -> i32 {
    with_app(app, vis::STAY_SECONDS as i32, |a| a.vis_stay())
}

/// Frames a second the Visualizer window draws, as the listener set it: 30 or 60.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_fps(app: *mut AndampApp) -> i32 {
    with_app(app, 30, |a| a.vis_fps())
}

/// The preferences as JSON, to free with [`andamp_string_free`](super::andamp_string_free),
/// for the shell to draw as a window: `{"pages": [...]}`, each page a `title` and its `rows`.
/// A row has an `id`, a `kind` (`toggle`, `choice`, `slider`, `action`, `text` or `heading`),
/// a `label` and a `value` as text: `1` or `0` for a toggle, an option's value for a choice,
/// a number for a slider. A choice also has its `options`, each a `name` and a `value`; a
/// slider its `min`, `max`, `step` and `unit`.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_preferences(app: *mut AndampApp) -> *mut c_char {
    c_text(&with_app(app, "{\"pages\":[]}".to_string(), |a| {
        a.preferences()
    }))
}

/// Sets the preference `id` to `value`, as [`andamp_app_preferences`] spells values out, or
/// does the action `id` (its value is not looked at). The shell then asks for the
/// preferences again: one change may move another row.
///
/// # Safety
/// `app` is null or a live app; `id` and `value` are C strings.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_set_preference(
    app: *mut AndampApp,
    id: *const c_char,
    value: *const c_char,
) {
    let (id, value) = (string(id), string(value));
    with_app(app, (), |a| a.set_preference(&id, &value));
}

/// What is known of the playlist's entry at `row`, to free with
/// [`andamp_string_free`](super::andamp_string_free), for a request `info`: its title on the
/// first line, then a line for each thing known, a name and its value with a colon between.
/// Null when the playlist has no such row.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_file_info(app: *mut AndampApp, row: i32) -> *mut c_char {
    let info = with_app(app, None, |a| {
        usize::try_from(row).ok().and_then(|row| a.file_info(row))
    });
    match info {
        Some(text) => c_text(&text),
        None => std::ptr::null_mut(),
    }
}

/// A preset's stay is over: the next one blends in, unless the listener holds the one shown.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_vis_tick(app: *mut AndampApp) {
    with_app(app, (), App::vis_tick);
}

/// Loads every effect plug-in once, those that come with the app and the listener's own, and
/// puts each back as it was left: before the rack is shown.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_load_effects(app: *mut AndampApp) {
    with_app(app, (), App::load_effects);
}

/// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder. Gives
/// its place in the rack, or -1 and, when `error` is not null, why in `*error`, to free with
/// [`andamp_string_free`](super::andamp_string_free).
///
/// # Safety
/// `app` is null or a live app; `file` is a C string; `error` is null or a place for a pointer.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_add_plugin(
    app: *mut AndampApp,
    file: *const c_char,
    error: *mut *mut c_char,
) -> i32 {
    let file = string(file);
    match with_app(app, Err("the app stopped".to_string()), |a| {
        a.add_plugin(Path::new(&file))
    }) {
        Ok(index) => index as i32,
        Err(message) => {
            if !error.is_null() {
                *error = c_text(&message);
            }
            -1
        }
    }
}

/// Whether effect `index` of the rack is a plug-in the listener added, which
/// [`andamp_app_remove_plugin`] can take out again.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_effect_own(app: *mut AndampApp, index: i32) -> bool {
    with_app(app, false, |a| {
        usize::try_from(index).is_ok_and(|i| a.is_own(i))
    })
}

/// Takes a plug-in the listener added out of the rack and out of the app's folder.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_remove_plugin(app: *mut AndampApp, index: i32) {
    with_app(app, (), |a| {
        if let Ok(index) = usize::try_from(index) {
            a.remove_plugin(index);
        }
    });
}

/// Keeps what the next run starts from: the skin and where its windows are, the size, the
/// playlist, the volume, the equalizer, the effects. The shell calls it when the app goes to
/// the background and when it ends.
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_save(app: *mut AndampApp) {
    with_app(app, (), App::save);
}

/// What the command line gave for one of the options the shell answers itself, to free with
/// [`andamp_string_free`](super::andamp_string_free), or null when it was not given: `dump`, `bench`, `quit-after`,
/// `then-skin`, `presets`, `key-window` with their value, `hidden`, `show-vis`, `show-effects`, `show-prefs` with `1`,
/// and `click` and `press-pref` with a line for each.
///
/// # Safety
/// `app` is null or a live app; `name` is a C string.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_option(
    app: *mut AndampApp,
    name: *const c_char,
) -> *mut c_char {
    let name = string(name);
    match with_app(app, None, |a| a.options.get(&name)) {
        Some(value) => c_text(&value),
        None => std::ptr::null_mut(),
    }
}

/// What the app would keep for the next run, in a line, to free with [`andamp_string_free`](super::andamp_string_free).
///
/// # Safety
/// `app` is null or a live app.
#[no_mangle]
pub unsafe extern "C" fn andamp_app_summary(app: *mut AndampApp) -> *mut c_char {
    c_text(&with_app(app, String::new(), |a| a.summary()))
}
