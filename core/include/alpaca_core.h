// SPDX-License-Identifier: GPL-3.0-or-later
//
// The C interface of the alpaca-core library (core/src/ffi): the app, its player and the skin
// worn, for a shell written in another language than Rust. A shell makes the app
// (alpaca_app_new), starts it, steps it and shows the windows of the skin it wears; the
// alpaca_skin_ and alpaca_player_ calls take the skin and the player the app gives out.
//
// Every call runs on the one thread the shell made the app on. Text is UTF-8; a `char *` a
// call gives is freed with alpaca_string_free.
#ifndef ALPACA_CORE_H
#define ALPACA_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/// The skin the app wears (alpaca_app_skin).
typedef struct AlpacaSkin AlpacaSkin;

/// The app's player (alpaca_app_player): the playlist, the transport and the audio. Every
/// skin shows and drives it.
typedef struct AlpacaPlayer AlpacaPlayer;

/// One window of a skin: where it is on the viewport in layout pixels, its `w * h` pixels
/// (rows top first, BGRA in memory, the color multiplied by the alpha), a count that goes up
/// whenever the pixels are drawn again, and the part of the window the last drawing changed,
/// in its own pixels: an app that showed the drawing before needs to take over only that
/// part. The pixels stay valid until the next step.
typedef struct {
    uint32_t id;
    int32_t x, y, w, h;
    const uint32_t *pixels;
    uint64_t generation;
    int32_t changed_x, changed_y, changed_w, changed_h;
} AlpacaWindow;

/// alpaca_app_step: a window moved, opened, closed or changed its place in the order.
#define ALPACA_MOVED 1u
/// alpaca_app_step: a window's pixels changed.
#define ALPACA_DRAWN 2u

/// Frees text a call gave.
void alpaca_string_free(char *text);

/// Frees whole numbers a call gave, with the count it gave.
void alpaca_words_free(uint32_t *words, size_t count);

/// How many windows the skin shows.
int32_t alpaca_skin_window_count(AlpacaSkin *skin);

/// Window `index`, counted from the bottom, into `*out`; false when there is none.
bool alpaca_skin_window(AlpacaSkin *skin, int32_t index, AlpacaWindow *out);

/// What drawing has cost so far: steps in which something changed, windows drawn again, and
/// the nanoseconds spent in the engine and in drawing pixels. A null pointer is skipped.
void alpaca_skin_stats(AlpacaSkin *skin, uint64_t *steps, uint64_t *windows_drawn,
                       uint64_t *engine_nanos, uint64_t *draw_nanos);

/// Shows a skin's popup menu: `menu` is its items as lines of text (C, the id, 1 when checked,
/// 1 when disabled and the text, with tabs between, for a command; S for a separator; > and
/// the text where a submenu starts, < where it ends). It gives the id of the command picked,
/// or 0. It is called from inside a mouse call, and must not call the library.
typedef int32_t (*AlpacaMenuHandler)(void *context, const char *menu);

/// How many entries the playlist has.
int32_t alpaca_player_count(AlpacaPlayer *player);

/// Sets the volume, 0..255.
void alpaca_player_set_volume(AlpacaPlayer *player, int32_t volume);

/// The volume, 0..255.
int32_t alpaca_player_volume(AlpacaPlayer *player);

/// Moves the player on when its song has ended. A skin's step does this by itself; the app
/// calls it while no skin steps, so that the playlist goes on with every window out of sight.
void alpaca_player_poll(AlpacaPlayer *player);

/// What the transport is doing: 0 stopped, 1 playing, 2 paused.
int32_t alpaca_player_status(AlpacaPlayer *player);

/// Which entry of the playlist is current.
int32_t alpaca_player_index(AlpacaPlayer *player);

/// Where the song is, in milliseconds.
int32_t alpaca_player_position_ms(AlpacaPlayer *player);

/// Moves the song to `ms` milliseconds.
void alpaca_player_seek_to(AlpacaPlayer *player, int32_t ms);

/// Entry `index` of the playlist: its length in milliseconds, or -1 when there is none. The
/// pointers that are not null get its title, its artist, its album and its file's path, each to
/// free with alpaca_string_free; a tag the file does not carry comes out empty.
int32_t alpaca_player_entry(AlpacaPlayer *player, int32_t index, char **title, char **artist,
                            char **album, char **path);

