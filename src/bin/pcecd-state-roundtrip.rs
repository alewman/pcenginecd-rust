// SPDX-License-Identifier: MIT
//! Rung 6: save states proven by equivalence, and the snapshot pair in a run-ahead pass.
//!
//!     pcecd-state-roundtrip CARD [options of trace::Setup] [--at 480] [--to 540] [--extra 777]
//!                           [--ahead 2] [--ahead-frames 30]
//!
//! `trace::roundtrip` (the reference's scripts/cd_state.py) and `trace::runahead` from frame
//! `--at`.

use pcenginecd_rust::trace::{roundtrip, runahead, Setup};

fn run() -> Result<String, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let setup = Setup::parse(&args)?;
    let (mut at, mut to, mut extra, mut ahead, mut ahead_frames) = (480, 540, 777, 2, 30);
    let mut rest = setup.rest.iter();
    while let Some(arg) = rest.next() {
        let value: i64 = rest
            .next()
            .and_then(|text| text.parse().ok())
            .ok_or_else(|| format!("{arg} needs a number"))?;
        match arg.as_str() {
            "--at" => at = value,
            "--to" => to = value,
            "--extra" => extra = value,
            "--ahead" => ahead = value,
            "--ahead-frames" => ahead_frames = value,
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let saved = roundtrip(&setup, at, to, extra)?;
    let looked = runahead(&setup, at, ahead_frames, ahead)?;
    Ok(format!("{saved}; run-ahead: {looked}"))
}

fn main() {
    match run() {
        Ok(what) => println!("ok: {what}"),
        Err(error) => {
            println!("FAIL: {error}");
            std::process::exit(1);
        }
    }
}
