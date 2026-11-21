<!-- SPDX-License-Identifier: MIT -->
# The ladder in detail

The README has the results table and the commands; this is how each rung is built and what its
cases are. The figures are those of the runs recorded at the commits that added them (2026-11-12,
Linux x86_64, rustc 1.93.1, the reference on PyPy 7.3.20 with `PYPY_GC_NURSERY=1M`).

## The gate

`scripts/lockstep_cd.py` runs each case of `scripts/cases_cd.tsv` on both sides:
`scripts/pcecd_trace.py` builds the reference's `PcEngineCd` on PyPy (the pinned checkouts of
`external/` on `PYTHONPATH`) and `target/release/pcecd-trace` builds this crate's from the same
command line. Each writes one line a frame: the base gate's line (pcengine-rust's
`scripts/pce_trace.py`, imported by path, and its `trace::frame_line`: the frame counter, the master
clock, the count and SHA-1 of the CPU bus log, the SHA-1 of the picture, the count and SHA-1 of the
mixed audio and every field of the CPU's state) and `cmd=N:SHA-1`, the drive's command log of the
frame (each command's bytes with the master clock the bus had when it ran). A case passes when the
two files are identical. Otherwise the first differing frame is named and both sides run again with
`--detail` on it, which narrows the difference to the first differing bus cycle (the base gate's
comparison and narrowing, imported by path from a checkout of
[pcengine-rust](https://github.com/alewman/pcengine-rust): `$PCENGINE_RUST`, default
`../pcengine-rust`, at the revision `Cargo.toml` names).

The reference runs at an eighth to a quarter of real time, and its trace of a case never changes while
the pins do not: each is kept in `external/refcache`, named by the case and a hash of its frames,
card, disc, arguments, the reference checkouts' commits and the base gate's `pce_trace.py`, so a rerun costs the Rust side only (the 45 probe cases then take half a
minute on six cores, against about seven minutes of six PyPy processes; the six discs, with their
extraction, two and a half minutes against an hour and a half). A real disc is extracted with `chdman
extractcd` into the ignored `scratch/disc/`, **one at a time**, and removed when its case ends. A
change to the probe card or to a synthetic disc changes the cache key of every probe case; the discs'
traces are keyed by the CHD's name and size.

In all, 42,100 frames in 52 cases, and no case has ever differed: the transcription was identical to
the reference on the first run of each. Again on 2026-11-19, after the reference's clean rewrite of
the drive's bus (pcenginecd-python `4abb3f5`, [restart.md](restart.md)) was transcribed: 56 cases,
46,320 frames (the 49 probe cases now run 600 frames each), all identical on their first run, the
reference cache rebuilt from the new reference (the old one kept as `external/refcache-v0.2.0`).
Again later that day, on the clean base board (pcengine-rust `37f17ba`, the reference
pcenginecd-python `5c06d1b` on pcengine-python `03ac699`; [restart.md](restart.md), "The clean base
board"): 56 cases, 48,100 frames, all identical on their first run, the cache rebuilt (the
`4abb3f5` one kept as `external/refcache-4abb3f5`; the key now also hashes the base gate's
`pce_trace.py`). The reference's traces themselves changed from the early frames on (the new VDC),
so this was a new comparison, not a replay. (The per-cycle narrowing has therefore only been exercised by
the seeded faults.)

## Rungs 2 and 3: the probe card

`scripts/make_cdprobe.py` assembles an 8 KiB card, this repository's own code and not a ROM of
anything, that takes the System Card's place in the slot and drives the unit as a BIOS would, through
`$1800-$180F`, `$18C0-$18C7`, `$1A00-$1AFF` and the banks the unit claims: the registers at power-on;
backup RAM (locked reads, the unlock, the header, the lock by a read of `$1803`); CD RAM and the Super
System Card's RAM; TEST UNIT READY, READ TOC of every kind, an unknown command, a READ past the disc
and REQUEST SENSE in both lengths; READ through `$1801` with the handshake and through `$1808` with
the auto-ACK, of data and of an audio sector, with a count of zero, by a late reader (the re-read
penalty), with a sector arriving while the CPU holds ACK, aborted by SEL in data in and while a READ
seeks (since 2026-11-19: eight status bytes and the message, taken as the BIOS takes them, every REQ
until BSY drops), and reset through `$1804` in mid-transfer; the ADPCM RAM port both ways with the busy bits
read every 15 master clocks, a write at length zero, the length latch, playback with and without the
stop bit, the reset bit held, a signal driven into the rail, the CD-to-ADPCM DMA of a sector and its
playback under the fader aimed at ADPCM; CD audio started by track, by MSF and by LBA, playing and
paused, the `$1805/$1806` latch and its guard, READ SUBQ while playing, paused and stopped, the fader
slow and fast, PAUSE, AUDIO END in modes 1 (loop), 2 (busy to the end, then status), 3 and 0, by track
number, PAUSE with no audio and its sense, AUDIO END on a stopped player, a repeat of an empty range, a new selection while AUDIO
START seeks, a start
past the disc and a data read that takes the head from the music; the Arcade Card's ID, its four ports
through `$1A00` and through banks `$40` and `$42`, the offset triggers, the signed offset, the mirror,
and the shift and rotate unit. **Every IRQ2 source the unit has** (status/message `$20`, data in
`$40`, ADPCM half `$04` and end `$08`) is enabled in turn and taken by a handler that reads `$1803`,
`$1800` and `$180C` (eight interrupts in the default run; the subchannel source `$10` is not built in
the reference). It ends in a loop, music and ADPCM repeating, that reads the registers for ever, so a
reset from the host shows. 61 drive commands, 7,695 ADPCM samples, the for-ever loop from about frame 565 (43 and 9,636 at 0.2.0, in 520 frames). Every wait has a timeout, so the
card goes on, differently, whatever a switch does. `examples/probe_report.rs` says what a run of it
touched.

The cases, 600 frames each (520 before 2026-11-19) on the synthetic discs of `scripts/make_discs.py` (`mixed`: audio, data,
audio; `data`: one data track; `pregap`: two FILEs with stored `INDEX 00` pregaps, the shape `chdman`
writes):

| Cases | What is crossed |
| --- | --- |
| `probe-base` | the Arcade Card fitted, the card built as a Super System Card |
| `probe-plain`, `probe-no-super`, `probe-us-tg16` | no Arcade Card and a plain System Card (by size); a Super System Card refused by `super_system_card = false`; the US ids on the TurboGrafx-16 row |
| `probe-no-disc`, `probe-data-disc`, `probe-pregap-disc` | an empty drive (every command but REQUEST SENSE fails), a disc without audio, a disc in two files with pregaps |
| `probe-bram` | a backup RAM image put back by the host |
| `probe-reset-button`, `probe-reset-power`, `probe-reset-power-early` | `Board::reset` from the host: `Button` at frame 580; `PowerOn` at frame 590 with music and ADPCM running (460 and 470 before the probe grew); `PowerOn` at frame 250 in the middle of the ADPCM section, then `Button` |
| 37 `sw-` cases | each of the 34 named switches away from its default: the drive's sixteen (`ScsiSwitches`, since 2026-11-19; twelve before), the unit's two (`UnitSwitches`), the fader's four times and `PCETECH_FADE` and `SUBQ_STATUS` (the reference's `CddaPlayer` attributes), the ADPCM unit's six (`AdpcmSwitches`; the two busy times at Mednafen's values and at 0), the Arcade Card's five (`ArcadeSwitches`); the DMA's delay both shorter (14 master clocks, where the DMA finds the RAM still busy and asks again, on a step where asking every 3 clocks and every 6 differ; 30 before the 45-clock auto-ACK made that too long to find it busy) and longer |