/// Blocks of audio in which the output ran dry while a song went on, so far.
uint64_t alpaca_player_underruns(AlpacaPlayer *player);

/// The rack as JSON, to free with alpaca_string_free: {"effects":[...]}, each effect with id,
/// name, version, author, about, on, error (why it is left out of the stream playing, or
/// empty), params (each id, name, kind "number", "toggle" or "choice", min, max, default,
/// value, unit, help, scale, decimals or -1, zero, options of name and value) and presets
/// (each name and values as pairs of a control's place and a number).
char *alpaca_player_effects(AlpacaPlayer *player);

/// Switches effect `index`: it fades in or out, so the switch does not click.
void alpaca_player_set_effect_on(AlpacaPlayer *player, int32_t index, bool on);

/// Moves control `param` of effect `index` to `value`, in the control's own units.
void alpaca_player_set_effect_value(AlpacaPlayer *player, int32_t index, int32_t param, float value);

/// Applies preset `preset` of effect `index`, or with -1 puts its controls back at their
/// defaults; either way in one step, so nothing sweeps.
void alpaca_player_apply_preset(AlpacaPlayer *player, int32_t index, int32_t preset);

/// The samples heard since the last call, both sides mixed, oldest first, into `out`, which
/// has room for `max`; gives how many there were. Nothing while no song plays. For a visualizer
/// that takes every sample once.
int32_t alpaca_player_take_wave(AlpacaPlayer *player, float *out, int32_t max);

/// Control `param` of effect `index` at `value` as the listener reads it ("50 %", "-3.0 dB"),
/// to free with alpaca_string_free.
char *alpaca_player_effect_readout(AlpacaPlayer *player, int32_t index, int32_t param, float value);

// The app. It owns the player, the settings and the skin worn; it knows the menu, what each
// command does, what the skin's buttons ask for, what a start puts back and what is kept for
// the next run.

/// The app.
typedef struct AlpacaApp AlpacaApp;

/// Makes the app from the `count` arguments of the command line at `args` (without the
/// program's name), with what comes with it in the folder `resources` (`skins`, `wasabi`,
/// `fonts`, `plugins`), on one screen of `screen_w` x `screen_h` points at the desktop's top
/// left corner, until alpaca_app_set_screens says where the screens are. It reads the settings of the last run. Nothing shows until alpaca_app_start.
/// Free it with alpaca_app_free.
AlpacaApp *alpaca_app_new(const char *const *args, int32_t count, const char *resources,
                          int32_t screen_w, int32_t screen_h);

/// Frees the app, with its skin and its player.
void alpaca_app_free(AlpacaApp *app);

/// Wears the first skin. The playlist comes back, or what the command line names plays, at
/// the second alpaca_app_step after it, so the skin shows before the songs are read.
void alpaca_app_start(AlpacaApp *app);

/// The app's player, for the alpaca_player_ calls. It lives as long as the app and is not to
/// be freed.
AlpacaPlayer *alpaca_app_player(AlpacaApp *app);

/// The skin worn, for the alpaca_skin_ calls, or null when none is. It is the app's: not to be
/// freed, and gone when another skin is worn, which the request `skin` says.
AlpacaSkin *alpaca_app_skin(AlpacaApp *app);

/// The file name of the skin worn, to free with alpaca_string_free.
char *alpaca_app_skin_name(AlpacaApp *app);

/// Runs the skin and answers what its buttons asked for. ALPACA_MOVED and ALPACA_DRAWN are
/// set in the answer for what changed.
uint32_t alpaca_app_step(AlpacaApp *app);

/// Milliseconds until the app next has something to do by itself, and so until the next
/// alpaca_app_step: 0 for now, -1 for never.
int64_t alpaca_app_next_due_ms(AlpacaApp *app);

