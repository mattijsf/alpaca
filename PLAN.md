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
core/   Rust, a static library with a C interface (include/andamp_core.h)
          skin.rs      the engine with its windows set out apart; what each window shows
          host.rs      the engine's host: pictures decoded, text measured
          render.rs    the draw list into one buffer of pixels per window
          player/      decode, output, playlist, equalizer, what the visualizer reads
app/    Swift and AppKit: the windows, menus, files, the Mac's media keys
          project.yml  the Xcode project is made from it with XcodeGen
```

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

Measured on an M1 Max with the built-in skins, a track playing. The walkit example app this
grew from is the baseline (see [spikes/](spikes/README.md)).

| | Baseline | Aim |
| --- | --- | --- |
| Classic skin playing | 47% of a core | under 2% |
| Modern skin playing | 99% | under 3% |
| Paused or stopped, nothing moving | 4% | 0, no wakeups |
| Memory | 68 MB | under 50 MB |

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

- [ ] 1. Windows
- [ ] 2. Playback
- [ ] 3. Equalizer and visualizer
- [ ] 4. The Mac around it
- [ ] 5. Effects
- [ ] 6. Milkdrop and AVS
- [ ] 7. The App Store
- [ ] 8. Music sources

### Log

- 2026-11-23: plan written. Spikes done before it: a source in another app works through
  ExtensionKit; the example app's cost is where and how large it draws, not the engine.
