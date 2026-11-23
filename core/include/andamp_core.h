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

/// One window of a skin: where it is on the viewport in layout pixels, its `w * h` pixels
/// (rows top first, BGRA in memory, the color multiplied by the alpha) and a count that goes
/// up whenever the pixels are drawn again. The pixels stay valid until the next step.
typedef struct {
    uint32_t id;
    int32_t x, y, w, h;
    const uint32_t *pixels;
    uint64_t generation;
} AndampWindow;

/// andamp_skin_step: a window moved, opened, closed or changed its place in the order.
#define ANDAMP_MOVED 1u
/// andamp_skin_step: a window's pixels changed.
#define ANDAMP_DRAWN 2u

/// Opens the skin archive (.wsz or .wal) of `skin_len` bytes at `skin`, named `name` (its file
/// name), on a screen of `viewport_w` x `viewport_h` layout pixels. `base` is a classic skin
/// laid under a classic skin, `wasabi` a zip of a wasabi folder laid under a modern one; either
/// may be null. `fonts_dir` is a folder of the stand-in fonts, or null. On failure it gives
/// null and, when `error` is not null, a message in `*error` to free with andamp_string_free.
AndampSkin *andamp_skin_open(const uint8_t *skin, size_t skin_len, const char *name,
                             const uint8_t *base, size_t base_len, const uint8_t *wasabi,
                             size_t wasabi_len, const char *fonts_dir, int32_t viewport_w,
                             int32_t viewport_h, char **error);

/// Frees a skin andamp_skin_open gave.
void andamp_skin_free(AndampSkin *skin);

/// Frees a message andamp_skin_open gave.
void andamp_string_free(char *text);

/// Sets the screen the windows move on, `w` x `h` layout pixels.
void andamp_skin_set_viewport(AndampSkin *skin, int32_t w, int32_t h);

/// Moves every window by `dx`, `dy` layout pixels.
void andamp_skin_move_windows(AndampSkin *skin, int32_t dx, int32_t dy);

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

#endif
