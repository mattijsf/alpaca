# Plan

Andamp for macOS: Winamp on the Mac's desktop, from real skin files, on the
[walkit](https://github.com/mattijsf/walkit) engine. The player itself comes first. Music
sources from other apps come last, on top of a player that is whole without them.

Speed is the first measure of every choice here: what the app costs while it plays, while it
sits still, and when it starts.

## Order of work

Each step ends with something that runs and is committed. [Status](#status) says where it stands.

1. **Windows.** Every window of a skin is a window of its own on the desktop: no frame, see
   through where the skin is, dragged by the skin's title bar, snapping and docking as Winamp
   does. Classic (`.wsz`) and modern (`.wal`) skins, the built-in skins, a skin of your own.
2. **Playback.** Files from disk, the playlist, play, pause, stop, next, previous, seek,
   volume and balance. The skin's readouts come from the player: title, time, bitrate.
3. **Equalizer and the skin's visualizer.** Ten bands and a preamp on the audio; the spectrum
   and oscilloscope from what is heard.
4. **The Mac around it.** Media keys and Now Playing, files and skins opened from Finder and
   by drop, the last skin, window places and playlist kept between runs, internet radio.
5. **Effects.** The effect rack and `.lua` plug-ins, as in Andamp on Android (`core/dsp`,
   `core/plugin` there, in Kotlin: a port).
6. **Milkdrop and AVS.** A visualizer window. projectM is LGPL and draws with OpenGL, which
   macOS has deprecated: how it links and what it draws through is decided here.
7. **The App Store.** Sandbox, signing, review.
8. **Music sources** as extensions other apps bring. [spikes/](spikes) holds what was
   measured for this.

## How it is built

```
core/   Rust, the same on every system: a crate, and a static library with a C interface
        (include/andamp_core.h)
          skin.rs      the engine with its windows set out apart; what each window shows
          host.rs      the engine's host: pictures decoded, text measured
          render.rs    the draw list into one buffer of pixels per window
          player/      decode, playlist, equalizer, what the visualizer reads
          player/device/  the audio device, the one part written for each system
          dsp/         effects: the Lua plug-in loader, the graph engine, the rack
          settings.rs  what is kept between runs, in one file
app/    the Mac's shell, in Swift and AppKit: the windows, menus, file pickers, media keys
          project.yml  the Xcode project is made from it with XcodeGen
vis/    builds libprojectM, the Milkdrop engine, as a library beside the app
```

[PORTING.md](PORTING.md) says what a shell for Windows or Linux has to do and what the core
already does for it.

The engine does the hard parts of the desktop already, and the app uses them as they are:

- `Presentation::free_windows` sets the windows out apart and moves, docks and snaps them by
  Winamp's rules. The app gives the engine the screen as its viewport and puts one `NSWindow`
  where the engine has each window. A drag is the engine's: the app passes the pointer in
  screen places and moves its windows to where the engine put them.
- `draw_windows_owned` hands out the same list for a window that did not change, so the app
  draws only the windows that did. `take_placed_only` says when windows only moved.
- `next_due_at` says when the skin next changes by itself. The app sleeps until then.

Drawing: each window is drawn at one device pixel per layout pixel into its own buffer, with
alpha, and shown through an `IOSurface` on a layer that scales by whole numbers with nearest
pixels. The picture is tagged sRGB, so the window server converts colors, not the app.

Audio: a decode thread fills a ring; the output callback empties it and applies the
equalizer, balance and volume, with no lock and no allocation. The visualizer reads a copy of
the last block.

## What "fast" means here

Measured on an M1 Max with the built-in skins, a song playing (`--bench`). The walkit example
app this grew from is the baseline (see [spikes/](spikes/README.md)).

| | Baseline | Aim | Now |
| --- | --- | --- | --- |
| Classic skin playing (AndAmp Dark) | 47% of a core | under 2% | 5.4%, at any zoom |
| Modern skin playing (walkit's) | 99% | under 3% | 5.0% |
| Bento playing, its one window 1196 by 798 | | | 8% |
| MMD3 playing, with its scripts | | | 10% |
| Playing with every window covered or the app hidden | | | 0.5% |
| Nothing playing | 4% | 0, no wakeups | 0.0%, no steps |
| Drawing one window | 4 to 20 ms a frame | | 16 to 45 microseconds |
| Memory (footprint, as Activity Monitor shows it) | 75 MB | under 50 MB | 25 MB |
| From the process starting to the first window | | under 150 ms | 190 to 250 ms |

An effect adds what `cargo run --release --example effectcost` prints for it, about three
times over in the app, where the audio thread runs cold: Bass & Treble 1%, all six that come
with the app 8%. Andamp on Android runs a graph a block at a time where nothing in it needs a
sample the block has not made yet (`BlockRunner`); the engine here runs a frame at a time, and
that is the first thing to port when an effect's cost matters.

Where the 5% goes: the engine's tick and draw lists 1.2%, drawing pixels 0.3%, the rest is
the 40 wakeups a second themselves (the timer, handing the layer its pixels, the window
server's turn, the audio thread) and decoding. The processor is asleep in all but a few of
every hundred samples; each wake runs cold. Only the part of a window that changed is drawn
and handed over (`render::damage` compares the draw list with the last one), which is what
keeps a large window like Bento's cheap. What is left is the engine's own work in a tick
that changes nothing but the visualizer. Of the start, 30 to 60 ms is the skin opening
(`--bench` prints it); the rest is the process and AppKit coming up.

## Working on it

```sh
core/build.sh                       # the Rust library, release
(cd core && cargo test)             # the engine side: windows, drags, the player
(cd app && xcodegen generate)       # after project.yml changes
xcodebuild -project app/Andamp.xcodeproj -scheme Andamp -configuration Release \
  -derivedDataPath build build
build/Build/Products/Release/Andamp.app/Contents/MacOS/Andamp [skin] [files...]
```

`--dump <folder>` writes every window as a PNG after two seconds and quits: the way to look
at a change without a person at the screen. `--bench <seconds>` prints frames drawn and time
spent drawing.

## Status

Updated with every step. Whoever picks this up, person or model, starts here.

- [x] 1. Windows: one desktop window for each of the skin's, drawn only when it changes;
  drags, docking and snapping by the engine; double size; always on top; the skin's close,
  minimize, menu and clutter bar buttons; the skin's popup menus as Mac menus.
  - [ ] A click on a clear part of a shaped window goes to the app, not to what is under it.
  - [ ] More than one screen: the windows move on the main screen's visible part only.
  - [x] A skin that builds on another finds it beside its own file: Bento reads Big Bento's
    scripts and pictures from `../Big Bento/`, and shows whole with `BigBento.wal` or a
    `Big Bento` folder next to `Bento.wal`. Checked with Bento, MMD3, Winamp3's default and
    CornerAmp from the local corpus.
- [x] 2. Playback: MP3, AAC, ALAC, FLAC, Ogg Vorbis, WAV and AIFF files; the playlist; play,
  pause, stop, next, previous, seek, shuffle, repeat; volume and balance; Winamp's keys.
  - [ ] The playlist window's own buttons (add, remove, select, sort) do nothing yet.
  - [ ] Gapless play from one song to the next; the song's sample rate changing reopens the output.
- [x] 3. Equalizer (ten bands, preamp) and the skin's spectrum and oscilloscope from what is heard.
- [x] 4. The Mac around it: the skin, its size and places, the volume, the playlist and the
  effects are kept between runs; media keys and Now Playing; Finder opens songs, folders,
  playlist files and skins with the app; files dropped on a window; nothing runs or draws
  while no window is in sight.
  - [x] Internet radio: an `http(s)` address in the playlist plays as a stream (MP3, AAC,
    Ogg, FLAC), with the station's name and the song's title from the stream. File > Add
    Radio Stream…, the playlist's Add menu, a playlist file, or the command line.
  - [x] The equalizer's factory presets, the mouse wheel for the volume, View > Opacity.
  - [ ] An icon.
  - [ ] Hovering, and the mouse wheel on a list: the engine has no call for either.
- [x] 5. Effects: Andamp's `.lua` plug-in format (`core/src/dsp`): the loader in a shut-in
  Lua, the checks, the program and the engine, with the arithmetic of the Kotlin original;
  the six plug-ins Andamp ships; the rack after the equalizer, each effect fading when
  switched; the Effects window (View > Effects…) with switches, presets, controls and
  Add Plug-in….
  - [ ] The `ui` block is read but not laid out: controls show in the order declared.
    `enabledBy` is not checked against the graph.
  - [ ] Andamp's built-in Karaoke, Modulation and Reverb are Kotlin graphs, not ported.
  - [ ] The block runner (see above).
- [ ] 6. Milkdrop and AVS
  - [x] Milkdrop in its first form: the Visualizer window (View > Visualizer, or the skin's
    button for it) draws `.milk` presets with libprojectM to what plays. The engine is a
    library of its own, `vis/lib/libprojectM-4.dylib`, built by `vis/build-projectm.sh` from a
    projectM 4.1 checkout (it needs CMake) and copied into the app when it is there; without
    it the app builds and runs, and the window says what is missing. Presets come from a
    folder the listener picks (View > Choose Milkdrop Presets…); none come with the app.
    Right and left arrows, R for a random one, space to keep the one shown.
  - [ ] What it costs: about 0.5% of a core for every frame a second (16% at the 30 it runs
    at, 33% at 60), at 640 by 400, most of it inside the system's OpenGL, which macOS runs
    on top of Metal. Not looked into further.
  - [ ] The window is a plain Mac window, not a skin's. A skin's own visualizer frame stays shut.
  - [ ] projectM's sources are not in this repository: `vis/projectm` as a submodule, or
    `PROJECTM_SRC`. It was built here from Andamp's own checkout of v4.1.7.
  - [ ] AVS: Andamp's is 15,000 lines of Kotlin on a framebuffer of its own, with the ns-eel
    evaluator in C. A port, not started.
- [ ] 7. The App Store
- [ ] 8. Music sources

### Other systems

- [x] The core compiles for Windows and Linux (`tools/cross-check.sh`: every line, its tests
  and examples, type-checked from the Mac; nothing linked or run there yet). The audio device
  is the one part with a version for each system; the portable one (`cpal`, with a sample
  rate converter) was also built into the Mac's app and measured there: the right speed, no
  underruns, the same cost.
- [x] Settings, what an opened file is, folders of songs, Winamp's keys and the effects'
  loading, state and readouts moved from Swift into the core.
- [ ] `.github/workflows/ci.yml` builds and tests on the three systems; it has never run,
  as the repository has no remote.
- [ ] Still in the Mac's shell, to move before a second shell is written: the app's menu and
  what its items do, what each of the skin's actions leads to, the Visualizer's presets and
  keys, the order of a start (PORTING.md lists them).
- [ ] A shell for Windows: not started. PORTING.md has what is known.
- [ ] The portable device's sample rate converter draws straight lines between samples; a
  better one can take its place behind the same two calls.

### What to do next, in this order

1. Look at it. Everything above was checked by tests, by pictures the app wrote of its own
   windows (`--dump`) and by measurements, with the sound turned down to nothing; nobody has
   yet dragged a window, heard a song or watched Milkdrop move. The first hour with it will
   find things.
   - Does a click on a clear part of a shaped window (MMD3's corner) reach what is under it?
   - Dragging, docking and snapping are the engine's, and tested through it; how they feel
     on the desktop is not.
   - The Effects and Visualizer windows are plain Mac windows.
2. What the engine could offer, since the rest of the cost is its own: a tick that says what
   changed without building the draw lists; the playlist as something that says when it
   changed (it is asked for whole, several times a tick); the mouse wheel and hovering; a
   skin's visualizer frame that the app can draw into.
3. The effects' block runner, when an effect's cost matters.
4. Milkdrop's cost, and its window as a skin's own.
5. Step 7, the App Store: the sandbox (files by bookmark, the network for radio), signing,
   an icon. The plug-in folder and the settings are already under the app's own names.
6. AVS.

What comes with the app: AndAmp Dark (the skin shown first, and the one under every classic
skin), Light and Spot, from Andamp's own `skin/dist`; walkit's two example skins; Andamp's
own element set under modern skins (`app/Resources/wasabi`, from Andamp's `defaultset`); the
stand-in fonts.

### Log

- 2026-11-23: plan written. Spikes done before it: a source in another app works through
  ExtensionKit; the example app's cost is where and how large it draws, not the engine.
- 2026-11-23, night: steps 1 to 3 in their first form. `core/` is the engine's host, a
  renderer that draws one window at one pixel per layout pixel, and the audio (Symphonia,
  the default output unit, a ring between them). `app/` is the AppKit shell. Tests in
  `core/tests` cover windows, drags, the skin's buttons, the player, the equalizer and the
  spectrum; none makes a sound. Checked by `--dump` with AndAmp Dark, walkit's two skins,
  two classic skins and four modern ones from the local test corpus.
- 2026-11-24, morning: prepared for Windows and Linux: the audio device behind one small
  interface with a portable version, the core cross-checked for both systems, and what every
  shell would write again moved out of Swift.
- 2026-11-24, night, later: damage-only drawing, Bento through its sibling skin, internet
  radio, the equalizer's presets, and Milkdrop through libprojectM in a window of its own.
- 2026-11-24, night: steps 4 and 5 in their first form: settings, media keys, Finder, the
  playlist window's buttons, and the effect rack with Andamp's plug-in format ported from
  Kotlin. The eleven plug-ins of Andamp (six shipped, five examples) load and build; the two
  made to be refused are refused.
