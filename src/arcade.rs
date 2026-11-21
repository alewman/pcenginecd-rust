// SPDX-License-Identifier: MIT
//! The Arcade Card: 2 MiB of RAM behind four address ports, and a 32-bit shift/rotate unit
//! (`arcade.py`).
//!
//! An Arcade Card (Hudson, 1994) sits on the same EXT BUS as the CD unit; here it is a component
//! of `CdUnit`, which claims its pages (one add-on: CD-ROM² + Arcade Card). It has no clock:
//! every access is immediate, so it takes no part in `catch_up`.
//!
//! Sources, and what is tier what. **No hardware documentation of the card is in the
//! reference's `reference/`**: the card is not in MacDonald's `pcetech.txt`. Every behaviour
//! below is emulator tier, from the readable, permissively licensed sources: MAME
//! `bus/pce/pce_acard.cpp` (BSD-3-Clause) and ares's Arcade Card boards (ISC), facts only, no
//! code ported (the reference's docs/arcade-sources.md); Mednafen, Geargrafx and Mesen2 (GPL)
//! run as closed boxes agree on the discs (the reference's docs/m9-record.md). Where sources
//! differ the choice is a field of `ArcadeSwitches` and a U/C entry in the reference's
//! docs/m9-record.md.
//!
//! Address map [emulator tier: MAME, ares]:
//!
//! | where | what |
//! | --- | --- |
//! | banks `$40-$43` | the data of port `bank & 3` at any offset, as `$1A00 + 16 * port` |
//! | `$1A00-$1A7F` | four port groups, `port = (addr >> 4) & 3`: `$1A40-$1A7F` mirrors `$1A00` |
//! | `$1AE0-$1AE3` | the 32-bit value register, low byte first |
//! | `$1AE4` | shift: write n (4 bits) shifts the value, read returns n |
//! | `$1AE5` | rotate: write n (4 bits) rotates the value, read returns n |
//! | `$1AFE`, `$1AFF` | version `$10` and ID `$51` |
//!
//! Port registers (low nibble of the address):
//!
//! | reg | meaning |
//! | --- | --- |
//! | `$0`, `$1` | data: read or write RAM at the effective address, then auto-increment |
//! | `$2-$4` | base address, 24 bits, low byte first |
//! | `$5`, `$6` | offset, 16 bits, low byte then high byte |
//! | `$7`, `$8` | increment, 16 bits |
//! | `$9` | control (below) |
//! | `$A` | trigger write (reads 0) |
//!
//! Control bits: D0 auto-increment; D1 add the offset to the base for the access; D2 signed
//! increment (no emulator uses it); D3 signed offset; D4 the increment goes to the base (else to
//! the offset); D6-D5 when the offset is written it is added into the base: 0 never, 1 on the
//! low-byte write, 2 on the high-byte write, 3 on a write to `$A`.
//!
//! The effective address is `(base [+ offset [+ $FF0000 if signed]]) & $1FFFFF` (2 MiB).
//!
//! Differences forced by Rust: the reference's class attributes are `ArcadeSwitches` (a
//! subclass there, a value handed to `ArcadeCard::with` here).

use pcengine_rust::sockets::ResetKind;
use serde::{Deserialize, Serialize};

pub const ARCADE_RAM_BYTES: usize = 2 * 1024 * 1024;
pub const FIRST_PORT_BANK: usize = 0x40;
pub const PORT_BANKS: usize = 4;
/// banks $40-$43 at physical $080000-$087FFF (8 KiB banks)
pub const PORT_BANK_PAGES: (usize, usize) = (0x0800, 0x087F);
/// $1A00-$1AFF
pub const REGISTER_PAGE: usize = 0x1FFA;

pub const CTL_AUTO_INCREMENT: u8 = 0x01;
pub const CTL_ADD_OFFSET: u8 = 0x02;
pub const CTL_SIGNED_OFFSET: u8 = 0x08;
pub const CTL_INCREMENT_BASE: u8 = 0x10;
pub const CTL_TRIGGER_SHIFT: u8 = 5;
pub const TRIGGER_NONE: u8 = 0;
pub const TRIGGER_LOW: u8 = 1;
pub const TRIGGER_HIGH: u8 = 2;
pub const TRIGGER_REG_A: u8 = 3;

