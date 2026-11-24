// SPDX-License-Identifier: GPL-3.0-or-later
//
// The C interface of the andamp-core library (core/src/ffi): the app, its player and the skin
// worn, for a shell written in another language than Rust. A shell makes the app
// (andamp_app_new), starts it, steps it and shows the windows of the skin it wears; the
// andamp_skin_ and andamp_player_ calls take the skin and the player the app gives out.
//
// Every call runs on the one thread the shell made the app on. Text is UTF-8; a `char *` a
// call gives is freed with andamp_string_free.
#ifndef ANDAMP_CORE_H
#define ANDAMP_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/// The skin the app wears (andamp_app_skin).
typedef struct AndampSkin AndampSkin;

/// The app's player (andamp_app_player): the playlist, the transport and the audio. Every
/// skin shows and drives it.
typedef struct AndampPlayer AndampPlayer;

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
} AndampWindow;

/// andamp_app_step: a window moved, opened, closed or changed its place in the order.
#define ANDAMP_MOVED 1u
/// andamp_app_step: a window's pixels changed.
#define ANDAMP_DRAWN 2u

/// Frees text a call gave.
void andamp_string_free(char *text);

/// How many windows the skin shows.
int32_t andamp_skin_window_count(AndampSkin *skin);

/// Window `index`, counted from the bottom, into `*out`; false when there is none.
bool andamp_skin_window(AndampSkin *skin, int32_t index, AndampWindow *out);

/// What drawing has cost so far: steps in which something changed, windows drawn again, and
/// the nanoseconds spent in the engine and in drawing pixels. A null pointer is skipped.
void andamp_skin_stats(AndampSkin *skin, uint64_t *steps, uint64_t *windows_drawn,
                       uint64_t *engine_nanos, uint64_t *draw_nanos);

/// The mouse, in layout pixels on the viewport: down (`touch` numbers the press), moved while
/// down, and up.
void andamp_skin_pointer_down(AndampSkin *skin, int32_t x, int32_t y, int32_t touch);
void andamp_skin_pointer_move(AndampSkin *skin, int32_t x, int32_t y);
void andamp_skin_pointer_up(AndampSkin *skin, int32_t x, int32_t y);

/// A double click; `touch` numbers its press.
void andamp_skin_double_click(AndampSkin *skin, int32_t x, int32_t y, int32_t touch);

/// A right click on what press `touch` landed on.
void andamp_skin_right_click(AndampSkin *skin, int32_t touch);

/// Shows a skin's popup menu: `menu` is its items as lines of text (C, the id, 1 when checked,
/// 1 when disabled and the text, with tabs between, for a command; S for a separator; > and
/// the text where a submenu starts, < where it ends). It gives the id of the command picked,
/// or 0. It is called from inside a mouse call, and must not call the library.
typedef int32_t (*AndampMenuHandler)(void *context, const char *menu);

/// How many entries the playlist has.
int32_t andamp_player_count(AndampPlayer *player);

/// Sets the volume, 0..255.
void andamp_player_set_volume(AndampPlayer *player, int32_t volume);

/// The volume, 0..255.
int32_t andamp_player_volume(AndampPlayer *player);

/// Moves the player on when its song has ended. A skin's step does this by itself; the app
/// calls it while no skin steps, so that the playlist goes on with every window out of sight.
void andamp_player_poll(AndampPlayer *player);

/// What the transport is doing: 0 stopped, 1 playing, 2 paused.
int32_t andamp_player_status(AndampPlayer *player);

/// Which entry of the playlist is current.
int32_t andamp_player_index(AndampPlayer *player);

/// Where the song is, in milliseconds.
int32_t andamp_player_position_ms(AndampPlayer *player);

/// Moves the song to `ms` milliseconds.
void andamp_player_seek_to(AndampPlayer *player, int32_t ms);

/// Entry `index` of the playlist: its length in milliseconds, or -1 when there is none. The
/// pointers that are not null get its title, its artist, its album and its file's path, each to
/// free with andamp_string_free; a tag the file does not carry comes out empty.
int32_t andamp_player_entry(AndampPlayer *player, int32_t index, char **title, char **artist,
                            char **album, char **path);

/// Blocks of audio in which the output ran dry while a song went on, so far.
uint64_t andamp_player_underruns(AndampPlayer *player);

/// The rack as JSON, to free with andamp_string_free: {"effects":[...]}, each effect with id,
/// name, version, author, about, on, error (why it is left out of the stream playing, or
/// empty), params (each id, name, kind "number", "toggle" or "choice", min, max, default,
/// value, unit, help, scale, decimals or -1, zero, options of name and value) and presets
/// (each name and values as pairs of a control's place and a number).
char *andamp_player_effects(AndampPlayer *player);

