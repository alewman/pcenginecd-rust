// SPDX-License-Identifier: MIT
//! The reference's `tests/discs.py` and `tests/helpers.py`: synthetic discs (nothing here is a
//! real disc, a real sector, or anything copied), a host that talks to the CD unit the way the
//! System Card does (handshakes through `$1800-$1802`, `$1801`/`$1808`), and the base crate's
//! scripted CPU for the board-level tests.
#![allow(dead_code)]

use pcengine_rust::sockets::{DetachedPins, PageHandler, PortDevice};
use pcenginecd_rust::disc::{to_bcd, Disc, RAW_SECTOR, SYNC};
use pcenginecd_rust::unit::CdUnit;

pub const MASTER_HZ: i64 = 21_477_272;

/// A Mode 1 raw sector: sync, header (MSF, mode 1), 2048 payload bytes, zero EDC/ECC.
pub fn data_sector(lba: i64, payload: &[u8]) -> Vec<u8> {
    let (minutes, seconds, frames) = (
        (lba + 150) / 4500,
        ((lba + 150) / 75) % 60,
        (lba + 150) % 75,
    );
    let mut out = SYNC.to_vec();
    out.extend([to_bcd(minutes), to_bcd(seconds), to_bcd(frames), 1]);
    out.extend(
        payload
            .iter()
            .copied()
            .chain(std::iter::repeat(0))
            .take(2048),
    );
    out.resize(RAW_SECTOR, 0);
    out
}

/// `count` data sectors whose payload bytes are (lba + index) & 0xFF: easy to assert.
pub fn numbered_data(first_lba: i64, count: i64) -> Vec<u8> {
    (0..count)
        .flat_map(|n| {
            let payload: Vec<u8> = (0..2048).map(|i| (first_lba + n + i) as u8).collect();
            data_sector(first_lba + n, &payload)
        })
        .collect()
}

/// `count` audio sectors of two sine waves (a different one each channel).
pub fn audio_sectors(count: i64) -> Vec<u8> {
    let (left_hz, right_hz) = (441.0, 882.0);
    let mut out = Vec::new();
    for n in 0..count * 588 {
        let phase = 2.0 * std::f64::consts::PI * n as f64 / 44100.0;
        let left = (12000.0 * (left_hz * phase).sin()) as i16;
        let right = (12000.0 * (right_hz * phase).sin()) as i16;
        out.extend(left.to_le_bytes());
        out.extend(right.to_le_bytes());
    }
    out
}

pub fn disc_data_only(sectors: i64) -> Disc {
    Disc::from_memory(vec![(true, numbered_data(0, sectors))]).unwrap()
}

/// Track 1 audio, track 2 data (as Insanity), then track 3 audio.
pub fn disc_audio_then_data(audio: i64, data: i64) -> Disc {
    Disc::from_memory(vec![
        (false, audio_sectors(audio)),
        (true, numbered_data(audio, data)),
        (false, audio_sectors(30)),
    ])
    .unwrap()
}

pub const REG: u32 = 0x1FF800;

/// Register-level driver with its own master clock and an IRQ2 line of its own (the
/// reference's `Host`, and the `IrqLine` its tests bind the unit to).
pub struct Host {
    pub unit: CdUnit,
    pub pins: DetachedPins,
    pub t: i64,
}

impl Host {
    pub fn new(unit: CdUnit) -> Host {
        Host {
            unit,
            pins: DetachedPins::default(),
            t: 0,
        }
    }

    /// The level of the IRQ2 line the unit is bound to.
    pub fn irq(&self) -> bool {
        self.pins.irq2.level()
    }

    pub fn read(&mut self, address: u32, mclk: i64) -> u8 {
        self.unit.read(address, mclk, &mut self.pins.pins())
    }

    pub fn write(&mut self, address: u32, value: u8, mclk: i64) {
        self.unit.write(address, value, mclk, &mut self.pins.pins());
    }

    pub fn r(&mut self, reg: u32) -> u8 {
        self.t += 12;
        self.read(REG + reg, self.t)
    }

    pub fn w(&mut self, reg: u32, value: u8) {
        self.t += 12;
        self.write(REG + reg, value, self.t);
    }

    pub fn advance(&mut self, clocks: i64) {
        self.t += clocks;
        self.unit.catch_up(self.t, &mut self.pins.pins());
    }

    pub fn status(&mut self) -> u8 {
        self.r(0) & 0xF8
    }

    /// Advance until `$1800 & 0xF8 == want`; returns the clocks waited.
    pub fn wait(&mut self, want: u8) -> i64 {
        self.wait_mask(want, 0xF8)
    }

    pub fn wait_mask(&mut self, want: u8, mask: u8) -> i64 {
        let start = self.t;
        while self.r(0) & mask != want {
            self.advance(3000);
            assert!(
                self.t - start < 400_000_000,
                "timeout waiting for {want:02X}, now {:02X}",
                self.r(0)
            );
        }
        self.t - start
    }

    /// ACK up, REQ drops, ACK down (the BIOS's $EC0B shape).
    pub fn handshake(&mut self) {
        self.w(2, 0x80 | self.unit.enable);
        self.wait_mask(0, 0x40);
        self.w(2, self.unit.enable);
    }

    pub fn select(&mut self) {
        self.w(0, 0);
    }

    pub fn command(&mut self, cmd: &[u8]) {
        self.select();
        self.wait(0xD0); // BSY CD REQ, no I/O: command phase
        for (i, &byte) in cmd.iter().enumerate() {
            self.w(1, byte);
            self.handshake();
            if i + 1 < cmd.len() {
                self.wait(0xD0);
            }
        }
    }

    /// Take `count` bytes in data-in (REQ + I/O, no C/D); `via` is the register.
    pub fn read_data_bytes(&mut self, count: usize, via: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for _ in 0..count {
            self.wait(0xC8);
            out.push(self.r(via));
            if via == 1 {
                self.handshake();
            } else {
                self.advance(60); // the auto-ack pulse (21 clocks) drops REQ and raises it again
            }
        }
        out
    }

    /// Take the status byte and the message byte; the bus ends free.
    pub fn finish(&mut self) -> (u8, u8) {
        self.wait(0xD8);
        let status = self.r(1);
        self.handshake();
        self.wait(0xF8);
        let message = self.r(1);
        self.handshake();
        self.wait_mask(0, 0xF8);
        (status, message)
    }

    pub fn run(&mut self, cmd: &[u8], count: usize, via: u32) -> (Vec<u8>, (u8, u8)) {
        self.command(cmd);
        let data = if count != 0 {
            self.read_data_bytes(count, via)
        } else {
            Vec::new()
        };
        (data, self.finish())
    }
}

/// Bank number in the first byte of every 8 KiB bank, so the mirroring can be read back.
pub fn card_image(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    for bank in 0..size / 8192 {
        data[bank * 8192] = bank as u8;
    }
    data
}
