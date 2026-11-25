#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Makes the Mac's app icon, apps/mac/Resources/AppIcon.icns, from Andamp's icon (apps/mac/icon/andamp.svg).
# Run it after the drawing changes; the icon it makes is committed. It needs Xcode's tools.
set -eu
cd "$(dirname "$0")/.."
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
swift tools/icon.swift apps/mac/icon/andamp.svg "$work/icon.png"
set="$work/AppIcon.iconset"
mkdir "$set"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$work/icon.png" --out "$set/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$work/icon.png" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o apps/mac/Resources/AppIcon.icns
echo "apps/mac/Resources/AppIcon.icns"