/// Switches effect `index`: it fades in or out, so the switch does not click.
void andamp_player_set_effect_on(AndampPlayer *player, int32_t index, bool on);

/// Moves control `param` of effect `index` to `value`, in the control's own units.
void andamp_player_set_effect_value(AndampPlayer *player, int32_t index, int32_t param, float value);

/// Applies preset `preset` of effect `index`, or with -1 puts its controls back at their
/// defaults; either way in one step, so nothing sweeps.
void andamp_player_apply_preset(AndampPlayer *player, int32_t index, int32_t preset);

/// The samples heard since the last call, both sides mixed, oldest first, into `out`, which
/// has room for `max`; gives how many there were. Nothing while no song plays. For a visualizer
/// that takes every sample once.
int32_t andamp_player_take_wave(AndampPlayer *player, float *out, int32_t max);

/// Answers one of Winamp's keys, by its name: "z" previous, "x" play, "c" or "space" pause,
/// "v" stop, "b" next, "up" and "down" for the volume, "left" and "right" for five seconds
/// back and on. False for any other key.
bool andamp_player_key(AndampPlayer *player, const char *key);

/// Control `param` of effect `index` at `value` as the listener reads it ("50 %", "-3.0 dB"),
/// to free with andamp_string_free.
char *andamp_player_effect_readout(AndampPlayer *player, int32_t index, int32_t param, float value);

// The app. It owns the player, the settings and the skin worn; it knows the menu, what each
// command does, what the skin's buttons ask for, what a start puts back and what is kept for
// the next run.

/// The app.
typedef struct AndampApp AndampApp;

/// Makes the app from the `count` arguments of the command line at `args` (without the
/// program's name), with what comes with it in the folder `resources` (`skins`, `wasabi`,
/// `fonts`, `plugins`), on a screen of `screen_w` x `screen_h` points: the part the windows
/// move on. It reads the settings of the last run. Nothing shows until andamp_app_start.
/// Free it with andamp_app_free.
AndampApp *andamp_app_new(const char *const *args, int32_t count, const char *resources,
                          int32_t screen_w, int32_t screen_h);

/// Frees the app, with its skin and its player.
void andamp_app_free(AndampApp *app);

/// Wears the first skin. The playlist comes back, or what the command line names plays, at
/// the second andamp_app_step after it, so the skin shows before the songs are read.
void andamp_app_start(AndampApp *app);

/// The app's player, for the andamp_player_ calls. It lives as long as the app and is not to
/// be freed.
AndampPlayer *andamp_app_player(AndampApp *app);

/// The skin worn, for the andamp_skin_ calls, or null when none is. It is the app's: not to be
/// freed, and gone when another skin is worn, which the request `skin` says.
AndampSkin *andamp_app_skin(AndampApp *app);

/// The file name of the skin worn, to free with andamp_string_free.
char *andamp_app_skin_name(AndampApp *app);

/// Runs the skin and answers what its buttons asked for. ANDAMP_MOVED and ANDAMP_DRAWN are
/// set in the answer for what changed.
uint32_t andamp_app_step(AndampApp *app);

/// Milliseconds until the app next has something to do by itself, and so until the next
/// andamp_app_step: 0 for now, -1 for never.
int64_t andamp_app_next_due_ms(AndampApp *app);

/// What the app asked of the shell since the last call, or null for nothing: a line for each
/// request, its parts with tabs between. Free it with andamp_string_free.
///
/// - `quit`, `hide`, `about`
/// - `ask` and what to let the listener pick: `open_songs`, `add_songs`, `add_folders`,
///   `load_list`, `save_list`, `skin`, `presets`, or `url` for the address of a radio stream.
///   The answer goes to andamp_app_picked.
/// - `popup`, the number of one of the app's menus (andamp_app_menu) and, when there is one,
///   the place for it in points from the screen's top left corner; without a place it opens
///   at the mouse. The pick goes to andamp_app_command.
/// - `skin`: another skin is worn, and the windows of the one before are gone.
/// - `look`: the size, the opacity or staying on top changed.
/// - `menu`: the menu bar changed.
/// - `effects`: show the Effects window.
/// - `vis` and 1 to open the Visualizer window, 0 to close it; andamp_app_vis_shown says when
///   it is done.
/// - `preset`, 1 for blending into it, the window's title and the preset's file.
/// - `error` and a message for the listener.
char *andamp_app_take_requests(AndampApp *app);

