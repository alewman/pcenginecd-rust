// SPDX-License-Identifier: MIT
//! The CD unit: the expansion-port device that owns `$1800-$18FF`, CD RAM and the backup RAM
//! (`unit.py`).
//!
//! It plugs into the base board's socket 2 (`PortDevice`): `claims()` lists the pages, every
//! access carries its master-clock time, `catch_up(mclk)` runs the drive up to it (the board
//! calls it at every advance, base issue 0002; a register access also brings the drive's clock
//! to its own time), `present` pulls the I/O port's bit 7 low, and IRQ2 is the board's wired-OR
//! line with this unit's own bit, "cd".
//!
//! Registers [hw: pcetech 8 for the names; the bit-level behaviour is emulator-derived, tier 5,
//! and each is marked]:
//!
//! | addr | read | write |
//! | --- | --- | --- |
//! | `$1800` | SCSI status: D3 I/O, D4 C/D, D5 MSG, D6 REQ, D7 BSY | select the drive (SEL pulse) |
//! | `$1801` | the SCSI data bus | the data bus (command bytes) |
//! | `$1802` | IRQ enable mask (D6-D0) and ACK (D7) | the same: D7 sets ACK, D6-D0 the mask |
//! | `$1803` | active IRQ bits, D1 = left/right flag; **locks BRAM** | -- |
//! | `$1804` | the reset register | D1 = SCSI reset (the BIOS's CD_RESET sets then clears it) |
//! | `$1805` | CD-DA sample, low byte | toggles left/right and latches that channel's sample |
//! | `$1806` | CD-DA sample, high byte | -- |
//! | `$1807` | subchannel byte (not modelled: 0) | D7 set **unlocks BRAM** |
//! | `$1808` | the SCSI data bus; a read of data in pulses ACK | ADPCM address low |
//! | `$1809` | -- | ADPCM address high |
//! | `$1808/9`, `$180A-$180E` | the ADPCM unit (adpcm.rs); `$180B` bit 1 = CD-to-ADPCM DMA | the same |
//! | `$180F` | the fader register | fade control |
//! | `$1A00-$1AFF`, banks `$40-$43` | the Arcade Card, when fitted (arcade.rs) | the same |
//! | `$18C0-$18C7` | the Super System Card id registers (answered by the card) | -- |
//!
//! IRQ2 = (enable mask & active bits) != 0. Active bits: 0x04 ADPCM half, 0x08 ADPCM end, 0x10
//! subchannel, 0x20 status/message-in (REQ in those phases), 0x40 data-in (REQ in data in). The
//! status/message and data-in bits are derived from the bus levels, the ADPCM bits are the
//! unit's half and end flags (adpcm.rs), the subchannel bit is U9.
//!
//! Differences forced by Rust:
//!
//! - **`BoardPins` arrives with each call** (the base board's sockets.rs), so there is no
//!   `bind`; `sync_irq` is the reference's `_bus_irq`, `_adpcm_irq` and `_update_irq`: after
//!   each call into the bus or the ADPCM unit it applies the level changes they listed, in
//!   order, and sets the unit's bit on IRQ2. A unit driven with no board behind it takes a
//!   `DetachedPins` (the reference's unbound unit).
//! - **The audio player is the bus's** (`unit.bus.cdda`; `unit.cdda()`), **the ADPCM unit is
//!   its own audio source**, and the two are what `audio_sources` hands the mixer (the
//!   reference appends them to the board's list).
//! - **The disc is shared** by the bus and the player as an `Rc<Disc>`.
//! - **The id registers** `$18C0-$18C7` are asked of `pins.card` when it is a `SystemCard`
//!   (the reference's unit keeps the card `PcEngineCd` built it with).
//! - **Save states** (`DeviceState`): everything that changes as the console runs, in one
//!   bincode value; the switches, the wiring and the drive's command log stay the unit's own.
//!   The identity names the disc by its table of contents and says whether an Arcade Card is
//!   fitted, so a machine with another disc, or without the card, refuses the state.
//! - A `bram` image longer than 2 KiB is an error (the reference's `ValueError`): `with`
//!   returns a `Result`.

