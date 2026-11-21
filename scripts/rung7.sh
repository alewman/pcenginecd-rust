#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 7: seeded faults. One single-line fault at a time, each to be caught by a rung
# (scripts/seeded_faults_cd.py). Needs the reference cache a full gate run left.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/seeded_faults_cd.py" --jobs "${JOBS:-6}" "$@"
echo "rung 7: PASS"
