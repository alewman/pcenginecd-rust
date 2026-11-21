#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Build scratch/cdprobe.pce: a synthetic 8 KiB card that drives the CD unit through its
registers, so the lockstep gate has a case on which every part of the add-on and every named
switch changes what the machine does.

Everything here is this repository's own: HuC6280 code assembled by the helper below (the shape
of pcengine-rust's ``scripts/make_probe.py``). It is not a ROM of anything, it takes the place of
the System Card in the slot (``PcEngineCd(card bytes, disc)``), and it is not committed
(scratch/ is ignored); the gate rebuilds it, byte for byte the same.

It expects the ``mixed`` disc of ``scripts/make_discs.py`` (audio 0-39, data 40-439, audio
440-469) and does, in order:

- **registers**: `$1800-$180F` at power-on, unmapped ones, the Super System Card ids `$18C0-C7`;
- **backup RAM**: locked reads, `$1807` unlock, the header, writes, the `$1803` read that locks;
- **CD RAM** (bank `$80`) and the **Super System Card RAM** (bank `$68`);
- **the drive**: TEST UNIT READY, READ TOC of every kind (and track `$AA`, the lead-out, and a
  kind there is not), an unknown command, a READ past the
  disc (and past it by the LBA's top bits alone) and REQUEST SENSE (18 bytes and the 4-byte
  form), a SEL while `$1804` holds the reset bit (no answer), READ through `$1801` with the
  handshake and through `$1808` with the auto-ACK, of data and of an audio sector, a count of
  zero, a late reader (the re-read penalty), a sector that arrives while ACK is held, the CPU's byte read back from `$1801`, SEL during
  data in and while a READ seeks (the abort's status bytes and message, taken as the BIOS takes
  them until BSY drops), the `$1804` reset in mid-transfer;
- **ADPCM**: the RAM port both ways with the busy bits read every 15 master clocks and polled,
  a write at length zero, the length latch, playback with
  and without the stop bit, the reset bit held, a signal driven into the rail, the CD-to-ADPCM
  DMA of a sector and its playback under the fader aimed at ADPCM;
- **CD audio**: start by track, by MSF and by LBA, playing and paused; the `$1805/$1806` latch
  with its guard; READ SUBQ while playing, paused and stopped; the fader slow and fast; PAUSE;
  AUDIO END in modes 1 (loop), 2 (busy to the end, then status), 3 (stop at the end) and 0;
  PAUSE with no audio (CHECK CONDITION and its sense); AUDIO END on a stopped player; a repeat of an empty range (busy, nothing
  plays); AUDIO END by track number; a new selection while AUDIO START seeks; a start past the disc; a data read that takes the head from
  the music;
- **the Arcade Card**: ID and version, unmapped registers, the four ports (base, offset,
  increment, control), data through `$1A00` and through banks `$40` and `$42`, the offset
  triggers, the signed offset, the `$1A40` mirror, the shift and rotate unit;
- **every IRQ2 source the unit has**: status/message (`$20`), data in (`$40`), ADPCM half
  (`$04`) and end (`$08`), each enabled through `$1802` with a handler that reads `$1803`,
  `$1800` and `$180C` and masks the sources again;
- and then for ever: music looping, ADPCM repeating, and the registers read in a loop (so a
  reset from the host, ``--reset FRAME:button|power``, shows).

Every wait has a timeout, so the probe goes on (differently) whatever a switch does.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ORG = 0xE000

# zero page (bank $F8 at $2000)
MASK, WANT, T0, T1, T2, PTR, LEN, NLO, NHI, D2, IRQS = 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x07, 0x08, 0x09, 0x0A, 0x0B
ZP = 0x2000

BRAM, CDRAM, SUPER, AC0, AC2 = 0x4000, 0x6000, 0x8000, 0xA000, 0xC000  # MPR2..MPR6
CD = 0x1800
AC = 0x1A00


