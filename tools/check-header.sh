#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Every function behind the C interface is in the header, and the other way round.
set -eu
cd "$(dirname "$0")/.."
rust="$(grep -oh 'fn alpaca_[a-z0-9_]*' core/src/ffi/*.rs | sed 's/^fn //' | sort -u)"
header="$(grep -v '^ *//' core/include/alpaca_core.h | grep -o 'alpaca_[a-z0-9_]*(' | tr -d '(' | sort -u)"
if [ "$rust" != "$header" ]; then
    echo "check-header: core/include/alpaca_core.h and core/src/ffi name different functions" >&2
    exit 1
fi
