#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The lockstep gate of the CD machine: each case on both sides, the traces compared.

A case of ``scripts/cases_cd.tsv`` is ``name, frames, card, disc, arguments``. It runs
``scripts/pcecd_trace.py`` on the reference (PyPy, the pinned checkouts of ``external/`` on
``PYTHONPATH``, ``PYPY_GC_NURSERY=1M``, under ``nice``) and ``target/release/pcecd-trace`` here;
the two traces are compared line for line (a line is a frame: the CPU bus log, the picture, the
mixed audio, the CPU state and the drive's command log, digested). A case passes when they are
identical. Otherwise the first differing frame is named and both sides are run again with
``--detail`` on it, which narrows it to the first differing bus cycle. The comparison, the
narrowing and the spelling are the base gate's (pcengine-rust ``scripts/lockstep.py``, imported
by path).

    lockstep_cd.py [--only REGEX] [--jobs N] [--rust PATH] [--keep] [--no-narrow] [--quiet]
                   [--reference-only] [--no-cache]

``card``: ``$SYSCARD`` (the System Card, bring your own, read in place), ``$PROBE`` (the card
``scripts/make_cdprobe.py`` builds) or a path. ``disc``: ``-`` (none), ``$DISCS/NAME.cue`` (a
synthetic disc of ``scripts/make_discs.py``) or ``chd:NAME`` (``$LIBCD/NAME.chd``: extracted with
``chdman extractcd`` into ``scratch/disc/``, **one disc at a time**, and removed afterwards).

The reference runs at about a twentieth of real time, and its trace of a case never changes
while the pins do not: each is kept in ``external/refcache`` under the case's name and a hash of
its frames, card, disc, arguments, the four reference checkouts' commits and the base gate's
``pce_trace.py``, so a rerun costs the Rust side only and a repin traces the reference again. ``--reference-only``
fills that cache without this crate's binary.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shlex
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
BASE = Path(os.environ.get("PCENGINE_RUST", ROOT.parent / "pcengine-rust"))
sys.path.insert(0, str(BASE / "scripts"))
import lockstep as base  # noqa: E402  (first_difference, fields, safe_name)

EXTERNAL = ROOT / "external"
SCRATCH = ROOT / "scratch"
PYPY = os.environ.get("PYPY", str(Path.home() / ".local/opt/pypy3.11-v7.3.20-linux64/bin/pypy3.11"))
# The user's files come by environment variable only ($SYSCARD: the System Card image; $LIBCD:
# the directory of CHDs); a case that needs one that is not set fails with "no card at $SYSCARD"
# or the missing CHD's name. Ours are built into scratch/.
DEFAULTS = {
    "PROBE": str(SCRATCH / "cdprobe.pce"),
    "DISCS": str(SCRATCH / "discs"),
}
USER_FILES = ("SYSCARD", "LIBCD")
ENV = {
    **os.environ,
    "PYPY_GC_NURSERY": "1M",
    "PCENGINE_RUST": str(BASE),
    "PYTHONPATH": os.pathsep.join(
        str(EXTERNAL / name / "src")
        for name in ("pcenginecd-python", "pcengine-python", "huc6280-python", "6502-python")
    ),
}
MAX_JOBS = 8
ONE_DISC = threading.Lock()  # one extracted disc in scratch/ at a time

Case = tuple[str, int, str, str, list[str]]


def variables() -> dict[str, str]:
    names = (*DEFAULTS, *USER_FILES)
    return {**DEFAULTS, **{k: os.environ[k] for k in names if k in os.environ}}


def cases(path: Path) -> list[Case]:
    env = variables()
    rows = []
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, frames, card, disc, args = (line.split("\t") + ["", ""])[:5]
        for key, value in env.items():
            card = card.replace(f"${key}", value)
            disc = disc.replace(f"${key}", value)
            args = args.replace(f"${key}", value)
        rows.append((name, int(frames), card, disc, shlex.split(args)))
    return rows


def file_hash(path: Path) -> str:
    digest = hashlib.sha1()
    with path.open("rb") as handle:
        while block := handle.read(1 << 20):
            digest.update(block)
    return digest.hexdigest()


