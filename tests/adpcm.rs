// SPDX-License-Identifier: MIT
//! The ADPCM unit from the register semantics of pcetech 8, the Hudson BIOS routines (AD_STAT,
//! AD_PLAY, the `ad_*_addr_set` helpers) and the MSM5205 decoder. Directed tests, no emulator
//! (the reference's tests/test_adpcm.py).

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{Host, MASTER_HZ};
use pcengine_rust::sockets::{AudioSource, PortDevice};
use pcenginecd_rust::adpcm::{Adpcm, DELTA, INDEX_STEP};
use pcenginecd_rust::board::{unit, PcEngineCd};
use pcenginecd_rust::cdda::Fader;
use pcenginecd_rust::unit::CdUnit;

fn make() -> Host {
    Host::new(CdUnit::new(None))
}

fn bare() -> Adpcm {
    Adpcm::new(Rc::new(Cell::new(Fader::default())))
}

/// AD_WRITE: set the write pointer, then write bytes polling `$180C` bit 2.
fn load(h: &mut Host, address: u16, data: &[u8]) {
    h.w(0x08, address as u8);
    h.w(0x09, (address >> 8) as u8);
    h.w(0x0D, 0x03); // write pointer = latch (bit 0 set: no step back)
    h.w(0x0D, 0x02);
    for &byte in data {
        h.w(0x0A, byte);
        while h.r(0x0C) & 0x04 != 0 {
            h.advance(12);
        }
    }
    h.w(0x0D, 0x00);
}

/// AD_PLAY as the BIOS drives it: read pointer (with the priming read), length, rate, play.
fn start(h: &mut Host, address: u16, length: u16, rate: u8, stop_at_end: bool) {
    h.w(0x08, address as u8);
    h.w(0x09, (address >> 8) as u8);
    h.w(0x0D, 0x08); // read pointer = latch - 1 ...
    h.advance(100);
    h.r(0x0A); // ... and the priming read steps it onto the first byte
    h.advance(100);
    h.w(0x0D, 0x00);
    h.w(0x08, length as u8);
    h.w(0x09, (length >> 8) as u8);
    h.w(0x0D, 0x10); // latch the length
    h.w(0x0D, 0x00);
    h.w(0x0E, rate);
    h.w(0x0D, 0x20 | if stop_at_end { 0x40 } else { 0 });
}

#[test]
fn the_step_table_has_the_msm5205_shape() {
    assert_eq!(DELTA.len(), 49 * 8);
    assert_eq!(DELTA[..8], [2, 6, 10, 14, 18, 22, 26, 30]); // step 16: step/8 + b0 step/4 + ...
    assert_eq!(
        DELTA[49 * 8 - 8..],
        [194, 582, 970, 1358, 1746, 2134, 2522, 2910]
    ); // step 1552
    assert_eq!(INDEX_STEP, [-1, -1, -1, -1, 2, 4, 6, 8]);
    // the reference computes the steps as floor(16 * 1.1 ** index): the same integers
    for (index, &step) in pcenginecd_rust::adpcm::STEPS.iter().enumerate() {
        assert_eq!(
            step,
            (16.0 * 1.1f64.powi(index as i32)) as i32,
            "index {index}"
        );
    }
}

#[test]
fn the_decoder_moves_the_signal_by_sign_and_magnitude_and_the_index_by_the_magnitude() {
    let mut a = bare();
    a.ram[0] = 0x77; // +magnitude 7, then +7 (high nibble first)
    a.ram[1] = 0xF9; // sign bit and magnitude 7 (negative), then +1
    (a.control, a.length, a.rate) = (0x20, 4, 0x0F);
    (a.play_request, a.sample_at) = (true, 0);
    a.run(0);
    assert_eq!((a.signal, a.index), (DELTA[7], 8));
    a.run(a.next_event().unwrap());
    assert_eq!((a.signal, a.index), (DELTA[7] + DELTA[8 * 8 + 7], 16));
    a.run(a.next_event().unwrap()); // high nibble 0xF: negative, magnitude 7
    assert_eq!(
        (a.signal, a.index),
        (DELTA[7] + DELTA[8 * 8 + 7] - DELTA[16 * 8 + 7], 24)
    );
}

#[test]
fn the_signal_saturates_by_default_and_wraps_with_the_switch() {
    for saturate in [true, false] {
        let mut a = bare();
        a.switches.saturate = saturate;
        (a.signal, a.index) = (2000, 48);
        a.playing = true;
        a.ram[0] = 0x70;
        (a.control, a.length, a.sample_at) = (0x20, 2, 0);
        a.run(0);
        if saturate {
            assert_eq!(a.signal, 2047);
        } else {
            assert_eq!(a.signal, ((2000 + DELTA[48 * 8 + 7] + 2048) & 0xFFF) - 2048);
            assert_eq!(a.signal, 814);
        }
    }
}

