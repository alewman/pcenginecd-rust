// SPDX-License-Identifier: MIT
//! Save states: the base board's `state.rs` over the CD machine (unit, drive, ADPCM, Arcade
//! Card), the reference's tests/test_state.py, and the refusals. The card here is NOPs; the
//! runs that exercise the drive are rung 6's (scripts/rung6.sh).

mod common;

use common::{disc_audio_then_data, disc_data_only, Host};
use pcengine_rust::pads::Pad2;
use pcengine_rust::sockets::{DeviceState, PortDevice};
use pcengine_rust::state::{load_snapshot, load_state, save_snapshot, save_state};
use pcengine_rust::system::System;
use pcenginecd_rust::arcade::ArcadeCard;
use pcenginecd_rust::board::{arcade, card, unit, PcEngineCd};
use pcenginecd_rust::disc::Disc;
use pcenginecd_rust::unit::CdUnit;

/// A System Card that is only NOPs and its reset vector (to $E000).
fn image(size: usize) -> Vec<u8> {
    let mut data = vec![0xEA; size];
    data[size - 2] = 0x00;
    data[size - 1] = 0xE0;
    data
}

fn running(disc: Option<Disc>, arcade_card: bool) -> System {
    let mut cd = PcEngineCd::new(
        &image(256 * 1024),
        Some(disc.unwrap_or_else(|| disc_data_only(40))),
    );
    cd.arcade_card = arcade_card;
    cd.board.pad = Some(Box::new(Pad2::new()));
    cd.build().unwrap()
}

#[test]
fn the_unit_the_adpcm_ram_and_the_arcade_card_come_back() {
    let mut system = running(None, true);
    let board = system.board_mut();
    unit(board).unwrap().adpcm.ram[0x1234] = 0x5A;
    unit(board).unwrap().adpcm.write_ptr = 0x2345;
    arcade(board).unwrap().ram[0x1F_0000] = 0xA5;
    arcade(board).unwrap().base[2] = 0x12_3456;
    card(board).unwrap().ram.as_mut().unwrap()[0x2_0000] = 0x3C;
    for _ in 0..2000 {
        system.step();
    }
    let mut loaded = running(None, true);
    load_state(&mut loaded, &save_state(&system)).unwrap();
    let other = loaded.board_mut();
    assert!(unit(other).unwrap().adpcm.ram[0x1234] == 0x5A);
    assert!(unit(other).unwrap().adpcm.write_ptr == 0x2345);
    assert!(arcade(other).unwrap().ram[0x1F_0000] == 0xA5);
    assert!(arcade(other).unwrap().base[2] == 0x12_3456);
    assert!(card(other).unwrap().ram.as_ref().unwrap()[0x2_0000] == 0x3C);
    assert_eq!(unit(other).unwrap().audio_sources().len(), 2);
    assert!(save_state(&loaded) == save_state(&system));
    for _ in 0..2000 {
        system.step();
        loaded.step();
    }
    assert!(save_state(&loaded) == save_state(&system));
}

/// The CD machine's state is the base board's format: version 3 since pcengine-rust 37f17ba
/// (the clean VDC rewrite); a state of an earlier version is refused, not converted, and the
/// machine is left as it was. (Not from the reference: its states are its own format.)
#[test]
fn a_state_is_the_base_boards_version_3_and_an_older_one_is_refused() {
    use pcengine_rust::state::{MAGIC, VERSION};
    let system = running(Some(disc_audio_then_data(40, 20)), true);
    let state = save_state(&system);
    assert_eq!(VERSION, 3);
    assert_eq!(&state[..MAGIC.len()], MAGIC);
    let version = u32::from_le_bytes(state[MAGIC.len()..MAGIC.len() + 4].try_into().unwrap());
    assert_eq!(version, 3);
    let mut old = state.clone();
    old[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&2u32.to_le_bytes());
    let mut loaded = running(Some(disc_audio_then_data(40, 20)), true);
    let before = save_state(&loaded);
    let error = load_state(&mut loaded, &old).unwrap_err();
    assert!(error.contains("version 2"), "{error}");
    assert!(save_state(&loaded) == before);
}

