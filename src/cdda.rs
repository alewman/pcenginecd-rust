// SPDX-License-Identifier: MIT
//! CD-DA: the audio player behind `AUDIO START/END/PAUSE`, `READ SUBQ`, `$1805-$1806` and the
//! audio source the board mixes (`cdda.py`).
//!
//! Time is exact and lazy: the player keeps an anchor (the master clock at which a given sample
//! of a given sector played) and computes the position for any `mclk`, so playback does not
//! depend on how often the host asks for samples. 44,100 stereo samples a second, 588 per
//! 2352-byte sector, 75 sectors a second [hw: Red Book]. Samples are signed 16-bit little-endian
//! pairs.
//!
//! What is not modelled (U9): the drive's subchannel Q stream (the BIOS's `$1807` read and its
//! IRQ bit 0x10), the DAC's filtering (emulators differ; none is followed), and the real fader
//! curve: the times are the Hudson BIOS manual's 6 s / 2.5 s (tier 3; pcetech's tier-2 "about 8
//! seconds" / "about 2 seconds" is the `pcetech_fade` switch) and the shape between is linear
//! here (MAME calls its own fader "sketchy and unchecked against real HW").
//!
//! Differences forced by Rust:
//!
//! - **The fader is its own value** (`Fader`), shared by the player and the ADPCM unit through
//!   an `Rc<Cell<_>>`: both are audio sources the mixer borrows at once, and the reference's
//!   ADPCM source calls `unit.cdda.fade_gain`. The reference's class attributes (`FADE_*`,
//!   `PCETECH_*`) are the fader's fields; `SUBQ_STATUS` is the player's `subq_status`.
//! - **`on_good_status`** (the reference's callback into the SCSI bus) is a count,
//!   `good_status_due`, that the bus takes after each `run_to` and answers in the same place.
//!   `take_samples` (the mixer's call) also runs the player's events, as the reference's does;
//!   the board has always run the unit up to its clock before it mixes, so nothing fires there.
//! - The decoded sector kept between samples (`_cache`) is not part of a save state.

use std::cell::Cell;
use std::rc::Rc;

use pcengine_rust::machine::{py_floordiv, MASTER_HZ};
use pcengine_rust::sockets::{AudioSource, DeviceState};
use serde::{Deserialize, Serialize};

use crate::disc::Disc;

pub const SAMPLE_HZ: i64 = 44_100;
pub const SAMPLES_PER_SECTOR: i64 = 588;

/// Python's `-(-a // b)`: the quotient rounded toward plus infinity.
#[inline]
pub(crate) fn ceil_div(a: i64, b: i64) -> i64 {
    -py_floordiv(-a, b)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Playing,
    Paused,
    /// idle: nothing started, or stopped / a data read took the head
    Stopped,
    /// moving to the start; becomes `Playing` or `Paused`
    Seeking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndMode {
    Stop = 3,
    Loop = 1,
    /// stop at the end and give the busy command its good status
    Irq = 2,
}

/// The `$180F` fader: the register, when its timer started, and the fade times.
///
/// Fade times, linear. Default: [mfr: Hudson BIOS manual CD_FADE "6.0 s / 2.5 s", tier 3; ares
/// (ISC) uses the same times]. The manufacturer's manual outranks pcetech's research text.
/// Switch: `pcetech_fade = true` selects [hw: pcetech 8] "silence in about 8 seconds" (value
/// 8-9) and "about 2 seconds" (C-D), tier 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fader {
    pub fade_register: u8,
    pub fade_start: i64,
    pub fade_slow_seconds: f64,
    pub fade_fast_seconds: f64,
    pub pcetech_slow_seconds: f64,
    pub pcetech_fast_seconds: f64,
    pub pcetech_fade: bool,
}

impl Default for Fader {
    fn default() -> Self {
        Fader {
            fade_register: 0,
            fade_start: 0,
            fade_slow_seconds: 6.0,
            fade_fast_seconds: 2.5,
            pcetech_slow_seconds: 8.0,
            pcetech_fast_seconds: 2.0,
            pcetech_fade: false,
        }
    }
}

