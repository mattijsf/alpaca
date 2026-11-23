// SPDX-License-Identifier: GPL-3.0-or-later
//
// The C interface of the andamp-core library (core/src/lib.rs): a skin engine with its host,
// its windows set out apart on a screen, and a software renderer. Every call runs on the app's
// main thread.
#ifndef ANDAMP_CORE_H
#define ANDAMP_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/// A loaded skin.
typedef struct AndampSkin AndampSkin;

/// The app's player: the playlist, the transport and the audio. Every skin opened with it
/// shows and drives it.
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

/// andamp_skin_step: a window moved, opened, closed or changed its place in the order.
#define ANDAMP_MOVED 1u
/// andamp_skin_step: a window's pixels changed.
#define ANDAMP_DRAWN 2u

/// Opens the skin archive (.wsz or .wal) of `skin_len` bytes at `skin`, named `name` (its file
/// name), on a screen of `viewport_w` x `viewport_h` layout pixels. `base` is a classic skin
/// laid under a classic skin. Under a modern one goes `wasabi`, a zip of a wasabi folder, or
/// without it the files of the folder `wasabi_dir`. Each may be null. `fonts_dir` is a folder
/// of the stand-in fonts, or null. `beside_dir` is the folder the skin's file is in, where a skin
/// it builds on is looked for, or null. `player` is the player
/// the skin shows and drives; with null it shows a demo that plays nothing. On failure it
/// gives null and, when `error` is not null, a message in `*error` to free with
/// andamp_string_free.
AndampSkin *andamp_skin_open(const uint8_t *skin, size_t skin_len, const char *name,
                             const uint8_t *base, size_t base_len, const uint8_t *wasabi,
                             size_t wasabi_len, const char *wasabi_dir, const char *fonts_dir,
                             const char *beside_dir, int32_t viewport_w, int32_t viewport_h, AndampPlayer *player,
                             char **error);

/// Frees a skin andamp_skin_open gave.
void andamp_skin_free(AndampSkin *skin);

/// Frees a message andamp_skin_open gave.
void andamp_string_free(char *text);

/// Sets the screen the windows move on, `w` x `h` layout pixels.
void andamp_skin_set_viewport(AndampSkin *skin, int32_t w, int32_t h);

/// Moves every window by `dx`, `dy` layout pixels.
void andamp_skin_move_windows(AndampSkin *skin, int32_t dx, int32_t dy);

/// Puts window `id` with its top left corner at `x`, `y` on the viewport.
void andamp_skin_place_window(AndampSkin *skin, uint32_t id, int32_t x, int32_t y);

/// The id of the skin's main window, or 0 when it has none.
uint32_t andamp_skin_main_window(AndampSkin *skin);

/// Runs the engine's timers and animations and brings the windows up to date. The answer has
/// ANDAMP_MOVED and ANDAMP_DRAWN set for what changed.
uint32_t andamp_skin_step(AndampSkin *skin);

/// How many windows the skin shows.
int32_t andamp_skin_window_count(AndampSkin *skin);

/// Window `index`, counted from the bottom, into `*out`; false when there is none.
bool andamp_skin_window(AndampSkin *skin, int32_t index, AndampWindow *out);

/// Milliseconds until the skin next changes by itself: 0 for now, -1 for never.
int64_t andamp_skin_next_due_ms(AndampSkin *skin);

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

/// What the skin's buttons asked of the app since the last call, or null for nothing: one line
/// for each, the action (CLOSE, MINIMIZE, DOUBLESIZE, ALWAYS_ON_TOP, MENU and the like), its
/// param, and the place on the viewport a menu for it opens at as x and y, with tabs between;
/// an action from nothing on screen has no place. Free it with andamp_string_free.
char *andamp_skin_take_actions(AndampSkin *skin);

/// Tells the skin whether one of the app's switches is on, so its button lights up:
/// "doublesize" and "ontop" for a classic skin's clutter bar.
void andamp_skin_set_switch(AndampSkin *skin, const char *name, bool on);

/// Shows a skin's popup menu: `menu` is its items as lines of text (C, the id, 1 when checked,
/// 1 when disabled and the text, with tabs between, for a command; S for a separator; > and
/// the text where a submenu starts, < where it ends). It gives the id of the command picked,
/// or 0. It is called from inside a mouse call, and must not call the skin.
typedef int32_t (*AndampMenuHandler)(void *context, const char *menu);

/// Sets how the skin's popup menus are shown; `context` is handed back to `handler`. Both
/// stay valid for as long as the skin lives.
void andamp_skin_set_menu_handler(AndampSkin *skin, AndampMenuHandler handler, void *context);

/// Makes the app's player. Free it with andamp_player_free, after every skin opened with it.
AndampPlayer *andamp_player_new(void);

/// Frees a player andamp_player_new gave.
void andamp_player_free(AndampPlayer *player);

/// Adds the audio file at `path` to the end of the playlist. Gives its place in the list, or
/// -1 when the file cannot be read as audio.
int32_t andamp_player_add(AndampPlayer *player, const char *path);

/// Adds the songs the playlist file at `path` names (.m3u, .m3u8 or .pls). Gives how many
/// joined, or -1 when the file cannot be read.
int32_t andamp_player_add_list(AndampPlayer *player, const char *path);

/// Writes the playlist to `path` as an .m3u8 file; false when it cannot be written.
bool andamp_player_save_list(AndampPlayer *player, const char *path);

/// Empties the playlist and stops.
void andamp_player_clear(AndampPlayer *player);

/// How many entries the playlist has.
int32_t andamp_player_count(AndampPlayer *player);

/// Makes entry `index` current and plays it.
void andamp_player_play_index(AndampPlayer *player, int32_t index);

/// Makes entry `index` current without playing it, when nothing plays.
void andamp_player_select(AndampPlayer *player, int32_t index);

/// The transport, for andamp_player_command.
#define ANDAMP_PLAY 1
#define ANDAMP_PAUSE 2
#define ANDAMP_STOP 3
#define ANDAMP_NEXT 4
#define ANDAMP_PREVIOUS 5
void andamp_player_command(AndampPlayer *player, int32_t command);

/// Sets the volume, 0..255.
void andamp_player_set_volume(AndampPlayer *player, int32_t volume);

/// The volume, 0..255.
int32_t andamp_player_volume(AndampPlayer *player);

/// Moves the song by `ms` milliseconds, back for a negative number.
void andamp_player_seek_by(AndampPlayer *player, int32_t ms);

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

/// What the player asked the app for since the last call.
/// ANDAMP_REQUEST_OPEN_FILES: let the listener pick files to play.
#define ANDAMP_REQUEST_OPEN_FILES 1u
uint32_t andamp_player_take_requests(AndampPlayer *player);

/// Blocks of audio in which the output ran dry while a song went on, so far.
uint64_t andamp_player_underruns(AndampPlayer *player);

/// Adds an effect plug-in from the Lua source at `source`, off and at its defaults; a plug-in
/// with the id of one in the rack takes its place. Gives its place in the rack, or -1 with the
/// reason in `*error` (to free with andamp_string_free) when it is refused.
int32_t andamp_player_add_effect(AndampPlayer *player, const char *source, char **error);

/// Takes effect `index` out of the rack.
void andamp_player_remove_effect(AndampPlayer *player, int32_t index);

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

#endif
