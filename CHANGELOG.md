<!-- SPDX-License-Identifier: MIT -->
# Changelog

Not on crates.io; pin a commit (README, "Using it").

## Unreleased

### Pinned by git revision (2026-11-20)

- **Base board: pcengine-rust `8f672f0` by git revision**, not a path to a sibling clone
  (`37f17ba`'s code; the base board now takes huc6280-rust `e1d24a3` by git pin too, which a git
  dependency needs). `Cargo.toml` names the `--config` patch for a local checkout. CI no longer
  clones the base board and the core. `scripts/fetch_reference.sh` fetches the reference from
  GitHub by default now (the variables still point it at local clones). The gate's scripts still
  import the base board's tooling from a checkout (`../pcengine-rust`).

### The clean base board (pcengine-rust `37f17ba`, reference pcenginecd-python `5c06d1b`)

- **Base board: pcengine-rust `37f17ba`** (its clean rewrite of 2026-11-19, the VDC's new line
  machine, and the crop of a display that starts before the row; transcribed from pcengine-python
  `03ac699`) and through it **huc6280-rust `e1d24a3`**, by path from sibling clones until both
  were pushed (a git pin since 2026-11-20, above).
- What a host meets through the base board: **save states are format version 3** (versions 1 and
  2, including every state made with 0.2.0, are refused, not converted; new test
  `a_state_is_the_base_boards_version_3_and_an_older_one_is_refused`); the picture starts 8 dots
  earlier (disc boot pictures moved 8 dots left with the base display); `BoardOptions` lost
  `vertical_at_line_start`, `vblank_dots` defaults to `Some(-16)` and the timer window to 3. No
  code of this crate used a removed API, so nothing in `src/` changed.
- The gate's reference: **pcenginecd-python `5c06d1b`** (the clean drive bus `4abb3f5` repinned
  to the clean base board; no add-on behaviour change) on pcengine-python `03ac699` and
  huc6280-python `6ce348e` (`scripts/fetch_reference.sh`, from the local clones; nothing is
  installed, so no stale install of the same version can shadow them). The reference cache's key
  also hashes the base gate's `pce_trace.py` (the reference's line); the `4abb3f5` cache was
  moved aside to `external/refcache-4abb3f5` and rebuilt.
- `scripts/seeded_faults_cd.py` copies the tree with fresh modification times (`copytree`'s
  `copy2` kept the old ones, so cargo could keep the previous fault's build; found by
  supergrafx-rust) and copies the README into it.

The gate, the cache rebuilt from scratch: rung 1 109 tests; rung 2 identical; rung 3 49 of 49
identical, 47 of 47 switches change the trace and the button reset alone does not; rung 4
identical; rung 5 the six discs identical (18,000 frames; drive commands unchanged: 12, 411, 19,
18, 12, 10); rung 6 5 of 5 runs and 10 unit tests; rung 7 **68 caught, 0 missed**, each by the
same check as before; `cargo test` 122; clippy `-D warnings` on rustup 1.93.1 and stable 1.99.1,
fmt and the wasm32 build pass. Every case identical on its first run.

### The clean drive bus (pcenginecd-python `4abb3f5`)

The reference's clean rewrite of the drive's bus, carried (docs/restart.md; the provenance audit's
one **rewrite** item). Follows pcenginecd-python `4abb3f5` (a local commit, not yet on GitHub),
still on pcengine-rust `064c009` and huc6280-rust `43e5978`.

- **`src/scsi.rs` transcribed anew** from the reference's new `scsi.py`; the old module was moved
  aside unread. `Phase::Busy` (BSY with no information phase); one pending protocol step, the
  READ's sector clock and the auto-ACK end, handled in that order (ACK end, step, sector, CD-DA).
- **`ScsiSwitches` follows the reference's new class attributes**: `sel_to_command_phase` 74,940,
  `command_byte_clocks` 3,030, `test_unit_ready` 449,565, `toc_clocks` 9,015, `subq_clocks` 3,030,
  `quick_answer` 3,030, `unknown_command_clocks` 450,555, `data_to_status` 3,030,
  `status_to_message` and `message_to_free` 1,500, `auto_ack_clocks` 45,
  `seek_measurement_extra` 332,000, `sector_clocks` 286,363, `reread_penalty_ms` 290 (the
  missed sector comes 6,228,408 clocks after its turn), `abort_status_bytes` 8,
  `unknown_command_check_condition`. Gone: `next_command_byte_req`, `toc_answer`, `subq_answer`,
  `done_status`, `abort_to_command`, `ack_clear`, `seek_scale`, `toc_answer_bytes`.
