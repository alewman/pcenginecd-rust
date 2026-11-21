// SPDX-License-Identifier: MIT
//! The ADPCM unit: 64 KiB of RAM, its pointers, the MSM5205 decoder and the audio source
//! (`adpcm.py`).
//!
//! Tiers (the base board's docs/validation.md): [hw: pcetech 8] MacDonald, tier 2, names the
//! registers and a few bit meanings and says the rest is "currently trying to document"; [mfr:
//! Hudson BIOS manual and System Card ROM] tier 3, the BIOS source (`AD_STAT`, `AD_PLAY`,
//! `ad_*_addr_set` in `Super CD-ROM2 System V3.00 (J).ASM`) shows how the registers are driven;
//! \[emu\] Mesen2, Geargrafx and Mednafen (GPL: their behaviour is reported in the reference's
//! docs/m8-record.md, they are run as closed boxes) and MAME `nec/pce_cd.cpp` (BSD-3) are tier 5
//! detectors: where a behaviour comes only from them it is a field of `AdpcmSwitches` (a named
//! switch) with a U/C entry in the reference's docs/m8-record.md.
//!
//! Registers (offsets from `$1800`):
//!
//! | reg | write | read |
//! | --- | --- | --- |
//! | `$08/$09` | the 16-bit address latch (low / high) | (`$08` reads the CD data bus: the unit) |
//! | `$0A` | the byte for the write pointer, which then steps | the read buffer; next fetch starts |
//! | `$0B` | DMA control (bit 1 requests CD-to-ADPCM DMA; bit 0 follows a waiting block) | the same |
//! | `$0C` | -- | bit 7 read busy, bit 3 playing, bit 2 write busy, bit 0 end reached |
//! | `$0D` | control (below) | the last value written |
//! | `$0E` | rate: playback at 32 / (16 - n) kHz, n = value & $0F | the same |
//!
//! Control `$0D`: bit 7 reset (pointers and length to zero, flags cleared, the read buffer is
//! kept), bit 6 stop at length zero (clear: keep playing), bit 5 play, bit 4 latch the length
//! from the address latch (a length of zero then means 64 KiB), bit 3 copy the latch to the read
//! pointer on a rising edge (bit 2 set: as it is; clear: one less, so the first read fetch lands
//! on it), bit 1 the same for the write pointer (bit 0). [hw: pcetech 8; mfr: the BIOS routines
//! `ad_read_addr_set`, `ad_write_addr_set`, `ad_length_set`]
//!
//! Flags: the half flag (IRQ2 source `$04`, `$1803` bit 2) is set while less than half of the
//! length is left and the end flag (source `$08`, `$180C` bit 0) when the length has run out.
//! [hw: pcetech 8 `$1803` bit 2; mfr: `AD_STAT` reads `$180C` bit 0 then `$1803` bit 2, and
//! `$180C` bit 3 for busy]
//!
//! Decoder (MSM5205, 4-bit): a 12-bit signal and a step index 0-48. A nibble is a sign bit and
//! three magnitude bits; the signal moves by `step/8 + step/4*b0 + step/2*b1 + step*b2` (integer
//! divisions of `step = floor(16 * 1.1 ** index)`), the index by -1 -1 -1 -1 +2 +4 +6 +8 clamped
//! to 0..48. Each byte holds two samples, the high nibble first. The audio is the signal held
//! between samples (the `AudioSource` below).
//!
//! Differences forced by Rust:
//!
//! - **`Adpcm` is its own audio source**: the reference's `AdpcmSource` (an object that holds
//!   the unit) is the `origin`, `taken` and `held` fields and the `AudioSource` impl here; the
//!   fader it asks the audio player for is the shared `Fader` (cdda.rs). The reference's source
//!   first catches the unit up to the mixer's clock; the board has always done that already
//!   (it runs the port device at every advance), and this source cannot reach the unit.
//! - **`on_irq`** (the reference's callback into the unit) is the list `irq_changes`, applied
//!   by the unit after each call.
//! - **The step table** is the 49 integers `floor(16 * 1.1 ** index)` written out, so that it
//!   does not depend on a platform's `pow` (wasm).