def disc_identity(disc: str) -> str:
    if disc == "-":
        return "-"
    if disc.startswith("chd:"):
        path = Path(variables().get("LIBCD", "$LIBCD")) / (disc[4:] + ".chd")
        return f"{path.name}:{path.stat().st_size}"
    cue = Path(disc)
    names = re.findall(r'FILE\s+"(.*?)"', cue.read_text())
    return file_hash(cue) + "".join(file_hash(cue.parent / name) for name in names)


def reference_revisions() -> tuple[str, ...]:
    """The commits of the four reference checkouts: a cached trace belongs to them."""
    return tuple(
        subprocess.run(
            ["git", "-C", str(EXTERNAL / name), "rev-parse", "HEAD"], capture_output=True, text=True
        ).stdout.strip()
        for name in ("pcenginecd-python", "pcengine-python", "huc6280-python", "6502-python")
    )


def cache_name(case: Case) -> str:
    name, frames, card, disc, args = case
    hashed = [file_hash(Path(x)) if Path(x).is_file() else x for x in args]  # --bram FILE
    # The reference's line is the base gate's (pcengine-rust scripts/pce_trace.py, imported by
    # path): its text is part of the key too, so a base-board checkout whose line differs does
    # not reuse traces (added 2026-11-19, with the repin to pcengine-rust 37f17ba).
    line = file_hash(BASE / "scripts" / "pce_trace.py")
    what = (frames, file_hash(Path(card)), disc_identity(disc), hashed, reference_revisions(), line)
    key = hashlib.sha1(repr(what).encode()).hexdigest()[:10]
    return f"{base.safe_name(name)}-{key}.ref"


