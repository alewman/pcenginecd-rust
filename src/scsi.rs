// SPDX-License-Identifier: MIT
//! The CD drive as the interface unit sees it: a SCSI-1 target on an 8-bit bus with REQ/ACK
//! (`scsi.py`).
//!
//! Transcribed 2026-11-19 from pcenginecd-python `4abb3f5`, the reference's clean rewrite
//! (docs/restart.md); the Rust module it replaces was withdrawn unread. Evidence tags as in the
//! family's methodology; "closed box" means a value measured from outside a GPL emulator run as
//! a library (the reference's scripts/drive_probe.py: a probe program of its own stands in for
//! the System Card and counts 45-clock polling turns; Geargrafx libretro 57c257d), never its
//! source.
//!
//! Sources, in the order they are trusted:
//!
//! - [DOC: SCSI-1, X3.131-1986] the phases and their signals (MSG, C/D, I/O: command 010, data
//!   in 001, status 011, message in 111), the REQ/ACK handshake (the target raises REQ, the
//!   initiator raises ACK, the target drops REQ, the initiator drops ACK), selection (the
//!   initiator puts its id and the target's on the data bus and raises SEL, the target answers
//!   with BSY), the status byte (GOOD `$00`, CHECK CONDITION `$02`), COMMAND COMPLETE (`$00`),
//!   REQUEST SENSE (allocation length 0 means 4 bytes), the group-0 six-byte command block, and
//!   bus free when BSY drops.
//! - [DOC: pcetech 8] the drive is "SCSI compatible" with TEST UNIT READY, REQUEST SENSE, READ
//!   and the vendor commands D8, D9, DA, DD, DE; data bytes arrive through `$1801` (handshake)
//!   or `$1808`.
//! - [DOC: Hudson BIOS manual, Hu7 CD System BIOS Ver1.00] CD_PLAY's end modes (0 "mute play"
//!   with status at once, 1 repeat, busy "until the next command issuance", 2 busy "until
//!   playback completion", 3 status at once), CD_SEARCH (the drive is busy during the search),
//!   CD_SUBQ's status byte (`$00` playing, `$01` still, `$02` pause, `$03` not playing) and its
//!   ten bytes, CD_DINFO's four kinds (track range, lead-out MSF, a track's MSF and control, a
//!   track's LBA and control).
//! - [DOC: the System Card 3.00 disassembly in the MiSTer repository's docs/ (a Hudson
//!   document)] how the BIOS drives the bus: `$81` then a write to `$1800` to select; the
//!   handshake at `$EC0B` (ACK up, wait for REQ to drop, ACK down); after the command it returns
//!   on `$1800 & $B8` = `$98`, `$88` or `$80`; if BSY is up when it wants to select, it selects
//!   anyway, puts `$FF` on the bus and takes every REQ until BSY drops; it reads a sector
//!   through `$1808` at one byte per 26-44 CPU cycles without looking at REQ, so the auto-ACK
//!   and the next REQ fit in 26 cycles (78 clocks).
//! - [OBSERVED, closed box: Geargrafx 57c257d] the response times below, the abort's eight
//!   status bytes, the re-read after a missed sector.
//! - [emulator tier, permissive: MAME `nec/pce_cd.cpp` (BSD-3-Clause, Wilbert Pol, Angelo
//!   Salese) and ares `pce/pcd/scsi.cpp` (ISC, ares team)] facts only, no code ported: the
//!   10-byte vendor command blocks, READ TOC answer lengths 2/3/4 (MAME), the auto-ACK pulse of
//!   15 CPU cycles (MAME), a READ of zero sectors answering GOOD (both), PAUSE without audio
//!   answering CHECK CONDITION (both), an unknown opcode answered at its first byte (ares).
//!
//! Shape: one protocol step is pending at a time (`step_at` and what happens then, `step`),
//! plus the sector clock of a READ (`sector_at`) and the end of an auto-ACK pulse
//! (`ack_off_at`). The CD-DA player's events run through `run_to` too, so `next_event` covers
//! them.
//!
//! Differences forced by Rust:
//!
//! - **The timings are fields of `ScsiSwitches`** (`bus.switches`), the reference's class
//!   attributes with its defaults, so a gate case can move one (`--switch scsi.NAME=VALUE`).
//! - **The step is an enum** (`Step`), where the reference keeps the name of the method and
//!   calls it with `getattr`.
//! - **The IRQ callback is a list**: `irq_changes` collects the (mask, level) reports in order
//!   and the unit applies them after each call (unit.rs `sync_irq`).
//! - **The CD-DA player's `on_good_status` callback is a count** (`cdda.good_status_due`) that
//!   the bus answers right after each `cdda.run_to` it makes (`run_cdda`); the player changes
//!   nothing of the bus's, so the order of effects is the reference's.
//! - The reference's properties are methods (`bsy()`, `msg()`, `cd()`, `io()`,
//!   `status_bits()`, `out()`, `data_waiting()`); `req` and `ack` stay fields. Python's
//!   `round()` (to even) is `f64::round_ties_even`.
//! - **Save states**: the whole bus is one serde value; `restore` keeps the disc, the switches
//!   and the command log of the bus it lands in, and the player's wiring (cdda.rs).

