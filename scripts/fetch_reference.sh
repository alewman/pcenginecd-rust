#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# The reference at its pins into external/: pcenginecd-python 5c06d1b (v0.2.0, the clean rewrite of
# its drive's bus 4abb3f5 and the repin to the clean base board), the base board it pins
# (pcengine-python 03ac699: its clean rewrite and the early display start crop), the core
# (huc6280-python 6ce348e, its clean rewrite) and the frame the core pins (6502-python 2146eba).
# All four are on GitHub (the first three since 2026-11-20). No venv and nothing installed (so no stale install of the same version number can
# shadow a checkout):
# the gate runs PyPy with PYTHONPATH over the four src/ directories (scripts/pcecd_trace.py).
# Usage: scripts/fetch_reference.sh   (sources default to GitHub; set PCENGINECD_PYTHON,
# PCENGINE_PYTHON, HUC6280_PYTHON, SIXFIVEOHTWO_PYTHON to another git URL or a local clone, e.g.
# PCENGINE_PYTHON=../pcengine-python). Each commit is fetched from its source, so a local clone
# that has it works too.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
fetch() { # name pin commit source
    local dest="$ROOT/external/$1"
    if [ ! -d "$dest/.git" ]; then
        git clone -q "$4" "$dest"
    fi
    git -C "$dest" fetch -q --tags "$4" "$3" 2>/dev/null || true
    git -C "$dest" checkout -q "$3"
    [ "$(git -C "$dest" rev-parse HEAD)" = "$3" ] || { echo "$1: not at $3" >&2; exit 1; }
    echo "$1 $2 ($(git -C "$dest" rev-parse --short HEAD)) in $dest"
}
fetch pcenginecd-python 5c06d1b 5c06d1bb347df24ca331059987cf9463ab7a6bb3 "${PCENGINECD_PYTHON:-https://github.com/alewman/pcenginecd-python}"
fetch pcengine-python 03ac699 03ac6998c812a291e5f91882552df5ae712ba870 "${PCENGINE_PYTHON:-https://github.com/alewman/pcengine-python}"
fetch huc6280-python 6ce348e 6ce348e13c9ef3009bd07e7938c4b48956eeadc6 "${HUC6280_PYTHON:-https://github.com/alewman/huc6280-python}"
fetch 6502-python 2146eba 2146eba3b3da4d2cc6c8fa3994f100bcb5abf039 "${SIXFIVEOHTWO_PYTHON:-https://github.com/alewman/6502-python}"
