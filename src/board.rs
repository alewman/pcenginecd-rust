// SPDX-License-Identifier: MIT
//! The CD-ROM² / Super CD-ROM² machine: the base board plus a System Card and a CD unit
//! (`board.py`).
//!
//! Composition through the base board's sockets (the reference's docs/start-here.md):
//!
//! - socket 1 (HuCard slot): the System Card (`SystemCard`), which also holds the Super System
//!   Card's RAM at `$68-$7F` and its id registers;
//! - socket 2 (expansion port): `CdUnit` claims CD RAM (`$80-$87`), BRAM (`$F7`) and
//!   `$1800-$18FF`, and pulls the I/O port's bit 7 low (`present`);
//! - the Arcade Card (`arcade_card`) is a component of the unit: it adds banks `$40-$43` and
//!   `$1A00-$1AFF` to the unit's claims (arcade.rs);
//! - socket 6 (interrupts): the unit's bit "cd" on the board's IRQ2 line;
//! - socket 9 (audio): the CD-DA player and the ADPCM unit are the port device's audio
//!   sources, mixed after the PSG;
//! - socket 5 (clock domains): the board catches the unit up to its clock **every advance**.
//!
//! Differences forced by Rust: the reference's `PcEngineCd` object keeps `card`, `unit`,
//! `arcade` and `board` side by side; here the board owns the card and the unit, so
//! `PcEngineCd` is the description (the reference's constructor arguments, with its defaults,
//! and every named switch) and `build` hands back the base board's `System`; `unit`, `card`
//! and `arcade` reach the devices inside a board. The reference's `**board_kwargs` is `board`,
//! a `BoardOptions` whose card and port are replaced.

use pcengine_rust::board::{Board, BoardOptions};
use pcengine_rust::system::System;

use crate::adpcm::AdpcmSwitches;
use crate::arcade::{ArcadeCard, ArcadeSwitches};
use crate::cdda::Fader;
use crate::disc::Disc;
use crate::scsi::ScsiSwitches;
use crate::syscard::SystemCard;
use crate::unit::{CdUnit, UnitSwitches};

/// Every named switch of the add-on, with the reference's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CdSwitches {
    pub scsi: ScsiSwitches,
    pub unit: UnitSwitches,
    /// the fade times and `pcetech_fade` (the reference's `CddaPlayer` attributes)
    pub fader: Fader,
    /// the reference's `CddaPlayer.SUBQ_STATUS`: playing, paused, stopped, seeking
    pub subq_status: Option<[u8; 4]>,
    pub adpcm: AdpcmSwitches,
    pub arcade: ArcadeSwitches,
}

/// A CD-ROM² system. `system_card` is the BIOS image (bring your own).
pub struct PcEngineCd {
    pub system_card: Vec<u8>,
    pub disc: Option<Disc>,
    /// `None`: a 256 KiB image is a Super System Card (U37)
    pub super_system_card: Option<bool>,
    pub region_us: bool,
    /// the backup RAM's content (`None`: formatted and empty, U36)
    pub bram: Option<Vec<u8>>,
    pub arcade_card: bool,
    pub switches: CdSwitches,
    /// The base board's options (machine row, pad, switches); `card` and `port` are replaced.
    pub board: BoardOptions,
}

impl PcEngineCd {
    pub fn new(system_card: &[u8], disc: Option<Disc>) -> PcEngineCd {
        PcEngineCd {
            system_card: system_card.to_vec(),
            disc,
            super_system_card: None,
            region_us: false,
            bram: None,
            arcade_card: false,
            switches: CdSwitches::default(),
            board: BoardOptions::default(),
        }
    }

    /// The unit as this description builds it (the tests drive it without a board).
    pub fn make_unit(
        disc: Option<Disc>,
        bram: Option<&[u8]>,
        arcade_card: bool,
        switches: &CdSwitches,
    ) -> Result<CdUnit, String> {
        let arcade = arcade_card.then(|| ArcadeCard::with(switches.arcade));
        let mut unit = CdUnit::with(disc, bram, arcade)?;
        unit.switches = switches.unit;
        unit.bus.switches = switches.scsi;
        unit.adpcm.switches = switches.adpcm;
        unit.bus.cdda.fader.set(switches.fader);
        if let Some(codes) = switches.subq_status {
            unit.bus.cdda.subq_status = codes;
        }
        Ok(unit)
    }

    /// The board with the System Card in the slot and the unit on the port.
    pub fn build_board(self) -> Result<Board, String> {
        let card = SystemCard::new(&self.system_card, self.super_system_card, self.region_us)?;
        let unit = PcEngineCd::make_unit(
            self.disc,
            self.bram.as_deref(),
            self.arcade_card,
            &self.switches,
        )?;
        Board::new(BoardOptions {
            card: Some(Box::new(card)),
            port: Some(Box::new(unit)),
            ..self.board
        })
    }

    /// The real HuC6280 core on this board.
    pub fn build(self) -> Result<System, String> {
        Ok(System::new(self.build_board()?))
    }
}

/// The CD unit on a board's expansion port.
pub fn unit(board: &mut Board) -> Option<&mut CdUnit> {
    board.port.as_mut()?.as_any_mut().downcast_mut()
}

/// The System Card in a board's slot.
pub fn card(board: &mut Board) -> Option<&mut SystemCard> {
    board.slot.card.as_mut()?.as_any_mut().downcast_mut()
}

/// The Arcade Card, if the board's unit has one fitted.
pub fn arcade(board: &mut Board) -> Option<&mut ArcadeCard> {
    unit(board)?.arcade.as_mut()
}
