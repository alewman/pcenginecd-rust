// SPDX-License-Identifier: MIT
//! The Arcade Card: ports, auto-increment, the offset triggers, the shift unit, ID and mirrors
//! (the reference's tests/test_arcade.py).
//!
//! The semantics are those the three emulator sources agree on (tier 5, arcade.rs); the tests
//! state them, they do not judge the card against hardware. Registers are driven by physical
//! address as the board's page table hands them to the unit: `$1A00 + n` is `0x1FF800 + 0x200 +
//! n`.

mod common;

use common::{card_image, Host};
use pcengine_rust::fake_devices::ScriptedCpu;
use pcengine_rust::sockets::{PortDevice, ResetKind};
use pcenginecd_rust::arcade::{ArcadeCard, ArcadeSwitches, ARCADE_RAM_BYTES};
use pcenginecd_rust::board::{arcade, PcEngineCd};
use pcenginecd_rust::unit::CdUnit;

const AC: u32 = 0x1FFA00;

fn port(n: u32) -> u32 {
    AC + 0x10 * n
}

fn set_base(card: &mut ArcadeCard, n: u32, base: u32) {
    for i in 0..3 {
        card.write(port(n) + 2 + i, (base >> (8 * i)) as u8);
    }
}

fn set_offset(card: &mut ArcadeCard, n: u32, offset: u32) {
    card.write(port(n) + 5, offset as u8);
    card.write(port(n) + 6, (offset >> 8) as u8);
}

fn set_increment(card: &mut ArcadeCard, n: u32, inc: u32) {
    card.write(port(n) + 7, inc as u8);
    card.write(port(n) + 8, (inc >> 8) as u8);
}

fn value(card: &mut ArcadeCard) -> [u8; 4] {
    [0, 1, 2, 3].map(|i| card.read(AC + 0xE0 + i))
}

#[test]
fn ram_is_two_mebibytes_and_the_unit_claims_banks_40_43_and_the_1a_page() {
    let unit = CdUnit::with(None, None, Some(ArcadeCard::new())).unwrap();
    assert_eq!(unit.arcade.as_ref().unwrap().ram.len(), ARCADE_RAM_BYTES);
    assert_eq!(ARCADE_RAM_BYTES, 2 * 1024 * 1024);
    assert!(unit.claims().contains(&(0x0800, 0x087F)) && unit.claims().contains(&(0x1FFA, 0x1FFA)));
    assert!(!CdUnit::new(None).claims().contains(&(0x0800, 0x087F))); // nothing claimed without a card
}

#[test]
fn id_and_version() {
    let mut card = ArcadeCard::new();
    assert!(card.read(AC + 0xFF) == 0x51 && card.read(AC + 0xFE) == 0x10);
    assert!(card.read(AC + 0xFC) == 0 && card.read(AC + 0xFD) == 0);
}

#[test]
fn plain_port_write_then_read_at_the_base_with_no_increment() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x01234);
    card.write(port(0), 0xAB);
    assert_eq!(card.ram[0x1234], 0xAB);
    assert!(card.read(port(0)) == 0xAB && card.read(port(0) + 1) == 0xAB); // $0 and $1 both data
    assert!(card.read(port(0) + 2) == 0x34 && card.read(port(0) + 3) == 0x12);
    assert_eq!(card.read(port(0) + 4), 0x00);
}

#[test]
fn the_four_ports_are_independent() {
    let mut card = ArcadeCard::new();
    for n in 0..4 {
        set_base(&mut card, n, 0x100 * (n + 1));
        card.write(port(n), n as u8 + 1);
    }
    assert_eq!([1, 2, 3, 4usize].map(|n| card.ram[0x100 * n]), [1, 2, 3, 4]);
}

#[test]
fn auto_increment_of_the_offset_with_add_offset() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 1, 0x100);
    set_increment(&mut card, 1, 2);
    card.write(port(1) + 9, 0x03); // auto-increment + add offset to the base for the access
    for i in 0..3 {
        card.write(port(1), 0x10 + i);
    }
    assert_eq!(
        (card.ram[0x100], card.ram[0x102], card.ram[0x104]),
        (0x10, 0x11, 0x12)
    );
    assert!(card.read(port(1) + 5) == 6 && card.read(port(1) + 2) == 0x00); // offset moved, base not
}

