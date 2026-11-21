// SPDX-License-Identifier: MIT
//! Frames per second of this machine on a disc, nothing logged or digested.
//!
//!     pcecd-bench CARD [--disc CUE] [--arcade] [--frames N] [options of trace::Setup]
//!
//! Speed is not this crate's goal (the fast fork's); `scripts/bench_cd.py` times the reference
//! the same way.

use std::time::Instant;

use pcenginecd_rust::board::unit;
use pcenginecd_rust::trace::Setup;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let setup = match Setup::parse(&args) {
        Ok(setup) => setup,
        Err(error) => {
            eprintln!("pcecd-bench: {error}");
            std::process::exit(1);
        }
    };
    let mut system = setup.build().expect("a machine");
    if let Some(unit) = unit(system.board_mut()) {
        unit.bus.record_log = false;
    }
    let began = Instant::now();
    let mut samples = 0;
    for _ in 0..setup.frames {
        setup.before_frame(&mut system);
        system.run_frame();
        samples += system.board_mut().take_audio().len();
    }
    let seconds = began.elapsed().as_secs_f64();
    println!(
        "{} frames in {:.3} s: {:.1} frames per second ({} audio samples, mclk {})",
        setup.frames,
        seconds,
        setup.frames as f64 / seconds,
        samples,
        system.board().mclk
    );
}