impl Fader {
    /// The `$180F` fader's gain for CD-DA, or for ADPCM (`adpcm`: the fade is aimed at it by
    /// bit 1; pcetech did not test the ADPCM fade, the Hudson BIOS manual's CD_FADE lists $0A
    /// and $0E for it: U9/M8).
    pub fn gain(&self, mclk: i64, adpcm: bool) -> f64 {
        if self.fade_register & 0x08 == 0 || (self.fade_register & 0x02 != 0) != adpcm {
            return 1.0; // no fade, or it is aimed at the other source
        }
        let fast = self.fade_register & 0x04 != 0;
        let seconds = match (self.pcetech_fade, fast) {
            (true, true) => self.pcetech_fast_seconds,
            (true, false) => self.pcetech_slow_seconds,
            (false, true) => self.fade_fast_seconds,
            (false, false) => self.fade_slow_seconds,
        };
        (1.0 - (mclk - self.fade_start) as f64 / (seconds * MASTER_HZ as f64)).max(0.0)
    }
}

#[derive(Serialize, Deserialize)]
pub struct CddaPlayer {
    #[serde(skip)]
    pub disc: Option<Rc<Disc>>,
    /// The bus owes this many `good_status()` (the reference's `on_good_status` callback).
    pub good_status_due: u32,
    pub status: Status,
    /// Where the last AUDIO START put the head: where a repeat goes back to (the reference's
    /// read-only `start`; the bus reads it for AUDIO END's empty-range rule).
    pub start: i64,
    pub end: i64,
    pub end_mode: EndMode,
    pub anchor_clock: i64,
    /// absolute sample number (sector * 588 + offset) at the anchor
    pub anchor_sample: i64,
    /// seek done, or the end of the range
    pub event_at: Option<i64>,
    pub now: i64,
    /// samples handed out so far, in absolute stream time
    pub taken: i64,
    pub take_origin: Option<i64>,
    #[serde(skip)]
    cache: Option<(i64, Vec<i16>)>,
    #[serde(skip)]
    pub fader: Rc<Cell<Fader>>,
    /// AudioSource socket: the mixer's gain for this source
    pub gain: f64,
    pub paused_after_seek: bool,
    /// The audio-status byte of READ SUBQ [DOC: Hudson BIOS manual CD_SUBQ: $00 playing, $01
    /// still, $02 pause, $03 not playing; a seek reads as not playing, a choice], for playing,
    /// paused, stopped, seeking (the reference's `SUBQ_STATUS`)
    #[serde(skip, default = "default_subq_status")]
    pub subq_status: [u8; 4],
}

fn default_subq_status() -> [u8; 4] {
    [0, 2, 3, 3]
}

impl Default for CddaPlayer {
    fn default() -> Self {
        CddaPlayer::new()
    }
}

impl CddaPlayer {
    pub fn new() -> CddaPlayer {
        CddaPlayer {
            disc: None,
            good_status_due: 0,
            status: Status::Stopped,
            start: 0,
            end: 0,
            end_mode: EndMode::Stop,
            anchor_clock: 0,
            anchor_sample: 0,
            event_at: None,
            now: 0,
            taken: 0,
            take_origin: None,
            cache: None,
            fader: Rc::new(Cell::new(Fader::default())),
            gain: 1.0,
            paused_after_seek: false,
            subq_status: default_subq_status(),
        }
    }

    /// Take a decoded state's place: the wiring (the disc, the shared fader) and the switch
    /// stay this player's.
    pub(crate) fn restore(&mut self, mut saved: CddaPlayer) {
        saved.disc = self.disc.take();
        saved.fader = Rc::clone(&self.fader);
        saved.subq_status = self.subq_status;
        *self = saved;
    }

    // -- state the bus reads ---------------------------------------------------------------------

    pub fn active(&self) -> bool {
        self.status != Status::Stopped
    }

    pub fn status_code(&self) -> u8 {
        self.subq_status[match self.status {
            Status::Playing => 0,
            Status::Paused => 1,
            Status::Stopped => 2,
            Status::Seeking => 3,
        }]
    }