use std::cell::Cell;
use std::rc::Rc;

use pcengine_rust::machine::{py_floordiv, round_ties_even, MASTER_HZ};
use pcengine_rust::sockets::{AudioSource, DeviceState};
use serde::{Deserialize, Serialize};

use crate::cdda::{ceil_div, Fader};

pub const ADPCM_RAM_BYTES: usize = 64 * 1024;
pub const OUTPUT_HZ: i64 = 44_100;

pub const IRQ_HALF: u8 = 0x04;
pub const IRQ_END: u8 = 0x08;

/// [emu: Mesen2, Geargrafx] 17 bits: a length of 64 KiB needs the 17th
pub const LENGTH_MASK: u32 = 0x1FFFF;

pub const INDEX_STEP: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// `floor(16 * 1.1 ** index)` for index 0..48.
pub const STEPS: [i32; 49] = [
    16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796,
    876, 963, 1060, 1166, 1282, 1411, 1552,
];

/// `DELTA[index * 8 + magnitude]`, the signal change for a 3-bit magnitude [the MSM5205's step
/// formula as modelled by MAME `sound/msm5205.cpp` (BSD-3-Clause): emulator tier].
pub const DELTA: [i32; 49 * 8] = step_table();

const fn step_table() -> [i32; 49 * 8] {
    let mut table = [0; 49 * 8];
    let mut index = 0;
    while index < 49 {
        let step = STEPS[index];
        let mut magnitude = 0;
        while magnitude < 8 {
            table[index * 8 + magnitude] = step / 8
                + (if magnitude & 1 != 0 { step / 4 } else { 0 })
                + (if magnitude & 2 != 0 { step / 2 } else { 0 })
                + (if magnitude & 4 != 0 { step } else { 0 });
            magnitude += 1;
        }
        index += 1;
    }
    table
}

/// Switches: behaviours only tier 5 gives, or undecidable between sources (the reference's
/// docs/m8-record.md; its `Adpcm` class attributes, with its defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdpcmSwitches {
    /// Playback rate base: f = base_hz / (16 - n). [hw: pcetech 8; mfr: BIOS manual AD_PLAY
    /// "fKHz=32/(16-n)"] 32 kHz; Mednafen uses 32087.5 (a crystal-derived guess, tier 5).
    pub base_hz: f64,
    /// Master clocks until `$180C` bit 7 clears after a `$180A` read. [hw: pcetech 8: "about
    /// 24 cycles in high-speed mode"] 24 CPU cycles x 3. Mednafen: 57 clocks; Mesen2: a RAM slot
    /// model (30 cycles minimum, 32 safe); 0 = immediate.
    pub read_clocks: i64,
    /// The same for a `$180A` write (bit 2): pcetech says the BIOS polls bit 2 after each write
    /// but gives no time; the read's 24 cycles are used, a choice (ares estimates 20 for both).
    pub write_clocks: i64,
    /// The 12-bit signal saturates at -2048..2047 (MAME's MSM5205 model) or wraps (Mesen2,
    /// Geargrafx).
    pub saturate: bool,
    /// 16-bit output per signal unit. 16 would be full scale; 10 leaves headroom for the PSG
    /// and CD-DA (a choice, as reported of other emulators in the reference's docs/m8-record.md
    /// U9d). Tier 5.
    pub output_scale: i32,
    /// A write to RAM while the length is zero raises the end flag [emu: Mesen2, Geargrafx];
    /// the BIOS clears it with the length latch afterwards. Mednafen has no such rule.
    pub end_on_write_at_zero: bool,
}

impl Default for AdpcmSwitches {
    fn default() -> Self {
        AdpcmSwitches {
            base_hz: 32_000.0,
            read_clocks: 24 * 3,
            write_clocks: 24 * 3,
            saturate: true,
            output_scale: 10,
            end_on_write_at_zero: true,
        }
    }
}

fn zeroed_ram() -> Vec<u8> {
    vec![0; ADPCM_RAM_BYTES]
}

