# Other systems

The player is built to be the same on macOS, Windows and Linux, with as little as possible
written again for each. This page says what is shared, what a system's shell has to do, and
what is known about Windows and Linux before anyone has run it there.

## What is shared

`core/` is everything that does not touch a window: the skin engine's host, the renderer, the
player, decoding, internet radio, the equalizer, the effects and their Lua plug-ins, the
settings file, and the app itself (`app.rs`): the command line, the menu and what each of its
items does, what the skin's buttons ask for, the Visualizer's presets, what a start puts back
and what a quit keeps. It has no line that knows its system outside two places:

- `core/src/player/device/`: the audio device. `coreaudio.rs` is the Mac's output unit;
  `portable.rs` is the `cpal` crate (WASAPI on Windows, ALSA on Linux) with a converter for a
  song at another sample rate than the device's. Both are the same four lines to the rest:
  open for a rate with something to fill blocks, start, stop.
- `core/src/host.rs`: the list of system fonts to fall back to when the app brought none.
  And `core/src/settings.rs` names the app's folder on each system.

`cargo test --release` runs the same tests everywhere; none opens the audio device or a window.
`tools/cross-check.sh` type-checks the core for Windows and Linux from a Mac, and
`.github/workflows/ci.yml` builds and tests it on all three. The portable audio device also
builds on a Mac, with `--features portable-output`: there it was run and measured, with the
right speed through its converter and no underruns.

What has not been done: nobody has linked or run the core on Windows or Linux. The checks
above compile every line for those systems and stop there.

## What a shell does

A shell is the part written for one system: `app/` is the Mac's, in Swift. One written in
Rust can use the core as a crate (`andamp_core::app::App`); one in another language uses the
C interface (`core/include/andamp_core.h`, the `andamp_app_` calls), which is what the Mac's
does.

The shell makes the app from its command line, the folder of what comes with it and the size
of the screen, and starts it. From then on it does three things: it steps the app and shows
the windows, it hands over the mouse, the keys and the picks from the menu, and it answers
what the app asks of it.

| A shell's job | On the Mac | What the core gives it |
| --- | --- | --- |
| One window for each of the skin's, with no frame, placed where the core says | `SkinWindow.swift`, `SkinController.apply` | each window's place, size and stacking order after every step |
| Showing a window's pixels, scaled by whole numbers | `SurfaceView` | BGRA with the color multiplied by the alpha, and the part that changed |
| The mouse, in places on the screen | `SkinController`'s mouse calls | `pointer_down`, `pointer_move`, `pointer_up`, `double_click`, `right_click`, and `andamp_app_wheel` for the wheel and the trackpad |
| Waking up when there is next something to do | `SkinController.schedule` | `andamp_app_next_due_ms`; nothing to do while no window is in sight |
| The keys | `SkinController.keyDown`, `Visualizer.key` | `andamp_app_key` and `andamp_app_vis_key` with the key's name |
| Drawing menus: the bar, the popups, a skin's own | `NSMenu.from(lines:)` | every menu as lines of text (`andamp_app_menu`), by number: the bar, the main popup, and the popups a skin's buttons ask for (options, visualization, the playlist's five buttons, a row's menu, sorting), with the checks, the grayed items and the keys; the pick goes back as a number (`andamp_app_command`) |
| Doing what the app asks | `AppDelegate.answer` | `andamp_app_take_requests`: quit, hide, about, show a menu at a place, show the Effects or Visualizer window, load a preset, say what went wrong |
| Letting the listener pick files, folders, a stream's address | `AppDelegate.ask` | the request `ask` with what to pick; the answer goes to `andamp_app_picked` |
| What the system opens, and what is dropped on a window | `application(_:open:)`, `SurfaceView` | `andamp_app_open` |
| The look: size, on top, opacity | `SkinController.answer` | the request `look`, then `andamp_app_zoom`, `_on_top`, `_opacity` |
| The system's media keys and "now playing" | `NowPlaying.swift` | the player's status, position and the current entry |
| The Preferences window | `Preferences.swift` | the pages and their rows as JSON (`andamp_app_preferences`), by kind: a switch, a choice, a slider, a button, text; a change goes back as an id and a value (`andamp_app_set_preference`) |
| File info, opening a file, showing one in its folder | `AppDelegate.answer` | the requests `info` (with `andamp_app_file_info` for the text), `open` and `reveal` |
| The Effects window | `Effects.swift` | the rack as JSON; setters for switches, controls and presets; the listener's own plug-ins kept by `andamp_app_add_plugin` |
| The Visualizer window | `Visualizer.swift` | the samples heard since the last frame, and which preset to show (`preset` requests); libprojectM draws |
| Saving when the app goes to the background or ends | `AppDelegate` | `andamp_app_save` |

What is left in the Mac's shell opens windows, panels and timers, and draws; it decides
nothing. The Effects window's controls are built in Swift from the rack's description, and a
second shell builds its own.

## Windows

- **Windows with holes**: a layered window (`WS_EX_LAYERED` with `UpdateLayeredWindow`) takes
  exactly what the core draws: 32-bit BGRA with the color multiplied by the alpha. A click on
  a clear pixel of such a window goes to what is under it, by the system's own rule.
- **Scaling**: the core draws one pixel for each layout pixel; the shell stretches by whole
  numbers with nearest pixels (`StretchBlt` in `COLORONCOLOR` mode into the layered window's
  bitmap, or a swap chain with point sampling). On a screen at 150% the choice is between 1x
  and 2x, as on the Mac.
- **A shell in Rust** with the `windows` crate is the least code: it uses the core as a
  crate, with no C interface between, and the one language builds the whole app with `cargo`.
- **Audio**: `portable.rs` asks WASAPI for its shared-mode format. It compiles; it has not
  been heard.
- **Milkdrop**: libprojectM builds on Windows with CMake and wants an OpenGL 3.3 context, which
  WGL gives. The Visualizer's logic is the same.
- **Media keys**: the System Media Transport Controls.
- **Build**: `cargo build --release` in `core/` needs a C compiler for the Lua interpreter and
  for the TLS library under internet radio; MSVC's or MinGW's both do.

## Linux

- **Windows with holes**: on X11, a 32-bit visual and a compositor. On Wayland a client cannot
  say where its windows go, so several windows that dock to each other are not possible
  there: either X11 (through XWayland), or one surface that holds all of the skin's windows,
  which the core can also lay out (`Presentation::free_windows` off).
- **Audio**: ALSA through `cpal`, which PipeWire and PulseAudio both answer. Building needs
  ALSA's headers (`libasound2-dev`).
- **Media keys**: MPRIS over D-Bus.
- **Milkdrop**: libprojectM with GLX or EGL.
- **Fonts**: the stand-in fonts come with the app, as on the Mac; `host.rs` lists where a
  system keeps Liberation Sans and DejaVu Sans for when they do not.
