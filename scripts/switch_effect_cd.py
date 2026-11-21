#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""A switch that changes nothing proves nothing: say which probe cases their switch changes.

If the reference's trace with a switch away from its default is its trace without, both sides
agree whether or not this crate reads the switch. For every ``sw-`` and ``probe-`` case of
``scripts/cases_cd.tsv`` this compares the reference's cached trace with its baseline's (the
probe with every default, or the case named below) and fails if a case changes nothing.

    switch_effect_cd.py          (after the gate has filled external/refcache)
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import lockstep_cd as gate  # noqa: E402

# what a case is compared with, when it is not the probe with every default
# A reset of the `Button` kind does nothing to this machine unless a switch says so: the case
# is the baseline of that switch's, and is expected to change nothing itself.
EXPECTED_SILENT = {"probe-reset-button"}

BASELINE = {
    "sw-cdda.PCETECH_SLOW_SECONDS": "sw-cdda.PCETECH_FADE",
    "sw-cdda.PCETECH_FAST_SECONDS": "sw-cdda.PCETECH_FADE",
    "sw-arcade.RESET_CLEARS_REGISTERS": "probe-reset-button",
}


def main() -> int:
    cases = {c[0]: c for c in gate.cases(HERE / "cases_cd.tsv")}
    cache = gate.EXTERNAL / "refcache"
    silent = 0
    checked = 0
    for name, case in cases.items():
        if not name.startswith(("sw-", "probe-")) or name == "probe-base":
            continue
        base = BASELINE.get(name, "probe-base")
        ours, theirs = cache / gate.cache_name(case), cache / gate.cache_name(cases[base])
        if not ours.is_file() or not theirs.is_file():
            print(f"{name}: no reference trace yet (run the gate first)")
            silent += 1
            continue
        checked += 1
        a, b = ours.read_text().splitlines(), theirs.read_text().splitlines()
        first = next((n for n, (x, y) in enumerate(zip(a, b, strict=False)) if x != y), None)
        if first is None and name in EXPECTED_SILENT:
            print(f"{name}: changes nothing against {base}, as it should")
        elif first is None:
            print(f"{name}: changes NOTHING against {base}")
            silent += 1
        else:
            differing = sum(x != y for x, y in zip(a, b, strict=False))
            x, y = gate.base.fields(a[first]), gate.base.fields(b[first])
            what = ", ".join(key for key in x if x.get(key) != y.get(key))
            print(f"{name}: differs from {base} from frame {a[first].split()[0]} ({what}), {differing} frames")
    print(f"{checked} cases checked; {silent} change nothing or have no trace")
    return 1 if silent else 0


if __name__ == "__main__":
    sys.exit(main())