#[test]
fn auto_increment_without_add_offset_moves_only_the_offset_register() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x40);
    set_increment(&mut card, 0, 1);
    card.write(port(0) + 9, 0x01);
    card.write(port(0), 1);
    card.write(port(0), 2);
    assert!(card.ram[0x40] == 2 && card.ram[0x41] == 0); // the address ignores the offset
    assert_eq!(card.read(port(0) + 5), 2);
}

#[test]
fn auto_increment_of_the_base_when_bit_4_is_set() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x200);
    set_increment(&mut card, 0, 0x10);
    card.write(port(0) + 9, 0x11);
    card.write(port(0), 7);
    card.write(port(0), 8);
    assert!(card.ram[0x200] == 7 && card.ram[0x210] == 8);
    assert!(card.read(port(0) + 2) == 0x20 && card.read(port(0) + 3) == 0x02);
    assert_eq!(card.read(port(0) + 5), 0); // the offset stayed
}

#[test]
fn increment_wraps_the_offset_at_16_bits_and_the_base_at_24_bits() {
    let mut card = ArcadeCard::new();
    set_offset(&mut card, 0, 0xFFFF);
    set_increment(&mut card, 0, 1);
    card.write(port(0) + 9, 0x01);
    card.read(port(0));
    assert!(card.read(port(0) + 5) == 0 && card.read(port(0) + 6) == 0);
    set_base(&mut card, 1, 0xFFFFFF);
    set_increment(&mut card, 1, 2);
    card.write(port(1) + 9, 0x11);
    card.read(port(1));
    assert_eq!([2, 3, 4].map(|i| card.read(port(1) + i)), [1, 0, 0]);
    // (not in the reference's test, which a base wrapping at 16 bits passes too)
    set_base(&mut card, 2, 0x00FFFF);
    set_increment(&mut card, 2, 2);
    card.write(port(2) + 9, 0x11);
    card.read(port(2));
    assert_eq!([2, 3, 4].map(|i| card.read(port(2) + i)), [1, 0, 1]);
    // the reference's own check since its issue 0001: the carry crosses bit 16 into the top byte
    set_base(&mut card, 2, 0x12FFFF);
    set_increment(&mut card, 2, 2);
    card.write(port(2) + 9, 0x11);
    card.read(port(2));
    assert_eq!([2, 3, 4].map(|i| card.read(port(2) + i)), [1, 0, 0x13]);
}

#[test]
fn effective_address_is_21_bits() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x200005); // bit 21 is not decoded: wraps to 5
    card.write(port(0), 0x77);
    assert_eq!(card.ram[5], 0x77);
    set_base(&mut card, 0, 0xFFFFFF);
    card.write(port(0), 0x78);
    assert_eq!(card.ram[0x1FFFFF], 0x78);
}

#[test]
fn add_offset_with_a_signed_offset_reaches_below_the_base() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x100);
    set_offset(&mut card, 0, 0xFFF0); // -16 as a 16-bit value
    card.write(port(0) + 9, 0x0A); // add offset, signed
    card.write(port(0), 0x5A);
    assert_eq!(card.ram[0xF0], 0x5A);
    card.write(port(0) + 9, 0x02); // unsigned: base + 0xFFF0
    card.write(port(0), 0x5B);
    assert_eq!(card.ram[0x100 + 0xFFF0], 0x5B);
}

