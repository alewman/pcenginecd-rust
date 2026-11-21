// SPDX-License-Identifier: MIT
//! What a run touched, frame by frame: IRQ2 entries (the probe's handler reads `$1803`, `$1800`
//! and `$180C` in a row), frames with sound from each of the unit's sources, the ADPCM counters.
//! A tool for writing the probe card (scripts/make_cdprobe.py), not part of the gate.
//!
//!     cargo run --release --example probe_report -- CARD [options of trace::Setup]

use pcengine_rust::sockets::PortDevice;
use pcenginecd_rust::board::unit;
use pcenginecd_rust::trace::Setup;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let setup = Setup::parse(&args).expect("arguments");
    let mut system = setup.build().expect("a machine");
    let mut irqs: Vec<(i64, u8)> = Vec::new();
    let mut sound = [Vec::new(), Vec::new()];
    for _ in 0..setup.frames {
        setup.before_frame(&mut system);
        system.board_mut().bus_detail = Some(Vec::new());
        system.run_frame();
        let frame = system.frame();
        let detail = system.board_mut().bus_detail.take().unwrap();
        let registers: Vec<_> = detail
            .iter()
            .filter(|cycle| cycle.0 == 0 && cycle.1 >> 8 == 0x1FF8)
            .collect();
        for window in registers.windows(3) {
            if window[0].1 == 0x1FF803 && window[1].1 == 0x1FF800 && window[2].1 == 0x1FF80C {
                irqs.push((frame, window[0].2));
            }
        }
        let mclk = system.board().mclk;
        let unit = unit(system.board_mut()).unwrap();
        for (n, source) in unit.audio_sources().into_iter().enumerate() {
            if source.take_samples(mclk).iter().any(|&s| s != (0, 0)) {
                sound[n].push(frame);
            }
        }
    }
    let ranges = |frames: &[i64]| {
        let mut out: Vec<(i64, i64)> = Vec::new();
        for &frame in frames {
            match out.last_mut() {
                Some(last) if last.1 + 1 == frame => last.1 = frame,
                _ => out.push((frame, frame)),
            }
        }
        out
    };
    println!("IRQ2 entries (frame, $1803): {irqs:02x?}");
    println!("CD audio in frames {:?}", ranges(&sound[0]));
    println!("ADPCM in frames {:?}", ranges(&sound[1]));
    let unit = unit(system.board_mut()).unwrap();
    println!(
        "ADPCM samples {}, ends {}; bus phase {}; active {:02x} enable {:02x}",
        unit.adpcm.sample_count,
        unit.adpcm.end_count,
        unit.bus.phase.value(),
        unit.active,
        unit.enable
    );
}
