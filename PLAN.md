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

[ENGINEERING.md](ENGINEERING.md) has the layers, the module layout, the rules and the commands;
[PORTING.md](PORTING.md) says what a shell for Windows or Linux has to do and what the core
already does for it.

The engine does the hard parts of the desktop already, and the app uses them as they are:

- `Presentation::free_windows` sets the windows out apart and moves, docks and snaps them by
  Winamp's rules. The app gives the engine the screen the main window is on as its viewport
  and puts one `NSWindow` where the engine has each window. A drag is the engine's: the app passes the pointer in
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

## Status

Updated with every step. Whoever picks this up, person or model, starts here.

- [x] 1. Windows: one desktop window for each of the skin's, drawn only when it changes;
  drags, docking and snapping by the engine; double size; always on top; the skin's close,
  minimize, menu and clutter bar buttons; the skin's popup menus as Mac menus.
  - [ ] A click on a clear part of a shaped window goes to the app, not to what is under it.
  - [x] More than one screen: the windows go onto any screen and come back on the screen and
    at the place they were left; a screen gone brings them back whole onto the main screen,
    a docked group together; no window is left where no screen shows it. They snap to the
    edges of the screen the main window is on.
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
  - [x] An icon.
  - [x] The mouse wheel on a list, classic and modern.
  - [ ] Hovering: the engine has no call for it.
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
- [x] The app itself is in the core (`app.rs`, `vis.rs`): the command line, the menu and
  what its items do, what each of the skin's actions leads to, the Visualizer's presets and
  keys, the order of a start, what is kept for the next run, the listener's own plug-ins. A
  shell draws the menu from lines of text and answers a short list of requests (quit, hide,
  let the listener pick a file, show a menu at a place). The Mac's shell lost about 600
  lines; what is left in Swift opens windows, panels and timers.
- [x] A refactor pass: the C interface is its own module (`ffi`, the only one with unsafe
  code) and lost the 41 functions no shell needs since the app moved into the core; the app
  module is split by concern; rustfmt, clippy's pedantic set, doc comments on every public
  item and Apple's swift-format are held by one gate (`tools/gate.sh`) that a pre-push hook
  runs. ENGINEERING.md has the rules.