/// RAM, pointers, flags and the decoder. Time comes from the unit (`run(mclk)` and the `now`
/// of each register access); the unit calls `run` for every event from `next_event`.
#[derive(Serialize, Deserialize)]
pub struct Adpcm {
    #[serde(skip)]
    pub switches: AdpcmSwitches,
    #[serde(skip)]
    pub irq_changes: Vec<(u8, bool)>,
    #[serde(skip)]
    pub fader: Rc<Cell<Fader>>,
    pub ram: Vec<u8>,
    pub latch: u16,
    pub read_ptr: u16,
    pub write_ptr: u16,
    pub read_buffer: u8,
    pub control: u8,
    pub dma: u8,
    pub rate: u8,
    pub length: u32,
    pub half: bool,
    pub end: bool,
    pub playing: bool,
    pub play_request: bool,
    /// the next sample is the high nibble of the byte at read_ptr
    pub high_nibble_next: bool,
    pub signal: i32,
    pub index: i32,
    pub read_due: Option<i64>,
    pub write_due: Option<i64>,
    pub write_value: u8,
    /// 16.16 fixed-point master clocks
    pub sample_at: i64,
    /// the audio source wants (time, output) events
    pub record: bool,
    pub events: Vec<(i64, i32)>,
    pub last_output: i32,
    /// decoded samples (statistics)
    pub sample_count: u64,
    /// times the end flag was raised (statistics)
    pub end_count: u64,
    // the audio source (the reference's `AdpcmSource`)
    pub origin: Option<i64>,
    pub taken: i64,
    pub held: i32,
    /// AudioSource socket: the mixer's gain for this source
    pub gain: f64,
}

impl Adpcm {
    pub fn new(fader: Rc<Cell<Fader>>) -> Adpcm {
        Adpcm {
            switches: AdpcmSwitches::default(),
            irq_changes: Vec::new(),
            fader,
            ram: zeroed_ram(),
            latch: 0,
            read_ptr: 0,
            write_ptr: 0,
            read_buffer: 0,
            control: 0,
            dma: 0,
            rate: 0,
            length: 0,
            half: false,
            end: false,
            playing: false,
            play_request: false,
            high_nibble_next: true,
            signal: 0,
            index: 0,
            read_due: None,
            write_due: None,
            write_value: 0,
            sample_at: 0,
            record: false,
            events: Vec::new(),
            last_output: 0,
            sample_count: 0,
            end_count: 0,
            origin: None,
            taken: 0,
            held: 0,
            gain: 1.0,
        }
    }

    /// Take a decoded state's place: the switches and the shared fader stay this unit's.
    pub(crate) fn restore(&mut self, mut saved: Adpcm) {
        saved.switches = self.switches;
        saved.fader = Rc::clone(&self.fader);
        *self = saved;
    }

    // -- state the unit reads -----------------------------------------------------------------

    pub fn write_busy(&self) -> bool {
        self.write_due.is_some()
    }

    pub fn read_busy(&self) -> bool {
        self.read_due.is_some()
    }

    /// Master clocks per sample, 16.16 fixed point.
    pub fn period_fixed(&self) -> i64 {
        let clocks = MASTER_HZ * 65536 * (16 - (self.rate & 0x0F) as i64);
        (clocks as f64 / self.switches.base_hz) as i64
    }

    /// Power-on and the `$180D` bit 7 reset (the latter keeps the read buffer).
    pub fn reset(&mut self) {
        self.read_ptr = 0;
        self.write_ptr = 0;
        self.latch = 0;
        self.length = 0;
        self.high_nibble_next = true;
        self.signal = 0;
        self.index = 0;
        self.play_request = false;
        self.set_half(false);
        self.set_end(false);
    }

    /// Power-on at master clock `at` (the output goes idle then, the reference's issue 0001
    /// item 3).
    pub fn power_on(&mut self, at: i64) {
        self.reset();
        self.control = 0;
        self.dma = 0;
        self.rate = 0;
        self.read_buffer = 0;
        self.playing = false;
        self.read_due = None;
        self.write_due = None;
        self.output(at, 0);
    }

    // -- flags --------------------------------------------------------------------------------

    fn set_half(&mut self, value: bool) {
        if self.half != value {
            self.half = value;
            self.irq_changes.push((IRQ_HALF, value));
        }
    }