use std::rc::Rc;

use pcengine_rust::machine::MASTER_HZ;
use serde::{Deserialize, Serialize};

use crate::cdda::CddaPlayer;
use crate::disc::{from_bcd, lba_to_msf, msf_to_lba, to_bcd, Disc};
use crate::seek::seek_time_ms;

pub const GOOD: u8 = 0x00;
/// [DOC: SCSI-1] status byte bit 1
pub const CHECK_CONDITION: u8 = 0x02;
/// [DOC: SCSI-1] the message after the status
pub const COMMAND_COMPLETE: u8 = 0x00;

pub const SENSE_NONE: u8 = 0x00;
pub const SENSE_NOT_READY: u8 = 0x02;
pub const SENSE_ILLEGAL_REQUEST: u8 = 0x05;
/// [DOC: SCSI-2 additional sense codes]
pub const ASC_INVALID_COMMAND: u8 = 0x20;
pub const ASC_LBA_OUT_OF_RANGE: u8 = 0x21;
pub const ASC_SEQUENCE_ERROR: u8 = 0x2C;
pub const ASC_NO_MEDIUM: u8 = 0x3A;

/// the unit's active bits (unit.rs): REQ in status or message in
pub const IRQ_STATUS_MESSAGE: u8 = 0x20;
/// REQ in data in
pub const IRQ_DATA_IN: u8 = 0x40;

/// Command block lengths [DOC: SCSI-1 group 0 = 6 bytes; the vendor commands are 10 bytes: the
/// BIOS sends ten, MAME and ares take ten]. Any other opcode is answered at its first byte.
pub fn command_length(opcode: u8) -> usize {
    match opcode {
        0x00 | 0x03 | 0x08 => 6,
        0xD8 | 0xD9 | 0xDA | 0xDD | 0xDE => 10,
        _ => 1,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    BusFree,
    Command,
    DataIn,
    Status,
    MessageIn,
    /// BSY with no information phase: the drive is working (a seek, CD audio)
    Busy,
}

impl Phase {
    /// The reference's `Phase.value`.
    pub fn value(self) -> &'static str {
        match self {
            Phase::BusFree => "bus free",
            Phase::Command => "command",
            Phase::DataIn => "data in",
            Phase::Status => "status",
            Phase::MessageIn => "message in",
            Phase::Busy => "busy",
        }
    }

    /// (MSG, C/D, I/O) for each phase [DOC: SCSI-1 information transfer phases]
    pub fn lines(self) -> (bool, bool, bool) {
        match self {
            Phase::BusFree => (false, false, false),
            Phase::Command => (false, true, false),
            Phase::DataIn => (false, false, true),
            Phase::Status => (false, true, true),
            Phase::MessageIn => (true, true, true),
            Phase::Busy => (false, false, false),
        }
    }
}

/// The pending protocol step (the reference's `_step`, a method name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    CommandPhase,
    CommandReq,
    StatusPhase,
    MessagePhase,
    BusFree,
    Answer,
}

/// A closed-box window (low, high] in master clocks, as the middle multiple of 3 (one CPU
/// cycle): the probe can say no more.
pub const fn clocks(window_low: i64, window_high: i64) -> i64 {
    (window_low + window_high) / 2 / 3 * 3
}