def run(command: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(["nice", "-n", "10", *command], capture_output=True, text=True, env=ENV)


def extract(disc: str) -> tuple[Path, Path | None]:
    """(the cue, the directory to remove afterwards). Holds ONE_DISC for a ``chd:`` disc."""
    if not disc.startswith("chd:"):
        return Path(disc), None
    chd = Path(variables().get("LIBCD", "$LIBCD")) / (disc[4:] + ".chd")
    work = SCRATCH / "disc" / base.safe_name(disc[4:])
    work.mkdir(parents=True, exist_ok=True)
    cue = work / "disc.cue"
    if not cue.is_file():
        done = run(["chdman", "extractcd", "-i", str(chd), "-o", str(cue), "-ob", str(work / "disc.bin")])
        if done.returncode:
            raise RuntimeError(f"chdman: {done.stderr.strip()[-300:]}")
    return cue, work


def remove(work: Path | None) -> None:
    """Delete an extracted disc: only .bin/.cue files, as pcenginecd-python's scripts/scratch.py."""
    if work is None or not work.is_dir():
        return
    if any(p.suffix not in (".bin", ".cue") for p in work.iterdir()):
        print(f"not removing {work}: it holds more than .bin/.cue files", flush=True)
        return
    for p in work.iterdir():
        p.unlink()
    work.rmdir()


def narrow(common: list[str], name: str, rust: Path, traces: Path, frame: str) -> str:
    safe = base.safe_name(name)
    ref, ours = traces / f"{safe}.detail.ref", traces / f"{safe}.detail.rust"
    a = run([PYPY, str(HERE / "pcecd_trace.py"), *common, "--detail", frame, "--out", str(ref)])
    b = run([str(rust), *common, "--detail", frame, "--out", str(ours)])
    if a.returncode or b.returncode:
        return "narrowing failed: " + (a.stderr or b.stderr).strip()[-300:]
    found = base.first_difference(ref, ours)
    if found is None:
        return "every bus cycle of that frame is the same on both"
    number, left, right = found
    before = ref.read_text().splitlines()[max(0, number - 6) : number]
    context = "".join(f"\n        {line}" for line in before)
    return (
        f"bus cycle {number} of that frame (r/w ADDRESS VALUE MCLK, i MCLK){context}\n"
        f"      reference {left!r}\n      rust      {right!r}"
    )


def one(case: Case, rust: Path | None, traces: Path, o: argparse.Namespace):
    name, frames, card, disc, args = case
    safe = base.safe_name(name)
    began = time.monotonic()
    if not Path(card).is_file():
        return name, False, f"no card at {card}"
    ref = (traces / f"{safe}.ref") if o.no_cache else EXTERNAL / "refcache" / cache_name(case)
    ours = traces / f"{safe}.rust"
    real = disc.startswith("chd:")
    if real and rust is None and ref.is_file():
        return name, True, "reference cached"
    if real:
        ONE_DISC.acquire()
    work = None
    try:
        cue, work = extract(disc) if disc != "-" else (None, None)
        common = [card, "--frames", str(frames), *args]
        if cue is not None:
            common += ["--disc", str(cue)]
        board = None
        if rust is not None:  # both at once: the reference is the slow one
            board = subprocess.Popen(
                ["nice", "-n", "10", str(rust), *common, "--out", str(ours)],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=ENV,
            )  # fmt: skip
        if not ref.is_file():
            partial = ref.with_suffix(".partial")
            reference = run([PYPY, str(HERE / "pcecd_trace.py"), *common, "--out", str(partial)])
            if reference.returncode:
                if board is not None:
                    board.communicate()
                return name, False, "reference failed: " + reference.stderr.strip()[-600:]
            partial.replace(ref)
        if board is None:
            return name, True, f"reference traced, {frames} frames ({time.monotonic() - began:.0f} s)"
        _, stderr = board.communicate()
        if board.returncode:
            return name, False, "rust failed: " + stderr.strip()[-600:]
        seconds = time.monotonic() - began
        found = base.first_difference(ref, ours)
        if found is None:
            if not o.keep:
                ours.unlink()
            return name, True, f"identical, {frames} frames ({seconds:.0f} s)"
        number, left, right = found
        if not left or not right:
            return name, False, f"line {number}: one trace is longer"
        x, y = base.fields(left), base.fields(right)
        differing = [key for key in x if x.get(key) != y.get(key)]
        frame = left.split()[0]
        text = f"frame {frame} (line {number}) differs in {', '.join(differing)}"
        text += f"\n      reference {left!r}\n      rust      {right!r}"
        if not o.no_narrow and ("bus" in differing or "mclk" in differing):
            text += "\n      " + narrow(common, name, rust, traces, frame)
        return name, False, text
    except RuntimeError as error:
        return name, False, str(error)
    finally:
        if real:
            if not o.keep_disc:
                remove(work)
            ONE_DISC.release()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--only", help="a regular expression on the case name")
    parser.add_argument("--cases", type=Path, default=HERE / "cases_cd.tsv")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--rust", type=Path, help="a trace binary to judge instead of this tree's")
    parser.add_argument("--traces", type=Path, default=EXTERNAL / "traces")
    parser.add_argument("--keep", action="store_true", help="keep the traces of identical cases")
    parser.add_argument("--keep-disc", action="store_true", help="leave an extracted disc in scratch/")
    parser.add_argument("--no-cache", action="store_true", help="trace the reference again")
    parser.add_argument("--reference-only", action="store_true", help="only fill the reference cache")
    parser.add_argument("--no-narrow", action="store_true")
    parser.add_argument("--quiet", action="store_true", help="print only failures and the count")
    o = parser.parse_args()
    rust = o.rust
    if rust is None and not o.reference_only:
        build = subprocess.run(["cargo", "build", "--release", "-q", "--manifest-path", str(ROOT / "Cargo.toml")])
        if build.returncode:
            return 1
        rust = ROOT / "target" / "release" / "pcecd-trace"
    o.traces.mkdir(parents=True, exist_ok=True)
    (EXTERNAL / "refcache").mkdir(parents=True, exist_ok=True)
    todo = [c for c in cases(o.cases) if not o.only or re.search(o.only, c[0])]
    if any(c[3] != "-" and not c[3].startswith("chd:") for c in todo):
        subprocess.run([sys.executable, str(HERE / "make_discs.py")], check=True, env=ENV)
    if any(Path(c[2]).name == "cdprobe.pce" for c in todo):
        subprocess.run([sys.executable, str(HERE / "make_cdprobe.py")], check=True)
    failed = 0
    with ThreadPoolExecutor(max(1, min(o.jobs, MAX_JOBS))) as pool:
        for name, ok, text in pool.map(lambda c: one(c, rust, o.traces, o), todo):
            if not ok or not o.quiet:
                print(f"{name}: {text}", flush=True)
            failed += not ok
    print(f"{len(todo) - failed} of {len(todo)} cases identical")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
