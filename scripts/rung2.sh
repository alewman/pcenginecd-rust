#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 2: the gate itself (scripts/lockstep_cd.py: the reference's PcEngineCd on PyPy against
# pcecd-trace, every frame's bus log, picture, mixed audio, CPU state and drive commands), on
# the probe card with every default.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/lockstep_cd.py" --only '^probe-base$' "$@"
echo "rung 2: PASS"