/// The reference's `ScsiBus` class attributes, with its defaults. Timing in master clocks.
/// Closed-box windows from the reference's scripts/drive_probe.py on Geargrafx 57c257d (one turn
/// = 45 clocks, first sample 84 clocks after the write that starts it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScsiSwitches {
    /// SEL written to BSY, C/D and REQ of the command phase (BSY and REQ rise together)
    /// [OBSERVED: closed box, (74,919, 74,964]]. The BIOS gives up after about 20 ms [DOC].
    pub sel_to_command_phase: i64,
    /// ACK dropped after a command byte to REQ for the next one [OBSERVED: closed box,
    /// (3,009, 3,054]]
    pub command_byte_clocks: i64,
    /// The last command byte's ACK dropped to the answer's first REQ [OBSERVED: closed box]:
    /// TEST UNIT READY (449,544, 449,589], READ TOC (8,994, 9,039], READ SUBQ (3,009, 3,054].
    /// The Hudson manual says a drive just reset takes "a few seconds" to become ready: not
    /// modelled (the closed box answers TEST UNIT READY after a reset as quickly as ever).
    pub test_unit_ready: i64,
    pub toc_clocks: i64,
    pub subq_clocks: i64,
    /// Everything else that answers at once (REQUEST SENSE, PAUSE, AUDIO END modes 0 and 3, an
    /// error): [HYPOTHESIS] the quickest answer the closed box shows, READ SUBQ's.
    pub quick_answer: i64,
    /// An opcode the drive does not know: [OBSERVED: closed box] Geargrafx, which has no
    /// REQUEST SENSE, answers that opcode with GOOD after (450,534, 450,579]; taken as its
    /// unknown-opcode time, provisional.
    pub unknown_command_clocks: i64,
    /// Last data byte's ACK dropped to the status REQ [OBSERVED: closed box, (3,009, 3,054]]
    pub data_to_status: i64,
    /// Status byte's ACK dropped to the message REQ, and the message's to BSY dropping (bus
    /// free) [OBSERVED: closed box, both (1,479, 1,524]]
    pub status_to_message: i64,
    pub message_to_free: i64,
    /// In data in, REQ for the next byte comes as ACK drops [OBSERVED: closed box, within the
    /// first sample]; REQ drops as ACK rises, in every phase [DOC: SCSI-1; closed box likewise].
    /// A read of `$1808` pulses ACK for 15 CPU cycles [emulator tier: MAME, BSD-3]; the closed
    /// box shows ACK low again within (15, 45] clocks of the read, and the BIOS needs it within
    /// 78.
    pub auto_ack_clocks: i64,
    /// What Shadoff's seek times include besides the drive's seek: his test
    /// (seektest2_huc/seek2.c) times the whole BIOS CD_READ of one sector, so its numbers also
    /// hold the selection and the six command bytes (about 92,000 clocks at the times above),
    /// the BIOS reading 2,048 bytes through `$1808` at 38 CPU cycles a byte (233,472) and the
    /// status and message (about 7,000). The first sector's REQ comes this much before his
    /// time. [DOC: his test program; the sum is this model's own times, so provisional]
    pub seek_measurement_extra: i64,
    /// One sector at single speed [DOC: Red Book, 75 sectors a second]
    pub sector_clocks: i64,
    /// A sector that comes round while the last one is still in the buffer is missed; the drive
    /// delivers it this long after its turn [OBSERVED: closed box, 6.22 M clocks = 290 ms after
    /// the missed turn, the same whether the stall was 0.3 or 1.6 sector times].
    pub reread_penalty_ms: i64,
    /// SEL while the drive sends data (or waits to): it stops and offers this many status
    /// bytes, GOOD, then the message and bus free [OBSERVED: closed box, eight `$D8` REQs
    /// carrying `$00`, then `$F8`, both when the data had started and while the READ still
    /// sought].
    pub abort_status_bytes: i64,
    /// An unknown opcode: GOOD (closed box; MAME's table has no entry for one) or CHECK
    /// CONDITION with ILLEGAL REQUEST / INVALID COMMAND (SCSI's rule; ares answers CHECK
    /// CONDITION). Decision C22: GOOD.
    pub unknown_command_check_condition: bool,
}

impl Default for ScsiSwitches {
    fn default() -> Self {
        ScsiSwitches {
            sel_to_command_phase: clocks(74_919, 74_964),
            command_byte_clocks: clocks(3_009, 3_054),
            test_unit_ready: clocks(449_544, 449_589),
            toc_clocks: clocks(8_994, 9_039),
            subq_clocks: clocks(3_009, 3_054),
            quick_answer: clocks(3_009, 3_054),
            unknown_command_clocks: clocks(450_534, 450_579),
            data_to_status: clocks(3_009, 3_054),
            status_to_message: clocks(1_479, 1_524),
            message_to_free: clocks(1_479, 1_524),
            auto_ack_clocks: 45,
            seek_measurement_extra: 332_000,
            sector_clocks: MASTER_HZ / 75,
            reread_penalty_ms: 290,
            abort_status_bytes: 8,
            unknown_command_check_condition: false,
        }
    }
}

/// The bus signals and the drive behind them. `now` is the drive's clock (master clocks).
#[derive(Serialize, Deserialize)]
pub struct ScsiBus {
    #[serde(skip)]
    pub switches: ScsiSwitches,
    #[serde(skip)]
    pub disc: Option<Rc<Disc>>,
    pub cdda: CddaPlayer,
    /// The reference's `_irq` callback: (mask, level) for each level that changed, in order;
    /// the unit takes them.
    #[serde(skip)]
    pub irq_changes: Vec<(u8, bool)>,
    /// (data in, status/message) as last reported
    irq_levels: (bool, bool),
    /// the reset line, as `$1804` bit 1 holds it
    pub rst: bool,
    #[serde(skip)]
    pub record_log: bool,
    #[serde(skip)]
    pub log: Vec<(i64, String)>,
    pub now: i64,
    pub phase: Phase,
    pub req: bool,
    pub ack: bool,
    /// a byte changed hands during the present ACK
    took: bool,
    /// what the target drives
    pub data: u8,
    /// what the CPU last wrote to `$1801`
    pub initiator_data: u8,
    pub cdb: Vec<u8>,
    step: Option<Step>,
    step_at: Option<i64>,
    ack_off_at: Option<i64>,
    selecting: bool,
    block: Vec<u8>,
    taken: usize,
    /// the next LBA a READ delivers
    read_next: i64,
    /// sectors still to deliver
    read_left: i64,
    sector_at: Option<i64>,
    status: u8,
    status_left: i64,
    /// BUSY until the CD-DA player reports (AUDIO START, END mode 2)
    audio_wait: bool,
    /// the LBA under the head when no audio plays
    pub head: i64,
    pub sense_key: u8,
    pub sense_asc: u8,
}

