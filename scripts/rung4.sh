#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 4: the real System Card ($SYSCARD, bring your own, read in place) on a synthetic disc,
# 700 frames with a RUN press: identical.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/lockstep_cd.py" --only '^syscard-' "$@"
echo "rung 4: PASS"
