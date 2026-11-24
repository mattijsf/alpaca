# Andamp for the desktop: engineering notes

Andamp for the desktop is Winamp's player on macOS, and later Windows and Linux: classic and
modern skins run by the walkit engine, each of a skin's windows a window of its own on the
desktop, with a real player behind them. It is built in two parts. `core/` is a Rust crate
that is the app, the same on every system. A shell is the part written for one system; `app/`
is the Mac's, in Swift.

Two things decide most questions here. Speed is the first measure: an idle player costs
nothing, and a playing one a few percent of a core. And whatever a second shell would have to
write again belongs in the core.

[PLAN.md](PLAN.md) has the order of work and where it stands. [PORTING.md](PORTING.md) has
what a shell for another system has to do.

## Commands

```bash
tools/gate.sh                         # the push gate: format, lints, docs, tests, header, Swift
(cd core && cargo test --release)     # the tests alone
(cd core && cargo fmt)                # format the Rust
xcrun swift-format format -i -r app/Sources   # format the Swift
tools/cross-check.sh                  # the core type-checked for Windows and Linux (needs zig)
(cd app && xcodegen generate)         # the Xcode project, after project.yml changes
xcodebuild -project app/Andamp.xcodeproj -scheme Andamp -configuration Release \
  -derivedDataPath build build        # the Mac's app; it builds the core itself (core/build.sh)
build/Build/Products/Release/Andamp.app/Contents/MacOS/Andamp [skin] [songs...]
```

`--dump <folder>` writes every window as a PNG after two seconds and quits: the way to look at
a change with nobody at the screen. `--bench <seconds>` prints what that stretch cost. Both
start from no settings and leave none. Add `--volume 0` to every run that is not for
listening.

### Setup

