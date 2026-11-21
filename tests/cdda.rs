// SPDX-License-Identifier: MIT
//! CD-DA: audio start/end/pause, READ SUBQ, the sample source, the $1805 latch, the fader (the
//! reference's tests/test_cdda.py).

mod common;

use std::collections::HashSet;

use common::{audio_sectors, disc_audio_then_data, Host, MASTER_HZ as MASTER};
use pcengine_rust::sockets::AudioSource;
use pcenginecd_rust::cdda::{Fader, Status, SAMPLE_HZ};
use pcenginecd_rust::disc::to_bcd;
use pcenginecd_rust::unit::CdUnit;

/// The CD-DA fade gain `after` master clocks from the host's clock.
fn gain(h: &mut Host, after: i64) -> f64 {
    let at = h.t + after;
    h.unit.cdda().fade_gain(at, false)
}

fn make() -> Host {
    Host::new(CdUnit::new(Some(disc_audio_then_data(40, 20))))
}

fn play_track(h: &mut Host, track: i64) -> (u8, u8) {
    // AUDIO START by track number (mode bits 7-6 = 10) then wait for the busy phase to end
    h.command(&[0xD8, 1, to_bcd(track), 0, 0, 0, 0, 0, 0, 0x80]);
    h.finish()
}

#[test]
fn audio_start_is_busy_until_the_seek_ends_then_good_status() {
    let mut h = make();
    h.command(&[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
    h.wait(0x80); // BSY alone: the drive is busy while it finds the sector
    let began = h.t;
    assert_eq!(h.finish(), (0, 0));
    assert!(h.t - began > 1_000_000); // a seek costs milliseconds, not microseconds
    assert_eq!(h.unit.cdda().status, Status::Playing);
}

#[test]
fn subq_reports_position_track_and_status_while_playing() {
    let mut h = make();
    play_track(&mut h, 1);
    h.advance(MASTER / 75 * 5 + 1000); // five sectors of playing
    let (data, _) = h.run(&[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10, 1);
    assert_eq!(data[0], 0); // playing
    assert_eq!(data[1], 0x01); // ADR 1, audio
    assert!(data[2] == 0x01 && data[3] == 1); // track 1, index 1
    assert_eq!(data[4..7], [0, 0, 5]); // five frames into the track (relative BCD)
    assert_eq!(data[7..10], [0, 2, 5]); // absolute: 00:02:05
}

#[test]
fn pause_freezes_the_position_and_reports_paused() {
    let mut h = make();
    play_track(&mut h, 1);
    h.advance(MASTER / 75 * 3);
    h.run(&[0xDA, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0, 1);
    assert_eq!(h.unit.cdda().status, Status::Paused);
    let frozen = h.unit.cdda().position();
    h.advance(MASTER / 75 * 10);
    assert_eq!(h.unit.cdda().position(), frozen);
    let (data, _) = h.run(&[0xDD, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10, 1);
    assert_eq!(data[0], 2);
}

#[test]
fn audio_end_mode_2_keeps_the_bus_busy_until_the_range_ends() {
    let mut h = make();
    h.command(&[0xD8, 0, 0x01, 0, 0, 0, 0, 0, 0, 0x80]); // start, paused
    h.finish();
    assert_eq!(h.unit.cdda().status, Status::Paused);
    // end at LBA 9 (inclusive) by LBA address, mode 2 = busy until done, then status
    h.command(&[0xD9, 2, 0, 0, 0, 9, 0, 0, 0, 0x00]);
    h.wait(0x80);
    let began = h.t;
    assert_eq!(h.finish(), (0, 0));
    let ten_sectors = 10 * MASTER / 75;
    assert!((((h.t - began) - ten_sectors).abs() as f64) < ten_sectors as f64 * 0.15);
    assert_eq!(h.unit.cdda().status, Status::Stopped);
}

#[test]
fn audio_end_mode_1_loops() {
    let mut h = make();
    h.command(&[0xD8, 0, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
    h.finish();
    h.command(&[0xD9, 1, 0, 0, 0, 3, 0, 0, 0, 0x00]);
    h.advance(MASTER / 75 * 20); // five times the 4-sector range
    assert_eq!(h.unit.cdda().status, Status::Playing);
    assert!((0..=3).contains(&h.unit.cdda().position()));
}

#[test]
fn start_by_msf_uses_the_absolute_time_with_the_lead_in() {
    let mut h = make();
    // 00:02:10 = LBA 10, in track 1
    h.command(&[0xD8, 1, 0x00, 0x02, 0x10, 0, 0, 0, 0, 0x40]);
    h.finish();
    assert_eq!(h.unit.cdda().start, 10);
}

#[test]
fn samples_come_out_at_44100_and_match_the_disc() {
    let mut h = make();
    play_track(&mut h, 1);
    let start = h.t;
    let cdda = h.unit.cdda();
    cdda.take_samples(start); // set the origin
    let out = cdda.take_samples(start + MASTER / 10);
    assert!((out.len() as i64 - SAMPLE_HZ / 10).abs() <= 1);
    let first_sectors: HashSet<(i32, i32)> = audio_sectors(12)
        .chunks_exact(4)
        .map(|s| {
            (
                i16::from_le_bytes([s[0], s[1]]) as i32,
                i16::from_le_bytes([s[2], s[3]]) as i32,
            )
        })
        .collect();
    let played: Vec<_> = out.iter().filter(|&&s| s != (0, 0)).collect();
    assert!(played.len() > 3000); // the 0.1 s window is nearly all playing, 12 sectors' worth
    assert!(played.iter().all(|s| first_sectors.contains(s))); // exactly the disc's samples
    assert!(played.iter().map(|s| s.0.abs()).max().unwrap() > 1000);
    assert!(played.iter().map(|s| s.1.abs()).max().unwrap() > 1000);
}

#[test]
fn samples_are_silence_when_not_playing_but_still_counted() {
    let mut h = make();
    h.unit.cdda().take_samples(0);
    let out = h.unit.cdda().take_samples(MASTER / 100);
    assert!(out.len() >= 440 && out.iter().all(|&s| s == (0, 0)));
}

#[test]
fn register_1805_latches_left_then_right_sample() {
    let mut h = make();
    play_track(&mut h, 1);
    h.advance(MASTER / 75 * 2 + 5000);
    h.w(5, 0); // toggles to right
    let right = h.r(5) as u16 | (h.r(6) as u16) << 8;
    h.advance(2000);
    h.w(5, 0); // toggles to left
    let left = h.r(5) as u16 | (h.r(6) as u16) << 8;
    assert_ne!((left, right), (0, 0));
}

#[test]
fn fader_fades_cd_audio_over_six_or_two_and_a_half_seconds() {
    let mut h = make();
    h.w(0x0F, 0x08);
    assert_eq!(gain(&mut h, 0), 1.0);
    assert!((gain(&mut h, 3 * MASTER) - 0.5).abs() < 0.01);
    assert_eq!(gain(&mut h, 7 * MASTER), 0.0);
    h.w(0x0F, 0x0C); // fast: a different value restarts the timer
    assert!((gain(&mut h, MASTER * 5 / 4) - 0.5).abs() < 0.01);
    assert_eq!(gain(&mut h, 3 * MASTER), 0.0);
    h.w(0x0F, 0x00);
    assert_eq!(gain(&mut h, 100 * MASTER), 1.0);
    h.w(0x0F, 0x0A); // aimed at ADPCM: CD audio unaffected
    assert_eq!(gain(&mut h, 100 * MASTER), 1.0);
}

#[test]
fn the_pcetech_fade_times_are_a_switch() {
    let mut h = make();
    let fader = &h.unit.cdda().fader;
    fader.set(Fader {
        pcetech_fade: true, // this unit's fader: the default stays
        ..fader.get()
    });
    h.w(0x0F, 0x08);
    assert!((gain(&mut h, 4 * MASTER) - 0.5).abs() < 0.01); // 8 s
    assert_eq!(gain(&mut h, 9 * MASTER), 0.0);
    h.w(0x0F, 0x0C);
    assert!((gain(&mut h, MASTER) - 0.5).abs() < 0.01); // 2 s
    assert_eq!(gain(&mut h, MASTER * 5 / 2), 0.0);
    assert!(!Fader::default().pcetech_fade);
}
