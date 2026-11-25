#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The push gate: formatting, lints with warnings as errors, the docs' links, every test, that
# the C header and the Rust behind it name the same functions, and the Swift shell's format.
# The pre-push hook runs it (lefthook.yml); CI runs it only when started by hand.
set -eu
cd "$(dirname "$0")/.."
# rustup's toolchain, not another cargo on the PATH
export PATH="$HOME/.cargo/bin:$PATH"

(
    cd core
    cargo fmt -- --check
    cargo clippy --release --all-targets -- -D warnings
    # the audio device every other system uses also builds on a Mac
    if [ "$(uname)" = Darwin ]; then
        cargo clippy --release --features portable-output -- -D warnings
    fi
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --quiet
    # in release: with debug assertions the engine's own check stops two window tests (PLAN.md)
    cargo test --release
)

# every function behind the C interface is in the header, and the other way round
tools/check-header.sh

# the Mac's shell: Apple's swift-format, which comes with Xcode
if command -v xcrun >/dev/null 2>&1 && xcrun --find swift-format >/dev/null 2>&1; then
    xcrun swift-format lint --strict -r apps/mac/Sources
fi

# the core still compiles for Windows and Linux, where zig is there to be their C compiler
if [ -n "${ZIG:-}" ] || command -v zig >/dev/null 2>&1; then
    tools/cross-check.sh
fi
echo "gate: passed"
