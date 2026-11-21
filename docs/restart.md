<!-- SPDX-License-Identifier: MIT -->
# Restart: the drive's bus, carried from the reference's clean rewrite (2026-11-19)

The provenance audit of 2026-11-13 ([provenance.md](provenance.md)) marked one part of this crate
**rewrite**: `src/scsi.rs`, the transcription of the reference's `scsi.py`, whose state machine was
named after a GPL emulator's (Mesen2's) and whose timing constants were converted from that
emulator's code. The reference replaced `scsi.py` with a clean rewrite (pcenginecd-python
`4abb3f5`, its `docs/restart.md`); this pass carries that rewrite here.

## The rules of this pass

The PC Engine family's clean-rewrite rules, as the reference applied them, adapted to a
transcription:

1. **No GPL or LGPL source is read**: Mednafen, Beetle PCE, Mesen2, Geargrafx, the TurboGrafx16
   MiSTer core, MAME's GPL files. Not to learn, not to check a constant, not when a comparison
   fails.
2. **The code being replaced is not read.** `src/scsi.rs` was moved aside, unread, into the
   git-ignored `scratch/withdrawn/` (with a small Python helper, not a shell one-liner) before
   anything was written, and was not opened again. The seeded faults that quoted its text were
   removed from `scripts/seeded_faults_cd.py` by a script that printed only their names, and new
   ones were written against the new text. Nothing else in the crate followed GPL source
   (provenance.md), so nothing else was withdrawn.
3. **The source transcribed is the reference's new `scsi.py`**, read whole (not as a diff against
   the withdrawn module), line by line: the same names, the same order of effects, its comments
   and evidence tags. Its interface changes in `unit.py` (`bus.data_waiting` for `len(bus.out)`)
   and `cdda.py` (`start`) are carried; the reference's comment-only changes in `adpcm.py`,
   `arcade.py`, `cdda.py`, `disc.py` and `unit.py` are carried as the same words, so no comment
   here names a GPL emulator's files or functions.
4. **The facts the reference takes from MAME (BSD-3-Clause)** are credited in
   [NOTICE.md](../NOTICE.md) with those files' copyright lines; nothing of MAME's code is here.
5. **The gate is the reference**, as before: every bus cycle, picture, audio sample, CPU state and
   drive command identical, frame by frame. The reference's own judges (its probe against
   Geargrafx, its disc boots) are its business; its provenance.md has them.
6. House rules: only this repository edited; `nice`, at most 8 processes; romsets and the System
   Card by environment variable (`$SYSCARD`, `$LIBCD`); commits local, no push, no tag.

## What was replaced or changed

| Item | Now |
| --- | --- |
| `src/scsi.rs` | a new transcription of the reference's `scsi.py` at `4abb3f5`: `Phase::Busy`; one pending protocol step (`step`, `step_at`, a `Step` enum for the reference's method name), the READ's sector clock (`sector_at`) and the auto-ACK end (`ack_off_at`); `run_to` handles the ACK end, then the step, then the sector, then the player; the timings re-measured by the reference (closed box) as `ScsiSwitches` fields; two data latches; READ TOC kind 3 and track `$AA`; PAUSE without audio, the empty-range repeat, AUDIO END by track, the abort's eight status bytes |
| `src/unit.rs` | `bus.io()`, `bus.cd()` (now derived from the phase) and `bus.data_waiting()` for `bus.out.is_empty()`; comments re-sourced as the reference's |
| `src/cdda.rs` | `start` documented as the reference's new read-only `start`; comments re-sourced |
| `src/adpcm.rs`, `src/arcade.rs`, `src/disc.rs` | comments only, the reference's new words |
| `src/trace.rs` | the `--switch scsi.NAME` names follow the reference's new class attributes |
| `scripts/cases_cd.tsv` | the 13 old `sw-scsi.*` cases replaced by 16, one per new switch; the probe cases at 600 frames; the short `DMA_BYTE_CLOCKS` case at 14 clocks (at 30, with the new 45-clock auto-ACK, the DMA never found the RAM busy) |
| `scripts/make_cdprobe.py` | the probe card follows the new drive: READ TOC kind 3, track `$AA`, a kind there is not, and a missing track (now CHECK CONDITION, not four bytes); the CPU's byte read back from `$1801`; SEL during data in and while a READ seeks, the abort's bytes taken as the BIOS takes them (`drain`: every REQ until BSY drops); PAUSE with no audio and its sense; a repeat of an empty range; AUDIO END by track number and on a stopped player; a sector arriving while the CPU holds ACK; a new selection while AUDIO START seeks. It reaches its for-ever loop later, so its cases run 600 frames (520 before) and the host resets come at frames 580 and 590 |
| `scripts/lockstep_cd.py` | the reference cache's key now includes the four reference checkouts' commits (before, a repin reused stale traces); `$SYSCARD` and `$LIBCD` come from the environment only (the machine paths that were their defaults are gone) |
| `scripts/seeded_faults_cd.py` | the 13 faults on the withdrawn text dropped, 26 written on the new text |
| `tests/scsi.rs`, `tests/reference_notes.rs`, `tests/state.rs` | the reference's new abort test and empty-range test; SEL to the command phase is 74,940 clocks; the state test moves `sector_clocks` (`seek_scale` is gone) |
| `NOTICE.md` | MAME's BSD-3 notice for the facts the reference takes from it (id bytes, MSM5205 step formula, the drive's command facts) |