use std::any::Any;
use std::rc::Rc;

use pcengine_rust::sockets::{
    AudioSource, BoardPins, DeviceState, PageHandler, PortDevice, ResetKind,
};
use serde::{Deserialize, Serialize};

use crate::adpcm::Adpcm;
use crate::arcade::{ArcadeCard, REGISTER_PAGE as ARCADE_REGISTER_PAGE};
use crate::cdda::CddaPlayer;
use crate::disc::Disc;
use crate::scsi::ScsiBus;
use crate::syscard::SystemCard;

pub const CD_RAM_BYTES: usize = 64 * 1024;
pub const BRAM_BYTES: usize = 2 * 1024;

// The pages of the 256-byte-granular decode (docs/sockets.md of the base board).
/// banks $80-$87, physical $100000-$10FFFF [hw: pcetech 8]
pub const CD_RAM_PAGES: (usize, usize) = (0x1000, 0x10FF);
/// $1EE000-$1EE7FF, the first 2 KiB of bank $F7 [hw: pcetech 8]
pub const BRAM_PAGES: (usize, usize) = (0x1EE0, 0x1EE7);
/// $1800-$18FF
pub const REGISTER_PAGE: usize = 0x1FF8;

pub const IRQ_ADPCM_HALF: u8 = 0x04;
pub const IRQ_ADPCM_END: u8 = 0x08;
pub const IRQ_SUBCODE: u8 = 0x10;
pub const IRQ_STATUS_MESSAGE: u8 = 0x20;
pub const IRQ_DATA_IN: u8 = 0x40;

/// The BRAM as the BIOS leaves it after formatting: its "HUBM" header (a fact of the BIOS's data
/// format, BM_FORMAT), the rest zero. The real RAM is battery-backed and its content at an
/// owner's first power-up is unknown (U36); a formatted-empty RAM is a choice (Mesen2's author
/// reports that unformatted RAM upsets Far East of Eden: a report, emulator tier).
pub const BRAM_FORMATTED: [u8; 8] = [0x48, 0x55, 0x42, 0x4D, 0x00, 0xA0, 0x10, 0x80];

/// The reference's `CdUnit` class attributes, with its defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitSwitches {
    /// A second write to $1805 within this many master clocks of the first is ignored.
    /// [OBSERVED: closed box, Geargrafx 57c257d through the reference's scripts/drive_probe.py:
    /// writes 225 CPU cycles apart do not toggle $1803 bit 1, 245 apart do, so the guard is in
    /// (675, 735]; Mesen2's author reports a guard from test ROMs (a report, emulator tier)]
    /// Provisional.
    pub channel_latch_guard: i64,
    /// Master clocks from REQ to the DMA taking the byte. [Mesen2's author reports that the
    /// "verificator" test ROM passes with 18 to 22 CPU cycles and fails outside (a report,
    /// emulator tier; the middle rule): 20 cycles.] The closed box (Geargrafx 57c257d, the
    /// reference's scripts/drive_probe.py) takes a byte every 90 clocks, which with the 45-clock
    /// auto-ACK would be 45: open (provisional).
    pub dma_byte_clocks: i64,
}

impl Default for UnitSwitches {
    fn default() -> Self {
        UnitSwitches {
            channel_latch_guard: 700,
            dma_byte_clocks: 20 * 3,
        }
    }
}

/// CD RAM, backup RAM, the registers, the SCSI bus and the CD-DA player.
#[derive(Serialize, Deserialize)]
pub struct CdUnit {
    #[serde(skip)]
    pub switches: UnitSwitches,
    pub bus: ScsiBus,
    pub adpcm: Adpcm,
    pub ram: Vec<u8>,
    pub bram: Vec<u8>,
    /// [hw: pcetech 8] locked at power-up
    pub bram_locked: bool,
    /// the Arcade Card, if fitted: banks $40-$43 and $1A00-$1AFF (arcade.rs)
    pub arcade: Option<ArcadeCard>,
    pub enable: u8,
    pub active: u8,
    pub reset_register: u8,
    pub right_channel: bool,
    pub sample_latch: u16,
    latch_guard_until: i64,
    next_event: Option<i64>,
    dma_at: Option<i64>,
    pub mclk: i64,
}

