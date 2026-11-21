# Notes on the reference

Nothing found was ever a divergence: this crate did what pcenginecd-python
`v0.1.0` did in each case, and the gate ran through all of them. They were
places where the reference's behaviour looked unintended while it was being
transcribed, filed in pcenginecd-python as
[issue 0001](https://github.com/alewman/pcenginecd-python/blob/main/docs/issues/0001-findings-from-the-rust-port.md),
which is **settled** (2026-11-13, pcenginecd-python `v0.2.0`, code in its
`f8649b5`). This crate follows the settlement (0.2.0), item for item as the
issue's "What a lockstep port must carry" lists it:

- **1 and 2, fixed**: a register access runs the drive to its own time
  (`CdUnit::run_now`, the reference's `_run_now`); CD RAM and BRAM accesses
  run nothing (memory has no clock), and `write` arms the DMA and recomputes
  the next event after a register write only. `scripts/reference_notes.py`
  and `tests/reference_notes.rs` now give the same time both ways (75,000
  master clocks at v0.2.0; 74,940 since the reference's clean rewrite of the
  drive's bus, `4abb3f5`).
- **3, fixed**: a power-on reset clears the unit's `latch_guard_until` and
  `dma_at`, recomputes `next_event`, lets go of RST, and passes its time to
  `Adpcm::power_on(at)`.
- **4, fixed**: `ScsiBus::reset` keeps `rst` (the CPU's line: the drive stays
  reset while `$1804` bit 1 is held) and reports each IRQ level it drops;
  `irq_levels` and `rst` are set in `ScsiBus::new`.
- **5**: a `bram` image over 2 KiB is an error (`CdUnit::with` returns a
  `Result`); a cue `TRACK` before any `FILE` or `INDEX` before any `TRACK` is an
  error (this crate already refused both); `to_bcd` left (unreachable); the
  new switch `ScsiSwitches::unknown_command_check_condition` (C22, default
  `false`: Good) gives the CHECK CONDITION with ILLEGAL REQUEST /
  INVALID COMMAND when set (case `sw-scsi.UNKNOWN_COMMAND_CHECK_CONDITION`).

Directed tests: `tests/issue0001.rs` (the reference's `tests/test_issue0001.py`)
and the reference's stronger Arcade Card wrap check in `tests/arcade.rs`.

What the notes said about v0.1.0, kept for the record:

1. **The drive's clock stands still between events.** `CdUnit.catch_up` runs
   the bus, and so moves `ScsiBus.now`, only when an event is due or none is
   pending (`if self._next_event is None or mclk >= self._next_event`). While
   some event waits in the future (the end of a CD audio track, the next ADPCM
   sample), a register access does not move `now`, and every timer a write
   starts (SEL to the command phase, the REQ of the next command byte, an
   answer's delay, the auto-ACK pulse, a seek) counts from the last time the
   bus happened to run. `scripts/reference_notes.py` shows it on the
   reference: SEL to the command phase takes `SEL_TO_COMMAND_PHASE` = 75,000
   master clocks on an idle drive and **0** when CD audio has been playing for
   a second (the timer was queued a second in the past and fires at the next
   cycle). With ADPCM playing the clock is at most one sample period old.
   `tests/reference_notes.rs` asserts the same two numbers here. So the
   drive's tier-5 timings apply in full only while nothing else is pending;
   the fix would be to run the bus up to `mclk` in every register access.
2. **The unit's own `catch_up` calls change nothing on a board.** `read` and
   `write` begin with `self.catch_up(mclk)` (and the Arcade Card's path skips
   it, "no clock"), but the board has run the port device up to the same
   master clock an instant before every access, so the call finds nothing
   due. A first version of the seeded-fault list had "an Arcade Card access
   runs the drive" as a fault; no check could catch it, because it is not
   one. (Note 1 is the other face of this: neither call moves the bus's
   clock when nothing is due.)
3. **A power-on reset keeps some of the unit's timers**: `CdUnit.reset` does
   not clear `_dma_at`, `_next_event` or `_latch_guard_until`, and
   `Adpcm.power_on` reports the output going idle at time 0, not at the
   reset, so the event is filed behind later ones (it takes effect at the
   next mixed sample all the same). `probe-reset-power` and
   `probe-reset-power-early` run through it, identically.
4. **`ScsiBus.reset` drops its IRQ levels without telling the unit**
   (`_irq_levels = (False, False)` and then `_update_irq()`, which sees no
   change): the unit's active bits `$20`/`$40` survive a bus reset unless the
   caller clears them, which both callers do (`$1804` masks them, a power-on
   reset zeroes them). Not reachable as a fault, noted because it reads as if
   the callback did it.
5. Small things: `CdUnit(bram=...)` with more than 2 KiB grows the RAM (slice
   assignment); `Disc.from_cue` attaches a `TRACK` that comes before any
   `FILE` to the last file (index -1); `to_bcd` of 160 or more does not fit a
   byte; `ASC_INVALID_COMMAND` is never used (an unknown command answers
   Good); the unit's docstring says the board calls `catch_up` "every cycle",
   and it is called at every advance, several a cycle on a stretched access.