- [x] The player's basics the first use found missing (2026-11-24). Every item of the
  playlist's menus does what it says: select all, none and invert; remove selected, crop,
  clear, dead files and duplicates; sorting by title, file name or path, reversing and
  randomizing; file info; the list as a web page; reading files again; a right click on a row;
  and a modern skin's five playlist buttons, which open these as menus. The clutter bar's O,
  I and V open the options, file info and the visualization menu. A Preferences window
  (Command and comma) holds the skin, the size, shuffle and repeat, the time shown, the classic
  visualizer's mode and style, and the Milkdrop window's presets, stay and frame rate; all of
  it is kept between runs. The core describes the window; the Mac draws it. The delete key
  removes the selected entries and return plays the first; in the playlist's window the
  arrows, page up and down, home and end move the selection, and the list follows it. The
  wheel and the trackpad scroll the playlist under them, classic or modern, and turn the
  volume over the volume slider, and do nothing elsewhere. Command-` goes through the app's
  windows, and the skin draws the one the keys go to as the one in front. A classic playlist follows the
  song that plays: when another song becomes current the list brings it into view. A press
  too short to see, as a tap on a trackpad, still shows the button pressed for a moment. On
  the Mac the skin's windows are one group around the main one, which Mission Control shows
  as one, and the app has Andamp's icon.
- [x] The Mac's shell tells the app where its screens are, and again when one comes, goes or
  changes (`andamp_app_set_screens`).
- [ ] A shell for Windows: not started. PORTING.md has what is known.
- [ ] The portable device's sample rate converter draws straight lines between samples; a
  better one can take its place behind the same two calls.

### Known about the engine

- The engine is walkit's main branch as of 2026-11-24 (`[patch.crates-io]` in
  `core/Cargo.toml`, a revision by its number), ahead of release 0.10.4 by what the app
  uses: a row of a classic playlist brought into view (`show_classic_playlist_row`), the
  list under a point scrolled (`scroll_at`), a frame's divider dragged, the cursor that
  belongs at a point (`pointer_shape_at`), the focus given to a window (`focus_window`) and a wider snap for
  the equalizer's and the balance's sliders (`Presentation::snap_reach`).
  The owner expects more changes to walkit before its next release; with that release the
  pin goes.
- The same revision has the fix for the engine's own check, under debug assertions, that a
  window drawn in parts equals the window drawn whole: 0.10.4 stopped two tests of
  `tests/windows.rs` at it. The check can still trip when the player's own time moves
  between its two paintings, such as the second changing, so the gate tests in release. The
  tests were not run in debug again with this revision.
- A window's new place (`place_window`) shows in `window_rect` after the next tick.
- The engine does not say how many rows a classic playlist shows or how high they are. A
  page of the list, for page up and page down, is counted with rows of 16 pixels at most
  (`app/playlist.rs`), so it is never more than a screen and can be a row or two less.
- Nothing says which control is under a point but for a list (`scroll_at`): a modern
  skin's volume slider is found by the id `volume`, and one a skin names otherwise does not
  turn by the wheel.
- The engine has one viewport, and a window snaps to its edges only: the app gives it the
  screen the main window is on. A window apart on another screen snaps to the other windows
  there, not to that screen's edges.
- The engine has no call that takes the focus from every window: when another app takes
  the keys, the title bar lit last stays lit, where Winamp's all show as behind. Before the
  first window is given the keys none is lit.

### What to do next, in this order

1. Looked at, by the owner on 2026-11-24, with the build of that day: dragging, docking and
   snapping feel right; a click on a clear corner of a shaped window reaches what is under it;
   it sounds right, and the equalizer and an effect do what they should; the Effects and
   Visualizer windows are fine as plain Mac windows, so Milkdrop in a skin's own window can
   wait.
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
stand-in fonts, drawn only where the system has no Arial or Tahoma (`core/src/fonts.rs`).

### Log

- 2026-11-24, right after: the owner asked for one place for the player, not one for each
  skin. Another skin now opens with its main window at the corner of the one before, in the
  same run and the next (`corner` and `screen` in the settings); what a skin keeps for
  itself is only how its other windows sit around its main window. Places kept by skin
  before are read once, for the skin worn at the start. Tested in the core
  (`every_skin_opens_where_the_last_one_was`).
- 2026-11-24, after the skins' state: the owner found the player did not come back on the
  screen it was left on. The Mac's shell took the screen with the keys at the start
  (`NSScreen.main`) as the only one, and places were kept from its top left: with the player
  on the laptop's screen left of a monitor, a restart put it at the monitor's left edge (seen
  here: kept at -1000,300, back at 0,300), and with the keys on the other screen at the start
  every place counted from that screen instead. Now the shell reports every screen as a
  rectangle on one desktop and again when they change; the core keeps the places on that
  desktop with the screen the main window was on, gives the engine that screen as its
  viewport, and keeps every window where a screen shows it (`app/screens.rs`): a group of
  windows that touch moves as one, from a screen gone to one of its size if there is one,
  else to the main screen, at the place it had there and then whole. Places kept before
  count from the main screen's top left. Tested in the core with two screens
  (`tests/screens.rs`). On the Mac it was checked with this Mac's two screens by starting the
  app with places on each; the screens' coming and going while it runs only in the core.
- 2026-11-24, after that: the owner found the wheel dead on a modern skin. Big Bento shows
  its playlist inside its one window, and the app scrolled only over a playlist window. The
  wheel now asks the engine for the list under the point (`scroll_at`, written in walkit's
  branch `feat/scroll-list-under-point`): a playlist wherever a skin shows it, a list of
  color themes too. Checked in the core on Winamp3x, Winamp Modern, MMD3 and Big Bento: on
  each a list is found where it shows, and a turn of the wheel draws it anew.
- 2026-11-24, after that again: the owner found that Bento's panels cannot be resized,
  where Winamp's can: the engine drew a frame's divider and had no drag for it. Written in
  walkit's branch `feat/frame-divider-drag`: the divider follows the pointer between the
  frame's limits. Checked in the core on Big Bento: the playlist's pane goes from 35 pixels
  wide to all but 141 of its row. Not compared with Winamp in the VM, which did not answer:
  whether 5.666 moves the divider in steps, and what `jump` does below the minimum. The
  place of a divider is not kept between runs.
- 2026-11-24, and again: the owner found the divider thin to take and without a cursor. It
  is 8 pixels, as Winamp lays it out; a press now takes it from up to 6 pixels beside it
  where nothing else takes the press (the engine's touch slop), and the engine says which
  cursor belongs at a point (`pointer_shape_at`): left and right or up and down over a
  divider, and the same or a diagonal over a window's resize grip. The Mac's shell follows
  the mouse over the skin's windows and sets it. Checked in the core on Big Bento and with
  a test on a classic playlist's corner; the cursor itself was not seen on the screen.
- 2026-11-24, once more: two things the owner found. A classic skin's own cursors, moving
  ones too: the engine had them, the app now says which belongs where
  (`andamp_app_cursor`, 100 and up) and gives its pictures, and the Mac's shell makes
  cursors of them. Tested in the core with a skin given a cursor file; no cursor was seen
  on the screen by me. And a skin's windows came back twice as far apart, or half, when
  the size had changed while another skin was worn: places were kept in points only. The
  size is now kept with them (`at.<skin>.zoom`), and the windows other than the main one
  come back as far from it in the skin's own pixels as they were. Places kept before this
  have no size with them and are read as kept at the plain size.
- 2026-11-24, later still: the owner saw the cursor change at a slightly other place in
  Winamp. Read from how Winamp 5.666's frame works: the divider is taken and shows its
  cursor on its 8 pixels and nowhere beside them; dragged, its edge goes under the pointer,
  pixel by pixel; under its minimum a pane counted from the right shuts to nothing. walkit
  now does the same, and the app tells the engine a mouse drives it (a touch slop of 0), so
  a press means what is under it for every control. Not seen in the VM, which did not
  answer; one over the other, a pane still stops at its minimum.
- 2026-11-24, at the end: Command-` went nowhere. The Mac leaves a window that belongs to
  another's group out of its cycling, and the skin's windows are one group since Mission
  Control shows them as one. The shell now goes through the app's windows itself, the
  skin's from the top down and then the others, back with Shift. Checked by posting the
  key to a copy of the app: main, equalizer, playlist and round, and back; not by a hand on
  the keys. It answers the key left of 1 and the character it types, not another key the
  listener may have set for it in the system's settings.