impl CdUnit {
    pub fn new(disc: Option<Disc>) -> CdUnit {
        CdUnit::with(disc, None, None).expect("the formatted header fits")
    }

    /// `bram`: the backup RAM's first bytes (`None`: the formatted-empty header, U36); more than
    /// 2 KiB is an error (the RAM is 2 KiB, pcetech 8; the reference's issue 0001).
    pub fn with(
        disc: Option<Disc>,
        bram: Option<&[u8]>,
        arcade: Option<ArcadeCard>,
    ) -> Result<CdUnit, String> {
        let header = bram.unwrap_or(&BRAM_FORMATTED);
        if header.len() > BRAM_BYTES {
            return Err(format!(
                "bram holds {} bytes; the backup RAM is {BRAM_BYTES}",
                header.len()
            ));
        }
        let cdda = CddaPlayer::new();
        let adpcm = Adpcm::new(Rc::clone(&cdda.fader));
        let bus = ScsiBus::new(disc.map(Rc::new), cdda);
        let mut image = vec![0; BRAM_BYTES];
        image[..header.len()].copy_from_slice(header);
        Ok(CdUnit {
            switches: UnitSwitches::default(),
            bus,
            adpcm,
            ram: vec![0; CD_RAM_BYTES],
            bram: image,
            bram_locked: true,
            arcade,
            enable: 0,
            active: 0,
            reset_register: 0,
            right_channel: false,
            sample_latch: 0,
            latch_guard_until: 0,
            next_event: None,
            dma_at: None,
            mclk: 0,
        })
    }

    pub fn disc(&self) -> Option<&Rc<Disc>> {
        self.bus.disc.as_ref()
    }

    pub fn insert(&mut self, disc: Option<Disc>) {
        let disc = disc.map(Rc::new);
        self.bus.cdda.disc = disc.clone();
        self.bus.disc = disc;
    }

    /// The CD-DA player (the reference's `unit.cdda`).
    pub fn cdda(&mut self) -> &mut CddaPlayer {
        &mut self.bus.cdda
    }

    /// The 2 KiB of backup RAM, for the host to keep between sessions.
    pub fn bram(&self) -> &[u8] {
        &self.bram
    }

    /// Put kept backup RAM back (up to 2 KiB; the rest is left as it is).
    pub fn set_bram(&mut self, data: &[u8]) {
        let count = data.len().min(BRAM_BYTES);
        self.bram[..count].copy_from_slice(&data[..count]);
    }

    /// The unit's own timers: (the CD-to-ADPCM DMA's next byte, the next event of any kind).
    /// For tests and debuggers (the reference's tests read `_dma_at` and `_next_event`).
    pub fn pending_timers(&self) -> (Option<i64>, Option<i64>) {
        (self.dma_at, self.next_event)
    }

    // -- the PortDevice socket: time -------------------------------------------------------------

    /// Run the drive, the ADPCM unit and the ADPCM DMA, event by event, up to `mclk`.
    fn run(&mut self, mclk: i64) {
        while let Some(at) = self.soonest().filter(|&at| at <= mclk) {
            self.bus.run_to(at);
            self.adpcm.run(at);
            if self.dma_at == Some(at) {
                self.dma_at = None;
                self.dma_step(at);
            }
            self.dma_arm();
        }
        self.bus.run_to(mclk);
        self.adpcm.run(mclk);
        self.dma_arm();
        self.next_event = self.soonest();
    }

    fn soonest(&self) -> Option<i64> {
        [self.bus.next_event(), self.dma_at, self.adpcm.next_event()]
            .into_iter()
            .flatten()
            .min()
    }

