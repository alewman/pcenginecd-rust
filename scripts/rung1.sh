#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 1: the unit tests. pcenginecd-python's tests/test_disc.py, test_scsi.py, test_cdda.py,
# test_adpcm.py, test_arcade.py, test_unit.py and test_board.py transcribed (tests/*.rs), on the
# same synthetic discs (tests/common/mod.rs is its tests/discs.py and helpers.py).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
nice cargo test --release -q --manifest-path "$ROOT/Cargo.toml" \
    --test disc --test scsi --test cdda --test adpcm --test arcade --test unit --test board --test issue0001 --test reference_notes
echo "rung 1: PASS"
