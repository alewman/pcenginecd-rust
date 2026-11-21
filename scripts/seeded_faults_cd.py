#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""A gate that cannot fail proves nothing: build the crate with one fault at a time and check
that some rung catches each (the shape of pcengine-rust's ``scripts/seeded_faults.py``).

Each fault is one exact text replacement in a copy of ``src/`` (with ``tests/``), built into a
shared target directory under ``external/faults/``; then the checks run, cheapest first, until
one fails:

    rung 2      lockstep: the probe card with every default
    rung 4      lockstep: the real System Card on the synthetic disc
    rung 3      lockstep: the probe card on every machine, medium and switch
    rung 1      cargo test (the transcribed unit tests; tests/state.rs is rung 6's)
    rung 6      the save-state round trip on the System Card's boot and the probe card
    rung 5      lockstep: the real discs (extracted one at a time; only if nothing else caught it)

The reference's traces come from the cache a full gate run left (``external/refcache``), so a
fault costs a build and the Rust side of the cases. A fault no check catches is MISSED.

    seeded_faults_cd.py [--only NAME] [--from NAME] [--jobs N] [--list] [--no-discs]
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# name: (file, exact text, replacement, what it changes)
FAULTS = {
    # the reference's issue 0001 (pcenginecd-python 0.2.0)
    "register-access-runs-only-when-due": (
        "unit.rs",
        "        self.run_now(mclk, pins);\n        let value = self.read_register(",
        "        self.run_if_due(mclk, pins);\n        let value = self.read_register(",
        "a register read leaves the drive's clock at the last event while another waits (0.1.0)",
    ),
    "register-write-runs-only-when-due": (
        "unit.rs",
        "        self.run_now(mclk, pins);\n        self.write_register(",
        "        self.run_if_due(mclk, pins);\n        self.write_register(",
        "a register write (a SEL) leaves the drive's clock at the last event (0.1.0)",
    ),
    "memory-access-runs-the-drive": (
        "unit.rs",
        "            return self.ram[(address & 0xFFFF) as usize]; // memory: no clock either\n",
        "            self.run_if_due(mclk, pins);\n            return self.ram[(address & 0xFFFF) as usize];\n",
        "a CD RAM read runs the drive first",
    ),
    "power-on-keeps-the-latch-guard": (
        "unit.rs",
        "            self.latch_guard_until = 0;\n",
        "",
        "a power-on reset keeps the $1805 guard window running",
    ),
    "power-on-keeps-the-dma-timer": (
        "unit.rs",
        "            self.dma_at = None;\n            self.next_event = self.soonest();\n",
        "",
        "a power-on reset keeps the CD-to-ADPCM DMA's timer",
    ),
    "adpcm-idle-filed-at-zero": (
        "unit.rs",
        "self.adpcm.power_on(self.mclk);",
        "self.adpcm.power_on(0);",
        "a power-on reset files the ADPCM's idle output at time 0, behind later events (0.1.0)",
    ),
    "power-on-keeps-rst": (
        "unit.rs",
        "            self.bus.rst = false; // the reset register is cleared below: RST lets go\n",
        "",
        "a power-on reset leaves the drive held in reset if $1804 bit 1 was set",
    ),
    "bram-longer-than-the-ram-cut": (
        "unit.rs",
        "        if header.len() > BRAM_BYTES {",
        "        if false && header.len() > BRAM_BYTES {",
        "a backup RAM image longer than 2 KiB is accepted (and then panics or is cut)",
    ),
    # disc.rs
    "msf-without-the-lead-in": (
        "disc.rs",
        "    relative_msf(lba + LEAD_IN_FRAMES)\n",
        "    relative_msf(lba)\n",
        "an LBA's absolute time leaves out the two-second lead-in (READ TOC, READ SUBQ)",
    ),
    "data-from-the-sync": (
        "disc.rs",
        "out.copy_from_slice(&self.read_raw(lba)[16..16 + DATA_SECTOR]);",
        "out.copy_from_slice(&self.read_raw(lba)[12..12 + DATA_SECTOR]);",
        "a data sector's payload is taken 4 bytes early (the header counted as data)",
    ),
    "pregap-belongs-to-no-track": (
        "disc.rs",
        "            let first = t.index0.unwrap_or(t.start);\n",
        "            let first = t.start;\n",
        "a track's INDEX 00 pregap is not part of it: its sectors read as zeros",
    ),
    "second-file-at-lba-0": (
        "disc.rs",
        "            first_lba: lba,\n        });\n        lba += sectors;",
        "            first_lba: 0,\n        });\n        lba += sectors;",
        "every FILE of a cue sheet starts at LBA 0 (they do not lie end to end)",
    ),
    "audio-track-read-as-data": (
        "disc.rs",
        "if self.track_at(lba).is_some_and(|track| track.is_data) {",
        "if self.track_at(lba).is_some() {",
        "READ of an audio sector returns its bytes, not zeros",
    ),
    # seek.rs
    "short-seek-three-vsyncs": (
        "seek.rs",
        "        return 9.0 * VSYNC_MS + half;",
        "        return 3.0 * VSYNC_MS + half;",
        "a gap under 5 sectors costs 3 VSYNC (seektime.c's dead branch, C20)",
    ),
    "seek-ignores-the-rotation": (
        "seek.rs",
        "    let half = ZONES[dst].3 / 2.0;",
        "    let half = ZONES[src].3 / 2.0;",
        "the half rotation is the starting zone's, not the target's",
    ),
    # cdda.rs
    "fade-aimed-at-the-other-source": (
        "cdda.rs",
        "(self.fade_register & 0x02 != 0) != adpcm {",
        "(self.fade_register & 0x02 != 0) == adpcm {",
        "`$180F` bit 1 aims the fade at CD audio, not ADPCM",
    ),
    "loop-plays-on": (
        "cdda.rs",
        "                if self.end_mode == EndMode::Loop {",
        "                if self.end_mode == EndMode::Irq {",
        "AUDIO END mode 2 loops and mode 1 stops at the end of its range",
    ),
    "audio-end-exclusive": (
        "cdda.rs",
        "let remaining = (self.end + 1) * SAMPLES_PER_SECTOR - self.anchor_sample;",
        "let remaining = self.end * SAMPLES_PER_SECTOR - self.anchor_sample;",
        "playback stops a sector early (the end sector is not played)",
    ),
    "faded-sample-rounded": (
        "cdda.rs",
        "out.push(((left as f64 * g) as i32, (right as f64 * g) as i32));",
        "out.push(((left as f64 * g).round() as i32, (right as f64 * g).round() as i32));",
        "a faded CD sample is rounded, not cut toward zero (Python's int())",
    ),
    "audio-end-leaves-a-stopped-player": (
        "cdda.rs",
        "            Status::Paused | Status::Stopped => self.resume(),\n",
        "            Status::Paused => self.resume(),\n            Status::Stopped => {}\n",
        "AUDIO END does not start a stopped player",
    ),
    # scsi.rs (the clean rewrite, pcenginecd-python 4abb3f5)
    "sel-answered-under-rst": (
        "scsi.rs",
        "        if self.rst || self.selecting {\n",
        "        if self.selecting {\n",
        "a SEL is answered while $1804 holds the drive in reset",
    ),
    "bus-reset-hides-dropped-levels": (
        "scsi.rs",
        "        self.cdda.stop();\n        self.update_irq();\n    }\n",
        "        self.cdda.stop();\n        self.irq_levels = (false, false);\n    }\n",
        "the drive's reset does not report the IRQ levels it drops (issue 0001 item 4)",
    ),
    "reset-keeps-cd-audio": (
        "scsi.rs",
        "        self.sense_asc = 0;\n        self.cdda.stop();\n        self.update_irq();\n",
        "        self.sense_asc = 0;\n        self.update_irq();\n",
        "CD audio plays on through the drive's reset",
    ),
    "unknown-command-switch-not-read": (
        "scsi.rs",
        "            if self.switches.unknown_command_check_condition {\n",
        "            if false {\n",
        "UNKNOWN_COMMAND_CHECK_CONDITION changes nothing",
    ),
    "read-lba-16-bits": (
        "scsi.rs",
        "        let lba = ((cdb[1] & 0x1F) as i64) << 16 | (cdb[2] as i64) << 8 | cdb[3] as i64;\n",
        "        let lba = (cdb[2] as i64) << 8 | cdb[3] as i64;\n",
        "READ(6) drops the LBA's top five bits",
    ),
    "no-reread-penalty": (
        "scsi.rs",
        "            self.sector_at = Some(at + MASTER_HZ * self.switches.reread_penalty_ms / 1000);\n",
        "            self.sector_at = Some(at + self.switches.sector_clocks);\n",
        "a missed sector comes round again one sector later, not 290 ms after its turn",
    ),
    "sel-in-data-in-not-an-abort": (
        "scsi.rs",
        "        } else if self.phase == Phase::DataIn {\n            self.abort();\n",
        "        } else if self.phase == Phase::Command {\n            self.abort();\n",
        "SEL during data in is ignored (and aborts the command phase instead)",
    ),
    "abort-one-status-byte": (
        "scsi.rs",
        "        self.enter_status(GOOD, self.switches.abort_status_bytes);\n",
        "        self.enter_status(GOOD, 1);\n",
        "the abort offers one status byte, not eight",
    ),
    "irq-sources-swapped": (
        "scsi.rs",
        "            self.irq_changes.push((IRQ_DATA_IN, data_in));\n",
        "            self.irq_changes.push((IRQ_STATUS_MESSAGE, data_in));\n",
        "REQ in data in raises the status/message IRQ bit",
    ),
    "head-not-moved-by-a-read": (
        "scsi.rs",
        "        self.head = self.read_next;\n",
        "",
        "the next seek starts from where the head was before the READ",
    ),
    "first-sector-a-sector-after-the-seek": (
        "scsi.rs",
        "        let seek = (self.seek_clocks(lba) - self.switches.seek_measurement_extra)\n",
        "        let seek = (self.seek_clocks(lba) + self.switches.sector_clocks)\n",
        "a READ's first sector comes one sector after Shadoff's whole time, not before it",
    ),
    "toc-track-type-audio": (
        "scsi.rs",
        "                    (track.start, if track.is_data { 0x04 } else { 0x00 })\n",
        "                    (track.start, 0x00)\n",
        "READ TOC calls every track audio",
    ),
    "toc-kind-3-as-msf": (
        "scsi.rs",
        "                if kind == 2 {\n",
        "                if kind >= 2 {\n",
        "READ TOC kind 3 answers the track's MSF, not its LBA",
    ),
    "toc-lead-out-track-unknown": (
        "scsi.rs",
        "                let (lba, control) = if cdb[2] == 0xAA {\n",
        "                let (lba, control) = if cdb[2] == 0xAB {\n",
        "READ TOC refuses track $AA, the lead-out",
    ),
    "subq-relative-from-zero": (
        "scsi.rs",
        "        let relative = track.map_or(0, |t| (lba - t.start).abs());\n",
        "        let relative = track.map_or(0, |_| lba);\n",
        "READ SUBQ's track-relative time counts from the disc's start",
    ),
    "ack-pulse-a-cycle-long": (
        "scsi.rs",
        "            auto_ack_clocks: 45,\n",
        "            auto_ack_clocks: 3,\n",
        "the $1808 auto-ACK pulse lasts one CPU cycle",
    ),
    "sense-not-cleared": (
        "scsi.rs",
        "        self.sense_key = SENSE_NONE;\n        self.sense_asc = 0;\n        let data",
        "        let data",
        "REQUEST SENSE leaves the sense in place",
    ),
    "one-data-latch": (
        "scsi.rs",
        "        if self.io() {\n            self.data\n        } else {\n            self.initiator_data\n        }\n",
        "        self.data\n",
        "$1801 reads the target's byte whatever the direction",
    ),
    "command-byte-from-the-target-latch": (
        "scsi.rs",
        "            Phase::Command => self.cdb.push(self.initiator_data),\n",
        "            Phase::Command => self.cdb.push(self.data),\n",
        "a command byte is taken from the target's latch, not the CPU's",
    ),
    "pause-without-audio-good": (
        "scsi.rs",
        "        if !self.cdda.active() {\n            self.fail(SENSE_ILLEGAL_REQUEST, ASC_SEQUENCE_ERROR);\n",
        "        if self.rst {\n            self.fail(SENSE_ILLEGAL_REQUEST, ASC_SEQUENCE_ERROR);\n",
        "PAUSE with no audio started answers GOOD",
    ),
    "audio-end-track-mode-not-before": (
        "scsi.rs",
        "            end = end.map(|end| end - 1);\n",
        "            end = end.map(|end| end + 0);\n",
        "AUDIO END by track number plays into the named track",
    ),
    "busy-select-keeps-the-audio-wait": (
        "scsi.rs",
        "            self.audio_wait = false;\n            self.selecting = true;\n",
        "            self.selecting = true;\n",
        "a new selection of a busy drive keeps waiting for the audio's report",
    ),
    "answer-wait-shows-data-in": (
        "scsi.rs",
        "        self.phase = if data.is_empty() {\n            Phase::Busy\n",
        "        self.phase = if data.is_empty() {\n            Phase::DataIn\n",
        "a command with no data waits with I/O up, as one with data does",
    ),
    "message-after-data-to-status": (
        "scsi.rs",
        "                        self.now + self.switches.status_to_message,\n",
        "                        self.now + self.switches.data_to_status,\n",
        "the message comes DATA_TO_STATUS after the status, not STATUS_TO_MESSAGE",
    ),
    "subq-a-one-byte-command": (
        "scsi.rs",
        "        0xD8 | 0xD9 | 0xDA | 0xDD | 0xDE => 10,\n",
        "        0xD8 | 0xD9 | 0xDA | 0xDE => 10,\n",
        "READ SUBQ is answered at its first byte",
    ),
    "sector-offered-under-ack": (
        "scsi.rs",
        "        if self.ack {\n            return; // REQ waits for ACK to drop\n        }\n",
        "",
        "a sector that arrives while ACK is up raises REQ at once",
    ),
    # adpcm.rs
    "low-nibble-first": (
        "adpcm.rs",
        "            self.high_nibble_next = false;\n            byte >> 4\n",
        "            self.high_nibble_next = false;\n            byte & 0x0F\n",
        "a byte's two samples both come from its low nibble",
    ),
    "length-16-bits": (
        "adpcm.rs",
        "pub const LENGTH_MASK: u32 = 0x1FFFF;",
        "pub const LENGTH_MASK: u32 = 0xFFFF;",
        "the length counter wraps at 16 bits (playing past zero without the stop bit)",
    ),
    "adpcm-rounds-half-away": (
        "adpcm.rs",
        "let value = round_ties_even(self.held as f64 * fader.gain(at, true)) as i32;",
        "let value = (self.held as f64 * fader.gain(at, true)).round() as i32;",
        "a faded ADPCM sample rounds a half away from zero (Rust's round), not to even (Python's)",
    ),
    "read-pointer-not-stepped-back": (
        "adpcm.rs",
        ".wrapping_sub(if value & 0x04 != 0 { 0 } else { 1 });",
        ".wrapping_sub(if value & 0x04 != 0 { 0 } else { 0 });",
        "`$180D` bit 3 copies the latch to the read pointer as it is, whatever bit 2 says",
    ),
    "decoder-keeps-its-state": (
        "adpcm.rs",
        "            self.playing = true;\n            self.signal = 0;\n            self.index = 0;\n",
        "            self.playing = true;\n            self.signal = 0;\n",
        "a play request restarts the signal but not the step index (C-ad1)",
    ),
    "end-flag-survives-the-length-latch": (
        "adpcm.rs",
        "            self.length = self.latch as u32;\n            self.set_end(false);\n",
        "            self.length = self.latch as u32;\n",
        "latching a new length leaves the end flag (and its IRQ2 source) up",
    ),
    "sample-clock-from-the-write": (
        "adpcm.rs",
        "                self.sample_at = (now << 16) + self.period_fixed();",
        "                self.sample_at = now << 16;",
        "the first sample of a playback comes at the play write, not a period after it",
    ),
    # arcade.rs
    "signed-offset-unsigned": (
        "arcade.rs",
        "            address += self.offset[port];\n            if control & CTL_SIGNED_OFFSET != 0 {\n                address += 0xFF0000;",
        "            address += self.offset[port];\n            if control & CTL_SIGNED_OFFSET != 0 {\n                address += 0x000000;",
        "the signed-offset bit does not reach below the base for an access",
    ),
    "rotate-shifts": (
        "arcade.rs",
        "                        self.value.rotate_left(count)",
        "                        self.value << count",
        "`$1AE5` shifts left where it should rotate",
    ),
    "base-increment-16-bits": (
        "arcade.rs",
        "self.base[port] = (self.base[port] + self.increment[port]) & 0xFFFFFF;",
        "self.base[port] = (self.base[port] + self.increment[port]) & 0xFFFF;",
        "an auto-incremented base wraps at 16 bits",
    ),
    "trigger-on-either-byte": (
        "arcade.rs",
        "                    if self.trigger(port) == TRIGGER_HIGH {",
        "                    if self.trigger(port) != TRIGGER_NONE {",
        "a write of the offset's high byte adds it into the base in every trigger mode",
    ),
    # unit.rs
    # "irq-raised-in-reset" (the unit's guard against a bus level raised while `$1804` holds
    # the drive in reset, disabled) is not a fault since 0.2.0: the bus now keeps RST through its
    # reset (pcenginecd-python issue 0001), so it raises no level while the bit is held and the
    # guard, which the reference keeps, cannot be reached.
    "bram-stays-unlocked": (
        "unit.rs",
        "                self.bram_locked = true; // [hw: pcetech 8] a read of $1803 locks BRAM\n",
        "",
        "a read of `$1803` does not lock the backup RAM",
    ),
    "dma-retry-late": (
        "unit.rs",
        "            self.dma_at = Some(at + 3); // the RAM is still taking the last byte: ask again",
        "            self.dma_at = Some(at + 6); // the RAM is still taking the last byte: ask again",
        "the CD-to-ADPCM DMA asks the busy RAM again every 6 master clocks, not 3",
    ),
    "latch-guard-not-set": (
        "unit.rs",
        "                    self.latch_guard_until = mclk + self.switches.channel_latch_guard;\n",
        "",
        "a second `$1805` write right after the first toggles the channel again",
    ),
    "drive-enables-kept-in-reset": (
        "unit.rs",
        "                    self.enable &= 0x8F; // the drive's three IRQ enables drop",
        "                    self.enable &= 0xFF; // the drive's three IRQ enables drop",
        "`$1804` bit 1 does not clear the drive's IRQ enables",
    ),
    "unit-forgets-the-fader": (
        "unit.rs",
        "        (fader.fade_register, fader.fade_start) = fade;\n",
        "",
        "a loaded state keeps the fader of the machine it is loaded into",
    ),
    "identity-without-the-disc": (
        "unit.rs",
        '            "CdUnit disc {} arcade {}",\n            self.disc_identity(),',
        '            "CdUnit disc {} arcade {}",\n            self.disc().is_some(),',
        "a state does not name its disc: another disc's machine takes it",
    ),
    # syscard.rs
    "super-ram-a-bank-short": (
        "syscard.rs",
        "pub const SUPER_RAM_FIRST_BANK: u32 = 0x68;",
        "pub const SUPER_RAM_FIRST_BANK: u32 = 0x69;",
        "the Super System Card's RAM starts at bank $69",
    ),
    "id-registers-region-blind": (
        "syscard.rs",
        "            5 => {\n                if us {",
        "            5 => {\n                if !us {",
        "`$18C5` answers the other region's value",
    ),
    "card-forgets-its-ram": (
        "syscard.rs",
        "            Some(ram) if ram.len() == data.len() => ram.copy_from_slice(data),",
        "            Some(ram) if ram.len() == data.len() => {}",
        "a loaded state keeps the Super System Card RAM of the machine it is loaded into",
    ),
    # board.rs
    "switches-not-handed-to-the-bus": (
        "board.rs",
        "        unit.bus.switches = switches.scsi;\n",
        "",
        "the builder leaves the drive's timings at their defaults whatever it is given",
    ),
    "arcade-card-always-fitted": (
        "board.rs",
        "let arcade = arcade_card.then(|| ArcadeCard::with(switches.arcade));",
        "let arcade = Some(ArcadeCard::with(switches.arcade));",
        "every machine has an Arcade Card",
    ),
}

GATE = [
    ("rung 2", ["--only", "^probe-base$"]),
    ("rung 4", ["--only", "^syscard-"]),
    ("rung 3", ["--only", "^(probe|sw)-"]),
]


def first_failure(stdout: str) -> str:
    return next(
        (line.split(":")[0] for line in stdout.splitlines() if line and not line.startswith(" ") and ": " in line),
        "?",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--work", type=Path, default=ROOT / "external" / "faults")
    parser.add_argument("--only")
    parser.add_argument("--from", dest="first", help="start at this fault (to go on after a stop)")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--no-discs", action="store_true", help="do not fall back to the real discs")
    args = parser.parse_args()
    if args.list:
        for name, (file, _, _, what) in FAULTS.items():
            print(f"{name}\t{file}\t{what}")
        return 0
    missed = ran = 0
    tree = args.work / "tree"
    target = args.work / "target"
    names = list(FAULTS)
    for name, (file, old, new, _) in FAULTS.items():
        if args.only and name != args.only:
            continue
        if args.first and names.index(name) < names.index(args.first):
            continue
        ran += 1
        for part in ("src", "tests", "examples"):
            if (tree / part).exists():
                shutil.rmtree(tree / part)
            # shutil.copy, not copytree's copy2: a restored file with its old time looks older than
            # the last fault's build to cargo, which then keeps that fault (found by supergrafx-rust,
            # carried 2026-11-19)
            shutil.copytree(ROOT / part, tree / part, copy_function=shutil.copy)
        manifest = (ROOT / "Cargo.toml").read_text().replace(
            'path = "../pcengine-rust"', f'path = "{(ROOT.parent / "pcengine-rust").as_posix()}"'
        )
        (tree / "Cargo.toml").write_text(manifest)
        shutil.copy(ROOT / "Cargo.lock", tree / "Cargo.lock")
        # src/lib.rs includes the README as a doctest (cfg(doctest) only, so no check here runs it,
        # but the tree is the crate's whole; carried from supergrafx-rust's seeder, 2026-11-19)
        shutil.copy(ROOT / "README.md", tree / "README.md")
        text = (ROOT / "src" / file).read_text()
        if text.count(old) != 1:
            print(f"{name}: the fault's text is in {file} {text.count(old)} times, not once")
            return 2
        (tree / "src" / file).write_text(text.replace(old, new))
        cargo = ["nice", "cargo", "--quiet", "--manifest-path", str(tree / "Cargo.toml")]
        build = subprocess.run(
            [*cargo[:2], "build", "--release", *cargo[2:], "--target-dir", str(target)],
            capture_output=True, text=True,
        )  # fmt: skip
        if build.returncode:
            print(f"{name}: does not build\n{build.stderr[-800:]}")
            return 2
        binaries = target / "release"

        def lockstep(extra: list[str]) -> str | None:
            done = subprocess.run(
                [sys.executable, str(ROOT / "scripts" / "lockstep_cd.py"), "--rust", str(binaries / "pcecd-trace"),
                 "--traces", str(args.work / "traces"), "--jobs", str(args.jobs), "--no-narrow", "--quiet", *extra],
                capture_output=True, text=True,
            )  # fmt: skip
            return None if done.returncode == 0 else first_failure(done.stdout)

        def tests(which: list[str]) -> str | None:
            done = subprocess.run(
                [*cargo[:2], "test", "--release", *cargo[2:], "--target-dir", str(target), *which],
                capture_output=True, text=True,
            )  # fmt: skip
            if done.returncode == 0:
                return None
            # `cargo test --quiet` names a failed test under "failures:", indented
            out = done.stdout.splitlines()
            failed = [line.strip() for n, line in enumerate(out)
                      if line.startswith("    ") and " " not in line.strip() and "failures:" in out[:n]]  # fmt: skip
            return failed[0] if failed else "?"

        caught = None
        for label, extra in GATE:
            first = lockstep(extra)
            if first is not None:
                caught = f"{label} (`{first}`)"
                break
        if caught is None:
            unit_tests = ["--test", "disc", "--test", "scsi", "--test", "cdda", "--test", "adpcm",
                          "--test", "arcade", "--test", "unit", "--test", "board",
                          "--test", "issue0001", "--test", "reference_notes"]  # fmt: skip
            first = tests(unit_tests)
            if first is not None:
                caught = f"rung 1 (`{first}`)"
        if caught is None:
            first = tests(["--test", "state"])
            if first is not None:
                caught = f"rung 6 (`{first}`)"
        if caught is None:
            states = subprocess.run(
                [sys.executable, str(ROOT / "scripts" / "state_roundtrip_cd.py"), "--synthetic-only",
                 "--binary", str(binaries / "pcecd-state-roundtrip")],
                capture_output=True, text=True,
            )  # fmt: skip
            if states.returncode:
                first = next((line.split(":")[0] for line in states.stdout.splitlines() if "FAIL" in line), "?")
                caught = f"rung 6 (`{first}`)"
        if caught is None and not args.no_discs:
            first = lockstep(["--only", "^disc-", "--jobs", "1"])
            if first is not None:
                caught = f"rung 5 (`{first}`)"
        print(f"{name}: {'caught by ' + caught if caught else 'MISSED'}", flush=True)
        missed += caught is None
    print(f"{ran - missed} caught, {missed} missed")
    return 1 if missed else 0


if __name__ == "__main__":
    raise SystemExit(main())