    /// The reference's `catch_up`: the board's call, at every advance: do whatever falls due.
    /// While nothing is due the drive's clock is left where it is (a cheap early-out);
    /// `run_now` brings it to the present before a register access.
    fn run_if_due(&mut self, mclk: i64, pins: &mut BoardPins) {
        if self.next_event.is_none_or(|at| mclk >= at) {
            self.run(mclk);
            self.sync_irq(pins);
        }
        self.mclk = mclk;
    }

    /// The reference's `_run_now`: bring the drive, the ADPCM unit and the DMA to `mclk` even
    /// when no event is due, so a timer that a register access starts counts from that access
    /// (issue 0001 item 1: before, the bus's clock stood at the last event while another one
    /// waited in the future, and a SEL written a second after CD audio started got its command
    /// phase at once).
    fn run_now(&mut self, mclk: i64, pins: &mut BoardPins) {
        self.run(mclk);
        self.sync_irq(pins);
        self.mclk = mclk;
    }

    // -- CD to ADPCM DMA (AD_TRANS): `$180B` bits 1/0 [hw: pcetech 8, "no DMA to RAM ... only
    // one sector at a time into ADPCM RAM"; the BIOS sets bit 1, polls `$180C` bit 2 and `$1803`
    // bit 5]. One byte at a time: the drive's data is taken while REQ is up in data in, then ACK
    // pulses.

    fn dma_wanted(&self) -> bool {
        let bus = &self.bus;
        self.adpcm.dma & 0x03 != 0 && bus.req && bus.io() && !bus.cd() && !bus.ack
    }

    fn dma_arm(&mut self) {
        if self.dma_at.is_none() && self.dma_wanted() {
            self.dma_at = Some(self.bus.now + self.switches.dma_byte_clocks);
        }
    }

    fn dma_step(&mut self, at: i64) {
        if !self.dma_wanted() {
            return;
        }
        if !self.adpcm.dma_write(self.bus.read_data(), at) {
            self.dma_at = Some(at + 3); // the RAM is still taking the last byte: ask again
            return;
        }
        self.bus.pulse_ack();
        if self.bus.data_waiting() == 0 {
            // the sector is through (bit 1, the request, stays until cleared)
            self.adpcm.dma &= !0x01;
        }
    }

    // -- IRQ2 ------------------------------------------------------------------------------------

    /// The reference's `_bus_irq`, `_adpcm_irq` and `_update_irq`: apply the level changes the
    /// bus and the ADPCM unit listed, then set this unit's bit on the board's IRQ2 line.
    fn sync_irq(&mut self, pins: &mut BoardPins) {
        for (mask, level) in self.bus.irq_changes.drain(..) {
            if level {
                if self.reset_register & 0x02 == 0 {
                    // no IRQ while the drive is held in reset
                    self.active |= mask;
                }
            } else {
                self.active &= !mask;
            }
        }
        // The ADPCM half and end flags: IRQ2 sources `$04` and `$08` (the drive's reset does
        // not hold them off).
        for (mask, level) in self.adpcm.irq_changes.drain(..) {
            if level {
                self.active |= mask;
            } else {
                self.active &= !mask;
            }
        }
        pins.irq2.set("cd", self.enable & self.active != 0);
    }

    // -- the registers ---------------------------------------------------------------------------

    fn read_register(&mut self, reg: u8, mclk: i64, pins: &mut BoardPins) -> u8 {
        match reg {
            0x00 => self.bus.status_bits(),
            0x01 => self.bus.read_data(),
            0x02 => self.enable | if self.bus.ack { 0x80 } else { 0 },
            0x03 => {
                self.bram_locked = true; // [hw: pcetech 8] a read of $1803 locks BRAM
                self.active | if self.right_channel { 0 } else { 0x02 }
            }
            0x04 => self.reset_register,
            0x05 => self.sample_latch as u8,
            0x06 => (self.sample_latch >> 8) as u8,
            0x07 => 0, // the subchannel byte (U9)
            0x08 => {
                let value = self.bus.read_data();
                if self.bus.req && self.bus.io() && !self.bus.cd() {
                    self.bus.pulse_ack();
                    self.dma_arm();
                    self.next_event = self.soonest();
                }
                value
            }
            0x09..=0x0E => {
                let value = self.adpcm.read(reg, mclk);
                self.next_event = self.soonest();
                value
            }
            0x0F => self.bus.cdda.fade_register(),
            0xC0..=0xC7 => {
                let card = pins
                    .card
                    .as_mut()
                    .and_then(|card| card.as_any_mut().downcast_mut::<SystemCard>());
                card.map_or(0xFF, |card| card.register_read(reg & 7))
            }
            _ => 0xFF,
        }
    }