/// What the app asked of the shell since the last call, or null for nothing: a line for each
/// request, its parts with tabs between. Free it with alpaca_string_free.
///
/// - `quit`, `hide`, `about`
/// - `ask` and what to let the listener pick: `open_songs`, `add_songs`, `add_folders`,
///   `load_list`, `save_list`, `save_html`, `skin`, `presets`, or `url` for the address of a
///   radio stream.
///   The answer goes to alpaca_app_picked.
/// - `popup`, the number of one of the app's menus (alpaca_app_menu) and, when there is one,
///   the place for it in points from the screen's top left corner; without a place it opens
///   at the mouse. The pick goes to alpaca_app_command.
/// - `skin`: another skin is worn, and the windows of the one before are gone.
/// - `look`: the size, the opacity or staying on top changed.
/// - `menu`: the menu bar changed.
/// - `effects`: show the Effects window.
/// - `preferences`: show the Preferences window (alpaca_app_preferences).
/// - `info` and a row of the playlist: show what is known of it (alpaca_app_file_info).
/// - `open` and a file: open it with whatever the system opens it with.
/// - `reveal` and a file or folder: show it in its folder.
/// - `vis` and 1 to open the Visualizer window, 0 to close it; alpaca_app_vis_shown says when
///   it is done.
/// - `preset`, 1 for blending into it, the window's title and the preset's file.
/// - `error` and a message for the listener.
char *alpaca_app_take_requests(AlpacaApp *app);

/// The app's menus, by number. A request `popup` names the others by theirs.
#define ALPACA_MENU_BAR 0
#define ALPACA_MENU_POPUP 1

/// One of the app's menus as lines of text, to free with alpaca_string_free. A line is as a
/// skin's menu gives it (AlpacaMenuHandler), with the command's key after the text: a letter,
/// with `shift+` before it when Shift is held, to go with the system's command key.
char *alpaca_app_menu(AlpacaApp *app, int32_t menu);

/// The commands a shell asks for by itself; the rest come as the numbers of menu items.
#define ALPACA_COMMAND_PREVIOUS 10
#define ALPACA_COMMAND_PLAY 11
#define ALPACA_COMMAND_PAUSE 12
#define ALPACA_COMMAND_STOP 13
#define ALPACA_COMMAND_NEXT 14
#define ALPACA_COMMAND_QUIT 26

/// Does the command with the number `id`, which an item of alpaca_app_menu carries.
void alpaca_app_command(AlpacaApp *app, int32_t id);

/// What the listener picked when a request `ask` asked for `what`: a line for each file or
/// folder, or the text they typed. Nothing picked needs no call.
void alpaca_app_picked(AlpacaApp *app, const char *what, const char *answer);

/// What the system asks the app to open, or what was dropped on a window, a line for each: a
/// skin is worn; songs, folders, playlist files and radio streams play. Before
/// alpaca_app_start it waits for the start.
void alpaca_app_open(AlpacaApp *app, const char *locations);

/// The engine's id of the skin's main window, or 0 when there is none: the window a shell may
/// group the others around.
uint32_t alpaca_app_main_window(AlpacaApp *app);

/// The mouse, in layout pixels on the viewport: down (`touch` numbers the press), moved while
/// down, and up. After a press too short to have been seen, as a tap on a trackpad, the skin
/// shows the press for a moment first and acts then: alpaca_app_next_due_ms counts down to it.
void alpaca_app_pointer_down(AlpacaApp *app, int32_t x, int32_t y, int32_t touch);
void alpaca_app_pointer_move(AlpacaApp *app, int32_t x, int32_t y);
void alpaca_app_pointer_up(AlpacaApp *app, int32_t x, int32_t y);
/// The mouse moved with no button down, in layout pixels on the viewport: the skin gets the
/// hover events, and a modern skin's button shows its hover picture. And the mouse left the skin.
void alpaca_app_hover(AlpacaApp *app, int32_t x, int32_t y);
void alpaca_app_pointer_leave(AlpacaApp *app);
/// The tooltip to show beside the mouse now, empty for none (free with alpaca_string_free):
/// 350 ms after the mouse came onto an object with one, for 3 s. Ask after each step.
char *alpaca_app_tooltip(AlpacaApp *app);

/// The keys held with the next presses: `add` is the system's key for picking playlist rows one
/// by one (Command on a Mac, Control elsewhere), `range` is Shift, for picking the rows from
/// the last one picked. Call it before alpaca_app_pointer_down.
void alpaca_app_modifiers(AlpacaApp *app, bool add, bool range);