- Behaviour, as the reference: a READ's first sector at `max(seek - 332,000, one sector)`; REQ
  drops and the byte is taken at ACK's rise, the next step scheduled at its fall; command blocks of
  6 (`00 03 08`), 10 (`D8-DA DD DE`) or 1 byte; SEL ignored under RST or while a selection is
  pending, a new selection from bus free or busy, the abort in data in (eight GOOD status bytes,
  then the message), ignored otherwise; two data latches (`$1801` reads the target's byte while
  I/O is up, the CPU's otherwise); READ TOC kind 3 and track `$AA`; a missing track is CHECK
  CONDITION; PAUSE with no audio CHECK CONDITION (sense 05/2C); AUDIO END by track ends before the
  named track; a repeat of an empty range stops the audio and stays busy; the drive's reset stops
  CD audio.
- API: `ScsiBus::io()`, `cd()`, `msg()`, `bsy()`, `out()`, `data_waiting()` are methods (the
  reference's properties); `ScsiBus::head`, `initiator_data`; `CdUnit` uses `data_waiting()`.
- Comments re-sourced as the reference's: no GPL emulator's file or function is named
  (`unit.rs`, `cdda.rs`, `adpcm.rs`, `arcade.rs`, `disc.rs`); NOTICE.md carries MAME's BSD-3
  notice for the facts taken from it; SPDX headers on the files touched.
- The gate: the probe card follows the new drive (READ TOC kinds, the abort taken as the BIOS
  takes it, PAUSE with no audio, an empty repeat, AUDIO END by track and on a stopped player, a
  sector arriving under a held ACK, a new selection while AUDIO START seeks), so its cases run 600
  frames and the host resets come at 580 and 590; 16 `sw-scsi.*` cases for the 13 old; the
  shorter `DMA_BYTE_CLOCKS` case at 14 clocks; the reference cache is keyed by the reference's commits too (the v0.2.0 cache moved aside
  to `external/refcache-v0.2.0`); `$SYSCARD` and `$LIBCD` come from the environment only; the seeded
  faults on the withdrawn text replaced by 26 on the new.

## 0.2.0 (2026-11-13)

Follows pcenginecd-python 0.2.0 (`255875f`): its issue 0001, and its base board pcengine-python
0.2.0 (`4c32f26`) with huc6280-python 0.4.0 (`265df88`).

- **Base board: pcengine-rust 0.2.0** (by its GitHub revision, `064c009`):
  huc6280-rust 0.2.0 and the base board's VDC wait (pcengine-python issue 0004).
- **The drive's clock runs to every register access** (issue 0001 items 1-2): `CdUnit::run_now`
  before every `$1800-$18FF` access; CD RAM and BRAM accesses run nothing; the DMA is armed and the
  next event recomputed after a register write only.
- **A power-on reset clears the unit's timers** (item 3): `latch_guard_until`, `dma_at`,
  `next_event`; RST lets go; `Adpcm::power_on(at)` files the idle output at the reset's time.
- **`ScsiBus::reset` keeps RST and reports dropped IRQ levels** (item 4 and the RST line).
- **New switch** `ScsiSwitches::unknown_command_check_condition` (C22, default `false`), with the
  lockstep case `sw-scsi.UNKNOWN_COMMAND_CHECK_CONDITION`.
- **`CdUnit::with` returns a `Result`**: a backup RAM image over 2 KiB is an error (item 5);
  `PcEngineCd::make_unit` too.
- Save states: the unit's own part is unchanged; the base board's format is version 2, so a state
  of pcenginecd-rust 0.1.0 is refused.
- `CdUnit::pending_timers` (read-only: the DMA's and the next event's times, for tests).
- `tests/issue0001.rs`; eleven new seeded faults (55 caught, 0 missed; `irq-raised-in-reset` dropped:
  unreachable since the bus keeps RST).

## 0.1.0 (2026-11-12)

The CD-ROM2 / Super CD-ROM2 / Arcade Card add-on transcribed from pcenginecd-python v0.1.0
(`5fec99e`); all seven rungs pass.
