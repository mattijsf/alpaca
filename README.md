# Andamp for macOS

Winamp on the Mac's desktop, from real skin files: every window of a skin is a window of its
own, dragged by the skin's title bar, docking and snapping as Winamp's do. It runs classic
skins (`.wsz`) and modern ones (`.wal`) on the [walkit](https://github.com/mattijsf/walkit)
engine, and plays your music through an equalizer and an effect rack.

It is early. [PLAN.md](PLAN.md) says what is done, what is not, and what comes next.

## What works

- The skin's windows on the desktop, see-through where the skin is, at Winamp's size or
  double. AndAmp Dark, Light and Spot come with the app; a skin of your own opens from the
  File menu, from Finder, or by dropping it on a window, and one put in the app's `Skins`
  folder (Preferences shows it) is in the Skins menu from then on.
- MP3, AAC, ALAC, FLAC, Ogg Vorbis, WAV and AIFF files, folders of them and `.m3u`, `.m3u8`
  and `.pls` playlists, and internet radio streams by their address. The playlist window's
  menus select, remove, crop and sort, drop dead files and duplicates, show a file's info
  and write the list as a web page.
- Preferences (Command and comma): the skin, the size, shuffle and repeat, the time shown,
  what the skin's visualizer shows and how, and the Milkdrop window's presets and pace.
- The ten-band equalizer, balance and volume, and the skin's spectrum and oscilloscope.
- Effects: Andamp's `.lua` plug-ins. Six come with the app (View > Effects…), and you can
  add your own.
- Milkdrop: the Visualizer window draws `.milk` presets with libprojectM, once that library
  is built (`vis/build-projectm.sh`; it needs CMake and a projectM checkout).
- The Mac's media keys and Now Playing. Winamp's keys: Z X C V B, the arrows for volume and
  for five seconds back and on.
- It keeps the skin, its size and places, the volume, the playlist and the effects between
  runs.

## Build and run

It needs Xcode, Rust (rustup's `cargo` in `~/.cargo/bin`) and
[XcodeGen](https://github.com/yonaskolb/XcodeGen).

```sh
(cd apps/mac && xcodegen generate)
xcodebuild -project apps/mac/Andamp.xcodeproj -scheme Andamp -configuration Release \
  -derivedDataPath build build
open build/Build/Products/Release/Andamp.app
```

From a terminal the app takes a skin and songs to start with:

```sh
APP=build/Build/Products/Release/Andamp.app/Contents/MacOS/Andamp
$APP ~/Music/album                 # a folder, a playlist file or songs: played at once
$APP skin.wal --wasabi base.zip    # a modern skin over a wasabi base of your own
$APP --dump out                    # writes every window as a PNG after two seconds, and quits
$APP --bench 10 song.flac          # what ten seconds of playing cost
```

## How it is built

`core/` is Rust: the skin engine's host, a renderer that draws each window into its own
buffer of pixels, the player (decoding, the output, the equalizer), the effect engine and
the app itself (its menu, its commands, what it keeps between runs), behind a C interface.
`apps/mac/` is Swift and AppKit: it shows the windows, draws the menu the core describes, opens
the file pickers the core asks for, and has the Mac's media keys.
[ENGINEERING.md](ENGINEERING.md) has the architecture, its rules and the commands;
[PLAN.md](PLAN.md) the order of work and where it stands.

The core is written to be the same on Windows and Linux, and compiles for both;
[PORTING.md](PORTING.md) says what a shell for another system has to do.

## License

GNU General Public License, version 3 or later; see [LICENSE](LICENSE). The fonts in
`core/resources/fonts` have their own licenses beside them.