#[test]
fn offset_trigger_modes_add_the_offset_into_the_base() {
    let mut card = ArcadeCard::new();
    // trigger on the low-byte write (bits 6-5 = 1)
    set_base(&mut card, 0, 0x1000);
    card.write(port(0) + 9, 0x20);
    card.write(port(0) + 6, 0x01); // high byte: no add
    assert_eq!(card.read(port(0) + 3), 0x10);
    card.write(port(0) + 5, 0x05); // low byte: base += 0x0105
    assert_eq!(
        (card.read(port(0) + 2), card.read(port(0) + 3)),
        (0x05, 0x11)
    );
    // trigger on the high-byte write (2)
    set_base(&mut card, 1, 0);
    card.write(port(1) + 9, 0x40);
    card.write(port(1) + 5, 0x34);
    assert_eq!(card.read(port(1) + 2), 0);
    card.write(port(1) + 6, 0x12); // base += 0x1234
    assert_eq!(
        (card.read(port(1) + 2), card.read(port(1) + 3)),
        (0x34, 0x12)
    );
    // trigger on a write to register $A (3), whatever the value, repeatedly
    set_base(&mut card, 2, 0x10);
    set_offset(&mut card, 2, 3);
    card.write(port(2) + 9, 0x60);
    card.write(port(2) + 0x0A, 0);
    card.write(port(2) + 0x0A, 0xFF);
    assert_eq!(card.read(port(2) + 2), 0x16);
    assert_eq!(card.read(port(2) + 0x0A), 0); // reads 0
                                              // trigger 0 never adds
    set_base(&mut card, 3, 0x10);
    card.write(port(3) + 9, 0x00);
    set_offset(&mut card, 3, 3);
    card.write(port(3) + 0x0A, 0);
    assert_eq!(card.read(port(3) + 2), 0x10);
}

#[test]
fn signed_trigger_subtracts_through_the_ff0000_add() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 0, 0x1000);
    card.write(port(0) + 9, 0x68); // trigger 3, signed offset
    set_offset(&mut card, 0, 0xFFFE); // -2
    card.write(port(0) + 0x0A, 0);
    assert_eq!([2, 3, 4].map(|i| card.read(port(0) + i)), [0xFE, 0x0F, 0]);
}

#[test]
fn control_register_keeps_seven_bits() {
    let mut card = ArcadeCard::new();
    card.write(port(0) + 9, 0xFF);
    assert_eq!(card.read(port(0) + 9), 0x7F);
}

#[test]
fn port_groups_mirror_at_1a40_and_unused_port_registers_read_ff() {
    let mut card = ArcadeCard::new();
    set_base(&mut card, 2, 0x12345);
    assert_eq!(card.read(AC + 0x40 + 0x20 + 2), 0x45); // port 2 again, at $1A60
    card.write(AC + 0x40 + 0x20 + 3, 0x99);
    assert_eq!(card.read(port(2) + 3), 0x99);
    assert!(card.read(port(0) + 0x0B) == 0xFF && card.read(AC + 0x90) == 0xFF);
    assert!(card.read(AC + 0xE6) == 0xFF && card.read(AC + 0xFB) == 0xFF);
}

#[test]
fn banks_40_to_43_are_port_0_to_3_data_at_any_offset() {
    let mut h = Host::new(CdUnit::with(None, None, Some(ArcadeCard::new())).unwrap());
    let card = h.unit.arcade.as_mut().unwrap();
    set_base(card, 2, 0x300);
    set_increment(card, 2, 1);
    card.write(port(2) + 9, 0x11);
    h.write(0x42 * 0x2000 + 0x1234, 0xC1, 0); // bank $42 = port 2, the in-bank offset ignored
    h.write(0x42 * 0x2000, 0xC2, 0);
    let card = h.unit.arcade.as_mut().unwrap();
    assert!(card.ram[0x300] == 0xC1 && card.ram[0x301] == 0xC2);
    set_base(card, 2, 0x300);
    assert_eq!(h.read(0x42 * 0x2000 + 0x1FFF, 0), 0xC1);
}

#[test]
fn shift_unit_value_register_and_shifts() {
    let mut card = ArcadeCard::new();
    for (i, b) in [0x78, 0x56, 0x34, 0x12].into_iter().enumerate() {
        card.write(AC + 0xE0 + i as u32, b);
    }
    assert_eq!(value(&mut card), [0x78, 0x56, 0x34, 0x12]);
    card.write(AC + 0xE4, 0x04); // left by 4
    assert_eq!(value(&mut card), [0x80, 0x67, 0x45, 0x23]);
    assert_eq!(card.read(AC + 0xE4), 4);
    card.write(AC + 0xE4, 0x0C); // bit 3: right by 16 - 12 = 4
    assert_eq!(value(&mut card), [0x78, 0x56, 0x34, 0x02]);
    card.write(AC + 0xE4, 0x08); // right by 8
    assert_eq!(value(&mut card), [0x56, 0x34, 0x02, 0x00]);
    card.write(AC + 0xE4, 0x0F); // right by 1
    assert_eq!(card.read(AC + 0xE0), 0x2B);
    card.write(AC + 0xE4, 0x00); // no shift
    assert_eq!(card.read(AC + 0xE0), 0x2B);
}

