# Working in this repository

Read [PLAN.md](PLAN.md) first: the order of work, how it is built, and the Status section,
which says where things stand. Update Status and its log with every step you finish.

- The player comes first; music sources from other apps come last. Speed is the first measure.
- The player is for macOS, Windows and Linux ([PORTING.md](PORTING.md)). Whatever does not
  draw a window or call the system goes in `core/`, with a test; a shell (`app/` is the
  Mac's) only does what its system alone can: it asks `core/src/app.rs` what the menu is,
  hands it the picks, and answers its requests. In `core/`, only `player/device/` and the font
  and folder lists may know their system. After a change to `core/`, `tools/cross-check.sh`
  says whether it still compiles for Windows and Linux (it needs zig).
- The engine is the `walkit` crate from crates.io. Do not change the engine from here; work
  with what it offers and note what is missing in PLAN.md.
- The shell is zsh: an unquoted `$var` with several words is not split. Run loops over such
  variables under `bash -c '…'`. A measurement in `spikes/README.md` was wrong once for this
  reason: the app got `--builtin --modern` as one argument.
- Check a change yourself before saying it works: `cargo test --release` in `core/` (without
  `--release` two window tests stop at a check inside the engine; see PLAN.md), then the app with
  `--dump <folder>` and look at the pictures, or `--bench <seconds>` for cost.
- Commits: conventional type, then a sentence that says what is now true, as in the history.
  No attribution lines. Branches are `type/topic`. Nothing is pushed unless asked.
- Comments and docs say what the code does and why, in plain words; every source file starts
  with the SPDX line.
- No Winamp material in the repository: no base skin, no skins from the Skin Museum. The
  built-in skins are original.