**A switch that changes nothing proves nothing**: if the reference's trace with the switch is its
trace without, both sides agree whether or not this crate reads the switch.
`scripts/switch_effect_cd.py` compares each case's reference trace with its baseline's (the probe with
every default; for the two `PCETECH_*_SECONDS` the `PCETECH_FADE` case; for `RESET_CLEARS_REGISTERS`
the `Button` reset) and fails if a case changes nothing. The first run found six that did: the probe
never read backup RAM past the header before writing it, read the ADPCM busy bits only after both
busy times had passed, and wrote ADPCM RAM "at length zero" with the end flag already up (its own
first read of `$180A` had raised it). It does all of that now: 43 of 43 change the trace, and the
`Button` reset alone, which does nothing to this machine unless a switch says so, changes nothing, as
it should.

## Rung 4: the System Card

`syscard3.pce` on the `mixed` disc, RUN held for 20 frames from frame 450 (the reference's
`tests/test_boot_slow.py`): the BIOS draws its screen, takes RUN, issues TEST UNIT READY, READ TOC and
READ, and finds no IPL. 700 frames, 6 drive commands, identical.

## Rung 5: real discs

Each CHD of the author's library is extracted with `chdman extractcd` into `scratch/disc/`, run on both
sides with `syscard3.pce` on the PC Engine row with RUN held for 20 frames from frame 450 (the runs of
the reference's records), and removed; one disc at a time.

| Disc | Extracted | Arcade Card | Drive commands | What plays | The reference's trace took | Result |
| --- | --- | --- | --- | --- | --- | --- |
| Ryuuko no Ken (Japan) (En,Ja,Es) (FACT) | 464 MiB, 32 tracks | fitted | 12 | CD audio from frame 1154; ADPCM in four stretches (47,410 samples) | 311 s | identical, 3,000 frames |
| Mad Stalker - Full Metal Force (Japan) | 586 MiB, 34 tracks | fitted | 455 (streaming reads) | CD audio in frames 964-1253 and from 2275 | 694 s | identical, 3,000 frames |
| Ginga Fukei Densetsu Sapphire (Japan) | 389 MiB, 17 tracks | fitted | 19 | CD audio from frame 1792 | 968 s | identical, 3,000 frames |
| The Addams Family (USA) | 13 MiB, 1 track | no | 18 | ADPCM from frame 1005 (533,755 samples, DMA'd from the disc) | 1,113 s | identical, 3,000 frames |
| Pyramid Plunder (USA) (Unl) | 2 MiB, 1 track | no | 12 | ADPCM (114,688 samples, two ends) | 1,243 s | identical, 3,000 frames |
| Insanity (USA) (Unl) | 212 MiB, 13 tracks | no | 10 | CD audio from frame 603 (a looping track) | 1,178 s | identical, 3,000 frames |

18,000 frames; every bus cycle, every picture and each of the 13.3 million mixed samples equal. The
command counts and the ADPCM sample counts are also those the reference's own records give for these
runs (m7, m8 and m9: 12, 455 and 19 commands; 533,755 samples and one end flag; 114,688 and two). No
disc was left out. What these runs are is what the reference's were: openings and first screens, with
no input beyond RUN.

2026-11-19, after the clean rewrite of the drive's bus (reference `4abb3f5`): the six discs again,
identical, 18,000 frames (the reference's traces took 471 to 999 s at load 35-40). The drive
commands in 3,000 frames: Mad Stalker 411 (426 at v0.2.0: fewer READ SUBQ polls in the CD-audio
wait, as the reference's own record says), the others as before (12, 19, 18, 12, 10); the System
Card on `mixed` still 6. Again on the clean base board (reference `5c06d1b`, pcengine-rust
`37f17ba`): the six identical, 18,000 frames (226 to 453 s a disc, both sides), the drive commands
the same (12, 411, 19, 18, 12, 10; the System Card on `mixed` 6).

## Rung 6: save states

The base board's `save_state` / `load_state` and its snapshot pair cover this machine through
`DeviceState`: nothing of the base crate was changed. As the reference's `scripts/cd_state.py`
(`trace::roundtrip` over this machine): a run is saved some instructions into a frame, the state
loaded into a fresh machine (which opens the disc itself) and into one that had run half as long, and
the 60 frames after compared by the gate's own lines; the loaded machine saved at once gives the same
bytes. Then the snapshot pair: 30 frames, after each a snapshot, two frames run ahead with other
buttons held, the snapshot put back; every frame's line and the end state equal the plain run's.

| Run | Saved | A state | A snapshot |
| --- | --- | --- | --- |
| The System Card on the `mixed` disc (rung 4) | 777 instructions into frame 480, while the BIOS reads the disc | 10 KiB | 569 KiB |
| The probe card | 1,234 instructions into frame 200 (ADPCM playing), and 4,321 into frame 265 (CD audio playing) | 4 KiB | 2.6 MiB (the Arcade Card's RAM) |
| Ryuuko no Ken | 777 instructions into frame 2000 (Arcade Card, CD audio and ADPCM in use) | 340 KiB | 2.6 MiB |
| Mad Stalker | 2,001 instructions into frame 1500 (streaming reads) | 82 KiB | 2.6 MiB |

`tests/state.rs` has the reference's `tests/test_state.py` (the unit, the ADPCM RAM, the Arcade Card
and the Super System Card's RAM come back; a state holds no sector and no System Card byte) and the
refusals, each leaving the machine untouched: **another disc** ("a state made with another expansion
device or other media (another disc?)"; two discs with one table of contents are not told apart, the
reference's rule), an empty drive, **another System Card** (another image, or the same image without
its Super RAM), **a machine without the Arcade Card** (and one with it, for a state made without); and
the unit's own round trip in the middle of a read with ADPCM playing and the fader set, register for
register and sample for sample; that loading keeps the machine's switches and wiring; the snapshot
pair.

2026-11-19, on the clean base board (pcengine-rust `37f17ba`): a state is the base board's format
**version 3** (0.2.0's states, version 2, are refused, not converted; `tests/state.rs`
`a_state_is_the_base_boards_version_3_and_an_older_one_is_refused`). The five runs pass again (the
System Card's state 10,327 bytes, the probe card's 4,118 and 4,244, Ryuuko no Ken's 348,170, Mad
Stalker's 80,641); 10 unit tests.

## Rung 7: seeded faults

[seeded-faults.md](seeded-faults.md).