- 2026-11-24, the last entry of the day: the equalizer's sliders seemed not to snap to
  flat. They did, as Winamp's do: on a classic skin within a pixel of the middle. The owner
  asked for more, and for the balance too. walkit has a setting for it
  (`Presentation::snap_reach`), and the app asks for
  two pixels either side. Four, about
  what a modern skin's sliders have by themselves, was tried first and held on too long
  for the owner.
- 2026-11-24, one more: a skin is found as it was left. The app kept nothing of what a
  skin keeps for itself; now what its scripts wrote down (a drawer, a tab, a window's
  size) and its color theme are in the settings by the skin's file name (`kept.<skin>.`),
  written when the skin goes and at a save, and handed to the engine before the skin
  loads. A divider's place is the engine's to keep: by the frame's id, as Winamp keeps it. Checked in the core:
  Big Bento's playlist pane and Winamp Modern's color theme come back after a restart.
- 2026-11-24, the playlist's rules from Winamp's code: a modern skin's playlist in Winamp
  5.666 is Winamp's own playlist window, embedded and drawn in the skin's colors, so both
  kinds behave alike. A selection the keys move is brought into view as little as it must;
  the song that plays, when it does not show, comes into the middle. walkit has a call for
  each, for both kinds; the app used the first rule
  for both before. Checked in the core with walkit's modern example skin and the classic
  one; not on Big Bento, whose playlist is in its main window, where the arrow keys still
  turn the volume.
