# Handoff brief: pcenginecd-rust

> **History.** This is the brief the crate was built from on 2026-11-12, kept as written, with
> "What was built" appended by the session that built it. Its instructions (the rules on
> processes, commits, "do not push") were for that session and are history; paths under
> `/data/emu/` and `~/.local/opt/` are the author's machine (read them as "your checkout", "your ROM
> directory" and "your PyPy"). The README is the current account.

## The request

Aubrey, 2026-11-12: the PC Engine family in Rust, then its fast forks, then **play-pce, a browser
page that runs any game of the family**, CD-ROM², Super CD-ROM² and Arcade Card discs included.
The CPU core (`/data/emu/huc6280-rust`) and the base board (`/data/emu/pcengine-rust`, all seven
rungs, 275 lockstep cases identical) are built. This repository is the CD add-on;
`supergrafx-rust` is being built beside it.

## What it is

A Rust PC Engine CD-ROM² / Super CD-ROM² / Arcade Card **transcribed from**
[pcenginecd-python](https://github.com/alewman/pcenginecd-python) at **`v0.1.0`**
(`5fec99e`; local clone `/data/emu/pcenginecd-python`), as an add-on crate that plugs into
pcengine-rust's socket traits exactly as the Python add-on plugs into pcengine-python's: the
`SystemCard` (with the Super System Card's RAM and id registers), the `CdUnit` on the port (CD RAM,
backup RAM, the `$1800-$180F` interface, IRQ2 sources), the SCSI-class controller (`scsi.py`), the
seek model (`seek.py`: Dave Shadoff's MIT measurements, keep the NOTICE), CD audio with the fader
(`cdda.py`), the ADPCM unit (`adpcm.py`, an audio source), the `ArcadeCard`, and the `PcEngineCd`
builder. Every named switch with the Python default.

Crate `pcenginecd_rust`, package `pcenginecd-rust`, edition 2021, MIT, `publish = false`.
Depend on the base board and core by path (`pcengine_rust = { path = "../pcengine-rust" }`).

**The disc is the one part to design for the browser, not only transcribe.** `disc.py` reads
bin/cue files (and expands a CHD with `chdman`). In Rust: the TOC and the LBA/MSF logic are
transcribed; the sector store is a **trait** (`SectorSource`: read raw 2352-byte sector `lba`
into a buffer) with an in-memory implementation (the tests' synthetic discs), a file-backed one
for bin/cue (native), and room for a page to supply one that reads slices of a user's file. Cue
parsing takes text plus a way to learn each file's length, so it works without a filesystem. CHD
is **not** read in this crate (no `chdman` in a browser; a CHD reader would be a later, separately
licensed dependency): the gate extracts CHDs to bin/cue with `chdman` as the Python scripts do.
`Disc::state_identity` (the table of contents) as in Python.

Read first: pcenginecd-python `README.md`, `docs/start-here.md`, `docs/m7-record.md`,
`docs/m8-record.md`, `docs/m9-record.md` (what each switch is and why), then
`src/pcenginecd_python/` module by module and `tests/` (`discs.py`, `helpers.py`); then
pcengine-rust's `README.md` (the socket table, "Rust-forced differences"), `src/sockets.rs` and
`src/fake_devices.rs` (`LockstepPort` is the skeleton of the CD unit: a port device with an IRQ2
source and an audio source; `LockstepCard` a card with RAM), and its `scripts/` (the lockstep gate,
`pce_trace.py`, `state_roundtrip.py`, `seeded_faults.py`): reuse that shape, importing the base
repository's scripts by path where it saves copying.

## The ladder (each rung a `scripts/rungN.sh` that says PASS)

1. **Unit tests**: pcenginecd-python's `tests/test_disc.py`, `test_scsi.py`, `test_cdda.py`,
   `test_adpcm.py`, `test_arcade.py`, `test_unit.py`, `test_board.py` transcribed, on the same
   synthetic discs (`tests/discs.py` transcribed: nothing here is a real disc).
2. **The gate**: a trace script that runs the Python `PcEngineCd` (PyPy, checkouts at the pins
   under `external/`) and a Rust `pcecd-trace`, compared by the base gate's per-frame digests (CPU
   bus log, picture, **mixed audio: PSG, CD audio and ADPCM, sample for sample**, CPU state) plus
   the drive's command log, narrowing to the first differing bus cycle; a reference cache (the
   Python CD machine runs at about a twentieth of real time: cache everything).
3. **Synthetic programs**: a card of our own (as pcengine-rust's `scripts/make_probe.py` builds
   one) that drives the unit through its registers on synthetic discs: reads, audio play with
   each end mode and the fader, ADPCM play and DMA, the Arcade Card's ports and shift unit, backup
   RAM, every IRQ2 source; identical. Every named switch in a non-default position on a case it
   changes.
4. **The real System Card** (`/data/emu/share/batocera/bios/syscard3.pce`, bring-your-own, read in
   place, never committed) on a synthetic disc, 700 frames with a RUN press: identical.
5. **Real discs** (`/data/emu/roms-retrobat/pcenginecd/*.chd`: extract one at a time with
   `chdman extractcd` into `scratch/` through a small helper, remove after; they are 200-800 MiB;
   never commit): the three Arcade Card discs of `docs/m9-record.md` (Ryuuko no Ken, Mad Stalker,
   Ginga Fukei Densetsu Sapphire, 3,000 frames, RUN at 450) and the discs the m7 and m8 records
   name, each to the frame count its record used; identical, audio included. If the machine is too
   loaded for all of them, do the three Arcade Card discs and two others and say which were left.
6. **Save states**: the base board's `save_state`/`load_state` and the snapshot pair cover this
   machine through `DeviceState`; the disc is named by `state_identity`, never stored. The round
   trip on rung 4's boot and on two real discs mid-boot; the refusals (another disc, another
   System Card, a machine without the Arcade Card).
