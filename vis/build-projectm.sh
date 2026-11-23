#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Builds libprojectM, the Milkdrop visualizer's engine, as a library the app loads when it
# starts: vis/lib/libprojectM-4.dylib. The app builds and runs without it; with it, the
# Visualizer window shows Milkdrop presets.
#
#   PROJECTM_SRC=<a checkout of projectM 4.1 with its submodules> vis/build-projectm.sh
#
# Without PROJECTM_SRC it looks for vis/projectm. It needs CMake (and takes Ninja when it is
# there). projectM is under the LGPL; it stays a library of its own beside the app, so it can
# be replaced with another build.
set -eu
cd "$(dirname "$0")"

src="${PROJECTM_SRC:-$PWD/projectm}"
if [ ! -f "$src/CMakeLists.txt" ]; then
    echo "build-projectm.sh: no projectM sources at $src; set PROJECTM_SRC" >&2
    exit 1
fi
if ! command -v cmake >/dev/null 2>&1; then
    echo "build-projectm.sh: cmake is not installed" >&2
    exit 1
fi
generator=""
if command -v ninja >/dev/null 2>&1; then generator="-G Ninja"; fi

# shellcheck disable=SC2086 # the generator is two words, or none
cmake -S "$src" -B build $generator \
    -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=ON \
    -DENABLE_SDL_UI=OFF \
    -DBUILD_TESTING=OFF \
    -DENABLE_PLAYLIST=OFF \
    -DENABLE_SYSTEM_PROJECTM_EVAL=OFF \
    -DCMAKE_OSX_ARCHITECTURES="${ARCHS:-$(uname -m)}" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0 >/dev/null
cmake --build build
mkdir -p lib
# one file, by the name the app looks for
cp -f "$(find build/src/libprojectM -name 'libprojectM-4.*.*.*.dylib' | head -1)" lib/libprojectM-4.dylib
install_name_tool -id @rpath/libprojectM-4.dylib lib/libprojectM-4.dylib
echo "built vis/lib/libprojectM-4.dylib"
