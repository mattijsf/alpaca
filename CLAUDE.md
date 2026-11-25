# Working in this repository

Read [ENGINEERING.md](ENGINEERING.md) first: the commands, the gate, the architecture's rules,
testing and code style. [PLAN.md](PLAN.md) has the order of work and the Status section, which
says where things stand; update Status and its log with every step you finish.

- The player comes first; music sources from other apps come last. Speed is the first measure.
- The player is for macOS, Windows and Linux ([PORTING.md](PORTING.md)). Whatever a second
  shell would have to write again goes in `core/`, with a test; a shell (`apps/mac/` is the Mac's)
  draws, opens panels and answers the app's requests, and decides nothing.
- The engine is the `walkit` crate from crates.io; between its releases `core/Cargo.toml` may
  pin a revision of its main branch (`[patch.crates-io]`). Do not change the engine from
  here; work with what it offers and note what is missing in PLAN.md.
- The shell is zsh: an unquoted `$var` with several words is not split. Run loops over such
  variables under `bash -c '…'`. A measurement in `spikes/README.md` was wrong once for this
  reason: the app got `--builtin --modern` as one argument.
- Check a change yourself before saying it works: `tools/gate.sh`, then the app with
  `--dump <folder>` and look at the pictures, or `--bench <seconds>` for cost. Run the app
  with `--volume 0` unless the owner asks to hear it.
- The gate builds a lot: when the disk is nearly full, `cargo clean` in `core/` gives back
  several gigabytes.
- Commits: conventional type, then a sentence that says what is now true, as in the history.
  No attribution lines. Branches are `type/topic`. Nothing is pushed unless asked.
- No Winamp material in the repository: no base skin, no skins from the Skin Museum. The
  built-in skins are original.
