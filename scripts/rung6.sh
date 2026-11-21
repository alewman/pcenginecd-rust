#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 6: save states. The base board's save_state/load_state and the snapshot pair cover this
# machine through DeviceState; the disc is named by its table of contents, never stored. The
# refusals (another disc, another System Card, a machine without the Arcade Card) and the unit's
# own round trip are tests/state.rs; then the round trip and the run-ahead pass on rung 4's
# boot, on the probe card and on two real discs in mid-boot (scripts/state_roundtrip_cd.py).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
nice cargo test --release -q --manifest-path "$ROOT/Cargo.toml" --test state
python3 "$ROOT/scripts/state_roundtrip_cd.py" "$@"
echo "rung 6: PASS"
