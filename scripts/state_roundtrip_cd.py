#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Rung 6's driver: the save-state round trip and the run-ahead pass on the CD machine.

``pcecd-state-roundtrip`` on rung 4's boot (the real System Card on the synthetic disc, saved
inside frame 480 while the BIOS reads the disc) and on two real discs in mid-boot (the cases and
frames of pcenginecd-python's ``scripts/cd_state.py`` runs: Ryuuko no Ken inside frame 2000, with
the Arcade Card and ADPCM in use, and Mad Stalker inside frame 1500, streaming reads). The state
is loaded into a fresh machine and into one that had run; 60 frames after the save are compared
by the gate's own lines (bus log, picture, mixed audio, CPU state, drive commands); the loaded
machine saved at once must give the same bytes; then 30 frames with a snapshot taken, two
frames run ahead and the snapshot put back after every frame. Discs are extracted one at a time
and removed, as the gate does.

    state_roundtrip_cd.py [--only REGEX] [--binary PATH] [--synthetic-only]
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
import lockstep_cd as gate  # noqa: E402

# case of cases_cd.tsv, frame saved in, instructions past it
RUNS = [
    ("syscard-mixed", 480, 777),
    ("probe-base", 200, 1234),
    ("probe-base", 265, 4321),
    ("disc-Ryuuko no Ken", 2000, 777),
    ("disc-Mad Stalker", 1500, 2001),
]


def one(case: gate.Case, at: int, extra: int, binary: Path) -> tuple[bool, str]:
    name, _, card, disc, args = case
    real = disc.startswith("chd:")
    work = None
    try:
        cue, work = gate.extract(disc) if disc != "-" else (None, None)
        command = [str(binary), card, *args]
        if cue is not None:
            command += ["--disc", str(cue)]
        command += ["--at", str(at), "--to", str(at + 60), "--extra", str(extra), "--ahead", "2", "--ahead-frames", "30"]
        done = gate.run(command)
        text = (done.stdout + done.stderr).strip()
        return done.returncode == 0, f"saved {extra} instructions into frame {at}: {text}"
    except RuntimeError as error:
        return False, str(error)
    finally:
        if real:
            gate.remove(work)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--only")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--synthetic-only", action="store_true", help="no real disc (the seeded faults' quick pass)")
    o = parser.parse_args()
    binary = o.binary
    if binary is None:
        build = subprocess.run(["cargo", "build", "--release", "-q", "--manifest-path", str(ROOT / "Cargo.toml")])
        if build.returncode:
            return 1
        binary = ROOT / "target" / "release" / "pcecd-state-roundtrip"
    cases = {c[0]: c for c in gate.cases(HERE / "cases_cd.tsv")}
    subprocess.run([sys.executable, str(HERE / "make_discs.py")], check=True, env=gate.ENV)
    subprocess.run([sys.executable, str(HERE / "make_cdprobe.py")], check=True)
    failed = ran = 0
    for name, at, extra in RUNS:
        case = cases[name]
        if (o.only and not re.search(o.only, name)) or (o.synthetic_only and case[3].startswith("chd:")):
            continue
        ran += 1
        ok, text = one(case, at, extra, binary)
        print(f"{name}: {text}", flush=True)
        failed += not ok
    print(f"{ran - failed} of {ran} runs: a loaded run continues the same")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