#[test]
fn the_rate_register_sets_32_over_16_minus_n_kilohertz() {
    let mut a = bare();
    for (n, hz) in [(0, 2000.0), (0x0E, 16000.0), (0x0F, 32000.0)] {
        a.rate = n | 0xF0; // only the low four bits count
        assert!((MASTER_HZ as f64 * 65536.0 / a.period_fixed() as f64 - hz).abs() < 0.01);
    }
}

#[test]
fn a_read_is_buffered_and_bit_7_of_180c_clears_in_about_24_cycles() {
    let mut h = make();
    load(&mut h, 0x0100, &[0x11, 0x22]);
    h.w(0x08, 0x00);
    h.w(0x09, 0x01);
    h.w(0x0D, 0x08 | 0x04); // read pointer = latch
    let first = h.r(0x0A);
    assert_eq!(first, 0); // the old buffer
    assert!(h.r(0x0C) & 0x80 != 0); // busy
    h.advance(h.unit.adpcm.switches.read_clocks);
    assert!(h.r(0x0C) & 0x80 == 0);
    assert_eq!(h.r(0x0A), 0x11);
}

#[test]
fn a_write_is_busy_for_the_same_time_and_the_pointer_steps_after_it() {
    let mut h = make();
    h.w(0x0D, 0x02 | 0x01);
    h.w(0x0A, 0x5A);
    assert!(h.r(0x0C) & 0x04 != 0);
    assert_eq!(h.unit.adpcm.ram[0], 0);
    h.advance(h.unit.adpcm.switches.write_clocks);
    assert!(h.r(0x0C) & 0x04 == 0);
    assert!(h.unit.adpcm.ram[0] == 0x5A && h.unit.adpcm.write_ptr == 1);
}

#[test]
fn playback_runs_to_the_end_flag_and_raises_irq2_through_the_enable_mask() {
    let mut h = make();
    load(&mut h, 0x0000, &[0x77, 0x77, 0x77, 0x77]);
    h.w(0x02, 0x08); // enable the ADPCM end source
    start(&mut h, 0x0000, 4, 0x0E, true);
    assert!(!h.irq() && h.r(3) & 0x08 == 0);
    let t = h.t;
    h.advance(MASTER_HZ / 100); // 10 ms is far longer than 8 samples at 16 kHz
    assert!(h.irq() && h.r(3) & 0x08 != 0);
    assert!(h.r(0x0C) & 0x01 != 0); // end reached
    assert!(h.r(0x0C) & 0x08 == 0); // no longer playing
    assert!(h.unit.adpcm.read_ptr == 4 && h.unit.adpcm.length == 0);
    assert!(h.t - t > 0);
}

#[test]
fn playback_takes_the_time_the_rate_says() {
    let mut h = make();
    load(&mut h, 0x0000, &[0x11; 16]);
    start(&mut h, 0x0000, 16, 0x0E, true); // 16 bytes = 32 samples at 16 kHz = 2 ms
    h.advance(MASTER_HZ * 18 / 10_000); // 1.8 ms
    assert!(h.r(0x0C) & 0x01 == 0 && h.r(0x0C) & 0x08 != 0);
    h.advance(MASTER_HZ * 5 / 10_000); // 2.3 ms in all
    assert!(h.r(0x0C) & 0x01 != 0 && h.r(0x0C) & 0x08 == 0);
}

#[test]
fn the_end_source_stays_off_without_its_enable_bit_and_clears_with_a_new_length() {
    let mut h = make();
    load(&mut h, 0x0000, &[0x11, 0x11]);
    start(&mut h, 0x0000, 2, 0x0E, true);
    h.advance(MASTER_HZ / 100);
    assert!(h.unit.active & 0x08 != 0 && !h.irq());
    h.w(0x08, 0x10);
    h.w(0x09, 0x00);
    h.w(0x0D, 0x10); // the length latch clears the end flag
    assert!(h.unit.active & 0x08 == 0 && h.r(0x0C) & 0x01 == 0);
}

#[test]
fn the_half_flag_is_set_while_less_than_half_of_the_buffer_is_left() {
    let mut h = make();
    h.unit.adpcm.ram[0] = 0x11;
    h.w(0x08, 0x00);
    h.w(0x09, 0x00);
    h.w(0x0D, 0x08 | 0x04);
    h.advance(100);
    h.unit.adpcm.length = 0x9000; // more than half left: playing does not set it
    h.w(0x0E, 0x0F);
    h.w(0x0D, 0x20);
    h.advance(MASTER_HZ / 1000);
    assert!(h.unit.active & 0x04 == 0);
    h.unit.adpcm.length = 0x7FFF; // (the tests may set the counter directly)
    h.advance(MASTER_HZ / 1000);
    assert!(h.unit.active & 0x04 != 0 && h.r(3) & 0x04 != 0);
}

