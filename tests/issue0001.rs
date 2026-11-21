// SPDX-License-Identifier: MIT
//! The reference's issue 0001 (findings from the Rust port), settled in pcenginecd-python
//! 0.2.0: one directed test per settled item, as its `tests/test_issue0001.py`.

mod common;

use common::{disc_audio_then_data, disc_data_only, Host, MASTER_HZ, REG};
use pcengine_rust::sockets::{PortDevice, ResetKind};
use pcenginecd_rust::disc::parse_cue;
use pcenginecd_rust::scsi::ScsiSwitches;
use pcenginecd_rust::unit::{CdUnit, BRAM_BYTES};

// -- item 1: a timer counts from the register write that starts it ----------------------------

/// Master clocks from a SEL write to the command phase, a second after the drive was last used
/// (looked for every 12 clocks, the board's catch-up between looks).
fn sel_to_command_phase(play_audio: bool) -> i64 {
    let mut h = Host::new(CdUnit::new(Some(disc_audio_then_data(400, 20))));
    if play_audio {
        h.command(&[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]); // AUDIO START, track 1 (5.3 s)
        h.finish();
        assert!(h.unit.bus.next_event().is_some()); // the end of the track waits in the future
    }
    h.advance(MASTER_HZ);
    h.select();
    let began = h.t;
    loop {
        let t = h.t;
        if h.read(REG, t) & 0xF8 == 0xD0 {
            break;
        }
        h.t += 12;
        let t = h.t;
        h.unit.catch_up(t, &mut h.pins.pins());
    }
    h.t - began
}

#[test]
fn sel_to_the_command_phase_takes_the_same_time_with_cd_audio_playing() {
    let idle = sel_to_command_phase(false);
    let playing = sel_to_command_phase(true);
    let sel = ScsiSwitches::default().sel_to_command_phase;
    assert!(sel <= idle && idle < sel + 24, "{idle}");
    assert_eq!(playing, idle); // before the fix: 0 (the timer was filed a second in the past)
}

#[test]
fn a_register_access_brings_the_drive_clock_to_its_own_time() {
    let mut h = Host::new(CdUnit::new(Some(disc_audio_then_data(400, 20))));
    h.command(&[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
    h.finish();
    h.advance(MASTER_HZ / 2);
    h.r(0);
    assert!(h.unit.bus.now == h.t && h.unit.bus.cdda.now == h.t);
}

// -- item 2: CD RAM and BRAM accesses need no clock; the card's never had one ------------------

#[test]
fn memory_accesses_do_not_run_the_drive() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.select();
    let t = h.t;
    h.write(0x100000, 0x5A, t + MASTER_HZ); // CD RAM, a second later
    assert_eq!(h.read(0x100000, t + MASTER_HZ), 0x5A);
    assert_eq!(h.unit.bus.now, t); // only a register access (or the board's catch-up) moves it
}

// -- item 3: a power-on reset forgets the unit's own timers -----------------------------------

#[test]
fn power_on_reset_clears_the_units_timers_and_files_the_adpcm_idle_at_the_reset() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.w(5, 0); // the $1805 guard runs for CHANNEL_LATCH_GUARD clocks
    assert!(h.unit.right_channel);
    h.unit.adpcm.record = true;
    h.unit.adpcm.last_output = 123; // the decoder was playing
    h.advance(100);
    h.unit.reset(ResetKind::PowerOn, &mut h.pins.pins());
    assert_eq!(h.unit.adpcm.events.last(), Some(&(h.t, 0))); // before: (0, 0), behind later events
    assert!(!h.unit.right_channel);
    h.w(5, 0); // within the old guard window: the latch now takes it
    assert!(h.unit.right_channel);
}

