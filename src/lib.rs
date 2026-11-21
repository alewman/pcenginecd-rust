// SPDX-License-Identifier: MIT
//! A Rust PC Engine CD-ROM² / Super CD-ROM² / Arcade Card, transcribed from
//! [pcenginecd-python](https://github.com/alewman/pcenginecd-python) at `v0.1.0` (`5fec99e`) as
//! an add-on to [pcengine-rust](../pcengine_rust/index.html): the System Card in the HuCard
//! slot, the CD unit on the expansion port, through the base board's socket traits and nothing
//! copied from it.
//!
//! Each module is the transcription of the `pcenginecd_python` module of the same name, with
//! every named switch as a field with the reference's default; where Rust forced a difference
//! the module header says so. `disc` is the one module designed for the browser: the sector
//! store is a trait and the cue sheet is parsed from text.
//!
//! ```
//! use pcenginecd_rust::board::{self, PcEngineCd};
//! use pcenginecd_rust::disc::Disc;
//!
//! let system_card = vec![0xEAu8; 256 * 1024]; // the BIOS image (bring your own)
//! let disc = Disc::from_memory(vec![(true, vec![0u8; 2352 * 16])]).unwrap();
//! let mut machine = PcEngineCd::new(&system_card, Some(disc));
//! machine.arcade_card = true;
//! let mut system = machine.build().unwrap();
//! system.run_frame();
//! let _samples = system.board_mut().take_audio(); // PSG + CD audio + ADPCM, 44.1 kHz
//! let _backup: Vec<u8> = board::unit(system.board_mut()).unwrap().bram().to_vec();
//! ```

pub mod adpcm;
pub mod arcade;
pub mod board;
pub mod cdda;
pub mod disc;
pub mod scsi;
pub mod seek;
pub mod syscard;
pub mod trace;
pub mod unit;

pub use arcade::ArcadeCard;
pub use board::PcEngineCd;
pub use disc::{Disc, Track};
pub use pcengine_rust;
pub use syscard::SystemCard;
pub use unit::CdUnit;

/// The README's example, compiled with the documentation tests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeExample;