/// The emulator-only choices (the reference's `ArcadeCard` class attributes, its defaults).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArcadeSwitches {
    /// [emulator tier: MAME $FF; ares the open bus; a choice] a read of a decoded address the
    /// card has no register at ($1A0B-$1A0F per port group, $1A80-$1ADF, $1AE6-$1AFD,
    /// $1B00-$1BFF)
    pub unmapped_read: u8,
    /// [emulator tier: MAME and ares keep 4 bits] $1AE4 / $1AE5 reads
    pub shift_read_mask: u8,
    /// [emulator tier: MAME $FF, ares the open bus; 0 kept, a choice] port register $A on a
    /// read
    pub trigger_reg_read: u8,
    /// Power-up RAM: zero, a choice; the real RAM is unspecified (U7 again)
    pub power_on_ram_byte: u8,
    /// [emulator tier: MAME and ares clear on a machine reset, not the CD's warm reset; a
    /// choice] a warm reset leaves the registers alone unless this is true
    pub reset_clears_registers: bool,
}

impl Default for ArcadeSwitches {
    fn default() -> Self {
        ArcadeSwitches {
            unmapped_read: 0xFF,
            shift_read_mask: 0x0F,
            trigger_reg_read: 0x00,
            power_on_ram_byte: 0x00,
            reset_clears_registers: false,
        }
    }
}

/// The card's RAM, four ports and the shift unit.
#[derive(Serialize, Deserialize)]
pub struct ArcadeCard {
    #[serde(skip)]
    pub switches: ArcadeSwitches,
    pub ram: Vec<u8>,
    pub base: [u32; 4],
    pub offset: [u32; 4],
    pub increment: [u32; 4],
    pub control: [u8; 4],
    /// the 32-bit shift register, $1AE0-$1AE3
    pub value: u32,
    pub shift: u8,
    pub rotate: u8,
}

impl Default for ArcadeCard {
    fn default() -> Self {
        ArcadeCard::new()
    }
}

impl ArcadeCard {
    pub fn new() -> ArcadeCard {
        ArcadeCard::with(ArcadeSwitches::default())
    }

    pub fn with(switches: ArcadeSwitches) -> ArcadeCard {
        ArcadeCard {
            switches,
            ram: vec![switches.power_on_ram_byte; ARCADE_RAM_BYTES],
            base: [0; 4],
            offset: [0; 4],
            increment: [0; 4],
            control: [0; 4],
            value: 0,
            shift: 0,
            rotate: 0,
        }
    }

    // -- the socket ------------------------------------------------------------------------------

    pub fn claims(&self) -> Vec<(usize, usize)> {
        vec![PORT_BANK_PAGES, (REGISTER_PAGE, REGISTER_PAGE)]
    }

    pub fn reset(&mut self, kind: ResetKind) {
        if kind == ResetKind::PowerOn {
            self.ram.fill(self.switches.power_on_ram_byte);
            self.clear_registers();
        } else if self.switches.reset_clears_registers {
            self.clear_registers();
        }
    }

    fn clear_registers(&mut self) {
        self.base = [0; 4];
        self.offset = [0; 4];
        self.increment = [0; 4];
        self.control = [0; 4];
        self.value = 0;
        self.shift = 0;
        self.rotate = 0;
    }

    /// A physical address in the claimed pages: a bank `$40-$43` byte or a `$1Axx` register.
    pub fn read(&mut self, address: u32) -> u8 {
        if ((address >> 8) as usize) < REGISTER_PAGE {
            return self.read_data(((address >> 13) & 3) as usize);
        }
        self.read_register((address & 0xFF) as u8)
    }

    pub fn write(&mut self, address: u32, value: u8) {
        if ((address >> 8) as usize) < REGISTER_PAGE {
            self.write_data(((address >> 13) & 3) as usize, value);
        } else {
            self.write_register((address & 0xFF) as u8, value);
        }
    }

    // -- the ports -------------------------------------------------------------------------------

    pub fn effective_address(&self, port: usize) -> usize {
        let mut address = self.base[port];
        let control = self.control[port];
        if control & CTL_ADD_OFFSET != 0 {
            address += self.offset[port];
            if control & CTL_SIGNED_OFFSET != 0 {
                address += 0xFF0000;
            }
        }
        (address & 0x1FFFFF) as usize
    }

