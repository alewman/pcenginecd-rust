#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 5: real discs ($LIBCD/*.chd, bring your own): each extracted with chdman into scratch/,
# one at a time, run 3,000 frames with RUN at 450 on both sides, and removed. The three Arcade
# Card discs of the reference's docs/m9-record.md and the discs of its m7 and m8 records.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/lockstep_cd.py" --only '^disc-' --jobs 1 "$@"
echo "rung 5: PASS"
