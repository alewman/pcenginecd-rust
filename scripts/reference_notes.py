#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""A place where pcenginecd-python v0.1.0 did something that read as unintended, shown on the
reference itself (docs/notes-on-the-reference.md). Settled in v0.2.0 (its issue 0001): both
numbers are now equal (75,000 at v0.2.0, 74,940 since the clean rewrite of the drive's bus,
4abb3f5; v0.1.0 printed 0 for the second). The Rust crate does the same. Run with the gate's interpreter and PYTHONPATH
(``scripts/lockstep_cd.py`` shows both); tests/reference_notes.rs asserts the same numbers here.

1. **The drive's clock stands still between events.** ``CdUnit.catch_up`` runs the bus (and so
   moves ``ScsiBus.now``) only when an event is due or none is pending. While some event waits
   in the future (the end of a CD audio track, the ADPCM sample clock), a register access does
   not move ``now``, and the timers a write starts (SEL to the command phase, the REQ of the
   next command byte, an answer's delay, the auto-ACK pulse) count from the last time the bus
   happened to run. With music playing, a SEL written a second later gets its command phase at
   once instead of 75,000 master clocks later. (With ADPCM playing the bus runs at every sample,
   so the clock is at most one sample period old.)
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "external" / "pcenginecd-python" / "tests"))

from discs import disc_audio_then_data  # noqa: E402
from helpers import Host  # noqa: E402

from pcenginecd_python.scsi import ScsiBus  # noqa: E402
from pcenginecd_python.unit import CdUnit  # noqa: E402

MASTER = 21_477_272


def sel_to_command(prepare) -> int:
    """Master clocks from a SEL write to the command phase, looked for every 12 clocks."""
    h = Host(CdUnit(disc_audio_then_data(400, 20)))
    prepare(h)
    h.advance(MASTER)  # a second passes: `advance` is the board's catch_up
    h.select()
    began = h.t
    while h.unit.read(0x1FF800, h.t) & 0xF8 != 0xD0:
        h.t += 12
        h.unit.catch_up(h.t)
    return h.t - began


def music(h: Host) -> None:
    h.command([0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80])  # AUDIO START: track 1 (5.3 s long)
    h.finish()


def main() -> int:
    print(f"1. SEL to the command phase (ScsiBus.SEL_TO_COMMAND_PHASE = {ScsiBus.SEL_TO_COMMAND_PHASE:,} master clocks):")
    print(f"   drive idle, nothing pending:      {sel_to_command(lambda h: None):>6}")
    print(f"   CD audio playing (its end waits): {sel_to_command(music):>6}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
