#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Type-checks the core, its tests and its examples for Windows and Linux from a Mac, so a
# line that only compiles on a Mac is found here and not on the other system. It compiles
# everything and links nothing: that a build runs is for CI (.github/workflows/ci.yml) to say.
#
# It needs rustup (the targets are added when missing) and zig, which is the C compiler for
# the other systems: the Lua interpreter and the TLS library are C. `ZIG` names another zig
# than the one on the PATH, such as "python3 -m ziglang".
set -eu
cd "$(dirname "$0")/../core"

# rustup's toolchain has the other systems' libraries; another cargo on the PATH has not
export PATH="$HOME/.cargo/bin:$PATH"
zig="${ZIG:-zig}"
if ! $zig version >/dev/null 2>&1; then
    echo "cross-check.sh: zig is not installed; see the top of this file" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# zig as the C compiler for one system. The target is zig's own, so the one a build script
# names is left out.
compiler() {
    cat > "$work/cc-$1" <<SCRIPT
#!/bin/bash
args=()
for a in "\$@"; do case "\$a" in --target=*) ;; *) args+=("\$a");; esac; done
exec $zig cc -target $2 "\${args[@]}"
SCRIPT
    printf '#!/bin/sh\nexec %s ar "$@"\n' "$zig" > "$work/ar-$1"
    chmod +x "$work/cc-$1" "$work/ar-$1"
}
compiler windows x86_64-windows-gnu
compiler linux x86_64-linux-gnu

# stands in for pkg-config: ALSA is "there". Nothing is linked, so its flags only have to parse.
cat > "$work/pkg-config" <<'SCRIPT'
#!/bin/sh
case "$*" in
  *--libs*) echo "-lasound" ;;
  *--modversion*|*--version*) echo "1.2.8" ;;
esac
exit 0
SCRIPT
chmod +x "$work/pkg-config"

rustup target add x86_64-pc-windows-gnu x86_64-unknown-linux-gnu >/dev/null

echo "== Windows (x86_64-pc-windows-gnu)"
CC_x86_64_pc_windows_gnu="$work/cc-windows" AR_x86_64_pc_windows_gnu="$work/ar-windows" \
    cargo check --release --tests --examples --target x86_64-pc-windows-gnu

echo "== Linux (x86_64-unknown-linux-gnu)"
CC_x86_64_unknown_linux_gnu="$work/cc-linux" AR_x86_64_unknown_linux_gnu="$work/ar-linux" \
    PKG_CONFIG="$work/pkg-config" PKG_CONFIG_ALLOW_CROSS=1 \
    cargo check --release --tests --examples --target x86_64-unknown-linux-gnu

echo "the core compiles for Windows and Linux"
