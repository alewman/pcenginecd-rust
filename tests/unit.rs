// SPDX-License-Identifier: MIT
//! The CD unit's registers, RAM, BRAM, IRQ2 and the ADPCM register port (the reference's
//! tests/test_unit.py).

mod common;

use common::{disc_data_only, Host, REG};
use pcengine_rust::sockets::{PortDevice, ResetKind};
use pcenginecd_rust::disc::Disc;
use pcenginecd_rust::unit::{CdUnit, BRAM_FORMATTED};

fn make(disc: Option<Disc>) -> Host {
    Host::new(CdUnit::new(disc))
}

#[test]
fn claims_cover_cd_ram_bram_and_the_register_page() {
    let h = make(None);
    let claims = h.unit.claims();
    assert!(claims.contains(&(0x1000, 0x10FF))); // banks $80-$87 = $100000-$10FFFF
    assert!(claims.contains(&(0x1EE0, 0x1EE7))); // $1EE000-$1EE7FF
    assert!(claims.contains(&(0x1FF8, 0x1FF8)));
    assert!(h.unit.present());
}

#[test]
fn cd_ram_is_64k_not_mirrored_inside_its_window() {
    let mut h = make(None);
    h.write(0x100000, 0x11, 0);
    h.write(0x10FFFF, 0x22, 0);
    assert!(h.read(0x100000, 0) == 0x11 && h.read(0x10FFFF, 0) == 0x22);
    assert_eq!(h.read(0x100001, 0), 0);
}

#[test]
fn bram_is_locked_at_power_up_unlocked_by_1807_bit7_locked_by_reading_1803() {
    let mut h = make(None);
    assert_eq!(h.read(0x1EE000, 0), 0xFF); // reads $FF while locked
    h.write(0x1EE010, 0x5A, 0); // ignored
    h.w(7, 0x80);
    assert_eq!(h.read(0x1EE000, 0), BRAM_FORMATTED[0]);
    h.write(0x1EE010, 0x5A, 0);
    assert_eq!(h.read(0x1EE010, 0), 0x5A);
    h.r(3); // a read of $1803 locks
    assert_eq!(h.read(0x1EE010, 0), 0xFF);
    h.w(7, 0x80 | 0x48); // "some games write 48 75 80": only bit 7 of the last matters
    assert_eq!(h.read(0x1EE010, 0), 0x5A);
    h.w(7, 0x00);
    assert_eq!(h.read(0x1EE010, 0), 0xFF);
}

#[test]
fn bram_is_2k_only_the_rest_of_the_bank_is_open_bus() {
    let mut h = make(None);
    h.w(7, 0x80);
    assert!(!(0x1EE0..0x1EE8).contains(&(0x1EE800 >> 8)));
    assert!([0x1EE000 >> 8, 0x1EE7FF >> 8]
        .iter()
        .all(|p| (0x1EE0..=0x1EE7).contains(p)));
}

#[test]
fn backup_ram_goes_out_and_comes_back_for_the_host_to_keep() {
    // not in the reference: `bram()` / `set_bram()` and the constructor's image
    let mut h = make(None);
    h.w(7, 0x80);
    h.write(0x1EE123, 0x77, 0);
    let kept = h.unit.bram().to_vec();
    assert_eq!((kept.len(), kept[0], kept[0x123]), (2048, 0x48, 0x77));
    let mut again = Host::new(CdUnit::with(None, Some(&kept), None).unwrap());
    again.w(7, 0x80);
    assert_eq!(again.read(0x1EE123, 0), 0x77);
    let mut third = make(None);
    third.unit.set_bram(&kept);
    third.w(7, 0x80);
    assert_eq!(third.read(0x1EE123, 0), 0x77);
}

#[test]
fn irq2_follows_enable_mask_and_active_bits_per_source() {
    let mut h = make(Some(disc_data_only(40)));
    h.w(2, 0x00);
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.wait(0xD8); // status phase raises active bit 0x20
    assert!(h.unit.active == 0x20 && !h.irq()); // masked
    h.w(2, 0x20);
    assert!(h.irq());
    assert_eq!(h.pins.irq2.sources().iter().collect::<Vec<_>>(), ["cd"]);
    h.w(2, 0x40); // a different mask
    assert!(!h.irq());
    h.w(2, 0x20);
    assert!(h.r(3) & 0x20 != 0); // $1803 shows the active bits (and does not clear them)
    h.handshake();
    h.wait(0xF8); // message in: same bit
    assert!(h.irq());
    h.handshake();
    h.wait_mask(0, 0xF8);
    assert!(!h.irq()); // REQ gone: level source
}