/// The reference's test sets `_dma_at` itself; here the DMA is armed for real: a sector in data
/// in with `$180B` bit 1 set.
#[test]
fn power_on_reset_forgets_an_armed_dma() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.command(&[0x08, 0, 0, 0, 1, 0]); // READ(6): one sector
    h.wait(0xC8); // data in, REQ up
    h.w(0x0B, 0x02); // CD-to-ADPCM DMA requested: the next byte is timed
    assert!(h.unit.pending_timers().0.is_some());
    h.unit.reset(ResetKind::PowerOn, &mut h.pins.pins());
    let (dma, next) = h.unit.pending_timers();
    assert_eq!(dma, None);
    assert_eq!(
        next,
        h.unit
            .bus
            .next_event()
            .into_iter()
            .chain(h.unit.adpcm.next_event())
            .min()
    );
}

/// The reset register goes to 0 at power-on, so RST lets go: a drive held in reset when the
/// power-on reset comes answers a SEL after it.
#[test]
fn power_on_reset_releases_a_drive_held_in_reset() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.w(4, 0x02); // the drive held in reset
    h.unit.reset(ResetKind::PowerOn, &mut h.pins.pins());
    assert!(!h.unit.bus.rst);
    h.select();
    h.advance(ScsiSwitches::default().sel_to_command_phase + 100);
    assert_eq!(h.status(), 0xD0); // BSY, C/D, REQ: the command phase
}

// -- item 4: a bus reset reports the IRQ levels it drops --------------------------------------

#[test]
fn a_bus_reset_tells_the_unit_that_req_dropped() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.wait(0xD8); // status phase: the status/message level is up
    assert!(h.unit.bus.irq_changes.is_empty()); // the unit has taken every change so far
    h.unit.bus.reset();
    assert_eq!(h.unit.bus.irq_changes, [(0x20, false)]); // before: nothing
}

// -- item 5: the small ones -------------------------------------------------------------------

#[test]
fn bram_longer_than_the_ram_is_refused() {
    let full = vec![0u8; BRAM_BYTES];
    assert_eq!(
        CdUnit::with(None, Some(&full), None).unwrap().bram.len(),
        BRAM_BYTES
    );
    let over = vec![0u8; BRAM_BYTES + 1];
    assert!(CdUnit::with(None, Some(&over), None).is_err());
}

#[test]
fn a_cue_track_or_index_before_its_file_or_track_is_refused() {
    let mut size = |_: &str| Ok(2352 * 4);
    let error = parse_cue(
        "TRACK 01 MODE1/2352\nFILE \"d.bin\" BINARY\nINDEX 01 00:00:00\n",
        &mut size,
    )
    .err()
    .unwrap();
    assert!(error.contains("TRACK before any FILE"), "{error}");
    let error = parse_cue("FILE \"d.bin\" BINARY\nINDEX 01 00:00:00\n", &mut size)
        .err()
        .unwrap();
    assert!(error.contains("INDEX before any TRACK"), "{error}");
}

#[test]
fn an_unknown_command_answers_good_or_check_condition_with_the_switch() {
    for check in [false, true] {
        let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
        h.unit.bus.switches.unknown_command_check_condition = check;
        h.command(&[0x42]);
        assert_eq!(h.finish(), if check { (0x02, 0) } else { (0, 0) });
        let (sense, _) = h.run(&[0x03, 0, 0, 0, 18, 0], 18, 1);
        assert_eq!(
            (sense[2], sense[12]),
            if check { (0x05, 0x20) } else { (0, 0) }
        );
    }
}

#[test]
fn reset_register_held_keeps_the_drive_irqs_off() {
    let mut h = Host::new(CdUnit::new(Some(disc_data_only(40))));
    h.w(2, 0x70);
    h.w(4, 0x02); // the drive held in reset
    h.w(2, 0x70); // enables written again while held
    h.select();
    h.w(1, 0);
    let s = ScsiSwitches::default();
    h.advance(s.sel_to_command_phase + s.test_unit_ready + 10_000);
    assert!(h.unit.active & 0x60 == 0 && !h.irq() && h.r(0) == 0);
    h.w(4, 0x00);
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.wait(0xD8);
    assert!(h.unit.active & 0x20 != 0 && h.irq()); // released: the status IRQ comes again
}
