// SPDX-License-Identifier: MIT
//! The lockstep gate's side of this crate: how a traced CD machine is built from a command
//! line, and a frame's line.
//!
//! Not a module of the reference: its counterpart is `scripts/pcecd_trace.py`, which builds
//! the reference's `PcEngineCd` from the same arguments. A line is the base gate's
//! (`pcengine_rust::trace::frame_line`: the CPU bus log, the picture, the mixed audio and the
//! CPU state, digested) plus `cmd=N:SHA1`, the drive's command log of the frame. `roundtrip`
//! and `runahead` are the base crate's save-state checks over this machine (theirs build a
//! plain board).

use pcengine_rust::machine::{PC_ENGINE, TURBOGRAFX_16};
use pcengine_rust::pads::{self, Pad2};
use pcengine_rust::sockets::ResetKind;
use pcengine_rust::system::System;
use pcengine_rust::trace::{held, parse_press, Press};
use sha1::{Digest, Sha1};

use crate::board::{unit, CdSwitches, PcEngineCd};
use crate::disc::Disc;

/// The command line shared by `pcecd-trace`, `pcecd-state-roundtrip`, `pcecd-bench` and
/// `scripts/pcecd_trace.py`.
#[derive(Clone, Debug)]
pub struct Setup {
    /// the System Card image, or a card of our own
    pub card: Vec<u8>,
    /// a `.cue` (the disc is opened again for every machine built)
    pub disc: Option<String>,
    pub arcade: bool,
    pub frames: i64,
    pub presses: Vec<Press>,
    pub machine: String,
    pub super_system_card: Option<bool>,
    pub region_us: bool,
    pub bram: Option<Vec<u8>>,
    pub switches: CdSwitches,
    /// `Board::reset` of this kind before the frame with this count of completed frames
    pub resets: Vec<(i64, ResetKind)>,
    pub detail: Option<i64>,
    pub out: Option<String>,
    /// Arguments this parser does not know (a binary's own).
    pub rest: Vec<String>,
}

fn integer(text: &str) -> Result<i64, String> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => i64::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("not a number: {text}"))
}

fn real(text: &str) -> Result<f64, String> {
    text.parse().map_err(|_| format!("not a number: {text}"))
}

fn flag(text: &str) -> bool {
    matches!(text.to_lowercase().as_str(), "true" | "1")
}

fn list<const N: usize>(text: &str) -> Result<[i64; N], String> {
    let values: Vec<i64> = text.split(',').map(integer).collect::<Result<_, _>>()?;
    values
        .try_into()
        .map_err(|_| format!("{N} numbers, not {text}"))
}