    fn set_end(&mut self, value: bool) {
        if self.end != value {
            self.end = value;
            self.end_count += value as u64;
            self.irq_changes.push((IRQ_END, value));
        }
    }

    // -- registers ----------------------------------------------------------------------------

    pub fn write(&mut self, reg: u8, value: u8, now: i64) {
        match reg {
            0x08 | 0x09 => {
                if reg == 0x08 {
                    self.latch = (self.latch & 0xFF00) | value as u16;
                } else {
                    self.latch = (self.latch & 0x00FF) | (value as u16) << 8;
                }
                if self.control & 0x10 != 0 {
                    // the length latch follows the address while bit 4 is held
                    self.length = self.latch as u32;
                }
            }
            0x0A => {
                self.write_value = value;
                self.write_due = Some(now + self.switches.write_clocks);
                if self.switches.write_clocks == 0 {
                    self.finish_write();
                }
            }
            0x0B => self.dma = value,
            0x0D => self.write_control(value, now),
            0x0E => self.rate = value,
            _ => {}
        }
    }

    pub fn read(&mut self, reg: u8, now: i64) -> u8 {
        match reg {
            0x0A => {
                let value = self.read_buffer;
                self.read_due = Some(now + self.switches.read_clocks);
                if self.switches.read_clocks == 0 {
                    self.finish_read();
                }
                value
            }
            0x0B => self.dma,
            0x0C => {
                (if self.end { 0x01 } else { 0 })
                    | (if self.write_busy() { 0x04 } else { 0 })
                    | (if self.playing { 0x08 } else { 0 })
                    | (if self.read_busy() { 0x80 } else { 0 })
            }
            0x0D => self.control,
            0x0E => self.rate,
            _ => 0,
        }
    }

    fn write_control(&mut self, value: u8, now: i64) {
        let old = self.control;
        self.control = value;
        if value & 0x02 != 0 && old & 0x02 == 0 {
            self.write_ptr = self
                .latch
                .wrapping_sub(if value & 0x01 != 0 { 0 } else { 1 });
        }
        if value & 0x08 != 0 && old & 0x08 == 0 {
            self.read_ptr = self
                .latch
                .wrapping_sub(if value & 0x04 != 0 { 0 } else { 1 });
        }
        if value & 0x20 != 0 && !self.playing {
            if !self.play_request {
                self.sample_at = (now << 16) + self.period_fixed();
            }
            self.play_request = true;
        }
        if value & 0x10 != 0 {
            self.length = self.latch as u32;
            self.set_end(false);
        }
        if value & 0x80 != 0 {
            self.reset();
            if value & 0x20 == 0 {
                self.stop(now);
            }
        }
    }

    // -- RAM port operations --------------------------------------------------------------------

    fn finish_read(&mut self) {
        self.read_due = None;
        self.read_buffer = self.ram[self.read_ptr as usize];
        self.read_ptr = self.read_ptr.wrapping_add(1);
        if self.control & 0x10 == 0 {
            if self.length > 0 {
                self.length -= 1;
                self.set_half(self.length < 0x8000);
            } else {
                self.set_end(true);
                self.set_half(false);
            }
        }
    }

    fn finish_write(&mut self) {
        self.write_due = None;
        self.ram[self.write_ptr as usize] = self.write_value;
        self.write_ptr = self.write_ptr.wrapping_add(1);
        if self.length == 0 && self.switches.end_on_write_at_zero {
            self.set_end(true);
        }
        self.set_half(self.length < 0x8000);
        if self.control & 0x10 == 0 {
            self.length = (self.length + 1) & LENGTH_MASK;
        }
    }

    /// One byte from the CD-to-ADPCM DMA, through the same write path as `$180A`. False if
    /// the previous write is still in progress (the DMA retries).
    pub fn dma_write(&mut self, value: u8, now: i64) -> bool {
        if self.write_busy() {
            return false;
        }
        self.write(0x0A, value, now);
        true
    }

    // -- time ---------------------------------------------------------------------------------

    fn sample_due(&self) -> i64 {
        ceil_div(self.sample_at, 65536)
    }