- 2026-11-24, after the playlist's rules: a skin's text is drawn in the real Arial and Tahoma, as walkit-cli
  draws it when given them (`--fonts`), which is how its text comes to match Winamp's. Before,
  Andamp's text was already walkit-cli's without them, pixel for pixel: the same engine
  drawing, the same free stand-ins (Liberation Sans for Arial, DejaVu Sans Condensed for
  Tahoma) under the same names. The stand-ins have Arial's widths but not its letters. macOS
  has both real fonts (`/System/Library/Fonts/Supplemental`, Monotype's hinted cuts with the
  `gasp` ranges Windows uses), Windows has them, Linux has Arial with Microsoft's core fonts:
  the core reads them from there when a skin first draws text, keeps them for every skin
  after, and draws the stand-ins where they are missing (`core/src/fonts.rs`). Nothing of
  them is in the repository. Compared with walkit-cli (its main branch, 0.1.4 and on) given
  the Mac's fonts, as Winamp 5.666: the playlist's rows of base-2.91, AndAmp Dark and
  walkit-classic are the same to the pixel, and so is the title text of Winamp3x, Winamp
  Modern and walkit-modern; Big Bento's tabs are in Tahoma as walkit-cli draws them; MMD3
  brings its own fonts and does not change. What still differs is the player's state, not text: the selected row, the
  time, the bitrate, the visualizer. Cost: the same, within what the bench can tell on a
  screen that was partly covered (Winamp Modern playing 5.5 to 8.1% of a core before, 7.4%
  after; walkit-modern 2.1 to 5.4% before, 4.6% after; idle no steps in either; the first
  window 150 to 200 ms in both).
- 2026-11-24, a playlist inside another window: Bento's playlist did not take the arrow
  keys. In Winamp the playlist inside a skin is its own window, which takes the keyboard on
  a click (its window procedure gives itself the focus on a left button press), and the
  owner saw the same in the VM. walkit now says which part of a skin has the keys
  (`focused_part`), and a modern playlist selects several rows by Command and Shift as a
  classic one does. The app sends the keys
  there; Shift with the arrows selects a run, Command and Shift with a click as in
  Winamp's playlist. Select All, Invert, Delete and Return already reached the playlist
  from any window. Checked in the core, also on Big Bento; not by hand.
- 2026-11-24, last of all: a folder of the listener's own skins (`Skins` in the app's
  folder, beside the settings). What is in it is offered in the Skins menu and in the
  Preferences window under the skins that come with the app; Preferences has a button that
  shows the folder. The owner asked for Winamp3x, Winamp Modern, MMD3 and Big Bento: they
  are in that folder on this Mac, not in the repository, and each was looked at with
  `--dump`. A file added while the app runs is in the menu bar when the bar is next made
  anew; the popup menus and the Preferences window read the folder each time.
- 2026-11-24, last: the engine pinned to walkit's main branch for its three new calls. The
  title bars follow the window the keys go to (Command-`), a modern skin's playlist scrolls
  by the wheel, and the app no longer counts a classic playlist's scrolling itself: the
  engine brings a row into view from wherever the list stands, a drag of its scroll bar
  included. Checked with `--key-window` and `--dump` on walkit's classic skin: the title bar
  of the window given the keys is lit and the other's is not, for the main window and for
  the playlist. Command-` itself was not tried by a hand on the keys, and the wheel on a
  modern list only in a test.
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
- 2026-11-24, night: the playlist's menus and a Preferences window, after the owner found
  both missing. The playing cost was not measured again: the screen was out of sight, and the
  step and the renderer were not touched.
- 2026-11-24, evening: the owner used the app for the first time and found nothing wrong.
- 2026-11-24, later still: a refactor pass and the gate; ENGINEERING.md written. No change to
  what the app does; cost and start-up measured again.
- 2026-11-24, later: the app itself moved into the core, behind `andamp_app_`: the Mac's
  shell asks the core what the menu is and does what the core requests. Start-up, cost and
  what a restart brings back were measured again and are as before. The songs of a start
  are now read after the skin's first drawing, also when the command line names them.
- 2026-11-24, night, later: damage-only drawing, Bento through its sibling skin, internet
  radio, the equalizer's presets, and Milkdrop through libprojectM in a window of its own.
- 2026-11-24, night: steps 4 and 5 in their first form: settings, media keys, Finder, the
  playlist window's buttons, and the effect rack with Andamp's plug-in format ported from
  Kotlin. The eleven plug-ins of Andamp (six shipped, five examples) load and build; the two
  made to be refused are refused.
