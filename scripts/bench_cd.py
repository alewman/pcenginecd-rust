#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Frames per second of this crate and of the reference on a case of scripts/cases_cd.tsv,
nothing logged or digested (the mixed audio is taken every frame on both sides).

    bench_cd.py CASE [--frames N] [--rust-runs 3] [--no-reference]

A ``chd:`` disc is extracted for the run and removed, as the gate does. The reference runs once
on PyPy (``PYPY_GC_NURSERY=1M``); it is slow, so the whole run and its second half are timed.
With ``--reference``, this script is the reference's side itself (run on its interpreter).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent


def reference(a: argparse.Namespace) -> int:
    from pcengine_python import pads

    from pcenginecd_python import Disc, PcEngineCd

    pad = pads.Pad2()
    cd = PcEngineCd(
        Path(a.card).read_bytes(), Disc.from_cue(a.disc) if a.disc else None, arcade_card=a.arcade, pad=pad
    )
    system = cd.build()
    start, hold = a.run_at, 20
    began = half = time.monotonic()
    samples = 0
    for frame in range(a.frames):
        pad.pressed = pads.RUN if start <= frame < start + hold else 0
        system.run_frame()
        samples += len(cd.board.take_audio())
        if frame + 1 == a.frames // 2:
            half = time.monotonic()
    end = time.monotonic()
    print(
        f"reference: {a.frames} frames in {end - began:.1f} s: {a.frames / (end - began):.2f} frames per second "
        f"(second half {(a.frames - a.frames // 2) / (end - half):.2f}); {samples} audio samples, mclk {cd.board.mclk}"
    )
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("case")
    p.add_argument("--frames", type=int)
    p.add_argument("--rust-runs", type=int, default=3)
    p.add_argument("--no-reference", action="store_true")
    p.add_argument("--reference", action="store_true", help=argparse.SUPPRESS)
    p.add_argument("--card", help=argparse.SUPPRESS)
    p.add_argument("--disc", help=argparse.SUPPRESS)
    p.add_argument("--arcade", action="store_true", help=argparse.SUPPRESS)
    p.add_argument("--run-at", type=int, default=449, help=argparse.SUPPRESS)
    a = p.parse_args()
    if a.reference:
        return reference(a)
    sys.path.insert(0, str(HERE))
    import lockstep_cd as gate

    case = next(c for c in gate.cases(HERE / "cases_cd.tsv") if c[0] == a.case)
    _, frames, card, disc, args = case
    frames = a.frames or frames
    subprocess.run(["cargo", "build", "--release", "-q", "--manifest-path", str(ROOT / "Cargo.toml")], check=True)
    cue, work = gate.extract(disc) if disc != "-" else (None, None)
    try:
        common = [card, "--frames", str(frames), *args] + (["--disc", str(cue)] if cue else [])
        for _ in range(a.rust_runs):
            done = gate.run([str(ROOT / "target" / "release" / "pcecd-bench"), *common])
            print("rust:", (done.stdout + done.stderr).strip(), flush=True)
        if not a.no_reference:
            press = next((x for x in args if re.match(r"run@\d+", x)), "run@449")
            ref = [gate.PYPY, str(Path(__file__).resolve()), a.case, "--reference", "--card", card,
                   "--frames", str(frames), "--run-at", re.search(r"\d+", press).group()]  # fmt: skip
            ref += (["--disc", str(cue)] if cue else []) + (["--arcade"] if "--arcade" in args else [])
            done = gate.run(ref)
            print((done.stdout + done.stderr).strip(), flush=True)
    finally:
        if disc.startswith("chd:"):
            gate.remove(work)
    return 0


if __name__ == "__main__":
    sys.exit(main())
