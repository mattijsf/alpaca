# Other systems

The player is built to be the same on macOS, Windows and Linux, with as little as possible
written again for each. This page says what is shared, what a system's shell has to do, and
what is known about Windows and Linux before anyone has run it there.

## What is shared

`core/` is everything that does not touch a window: the skin engine's host, the renderer, the
player, decoding, internet radio, the equalizer, the effects and their Lua plug-ins, the
settings file, what a file the listener opens is, Winamp's keys. It has no line that knows its
system outside two places:

- `core/src/player/device/`: the audio device. `coreaudio.rs` is the Mac's output unit;
  `portable.rs` is the `cpal` crate (WASAPI on Windows, ALSA on Linux) with a converter for a
  song at another sample rate than the device's. Both are the same four lines to the rest:
  open for a rate with something to fill blocks, start, stop.
- `core/src/host.rs`: the list of system fonts to fall back to when the app brought none.
  And `core/src/settings.rs` names the app's folder on each system.

`cargo test` runs the same tests everywhere; none opens the audio device or a window.
`tools/cross-check.sh` type-checks the core for Windows and Linux from a Mac, and
`.github/workflows/ci.yml` builds and tests it on all three. The portable audio device also
builds on a Mac, with `--features portable-output`: there it was run and measured, with the
right speed through its converter and no underruns.

What has not been done: nobody has linked or run the core on Windows or Linux. The checks
above compile every line for those systems and stop there.

## What a shell does

A shell is the part written for one system: `app/` is the Mac's, in Swift. One written in
Rust can use the core as a crate (`andamp_core::skin::Skin`, `player::Player`,
`settings::Settings`); one in another language uses the C interface
(`core/include/andamp_core.h`), which is what the Mac's does.

| A shell's job | On the Mac | What the core gives it |
| --- | --- | --- |
| One window for each of the skin's, with no frame, placed where the core says | `SkinWindow.swift`, `SkinController.apply` | each window's place, size and stacking order after every step |
| Showing a window's pixels, scaled by whole numbers | `SurfaceView` | BGRA with the color multiplied by the alpha, and the part that changed |
| The mouse, in places on the screen | `SkinController`'s mouse calls | `pointer_down`, `pointer_move`, `pointer_up`, `double_click`, `right_click` |
| Waking up when the skin next changes | `SkinController.schedule` | `next_due_ms`; nothing to do while no window is in sight |
| The keys | `SkinController.keyDown` | `andamp_player_key` with the key's name |
| Menus: the skin's popups, the app's own | `showSkinMenu`, `main.swift` | a skin's menu as lines of text; the pick goes back as a number |
| What the skin's buttons ask for: close, minimize, double size, a file to open | `SkinController.act` | `take_actions`, a line for each |
| Picking files and folders | `main.swift` | `andamp_location_kind`, `andamp_player_add_location` |
| The system's media keys and "now playing" | `NowPlaying.swift` | the player's status, position and the current entry |
| The Effects window | `Effects.swift` | the rack as JSON; setters for switches, controls and presets |
| The Visualizer window | `Visualizer.swift` | the samples heard since the last frame; libprojectM draws |
| Keeping things between runs | `Settings.swift` | the settings file, read and written by the core |

Still in the Mac's shell and worth moving into the core before a second shell is written,
because every shell would write it again: which menu items the app has and what each does,
what to do with each of the skin's actions, the Visualizer's list of presets and its keys, and
the order in which a start puts things back. They are a few hundred lines of `main.swift`,
`SkinController.swift` and `Visualizer.swift` that decide and do not draw.

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
