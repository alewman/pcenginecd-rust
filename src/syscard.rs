// SPDX-License-Identifier: MIT
//! The System Card: a HuCard holding the BIOS, and on a Super System Card 192 KiB more RAM
//! (`syscard.py`).
//!
//! [hw: pcetech 8, 8.1] The Super System Card (v3.0) maps 192 KiB of RAM at banks `$68-$7F` and
//! answers identification registers `$18C1-$18C6` (and `$18C7`) in the CD unit's register page;
//! with the unit's own 64 KiB at `$80-$87` that is 256 KiB. The BIOS tests `$18C5 == $55 &&
//! $18C6 == $AA` (US) or `$18C1 == $AA && $18C2 == $55` with `$18C3` as a version (Japan) and
//! refuses to run without them ("Dragon Slayer will run for a bit like this").
//!
//! How a card is recognised as a Super System Card is not documented by the hardware sources:
//! MAME makes it a separate cartridge type from its software list, Mednafen picks it by the
//! card's identity in its own database; here a 256 KiB image is a Super System Card (the System
//! Card v1/v2 images are 128 KiB) -- U37, tier 5. `super_system_card` overrides.
//!
//! The id registers' values are MAME's (`pce_scdsys_shared::register_r`: `$1`=AA, `$2`=55,
//! `$3`=00, `$5`=AA/55, `$6`=55/AA for Japan/US, `$7`=03); Mesen2 answers `$C0-$C3` = 00 AA 55
//! 03 instead (C16: they differ at `$18C3`, which only changes the X value the BIOS reports).
//!
//! Differences forced by Rust: the card holds a `RomCard` where the reference subclasses it;
//! the unit reaches `register_read` through `BoardPins::card` (the reference's unit keeps the
//! card it was built with).

use std::any::Any;

use pcengine_rust::cards::{RomCard, BANK};
use pcengine_rust::sockets::{BoardPins, Card, DeviceState, PageHandler, ResetKind};

pub const SUPER_RAM_FIRST_BANK: u32 = 0x68;
/// $68-$7F
pub const SUPER_RAM_BANKS: u32 = 24;
pub const SUPER_SYSTEM_CARD_BYTES: usize = 256 * 1024;

pub struct SystemCard {
    rom: RomCard,
    pub super_system_card: bool,
    pub region_us: bool,
    /// the Super System Card's RAM (`None` on a System Card v1/v2)
    pub ram: Option<Vec<u8>>,
}

impl SystemCard {
    /// `super_system_card`: `None` decides by the image's size (256 KiB is a Super System Card).
    pub fn new(
        data: &[u8],
        super_system_card: Option<bool>,
        region_us: bool,
    ) -> Result<SystemCard, String> {
        let rom = RomCard::new(data)?;
        let super_system_card =
            super_system_card.unwrap_or(rom.data.len() == SUPER_SYSTEM_CARD_BYTES);
        let ram = super_system_card.then(|| vec![0; SUPER_RAM_BANKS as usize * BANK]);
        Ok(SystemCard {
            rom,
            super_system_card,
            region_us,
            ram,
        })
    }

    /// The BIOS image (the copier header stripped, padded to whole banks).
    pub fn data(&self) -> &[u8] {
        &self.rom.data
    }

    fn ram_offset(address: u32) -> Option<usize> {
        let bank = (address >> 13) & 0xFF;
        (SUPER_RAM_FIRST_BANK..SUPER_RAM_FIRST_BANK + SUPER_RAM_BANKS)
            .contains(&bank)
            .then(|| address as usize - SUPER_RAM_FIRST_BANK as usize * BANK)
    }

    /// `$18C0 + offset`, called by the CD unit (it owns the page).
    pub fn register_read(&self, offset: u8) -> u8 {
        if !self.super_system_card {
            return 0xFF;
        }
        let us = self.region_us;
        match offset {
            1 => 0xAA,
            2 => 0x55,
            3 => 0x00,
            5 => {
                if us {
                    0x55
                } else {
                    0xAA
                }
            }
            6 => {
                if us {
                    0xAA
                } else {
                    0x55
                }
            }
            7 => 0x03,
            _ => 0x00,
        }
    }
}

impl DeviceState for SystemCard {
    fn save_state(&self) -> Vec<u8> {
        self.ram.clone().unwrap_or_default()
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        match self.ram.as_mut() {
            Some(ram) if ram.len() == data.len() => ram.copy_from_slice(data),
            None if data.is_empty() => {}
            _ => return Err("a state of another System Card (its RAM)".to_string()),
        }
        Ok(())
    }

    fn state_identity(&self) -> String {
        format!(
            "SystemCard super {} us {} {}",
            self.super_system_card,
            self.region_us,
            self.rom.state_identity()
        )
    }
}

impl PageHandler for SystemCard {
    fn read(&mut self, address: u32, mclk: i64, pins: &mut BoardPins) -> u8 {
        if let (Some(ram), Some(offset)) = (self.ram.as_ref(), Self::ram_offset(address)) {
            return ram[offset];
        }
        self.rom.read(address, mclk, pins)
    }

    fn write(&mut self, address: u32, value: u8, _mclk: i64, _pins: &mut BoardPins) {
        if let (Some(ram), Some(offset)) = (self.ram.as_mut(), Self::ram_offset(address)) {
            ram[offset] = value;
        }
    }
}

impl Card for SystemCard {
    fn grounds_cd(&self) -> bool {
        self.rom.grounds_cd()
    }

    fn claims(&self) -> Vec<(usize, usize)> {
        self.rom.claims()
    }

    fn reset(&mut self, kind: ResetKind, _pins: &mut BoardPins) {
        if kind == ResetKind::PowerOn {
            if let Some(ram) = self.ram.as_mut() {
                ram.fill(0); // U7: power-up RAM is arbitrary; Mednafen zeroes it
            }
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