    fn write_register(&mut self, reg: u8, mut value: u8, mclk: i64, pins: &mut BoardPins) {
        match reg {
            0x00 => self.bus.select(),
            0x01 => self.bus.write_data(value),
            0x02 => {
                self.bus.set_ack(value & 0x80 != 0);
                self.enable = value & 0x7F;
            }
            0x04 => {
                self.reset_register = value & 0x0F;
                let reset = value & 0x02 != 0;
                self.bus.set_reset(reset);
                self.sync_irq(pins);
                if reset {
                    self.enable &= 0x8F; // the drive's three IRQ enables drop
                    self.active &= 0x8F;
                }
            }
            0x05 => {
                if mclk >= self.latch_guard_until {
                    self.right_channel = !self.right_channel;
                    let (left, right) = self.bus.cdda.latest_sample();
                    self.sample_latch = if self.right_channel { right } else { left } as u16;
                    self.latch_guard_until = mclk + self.switches.channel_latch_guard;
                }
            }
            0x07 => self.bram_locked = value & 0x80 == 0,
            0x08..=0x0E => {
                if reg == 0x0B && self.bus.data_waiting() == 0 {
                    // bit 0 cannot be set when no data block is waiting [OBSERVED: closed box,
                    // Geargrafx 57c257d reads $03 written back as $02; provisional]
                    value &= !0x01;
                }
                self.adpcm.write(reg, value, mclk);
            }
            0x0F => self.bus.cdda.write_fade(value, mclk),
            _ => {}
        }
    }

    // -- save states -----------------------------------------------------------------------------

    fn disc_identity(&self) -> String {
        self.disc()
            .map_or("none".to_string(), |disc| disc.state_identity())
    }
}

impl PageHandler for CdUnit {
    fn read(&mut self, address: u32, mclk: i64, pins: &mut BoardPins) -> u8 {
        let page = (address >> 8) as usize;
        if let Some(arcade) = self.arcade.as_mut() {
            if page < 0x1000 || page == ARCADE_REGISTER_PAGE {
                return arcade.read(address); // the card has no clock
            }
        }
        if (CD_RAM_PAGES.0..=CD_RAM_PAGES.1).contains(&page) {
            return self.ram[(address & 0xFFFF) as usize]; // memory: no clock either
        }
        if (BRAM_PAGES.0..=BRAM_PAGES.1).contains(&page) {
            return if self.bram_locked {
                0xFF
            } else {
                self.bram[(address & 0x7FF) as usize]
            };
        }
        self.run_now(mclk, pins);
        let value = self.read_register((address & 0xFF) as u8, mclk, pins);
        self.sync_irq(pins);
        value
    }

    fn write(&mut self, address: u32, value: u8, mclk: i64, pins: &mut BoardPins) {
        let page = (address >> 8) as usize;
        if let Some(arcade) = self.arcade.as_mut() {
            if page < 0x1000 || page == ARCADE_REGISTER_PAGE {
                arcade.write(address, value);
                return;
            }
        }
        if (CD_RAM_PAGES.0..=CD_RAM_PAGES.1).contains(&page) {
            self.ram[(address & 0xFFFF) as usize] = value;
            return;
        }
        if (BRAM_PAGES.0..=BRAM_PAGES.1).contains(&page) {
            if !self.bram_locked {
                self.bram[(address & 0x7FF) as usize] = value;
            }
            return;
        }
        self.run_now(mclk, pins);
        self.write_register((address & 0xFF) as u8, value, mclk, pins);
        self.dma_arm();
        self.next_event = self.soonest();
        self.sync_irq(pins);
    }
}