7. **Seeded faults**: at least 20 across the modules, each caught by a rung.

Measure speed (frames per second on two discs against the Python add-on on PyPy).

## Rules

- pcenginecd-python is the spec. **Do not modify** it, pcengine-python, pcengine-rust,
  huc6280-rust or 6502-rust. If a socket of pcengine-rust lacks something the CD unit needs, stop
  and report exactly what (the base board is shared with the SuperGrafx crate being built at the
  same time).
- **No GPL or LGPL code, copied or transcribed** (pcengine-python `docs/provenance.md`): the only
  sources you transcribe are ours. Do not open any `reference/` directory.
- Never commit ROMs, discs or the System Card. At most 8 processes (another build runs beside
  you), `nice`; PyPy at `~/.local/opt/pypy3.11-v7.3.20-linux64/bin/pypy3.11` with
  `PYPY_GC_NURSERY=1M`. No `sed -i`, `rm -rf` or `ln -sfn`: small Python helpers (see
  pcenginecd-python `scripts/scratch.py`). Temp files under `external/`, `target/`, `scratch/`
  inside the repository; watch the disk (one extracted disc at a time).
- clippy: `~/.rustup/toolchains/1.93.1-*/bin/cargo-clippy` works (`cargo clippy` on PATH is too
  old); `-D warnings` clean, `cargo fmt`. Build the library for `wasm32-unknown-unknown` too (the
  file-backed sector source behind a `cfg`).
- Commits locally at good points, trailer `Co-Authored-By: Claude Fable 5.2
  <noreply@anthropic.com>`. **Do not create a GitHub repository and do not push.** Keep the
  brief's appended "What was built" current as you go, so a later session can pick up from the
  repository alone; if you cannot finish every rung, stop at a clean commit and say what remains.
- Report at the end: each rung with numbers, the faults table, the Rust-forced differences, speed,
  anything in the Python add-on that looked wrong, the final commit, and the public API a wasm
  page needs (building a CD machine from System Card bytes and a `SectorSource`, the Arcade Card
  option, backup RAM in and out for persistence, cue parsing without a filesystem).

## What was built (2026-11-12; kept current by the session that built it)