/// One named switch by the reference's spelling, `WHERE.NAME=VALUE` (`scsi.SECTOR_CLOCKS=143181`).
pub fn set_switch(switches: &mut CdSwitches, spec: &str) -> Result<(), String> {
    let (target, text) = spec
        .split_once('=')
        .ok_or_else(|| format!("--switch WHERE.NAME=VALUE, not {spec}"))?;
    let s = switches;
    match target {
        "scsi.SEL_TO_COMMAND_PHASE" => s.scsi.sel_to_command_phase = integer(text)?,
        "scsi.COMMAND_BYTE_CLOCKS" => s.scsi.command_byte_clocks = integer(text)?,
        "scsi.TEST_UNIT_READY" => s.scsi.test_unit_ready = integer(text)?,
        "scsi.TOC_CLOCKS" => s.scsi.toc_clocks = integer(text)?,
        "scsi.SUBQ_CLOCKS" => s.scsi.subq_clocks = integer(text)?,
        "scsi.QUICK_ANSWER" => s.scsi.quick_answer = integer(text)?,
        "scsi.UNKNOWN_COMMAND_CLOCKS" => s.scsi.unknown_command_clocks = integer(text)?,
        "scsi.DATA_TO_STATUS" => s.scsi.data_to_status = integer(text)?,
        "scsi.STATUS_TO_MESSAGE" => s.scsi.status_to_message = integer(text)?,
        "scsi.MESSAGE_TO_FREE" => s.scsi.message_to_free = integer(text)?,
        "scsi.AUTO_ACK_CLOCKS" => s.scsi.auto_ack_clocks = integer(text)?,
        "scsi.SEEK_MEASUREMENT_EXTRA" => s.scsi.seek_measurement_extra = integer(text)?,
        "scsi.SECTOR_CLOCKS" => s.scsi.sector_clocks = integer(text)?,
        "scsi.REREAD_PENALTY_MS" => s.scsi.reread_penalty_ms = integer(text)?,
        "scsi.ABORT_STATUS_BYTES" => s.scsi.abort_status_bytes = integer(text)?,
        "scsi.UNKNOWN_COMMAND_CHECK_CONDITION" => {
            s.scsi.unknown_command_check_condition = flag(text)
        }
        "unit.CHANNEL_LATCH_GUARD" => s.unit.channel_latch_guard = integer(text)?,
        "unit.DMA_BYTE_CLOCKS" => s.unit.dma_byte_clocks = integer(text)?,
        "cdda.FADE_SLOW_SECONDS" => s.fader.fade_slow_seconds = real(text)?,
        "cdda.FADE_FAST_SECONDS" => s.fader.fade_fast_seconds = real(text)?,
        "cdda.PCETECH_SLOW_SECONDS" => s.fader.pcetech_slow_seconds = real(text)?,
        "cdda.PCETECH_FAST_SECONDS" => s.fader.pcetech_fast_seconds = real(text)?,
        "cdda.PCETECH_FADE" => s.fader.pcetech_fade = flag(text),
        "cdda.SUBQ_STATUS" => s.subq_status = Some(list::<4>(text)?.map(|n| n as u8)),
        "adpcm.BASE_HZ" => s.adpcm.base_hz = real(text)?,
        "adpcm.READ_CLOCKS" => s.adpcm.read_clocks = integer(text)?,
        "adpcm.WRITE_CLOCKS" => s.adpcm.write_clocks = integer(text)?,
        "adpcm.SATURATE" => s.adpcm.saturate = flag(text),
        "adpcm.OUTPUT_SCALE" => s.adpcm.output_scale = integer(text)? as i32,
        "adpcm.END_ON_WRITE_AT_ZERO" => s.adpcm.end_on_write_at_zero = flag(text),
        "arcade.UNMAPPED_READ" => s.arcade.unmapped_read = integer(text)? as u8,
        "arcade.SHIFT_READ_MASK" => s.arcade.shift_read_mask = integer(text)? as u8,
        "arcade.TRIGGER_REG_READ" => s.arcade.trigger_reg_read = integer(text)? as u8,
        "arcade.POWER_ON_RAM_BYTE" => s.arcade.power_on_ram_byte = integer(text)? as u8,
        "arcade.RESET_CLEARS_REGISTERS" => s.arcade.reset_clears_registers = flag(text),
        _ => return Err(format!("no switch {target}")),
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn open_disc(path: &str) -> Result<Disc, String> {
    Disc::from_cue_path(path)
}

/// The gate's command line names files; a page hands `PcEngineCd` a `Disc` of its own.
#[cfg(target_arch = "wasm32")]
fn open_disc(path: &str) -> Result<Disc, String> {
    Err(format!("{path}: no filesystem here"))
}

impl Setup {
    /// `CARD [--disc CUE] [--arcade] [--frames N] [--press SPEC]... [--machine pce|tg16]
    /// [--super yes|no] [--region-us] [--bram FILE] [--switch WHERE.NAME=VALUE]...
    /// [--reset FRAME:button|power]... [--detail FRAME] [--out FILE]`
    pub fn parse(args: &[String]) -> Result<Setup, String> {
        let mut setup = Setup {
            card: Vec::new(),
            disc: None,
            arcade: false,
            frames: 60,
            presses: Vec::new(),
            machine: "pce".to_string(),
            super_system_card: None,
            region_us: false,
            bram: None,
            switches: CdSwitches::default(),
            resets: Vec::new(),
            detail: None,
            out: None,
            rest: Vec::new(),
        };
        let mut path = None;
        let mut iter = args.iter();
        let file = |path: &String| std::fs::read(path).map_err(|error| format!("{path}: {error}"));
        while let Some(arg) = iter.next() {
            let mut value = || iter.next().ok_or_else(|| format!("{arg} needs a value"));
            match arg.as_str() {
                "--disc" => setup.disc = Some(value()?.clone()),
                "--arcade" => setup.arcade = true,
                "--frames" => setup.frames = integer(value()?)?,
                "--press" => setup.presses.push(parse_press(value()?)?),
                "--machine" => setup.machine = value()?.clone(),
                "--super" => setup.super_system_card = Some(value()? == "yes"),
                "--region-us" => setup.region_us = true,
                "--bram" => setup.bram = Some(file(value()?)?),
                "--switch" => set_switch(&mut setup.switches, value()?)?,
                "--reset" => {
                    let text = value()?;
                    let (frame, kind) = text
                        .split_once(':')
                        .ok_or_else(|| format!("--reset FRAME:button|power, not {text}"))?;
                    let kind = match kind {
                        "button" => ResetKind::Button,
                        "power" => ResetKind::PowerOn,
                        _ => return Err(format!("--reset FRAME:button|power, not {text}")),
                    };
                    setup.resets.push((integer(frame)?, kind));
                }
                "--detail" => setup.detail = Some(integer(value()?)?),
                "--out" => setup.out = Some(value()?.clone()),
                _ if path.is_none() && !arg.starts_with("--") => path = Some(arg.clone()),
                _ => setup.rest.push(arg.clone()),
            }
        }
        setup.card = file(&path.ok_or("no System Card image given")?)?;
        Ok(setup)
    }

    /// One more machine, built the same way each time, the drive's command log recorded.
    pub fn build(&self) -> Result<System, String> {
        let disc = match &self.disc {
            Some(path) => Some(open_disc(path)?),
            None => None,
        };
        let mut machine = PcEngineCd::new(&self.card, disc);
        machine.super_system_card = self.super_system_card;
        machine.region_us = self.region_us;
        machine.bram = self.bram.clone();
        machine.arcade_card = self.arcade;
        machine.switches = self.switches;
        machine.board.machine = match self.machine.as_str() {
            "pce" => PC_ENGINE,
            "tg16" => TURBOGRAFX_16,
            other => return Err(format!("no machine {other}: pce or tg16")),
        };
        machine.board.pad = Some(Box::new(Pad2::new()));
        let mut system = machine.build()?;
        if let Some(unit) = unit(system.board_mut()) {
            unit.bus.record_log = true;
        }
        Ok(system)
    }

    /// Before a frame runs: hold the buttons of the presses, and reset if this frame asks.
    pub fn before_frame(&self, system: &mut System) {
        let frame = system.frame();
        system
            .board_mut()
            .set_pressed(0, held(&self.presses, frame));
        for &(at, kind) in &self.resets {
            if at == frame {
                system.board_mut().reset(kind);
            }
        }
    }
}

/// The line of the frame that just ran: the base gate's, then the drive's commands.
pub fn frame_line(system: &mut System) -> String {
    let line = pcengine_rust::trace::frame_line(system);
    let mut commands = Sha1::new();
    let mut count = 0;
    if let Some(unit) = unit(system.board_mut()) {
        count = unit.bus.log.len();
        for (at, text) in unit.bus.log.drain(..) {
            commands.update(format!("{at} {text}\n"));
        }
    }
    let digest: String = commands
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{line} cmd={count}:{digest}")
}

/// Run `frames` frames with the setup's presses and resets; a line a frame.
pub fn run_frames(setup: &Setup, system: &mut System, frames: i64) -> Vec<String> {
    (0..frames)
        .map(|_| {
            setup.before_frame(system);
            system.run_frame();
            frame_line(system)
        })
        .collect()
}

fn started(setup: &Setup) -> Result<System, String> {
    let mut system = setup.build()?;
    system.board_mut().bus_log = Some(Vec::new());
    Ok(system)
}

fn settle(system: &mut System) {
    if let Some(log) = system.board_mut().bus_log.as_mut() {
        log.clear();
    }
    system.board_mut().take_audio();
    if let Some(unit) = unit(system.board_mut()) {
        unit.bus.log.clear();
    }
}

/// Save states, proven complete (the base crate's `trace::roundtrip`, the reference's
/// `scripts/cd_state.py`): power on, run to frame `at` plus `extra` instructions, save, then
/// build a second machine the same way (it opens the disc itself), load the state into it, and
/// run both to frame `to`. Every frame after the save must be the same on both, by the gate's
/// own lines. A third run loads the state into a machine that has run elsewhere first; and
/// the loaded machine, saved at once, must give the same bytes back.
pub fn roundtrip(setup: &Setup, at: i64, to: i64, extra: i64) -> Result<String, String> {
    use pcengine_rust::state::{load_state, save_state};
    let mut first = started(setup)?;
    run_frames(setup, &mut first, at);
    for _ in 0..extra {
        first.step();
    }
    let state = save_state(&first);
    settle(&mut first);
    let expected = run_frames(setup, &mut first, to - at);
    for (label, warm) in [
        ("a fresh machine", 0),
        ("a machine that had run", 1.max(at / 2)),
    ] {
        let mut second = started(setup)?;
        run_frames(setup, &mut second, warm);
        load_state(&mut second, &state).map_err(|error| format!("{label}: {error}"))?;
        if save_state(&second) != state {
            return Err(format!(
                "{label}: saved again at once, the state is not the same bytes"
            ));
        }
        settle(&mut second);
        let got = run_frames(setup, &mut second, to - at);
        for (n, (a, b)) in expected.iter().zip(&got).enumerate() {
            if a != b {
                return Err(format!(
                    "{label}: differs in frame {} after the save\n  expected {a}\n  got      {b}",
                    n + 1
                ));
            }
        }
    }
    Ok(format!(
        "{} bytes; {} frames after the save equal, twice",
        state.len(),
        to - at
    ))
}

/// The snapshot pair in a run-ahead pass (the base crate's `trace::runahead`): every frame is
/// run, a snapshot taken, `ahead` more frames run with other buttons held, and the snapshot put
/// back. Each frame's line, and the state at the end, must equal a plain run's.
pub fn runahead(setup: &Setup, from: i64, frames: i64, ahead: i64) -> Result<String, String> {
    use pcengine_rust::state::{load_snapshot, save_snapshot, save_state};
    let mut plain = started(setup)?;
    run_frames(setup, &mut plain, from);
    let expected = run_frames(setup, &mut plain, frames);
    let mut system = started(setup)?;
    run_frames(setup, &mut system, from);
    let mut bytes = 0;
    for (n, wanted) in expected.iter().enumerate() {
        setup.before_frame(&mut system);
        system.run_frame();
        let line = frame_line(&mut system);
        if &line != wanted {
            return Err(format!(
                "frame {} differs after a snapshot was put back\n  expected {wanted}\n  got      {line}",
                n + 1
            ));
        }
        let snapshot = save_snapshot(&system);
        bytes = snapshot.len();
        for _ in 0..ahead {
            system
                .board_mut()
                .set_pressed(0, pads::RUN | pads::I | pads::RIGHT);
            system.run_frame();
        }
        load_snapshot(&mut system, &snapshot)?;
        settle_logs(&mut system);
    }
    if save_state(&system) != save_state(&plain) {
        return Err("the end state is not the plain run's".to_string());
    }
    Ok(format!(
        "{frames} frames from {from}, {ahead} ahead of each, equal to the plain run; a snapshot is {bytes} bytes"
    ))
}

fn settle_logs(system: &mut System) {
    if let Some(log) = system.board_mut().bus_log.as_mut() {
        log.clear();
    }
    if let Some(unit) = unit(system.board_mut()) {
        unit.bus.log.clear();
    }
}