    fn auto_increment(&mut self, port: usize) {
        let control = self.control[port];
        if control & CTL_AUTO_INCREMENT != 0 {
            if control & CTL_INCREMENT_BASE != 0 {
                self.base[port] = (self.base[port] + self.increment[port]) & 0xFFFFFF;
            } else {
                self.offset[port] = (self.offset[port] + self.increment[port]) & 0xFFFF;
            }
        }
    }

    fn add_offset_to_base(&mut self, port: usize) {
        let mut total = self.base[port] + self.offset[port];
        if self.control[port] & CTL_SIGNED_OFFSET != 0 {
            total += 0xFF0000;
        }
        self.base[port] = total & 0xFFFFFF;
    }

    pub fn read_data(&mut self, port: usize) -> u8 {
        let value = self.ram[self.effective_address(port)];
        self.auto_increment(port);
        value
    }

    pub fn write_data(&mut self, port: usize, value: u8) {
        let address = self.effective_address(port);
        self.ram[address] = value;
        self.auto_increment(port);
    }

    fn trigger(&self, port: usize) -> u8 {
        (self.control[port] >> CTL_TRIGGER_SHIFT) & 3
    }

    // -- the registers ---------------------------------------------------------------------------

    /// `reg` is the low byte of the address within `$1A00-$1AFF`.
    pub fn read_register(&mut self, reg: u8) -> u8 {
        if reg < 0x80 {
            let port = ((reg >> 4) & 3) as usize;
            let n = (reg & 0x0F) as u32;
            return match n {
                0 | 1 => self.read_data(port),
                2..=4 => (self.base[port] >> (8 * (n - 2))) as u8,
                5 | 6 => (self.offset[port] >> (8 * (n - 5))) as u8,
                7 | 8 => (self.increment[port] >> (8 * (n - 7))) as u8,
                9 => self.control[port],
                0x0A => self.switches.trigger_reg_read,
                _ => self.switches.unmapped_read,
            };
        }
        if reg < 0xE0 {
            return self.switches.unmapped_read;
        }
        let n = (reg & 0x1F) as u32;
        match n {
            0..=3 => (self.value >> (8 * n)) as u8,
            4 => self.shift & self.switches.shift_read_mask,
            5 => self.rotate & self.switches.shift_read_mask,
            0x1C | 0x1D => 0x00,
            0x1E => 0x10, // version [emulator tier: MAME, ares]
            0x1F => 0x51, // ID
            _ => self.switches.unmapped_read,
        }
    }

    pub fn write_register(&mut self, reg: u8, value: u8) {
        let wide = value as u32;
        if reg < 0x80 {
            let port = ((reg >> 4) & 3) as usize;
            let n = (reg & 0x0F) as u32;
            match n {
                0 | 1 => self.write_data(port, value),
                2..=4 => {
                    let shift = 8 * (n - 2);
                    self.base[port] = (self.base[port] & !(0xFF << shift)) | (wide << shift);
                }
                5 => {
                    self.offset[port] = (self.offset[port] & 0xFF00) | wide;
                    if self.trigger(port) == TRIGGER_LOW {
                        self.add_offset_to_base(port);
                    }
                }
                6 => {
                    self.offset[port] = (self.offset[port] & 0x00FF) | (wide << 8);
                    if self.trigger(port) == TRIGGER_HIGH {
                        self.add_offset_to_base(port);
                    }
                }
                7 => self.increment[port] = (self.increment[port] & 0xFF00) | wide,
                8 => self.increment[port] = (self.increment[port] & 0x00FF) | (wide << 8),
                9 => self.control[port] = value & 0x7F,
                0x0A if self.trigger(port) == TRIGGER_REG_A => self.add_offset_to_base(port),
                _ => {}
            }
            return;
        }
        if reg < 0xE0 {
            return;
        }
        let n = (reg & 0x1F) as u32;
        match n {
            0..=3 => {
                let shift = 8 * n;
                self.value = (self.value & !(0xFF << shift)) | (wide << shift);
            }
            4 => {
                self.shift = value;
                let count = wide & 0x0F;
                if count != 0 {
                    if count & 8 != 0 {
                        self.value >>= 16 - count; // right by 8..1 for 8..15
                    } else {
                        self.value <<= count;
                    }
                }
            }
            5 => {
                self.rotate = value;
                let count = wide & 0x0F;
                if count != 0 {
                    self.value = if count & 8 != 0 {
                        self.value.rotate_right(16 - count)
                    } else {
                        self.value.rotate_left(count)
                    };
                }
            }
            _ => {}
        }
    }
}