class Asm:
    """Bytes, labels, and the few instructions the probe uses."""

    def __init__(self) -> None:
        self.code = bytearray()
        self.labels: dict[str, int] = {}
        self.fixups: list[tuple[int, str, bool]] = []  # (offset, label, relative)
        self.unique = 0

    @property
    def here(self) -> int:
        return ORG + len(self.code)

    def label(self, name: str) -> None:
        assert name not in self.labels, name
        self.labels[name] = self.here

    def local(self) -> str:
        self.unique += 1
        return f".{self.unique}"

    def b(self, *values: int) -> None:
        self.code.extend(values)

    def abs(self, opcode: int, target: int | str) -> None:
        if isinstance(target, str):
            self.b(opcode, 0, 0)
            self.fixups.append((len(self.code) - 2, target, False))
        else:
            self.b(opcode, target & 0xFF, target >> 8)

    def branch(self, opcode: int, target: str) -> None:
        self.b(opcode, 0)
        self.fixups.append((len(self.code) - 1, target, True))

    def link(self) -> bytes:
        for offset, name, relative in self.fixups:
            address = self.labels[name]
            if relative:
                distance = address - (ORG + offset + 1)
                assert -128 <= distance <= 127, f"branch to {name} too far: {distance}"
                self.code[offset] = distance & 0xFF
            else:
                self.code[offset] = address & 0xFF
                self.code[offset + 1] = address >> 8
        return bytes(self.code)

    # instructions
    def lda(self, value: int) -> None:
        self.b(0xA9, value & 0xFF)

    def lda_abs(self, address: int) -> None:
        self.abs(0xAD, address)

    def lda_zp(self, zp: int) -> None:
        self.b(0xA5, zp)

    def sta(self, address: int) -> None:
        self.abs(0x8D, address)

    def sta_zp(self, zp: int) -> None:
        self.b(0x85, zp)

    def stz_zp(self, zp: int) -> None:
        self.b(0x64, zp)

    def store(self, address: int, value: int) -> None:
        self.lda(value)
        self.sta(address)

    def tam(self, mask: int, bank: int) -> None:
        self.lda(bank)
        self.b(0x53, mask)

    def jsr(self, target: str) -> None:
        self.abs(0x20, target)

    def jmp(self, target: str) -> None:
        self.abs(0x4C, target)

    def rts(self) -> None:
        self.b(0x60)

    def beq(self, target: str) -> None:
        self.branch(0xF0, target)

    def bne(self, target: str) -> None:
        self.branch(0xD0, target)

    def bra(self, target: str) -> None:
        self.branch(0x80, target)


def subroutines(a: Asm) -> None:
    # wait: until ($1800 & $F8) == A, or about 1.7 s
    a.label("wait")
    a.sta_zp(WANT)
    a.stz_zp(T0)
    a.stz_zp(T1)
    a.lda(8)
    a.sta_zp(T2)
    a.label("wait.loop")
    a.lda_abs(CD)
    a.b(0x29, 0xF8)  # AND #$F8
    a.b(0xC5, WANT)  # CMP want
    a.beq("wait.done")
    a.b(0xE6, T0)  # INC t0
    a.bne("wait.loop")
    a.b(0xE6, T1)
    a.bne("wait.loop")
    a.b(0xC6, T2)  # DEC t2
    a.bne("wait.loop")
    a.label("wait.done")
    a.rts()

    # handshake: ACK up, REQ drops (at most 256 looks), ACK down; the IRQ enables kept
    a.label("handshake")
    a.lda_zp(MASK)
    a.b(0x09, 0x80)  # ORA #$80
    a.sta(CD + 2)
    a.stz_zp(T0)
    a.label("handshake.loop")
    a.lda_abs(CD)
    a.b(0x29, 0x40)  # AND #$40
    a.beq("handshake.low")
    a.b(0xE6, T0)
    a.bne("handshake.loop")
    a.label("handshake.low")
    a.lda_zp(MASK)
    a.sta(CD + 2)
    a.rts()

    # send: select, then the command at (PTR): its length, then its bytes. sendns: no select
    a.label("send")
    a.sta(CD)
    a.label("sendns")
    a.lda(0xD0)
    a.jsr("wait")
    a.b(0xA0, 0x00)  # LDY #0
    a.b(0xB1, PTR)  # LDA (ptr),Y
    a.sta_zp(LEN)
    a.label("send.next")
    a.b(0xC8)  # INY
    a.b(0xB1, PTR)
    a.sta(CD + 1)
    a.jsr("handshake")
    a.b(0xC4, LEN)  # CPY len
    a.beq("send.done")
    a.lda(0xD0)
    a.jsr("wait")
    a.bra("send.next")
    a.label("send.done")
    a.rts()

    # dec16: the byte count NLO/NHI, Z when it reaches zero
    a.label("dec16")
    a.lda_zp(NLO)
    a.bne("dec16.low")
    a.b(0xC6, NHI)
    a.label("dec16.low")
    a.b(0xC6, NLO)
    a.lda_zp(NLO)
    a.b(0x05, NHI)  # ORA nhi
    a.rts()

    # take1: NLO/NHI bytes through $1801 with the handshake. take8: through $1808 (auto-ACK)
    a.label("take1")
    a.lda(0xC8)
    a.jsr("wait")
    a.lda_abs(CD + 1)
    a.jsr("handshake")
    a.jsr("dec16")
    a.bne("take1")
    a.rts()
    a.label("take8")
    a.lda(0xC8)
    a.jsr("wait")
    a.lda_abs(CD + 8)
    a.jsr("dec16")
    a.bne("take8")
    a.rts()

    # finish: the status byte and the message byte; the bus ends free
    a.label("finish")
    a.lda(0xD8)
    a.jsr("wait")
    a.lda_abs(CD + 1)
    a.jsr("handshake")
    a.lda(0xF8)
    a.jsr("wait")
    a.lda_abs(CD + 1)
    a.jsr("handshake")
    a.lda(0x00)
    a.jsr("wait")
    a.rts()

    # delay: A times about 3.3 frames
    a.label("delay")
    a.sta_zp(D2)
    a.label("delay.outer")
    a.b(0xA2, 0x00)  # LDX #0
    a.label("delay.x")
    a.b(0xA0, 0x00)  # LDY #0
    a.label("delay.y")
    a.b(0x88)  # DEY
    a.bne("delay.y")
    a.b(0xCA)  # DEX
    a.bne("delay.x")
    a.b(0xC6, D2)
    a.bne("delay.outer")
    a.rts()

    # adwait: until $180C & A == 0 (the RAM port's busy bits, "playing"), at most 65536 looks
    a.label("adwait")
    a.sta_zp(WANT)
    a.stz_zp(T0)
    a.stz_zp(T1)
    a.label("adwait.loop")
    a.lda_abs(CD + 0x0C)
    a.b(0x25, WANT)  # AND want
    a.beq("adwait.done")
    a.b(0xE6, T0)
    a.bne("adwait.loop")
    a.b(0xE6, T1)
    a.bne("adwait.loop")
    a.label("adwait.done")
    a.rts()

    # drain: as the BIOS after selecting a busy drive, take every REQ (data, status, message)
    # with the handshake until BSY drops; at most 65536 looks with no REQ
    a.label("drain")
    a.stz_zp(T0)
    a.stz_zp(T1)
    a.label("drain.loop")
    a.lda_abs(CD)
    a.b(0x29, 0x80)  # AND #$80: BSY
    a.beq("drain.done")
    a.lda_abs(CD)
    a.b(0x29, 0x40)  # AND #$40: REQ
    a.bne("drain.take")
    a.b(0xE6, T0)
    a.bne("drain.loop")
    a.b(0xE6, T1)
    a.bne("drain.loop")
    a.bra("drain.done")
    a.label("drain.take")
    a.lda_abs(CD + 1)
    a.jsr("handshake")
    a.bra("drain.loop")
    a.label("drain.done")
    a.rts()

    # IRQ2: read what raised it, mask every source again (ACK kept), count
    a.label("irq2")
    a.b(0x48)  # PHA
    a.lda_abs(CD + 3)
    a.lda_abs(CD)
    a.lda_abs(CD + 0x0C)
    a.stz_zp(MASK)
    a.lda_abs(CD + 2)
    a.b(0x29, 0x80)
    a.sta(CD + 2)
    a.b(0xE6, IRQS)
    a.b(0x68)  # PLA
    a.b(0x40)  # RTI
    a.label("irq1")
    a.b(0x48)
    a.lda_abs(0x0000)  # the VDC's status: acknowledged
    a.b(0x68)
    a.b(0x40)
    a.label("tiq")
    a.sta(0x1403)
    a.label("nmi")
    a.b(0x40)