#[test]
fn reset_zeroes_pointers_and_length_and_keeps_the_read_buffer() {
    let mut h = make();
    load(&mut h, 0x0000, &[0xC3]);
    h.w(0x08, 0x00);
    h.w(0x09, 0x00);
    h.w(0x0D, 0x0C);
    h.advance(100);
    h.r(0x0A);
    h.advance(100);
    let buffered = h.unit.adpcm.read_buffer;
    assert_eq!(buffered, 0xC3);
    h.w(0x0D, 0x80);
    h.w(0x0D, 0x00);
    let a = &h.unit.adpcm;
    assert!((a.read_ptr, a.write_ptr, a.length) == (0, 0, 0) && a.read_buffer == buffered);
}

#[test]
fn the_cd_dma_goes_through_the_write_path_and_counts_the_length() {
    // AD_TRANS bytes arrive as writes: the length counter follows (the half/end rules apply)
    let mut h = make();
    h.w(0x0D, 0x03);
    h.w(0x0D, 0x02);
    for value in [1, 2, 3] {
        let now = h.t;
        assert!(h.unit.adpcm.dma_write(value, now));
        assert!(!h.unit.adpcm.dma_write(9, now)); // still busy: the DMA must retry
        h.advance(h.unit.adpcm.switches.write_clocks);
    }
    assert!(h.unit.adpcm.ram[..3] == [1, 2, 3] && h.unit.adpcm.length == 3);
}

#[test]
fn the_audio_source_holds_the_decoder_output_at_44100_hz() {
    let mut h = make();
    assert!(h.unit.adpcm.take_samples(h.t).is_empty()); // the origin
    load(&mut h, 0x0000, &[0x77; 8]);
    start(&mut h, 0x0000, 8, 0x0E, true);
    h.advance(MASTER_HZ / 200); // 5 ms
    let samples = h.unit.adpcm.take_samples(h.t);
    assert!(200 < samples.len() && samples.len() < 230); // 44.1 kHz for 5 ms plus the loads above
    assert!(samples.iter().map(|s| s.0).max().unwrap() > 0); // the signal climbed
    assert!(samples.iter().all(|&(a, b)| a == b)); // mono on both channels
    assert_eq!(samples.last().unwrap().0, 0); // the unit stopped at the end: the DAC is idle
}

#[test]
fn the_fade_aimed_at_adpcm_scales_only_adpcm() {
    let mut h = make();
    h.unit.adpcm.take_samples(h.t);
    load(&mut h, 0x0000, &[0x77; 200]);
    start(&mut h, 0x0000, 200, 0x0E, true);
    let now = h.t;
    h.unit.cdda().write_fade(0x0A, now); // ADPCM, slow fade (the Hudson BIOS manual's CD_FADE $0A)
    h.advance(MASTER_HZ * 9 / 1000);
    let now = h.t;
    let early: Vec<i32> = h.unit.adpcm.take_samples(now).iter().map(|s| s.0).collect();
    let cdda = h.unit.cdda();
    assert_eq!(cdda.fade_gain(now, false), 1.0); // CD-DA untouched
    assert_eq!(cdda.fade_gain(now + MASTER_HZ * 20, true), 0.0);
    assert!((cdda.fade_gain(now + MASTER_HZ * 3, true) - 0.5).abs() < 0.01); // 6 s default
    cdda.fader.set(Fader {
        pcetech_fade: true,
        ..cdda.fader.get()
    });
    assert!((cdda.fade_gain(now + MASTER_HZ * 4, true) - 0.5).abs() < 0.01); // 8 s switch
    assert!(*early.iter().max().unwrap() > 0);
}

#[test]
fn a_faded_sample_on_a_half_rounds_to_even_as_pythons_round_does() {
    // not in the reference's tests: its `round(value * gain)` is Python's, ties to even
    let mut a = bare();
    assert!(a.take_samples(0).is_empty()); // the origin
    a.fader.set(Fader {
        fade_register: 0x0A, // the slow fade, aimed at ADPCM, begun at time zero
        ..Fader::default()
    });
    // sample 3 * 44100 falls exactly three seconds in, where the six-second fade is one half
    for (held, faded) in [(5, 2), (7, 4), (-5, -2), (6, 3)] {
        a.taken = 3 * 44100;
        a.held = held;
        assert_eq!(a.take_samples(3 * MASTER_HZ + 488), [(faded, faded)]);
    }
}

#[test]
fn the_board_mixes_adpcm_with_the_other_sources() {
    let mut board = PcEngineCd::new(&vec![0; 0x40000], None)
        .build_board()
        .unwrap();
    assert_eq!(unit(&mut board).unwrap().audio_sources().len(), 2); // CD-DA and ADPCM
    board.take_audio(); // CD-DA and ADPCM start their streams at the first call
    board.advance(MASTER_HZ / 100);
    let out = board.take_audio();
    assert!(out.len() > 100);
}