/// The press `touch` at `x`, `y` was the second of a double click; call it after
/// alpaca_app_pointer_down for that press.
void alpaca_app_double_click(AlpacaApp *app, int32_t x, int32_t y, int32_t touch);

/// A click of the other mouse button at `x`, `y`, the whole click: what is under it shows its
/// menu. `touch` numbers it as a press.
void alpaca_app_right_click(AlpacaApp *app, int32_t x, int32_t y, int32_t touch);

/// What alpaca_app_cursor answers: one of the system's cursors, or from ALPACA_CURSOR_SKIN on
/// one the skin brings.
#define ALPACA_CURSOR_ARROW 0
#define ALPACA_CURSOR_LEFT_RIGHT 1
#define ALPACA_CURSOR_UP_DOWN 2
#define ALPACA_CURSOR_DIAGONAL 3
#define ALPACA_CURSOR_OTHER_DIAGONAL 4
#define ALPACA_CURSOR_SKIN 100

/// The cursor for the mouse at `x`, `y` on the viewport, in layout pixels: the arrow, left and
/// right, up and down, the diagonal of a window's bottom right corner, the other diagonal, or
/// a cursor the skin brings, whose pictures alpaca_app_cursor_picture gives. While a divider
/// or a window's edge is dragged it stays that drag's.
uint32_t alpaca_app_cursor(AlpacaApp *app, int32_t x, int32_t y);

/// The pictures of the cursor alpaca_app_cursor gave as `cursor` (ALPACA_CURSOR_SKIN or more),
/// as whole numbers, `count` of them, to free with alpaca_words_free: how many steps, then for
/// each step the picture it shows and for how many milliseconds; how many pictures, then for
/// each its width, its height, the tip's place from the left and from the top, and its pixels
/// row by row from the top, each 0xAARRGGBB with the color not multiplied by the alpha. Null,
/// with a `count` of 0, when the skin worn has no such cursor.
uint32_t *alpaca_app_cursor_picture(AlpacaApp *app, uint32_t cursor, size_t *count);

/// Answers a key pressed in window `window` of the skin (its id, as AlpacaWindow has it), by
/// the key's name. In the playlist's window "up" and "down" move the selection a row, "pageup"
/// and "pagedown" a screen, "home" and "end" to the list's ends. Everywhere: "delete" takes the
/// selected entries out of the playlist and "return" plays the first of them; "z" previous,
/// "x" play, "c" or "space" pause, "v" stop, "b" next, "up" and "down" for the volume, "left"
/// and "right" for five seconds back and on. False for any other key.
bool alpaca_app_key(AlpacaApp *app, uint32_t window, const char *key);

/// Window `window` of the skin (its id, as AlpacaWindow has it) is the one the keys go to now,
/// by another way than a press on it: the skin shows it as the one in front on the next step.
void alpaca_app_focus(AlpacaApp *app, uint32_t window);

/// Whether the app has the keyboard: while another app has it, every window of the skin is drawn
/// inactive, and the window that had the focus shows it again when the app is active again.
void alpaca_app_set_active(AlpacaApp *app, bool active);

/// The mouse wheel at `x`, `y` on the viewport, in layout pixels: over a list of the skin,
/// wherever it shows one, it scrolls the list, over the volume slider it turns the volume,
/// and anywhere else it does nothing. `delta` is more than 0 for up, in notches of a wheel, or with `precise`
/// in points of a trackpad.
void alpaca_app_wheel(AlpacaApp *app, int32_t x, int32_t y, float delta, bool precise);

/// Sets how the skin's own popup menus are shown, for the skin worn and every one after it;
/// `context` is handed back to `handler`, which stays callable for as long as the app lives.
void alpaca_app_set_menu_handler(AlpacaApp *app, AlpacaMenuHandler handler, void *context);

/// Points to a layout pixel of the skin: 2 is Winamp's double size.
int32_t alpaca_app_zoom(AlpacaApp *app);

/// Whether the windows stay above other apps' windows.
bool alpaca_app_on_top(AlpacaApp *app);

/// How much of the windows shows over what is behind them, in percent: 20 to 100.
int32_t alpaca_app_opacity(AlpacaApp *app);

