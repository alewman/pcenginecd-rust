#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The lockstep gate's reference side: pcenginecd-python's ``PcEngineCd``, traced frame by frame.

Run on PyPy with the three pinned checkouts of ``external/`` on ``PYTHONPATH``
(``scripts/fetch_reference.sh``; ``scripts/lockstep_cd.py`` does that). A line is the base gate's
(pcengine-rust ``scripts/pce_trace.py``, imported by path: the frame counter, the master clock,
the count and SHA-1 of every CPU bus cycle, the SHA-1 of the picture, the count and SHA-1 of the
mixed audio -- PSG, CD audio and ADPCM -- and the CPU's state) plus ``cmd=N:SHA1``, the drive's
command log of the frame (``ScsiBus.log``: each entry as ``"{mclk} {text}\\n"``).
``target/release/pcecd-trace`` writes the same lines.

    pcecd_trace.py CARD [--disc CUE] [--arcade] --frames N [--press BUTTONS@FRAME+HOLD ...]
                   [--machine pce|tg16] [--super yes|no] [--region-us] [--bram FILE]
                   [--switch WHERE.NAME=VALUE ...] [--reset FRAME:button|power ...]
                   [--detail FRAME] --out FILE

CARD is the System Card image (bring your own) or a card of this repository's making
(``scripts/make_cdprobe.py``). ``--switch``: a named switch of the reference away from its default;
WHERE is ``scsi`` (``ScsiBus``), ``unit`` (``CdUnit``), ``cdda`` (``CddaPlayer``), ``adpcm``
(``Adpcm``) or ``arcade`` (``ArcadeCard``); a tuple is written ``2,3,4``; ``cdda.SUBQ_STATUS`` is
four numbers in the order playing, paused, stopped, seeking. ``--reset``: ``Board.reset`` of that
kind before the frame with that count of completed frames runs.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
BASE_SCRIPTS = Path(os.environ.get("PCENGINE_RUST", HERE.parents[1] / "pcengine-rust")) / "scripts"
sys.path.insert(0, str(BASE_SCRIPTS))

import pce_trace  # noqa: E402  (the base gate's tap, lines and press spelling)
from pcengine_python import pads  # noqa: E402
from pcengine_python.machine import PC_ENGINE, TURBOGRAFX_16  # noqa: E402
from pcengine_python.sockets import ResetKind  # noqa: E402

from pcenginecd_python import Disc, PcEngineCd  # noqa: E402
from pcenginecd_python.adpcm import Adpcm  # noqa: E402
from pcenginecd_python.arcade import ArcadeCard  # noqa: E402
from pcenginecd_python.cdda import CddaPlayer, Status  # noqa: E402
from pcenginecd_python.scsi import ScsiBus  # noqa: E402
from pcenginecd_python.unit import CdUnit  # noqa: E402

WHERE = {"scsi": ScsiBus, "unit": CdUnit, "cdda": CddaPlayer, "adpcm": Adpcm, "arcade": ArcadeCard}


def set_switch(spec: str) -> None:
    target, _, text = spec.partition("=")
    where, _, name = target.partition(".")
    cls = WHERE[where]
    current = getattr(cls, name)  # an unknown name is an error, not a new attribute
    if name == "SUBQ_STATUS":
        codes = [int(x) for x in text.split(",")]
        value = dict(zip((Status.PLAYING, Status.PAUSED, Status.STOPPED, Status.SEEKING), codes, strict=True))
    elif isinstance(current, bool):
        value = text.lower() in ("true", "1")
    elif isinstance(current, int):
        value = int(text, 0)
    elif isinstance(current, float):
        value = float(text)
    elif isinstance(current, tuple):
        value = tuple(int(x) for x in text.split(","))
    else:
        raise SystemExit(f"cannot set {target}")
    setattr(cls, name, value)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("card")
    p.add_argument("--disc")
    p.add_argument("--arcade", action="store_true")
    p.add_argument("--frames", type=int, default=60)
    p.add_argument("--press", action="append", default=[])
    p.add_argument("--machine", choices=("pce", "tg16"), default="pce")
    p.add_argument("--super", choices=("yes", "no"))
    p.add_argument("--region-us", action="store_true")
    p.add_argument("--bram")
    p.add_argument("--switch", action="append", default=[], metavar="WHERE.NAME=VALUE")
    p.add_argument("--reset", action="append", default=[], metavar="FRAME:KIND")
    p.add_argument("--detail", type=int)
    p.add_argument("--out", required=True)
    a = p.parse_args()
    for spec in a.switch:
        set_switch(spec)
    presses = [pce_trace.parse_press(spec) for spec in a.press]
    resets = {}
    for spec in a.reset:
        frame, _, kind = spec.partition(":")
        resets[int(frame)] = {"button": ResetKind.BUTTON, "power": ResetKind.POWER_ON}[kind]
    pad = pads.Pad2()
    cd = PcEngineCd(
        Path(a.card).read_bytes(),
        Disc.from_cue(a.disc) if a.disc else None,
        machine=TURBOGRAFX_16 if a.machine == "tg16" else PC_ENGINE,
        super_system_card=None if a.super is None else a.super == "yes",
        region_us=a.region_us,
        bram=Path(a.bram).read_bytes() if a.bram else None,
        arcade_card=a.arcade,
        pad=pad,
    )
    board, bus = cd.board, cd.unit.bus
    bus.record_log = True
    cycles: list[str] = []
    log = pce_trace.observe(board, cycles if a.detail is not None else None)
    system = cd.build()
    with open(a.out, "w") as out:
        for _ in range(a.frames):
            pad.pressed = pce_trace.held(presses, system.frame)
            if system.frame in resets:
                board.reset(resets[system.frame])
            wanted = a.detail is not None and a.detail == system.frame + 1
            del cycles[:]
            system.run_frame()
            line = pce_trace.frame_line(system, log)
            commands = hashlib.sha1("".join(f"{t} {text}\n" for t, text in bus.log).encode())
            line += f" cmd={len(bus.log)}:{commands.hexdigest()}"
            del bus.log[:]
            if a.detail is None:
                out.write(line + "\n")
                out.flush()
            elif wanted:
                out.write("\n".join(cycles) + "\n")
                break
    return 0


if __name__ == "__main__":
    sys.exit(main())