**Status: all seven rungs pass** (see the README's table for the numbers and the scripts). The
README is the document to read; this section is the short state for a later session.

- **The crate**: `src/disc.rs`, `seek.rs`, `cdda.rs`, `scsi.rs`, `adpcm.rs`, `arcade.rs`,
  `unit.rs`, `syscard.rs`, `board.rs`, each the transcription of the pcenginecd-python v0.1.0
  module of the same name, plus `trace.rs` (the gate's side) and the binaries `pcecd-trace`,
  `pcecd-state-roundtrip`, `pcecd-bench`. It depends on `../pcengine-rust` (49e3d48) by path and
  on nothing of it beyond its public API: **no socket was missing**, and no file outside this
  repository was modified.
- **The disc for the browser**: `SectorSource` (trait), `MemorySource`, `CueSource<R:
  ImageReader>` with `MemoryImages`, `FileImages` (not on wasm) and any closure as the reader;
  `parse_cue(text, &mut |name| -> length)`; `Disc::from_cue`, `from_cue_path`, `from_memory`,
  `state_identity` (the reference's digest, the same string). No CHD.
- **The gate**: `scripts/pcecd_trace.py` (the reference on PyPy, `PYTHONPATH` over
  `external/*/src`, no venv; `scripts/fetch_reference.sh` clones the four pins) against
  `pcecd-trace`; `scripts/lockstep_cd.py` over `scripts/cases_cd.tsv` with the reference cache
  in `external/refcache` (keep it: the six disc traces took 5 to 21 minutes each); the base
  gate's scripts are imported by path from `../pcengine-rust/scripts`.
- **The cases**: the probe card (`scripts/make_cdprobe.py`) on synthetic discs
  (`scripts/make_discs.py`), 45 cases; the System Card on a synthetic disc; six real discs
  (Ryuuko no Ken, Mad Stalker, Sapphire with the Arcade Card; The Addams Family, Pyramid
  Plunder, Insanity), 3,000 frames each. 52 cases, 42,100 frames, all identical, audio included.
  `scripts/switch_effect_cd.py` checks each probe case changes the reference's trace.
- **Save states**: nothing of the base crate needed changing; `DeviceState` of `CdUnit` and
  `SystemCard`. Round trips on the System Card's boot, the probe and two discs; refusals in
  `tests/state.rs`.
- **Seeded faults**: 45 single-line faults across the nine modules, 45 caught (33 by the probe
  with every default); the first run's seven misses were five holes in the probe card (closed),
  one non-fault (dropped) and one reachable only by a unit test (written).
- **Speed**: about 230 frames a second on a disc (3.8 times real time) against the reference's
  11 to 12 on PyPy.
- **Rungs**: `scripts/rung1.sh` ... `rung7.sh`; `scripts/check.sh` (fmt, clippy with the rustup
  1.93.1 toolchain in its own target directory, the library for wasm32-unknown-unknown).
- **Found in the reference** (the README's "Notes on the reference", shown by
  `scripts/reference_notes.py`): the drive's clock stands still between events, so SEL to the
  command phase takes 0 master clocks, not 75,000, while CD audio plays. Transcribed as it is.

What a later session should know before touching anything: a change to the probe card or to a
synthetic disc changes the cache key of every probe case (the gate then traces the reference
again, about seven minutes on six cores); the discs' reference traces are keyed by the CHD's
name and size and survive. `external/` and `scratch/` are ignored and hold nothing that cannot
be rebuilt, except time.

What is left for later sessions: nothing of the brief. Next in the plan are the fast fork and
play-pce; for those, the README's "Using the crate" is the API (a machine from System Card bytes
and a `SectorSource`, the Arcade Card option, backup RAM in and out, the cue sheet from text).
Not built because the reference has not built it: the subchannel Q stream. The path dependency
is repinned to a GitHub revision once the three crates are pushed (nothing was pushed, no
remote was created).

## Since then (2026-11-13)

Rungs 1 to 5, the six real discs included, pass with the base board replaced by
pcengine-rust-fast, and the crate runs in play-pce. The README and `docs/` were prepared for
publication (the long tables moved into `docs/`), and the numbers re-checked: `cargo test` and
rungs 1 to 5 (from the reference cache), against pcengine-rust `9c4608f`. The NOTICE now names
this crate's `src/seek.rs`.