/// The screens are now the `count` rectangles at `rects`, four numbers each: the left and
/// top edges, the width and the height of the part of a screen the windows move on, in
/// points on the desktop with y down; the main screen first. Said before the start and
/// whenever the screens change; the windows no screen shows come back into view at the next
/// step.
void alpaca_app_set_screens(AlpacaApp *app, const int32_t *rects, int32_t count);

/// Where the viewport's top left corner is on the desktop, in points: a window at `x`, `y`
/// in layout pixels is at `origin + (x, y) * zoom`, and the mouse is handed over in layout
/// pixels from it. It moves when the main window goes to another screen, so a shell asks
/// after every step. A null pointer is skipped.
void alpaca_app_origin(AlpacaApp *app, int32_t *x, int32_t *y);

/// The shell opened the Visualizer window (`open`) or it closed. When it opens, the app asks
/// for the first preset with a request `preset`.
void alpaca_app_vis_shown(AlpacaApp *app, bool open);

/// A key in the Visualizer window, by its name: `right` the next preset, `left` the one
/// before, `r` one at random, `space` holds the one shown or lets it go. False for any other.
bool alpaca_app_vis_key(AlpacaApp *app, const char *key);

/// Seconds a preset stays, as the listener set it: the shell calls alpaca_app_vis_tick that
/// often while the Visualizer window can be seen.
int32_t alpaca_app_vis_stay(AlpacaApp *app);

/// Frames a second the Visualizer window draws, as the listener set it: 30 or 60.
int32_t alpaca_app_vis_fps(AlpacaApp *app);

/// The preferences as JSON, to free with alpaca_string_free, for the shell to draw as a
/// window: {"pages": [...]}, each page a `title` and its `rows`. A row has an `id`, a `kind`
/// (toggle, choice, slider, action, text or heading), a `label` and a `value` as text: 1 or 0
/// for a toggle, an option's value for a choice, a number for a slider. A choice also has its
/// `options`, each a `name` and a `value`; a slider its `min`, `max`, `step` and `unit`.
char *alpaca_app_preferences(AlpacaApp *app);

/// Sets the preference `id` to `value`, as alpaca_app_preferences spells values out, or does
/// the action `id` (its value is not looked at). The shell then asks for the preferences
/// again: one change may move another row.
void alpaca_app_set_preference(AlpacaApp *app, const char *id, const char *value);

/// What is known of the playlist's entry at `row`, to free with alpaca_string_free, for a
/// request `info`: its title on the first line, then a line for each thing known, a name and
/// its value with a colon between. Null when the playlist has no such row.
char *alpaca_app_file_info(AlpacaApp *app, int32_t row);

/// A preset's stay is over: the next one blends in, unless the listener holds the one shown.
void alpaca_app_vis_tick(AlpacaApp *app);

/// Loads every effect plug-in once, those that come with the app and the listener's own, and
/// puts each back as it was left: before the rack is shown.
void alpaca_app_load_effects(AlpacaApp *app);

/// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder. Gives
/// its place in the rack, or -1 and, when `error` is not null, why in `*error`, to free with
/// alpaca_string_free.
int32_t alpaca_app_add_plugin(AlpacaApp *app, const char *file, char **error);

/// Whether effect `index` of the rack is a plug-in the listener added, which
/// alpaca_app_remove_plugin can take out again.
bool alpaca_app_effect_own(AlpacaApp *app, int32_t index);

/// Takes a plug-in the listener added out of the rack and out of the app's folder.
void alpaca_app_remove_plugin(AlpacaApp *app, int32_t index);

/// Keeps what the next run starts from: the skin and where its windows are, the size, the
/// playlist, the volume, the equalizer, the effects. The shell calls it when the app goes to
/// the background and when it ends.
void alpaca_app_save(AlpacaApp *app);

/// What the command line gave for one of the options the shell answers itself, to free with
/// alpaca_string_free, or null when it was not given: `dump`, `bench`, `quit-after`,
/// `then-skin`, `presets`, `key-window` with their value, `hidden`, `show-vis`, `show-effects`, `show-prefs`
/// with `1`, and
/// `click` and `press-pref` with a line for each.
char *alpaca_app_option(AlpacaApp *app, const char *name);

/// What the app would keep for the next run, in a line, to free with alpaca_string_free.
char *alpaca_app_summary(AlpacaApp *app);

#endif