impl ScsiBus {
    pub fn new(disc: Option<Rc<Disc>>, mut cdda: CddaPlayer) -> ScsiBus {
        cdda.disc = disc.clone();
        let mut bus = ScsiBus {
            switches: ScsiSwitches::default(),
            disc,
            cdda,
            irq_changes: Vec::new(),
            irq_levels: (false, false),
            rst: false,
            record_log: false,
            log: Vec::new(),
            now: 0,
            phase: Phase::BusFree,
            req: false,
            ack: false,
            took: false,
            data: 0,
            initiator_data: 0,
            cdb: Vec::new(),
            step: None,
            step_at: None,
            ack_off_at: None,
            selecting: false,
            block: Vec::new(),
            taken: 0,
            read_next: 0,
            read_left: 0,
            sector_at: None,
            status: GOOD,
            status_left: 0,
            audio_wait: false,
            head: 0,
            sense_key: SENSE_NONE,
            sense_asc: 0,
        };
        bus.reset();
        bus
    }

    /// Take a decoded state's place: the disc, the switches and the command log stay this
    /// bus's, and the player keeps its wiring (cdda.rs).
    pub(crate) fn restore(&mut self, mut saved: ScsiBus) {
        let cdda = std::mem::take(&mut saved.cdda);
        saved.switches = self.switches;
        saved.disc = self.disc.take();
        saved.record_log = self.record_log;
        saved.log = std::mem::take(&mut self.log);
        saved.irq_changes = std::mem::take(&mut self.irq_changes);
        let mut player = std::mem::take(&mut self.cdda);
        player.restore(cdda);
        saved.cdda = player;
        *self = saved;
    }

    // -- the lines ------------------------------------------------------------------------------

    pub fn bsy(&self) -> bool {
        self.phase != Phase::BusFree
    }

    pub fn msg(&self) -> bool {
        self.phase.lines().0
    }

    pub fn cd(&self) -> bool {
        self.phase.lines().1
    }

    pub fn io(&self) -> bool {
        self.phase.lines().2
    }

    /// `$1800`: D3 I/O, D4 C/D, D5 MSG, D6 REQ, D7 BSY [DOC: pcetech 8, the BIOS's masks].
    pub fn status_bits(&self) -> u8 {
        let (msg, cd, io) = self.phase.lines();
        (if self.phase != Phase::BusFree {
            0x80
        } else {
            0
        }) | (if self.req { 0x40 } else { 0 })
            | (if msg { 0x20 } else { 0 })
            | (if cd { 0x10 } else { 0 })
            | (if io { 0x08 } else { 0 })
    }

    /// The data-in bytes of the current block not yet taken (the one on the bus first).
    pub fn out(&self) -> &[u8] {
        &self.block[self.taken..]
    }

    /// How many data-in bytes of the current block are not yet taken.
    pub fn data_waiting(&self) -> usize {
        self.block.len() - self.taken
    }

    /// The data bus: the target drives it while I/O is up (data in, status, message in), the
    /// initiator otherwise [DOC: SCSI-1, I/O gives the direction; OBSERVED: closed box, the
    /// abort's status bytes read `$00` after the BIOS-style `$FF` write].
    pub fn read_data(&self) -> u8 {
        if self.io() {
            self.data
        } else {
            self.initiator_data
        }
    }

    /// `$1801` written: what the initiator drives (a command byte, or the BIOS's ids).
    pub fn write_data(&mut self, value: u8) {
        self.initiator_data = value;
    }

    // -- reset ----------------------------------------------------------------------------------

    /// The drive's own reset (power-on, or RST): bus free, nothing pending, audio stopped (MAME
    /// and ares stop CD audio on the drive's reset: emulator tier).
    pub fn reset(&mut self) {
        self.phase = Phase::BusFree;
        self.req = false;
        self.ack = false;
        self.took = false;
        self.data = 0;
        self.initiator_data = 0;
        self.cdb = Vec::new();
        self.step = None;
        self.step_at = None;
        self.ack_off_at = None;
        self.selecting = false;
        self.block = Vec::new();
        self.taken = 0;
        self.read_next = 0;
        self.read_left = 0;
        self.sector_at = None;
        self.status = GOOD;
        self.status_left = 0;
        self.audio_wait = false;
        self.head = 0;
        self.sense_key = SENSE_NONE;
        self.sense_asc = 0;
        self.cdda.stop();
        self.update_irq();
    }

    /// `$1804` bit 1: the drive is reset while the line is held [DOC: pcetech 8 (it names bit
    /// 2, the BIOS's CD_RESET sets and clears bit 1, `$02`)].
    pub fn set_reset(&mut self, level: bool) {
        self.rst = level;
        if level {
            self.reset();
        }
    }

    // -- the initiator's side --------------------------------------------------------------------

