// SPDX-License-Identifier: MIT
//! docs/notes-on-the-reference.md: note 1, behaviour of pcenginecd-python v0.1.0 that read as
//! unintended, settled in its v0.2.0 (issue 0001) and followed here. `scripts/reference_notes.py`
//! prints the same numbers from the reference.

mod common;

use common::{disc_audio_then_data, Host, MASTER_HZ};
use pcenginecd_rust::unit::CdUnit;

/// Master clocks from a SEL write to the command phase, looked for every 12 clocks.
fn sel_to_command(prepare: fn(&mut Host)) -> i64 {
    let mut h = Host::new(CdUnit::new(Some(disc_audio_then_data(400, 20))));
    prepare(&mut h);
    h.advance(MASTER_HZ); // a second passes: `advance` is the board's catch_up
    h.select();
    let began = h.t;
    while h.read(0x1FF800, h.t) & 0xF8 != 0xD0 {
        h.t += 12;
        h.advance(0);
    }
    h.t - began
}

/// Note 1, settled. In v0.1.0 the drive's clock stood still between events (0 here with CD
/// audio playing); a register access now runs the drive to its own time (`run_now`), so the
/// timer a SEL write starts counts from the write either way.
#[test]
fn the_drives_clock_runs_to_every_register_access() {
    // ScsiSwitches::sel_to_command_phase: 75,000 at v0.2.0, 74,940 since the clean rewrite
    assert_eq!(sel_to_command(|_| {}), 74_940);
    let music = |h: &mut Host| {
        h.command(&[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]); // AUDIO START: track 1 (5.3 s)
        h.finish();
    };
    assert_eq!(sel_to_command(music), 74_940); // v0.1.0: 0
}
