// SPDX-License-Identifier: MIT
//! The drive's bus and command set through the registers, as the BIOS drives them (the
//! reference's tests/test_scsi.py).

mod common;

use common::{disc_audio_then_data, disc_data_only, Host, MASTER_HZ};
use pcenginecd_rust::disc::{to_bcd, Disc};
use pcenginecd_rust::scsi::ScsiSwitches;
use pcenginecd_rust::unit::CdUnit;

fn host(disc: Option<Disc>, enable: u8) -> Host {
    let mut h = Host::new(CdUnit::new(disc));
    h.w(2, enable);
    h
}

fn timing() -> ScsiSwitches {
    ScsiSwitches::default()
}

#[test]
fn bus_is_free_until_selected_then_enters_the_command_phase_after_a_delay() {
    let mut h = host(Some(disc_data_only(40)), 0);
    assert_eq!(h.r(0), 0x00);
    h.select();
    assert_eq!(h.r(0) & 0xF8, 0); // not yet: the drive takes time to answer the selection
    let waited = h.wait(0xD0);
    assert!(waited >= timing().sel_to_command_phase - 100);
}

#[test]
fn test_unit_ready_takes_the_spin_up_time_then_status_good_message_zero() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[0, 0, 0, 0, 0, 0]);
    let began = h.t;
    assert_eq!(h.finish(), (0, 0));
    assert!(h.t - began >= timing().test_unit_ready - 20_000);
}

#[test]
fn no_disc_is_check_condition_not_ready_and_request_sense_says_why() {
    let mut h = host(None, 0);
    h.command(&[0, 0, 0, 0, 0, 0]);
    assert_eq!(h.finish(), (2, 0)); // CHECK CONDITION
    let (data, (status, _)) = h.run(&[3, 0, 0, 0, 18, 0], 18, 1);
    assert_eq!(status, 0);
    assert!(data[0] == 0x70 && data[2] == 0x02 && data[12] == 0x3A); // not ready, no medium
}

#[test]
fn read_one_sector_through_1801() {
    let mut h = host(Some(disc_data_only(40)), 0);
    let (data, done) = h.run(&[8, 0, 0, 7, 1, 0], 2048, 1);
    assert_eq!(done, (0, 0));
    assert_eq!(data[..6], [7, 8, 9, 10, 11, 12]);
    assert_eq!(data[2047], ((7 + 2047) & 0xFF) as u8);
}

#[test]
fn read_via_1808_pulses_ack_by_itself() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 3, 1, 0]);
    let data = h.read_data_bytes(2048, 8);
    assert!(data[0] == 3 && data[100] == 103);
    assert_eq!(h.finish(), (0, 0));
}

#[test]
fn read_three_sectors_at_single_speed() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 5, 3, 0]);
    let began = h.t;
    let data = h.read_data_bytes(3 * 2048, 8);
    let elapsed = h.t - began;
    assert_eq!([data[0], data[2048], data[4096]], [5, 6, 7]);
    // three sectors: a seek, then 75 a second; never faster than 2 sector-times after the seek
    assert!(elapsed >= 2 * timing().sector_clocks);
    assert_eq!(h.finish(), (0, 0));
}

#[test]
fn a_late_reader_waits_for_a_re_read() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 0, 2, 0]);
    let first = h.read_data_bytes(1, 8); // take one byte of sector 0, then stall
    h.advance(3 * timing().sector_clocks); // sector 1 is due, the buffer is not empty
    assert!(!h.unit.bus.out().is_empty()); // still holding sector 0's bytes
    let began = h.t;
    let rest = h.read_data_bytes(2047 + 1, 8);
    assert!(first[0] == 0 && rest[2047] == 1); // the next byte after sector 0 is sector 1's first
    let penalty_clocks = MASTER_HZ * timing().reread_penalty_ms / 1000;
    assert!(h.t - began >= penalty_clocks / 2);
}

#[test]
fn read_beyond_the_disc_is_check_condition_illegal_request() {
    let mut h = host(Some(disc_data_only(10)), 0);
    h.command(&[8, 0, 0, 200, 1, 0]);
    assert_eq!(h.finish(), (2, 0));
    assert_eq!((h.unit.bus.sense_key, h.unit.bus.sense_asc), (5, 0x21));
}

#[test]
fn read_count_zero_is_nothing_to_read() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 0, 0, 0]);
    assert_eq!(h.finish(), (0, 0));
}

#[test]
fn unknown_command_is_good_with_no_data() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[0x42]);
    assert_eq!(h.finish(), (0, 0));
}

