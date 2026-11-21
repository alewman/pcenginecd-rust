<!-- SPDX-License-Identifier: MIT -->
# Seeded faults

`scripts/seeded_faults_cd.py` plants one single-line fault at a time in a copy
of `src/`, builds it, and runs the checks cheapest first until one fails:
rung 2 (the probe with every default), rung 4 (the System Card), rung 3 (the
probe on every machine, medium and switch), rung 1 (`cargo test`, the
transcribed unit tests), rung 6 (`tests/state.rs`, then the round trips on
the synthetic cases), and the real discs last. It uses the reference cache, so run it after a
full gate run.

| Fault | Module | What it changes | Caught by |
| --- | --- | --- | --- |
| `register-access-runs-only-when-due` | `unit.rs` | a register read leaves the drive's clock at the last event while another waits (0.1.0) | rung 1 (`a_register_access_brings_the_drive_clock_to_its_own_time`) |
| `register-write-runs-only-when-due` | `unit.rs` | a register write (a SEL) leaves the drive's clock at the last event (0.1.0) | rung 2 (`probe-base`) |
| `memory-access-runs-the-drive` | `unit.rs` | a CD RAM read runs the drive first | rung 1 (`memory_accesses_do_not_run_the_drive`) |
| `power-on-keeps-the-latch-guard` | `unit.rs` | a power-on reset keeps the $1805 guard window running | rung 1 (`power_on_reset_clears_the_units_timers_and_files_the_adpcm_idle_at_the_reset`) |
| `power-on-keeps-the-dma-timer` | `unit.rs` | a power-on reset keeps the CD-to-ADPCM DMA's timer | rung 1 (`power_on_reset_forgets_an_armed_dma`) |
| `adpcm-idle-filed-at-zero` | `unit.rs` | a power-on reset files the ADPCM's idle output at time 0, behind later events (0.1.0) | rung 3 (`probe-reset-power`) |
| `power-on-keeps-rst` | `unit.rs` | a power-on reset leaves the drive held in reset if $1804 bit 1 was set | rung 1 (`power_on_reset_releases_a_drive_held_in_reset`) |
| `bram-longer-than-the-ram-cut` | `unit.rs` | a backup RAM image longer than 2 KiB is accepted (and then panics or is cut) | rung 1 (`bram_longer_than_the_ram_is_refused`) |
| `msf-without-the-lead-in` | `disc.rs` | an LBA's absolute time leaves out the two-second lead-in (READ TOC, READ SUBQ) | rung 2 (`probe-base`) |
| `data-from-the-sync` | `disc.rs` | a data sector's payload is taken 4 bytes early (the header counted as data) | rung 2 (`probe-base`) |
| `pregap-belongs-to-no-track` | `disc.rs` | a track's INDEX 00 pregap is not part of it: its sectors read as zeros | rung 3 (`probe-pregap-disc`) |
| `second-file-at-lba-0` | `disc.rs` | every FILE of a cue sheet starts at LBA 0 (they do not lie end to end) | rung 3 (`probe-pregap-disc`) |
| `audio-track-read-as-data` | `disc.rs` | READ of an audio sector returns its bytes, not zeros | rung 2 (`probe-base`) |
| `short-seek-three-vsyncs` | `seek.rs` | a gap under 5 sectors costs 3 VSYNC (seektime.c's dead branch, C20) | rung 2 (`probe-base`) |
| `seek-ignores-the-rotation` | `seek.rs` | the half rotation is the starting zone's, not the target's | rung 1 (`seek_times_are_the_references_doubles`) |
| `fade-aimed-at-the-other-source` | `cdda.rs` | `$180F` bit 1 aims the fade at CD audio, not ADPCM | rung 2 (`probe-base`) |
| `loop-plays-on` | `cdda.rs` | AUDIO END mode 2 loops and mode 1 stops at the end of its range | rung 2 (`probe-base`) |
| `audio-end-exclusive` | `cdda.rs` | playback stops a sector early (the end sector is not played) | rung 2 (`probe-base`) |
| `faded-sample-rounded` | `cdda.rs` | a faded CD sample is rounded, not cut toward zero (Python's int()) | rung 2 (`probe-base`) |
| `audio-end-leaves-a-stopped-player` | `cdda.rs` | AUDIO END does not start a stopped player | rung 2 (`probe-base`) |
| `sel-answered-under-rst` | `scsi.rs` | a SEL is answered while $1804 holds the drive in reset | rung 2 (`probe-base`) |
| `bus-reset-hides-dropped-levels` | `scsi.rs` | the drive's reset does not report the IRQ levels it drops (issue 0001 item 4) | rung 1 (`a_bus_reset_tells_the_unit_that_req_dropped`) |
| `reset-keeps-cd-audio` | `scsi.rs` | CD audio plays on through the drive's reset | rung 3 (`probe-reset-power`) |
| `unknown-command-switch-not-read` | `scsi.rs` | UNKNOWN_COMMAND_CHECK_CONDITION changes nothing | rung 3 (`sw-scsi.UNKNOWN_COMMAND_CHECK_CONDITION`) |
| `read-lba-16-bits` | `scsi.rs` | READ(6) drops the LBA's top five bits | rung 2 (`probe-base`) |
| `no-reread-penalty` | `scsi.rs` | a missed sector comes round again one sector later, not 290 ms after its turn | rung 2 (`probe-base`) |
| `sel-in-data-in-not-an-abort` | `scsi.rs` | SEL during data in is ignored (and aborts the command phase instead) | rung 2 (`probe-base`) |
| `abort-one-status-byte` | `scsi.rs` | the abort offers one status byte, not eight | rung 2 (`probe-base`) |
| `irq-sources-swapped` | `scsi.rs` | REQ in data in raises the status/message IRQ bit | rung 2 (`probe-base`) |
| `head-not-moved-by-a-read` | `scsi.rs` | the next seek starts from where the head was before the READ | rung 2 (`probe-base`) |
| `first-sector-a-sector-after-the-seek` | `scsi.rs` | a READ's first sector comes one sector after Shadoff's whole time, not before it | rung 2 (`probe-base`) |
| `toc-track-type-audio` | `scsi.rs` | READ TOC calls every track audio | rung 2 (`probe-base`) |
| `toc-kind-3-as-msf` | `scsi.rs` | READ TOC kind 3 answers the track's MSF, not its LBA | rung 2 (`probe-base`) |
| `toc-lead-out-track-unknown` | `scsi.rs` | READ TOC refuses track $AA, the lead-out | rung 2 (`probe-base`) |
| `subq-relative-from-zero` | `scsi.rs` | READ SUBQ's track-relative time counts from the disc's start | rung 2 (`probe-base`) |
| `ack-pulse-a-cycle-long` | `scsi.rs` | the $1808 auto-ACK pulse lasts one CPU cycle | rung 2 (`probe-base`) |
| `sense-not-cleared` | `scsi.rs` | REQUEST SENSE leaves the sense in place | rung 2 (`probe-base`) |
| `one-data-latch` | `scsi.rs` | $1801 reads the target's byte whatever the direction | rung 2 (`probe-base`) |
| `command-byte-from-the-target-latch` | `scsi.rs` | a command byte is taken from the target's latch, not the CPU's | rung 2 (`probe-base`) |
| `pause-without-audio-good` | `scsi.rs` | PAUSE with no audio started answers GOOD | rung 2 (`probe-base`) |
| `audio-end-track-mode-not-before` | `scsi.rs` | AUDIO END by track number plays into the named track | rung 2 (`probe-base`) |
| `busy-select-keeps-the-audio-wait` | `scsi.rs` | a new selection of a busy drive keeps waiting for the audio's report | rung 2 (`probe-base`) |
| `answer-wait-shows-data-in` | `scsi.rs` | a command with no data waits with I/O up, as one with data does | rung 2 (`probe-base`) |
| `message-after-data-to-status` | `scsi.rs` | the message comes DATA_TO_STATUS after the status, not STATUS_TO_MESSAGE | rung 2 (`probe-base`) |
| `subq-a-one-byte-command` | `scsi.rs` | READ SUBQ is answered at its first byte | rung 2 (`probe-base`) |
| `sector-offered-under-ack` | `scsi.rs` | a sector that arrives while ACK is up raises REQ at once | rung 2 (`probe-base`) |
| `low-nibble-first` | `adpcm.rs` | a byte's two samples both come from its low nibble | rung 2 (`probe-base`) |
| `length-16-bits` | `adpcm.rs` | the length counter wraps at 16 bits (playing past zero without the stop bit) | rung 2 (`probe-base`) |
| `adpcm-rounds-half-away` | `adpcm.rs` | a faded ADPCM sample rounds a half away from zero (Rust's round), not to even (Python's) | rung 1 (`a_faded_sample_on_a_half_rounds_to_even_as_pythons_round_does`) |
| `read-pointer-not-stepped-back` | `adpcm.rs` | `$180D` bit 3 copies the latch to the read pointer as it is, whatever bit 2 says | rung 2 (`probe-base`) |
| `decoder-keeps-its-state` | `adpcm.rs` | a play request restarts the signal but not the step index (C-ad1) | rung 2 (`probe-base`) |
| `end-flag-survives-the-length-latch` | `adpcm.rs` | latching a new length leaves the end flag (and its IRQ2 source) up | rung 2 (`probe-base`) |
| `sample-clock-from-the-write` | `adpcm.rs` | the first sample of a playback comes at the play write, not a period after it | rung 2 (`probe-base`) |
| `signed-offset-unsigned` | `arcade.rs` | the signed-offset bit does not reach below the base for an access | rung 2 (`probe-base`) |
| `rotate-shifts` | `arcade.rs` | `$1AE5` shifts left where it should rotate | rung 2 (`probe-base`) |
| `base-increment-16-bits` | `arcade.rs` | an auto-incremented base wraps at 16 bits | rung 2 (`probe-base`) |
| `trigger-on-either-byte` | `arcade.rs` | a write of the offset's high byte adds it into the base in every trigger mode | rung 2 (`probe-base`) |
| `bram-stays-unlocked` | `unit.rs` | a read of `$1803` does not lock the backup RAM | rung 2 (`probe-base`) |
| `dma-retry-late` | `unit.rs` | the CD-to-ADPCM DMA asks the busy RAM again every 6 master clocks, not 3 | rung 3 (`sw-unit.DMA_BYTE_CLOCKS`) |
| `latch-guard-not-set` | `unit.rs` | a second `$1805` write right after the first toggles the channel again | rung 2 (`probe-base`) |
| `drive-enables-kept-in-reset` | `unit.rs` | `$1804` bit 1 does not clear the drive's IRQ enables | rung 1 (`reset_register_clears_drive_irq_enables_and_the_drive_works_after_release`) |
| `unit-forgets-the-fader` | `unit.rs` | a loaded state keeps the fader of the machine it is loaded into | rung 6 (`loading_keeps_the_switches_and_the_wiring`) |
| `identity-without-the-disc` | `unit.rs` | a state does not name its disc: another disc's machine takes it | rung 6 (`a_state_names_the_disc_by_its_contents_table_and_holds_no_sector`) |
| `super-ram-a-bank-short` | `syscard.rs` | the Super System Card's RAM starts at bank $69 | rung 2 (`probe-base`) |
| `id-registers-region-blind` | `syscard.rs` | `$18C5` answers the other region's value | rung 2 (`probe-base`) |
| `card-forgets-its-ram` | `syscard.rs` | a loaded state keeps the Super System Card RAM of the machine it is loaded into | rung 6 (`the_unit_the_adpcm_ram_and_the_arcade_card_come_back`) |
| `switches-not-handed-to-the-bus` | `board.rs` | the builder leaves the drive's timings at their defaults whatever it is given | rung 3 (`sw-scsi.SEL_TO_COMMAND_PHASE`) |
| `arcade-card-always-fitted` | `board.rs` | every machine has an Arcade Card | rung 3 (`probe-plain`) |

**68 caught, 0 missed** (2026-11-19, against the cache rebuilt from pcenginecd-python `4abb3f5`, the
clean rewrite of the drive's bus; [restart.md](restart.md)): 47 by rung 2, 10 by rung 1, 8 by rung 3,
3 by rung 6. The thirteen faults of 0.2.0 on `src/scsi.rs` quoted the withdrawn module's text; they
were removed by a script that printed only their names (`bus-reset-lets-go-of-rst`,
`bus-reset-hides-dropped-levels`, `unknown-command-switch-not-read`, `read-lba-16-bits`,
`no-reread-penalty`, `sel-in-data-in-not-an-abort`, `irq-sources-swapped`, `seek-from-the-last-read`,
`queued-timer-rearmed`, `toc-track-type-audio`, `subq-relative-from-zero`, `ack-pulse-a-cycle-long`,
`sense-not-cleared`), and 26 were written on the new text, several aimed at the same behaviours. The
first runs missed five: four were holes in the probe card, which grew to close them (a sector that
arrives while the CPU holds ACK, `sector-offered-under-ack`, needed a wait past the re-read; a new
selection while AUDIO START seeks, `busy-select-keeps-the-audio-wait`, needed a wait past the
longest seek; `audio-end-leaves-a-stopped-player` lost its catch when the probe grew and gained an
AUDIO END on a stopped player; `audio-end-track-mode-not-before` needed a play that runs into the
named track's end), and `dma-retry-late` needed the `DMA_BYTE_CLOCKS` case moved from 30 to 14
master clocks (with the 45-clock auto-ACK a 30-clock DMA no longer finds the RAM busy, and at 15 the
3- and 6-clock retries land on the same clock). Not seeded: the empty-range guard of AUDIO END's
repeat (without it the CD-DA player's repeat never ends, so the faulted build hangs rather than
fails) and the order of `run_to`'s checks (a faulted order spins at one instant).

**Again 68 caught, 0 missed** (2026-11-19, later, on the clean base board pcengine-rust `37f17ba`,
against the cache rebuilt from pcenginecd-python `5c06d1b`), each by the same check as before, run
with `--no-discs` (no fault reached the disc fallback before either). The seeder now copies the tree
with fresh modification times (`copytree`'s `copy2` kept the old ones, so cargo could take a
restored file for one older than the last fault's build and keep that fault; found by
supergrafx-rust) and copies the README into it; that no catch changed says no earlier catch was a
stale build.

What the 0.2.0 record said:
**55 caught, 0 missed** (2026-11-13, 0.2.0, against the rebuilt cache of pcenginecd-python
`v0.2.0`): 34 by rung 2, 10 by rung 1, 8 by rung 3, 3 by rung 6. Eleven faults are new, on what
issue 0001 changed (the first eleven rows). The first run missed two of them, `power-on-keeps-the-dma-timer`
and `power-on-keeps-rst`, and `irq-raised-in-reset` of 0.1.0: the reference's directed test sets the
unit's DMA timer itself and the probe never resets the machine with `$1804` held, so
`tests/issue0001.rs` gained `power_on_reset_forgets_an_armed_dma` (a DMA armed for real; the
read-only `CdUnit::pending_timers` added for it) and `power_on_reset_releases_a_drive_held_in_reset`.
`irq-raised-in-reset` is dropped: since the bus keeps RST through its reset, it raises no level while
the bit is held, and the guard it disabled cannot be reached (an equivalent mutant now). Rung 1
now runs `tests/issue0001.rs` and `tests/reference_notes.rs` too (`scripts/rung1.sh`, the faults'
list).

What the 0.1.0 record said:

**45 caught, 0 missed**: 33 by rung 2 (the probe card with every default), 6
by rung 3 (the pregap disc twice, the data-only disc, the machine without an
Arcade Card, and two switch cases), 3 by rung 1's unit tests, 3 by
`tests/state.rs`. The System Card's boot, the round trips on the synthetic
cases and the real discs come later in the order and were never needed.

The first run of this list missed seven. One changed no behaviour, which
makes it not a fault, and was dropped ("an Arcade Card access runs the drive
up to the access": the board has already run the unit up to that master
clock; see [notes on the reference](notes-on-the-reference.md), 2). Five were holes in the probe card,
not in the crate: it never sent a READ whose LBA needs its top five bits
(`read-lba-16-bits`), never let the ADPCM length pass 16 bits
(`length-16-bits`), never stepped an Arcade Card base past 16 bits
(`base-increment-16-bits`; the reference's own test of the 24-bit wrap passes
with a 16-bit one too), never raised a drive source while `$1804` held the
reset bit (`irq-raised-in-reset`), and no case made the CD-to-ADPCM DMA find
the RAM busy, so its retry never ran (`dma-retry-late`). The probe does the
first four now, and the `DMA_BYTE_CLOCKS` switch case runs the fifth (at 30
master clocks the DMA arrives while the last byte is still being written;
at the default 60, and on every disc run, it never does). The seventh,
`adpcm-rounds-half-away`, cannot be reached from a program in any reasonable
time (a faded sample falls on an exact half only when the fader's gain is an
exact binary fraction at a sample instant) and is caught by a unit test
written for it. `signed-offset-unsigned` was caught by a unit test only, because the
probe wrote through a signed offset and never read back; it reads back now.