/// The app's menus, by number.
#define ANDAMP_MENU_BAR 0
#define ANDAMP_MENU_POPUP 1
#define ANDAMP_MENU_EQ_PRESETS 2

/// One of the app's menus as lines of text, to free with andamp_string_free. A line is as a
/// skin's menu gives it (AndampMenuHandler), with the command's key after the text: a letter,
/// with `shift+` before it when Shift is held, to go with the system's command key.
char *andamp_app_menu(AndampApp *app, int32_t menu);

/// The commands a shell asks for by itself; the rest come as the numbers of menu items.
#define ANDAMP_COMMAND_PREVIOUS 10
#define ANDAMP_COMMAND_PLAY 11
#define ANDAMP_COMMAND_PAUSE 12
#define ANDAMP_COMMAND_STOP 13
#define ANDAMP_COMMAND_NEXT 14
#define ANDAMP_COMMAND_QUIT 26

/// Does the command with the number `id`, which an item of andamp_app_menu carries.
void andamp_app_command(AndampApp *app, int32_t id);

/// What the listener picked when a request `ask` asked for `what`: a line for each file or
/// folder, or the text they typed. Nothing picked needs no call.
void andamp_app_picked(AndampApp *app, const char *what, const char *answer);

/// What the system asks the app to open, or what was dropped on a window, a line for each: a
/// skin is worn; songs, folders, playlist files and radio streams play. Before
/// andamp_app_start it waits for the start.
void andamp_app_open(AndampApp *app, const char *locations);

/// Sets how the skin's own popup menus are shown, for the skin worn and every one after it;
/// `context` is handed back to `handler`, which stays callable for as long as the app lives.
void andamp_app_set_menu_handler(AndampApp *app, AndampMenuHandler handler, void *context);

/// Points to a layout pixel of the skin: 2 is Winamp's double size.
int32_t andamp_app_zoom(AndampApp *app);

/// Whether the windows stay above other apps' windows.
bool andamp_app_on_top(AndampApp *app);

/// How much of the windows shows over what is behind them, in percent: 20 to 100.
int32_t andamp_app_opacity(AndampApp *app);

/// The part of the screen the windows move on changed, to `w` x `h` points.
void andamp_app_set_screen(AndampApp *app, int32_t w, int32_t h);

/// The shell opened the Visualizer window (`open`) or it closed. When it opens, the app asks
/// for the first preset with a request `preset`.
void andamp_app_vis_shown(AndampApp *app, bool open);

/// A key in the Visualizer window, by its name: `right` the next preset, `left` the one
/// before, `r` one at random, `space` holds the one shown or lets it go. False for any other.
bool andamp_app_vis_key(AndampApp *app, const char *key);

/// Seconds a preset stays: the shell calls andamp_app_vis_tick that often while the
/// Visualizer window can be seen.
int32_t andamp_app_vis_stay(void);

/// A preset's stay is over: the next one blends in, unless the listener holds the one shown.
void andamp_app_vis_tick(AndampApp *app);

/// Loads every effect plug-in once, those that come with the app and the listener's own, and
/// puts each back as it was left: before the rack is shown.
void andamp_app_load_effects(AndampApp *app);

/// Takes the plug-in file at `file` into the rack and keeps a copy in the app's folder. Gives
/// its place in the rack, or -1 and, when `error` is not null, why in `*error`, to free with
/// andamp_string_free.
int32_t andamp_app_add_plugin(AndampApp *app, const char *file, char **error);

/// Whether effect `index` of the rack is a plug-in the listener added, which
/// andamp_app_remove_plugin can take out again.
bool andamp_app_effect_own(AndampApp *app, int32_t index);

/// Takes a plug-in the listener added out of the rack and out of the app's folder.
void andamp_app_remove_plugin(AndampApp *app, int32_t index);

/// Keeps what the next run starts from: the skin and where its windows are, the size, the
/// playlist, the volume, the equalizer, the effects. The shell calls it when the app goes to
/// the background and when it ends.
void andamp_app_save(AndampApp *app);

/// What the command line gave for one of the options the shell answers itself, to free with
/// andamp_string_free, or null when it was not given: `dump`, `bench`, `quit-after`,
/// `then-skin`, `presets` with their value, `hidden`, `show-vis`, `show-effects` with `1`, and
/// `click` with a line for each.
char *andamp_app_option(AndampApp *app, const char *name);

/// What the app would keep for the next run, in a line, to free with andamp_string_free.
char *andamp_app_summary(AndampApp *app);

#endif
