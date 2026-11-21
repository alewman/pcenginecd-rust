// SPDX-License-Identifier: MIT
//! The CD machine through the base board's sockets, with a scripted CPU and a synthetic System
//! Card (nothing of Hudson's code is in this repository; the real card is only used by
//! scripts/). The reference's tests/test_board.py.

mod common;

use common::{card_image, disc_data_only};
use pcengine_rust::board::{Board, BoardOptions};
use pcengine_rust::fake_devices::ScriptedCpu;
use pcengine_rust::sockets::PortDevice;
use pcenginecd_rust::board::{card, unit, PcEngineCd};
use pcenginecd_rust::syscard::SystemCard;

const HW: u32 = 0x1FE000;
const IO_PORT: u32 = HW + 0x1000;
const CD: u32 = HW + 0x1800;

fn machine_of(size: usize, region_us: bool) -> ScriptedCpu {
    let mut cd = PcEngineCd::new(&card_image(size), Some(disc_data_only(40)));
    cd.region_us = region_us;
    ScriptedCpu::new(cd.build_board().unwrap(), false)
}

fn machine() -> ScriptedCpu {
    machine_of(256 * 1024, false)
}

#[test]
fn io_port_bit7_follows_the_unit_not_the_slot() {
    let mut cpu = machine();
    assert_eq!(cpu.read(IO_PORT) & 0x80, 0); // pcetech 4: 0 = a CD unit attached
    let alone = SystemCard::new(&card_image(256 * 1024), Some(false), false).unwrap();
    let board = Board::new(BoardOptions {
        card: Some(Box::new(alone)),
        ..BoardOptions::default()
    })
    .unwrap();
    let mut plain = ScriptedCpu::new(board, false);
    assert_ne!(plain.read(IO_PORT) & 0x80, 0); // the card is in the slot, but no unit on the port
}

#[test]
fn system_card_rom_mirrors_through_the_card_window() {
    let mut cpu = machine();
    assert!(cpu.read(0x000000) == 0 && cpu.read(0x002000) == 1);
    assert_eq!(cpu.read(0x040000), 0); // 256 KiB repeats at $40000
    assert_eq!(cpu.read(0x042000), 1);
}

#[test]
fn super_system_card_ram_at_banks_68_to_7f_overrides_the_rom() {
    let mut cpu = machine();
    assert_eq!(cpu.read(0x68 * 0x2000), 0); // a fresh, zeroed RAM (not the ROM's mirror)
    cpu.write(0x68 * 0x2000, 0xA5);
    cpu.write(0x7F * 0x2000 + 0x1FFF, 0x5A);
    assert!(cpu.read(0x68 * 0x2000) == 0xA5 && cpu.read(0x7F * 0x2000 + 0x1FFF) == 0x5A);
    assert_eq!(cpu.read(0x67 * 0x2000), 0x67 % 32); // bank $67 is still ROM
    assert_eq!(cpu.read(0x80 * 0x2000), 0); // $80 is CD RAM, not the card
    let ram = card(&mut cpu.board).unwrap().ram.as_ref().unwrap();
    assert_eq!(ram.len(), 192 * 1024);
}

#[test]
fn the_original_system_card_has_no_extra_ram_and_no_id_registers() {
    let mut cpu = machine_of(128 * 1024, false);
    let system_card = card(&mut cpu.board).unwrap();
    assert!(!system_card.super_system_card && system_card.ram.is_none());
    cpu.write(0x68 * 0x2000, 0xA5);
    assert_eq!(cpu.read(0x68 * 0x2000), 0x68 % 16); // still the (mirrored) ROM
    assert_eq!(cpu.read(CD + 0xC1), 0xFF);
}

#[test]
fn super_system_card_id_registers_for_japan_and_the_us() {
    let mut cpu = machine();
    let ids: Vec<u8> = (0..8).map(|i| cpu.read(CD + 0xC0 + i)).collect();
    assert_eq!(ids, [0, 0xAA, 0x55, 0, 0, 0xAA, 0x55, 3]);
    let mut cpu2 = machine_of(256 * 1024, true);
    assert_eq!([5, 6].map(|i| cpu2.read(CD + 0xC0 + i)), [0x55, 0xAA]);
}

#[test]
fn cd_ram_and_bram_are_on_the_bus_at_the_documented_banks() {
    let mut cpu = machine();
    cpu.write(0x100000, 0x77);
    cpu.write(0x10FFFF, 0x78);
    assert!(unit(&mut cpu.board).unwrap().ram[0] == 0x77 && cpu.read(0x10FFFF) == 0x78);
    assert_eq!(cpu.read(0x1EE000), 0xFF); // locked
    cpu.write(CD + 7, 0x80);
    assert_eq!(cpu.read(0x1EE000), 0x48); // the formatted header
    assert_eq!(cpu.read(0x1EE800), 0xFF); // past 2 KiB: open bus
    assert_eq!(cpu.read(0x110000), 0xFF);
}

#[test]
fn other_hardware_pages_are_untouched() {
    let mut cpu = machine();
    assert!(cpu.read(HW + 0x1900) == 0xFF && cpu.read(HW + 0x1A00) == 0xFF); // no Arcade Card yet
    cpu.write(HW + 0x400 + 2, 0); // the VCE is still the base board's
}

#[test]
fn the_clock_reaches_the_unit_without_a_register_access_and_raises_irq2() {
    let mut cpu = machine();
    cpu.write(CD + 2, 0x20); // enable the status/message IRQ
    cpu.write(CD, 0); // select
    cpu.board.advance(300_000);
    assert_eq!(cpu.read(CD) & 0xF8, 0xD0); // command phase
    for byte in [0, 0, 0, 0, 0, 0] {
        // TEST UNIT READY
        cpu.write(CD + 1, byte);
        cpu.write(CD + 2, 0xA0);
        cpu.board.advance(100);
        cpu.write(CD + 2, 0x20);
        cpu.board.advance(4000);
    }
    assert!(!cpu.board.lines().1); // (the reference asks its scripted CPU; the board has no CPU here)
    cpu.board.advance(2_000_000); // the spin-up passes with no register access at all
    assert!(cpu.board.lines().1 && cpu.board.irq2.sources().contains("cd"));
}

#[test]
fn cdda_is_on_the_audio_source_list() {
    let mut cpu = machine();
    let unit = unit(&mut cpu.board).unwrap();
    let sources = unit.audio_sources(); // mixed after the PSG: CD-DA, then ADPCM
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].rate_hz(), 44100.0);
}

#[test]
fn a_system_with_no_disc_answers_not_ready_through_the_bus() {
    let mut board = PcEngineCd::new(&card_image(256 * 1024), None)
        .build_board()
        .unwrap();
    let unit = unit(&mut board).unwrap();
    assert!(unit.disc().is_none() && unit.present());
}
