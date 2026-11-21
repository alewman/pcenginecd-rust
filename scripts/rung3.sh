#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
# Rung 3: synthetic programs. The probe card (scripts/make_cdprobe.py) drives the unit through
# its registers on synthetic discs, on each machine and medium and with every named switch away
# from its default; then scripts/switch_effect_cd.py says that each case changes the reference's
# trace (a switch that changes nothing proves nothing).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
python3 "$ROOT/scripts/lockstep_cd.py" --only '^(probe|sw)-' --jobs "${JOBS:-6}" "$@"
python3 "$ROOT/scripts/switch_effect_cd.py"
echo "rung 3: PASS"