#[test]
fn register_1802_reads_back_mask_and_ack() {
    let mut h = make(None);
    h.w(2, 0xE5);
    assert_eq!(h.r(2), 0xE5);
    h.w(2, 0x65);
    assert_eq!(h.r(2), 0x65);
}

#[test]
fn register_1803_bit1_is_the_channel_flag_toggled_by_1805_with_a_guard() {
    let mut h = make(None);
    assert!(h.r(3) & 0x02 != 0);
    h.w(5, 0);
    assert!(h.r(3) & 0x02 == 0);
    h.w(5, 0); // within the guard time: ignored
    assert!(h.r(3) & 0x02 == 0);
    h.advance(2000);
    h.w(5, 0);
    assert!(h.r(3) & 0x02 != 0);
}

#[test]
fn reset_register_clears_drive_irq_enables_and_the_drive_works_after_release() {
    // (the blocking while held: tests/issue0001.rs)
    let mut h = make(Some(disc_data_only(40)));
    h.w(2, 0x70);
    h.w(4, 0x02);
    assert!(h.unit.enable == 0x00 && h.r(4) == 0x02);
    h.w(4, 0x00);
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.wait(0xD8);
    assert!(h.unit.active & 0x20 != 0);
}

#[test]
fn adpcm_ram_port_reads_are_buffered_and_pointers_increment_and_wrap() {
    let mut h = make(None);
    h.w(8, 0x00);
    h.w(9, 0x10); // latch $1000
    h.w(0x0D, 0x02 | 0x01); // copy to the write pointer, no step back
    for v in [0xA1, 0xA2, 0xA3] {
        h.w(0x0A, v);
        h.advance(100); // the RAM takes a write in about 24 CPU cycles ($180C bit 2)
    }
    assert_eq!(h.unit.adpcm.ram[0x1000..0x1003], [0xA1, 0xA2, 0xA3]);
    h.w(0x0D, 0x00);
    h.w(0x0D, 0x08 | 0x04); // read pointer = latch
    let first = h.r(0x0A); // the buffer from before
    let mut values = Vec::new();
    for _ in 0..3 {
        h.advance(100); // a read is buffered: bit 7 of $180C clears in about 24 cycles
        values.push(h.r(0x0A));
    }
    assert_eq!(values, [0xA1, 0xA2, 0xA3]);
    assert_eq!(first, 0);
    h.w(0x0D, 0x80); // reset: pointers to zero
    assert!(h.unit.adpcm.read_ptr == 0 && h.unit.adpcm.write_ptr == 0);
}

#[test]
fn adpcm_status_reads_idle_and_unknown_registers_read_ff() {
    let mut h = make(None);
    assert_eq!(h.r(0x0C), 0);
    assert!(h.read(REG + 0x20, 0) == 0xFF && h.read(REG + 0xFE, 0) == 0xFF);
}

#[test]
fn power_on_reset_locks_bram_and_clears_the_bus() {
    let mut h = make(Some(disc_data_only(40)));
    h.w(7, 0x80);
    h.command(&[0, 0, 0, 0, 0, 0]);
    h.unit.reset(ResetKind::PowerOn, &mut h.pins.pins());
    assert!(h.unit.bram_locked && h.r(0) == 0 && !h.irq());
}

#[test]
fn adpcm_dma_moves_a_sector_from_the_drive_into_adpcm_ram() {
    // AD_TRANS: set the write pointer, send READ, set $180B bit 1, wait for the status phase
    let mut h = make(Some(disc_data_only(40)));
    h.w(8, 0x00);
    h.w(9, 0x20);
    h.w(0x0D, 0x02 | 0x01); // write pointer = $2000
    h.command(&[8, 0, 0, 4, 1, 0]);
    h.w(0x0B, 0x02);
    h.wait(0xD8); // the status phase: every byte was taken by the DMA, none by the CPU
    h.advance(200); // the last byte is still going into the RAM
    assert_eq!(h.unit.adpcm.ram[0x2000..0x2004], [4, 5, 6, 7]);
    assert_eq!(h.unit.adpcm.ram[0x2000 + 2047], ((4 + 2047) & 0xFF) as u8);
    assert_eq!(h.unit.adpcm.write_ptr, 0x2000 + 2048);
    assert!(h.r(3) & 0x20 != 0); // the BIOS polls this bit
    h.w(0x0B, 0x00);
    assert_eq!(h.finish(), (0, 0));
}

#[test]
fn dma_bit0_cannot_be_set_without_a_waiting_block_and_bit1_survives() {
    let mut h = make(Some(disc_data_only(40)));
    h.w(0x0B, 0x03);
    assert_eq!(h.unit.adpcm.dma, 0x02);
}