    pub fn next_event(&self) -> Option<i64> {
        let mut due = match (self.read_due, self.write_due) {
            (Some(read), Some(write)) => Some(read.min(write)),
            (read, write) => read.or(write),
        };
        if self.playing || self.play_request {
            let sample = self.sample_due();
            due = Some(due.map_or(sample, |at| at.min(sample)));
        }
        due
    }

    /// Do every event due at or before `mclk`, in time order.
    pub fn run(&mut self, mclk: i64) {
        while let Some(at) = self.next_event().filter(|&at| at <= mclk) {
            if self.read_due.is_some_and(|due| due <= at) {
                self.finish_read();
            }
            if self.write_due.is_some_and(|due| due <= at) {
                self.finish_write();
            }
            if (self.playing || self.play_request) && self.sample_due() <= at {
                self.play_sample(at);
            }
        }
    }

    fn stop(&mut self, at: i64) {
        self.playing = false;
        self.play_request = false;
        self.output(at, 0);
    }

    /// The decoder output changed at `at` (0 = the DAC is idle).
    fn output(&mut self, at: i64, value: i32) {
        if value != self.last_output {
            self.last_output = value;
            if self.record {
                self.events.push((at, value));
            }
        }
    }

    fn play_sample(&mut self, at: i64) {
        let control = self.control;
        self.sample_at += self.period_fixed();
        if control & 0x80 != 0 {
            // reset held: play follows bit 5 but nothing is fetched
            self.playing = control & 0x20 != 0;
            self.play_request = false;
            if !self.playing {
                self.output(at, 0);
            }
            return;
        }
        if (control & 0x40 != 0 && self.length == 0) || control & 0x20 == 0 {
            self.stop(at);
            return;
        }
        if self.play_request {
            self.play_request = false;
            self.playing = true;
            self.signal = 0;
            self.index = 0;
        }
        let byte = self.ram[self.read_ptr as usize];
        let nibble = if self.high_nibble_next {
            self.high_nibble_next = false;
            byte >> 4
        } else {
            self.high_nibble_next = true;
            self.read_ptr = self.read_ptr.wrapping_add(1);
            self.length = self.length.wrapping_sub(1) & LENGTH_MASK;
            byte & 0x0F
        };
        let delta = DELTA[self.index as usize * 8 + (nibble & 7) as usize];
        let signal = self.signal + if nibble & 8 != 0 { -delta } else { delta };
        self.signal = if self.switches.saturate {
            signal.clamp(-2048, 2047)
        } else {
            ((signal + 2048) & 0xFFF) - 2048
        };
        self.index = (self.index + INDEX_STEP[(nibble & 7) as usize]).clamp(0, 48);
        self.sample_count += 1;
        self.set_half(self.length < 0x8000);
        if self.length == 0 {
            self.set_end(true);
        }
        self.output(at, self.signal * self.switches.output_scale);
    }
}

impl DeviceState for Adpcm {} // saved with the unit

/// The ADPCM audio for the board's mixer (socket 9): the decoder output held, sampled at 44.1
/// kHz at exact master-clock times, with the `$180F` fader when it is aimed at ADPCM.
impl AudioSource for Adpcm {
    fn rate_hz(&self) -> f64 {
        OUTPUT_HZ as f64
    }

    fn gain(&self) -> f64 {
        self.gain
    }

    fn take_samples(&mut self, mclk: i64) -> Vec<(i32, i32)> {
        let Some(origin) = self.origin else {
            self.origin = Some(mclk);
            self.record = true;
            self.events.clear();
            self.held = self.last_output;
            return Vec::new();
        };
        let total = py_floordiv((mclk - origin) * OUTPUT_HZ, MASTER_HZ);
        let fader = self.fader.get();
        let mut used = 0;
        let mut out = Vec::with_capacity((total - self.taken).max(0) as usize);
        for n in self.taken..total {
            let at = origin + ceil_div(n * MASTER_HZ, OUTPUT_HZ);
            while used < self.events.len() && self.events[used].0 <= at {
                self.held = self.events[used].1;
                used += 1;
            }
            let value = round_ties_even(self.held as f64 * fader.gain(at, true)) as i32;
            out.push((value, value));
        }
        self.events.drain(..used);
        self.taken = total;
        out
    }
}