#[test]
fn a_state_names_the_disc_by_its_contents_table_and_holds_no_sector() {
    let system = running(None, false);
    let state = save_state(&system);
    assert!(state.len() < 200_000); // no disc, no System Card image
    let mut loaded = running(Some(disc_audio_then_data(40, 20)), false);
    let error = load_state(&mut loaded, &state).unwrap_err();
    assert!(error.contains("other media"), "{error}");
    // two discs with one layout are not told apart (the reference's rule)
    let mut same_layout = running(Some(disc_data_only(40)), false);
    load_state(&mut same_layout, &state).unwrap();
    // and a machine with no disc in the drive is another machine
    let mut cd = PcEngineCd::new(&image(256 * 1024), None);
    cd.board.pad = Some(Box::new(Pad2::new()));
    assert!(load_state(&mut cd.build().unwrap(), &state).is_err());
}

#[test]
fn a_machine_without_the_arcade_card_refuses_a_state_made_with_it() {
    let system = running(None, true);
    let state = save_state(&system);
    let mut plain = running(None, false);
    assert!(load_state(&mut plain, &state).is_err());
    let mut fitted = running(None, true);
    assert!(load_state(&mut fitted, &save_state(&plain)).is_err());
}

#[test]
fn another_system_card_refuses_the_state() {
    let system = running(None, false);
    let state = save_state(&system);
    let mut other_image = image(256 * 1024);
    other_image[0x100] = 0x00;
    let mut cd = PcEngineCd::new(&other_image, Some(disc_data_only(40)));
    cd.board.pad = Some(Box::new(Pad2::new()));
    let error = load_state(&mut cd.build().unwrap(), &state).unwrap_err();
    assert!(error.contains("another ROM image"), "{error}");
    // the same image as a plain System Card (no Super RAM) is another card too
    let mut cd = PcEngineCd::new(&image(256 * 1024), Some(disc_data_only(40)));
    cd.super_system_card = Some(false);
    cd.board.pad = Some(Box::new(Pad2::new()));
    assert!(load_state(&mut cd.build().unwrap(), &state).is_err());
}

#[test]
fn a_refused_state_leaves_the_machine_untouched() {
    let system = running(None, true);
    let state = save_state(&system);
    let mut plain = running(None, false);
    for _ in 0..500 {
        plain.step();
    }
    let before = save_state(&plain);
    assert!(load_state(&mut plain, &state).is_err());
    assert!(save_state(&plain) == before);
}

/// Drive the unit into the middle of things: a read under way, ADPCM playing, the fader set.
fn busy_unit(arcade: bool) -> Host {
    let card = arcade.then(ArcadeCard::new);
    let mut h = Host::new(CdUnit::with(Some(disc_audio_then_data(40, 60)), None, card).unwrap());
    h.w(2, 0x7C);
    h.w(0x0D, 0x03);
    for byte in [0x12, 0x34, 0x56, 0x78] {
        h.w(0x0A, byte);
        h.advance(100);
    }
    h.w(0x0D, 0x00);
    h.w(0x0E, 0x08);
    h.w(0x0D, 0x20);
    h.w(0x0F, 0x0E);
    h.w(7, 0x80);
    h.write(0x1EE040, 0x99, h.t);
    h.command(&[8, 0, 0, 45, 3, 0]);
    h.read_data_bytes(700, 8);
    h
}