    /// The sector under the head at the last time the bus ran (`now`).
    pub fn position(&self) -> i64 {
        py_floordiv(self.sample_at(self.now), SAMPLES_PER_SECTOR)
    }

    fn sample_at(&self, mclk: i64) -> i64 {
        if self.status == Status::Playing {
            return self.anchor_sample
                + py_floordiv((mclk - self.anchor_clock).max(0) * SAMPLE_HZ, MASTER_HZ);
        }
        self.anchor_sample
    }

    // -- commands --------------------------------------------------------------------------------

    /// AUDIO START: seek to `start`, then play to the end of its track (or wait for an AUDIO
    /// END if `paused`). The bus holds BUSY until the seek is done.
    pub fn play(&mut self, start: i64, paused: bool, seek_clocks: i64) {
        let Some(disc) = self.disc.as_ref() else {
            return;
        };
        let end = disc.track_at(start).map_or(disc.sector_count, |t| t.end);
        self.start = start;
        self.end = end - 1;
        self.end_mode = EndMode::Stop;
        self.anchor_sample = start * SAMPLES_PER_SECTOR;
        self.anchor_clock = self.now + seek_clocks;
        self.event_at = Some(self.anchor_clock);
        self.paused_after_seek = paused;
        self.status = Status::Seeking;
    }

    /// AUDIO END (modes 1-3): play (or resume) up to sector `end` inclusive.
    pub fn set_end(&mut self, end: i64, mode: u8) {
        let Some(disc) = self.disc.as_ref() else {
            return;
        };
        self.end = end.min(disc.sector_count - 1);
        self.end_mode = match mode {
            1 => EndMode::Loop,
            2 => EndMode::Irq,
            _ => EndMode::Stop,
        };
        match self.status {
            Status::Paused | Status::Stopped => self.resume(),
            Status::Seeking => self.paused_after_seek = false,
            Status::Playing => {}
        }
        self.schedule_end();
    }

    fn resume(&mut self) {
        self.anchor_clock = self.now;
        self.status = Status::Playing;
    }

    pub fn pause(&mut self) {
        if self.status == Status::Playing {
            self.anchor_sample = self.sample_at(self.now);
            self.status = Status::Paused;
            self.event_at = None;
        } else if self.status == Status::Seeking {
            self.paused_after_seek = true;
        }
    }

    pub fn stop(&mut self) {
        if self.status == Status::Playing {
            self.anchor_sample = self.sample_at(self.now);
        }
        self.status = Status::Stopped;
        self.event_at = None;
    }

    /// A data read took the head: audio stops.
    pub fn idle(&mut self) {
        self.stop();
    }

    fn schedule_end(&mut self) {
        if self.status != Status::Playing {
            self.event_at = (self.status == Status::Seeking).then_some(self.anchor_clock);
            return;
        }
        let remaining = (self.end + 1) * SAMPLES_PER_SECTOR - self.anchor_sample;
        self.event_at = Some(self.anchor_clock + ceil_div(remaining * MASTER_HZ, SAMPLE_HZ));
    }

    // -- time ------------------------------------------------------------------------------------

    pub fn next_event(&self) -> Option<i64> {
        self.event_at
    }

    /// Fire the seek-done and end-of-range events due at or before `mclk`.
    pub fn run_to(&mut self, mclk: i64) {
        while let Some(at) = self.event_at.filter(|&at| at <= mclk) {
            self.event_at = None;
            self.now = at;
            if self.status == Status::Seeking {
                self.status = if self.paused_after_seek {
                    Status::Paused
                } else {
                    Status::Playing
                };
                self.anchor_clock = at;
                self.schedule_end();
                self.good_status_due += 1;
            } else if self.status == Status::Playing {
                if self.end_mode == EndMode::Loop {
                    self.anchor_sample = self.start * SAMPLES_PER_SECTOR;
                    self.anchor_clock = at;
                    self.schedule_end();
                } else {
                    self.anchor_sample = (self.end + 1) * SAMPLES_PER_SECTOR;
                    self.status = Status::Stopped;
                    if self.end_mode == EndMode::Irq {
                        self.good_status_due += 1;
                    }
                }
            }
        }
        self.now = self.now.max(mclk);
    }

