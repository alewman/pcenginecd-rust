// SPDX-License-Identifier: MIT
//! The lockstep gate's Rust side: run a CD machine and write one line a frame (trace.rs), the
//! same lines `scripts/pcecd_trace.py` writes for the reference.
//!
//!     pcecd-trace CARD [--disc CUE] [--arcade] --frames N [options of trace::Setup]
//!                 [--detail FRAME] --out FILE
//!
//! With `--detail FRAME` only the bus cycles of the frame whose line would start with FRAME are
//! written, one a line (`r ADDRESS VALUE MCLK`, `w ...`, `i MCLK`).

use std::io::Write;

use pcengine_rust::trace::detail_lines;
use pcenginecd_rust::trace::{frame_line, Setup};

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let setup = Setup::parse(&args)?;
    if let Some(unknown) = setup.rest.first() {
        return Err(format!("unknown argument {unknown}"));
    }
    let mut system = setup.build()?;
    system.board_mut().bus_log = Some(Vec::new());
    let mut out: Box<dyn Write> = match &setup.out {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path).map_err(|error| format!("{path}: {error}"))?,
        )),
        None => Box::new(std::io::stdout().lock()),
    };
    let io = |error: std::io::Error| error.to_string();
    for _ in 0..setup.frames {
        setup.before_frame(&mut system);
        let wanted = setup.detail.map(|frame| frame == system.frame() + 1);
        if wanted == Some(true) {
            system.board_mut().bus_detail = Some(Vec::new());
        }
        system.run_frame();
        let line = frame_line(&mut system);
        match wanted {
            None => writeln!(out, "{line}").map_err(io)?,
            Some(true) => {
                for cycle in detail_lines(&mut system) {
                    writeln!(out, "{cycle}").map_err(io)?;
                }
                break;
            }
            Some(false) => {}
        }
    }
    out.flush().map_err(io)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("pcecd-trace: {error}");
        std::process::exit(1);
    }
}
