<!-- SPDX-License-Identifier: MIT -->
# pcenginecd-rust

A Rust PC Engine **CD-ROM² / Super CD-ROM² / Arcade Card**, transcribed from
[pcenginecd-python](https://github.com/alewman/pcenginecd-python) at `4abb3f5` (`v0.2.0` and the clean
rewrite of its drive's bus) and proven equivalent to it: every CPU bus cycle (physical address, value, kind, master clock), every frame's
picture, every mixed audio sample (PSG, CD audio and ADPCM), the CPU's state and the drive's command
log, frame by frame, on a card of our own that drives the unit through every register, on the real
System Card and on six real discs. It is an add-on crate on the family's Rust PC Engine board,
[pcengine-rust](https://github.com/alewman/pcengine-rust), whose CPU is
[huc6280-rust](https://github.com/alewman/huc6280-rust); the SuperGrafx add-on beside it is
[supergrafx-rust](https://github.com/alewman/supergrafx-rust). pcenginecd-python is the
specification (readable, cited, compared with Geargrafx and Beetle PCE); this crate adds nothing and
tunes nothing. MIT.

The System Card (with the Super System Card's RAM and id registers) goes in the HuCard slot; the CD
unit (64 KiB CD RAM, 2 KiB backup RAM, the `$1800-$180F` interface, the SCSI-class controller, the
seek model, CD audio with the fader, the ADPCM unit, the Arcade Card) goes on the expansion port,
raises IRQ2 with its own bit and hands the mixer its two audio sources, all through the base board's
socket traits; nothing is copied from it. Module for module the reference's, with the same names,
every named switch as a field with the reference's default, and the comments that cite a source
carried over.

**The disc is the one part designed for the browser and not only transcribed**: the sector store is a
trait (`SectorSource`), with an in-memory implementation, a file-backed one for bin/cue, and room for
a web page to answer from slices of a file the user chose; the cue sheet is parsed from its text, with
no filesystem. CHD is not read here (the reference shells out to `chdman`): extract to bin/cue first,
as the gate does.

Written by Claude Fable 5.2 (Anthropic) for alewman, and prepared for publication by Claude Opus
5.5. No System Card, BIOS or disc is included: bring your own.

## Status

**2026-11-19, later: on the clean base board.** pcengine-rust `37f17ba` (its clean rewrite and the
early display start crop) and huc6280-rust `e1d24a3`, by git pin since 2026-11-20 (pcengine-rust
`8f672f0`: that code plus its own git pin of the core; by path from sibling clones before); the gate's reference is pcenginecd-python `5c06d1b` (the same add-on code as
`4abb3f5`, on pcengine-python `03ac699` and huc6280-python `6ce348e`). All seven rungs pass again,
the cache rebuilt: 56 of 56 cases identical (48,100 frames), 47 of 47 switches change the trace,
save states 5 of 5 (now the base board's **format version 3**: 0.2.0 states are refused), seeded
faults 68 caught, 0 missed; `cargo test` 122. **CI fails until pcengine-rust `37f17ba` and
huc6280-rust `e1d24a3` are on GitHub** (it clones them beside the checkout). The paragraph and
table below are the drive bus's clean rewrite, on the old base board; the figures are the same now
but for 122 tests and rung 6's 10 unit tests ([docs/restart.md](docs/restart.md)).

**2026-11-19: unreleased, following pcenginecd-python `4abb3f5`, its clean rewrite of the drive's bus
(the provenance audit's one rewrite item; [docs/restart.md](docs/restart.md)), on pcengine-python
`v0.2.0` and huc6280-python `v0.4.0`; all seven rungs pass against the new reference.** (0.2.0,
2026-11-13, followed `v0.2.0`.) Each rung is a
script that says PASS; the
lockstep gate compares this crate with the reference frame by frame, on PyPy 7.3.20 against rustc
1.93.1 on Linux x86_64.

| Rung | What | Result |
| --- | --- | --- |
| 1 | The reference's unit tests, transcribed, on the same synthetic discs | 109 tests pass (the reference's 91 and 7 of this crate's, and 11 on the reference's issue 0001: its 9 and two more) |
| 2 | The gate itself: the probe card with every default | identical, 600 frames |
| 3 | The probe card on every machine and medium, and with each named switch away from its default | 49 of 49 cases identical (29,400 frames); 47 of 47 that should change the reference's trace do, and the one that should not does not |
| 4 | The real System Card on a synthetic disc, 700 frames with a RUN press | identical |
| 5 | Six real discs, 3,000 frames each with RUN at 450: three with the Arcade Card | 6 of 6 identical, audio included (18,000 frames) |
| 6 | Save states: round trip and run-ahead on the System Card's boot, the probe card and two real discs; the refusals | 5 of 5 runs equal, twice each; 9 unit tests |
| 7 | Seeded faults | 68 caught, 0 missed (the 13 on the withdrawn `scsi.rs` replaced by 26 on the new one) |

In all 48,100 frames in 56 lockstep cases, and no case has ever differed: at `v0.1.0` (2026-11-12),
at `v0.2.0` (2026-11-13: the reference cache rebuilt from the new reference, the old one kept
as `external/refcache-v0.1.0`; every case identical on its first run) and at `4abb3f5` (2026-11-19:
`src/scsi.rs` moved aside unread and transcribed anew, the cache rebuilt, the old one kept as
`external/refcache-v0.2.0`; every case identical on its first run; the probe card grew to reach the
new drive's answers, its cases run 600 frames, and the 13 `sw-scsi.*` cases became 16 for the new
switches). Two cases were changed for
`v0.2.0`: `sw-scsi.UNKNOWN_COMMAND_CHECK_CONDITION` is new (C22), and the probe card no longer starts a
command while `$1804` holds the reset bit (the drive now stays in reset there, so the card waited for
ever and every later switch changed nothing); it writes SEL, waits, sees no answer, and releases. `cargo test` runs 121: the
109 of rung 1, the 9 of save states, one asserting the notes on the reference, and the two
documentation examples. At 0.1.0, rungs 1 to 5, the six real discs included, also passed with the base
board replaced by its speed fork, pcengine-rust-fast (not re-run for 0.2.0), and the crate runs in the family's browser page,
[play-pce.supertux.com](https://play-pce.supertux.com). Nothing here was run on a real console. Not
covered: the subchannel Q stream and its IRQ2 source (the reference has not built them); CHD
images; the discs were run through their openings and first screens with no input beyond RUN, not
played; the reference itself is compared with emulators and, for seek times, Dave Shadoff's
measurements, not with a console, and this crate inherits that.

## Using it

```toml
[dependencies]
pcenginecd-rust = { git = "https://github.com/alewman/pcenginecd-rust", rev = "<commit>" }
```

Not on crates.io; name a commit of this repository. (It takes pcengine-rust, and through it
huc6280-rust, by git revision; `Cargo.toml` shows the `--config` patch for a local checkout.) The crate re-exports the base board as
`pcenginecd_rust::pcengine_rust`, so a host needs only this dependency. The example is compiled with
the documentation tests:

```rust
use pcenginecd_rust::board::{self, PcEngineCd};
use pcenginecd_rust::disc::{parse_cue, CueSource, Disc};
use pcenginecd_rust::pcengine_rust::pads::{self, Pad2};
use pcenginecd_rust::pcengine_rust::state::{load_state, save_state};

/// `system_card`: the BIOS image. `cue`: the sheet's text. `length_of`: the byte length of a
/// file the sheet names. `read`: bytes of file `index` from `offset` into the buffer, returning
/// how many (a page slices the user's `File`). `kept_bram`: backup RAM from the last session.
fn run(
    system_card: &[u8],
    cue: &str,
    mut length_of: impl FnMut(&str) -> Result<u64, String>,
    read: impl FnMut(usize, u64, &mut [u8]) -> usize + 'static,
    kept_bram: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    // The cue sheet without a filesystem, and a disc over any reader.
    let sheet = parse_cue(cue, &mut length_of)?;
    let disc = Disc::from_cue(&sheet, Box::new(CueSource::new(&sheet, read)));

    // The machine: System Card bytes, a disc (or None), the options.
    let mut machine = PcEngineCd::new(system_card, Some(disc));
    machine.arcade_card = true; // fit the Arcade Card (Sapphire, Mad Stalker, Ryuuko no Ken)
    machine.bram = kept_bram; // None: formatted and empty
    machine.board.pad = Some(Box::new(Pad2::new()));
    let mut system = machine.build()?;

    // One frame, as on the base board: buttons, run, sound, picture.
    system.board_mut().set_pressed(0, pads::RUN);
    system.run_frame();
    let _samples: Vec<(i16, i16)> = system.board_mut().take_audio(); // PSG + CD audio + ADPCM
    let _picture = system.board().capture.last_frame.as_ref();

    // Save states are the base board's; the disc is named, never stored.
    let state = save_state(&system);
    load_state(&mut system, &state)?;

    // Backup RAM out, for the host to keep (2,048 bytes).
    let unit = board::unit(system.board_mut()).ok_or("no CD unit")?;
    Ok(unit.bram().to_vec())
}
```

- **Building a machine**: `PcEngineCd::new(system_card: &[u8], disc: Option<Disc>)`, then its public
  fields: `arcade_card`, `bram`, `super_system_card` (`None`: a 256 KiB image is a Super System
  Card), `region_us` (the id registers a US System Card answers), `switches` (`CdSwitches`), and
  `board`, the base board's `BoardOptions` (machine row, pad, its own switches). `build()` gives the
  base board's `System`, `build_board()` the `Board`. Everything of the base board's README then
  applies: `run_frame`, `take_audio`, `capture.last_frame`, `set_pressed`, `save_state` /
  `load_state`, `save_snapshot` / `load_snapshot`.
- **A disc**: `Disc::new(tracks, sector_count, Box<dyn SectorSource>)` is the whole of it.
  `SectorSource` has one method, `read_sector(&mut self, lba, &mut [u8; 2352])`, called only for
  `0 <= lba < sector_count` (the disc answers zeros outside by itself). Ready-made: `MemorySource` (a
  whole image in memory), and `CueSource<R: ImageReader>`, which maps an LBA to a byte range of one
  of a cue sheet's files; `ImageReader` is `read_at(file, offset, &mut [u8]) -> usize`, implemented
  by any closure of that shape (a page over `Blob.slice`), by `MemoryImages` (whole files in memory)
  and, off wasm, by `FileImages`. Reads are synchronous: a page whose file access is asynchronous
  keeps the sectors it has fetched and feeds the source from them.
- **A cue sheet without a filesystem**: `parse_cue(text, &mut |name| -> Result<u64, String>)` gives
  a `CueSheet { files, tracks, sector_count }`; the closure answers a named file's length in bytes.
  `Disc::from_cue(&sheet, source)` makes the disc; `Disc::from_cue_path(path)` (not on wasm) is the
  reference's `Disc.from_cue`. The grammar is the reference's: `FILE`, `TRACK` with `MODE1/2352` or
  `AUDIO`, `INDEX 00` and `01`, several FILEs end to end; `PREGAP` and other track modes are
  refused. `Disc::from_memory(vec![(is_data, bytes), ...])` builds a disc from raw tracks.
- **The Arcade Card**: `machine.arcade_card = true`. A state made with it is refused by a machine
  without, and the other way round.
- **Backup RAM**: `board::unit(system.board_mut())` is the `CdUnit`; `unit.bram()` is its 2,048
  bytes, `unit.set_bram(&bytes)` puts them back into a running machine, and `PcEngineCd::bram`
  starts one with them. (A save state carries backup RAM too.) `unit.insert(Some(disc))` /
  `insert(None)` changes the disc.
- **wasm**: the library builds for `wasm32-unknown-unknown`; nothing in it touches a file, a clock or
  a thread outside `Disc::from_cue_path`, `FileImages` and `trace::Setup` (the first two are not
  compiled there, the third refuses a disc path). The devices are not `Send` (`Rc`). The seek
  model's doubles decide timing and are plain `f64` arithmetic with no library function.

## How it is proven

**The gate** (`scripts/lockstep_cd.py`) runs each case of `scripts/cases_cd.tsv` on both sides: the
reference's `PcEngineCd` on PyPy (`scripts/pcecd_trace.py`) and this crate's (`pcecd-trace`). Each
writes one line a frame (the master clock, the count and SHA-1 of the CPU bus log, the SHA-1 of the
picture and of the mixed audio, every field of the CPU's state, and the drive's commands); a case
passes when the two files are identical, and a difference is narrowed to the first differing bus
cycle. The frame line and the comparison are pcengine-rust's scripts, imported by path from a
checkout of it. `scripts/make_cdprobe.py` builds the probe card, an 8 KiB card of this repository's
own code (not a ROM of anything) that drives the unit as a BIOS would and reaches every register,
every IRQ2 source and every audio path, on the synthetic discs of `scripts/make_discs.py`; and a
switch that changes no trace proves nothing, so `scripts/switch_effect_cd.py` fails if one does not.
[docs/ladder.md](docs/ladder.md) has the probe card, the cases, the six discs and the save states;
[docs/seeded-faults.md](docs/seeded-faults.md) the seeded faults and which rung caught each.

```text
cargo test --release              # needs nothing but the synthetic discs it builds
scripts/fetch_reference.sh        # pcenginecd-python, pcengine-python, huc6280-python, 6502-python at their pins
scripts/rung1.sh
scripts/rung2.sh
scripts/rung3.sh
scripts/rung4.sh
scripts/rung5.sh                  # extracts one disc at a time into scratch/ and removes it
scripts/rung6.sh
scripts/rung7.sh                  # after a full gate run (it uses the reference cache)
scripts/check.sh                  # cargo fmt --check, clippy -D warnings, the library for wasm32
scripts/bench_cd.py CASE          # frames per second, both sides
```

What the rungs need:

- **The reference** on PyPy 3.11 (`$PYPY`): `scripts/fetch_reference.sh` clones the four
  repositories at their pins into `external/`; nothing is installed, the gate runs PyPy with
  `PYTHONPATH` over their `src/` directories. pcenginecd-python is private at the time of writing,
  so rungs 2 to 7 need access to it.
- **A checkout of pcengine-rust** (`$PCENGINE_RUST`, default `../pcengine-rust`) at the revision
  `Cargo.toml` names (`37f17ba`; the crate builds from that same sibling clone, and it builds
  huc6280-rust from `../huc6280-rust` at `e1d24a3`): the gate imports its `scripts/`.
- **The user's files**: a System Card image (`$SYSCARD`; the runs above used Super CD-ROM² System
  v3.0 (Japan), 262,144 bytes, SHA-1 `79f5ff55…bd1f56a2c`) for rungs 4 to 7, and the discs as CHDs
  (`$LIBCD`, by the names in `scripts/cases_cd.tsv`) for rung 5, extracted with `chdman` (0.285 here)
  into the ignored `scratch/` directory, one at a time.
- **The reference cache**: each reference trace is kept in `external/refcache`, named by the case and
  a hash of its inputs, of the reference checkouts' commits and of the base gate's `pce_trace.py`; the reference runs at an eighth to a quarter of real time (the six discs'
  traces took 5 to 21 minutes each), and from the cache a rerun costs the Rust side only.

`$SYSCARD` and `$LIBCD` come from the environment only (a case that needs one fails without it);
`$PYPY` defaults to a PyPy 3.11 under `~/.local/opt`.
`JOBS` sets the parallel cases (at most 8). CI (`.github/workflows/ci.yml`) runs `cargo fmt
--check`, clippy with `-D warnings`, `cargo test` and the wasm build, after cloning pcengine-rust and
huc6280-rust beside the checkout at the commits `Cargo.toml` names (so it fails until those are
pushed); the gate needs the reference,
the System Card and the discs and runs by hand.

## Rust, speed, the reference, provenance

**Where Rust forced a difference**, each module's header says so, and
[docs/rust-differences.md](docs/rust-differences.md) lists them together. The ones a host meets: the
board owns the card and the unit, so `PcEngineCd` is a description and `build()` hands back the base
board's `System`, with `board::unit`, `card` and `arcade` reaching the devices; the reference's
callbacks between the drive, the ADPCM unit and the player became ordered lists of effects, applied
in the reference's order; the switches are fields grouped as `CdSwitches`; Python's arithmetic
(floor division, `int()`, `round()` to even, the seek model's `int`/`float` mix) is spelled out.

**Speed** is the fast fork's job; this is the figure to measure it against. From power-on, nothing
logged, the mixed audio taken every frame, one core (`pcecd-bench`, `scripts/bench_cd.py`):

| Run | pcenginecd-rust | pcenginecd-python on PyPy 7.3.20 (whole run; second half) |
| --- | --- | --- |
| Ryuuko no Ken, 3,000 frames, Arcade Card | 210-247 frames a second | 10.9; 16.3 |
| Mad Stalker, 3,000 frames, Arcade Card | 228-232 | 12.4; 15.2 |
| The System Card on the `mixed` disc, 700 frames | 164-174 | 7.4; 7.7 |

About 3.8 times real time (59.8 frames a second) on a disc, some 20 times the reference, on a machine
at load 16 (three Rust runs each, the reference's once). The time is the base board's: the unit costs
one early-out a CPU cycle, as in the reference.

**The reference**: nothing found was ever a divergence; the gate ran through each case. Five places
looked unintended while it was being transcribed
([docs/notes-on-the-reference.md](docs/notes-on-the-reference.md)), filed in pcenginecd-python as
[issue 0001](https://github.com/alewman/pcenginecd-python/blob/main/docs/issues/0001-findings-from-the-rust-port.md),
which is **settled** (pcenginecd-python `v0.2.0`), and this crate follows it (0.2.0): the drive's
clock runs to every register access (SEL to the command phase now takes 75,000 master clocks with or
without CD audio playing; `scripts/reference_notes.py` and `tests/reference_notes.rs` show it), a
power-on reset clears the unit's timers, the bus keeps RST and reports dropped IRQ levels, a backup
RAM image over 2 KiB is an error (`CdUnit::with` returns a `Result`), and the new switch
`ScsiSwitches::unknown_command_check_condition` (C22) gives the CHECK CONDITION answer to an unknown
command. (Since the clean rewrite of the drive's bus, below, SEL to the command phase takes 74,940.)

**Provenance**: no GPL or LGPL code was copied or transcribed. The only source transcribed is
pcenginecd-python, which is ours and MIT; no emulator's source was opened. The rule and the check
that looks for copied code are pcengine-python's
[docs/provenance.md](https://github.com/alewman/pcengine-python/blob/main/docs/provenance.md). The
seek model's zone table and shape derive from Dave Shadoff's MIT-licensed `seektime.c` and
measurements, through the reference ([NOTICE.md](NOTICE.md)). The audit of 2026-11-13 ([docs/provenance.md](docs/provenance.md))
found that the reference's drive bus, and so `src/scsi.rs`, followed a GPL emulator's code; the
reference rewrote it clean and this crate carried the rewrite (2026-11-19,
[docs/restart.md](docs/restart.md)): the old module was moved aside unread. The facts the reference
takes from MAME's BSD-3 files are credited in NOTICE.md.

## Layout

Each `src/*.rs` file is the transcription of the `pcenginecd_python` module of the same name: `disc`,
`seek`, `cdda`, `scsi`, `adpcm`, `arcade`, `unit`, `syscard`, `board`. Not from the reference's
package: `trace.rs` (the gate's command line and line, `roundtrip` and `runahead` over this machine),
the binaries `pcecd-trace`, `pcecd-state-roundtrip`, `pcecd-bench`, and `examples/probe_report.rs`
(what a run of the probe card touched). Tests: `tests/common/mod.rs` is the reference's
`tests/discs.py` and `helpers.py`; `disc.rs`, `scsi.rs`, `cdda.rs`, `adpcm.rs`, `arcade.rs`,
`unit.rs`, `board.rs` and `state.rs` are its `test_*.py`; `reference_notes.rs` asserts the notes on
the reference. Not transcribed: the reference's two slow tests as tests (they are rungs 4 and 6 here),
`scripts/cd_boot.py` and its picture tools, `beetle_press.py`.

[docs/handoff-brief.md](docs/handoff-brief.md) is the brief this was built from, with what was built
appended.

| What it was built and proven against | Version |
| --- | --- |
| pcenginecd-python (the reference) | `5c06d1b` (`scripts/fetch_reference.sh`; on GitHub since 2026-11-20; the add-on code of `4abb3f5`, repinned to the clean base board); 0.2.0 was proven against `v0.2.0`, `255875f`, 0.1.0 against `v0.1.0`, `5fec99e` |
| pcengine-python (what the reference runs on) | `03ac699` (0.2.0: `v0.2.0`, `4c32f26`) |
| huc6280-python, 6502-python (its core and frame) | `6ce348e` (0.2.0: `v0.4.0`, `265df88`), `2146eba` |
| pcengine-rust (the base board) | `8f672f0` by git revision (the code of `37f17ba`, by path until 2026-11-20, plus the base board's git pin of the core); 0.2.0 was `064c009` by its GitHub revision |
| huc6280-rust (the CPU, through the base board) | `e1d24a3`, by the base board's git pin; 0.2.0 was `43e5978` |
| The System Card | Super CD-ROM² System v3.0 (Japan), 262,144 bytes, SHA-1 `79f5ff55…bd1f56a2c` (the author's) |
| The discs | six CHDs of the author's library ([docs/ladder.md](docs/ladder.md)), extracted with `chdman` 0.285 |

## Licence

MIT (see [LICENSE](LICENSE)). The seek model's zone table and shape derive from Dave Shadoff's
MIT-licensed `seektime.c` and measurements, through the reference: [NOTICE.md](NOTICE.md). Nothing
else third-party is committed: the reference, the System Card and the discs are fetched or read in
place, and `.gitignore` refuses their extensions.