    // -- samples ---------------------------------------------------------------------------------

    fn frame(&mut self, absolute_sample: i64) -> (i16, i16) {
        let lba = py_floordiv(absolute_sample, SAMPLES_PER_SECTOR);
        let offset = absolute_sample.rem_euclid(SAMPLES_PER_SECTOR) as usize;
        if self.cache.as_ref().is_none_or(|(cached, _)| *cached != lba) {
            let mut values = vec![0i16; SAMPLES_PER_SECTOR as usize * 2];
            if let Some(disc) = self.disc.as_ref() {
                if disc.track_at(lba).is_some_and(|track| !track.is_data) {
                    let raw = disc.read_raw(lba);
                    for (value, pair) in values.iter_mut().zip(raw.chunks_exact(2)) {
                        *value = i16::from_le_bytes([pair[0], pair[1]]);
                    }
                }
            }
            self.cache = Some((lba, values));
        }
        let values = &self.cache.as_ref().expect("just filled").1;
        (values[2 * offset], values[2 * offset + 1])
    }

    /// The sample being played at the last time run (`$1805/$1806`): zeros unless playing.
    pub fn latest_sample(&mut self) -> (i16, i16) {
        if self.status != Status::Playing || self.disc.is_none() {
            return (0, 0);
        }
        self.frame(self.sample_at(self.now))
    }

    /// The fader's gain (the reference's `fade_gain`).
    pub fn fade_gain(&self, mclk: i64, adpcm: bool) -> f64 {
        self.fader.get().gain(mclk, adpcm)
    }

    pub fn fade_register(&self) -> u8 {
        self.fader.get().fade_register
    }

    /// `$180F` [hw: pcetech 8]: bit 3 fades, bit 2 chooses the fast fade, bit 1 aims it at
    /// ADPCM. Rewriting the same value changes nothing; a new value restarts the timer (the
    /// finer cases in pcetech, a cancelled fade whose timer still runs, are U9).
    pub fn write_fade(&mut self, value: u8, mclk: i64) {
        let mut fader = self.fader.get();
        if value & 0x0F != fader.fade_register & 0x0F {
            fader.fade_start = mclk;
        }
        fader.fade_register = value;
        self.fader.set(fader);
    }

    /// Was audio playing at `mclk`? (Past events have already moved the state, so a sample
    /// before the current anchor or after the end is silence.)
    fn plays_at(&self, mclk: i64) -> bool {
        self.status == Status::Playing && mclk >= self.anchor_clock
    }
}

impl DeviceState for CddaPlayer {} // saved with the unit

impl AudioSource for CddaPlayer {
    /// AudioSource attributes (socket 9): a fixed rate; the mixer applies `gain`
    fn rate_hz(&self) -> f64 {
        SAMPLE_HZ as f64
    }

    fn gain(&self) -> f64 {
        self.gain
    }

    /// Stereo samples since the last call, one per 1/44100 s of master-clock time (zeros when
    /// not playing, so a mixer keeps its place). The fader's gain is applied.
    fn take_samples(&mut self, mclk: i64) -> Vec<(i32, i32)> {
        self.run_to(mclk);
        let origin = *self.take_origin.get_or_insert(mclk);
        let total = py_floordiv((mclk - origin) * SAMPLE_HZ, MASTER_HZ);
        let mut out = Vec::with_capacity((total - self.taken).max(0) as usize);
        for n in self.taken..total {
            let at = origin + ceil_div(n * MASTER_HZ, SAMPLE_HZ);
            if self.disc.is_some() && self.plays_at(at) {
                let (left, right) = self.frame(self.sample_at(at));
                let g = self.fade_gain(at, false);
                // Python's int(): toward zero
                out.push(((left as f64 * g) as i32, (right as f64 * g) as i32));
            } else {
                out.push((0, 0));
            }
        }
        self.taken = total;
        out
    }
}