The reference cache built from `v0.2.0` was moved aside to `external/refcache-v0.2.0` (ignored, not
deleted) and rebuilt from `4abb3f5`.

## Before and after

| Rung | Before (0.2.0, `v0.2.0`, 2026-11-13, the withdrawn module) | After (`4abb3f5`, 2026-11-19) |
| --- | --- | --- |
| 1 | 108 tests pass | 109 pass (the reference's new empty-range test; its abort test replaced) |
| 2 | probe, 520 frames, identical | probe, 600 frames, identical |
| 3 | 46 of 46 identical; 44 of 44 switches change the trace | 49 of 49 identical; 47 of 47 change it, the button reset alone does not |
| 4 | System Card, 700 frames, identical (6 commands) | identical (6 commands) |
| 5 | six discs, 18,000 frames, identical | six discs, 18,000 frames, identical; Mad Stalker 411 drive commands (426 before), the others the same |
| 6 | 5 of 5 round trips | 5 of 5 |
| 7 | 55 caught, 0 missed | 68 caught, 0 missed (13 on the withdrawn text replaced by 26) |
| checks | fmt, clippy `-D warnings`, wasm32 | fmt; clippy `-D warnings` on rustup 1.93.1 (`scripts/check.sh`, with the wasm32 build) and on `rustup run stable` (1.99.1), each in its own target directory; `cargo test` 121 pass |

The rustup default toolchain on the build machine is a nightly 1.78, older than the crate's
`rust-version` 1.85 (and the base board's), so it cannot build the crate at all; that was so before
this pass.

## What stays open

Everything the reference lists as open in its `docs/restart.md` holds here unchanged (the DMA rate,
a drive just reset, AUDIO END mode 0, READ of 0 sectors, SEL in the command/status/message phase,
the empty-range repeat, the BIOS's share of Shadoff's interval, the abort's further status bytes,
every closed-box number being one emulator's model). In this crate besides:

- ~~The base board is still pcengine-rust `064c009` (0.2.0), whose VDC carries its own **rewrite**
  item.~~ Carried the same day: see "The clean base board", below.
- The empty-range guard in `ScsiBus::audio_end` has no seeded fault: without it the CD-DA player's
  repeat never ends, so the faulted build would hang rather than fail.
- The reference (now `5c06d1b`), pcengine-python `03ac699`, huc6280-python `6ce348e`,
  pcengine-rust `37f17ba` and huc6280-rust `e1d24a3` were local commits then; all are on GitHub
  since 2026-11-20, `scripts/fetch_reference.sh` fetches from there by default
  (`$PCENGINECD_PYTHON` and the like point it at local clones), and `Cargo.toml` pins
  pcengine-rust `8f672f0` (that code with its own git pin of the core).

## The clean base board (2026-11-19, later)

The base board's own clean rewrite, carried: pcengine-rust `37f17ba` (pcengine-rust's
`docs/restart.md`: the VDC's line machine, the DMAs and the scroll state transcribed anew from
pcengine-python `3318027`, then the crop of a display that starts before the row from `03ac699`)
and through it huc6280-rust `e1d24a3`, both by sibling path (`Cargo.toml` names the git pins for
after the push; CI clones them). The gate's reference became pcenginecd-python `5c06d1b`, the same
add-on code as `4abb3f5` repinned to pcengine-python `03ac699` and huc6280-python `6ce348e`. This
crate's own code is unchanged: none of the base board's removed API (`Vdc::new`'s
`vertical_at_line_start`, `BoardOptions::vertical_at_line_start`, the core's
`VanishedInterrupt::Latched` and `IrqPollCycle::Mednafen`, `lockstep.py`'s `cache_name` signature)
was used here. No GPL source and no withdrawn code was read for this step; nothing was transcribed.

What changed for a host, all through the base board: save states are its format version 3 (0.2.0
states are refused; a new test in `tests/state.rs` says so), the picture starts 8 dots earlier,
`vblank_dots` defaults to -16 and the timer window to 3.

| Check | Before (`4abb3f5` on pcengine-rust `064c009`) | After (`5c06d1b` on pcengine-rust `37f17ba`) |
| --- | --- | --- |
| Rung 1 | 109 pass | 109 pass |
| Rung 2 | identical, 600 frames | identical, 600 frames |
| Rung 3 | 49 of 49; 47 of 47 change the trace | 49 of 49; 47 of 47 change it, the button reset alone does not |
| Rung 4 | identical, 6 commands | identical, 6 commands |
| Rung 5 | 6 of 6, 18,000 frames | 6 of 6, 18,000 frames; drive commands unchanged (12, 411, 19, 18, 12, 10) |
| Rung 6 | 5 of 5; 9 unit tests | 5 of 5; 10 unit tests (the version-3 test) |
| Rung 7 | 68 caught, 0 missed | 68 caught, 0 missed, each by the same check (seeder with fresh times) |
| checks | `cargo test` 121; clippy 1.93.1 and stable; fmt; wasm32 | `cargo test` 122; clippy `-D warnings` 1.93.1 and stable 1.99.1; fmt; wasm32 |

The reference cache was rebuilt from scratch (the `4abb3f5` one moved aside to
`external/refcache-4abb3f5`); its key now hashes the base gate's `pce_trace.py` as well as the four
reference commits. The reference's traces differ from the old ones (the bus and CPU from early
frames: the new VDC), and every case was identical on its first run.
