# Provenance

## Licence provenance (audit 2026-11-13)

pcenginecd-rust is a transcription of pcenginecd-python (ours, MIT) and **inherits its verdicts**
(pcenginecd-python `docs/provenance.md`, same date; audited from the records, no GPL source opened):

| Rust module | Verdict |
| --- | --- |
| `src/scsi.rs`: the bus state machine, timing constants, auto-ACK pulse and abort path (named after and converted from Mesen2's `PceScsiBus.cpp`) | **rewrite** |
| `src/scsi.rs`: command set, lengths, sense codes | **clean** |
| `src/seek.rs` (Shadoff's `seektime.c`, MIT; notice in NOTICE.md) | **cite** (done) |
| `src/syscard.rs` (MAME `pce_scdsys.cpp` id bytes, BSD-3) | **cite**: add MAME's BSD-3 notice to NOTICE.md |
| `src/adpcm.rs` (MSM5205 table as modelled by MAME `msm5205.cpp`, BSD-3) | **cite** |
| `src/arcade.rs` (every behaviour also in MAME `pce_acard.cpp`, BSD-3, and ares, ISC) | **cite**: re-cite to those |
| `src/unit.rs`, `src/cdda.rs`, `src/disc.rs`, `src/board.rs` | **clean** |

Added by the Rust tier: `src/trace.rs`, `src/bin/*`, `examples/probe_report.rs`, `scripts/*`
(lockstep, faults, `make_cdprobe.py` and `make_discs.py` building our own probe program and test
discs): ours, **clean** (provenance_check's one hit, an LBA byte split shared with `oss-pce-cd-bios`'s
MIT tools, is a generic idiom). A stray tracked file named `--help` (8 KiB, added in `f60c1eb`) is
one of our own generated probe programs, not third-party: housekeeping only. Crates are the base
board's set, all permissive.

Counts: clean 3, cite 4, rewrite 1, unclear 0. The base board (pcengine-rust) carries its own
**rewrite** items (the VDC line machine). The System Card is never in this repository.
History: 14 commits, no deleted files.

## Clean rewrite carried, 2026-11-19

The **rewrite** item is done: `src/scsi.rs` was moved aside unread (`scratch/withdrawn/`,
git-ignored) and transcribed anew from the reference's clean rewrite, pcenginecd-python `4abb3f5`
([restart.md](restart.md)). The audit's items, one by one:

| Rust module | Was | Now |
| --- | --- | --- |
| `src/scsi.rs`: the bus state machine, timing constants, auto-ACK pulse and abort path | **rewrite** | **clean**: a transcription of the reference's new `scsi.py` (SCSI-1, pcetech 8, the Hudson BIOS manual and System Card disassembly; MAME `nec/pce_cd.cpp`, BSD-3, and ares, ISC, for facts, nothing ported; timings closed-box measurements of Geargrafx by the reference). The withdrawn module was not read |
| `src/scsi.rs`: command set, lengths, sense codes | **clean** | **clean** (rewritten with the rest; the MAME facts credited in NOTICE.md) |
| `src/seek.rs` | **cite** (done) | unchanged |
| `src/syscard.rs` (MAME `pce_scdsys.cpp` id bytes, BSD-3) | **cite**: add MAME's notice | **done**: NOTICE.md carries MAME's BSD-3 notice with the file's copyright line |
| `src/adpcm.rs` (MSM5205 table as modelled by MAME `msm5205.cpp`, BSD-3) | **cite** | **done**: the comment names MAME `sound/msm5205.cpp` (BSD-3-Clause), NOTICE.md carries the notice; the GPL file names are gone from the comments (the reference's new words) |
| `src/arcade.rs` | **cite**: re-cite to MAME and ares | **done**: re-cited to MAME `bus/pce/pce_acard.cpp` (BSD-3) and ares (ISC) as the reference now does; the GPL emulators appear only as closed boxes that agree on the discs |
| `src/unit.rs`, `src/cdda.rs`, `src/disc.rs`, `src/board.rs` | **clean** | **clean**; the comments that named GPL files (`unit.rs`'s BRAM header, latch guard, DMA time and `$180B` rule; `cdda.rs`'s filtering, fade times and SUBQ byte; `disc.rs`'s READ TOC addresses) carry the reference's re-sourced words |
| `scripts/*`, `src/trace.rs`, `src/bin/*`, `examples/*` | **clean** | **clean**; the probe card (`make_cdprobe.py`) and the cases follow the new drive; `lockstep_cd.py` no longer names machine paths |

`src/syscard.rs` still reports what other emulators answer (behaviour, as the reference's
`syscard.py` does), not their code. Counts now: rewrite 0, unclear 0; every cite action done. The base board pin (pcengine-rust `064c009`) still carries the base board's
own **rewrite** item, the VDC; it is carried separately.