class Probe(Asm):
    def __init__(self) -> None:
        super().__init__()
        self.commands: list[bytes] = []

    def read(self, *addresses: int) -> None:
        for address in addresses:
            self.lda_abs(address)

    def enable(self, mask: int) -> None:
        self.lda(mask)
        self.sta_zp(MASK)
        self.sta(CD + 2)

    def command(self, *cmd: int, select: bool = True) -> None:
        self.commands.append(bytes([len(cmd), *cmd]))
        name = f"cmd{len(self.commands) - 1}"
        self.b(0xA9, 0)
        self.fixups.append((len(self.code) - 1, name + ".lo", True))
        self.sta_zp(PTR)
        self.b(0xA9, 0)
        self.fixups.append((len(self.code) - 1, name + ".hi", True))
        self.sta_zp(PTR + 1)
        self.jsr("send" if select else "sendns")

    def take(self, count: int, via: int = 1) -> None:
        self.lda(count & 0xFF)
        self.sta_zp(NLO)
        self.lda(count >> 8)
        self.sta_zp(NHI)
        self.jsr("take1" if via == 1 else "take8")

    def finish(self) -> None:
        self.jsr("finish")

    def delay(self, units: int) -> None:
        self.lda(units)
        self.jsr("delay")

    def wait(self, want: int) -> None:
        self.lda(want)
        self.jsr("wait")

    def adwait(self, bits: int) -> None:
        self.lda(bits)
        self.jsr("adwait")

    def latch(self, value: int) -> None:
        self.store(CD + 8, value & 0xFF)
        self.store(CD + 9, value >> 8)

    def subq(self) -> None:
        self.command(0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        self.take(10)
        self.finish()

    def link(self) -> bytes:
        # the command table, after the code; the immediate operands that point into it
        for index, cmd in enumerate(self.commands):
            address = self.here
            self.code.extend(cmd)
            for offset, name, _ in self.fixups:
                if name == f"cmd{index}.lo":
                    self.code[offset] = address & 0xFF
                elif name == f"cmd{index}.hi":
                    self.code[offset] = address >> 8
        self.fixups = [f for f in self.fixups if not f[1].startswith("cmd")]
        return super().link()


def read6(lba: int, count: int) -> tuple[int, ...]:
    return (0x08, (lba >> 16) & 0x1F, (lba >> 8) & 0xFF, lba & 0xFF, count, 0)


def program(a: Probe) -> None:
    a.label("reset")
    a.b(0x78, 0xD4, 0xD8)  # SEI, CSH, CLD
    a.tam(0x01, 0xFF)  # the hardware page at $0000
    a.tam(0x02, 0xF8)  # work RAM at $2000 (zero page, stack)
    a.b(0xA2, 0xFF, 0x9A)  # LDX #$FF, TXS
    a.tam(0x04, 0xF7)  # backup RAM at $4000
    a.tam(0x08, 0x80)  # CD RAM at $6000
    a.tam(0x10, 0x68)  # the Super System Card's RAM at $8000
    a.tam(0x20, 0x40)  # Arcade Card port 0 at $A000
    a.tam(0x40, 0x42)  # Arcade Card port 2 at $C000
    a.stz_zp(MASK)
    a.stz_zp(IRQS)
    a.abs(0x9C, 0x1402)  # STZ $1402: no interrupt disabled
    a.b(0x58)  # CLI

    # -- registers at power-on -----------------------------------------------------------------
    a.read(*(CD + r for r in range(16)), CD + 0x20, CD + 0xFF, *(CD + 0xC0 + r for r in range(8)))
    a.read(0x1000)  # the I/O port: D7 low with a unit on the port

    # -- backup RAM ----------------------------------------------------------------------------
    a.read(BRAM)
    a.store(BRAM + 0x10, 0x5A)
    a.store(CD + 7, 0x80)
    a.read(*(BRAM + i for i in range(8)), BRAM + 0x20, BRAM + 0x7FE)  # the image the host put back
    a.store(BRAM + 0x10, 0x5A)
    a.read(BRAM + 0x10, CD + 3, BRAM + 0x10)
    a.store(CD + 7, 0xC8)
    a.store(BRAM + 0x7FF, 0xA5)
    a.read(BRAM + 0x7FF, BRAM + 0x800)
    a.store(CD + 7, 0x00)
    a.read(BRAM + 0x7FF)

    # -- CD RAM, Super System Card RAM ---------------------------------------------------------
    a.store(CDRAM, 0x11)
    a.store(CDRAM + 0x1FFF, 0x22)
    a.read(CDRAM, CDRAM + 0x1FFF, CDRAM + 1)
    a.store(SUPER, 0x33)
    a.read(SUPER, SUPER + 0x1FFF)

    # -- the drive -----------------------------------------------------------------------------
    a.enable(0x20)  # IRQ2 source: status / message in
    a.command(0, 0, 0, 0, 0, 0)  # TEST UNIT READY
    a.finish()
    a.command(0xDE, 0, 0, 0, 0, 0, 0, 0, 0, 0)  # READ TOC: first and last track
    a.take(2)
    a.finish()
    a.command(0xDE, 1, 0, 0, 0, 0, 0, 0, 0, 0)  # the lead-out
    a.take(3)
    a.finish()
    a.command(0xDE, 2, 0x02, 0, 0, 0, 0, 0, 0, 0)  # track 2
    a.take(4)
    a.finish()
    a.command(0xDE, 2, 0x09, 0, 0, 0, 0, 0, 0, 0)  # a track the disc has not: CHECK CONDITION
    a.finish()
    a.command(0xDE, 3, 0x02, 0, 0, 0, 0, 0, 0, 0)  # kind 3: track 2's LBA and control
    a.take(4)
    a.finish()
    a.command(0xDE, 2, 0xAA, 0, 0, 0, 0, 0, 0, 0)  # the lead-out as track $AA
    a.take(4)
    a.finish()
    a.command(0xDE, 4, 0, 0, 0, 0, 0, 0, 0, 0)  # no such kind: CHECK CONDITION
    a.finish()
    a.command(0x42)  # an unknown command
    a.finish()
    a.command(*read6(0x200, 1))  # past the disc: CHECK CONDITION
    a.finish()
    a.command(*read6(0x01002F, 1))  # and past it by the top five bits of the 21-bit LBA alone
    a.finish()
    a.command(3, 0, 0, 0, 18, 0)  # REQUEST SENSE
    a.take(18)
    a.finish()
    a.command(3, 0, 0, 0, 0, 0)  # and its 4-byte form
    a.take(4)
    a.finish()
    a.command(*read6(47, 1))  # one sector through $1801
    a.take(2048)
    a.finish()
    a.enable(0x40)  # IRQ2 source: data in
    a.command(*read6(45, 3))  # three sectors through $1808
    a.take(3 * 2048, via=8)
    a.finish()
    a.command(*read6(3, 1))  # an audio sector read as data: zeros
    a.take(2048, via=8)
    a.finish()
    a.command(*read6(40, 0))  # nothing to read
    a.finish()
    a.command(*read6(60, 2))  # a late reader: one byte, a pause, the rest
    a.take(1, via=8)
    a.delay(3)
    a.take(2 * 2048 - 1, via=8)
    a.finish()
    a.command(*read6(70, 2))  # the next sector comes while the CPU holds ACK: REQ waits for it
    a.take(2047)
    a.wait(0xC8)
    a.read(CD + 1)
    a.lda_zp(MASK)
    a.b(0x09, 0x80)  # ORA #$80: ACK up, and held
    a.sta(CD + 2)
    a.delay(7)  # past the re-read of the sector missed while the first was taken
    a.read(CD)
    a.lda_zp(MASK)
    a.sta(CD + 2)  # ACK down: the held sector's first REQ
    a.take(2048)
    a.finish()
    a.store(CD + 1, 0x5A)  # the CPU's byte on the bus: read back while I/O is down
    a.read(CD + 1)
    a.command(*read6(50, 1))  # SEL during data in: the abort's status bytes, then the message
    a.wait(0xC8)
    a.store(CD + 1, 0xFF)  # as the BIOS: $FF on the bus, select, take every REQ
    a.sta(CD)
    a.read(CD, CD + 1)
    a.jsr("drain")
    a.command(*read6(300, 1))  # SEL while the READ still seeks: the same abort
    a.sta(CD)
    a.read(CD, CD + 1)
    a.jsr("drain")
    a.command(0, 0, 0, 0, 0, 0)  # and the drive answers a new selection
    a.finish()
    a.enable(0x60)
    a.command(*read6(41, 1))  # the $1804 reset in mid-transfer
    a.wait(0xC8)
    a.read(CD + 8, CD + 8, CD + 3)
    a.store(CD + 4, 0x02)
    a.read(CD, CD + 2, CD + 3, CD + 4)
    # a SEL with the reset bit still set: the drive stays in reset (RST is the CPU's line,
    # pcenginecd-python issue 0001), so nothing answers however long the card waits
    a.store(CD, 0)
    a.delay(1)
    a.read(CD, CD + 3)
    a.store(CD + 4, 0x00)
    a.read(CD, CD + 3)
    a.command(0, 0, 0, 0, 0, 0)  # released: the drive answers again
    a.finish()

    # -- ADPCM: the RAM port ---------------------------------------------------------------------
    pattern = bytes([0x77] * 20 + [0xF7, 0x7F, 0x08, 0x80] * 4 + [0xFF] * 12 + [0x1A, 0xC3, 0x5E, 0x90] * 4)
    a.latch(0x0000)  # a length of zero, the end flag cleared (the first read above raised it)
    a.store(CD + 0x0D, 0x10)
    a.store(CD + 0x0D, 0x00)
    a.read(CD + 0x0C, CD + 3)
    a.latch(0x1000)
    a.store(CD + 0x0D, 0x03)  # the write pointer takes the latch
    a.store(CD + 0x0D, 0x02)
    a.store(CD + 0x0A, 0x00)  # a write at length zero raises the end flag; how long it is busy
    a.read(*[CD + 0x0C] * 6)
    a.latch(0x1000)
    a.store(CD + 0x0D, 0x00)
    a.store(CD + 0x0D, 0x03)
    a.store(CD + 0x0D, 0x02)
    a.b(0xA2, 0x00)  # LDX #0
    a.label("adload")
    a.abs(0xBD, "pattern")  # LDA pattern,X
    a.sta(CD + 0x0A)
    a.adwait(0x04)
    a.b(0xE8, 0xE0, len(pattern))  # INX, CPX #n
    a.bne("adload")
    a.store(CD + 0x0D, 0x00)
    a.latch(0x1000)
    a.store(CD + 0x0D, 0x08)  # the read pointer: latch - 1, and the priming read
    a.read(CD + 0x0A, *[CD + 0x0C] * 6)  # how long a read is busy
    a.adwait(0x80)
    for _ in range(4):
        a.read(CD + 0x0A, CD + 0x0C)
        a.adwait(0x80)
    a.store(CD + 0x0D, 0x00)
    # play it to the end (stop bit set), with the half then the end source enabled
    for mask, rate in ((0x04, 0x0E), (0x08, 0x0B)):
        a.latch(0x1000)
        a.store(CD + 0x0D, 0x08)
        a.read(CD + 0x0A)
        a.adwait(0x80)
        a.store(CD + 0x0D, 0x00)
        a.latch(len(pattern))
        a.store(CD + 0x0D, 0x10)  # the length latch
        a.store(CD + 0x0D, 0x00)
        a.store(CD + 0x0E, rate)
        a.enable(mask)
        a.store(CD + 0x0D, 0x60)
        a.read(CD + 0x0C, CD + 3)
        a.delay(1)
        a.adwait(0x08)
        a.read(CD + 0x0C, CD + 3, CD + 0x0D, CD + 0x0E)
    # without the stop bit, then the reset bit held with play, then off; the length following
    # the latch while bit 4 is held
    a.latch(0x1000)
    a.store(CD + 0x0D, 0x0C)
    a.latch(8)
    a.store(CD + 0x0D, 0x10)
    a.store(CD + 0x0D, 0x00)
    a.store(CD + 0x0E, 0x0F)
    a.store(CD + 0x0D, 0x20)
    a.delay(1)
    a.read(CD + 0x0C, CD + 3)
    a.store(CD + 0x0D, 0xA0)
    a.read(CD + 0x0C)
    a.delay(1)
    a.read(CD + 0x0C, CD + 3)
    a.store(CD + 0x0D, 0x80)
    a.delay(1)
    a.store(CD + 0x0D, 0x10)
    a.latch(0x0003)
    a.store(CD + 0x0D, 0x00)
    a.read(CD + 0x0A)
    a.adwait(0x80)
    a.read(CD + 0x0C, CD + 3)

    a.latch(0xFFFF)  # the length's 17th bit: a write at $FFFF makes $10000, not zero
    a.store(CD + 0x0D, 0x10)
    a.store(CD + 0x0D, 0x00)
    for value in (0x01, 0x02):
        a.store(CD + 0x0A, value)
        a.adwait(0x04)
    a.read(CD + 0x0C, CD + 3)

    # -- the CD-to-ADPCM DMA, and its sector played under the ADPCM fade -------------------------
    a.store(CD + 0x0B, 0x03)  # bit 0 does not stick with no block waiting
    a.read(CD + 0x0B)
    a.store(CD + 0x0B, 0x00)
    a.latch(0x2000)
    a.store(CD + 0x0D, 0x03)
    a.store(CD + 0x0D, 0x02)
    a.store(CD + 0x0D, 0x00)
    a.command(*read6(100, 1))
    a.store(CD + 0x0B, 0x02)
    a.wait(0xD8)
    a.read(CD + 0x0B, CD + 3, CD + 0x0C)
    a.store(CD + 0x0B, 0x00)
    a.finish()
    a.latch(0x2000)
    a.store(CD + 0x0D, 0x08)
    a.read(CD + 0x0A)
    a.adwait(0x80)
    a.store(CD + 0x0D, 0x00)
    a.latch(0x0800)
    a.store(CD + 0x0D, 0x10)
    a.store(CD + 0x0D, 0x00)
    a.store(CD + 0x0E, 0x0E)
    a.store(CD + 0x0D, 0x60)
    a.store(CD + 0x0F, 0x0A)  # the fader, slow, aimed at ADPCM
    a.delay(2)
    a.store(CD + 0x0F, 0x0E)  # fast
    a.delay(2)
    a.read(CD + 0x0F, CD + 0x0C)
    a.store(CD + 0x0F, 0x00)

    # -- CD audio --------------------------------------------------------------------------------
    a.enable(0x20)
    a.command(0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80)  # AUDIO START: track 1, playing
    a.finish()
    a.sta(CD + 5)  # the channel latch: toggles to the right channel
    a.sta(CD + 5)  # within the guard time
    a.read(CD + 3, CD + 5, CD + 6)
    a.delay(1)
    a.sta(CD + 5)
    a.read(CD + 3, CD + 5, CD + 6)
    a.subq()
    a.store(CD + 0x0F, 0x08)  # the fader: slow
    a.delay(3)
    a.subq()
    a.store(CD + 0x0F, 0x0C)  # fast
    a.delay(2)
    a.store(CD + 0x0F, 0x00)
    a.command(0xDA, 0, 0, 0, 0, 0, 0, 0, 0, 0)  # PAUSE
    a.finish()
    a.subq()
    a.command(0xD9, 1, 0, 0, 0, 30, 0, 0, 0, 0x00)  # AUDIO END: loop to LBA 30; the bus stays busy
    a.delay(4)
    a.read(CD)
    a.subq()
    a.command(0xD8, 0, 0x00, 0x02, 0x20, 0, 0, 0, 0, 0x40)  # AUDIO START: 00:02:20, paused
    a.finish()
    a.subq()
    a.enable(0x20)
    a.command(0xD9, 2, 0, 0, 0, 23, 0, 0, 0, 0x00)  # AUDIO END: to LBA 23, busy until then
    a.finish()
    a.subq()
    a.command(0xD8, 1, 0, 0, 0x01, 0xBD, 0, 0, 0, 0x00)  # AUDIO START: LBA 445
    a.finish()
    a.command(0xD9, 3, 0, 0, 0x01, 0xC0, 0, 0, 0, 0x00)  # AUDIO END: stop at LBA 448
    a.finish()
    a.delay(2)
    a.subq()
    a.command(0xD8, 1, 0x03, 0, 0, 0, 0, 0, 0, 0x80)  # track 3
    a.finish()
    a.delay(1)
    a.command(0xD9, 0, 0, 0, 0, 0, 0, 0, 0, 0)  # AUDIO END mode 0: stop
    a.finish()
    a.subq()
    a.command(0xDA, 0, 0, 0, 0, 0, 0, 0, 0, 0)  # PAUSE with no audio: CHECK CONDITION ...
    a.finish()
    a.command(3, 0, 0, 0, 18, 0)  # ... and the sense says why
    a.take(18)
    a.finish()
    a.command(0xD9, 3, 0, 0, 0x01, 0xD0, 0, 0, 0, 0x00)  # AUDIO END on the stopped player: it plays on
    a.finish()
    a.delay(1)
    a.command(0xD8, 1, 0x03, 0, 0, 0, 0, 0, 0, 0x80)  # track 3 ...
    a.finish()
    a.command(0xD9, 1, 0x02, 0, 0, 0, 0, 0, 0, 0x80)  # ... repeated up to track 2: an empty range
    a.delay(1)
    a.read(CD)
    a.subq()
    a.command(0xD8, 1, 0, 0, 0, 35, 0, 0, 0, 0x00)  # LBA 35 ...
    a.finish()
    a.command(0xD9, 3, 0x02, 0, 0, 0, 0, 0, 0, 0x80)  # ... to the end of track 1 (track mode)
    a.finish()
    a.delay(2)  # past the end: the head stops before track 2
    a.subq()
    a.command(0xD8, 1, 0, 0, 0x01, 0xBD, 0, 0, 0, 0x00)  # AUDIO START, LBA 445 ...
    a.command(0, 0, 0, 0, 0, 0)  # ... selected again while it seeks: TEST UNIT READY
    a.finish()
    a.command(0xD9, 1, 0, 0, 0x01, 0xC8, 0, 0, 0, 0x00)  # a repeat to LBA 456: busy, nothing owed
    a.delay(8)  # the seek ends meanwhile: no status comes
    a.read(CD)
    a.subq()
    a.command(0xD8, 1, 0, 0x10, 0, 0, 0, 0, 0, 0x00)  # a start past the disc
    a.finish()
    a.command(0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80)  # music again ...
    a.finish()
    a.delay(1)
    a.command(*read6(400, 1))  # ... and a data read takes the head
    a.take(2048, via=8)
    a.finish()
    a.subq()

    # -- the Arcade Card -------------------------------------------------------------------------
    a.read(AC + 0xFF, AC + 0xFE, AC + 0xFD, AC + 0xFC, AC + 0x0A, AC + 0x0B, AC + 0x90, AC + 0xE6, AC + 0xFB)
    a.read(AC, AC0 + 5, AC2 + 9)  # RAM nothing has written
    a.store(AC + 2, 0x34)  # port 0: base $001234
    a.store(AC + 3, 0x12)
    a.store(AC + 4, 0x00)
    a.store(AC, 0xAB)
    a.read(*(AC + r for r in range(10)))
    a.store(AC + 0x12, 0x00)  # port 1: base $000100, increment 2, offset added and stepped
    a.store(AC + 0x13, 0x01)
    a.store(AC + 0x17, 0x02)
    a.store(AC + 0x19, 0x03)
    for value in (0x10, 0x11, 0x12):
        a.store(AC + 0x10, value)
    a.read(AC + 0x15, AC + 0x16, AC + 0x12)
    a.store(AC + 0x15, 0x00)
    a.read(AC + 0x10, AC + 0x10, AC + 0x10)
    a.store(AC + 7, 0x10)  # port 0: the increment goes to the base; data through bank $40
    a.store(AC + 9, 0x11)
    a.store(AC0, 0x07)
    a.store(AC0 + 0x123, 0x08)
    a.read(AC + 2, AC + 3, AC + 5)
    a.store(AC + 0x22, 0x00)  # port 2 through bank $42
    a.store(AC + 0x23, 0x03)
    a.store(AC + 0x27, 0x01)
    a.store(AC + 0x29, 0x11)
    a.store(AC2, 0xC1)
    a.store(AC2 + 0x1FFF, 0xC2)
    a.store(AC + 0x22, 0x00)
    a.read(AC2, AC2 + 1, AC + 0x62, AC + 0x63)  # and the mirror at $1A60
    a.store(AC + 0x32, 0x00)  # port 3: a signed offset below the base, then unsigned
    a.store(AC + 0x33, 0x01)
    a.store(AC + 0x35, 0xF0)
    a.store(AC + 0x36, 0xFF)
    a.store(AC + 0x39, 0x0A)
    a.store(AC + 0x30, 0x5A)
    a.store(AC + 0x39, 0x02)
    a.store(AC + 0x30, 0x5B)
    a.read(AC + 0x30, AC + 0x39)
    a.store(AC + 0x39, 0x0A)
    a.read(AC + 0x30)  # the byte below the base again
    a.store(AC + 0x32, 0xFF)  # a base that steps past 16 bits
    a.store(AC + 0x33, 0xFF)
    a.store(AC + 0x37, 0x02)
    a.store(AC + 0x39, 0x11)
    a.read(AC + 0x30, AC + 0x32, AC + 0x33, AC + 0x34)
    a.store(AC + 9, 0x20)  # the offset triggers: on the low byte,
    a.store(AC + 6, 0x01)
    a.read(AC + 3)
    a.store(AC + 5, 0x05)
    a.read(AC + 2, AC + 3)
    a.store(AC + 0x19, 0x40)  # on the high byte,
    a.store(AC + 0x15, 0x34)
    a.read(AC + 0x12)
    a.store(AC + 0x16, 0x12)
    a.read(AC + 0x12, AC + 0x13)
    a.store(AC + 0x29, 0x68)  # on a write to register $A, signed
    a.store(AC + 0x25, 0xFE)
    a.store(AC + 0x26, 0xFF)
    a.store(AC + 0x2A, 0x00)
    a.store(AC + 0x2A, 0xFF)
    a.read(AC + 0x22, AC + 0x23, AC + 0x24, AC + 0x2A, AC + 0x29)
    a.store(AC + 0x39, 0xFF)
    a.read(AC + 0x39)
    for i, value in enumerate((0x78, 0x56, 0x34, 0x12)):  # the shift unit
        a.store(AC + 0xE0 + i, value)
    for count in (0x04, 0x0C, 0x08, 0x0F, 0x00, 0x13):
        a.store(AC + 0xE4, count)
        a.read(*(AC + 0xE0 + i for i in range(5)))
    a.store(AC + 0xE3, 0x81)
    for count in (0x01, 0x0F, 0x1D, 0x07):
        a.store(AC + 0xE5, count)
        a.read(*(AC + 0xE0 + i for i in range(6)))

    # -- for ever: music looping, ADPCM repeating, the registers read ------------------------------
    a.store(CD + 7, 0x80)  # backup RAM unlocked (a power-on reset locks it)
    a.command(0xD8, 1, 0x03, 0, 0, 0, 0, 0, 0, 0x80)
    a.finish()
    a.command(0xD9, 1, 0, 0, 0x01, 0xD0, 0, 0, 0, 0x00)  # loop track 3 to LBA 464
    a.latch(0x1000)
    a.store(CD + 0x0D, 0x0C)
    a.latch(0x0030)
    a.store(CD + 0x0D, 0x10)
    a.store(CD + 0x0D, 0x00)
    a.store(CD + 0x0E, 0x04)
    a.store(CD + 0x0D, 0x20)
    a.enable(0x7C)
    a.label("main")
    a.read(CD, CD + 2, CD + 0x0C, CD + 0x0D, CD + 0x0F, BRAM, AC + 2, AC + 0x12, AC + 0x19, AC + 0xE0, AC + 0xE4)
    a.sta(CD + 5)
    a.read(CD + 5, CD + 6)
    a.b(0xA2, 0x00)  # LDX #0: a short pause between rounds
    a.label("main.pause")
    a.b(0xCA)
    a.bne("main.pause")
    a.jmp("main")

    subroutines(a)
    a.label("pattern")
    a.code.extend(pattern)


def build() -> bytes:
    a = Probe()
    program(a)
    code = a.link()
    if len(code) > 0x2000 - 10:
        raise SystemExit(f"the probe is {len(code)} bytes: more than a bank")
    image = bytearray(code + bytes(0x2000 - len(code)))
    vectors = ((0x1FF6, "irq2"), (0x1FF8, "irq1"), (0x1FFA, "tiq"), (0x1FFC, "nmi"), (0x1FFE, "reset"))
    for offset, name in vectors:
        address = a.labels[name]
        image[offset] = address & 0xFF
        image[offset + 1] = address >> 8
    return bytes(image)


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "scratch" / "cdprobe.pce"
    out.parent.mkdir(parents=True, exist_ok=True)
    image = build()
    if not out.is_file() or out.read_bytes() != image:
        out.write_bytes(image)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