#[test]
fn the_unit_saved_mid_read_continues_register_for_register() {
    for arcade in [false, true] {
        let mut first = busy_unit(arcade);
        let state = first.unit.save_state();
        let mut second = Host::new(
            CdUnit::with(
                Some(disc_audio_then_data(40, 60)),
                None,
                arcade.then(ArcadeCard::new),
            )
            .unwrap(),
        );
        second.unit.load_state(&state).unwrap();
        second.t = first.t;
        second.pins.irq2 = first.pins.irq2.clone();
        assert!(second.unit.save_state() == state);
        assert_eq!(second.unit.bus.phase, first.unit.bus.phase);
        // the rest of the three sectors, the status, then an audio play with samples taken
        for h in [&mut first, &mut second] {
            let rest = h.read_data_bytes(3 * 2048 - 700, 8);
            assert_eq!(rest[2048 - 700], 46);
            assert_eq!(h.finish(), (0, 0));
        }
        let script = |h: &mut Host| {
            let mut seen = Vec::new();
            h.command(&[0xD8, 1, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
            h.finish();
            for _ in 0..40 {
                h.advance(50_000);
                seen.push((h.r(0), h.r(3), h.r(0x0C), h.r(0x0A), h.irq()));
                h.w(5, 0);
                seen.push((h.r(5), h.r(6), h.r(0x0F), h.r(2), h.irq()));
            }
            let now = h.t;
            let sources = h.unit.audio_sources();
            let samples: Vec<Vec<(i32, i32)>> =
                sources.into_iter().map(|s| s.take_samples(now)).collect();
            (seen, samples)
        };
        assert!(script(&mut first) == script(&mut second));
        assert!(first.unit.save_state() == second.unit.save_state());
    }
}

#[test]
fn a_unit_refuses_a_state_of_a_unit_built_another_way() {
    let with_card = busy_unit(true).unit.save_state();
    let mut plain = CdUnit::new(Some(disc_audio_then_data(40, 60)));
    assert!(plain.load_state(&with_card).is_err());
    assert!(plain.load_state(&[1, 2, 3]).is_err());
    assert_ne!(
        CdUnit::new(Some(disc_data_only(40))).state_identity(),
        CdUnit::new(Some(disc_data_only(41))).state_identity()
    );
    assert_ne!(
        CdUnit::new(None).state_identity(),
        CdUnit::with(None, None, Some(ArcadeCard::new()))
            .unwrap()
            .state_identity()
    );
}

#[test]
fn loading_keeps_the_switches_and_the_wiring() {
    let state = busy_unit(false).unit.save_state();
    let mut unit = CdUnit::new(Some(disc_audio_then_data(40, 60)));
    unit.switches.dma_byte_clocks = 99;
    unit.bus.switches.sector_clocks = 99_999;
    unit.adpcm.switches.saturate = false;
    unit.bus.cdda.subq_status = [9, 9, 9, 9];
    let mut fader = unit.bus.cdda.fader.get();
    fader.pcetech_fade = true;
    unit.bus.cdda.fader.set(fader);
    unit.bus.record_log = true;
    unit.load_state(&state).unwrap();
    assert_eq!(unit.switches.dma_byte_clocks, 99);
    assert_eq!(unit.bus.switches.sector_clocks, 99_999);
    assert!(!unit.adpcm.switches.saturate && unit.bus.record_log);
    assert_eq!(unit.bus.cdda.subq_status, [9, 9, 9, 9]);
    let fader = unit.bus.cdda.fader.get();
    assert!(fader.pcetech_fade && fader.fade_register == 0x0E && fader.fade_start > 0);
    assert!(std::rc::Rc::ptr_eq(&unit.bus.cdda.fader, &unit.adpcm.fader));
    assert!(unit.disc().is_some() && unit.bus.cdda.disc.is_some());
}

#[test]
fn the_snapshot_pair_covers_the_machine() {
    let mut system = running(None, true);
    for _ in 0..3000 {
        system.step();
    }
    let snapshot = save_snapshot(&system);
    let state = save_state(&system);
    arcade(system.board_mut()).unwrap().ram[7] = 7;
    for _ in 0..3000 {
        system.step();
    }
    load_snapshot(&mut system, &snapshot).unwrap();
    assert!(save_state(&system) == state);
}