#[test]
fn read_toc_modes() {
    let mut h = host(Some(disc_audio_then_data(40, 20)), 0);
    let (data, _) = h.run(&[0xDE, 0, 0, 0, 0, 0, 0, 0, 0, 0], 2, 1);
    assert_eq!(data, [0x01, 0x03]); // first and last track, BCD
    let (data, _) = h.run(&[0xDE, 1, 0, 0, 0, 0, 0, 0, 0, 0], 3, 1);
    assert_eq!(data, [0x00, 0x03, 0x15]); // lead-out LBA 90 + 150 = 240 frames = 00:03:15
    let (data, _) = h.run(&[0xDE, 2, to_bcd(2), 0, 0, 0, 0, 0, 0, 0], 4, 1);
    assert_eq!(data, [0x00, 0x02, 0x40, 0x04]); // track 2 starts at LBA 40: 00:02:40, data (4)
    let (data, _) = h.run(&[0xDE, 2, to_bcd(1), 0, 0, 0, 0, 0, 0, 0], 4, 1);
    assert_eq!(data, [0x00, 0x02, 0x00, 0x00]); // track 1: 00:02:00, audio
}

#[test]
fn status_message_phases_and_irq_bits_follow_req() {
    let mut h = host(Some(disc_data_only(40)), 0x70);
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.wait(0xD8); // status phase: REQ with I/O and C/D
    assert!(h.unit.active & 0x20 != 0); // status/message-in
    h.handshake();
    h.wait(0xF8); // message in
    assert!(h.unit.active & 0x20 != 0);
    h.handshake();
    h.wait_mask(0, 0xF8);
    assert_eq!(h.unit.active, 0);
}

#[test]
fn data_in_raises_the_data_irq_bit() {
    let mut h = host(Some(disc_data_only(40)), 0x70);
    h.command(&[8, 0, 0, 1, 1, 0]);
    h.wait(0xC8);
    assert!(h.unit.active & 0x40 != 0 && h.unit.active & 0x20 == 0);
}

#[test]
fn reset_through_1804_aborts_everything() {
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 1, 1, 0]);
    h.wait(0xC8);
    h.w(4, 0x02);
    h.w(4, 0x00);
    assert!(h.r(0) == 0 && h.unit.bus.phase.value() == "bus free");
}

#[test]
fn selecting_during_data_in_aborts_with_eight_status_bytes_then_the_message() {
    // Closed-box Geargrafx (the reference's scripts/drive_probe.py): SEL in data in gives the
    // status phase at once, eight REQs carrying GOOD, then the message, then bus free; the BIOS
    // takes REQs until BSY drops. (The withdrawn test looked at the old model's buffer instead.)
    let mut h = host(Some(disc_data_only(40)), 0);
    h.command(&[8, 0, 0, 1, 1, 0]);
    h.wait(0xC8);
    h.select();
    assert!(h.unit.bus.phase.value() == "status" && h.r(0) & 0xF8 == 0xD8);
    let mut offered = Vec::new();
    while h.r(0) & 0x80 != 0 {
        if h.r(0) & 0x40 != 0 {
            offered.push((h.r(0) & 0xF8, h.r(1)));
            h.handshake();
        } else {
            h.advance(500);
        }
    }
    let mut want = vec![(0xD8, 0); 8];
    want.push((0xF8, 0));
    assert_eq!(offered, want);
    let (data, status) = h.run(&[8, 0, 0, 2, 1, 0], 2048, 8); // the drive works after the abort
    assert!(data[0] == 2 && status == (0, 0));
}

#[test]
fn audio_end_repeat_of_an_empty_range_plays_nothing_and_stays_busy() {
    // MAME's note (an end before the start plays nothing) and the closed box (BSY alone after a
    // repeat): the drive stays busy until the next selection, and nothing loops.
    let mut h = host(Some(disc_audio_then_data(40, 20)), 0);
    h.command(&[0xD8, 1, to_bcd(3), 0, 0, 0, 0, 0, 0, 0x80]); // start at track 3, play
    h.finish();
    h.command(&[0xD9, 1, to_bcd(2), 0, 0, 0, 0, 0, 0, 0x80]); // repeat up to track 2: empty
    h.advance(MASTER_HZ / 10);
    assert!(h.r(0) & 0xF8 == 0x80 && !h.unit.cdda().active());
    let (_, status) = h.run(&[0, 0, 0, 0, 0, 0], 0, 1); // the next selection is served
    assert_eq!(status, (0, 0));
}