It needs Xcode, rustup's Rust (`rust-toolchain.toml` names the toolchain and its components)
and [XcodeGen](https://github.com/yonaskolb/XcodeGen). Run `lefthook install` once
(`brew install lefthook`): it wires the pre-push hook, which runs the gate. If the hook
fails, fix the problem; do not push with `--no-verify`.

Two things are optional. zig is the C compiler `tools/cross-check.sh` uses for the other
systems; the gate runs the cross-check where zig is there. CMake builds libprojectM for the
Visualizer (`vis/build-projectm.sh`); the app builds and runs without it.

### The gate

`tools/gate.sh` runs, in order:

- `cargo fmt --check`
- `cargo clippy --all-targets` with warnings as errors, and once more on a Mac with the
  `portable-output` feature, so the audio device every other system uses is linted too
- `cargo doc` with warnings as errors, which finds a doc link that leads nowhere
- `cargo test --release`
- a check that `core/include/andamp_core.h` and `core/src/ffi` name the same functions
- `swift-format lint --strict` on the Mac's shell
- `tools/cross-check.sh`, where zig is installed

It is one script so the hook, CI and this file cannot disagree about what it contains. CI
(`.github/workflows/ci.yml`) runs only when started by hand: the gate on a Mac, the core's
tests on Windows and Linux, and a build of the Mac's app. The gate does not build the app.

The tests run in release: with debug assertions on, a check inside walkit 0.10.4 stops two
window tests (PLAN.md, Known about the engine).

## Architecture

### Layers and dependency direction

```
shell ──▶ ffi ──▶ app ──┬──▶ skin ──▶ host, render ──▶ walkit (the engine, a crate)
(app/)                  ├──▶ player ──▶ decode, output ──▶ device  (one per system)
                        │       └──▶ dsp: lua ──▶ graph ──▶ rack
                        └──▶ settings, vis
```

- **A shell depends on `app` and on nothing behind it.** A shell in Rust uses
  `andamp_core::app::App`; one in another language uses `ffi`, the same thing in C. A shell
  does not open a skin, read a setting or build a playlist itself: it asks the app.
- **`app` decides, the shell does.** The app knows the command line, the menu, what each
  command does, what a skin's buttons ask for, what a start puts back and what a quit keeps.
  What only the system can do it asks of the shell as a `Request`: quit, hide, let the
  listener pick a file, show a menu at a place, show a window. A shell draws, opens panels
  and keeps timers. The test for where a line goes: would a shell for Windows have to write
  it again? Then it is the core's.
- **`ffi` holds no logic.** Every function turns its pointers into Rust values, calls one
  method and turns the answer back. What it needs to decide belongs in the module it calls.
- **`player`, `dsp`, `settings` and `vis` know nothing of skins**, and nothing depends on
  `app` but `ffi`. `skin` holds the player for two things only: to let it move on before a
  step, and to answer the playlist window's own buttons.
- **The engine is walkit from crates.io.** It is not changed from here. What it lacks is
  noted in PLAN.md and worked around in `skin` or `host`.
- **Only three places know their system**: `player/device/` (the audio device), the list of
  system fonts in `host.rs`, and the app's folder in `settings.rs`. A `cfg(target_os)`
  anywhere else is wrong.

### Module layout

| Path | What it is |
| --- | --- |
| `core/src/app/` | The app: `mod.rs` the start, the look and saving; `options.rs` the command line; `request.rs` the commands' numbers and what the app asks of a shell; `menu.rs` the menus, the commands and what a pick leads to; `worn.rs` the skin worn, its windows' places, the step and the skin's buttons; `visualizer.rs` and `effects.rs` those two windows as the app sees them. |
| `core/src/skin.rs` | The engine running one skin, its windows set out apart, each drawn into its own pixels. |
| `core/src/host.rs` | What the engine asks of an app: pictures decoded, text measured, actions and switches. |
| `core/src/render.rs` | A window's draw list into pixels: only what changed, with shapes as runs of rows. |
| `core/src/player/` | The playlist and transport (`mod.rs`), decoding and radio (`decode.rs`), what the audio thread runs (`output.rs`), the audio device (`device/`). |
| `core/src/dsp/` | Effects: Lua plug-ins (`lua.rs`) describe a graph, `graph.rs` runs it, `rack.rs` is the chain the audio passes through, `describe.rs` spells the rack out for an Effects window. |
| `core/src/settings.rs` | The one file kept between runs. |
| `core/src/vis.rs` | The Visualizer's presets: which shows, how long, what its keys do. |
| `core/src/ffi/` | The C interface, by what it takes: `app.rs`, `player.rs`, `skin.rs`. `core/include/andamp_core.h` is written by hand, in step with it. |
| `core/tests/` | The tests, a file to an area: `app`, `windows`, `player`, `effects`, `settings`. |
| `app/Sources/` | The Mac's shell: `main.swift` answers the app's requests and draws its menus; `SkinController.swift` steps the app and places the windows; `SkinWindow.swift` shows pixels; `Effects.swift`, `Visualizer.swift` and `NowPlaying.swift` are the Mac's own windows and media keys; `Checks.swift` is `--dump` and `--bench`. |
| `vis/` | Builds libprojectM as a library beside the app. |
| `tools/` | The gate and the cross-check. |
| `spikes/` | Experiments kept for their findings; outside the gate. |

**The split rule.** A file that holds two concerns is split by concern, not by length: the
`app` module is one `impl App` over several files. A second shell gets a folder beside
`app/`. The core becomes a workspace only when a second crate needs it, such as a shell in
Rust.

### Threads

- Everything runs on the one thread the shell made the app on, but the audio. The app, the
  skin and the player are `Rc` and `RefCell` on purpose: they are not shared.
- A song is decoded on its own thread, which fills a ring. The device's callback empties it
  and applies the equalizer, the effects, the balance and the volume.
- **The audio callback takes no lock and allocates nothing.** What the listener changes
  reaches it as atomics (`output::Shared`) or as commands through a ring (`dsp::rack`). A
  chain of effects the rack swaps out is handed back to the main thread to be dropped there.

### State

- **The app owns what lasts**: the player, the settings, the skin worn, the size, staying on
  top, the opacity, the Visualizer's presets, which plug-ins are the listener's own. A shell
  owns only what dies with it: windows, timers, a menu being shown.
- **One place writes the settings**: `App::save`, which the shell calls when the app goes to
  the background and when it ends. A run that only looks or measures (`--dump`, `--bench`,
  `--click`) reads no settings and leaves none.
- **Places on the screen are in points from the top left corner** of the part of the screen
  the windows move on, the same on every system; a layout pixel is `zoom` points. A shell
  turns them into its system's own.
- **The shell learns of a change by a request**, not by asking: `skin` when another is worn,
  `look` when the size, the opacity or staying on top changed, `menu` when the bar did.

### Speed

- **Nothing runs when nothing changes.** The shell sleeps until `next_due_ms`, and does not
  step the app at all while no window is in sight; a playing song is then only asked twice a
  second whether it is over.
- **Only what changed is drawn**, and only the changed part of a window is handed over.
- **The first window comes before everything else.** The songs of a start are read after
  the skin's first drawing; plug-ins load when the Effects window opens, or at the start only
  when one was left on.
- **A change to the step, the renderer or the audio path is measured before and after** with
  `--bench`, playing and idle, and PLAN.md's table is brought up to date.

## Testing

A change is done when the gate passes and, where it shows, the app was looked at with
`--dump`.

- **Every mechanical bug gets a regression test.** Write the failing test first, then fix.
  Mechanical means deterministic: parsing, places, value mapping, what a command does. If a
  bug cannot be caught in a test of the core, the logic is in the wrong place: move it there.
- **A feature ships with its tests in the same change.**
- **No test opens a window or the audio device, and none makes a sound.** A test's player is
  `Player::silent()`, which decodes and keeps time and plays nothing. So the same tests run
  on every system.
- **A test makes its own files** in a scratch folder: songs as WAV bytes, settings, presets.
  The skins are the ones that come with the app.
- **What only a shell can show is checked through the app**: `--dump` for pictures,
  `--bench` for cost, `--click` and `--quit-after` for what a click or a restart does.
  What nobody has checked is said so in PLAN.md.

## Code style

- **Rust formatting** is rustfmt with its defaults; there is no `rustfmt.toml`.
- **Rust lints** are clippy's pedantic set, with the list of exceptions in `core/Cargo.toml`,
  the same list walkit uses. For a new finding, fix the code. If the rule does not fit,
  change the list with a comment that says why. No `#[allow]` in the code without a comment.
- **Every public item has a doc comment** (`#![warn(missing_docs)]`).
- **Unsafe code is in `ffi` only** (`#![deny(unsafe_code)]` on the crate). Every unsafe
  function says what its caller promises under `# Safety`.
- **A panic stops at the C boundary**: the call gives its fallback answer, and a skin whose
  engine panicked does nothing more. Library code does not `unwrap` what can fail at run
  time; what can go wrong for the listener is a `Result` with a message they can read.
- **Swift formatting** is Apple's swift-format (`.swift-format`: 4 spaces, 120 columns), which
  comes with Xcode. The shell's code opens, draws and forwards; an `if` that decides what the
  app does is in the wrong language.
- **The C header is written by hand.** A function added to `ffi` is added to the header with
  the same words; the gate checks the names.
- **Comments and docs** say what the code does and why, in plain words. Every source file
  starts with the SPDX line.
- **Commits**: a conventional type, then a sentence that says what is now true. No
  attribution lines. Branches are `type/topic`. Nothing is pushed unless asked.

## Rules

- No Winamp material in the repository: no base skin, no skins from the Skin Museum. The
  skins that come with the app are original.
- The player comes first; music sources from other apps come last, as extensions.
- Skins and plug-ins are untrusted input. A skin's script runs inside the engine's limits; a
  plug-in runs in a Lua sandbox that can only describe a graph, and the graph is checked
  before it is built.
- Run `tools/gate.sh` before every push; the pre-push hook does.
- When a step is finished, PLAN.md's Status and log are brought up to date in the same
  change.