impl PortDevice for CdUnit {
    /// K7 low: the I/O port reports "CD unit attached" (C6 polarity)
    fn present(&self) -> bool {
        true
    }

    fn claims(&self) -> Vec<(usize, usize)> {
        let mut pages = vec![CD_RAM_PAGES, BRAM_PAGES, (REGISTER_PAGE, REGISTER_PAGE)];
        if let Some(arcade) = self.arcade.as_ref() {
            pages.extend(arcade.claims());
        }
        pages
    }

    fn catch_up(&mut self, mclk: i64, pins: &mut BoardPins) {
        self.run_if_due(mclk, pins);
    }

    fn reset(&mut self, kind: ResetKind, pins: &mut BoardPins) {
        if let Some(arcade) = self.arcade.as_mut() {
            arcade.reset(kind);
        }
        if kind == ResetKind::PowerOn {
            self.bram_locked = true;
            self.bus.rst = false; // the reset register is cleared below: RST lets go
            self.bus.reset();
            self.adpcm.power_on(self.mclk);
            self.sync_irq(pins);
            self.enable = 0;
            self.active = 0;
            self.reset_register = 0;
            self.right_channel = false;
            self.sample_latch = 0;
            // issue 0001 item 3: the unit's own timers go too
            self.latch_guard_until = 0;
            self.dma_at = None;
            self.next_event = self.soonest();
            self.sync_irq(pins);
        }
    }

    /// Socket 9: CD-DA, then ADPCM (the order the reference appends them to the board's list).
    fn audio_sources(&mut self) -> Vec<&mut dyn AudioSource> {
        vec![&mut self.bus.cdda, &mut self.adpcm]
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl DeviceState for CdUnit {
    fn save_state(&self) -> Vec<u8> {
        let fader = self.bus.cdda.fader.get();
        let mut out = bincode::serialize(&(fader.fade_register, fader.fade_start))
            .expect("a fader serialises");
        out.extend(bincode::serialize(self).expect("a unit serialises"));
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let bad = |error: bincode::Error| format!("a CD unit's state: {error}");
        let fade: (u8, i64) = bincode::deserialize(data).map_err(bad)?;
        let head = bincode::serialized_size(&fade).map_err(bad)? as usize;
        let saved: CdUnit = bincode::deserialize(&data[head..]).map_err(bad)?;
        if saved.arcade.is_some() != self.arcade.is_some() {
            return Err("a state of a machine with another Arcade Card option".to_string());
        }
        // the bus, the player and the ADPCM unit keep their wiring (the disc, the shared fader)
        // and every device its switches
        let CdUnit {
            switches: _,
            bus,
            adpcm,
            ram,
            bram,
            bram_locked,
            arcade,
            enable,
            active,
            reset_register,
            right_channel,
            sample_latch,
            latch_guard_until,
            next_event,
            dma_at,
            mclk,
        } = saved;
        self.bus.restore(bus);
        self.adpcm.restore(adpcm);
        if let (Some(ours), Some(mut theirs)) = (self.arcade.as_mut(), arcade) {
            theirs.switches = ours.switches;
            *ours = theirs;
        }
        self.ram = ram;
        self.bram = bram;
        self.bram_locked = bram_locked;
        self.enable = enable;
        self.active = active;
        self.reset_register = reset_register;
        self.right_channel = right_channel;
        self.sample_latch = sample_latch;
        self.latch_guard_until = latch_guard_until;
        self.next_event = next_event;
        self.dma_at = dma_at;
        self.mclk = mclk;
        let mut fader = self.bus.cdda.fader.get();
        (fader.fade_register, fader.fade_start) = fade;
        self.bus.cdda.fader.set(fader);
        Ok(())
    }

    /// What the unit is and which media it holds: the disc by its table of contents (never its
    /// sectors), and whether the Arcade Card is fitted.
    fn state_identity(&self) -> String {
        format!(
            "CdUnit disc {} arcade {}",
            self.disc_identity(),
            self.arcade.is_some()
        )
    }
}