    /// A write to `$1800`: SEL, with whatever the CPU left on the data bus (the BIOS: `$81`).
    pub fn select(&mut self) {
        if self.rst || self.selecting {
            return;
        }
        if self.phase == Phase::BusFree || self.phase == Phase::Busy {
            // A busy drive (CD audio, a seek) takes a new selection as from bus free [OBSERVED:
            // closed box, AUDIO END modes 1 and 2: BSY stays up, the command phase comes after
            // the usual time]; what it was waiting for is dropped (the audio plays on).
            self.audio_wait = false;
            self.selecting = true;
            self.schedule(
                Step::CommandPhase,
                self.now + self.switches.sel_to_command_phase,
            );
        } else if self.phase == Phase::DataIn {
            self.abort();
        }
        // In the command, status or message phase SEL is ignored [HYPOTHESIS: no source; the
        // BIOS only selects a busy drive between handshakes it has finished].
    }

    /// `$1802` bit 7: the CPU's ACK.
    pub fn set_ack(&mut self, level: bool) {
        self.ack_off_at = None;
        if level && !self.ack {
            self.ack = true;
            self.ack_rises();
        } else if !level && self.ack {
            self.ack = false;
            self.ack_falls();
        }
    }

    /// A read of `$1808` in data in: ACK up for `auto_ack_clocks`, then down.
    pub fn pulse_ack(&mut self) {
        if self.ack {
            return;
        }
        self.ack = true;
        self.ack_rises();
        self.ack_off_at = Some(self.now + self.switches.auto_ack_clocks);
    }

    /// The target takes the byte (or sees its own taken) and drops REQ [DOC: SCSI-1].
    fn ack_rises(&mut self) {
        if !self.req {
            self.took = false;
            return;
        }
        self.req = false;
        self.took = true;
        match self.phase {
            Phase::Command => self.cdb.push(self.initiator_data),
            Phase::DataIn => self.taken += 1,
            Phase::Status => self.status_left -= 1,
            _ => {}
        }
        self.update_irq();
    }

    fn ack_falls(&mut self) {
        if !self.took {
            if self.phase == Phase::DataIn && self.taken < self.block.len() {
                self.offer_data(); // a sector that arrived while ACK was held
            }
            return;
        }
        self.took = false;
        match self.phase {
            Phase::Command => {
                let want = command_length(self.cdb[0]);
                if self.cdb.len() < want {
                    self.schedule(
                        Step::CommandReq,
                        self.now + self.switches.command_byte_clocks,
                    );
                } else {
                    self.execute();
                }
            }
            Phase::DataIn => {
                if self.taken < self.block.len() {
                    self.offer_data();
                } else if self.read_left == 0 {
                    self.schedule(Step::StatusPhase, self.now + self.switches.data_to_status);
                }
                // else: the next sector raises REQ when it arrives
            }
            Phase::Status => {
                if self.status_left > 0 {
                    self.req = true; // the abort's further status bytes [HYPOTHESIS: at once]
                    self.update_irq();
                } else {
                    self.schedule(
                        Step::MessagePhase,
                        self.now + self.switches.status_to_message,
                    );
                }
            }
            Phase::MessageIn => {
                self.schedule(Step::BusFree, self.now + self.switches.message_to_free);
            }
            _ => {}
        }
    }

    /// The CD-DA player's report: a seek ended (AUDIO START), or the range ended in IRQ mode
    /// (AUDIO END mode 2). The drive answers the command it was busy with.
    pub fn good_status(&mut self) {
        if self.audio_wait && self.phase == Phase::Busy {
            self.audio_wait = false;
            self.enter_status(GOOD, 1);
        }
    }

    // -- time -----------------------------------------------------------------------------------