#[test]
fn left_shift_drops_bits_off_the_top_of_32() {
    let mut card = ArcadeCard::new();
    card.write(AC + 0xE3, 0x81);
    card.write(AC + 0xE4, 0x01);
    assert_eq!(value(&mut card), [0, 0, 0, 0x02]);
}

#[test]
fn rotate_unit_wraps_bits() {
    let mut card = ArcadeCard::new();
    card.write(AC + 0xE3, 0x81);
    card.write(AC + 0xE0, 0x01);
    card.write(AC + 0xE5, 0x01); // rotate left by 1
    assert_eq!(value(&mut card), [0x03, 0, 0, 0x02]);
    assert_eq!(card.read(AC + 0xE5), 1);
    card.write(AC + 0xE5, 0x0F); // rotate right by 1: back
    assert_eq!(value(&mut card), [0x01, 0, 0, 0x81]);
}

#[test]
fn switches_for_the_emulator_only_choices() {
    let mut card = ArcadeCard::with(ArcadeSwitches {
        unmapped_read: 0x00,
        shift_read_mask: 0xFF,
        trigger_reg_read: 0xFF,
        ..ArcadeSwitches::default()
    });
    card.write(AC + 0xE4, 0x13);
    assert!(card.read(AC + 0xE4) == 0x13 && ArcadeCard::new().read(AC + 0xE4) == 0);
    assert!(card.read(AC + 0x90) == 0 && card.read(port(0) + 0x0A) == 0xFF);
    // the power-on fill (the reference's POWER_ON_RAM_BYTE)
    let mut filled = ArcadeCard::with(ArcadeSwitches {
        power_on_ram_byte: 0xA5,
        ..ArcadeSwitches::default()
    });
    assert_eq!(filled.read(port(0)), 0xA5);
    filled.write(port(0), 1);
    filled.reset(ResetKind::PowerOn);
    assert_eq!(filled.read(port(0)), 0xA5);
}

#[test]
fn power_on_clears_ram_and_registers_a_warm_reset_keeps_registers() {
    let mut card = ArcadeCard::new();
    card.write(port(0), 9);
    set_base(&mut card, 0, 0x55);
    card.write(AC + 0xE0, 0x12);
    card.reset(ResetKind::Button);
    assert!(card.ram[0] == 9 && card.read(port(0) + 2) == 0x55 && card.read(AC + 0xE0) == 0x12);
    card.reset(ResetKind::PowerOn);
    assert!(card.ram[0] == 0 && card.read(port(0) + 2) == 0 && card.read(AC + 0xE0) == 0);

    let mut other = ArcadeCard::with(ArcadeSwitches {
        reset_clears_registers: true,
        ..ArcadeSwitches::default()
    });
    set_base(&mut other, 0, 0x55);
    other.reset(ResetKind::Button);
    assert_eq!(other.read(port(0) + 2), 0);
}

#[test]
fn the_board_wires_the_card_through_the_unit_and_without_it_nothing_answers() {
    let mut cd = PcEngineCd::new(&card_image(256 * 1024), None);
    cd.arcade_card = true;
    let mut cpu = ScriptedCpu::new(cd.build_board().unwrap(), false);
    assert_eq!(cpu.read(0x1FFAFF), 0x51);
    cpu.write(0x1FFA00 + 2, 0x10); // port 0 base = 0x10
    cpu.write(0x40 * 0x2000, 0x6C); // bank $40
    assert_eq!(arcade(&mut cpu.board).unwrap().ram[0x10], 0x6C);
    assert_eq!(cpu.read(0x1FFA00), 0x6C);
    cpu.read(0x1FF800); // the CD unit's own page still answers
    let bare = PcEngineCd::new(&card_image(256 * 1024), None);
    let mut cpu2 = ScriptedCpu::new(bare.build_board().unwrap(), false);
    assert!(arcade(&mut cpu2.board).is_none());
    assert_ne!(cpu2.read(0x1FFAFF), 0x51);
    assert_eq!(cpu2.read(0x40 * 0x2000), 0x40 % 32); // the System Card's ROM mirror, as before
}
