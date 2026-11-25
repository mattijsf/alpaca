# Alpaca

Winamp's skins as a music player: classic skins (`.wsz`) and modern ones (`.wal`) drawn by the
[walkit](https://github.com/mattijsf/walkit) engine, with an equalizer and an effect rack.
Formerly Andamp.

| Folder | What |
| --- | --- |
| [`core/`](core) | The shared core, in Rust: the app, the player, the effects and the skin engine's host, behind a C interface. `core/resources/` is what every app brings: the built-in skins, fonts and effect plug-ins. |
| [`apps/mac/`](apps/mac) | The Mac app, in Swift and AppKit, over the core. |
| [`apps/android/`](apps/android) | The Android app, in Kotlin, on Google Play's internal testing. It does not use the core yet. |
| [`apps/web/`](apps/web) | The web app, on the core built for WebAssembly. Not started. |
| [`tools/`](tools) | The gate and the scripts around it. |

[ENGINEERING.md](ENGINEERING.md) has the commands, the architecture and its rules;
[PLAN.md](PLAN.md) the order of work and where it stands; [PORTING.md](PORTING.md) what a new
app on the core has to do.

## License

GNU General Public License, version 3 or later; see [LICENSE](LICENSE). The fonts in
`core/resources/fonts` have their own licenses beside them.