    pub fn next_event(&self) -> Option<i64> {
        [
            self.step_at,
            self.sector_at,
            self.ack_off_at,
            self.cdda.next_event(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// The player's `run_to`, then the `good_status` reports it owes (its callback).
    fn run_cdda(&mut self, mclk: i64) {
        self.cdda.run_to(mclk);
        while self.cdda.good_status_due > 0 {
            self.cdda.good_status_due -= 1;
            self.good_status();
        }
    }

    /// Do everything that falls due up to `mclk`, in time order, then stand at `mclk`.
    pub fn run_to(&mut self, mclk: i64) {
        while let Some(at) = self.next_event().filter(|&at| at <= mclk) {
            self.now = self.now.max(at);
            if self.ack_off_at.is_some_and(|t| t <= at) {
                self.ack_off_at = None;
                self.ack = false;
                self.ack_falls();
            } else if self.step_at.is_some_and(|t| t <= at) {
                let step = self.step.take().expect("a step is due");
                self.step_at = None;
                match step {
                    Step::CommandPhase => self.command_phase(),
                    Step::CommandReq => self.command_req(),
                    Step::StatusPhase => self.status_phase(),
                    Step::MessagePhase => self.message_phase(),
                    Step::BusFree => self.bus_free(),
                    Step::Answer => self.answer(),
                }
            } else if self.sector_at.is_some_and(|t| t <= at) {
                self.sector_at = None;
                self.sector_turn(at);
            } else {
                self.run_cdda(at);
            }
        }
        self.now = self.now.max(mclk);
        self.run_cdda(self.now);
    }

    fn schedule(&mut self, step: Step, at: i64) {
        self.step = Some(step);
        self.step_at = Some(at);
    }

    // -- protocol steps ---------------------------------------------------------------------------

    /// The selection answered: BSY, C/D and REQ for the first command byte.
    fn command_phase(&mut self) {
        self.selecting = false;
        self.phase = Phase::Command;
        self.cdb = Vec::new();
        self.req = true;
        self.update_irq();
    }

    fn command_req(&mut self) {
        self.req = true;
        self.update_irq();
    }

    fn status_phase(&mut self) {
        self.enter_status(self.status, 1);
    }

    fn message_phase(&mut self) {
        self.phase = Phase::MessageIn;
        self.data = COMMAND_COMPLETE;
        self.req = true;
        self.update_irq();
    }

    fn bus_free(&mut self) {
        self.phase = Phase::BusFree;
        self.req = false;
        self.update_irq();
    }

    /// An answer's data (READ TOC, READ SUBQ, REQUEST SENSE) or, with none, its status.
    fn answer(&mut self) {
        if !self.block.is_empty() {
            self.phase = Phase::DataIn;
            self.offer_data();
        } else {
            self.enter_status(self.status, 1);
        }
    }

    fn enter_status(&mut self, status: u8, count: i64) {
        self.phase = Phase::Status;
        self.status = status;
        self.status_left = count;
        self.data = status;
        self.req = true;
        self.update_irq();
    }

    fn offer_data(&mut self) {
        if self.ack {
            return; // REQ waits for ACK to drop
        }
        self.data = self.block[self.taken];
        self.req = true;
        self.update_irq();
    }

    /// SEL during data in: the read stops at once and the status phase offers
    /// `abort_status_bytes` bytes of GOOD [OBSERVED: closed box]; the BIOS takes them all.
    fn abort(&mut self) {
        self.read_left = 0;
        self.sector_at = None;
        self.block = Vec::new();
        self.taken = 0;
        self.step = None;
        self.step_at = None;
        self.enter_status(GOOD, self.switches.abort_status_bytes);
    }

    // -- commands -------------------------------------------------------------------------------

    fn execute(&mut self) {
        let cdb = self.cdb.clone();
        if self.record_log {
            let text: Vec<String> = cdb.iter().map(|b| format!("{b:02X}")).collect();
            self.log.push((self.now, format!("cmd {}", text.join(" "))));
        }
        self.run_cdda(self.now);
        self.block = Vec::new();
        self.taken = 0;
        self.status = GOOD;
        let handler: Option<fn(&mut ScsiBus, &[u8])> = match cdb[0] {
            0x00 => Some(ScsiBus::test_unit_ready),
            0x03 => Some(ScsiBus::request_sense),
            0x08 => Some(ScsiBus::read),
            0xD8 => Some(ScsiBus::audio_start),
            0xD9 => Some(ScsiBus::audio_end),
            0xDA => Some(ScsiBus::audio_pause),
            0xDD => Some(ScsiBus::read_subq),
            0xDE => Some(ScsiBus::read_toc),
            _ => None,
        };
        let Some(handler) = handler else {
            if self.switches.unknown_command_check_condition {
                self.fail(SENSE_ILLEGAL_REQUEST, ASC_INVALID_COMMAND);
            } else {
                self.reply(GOOD, self.switches.unknown_command_clocks, &[]);
            }
            return;
        };
        if cdb[0] != 0x03 && self.disc.is_none() {
            self.fail(SENSE_NOT_READY, ASC_NO_MEDIUM);
            return;
        }
        handler(self, &cdb);
    }

    /// Answer `after` clocks from now: the data (if any), then the status.
    fn reply(&mut self, status: u8, after: i64, data: &[u8]) {
        self.status = status;
        self.block = data.to_vec();
        self.taken = 0;
        self.phase = if data.is_empty() {
            Phase::Busy
        } else {
            Phase::DataIn
        };
        self.req = false;
        self.update_irq();
        self.schedule(Step::Answer, self.now + after);
    }

    fn fail(&mut self, key: u8, asc: u8) {
        self.sense_key = key;
        self.sense_asc = asc;
        self.reply(CHECK_CONDITION, self.switches.quick_answer, &[]);
    }

    fn test_unit_ready(&mut self, _cdb: &[u8]) {
        self.reply(GOOD, self.switches.test_unit_ready, &[]);
    }

    /// Extended sense, 18 bytes [DOC: SCSI-1 extended sense format `$70`]; allocation length 0
    /// means 4 [DOC: SCSI-1]. The sense is cleared once reported.
    fn request_sense(&mut self, cdb: &[u8]) {
        let mut sense = [0u8; 18];
        sense[0] = 0x70;
        sense[2] = self.sense_key;
        sense[7] = 10; // additional sense length
        sense[12] = self.sense_asc;
        let length = if cdb[4] == 0 { 4 } else { cdb[4] as usize };
        self.sense_key = SENSE_NONE;
        self.sense_asc = 0;
        let data = &sense[..length.min(sense.len())];
        self.reply(GOOD, self.switches.quick_answer, data);
    }

    fn position(&self) -> i64 {
        if self.cdda.active() {
            self.cdda.position()
        } else {
            self.head
        }
    }

    /// Shadoff's measured seek time (seek.rs, tier 3) in master clocks.
    fn seek_clocks(&self, target: i64) -> i64 {
        (seek_time_ms(self.position(), target) * MASTER_HZ as f64 / 1000.0).round_ties_even() as i64
    }

    fn disc(&self) -> Rc<Disc> {
        Rc::clone(
            self.disc
                .as_ref()
                .expect("a command that needs the disc has one"),
        )
    }

    /// READ(6): a 21-bit LBA and a count of 2048-byte sectors [DOC: SCSI-1, pcetech 8].
    fn read(&mut self, cdb: &[u8]) {
        let disc = self.disc();
        let lba = ((cdb[1] & 0x1F) as i64) << 16 | (cdb[2] as i64) << 8 | cdb[3] as i64;
        let count = cdb[4] as i64;
        if count == 0 {
            // [emulator tier: MAME, ares] nothing is read and the answer is GOOD; SCSI-1 reads
            // 256 sectors for a count of 0 (decision C18 stands: provisional).
            self.reply(GOOD, self.switches.quick_answer, &[]);
            return;
        }
        if lba + count > disc.sector_count {
            self.fail(SENSE_ILLEGAL_REQUEST, ASC_LBA_OUT_OF_RANGE);
            return;
        }
        let seek = (self.seek_clocks(lba) - self.switches.seek_measurement_extra)
            .max(self.switches.sector_clocks);
        self.cdda.idle(); // a data read takes the head: CD audio stops [emulator tier: MAME]
        self.read_next = lba;
        self.read_left = count;
        self.phase = Phase::DataIn; // I/O up, REQ waits for the sector [OBSERVED: closed box `$88`]
        self.req = false;
        self.update_irq();
        self.sector_at = Some(self.now + seek);
    }

    /// A sector of the READ comes under the head.
    fn sector_turn(&mut self, at: i64) {
        if self.taken < self.block.len() {
            // missed: the buffer still holds the last one [OBSERVED: closed box]
            self.sector_at = Some(at + MASTER_HZ * self.switches.reread_penalty_ms / 1000);
            return;
        }
        let disc = self.disc();
        let lba = self.read_next;
        self.block = disc.read_data(lba).to_vec();
        self.taken = 0;
        self.read_next += 1;
        self.read_left -= 1;
        self.head = self.read_next;
        if self.read_left != 0 {
            self.sector_at = Some(at + self.switches.sector_clocks);
        }
        self.offer_data();
    }

    /// The LBA a D8/D9 command names: byte 9 bits 7-6 = 00 LBA (bytes 3-5), 01 absolute MSF in
    /// BCD (bytes 2-4), 10 a track number in BCD (byte 2) [DOC: Hudson BIOS manual CD_PLAY's
    /// address types; the byte positions as the BIOS fills them].
    fn address(&self, cdb: &[u8]) -> Option<i64> {
        let disc = self.disc();
        let kind = cdb[9] & 0xC0;
        let lba = if kind == 0x00 {
            (cdb[3] as i64) << 16 | (cdb[4] as i64) << 8 | cdb[5] as i64
        } else if kind == 0x40 {
            msf_to_lba(from_bcd(cdb[2]), from_bcd(cdb[3]), from_bcd(cdb[4]))
        } else if kind == 0x80 {
            disc.track(from_bcd(cdb[2]))?.start
        } else {
            return None;
        };
        (0 <= lba && lba < disc.sector_count).then_some(lba)
    }

    /// D8: seek to the start, then play (byte 1 bit 0) or pause there. BUSY until the seek
    /// ends, then GOOD [DOC: Hudson BIOS manual CD_SEARCH; OBSERVED: closed box].
    fn audio_start(&mut self, cdb: &[u8]) {
        let Some(start) = self.address(cdb) else {
            self.fail(SENSE_ILLEGAL_REQUEST, ASC_LBA_OUT_OF_RANGE);
            return;
        };
        let seek_clocks = self.seek_clocks(start);
        self.cdda.play(start, cdb[1] & 0x01 == 0, seek_clocks);
        self.busy_until_audio();
    }

    /// D9: where playing stops, and how (byte 1 bits 1-0) [DOC: Hudson BIOS manual CD_PLAY]: 0
    /// stop (the manual: "mute play", status at once; MAME and ares stop), 1 repeat and stay
    /// busy until the next selection, 2 play and stay busy until the range ends, 3 play with the
    /// status at once. A track number names the end of the track before it.
    fn audio_end(&mut self, cdb: &[u8]) {
        let mode = cdb[1] & 0x03;
        let mut end = self.address(cdb);
        if cdb[9] & 0xC0 == 0x80 {
            end = end.map(|end| end - 1);
        }
        let Some(end) = end else {
            self.fail(SENSE_ILLEGAL_REQUEST, ASC_LBA_OUT_OF_RANGE);
            return;
        };
        if mode == 0 {
            self.cdda.stop();
            self.reply(GOOD, self.switches.quick_answer, &[]);
            return;
        }
        if mode == 1 && end < self.cdda.start {
            // A repeat of an empty range plays nothing [emulator tier: MAME's note that an end
            // before the start plays nothing "according to real HW refs"]; the drive stays busy
            // as for any repeat [DOC; OBSERVED: closed box, BSY alone for 11 s].
            self.cdda.stop();
        } else {
            self.cdda.set_end(end, mode); // 1 loop, 2 IRQ, 3 stop
        }
        if mode == 1 {
            self.phase = Phase::Busy; // until the next selection [DOC; OBSERVED: closed box]
            self.req = false;
            self.update_irq();
        } else if mode == 2 {
            self.busy_until_audio();
        } else {
            self.reply(GOOD, self.switches.quick_answer, &[]);
        }
    }

    fn busy_until_audio(&mut self) {
        self.phase = Phase::Busy;
        self.req = false;
        self.audio_wait = true;
        self.update_irq();
    }

    /// DA: pause [DOC: Hudson BIOS manual CD_PAUSE]; with no audio started, CHECK CONDITION
    /// [emulator tier: MAME, ares] (the sense code is a choice: SCSI-2's sequence error).
    fn audio_pause(&mut self, _cdb: &[u8]) {
        if !self.cdda.active() {
            self.fail(SENSE_ILLEGAL_REQUEST, ASC_SEQUENCE_ERROR);
            return;
        }
        self.cdda.pause();
        self.reply(GOOD, self.switches.quick_answer, &[]);
    }

    /// DD: ten bytes [DOC: Hudson BIOS manual CD_SUBQ]: play status, control/ADR, track,
    /// index, the track-relative MSF and the absolute MSF, all BCD.
    fn read_subq(&mut self, _cdb: &[u8]) {
        let disc = self.disc();
        let lba = self.position();
        let track = disc.track_at(lba);
        let control = if track.is_some_and(|t| t.is_data) {
            0x04
        } else {
            0x00
        };
        let number = track.map_or(0, |t| t.number);
        let index = if track.is_some_and(|t| lba >= t.start) {
            1
        } else {
            0
        };
        let relative = track.map_or(0, |t| (lba - t.start).abs());
        let (rm, rs, rf) = (relative / 4500, (relative / 75) % 60, relative % 75);
        let (am, as_, af) = lba_to_msf(lba);
        let mut data = vec![
            self.cdda.status_code(),
            control << 4 | 0x01,
            to_bcd(number),
            to_bcd(index),
        ];
        data.extend([rm, rs, rf, am, as_, af].map(to_bcd));
        self.reply(GOOD, self.switches.subq_clocks, &data);
    }

    /// DE [DOC: Hudson BIOS manual CD_DINFO's kinds]: 0 first and last track (BCD), 1 the
    /// lead-out's absolute MSF, 2 a track's absolute MSF and its control (`$04` data, `$00`
    /// audio), 3 a track's LBA and control. Answer lengths 2, 3, 4, 4 [emulator tier: MAME for
    /// 0-2 (the closed box sends 4 each); kind 3 from ares].
    fn read_toc(&mut self, cdb: &[u8]) {
        let disc = self.disc();
        let kind = cdb[1];
        let data: Vec<u8> = match kind {
            0 => vec![to_bcd(disc.first_track()), to_bcd(disc.last_track())],
            1 => {
                let (m, s, f) = lba_to_msf(disc.sector_count);
                vec![to_bcd(m), to_bcd(s), to_bcd(f)]
            }
            2 | 3 => {
                let (lba, control) = if cdb[2] == 0xAA {
                    (disc.sector_count, 0x04) // the lead-out as a track [emulator tier: MAME]
                } else {
                    let Some(track) = disc.track(from_bcd(cdb[2])) else {
                        self.fail(SENSE_ILLEGAL_REQUEST, ASC_LBA_OUT_OF_RANGE);
                        return;
                    };
                    (track.start, if track.is_data { 0x04 } else { 0x00 })
                };
                if kind == 2 {
                    let (m, s, f) = lba_to_msf(lba);
                    vec![to_bcd(m), to_bcd(s), to_bcd(f), control]
                } else {
                    vec![
                        (lba >> 16 & 0xFF) as u8,
                        (lba >> 8 & 0xFF) as u8,
                        (lba & 0xFF) as u8,
                        control,
                    ]
                }
            }
            _ => {
                self.fail(SENSE_ILLEGAL_REQUEST, ASC_INVALID_COMMAND);
                return;
            }
        };
        self.reply(GOOD, self.switches.toc_clocks, &data);
    }

    // -- IRQ2 levels ----------------------------------------------------------------------------

    /// Report each level that changed: data in (`$40`) is REQ in data in, status/message
    /// (`$20`) is REQ in status or message in [DOC: the BIOS's IRQ2 handler reads the status
    /// and message on `$20` and starts the ADPCM DMA on `$40`; pcetech 8].
    fn update_irq(&mut self) {
        let data_in = self.req && self.phase == Phase::DataIn;
        let status = self.req && matches!(self.phase, Phase::Status | Phase::MessageIn);
        let (old_data, old_status) = self.irq_levels;
        self.irq_levels = (data_in, status);
        if data_in != old_data {
            self.irq_changes.push((IRQ_DATA_IN, data_in));
        }
        if status != old_status {
            self.irq_changes.push((IRQ_STATUS_MESSAGE, status));
        }
    }
}
