#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""The gate's synthetic discs as bin/cue pairs in scratch/discs/ (ignored, never committed).

Nothing here is a real disc: the sectors come from pcenginecd-python's ``tests/discs.py`` (numbered
data sectors, two sine waves), imported by path from ``external/pcenginecd-python``. Both sides of
the gate read the same files (the reference with ``Disc.from_cue``, this crate with its
file-backed ``SectorSource``). Rebuilt byte for byte the same; a file that is already right is
left alone. Run with the gate's interpreter or the system's python3.

- ``data``: one data track of 400 sectors.
- ``mixed``: audio 40, data 400, audio 30 (rung 4's disc: ``disc_audio_then_data(40, 400)``).
- ``pregap``: the shape ``chdman extractcd`` writes: audio 60, then a data track of 300 with a
  stored pregap (``INDEX 00``) of 20, then audio 120 with a pregap of 10; in two FILEs.
- ``bram.bin``: a 2 KiB backup RAM image for ``--bram`` (not a disc).
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "external" / "pcenginecd-python" / "tests"))
sys.path.insert(0, str(ROOT / "external" / "pcenginecd-python" / "src"))
sys.path.insert(0, str(ROOT / "external" / "pcengine-python" / "src"))
sys.path.insert(0, str(ROOT / "external" / "huc6280-python" / "src"))

from discs import audio_sectors, numbered_data  # noqa: E402


def msf(frames: int) -> str:
    return f"{frames // 4500:02d}:{frames // 75 % 60:02d}:{frames % 75:02d}"


def write(path: Path, data: bytes | str) -> None:
    data = data.encode() if isinstance(data, str) else data
    if not path.is_file() or path.read_bytes() != data:
        path.write_bytes(data)


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "scratch" / "discs"
    out.mkdir(parents=True, exist_ok=True)
    write(out / "data.bin", numbered_data(0, 400))
    write(out / "data.cue", 'FILE "data.bin" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n')
    write(out / "mixed.bin", audio_sectors(40) + numbered_data(40, 400) + audio_sectors(30))
    write(
        out / "mixed.cue",
        'FILE "mixed.bin" BINARY\n'
        "  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n"
        f"  TRACK 02 MODE1/2352\n    INDEX 01 {msf(40)}\n"
        f"  TRACK 03 AUDIO\n    INDEX 01 {msf(440)}\n",
    )
    write(out / "pregap-a.bin", audio_sectors(60, left_hz=330.0, right_hz=550.0) + numbered_data(60, 300))
    write(out / "pregap-b.bin", audio_sectors(130, left_hz=220.0, right_hz=1100.0))
    write(
        out / "pregap.cue",
        'FILE "pregap-a.bin" BINARY\n'
        "  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n"
        f"  TRACK 02 MODE1/2352\n    INDEX 00 {msf(60)}\n    INDEX 01 {msf(80)}\n"
        'FILE "pregap-b.bin" BINARY\n'
        f"  TRACK 03 AUDIO\n    INDEX 00 {msf(0)}\n    INDEX 01 {msf(10)}\n",
    )
    # a backup RAM image for ``--bram``: the formatted header, then a fixed pattern
    bram = bytes([0x48, 0x55, 0x42, 0x4D, 0x00, 0xA0, 0x10, 0x80]) + bytes((i * 7 + 3) & 0xFF for i in range(2040))
    write(out / "bram.bin", bram)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
