// SPDX-License-Identifier: MIT
//! The disc model: LBA/MSF, tracks from a cue, data sectors, chdman's cue shape (the
//! reference's tests/test_disc.py), and what this crate adds to it: the cue sheet parsed from
//! text, the sector store as a trait, the reference's identity digest.

mod common;

use common::{audio_sectors, data_sector, disc_audio_then_data, disc_data_only, numbered_data};
use pcenginecd_rust::disc::{
    from_bcd, lba_to_msf, msf_to_lba, parse_cue, to_bcd, CueSource, Disc, MemoryImages,
    SectorSource, RAW_SECTOR,
};
use pcenginecd_rust::seek::seek_time_ms;

#[test]
fn bcd_and_msf_round_trip() {
    assert!(to_bcd(59) == 0x59 && from_bcd(0x59) == 59);
    assert_eq!(lba_to_msf(0), (0, 2, 0)); // LBA 0 is 00:02:00: the lead-in gap is not stored
    assert_eq!(lba_to_msf(75 * 60 - 150), (1, 0, 0));
    for lba in [0, 1, 74, 75, 4349, 4350, 99999] {
        let (m, s, f) = lba_to_msf(lba);
        assert_eq!(msf_to_lba(m, s, f), lba);
    }
}

#[test]
fn memory_disc_reads_its_payload_and_zeros_outside() {
    let disc = disc_data_only(10);
    assert_eq!(disc.sector_count, 10);
    assert_eq!(disc.read_data(3)[..4], [3, 4, 5, 6]);
    assert_eq!(disc.read_data(100), [0; 2048]);
    assert_eq!(disc.read_raw(3)[..12], data_sector(3, &[])[..12]); // sync pattern
}

#[test]
fn tracks_and_audio_are_not_data() {
    let disc = disc_audio_then_data(40, 20);
    let kinds: Vec<_> = disc
        .tracks
        .iter()
        .map(|t| (t.number, t.is_data, t.start, t.end))
        .collect();
    assert_eq!(
        kinds,
        [(1, false, 0, 40), (2, true, 40, 60), (3, false, 60, 90)]
    );
    assert_eq!(disc.track_at(45).unwrap().number, 2);
    assert_eq!(disc.read_data(10), [0; 2048]); // an audio sector has no data
    assert_eq!(disc.read_data(40)[0], 40);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const PREGAP_CUE: &str = "FILE \"d.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 MODE1/2352\n    INDEX 00 00:00:10\n    INDEX 01 00:00:15\n  TRACK 03 AUDIO\n    INDEX 01 00:00:30\n";

fn pregap_image() -> Vec<u8> {
    let mut image = audio_sectors(10);
    image.extend(numbered_data(10, 20));
    image.extend(audio_sectors(10));
    image
}

#[test]
fn cue_with_pregap_index_00() {
    // The shape chdman extractcd writes: one FILE, INDEX 00 (stored pregap) then INDEX 01.
    let dir = scratch("cue_with_pregap");
    std::fs::write(dir.join("d.bin"), pregap_image()).unwrap();
    std::fs::write(dir.join("d.cue"), PREGAP_CUE).unwrap();
    let disc = Disc::from_cue_path(dir.join("d.cue")).unwrap();
    let (t1, t2, t3) = (disc.tracks[0], disc.tracks[1], disc.tracks[2]);
    assert_eq!((t1.start, t1.end), (0, 10)); // track 1 ends where track 2's pregap begins
    assert_eq!((t2.index0, t2.start, t2.end), (Some(10), 15, 30));
    assert_eq!((t3.start, t3.end), (30, 40));
    assert_eq!(disc.sector_count, 40);
    assert_eq!(disc.read_data(12)[0], 12); // the pregap sectors of a data track are readable
}

#[test]
fn two_files_lie_end_to_end() {
    let dir = scratch("two_files");
    std::fs::write(dir.join("a.bin"), numbered_data(0, 5)).unwrap();
    std::fs::write(dir.join("b.bin"), audio_sectors(7)).unwrap();
    std::fs::write(
        dir.join("d.cue"),
        "FILE \"a.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\nFILE \"b.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n",
    )
    .unwrap();
    let disc = Disc::from_cue_path(dir.join("d.cue")).unwrap();
    let spans: Vec<_> = disc.tracks.iter().map(|t| (t.start, t.end)).collect();
    assert_eq!(spans, [(0, 5), (5, 12)]);
    assert_eq!(disc.read_data(4)[0], 4);
    assert_eq!(disc.read_raw(5)[..], audio_sectors(1)[..]);
}

// --- not in the reference: the pieces a page uses ---------------------------------------------------

#[test]
fn a_cue_sheet_parses_without_a_filesystem_and_reads_through_any_reader() {
    let image = pregap_image();
    let mut asked = Vec::new();
    let sheet = parse_cue(PREGAP_CUE, &mut |name| {
        asked.push(name.to_string());
        Ok(image.len() as u64)
    })
    .unwrap();
    assert_eq!(asked, ["d.bin"]);
    assert_eq!((sheet.files[0].sectors, sheet.files[0].first_lba), (40, 0));
    assert_eq!(sheet.locate(12), Some((0, 12 * RAW_SECTOR as u64)));
    assert_eq!(sheet.locate(40), None);
    // whole files in memory
    let disc = Disc::from_cue(
        &sheet,
        Box::new(CueSource::new(&sheet, MemoryImages(vec![image.clone()]))),
    );
    assert_eq!(disc.read_data(12)[0], 12);
    assert_eq!(disc.read_raw(40), [0; RAW_SECTOR]); // outside the disc
                                                    // a closure over slices of the user's file, as a page would give
    let slices = image.clone();
    let reader = move |file: usize, offset: u64, out: &mut [u8]| {
        assert_eq!(file, 0);
        let from = offset as usize;
        out.copy_from_slice(&slices[from..from + out.len()]);
        out.len()
    };
    let disc = Disc::from_cue(&sheet, Box::new(CueSource::new(&sheet, reader)));
    assert_eq!(disc.read_data(29)[..2], [29, 30]);
    assert_eq!(disc.read_raw(31)[..], audio_sectors(2)[RAW_SECTOR..]);
}

#[test]
fn the_cue_grammar_is_the_references() {
    let length = &mut |_: &str| Ok(10 * RAW_SECTOR as u64);
    // unquoted names, any case, CRLF, a name with spaces, the last word of any spelling
    let sheet = parse_cue(
        "file my disc.bin binary\r\n track 1 mode1/2352\r\n index 1 0:0:0\r\nFILE \"b c.bin\" WAVE\r\n TRACK 02 AUDIO\r\n INDEX 01 00:00:02\r\n",
        length,
    )
    .unwrap();
    assert_eq!(sheet.files[0].name, "my disc.bin");
    assert_eq!(sheet.files[1].name, "b c.bin");
    assert_eq!((sheet.tracks[0].start, sheet.tracks[0].end), (0, 12));
    assert_eq!((sheet.tracks[1].start, sheet.tracks[1].end), (12, 20));
    assert!(sheet.tracks[0].is_data && !sheet.tracks[1].is_data);
    // refused, as the reference refuses them
    let bad = |text: &str| parse_cue(text, &mut |_: &str| Ok(0)).unwrap_err();
    assert!(
        bad("FILE \"a.bin\" BINARY\n TRACK 01 MODE2/2352\n INDEX 01 00:00:00\n")
            .contains("MODE2/2352")
    );
    assert!(
        bad("FILE \"a.bin\" BINARY\n TRACK 01 AUDIO\n PREGAP 00:02:00\n INDEX 01 00:00:00\n")
            .contains("PREGAP")
    );
    assert!(bad("REM nothing\n").contains("no FILE or TRACK"));
    // a file the page cannot find
    assert!(
        parse_cue("FILE \"a.bin\" BINARY\n", &mut |name: &str| Err(format!(
            "no {name}"
        )))
        .is_err()
    );
}

#[test]
fn a_sector_source_can_be_anything() {
    struct Counting(i64);
    impl SectorSource for Counting {
        fn read_sector(&mut self, lba: i64, out: &mut [u8; RAW_SECTOR]) {
            self.0 += 1;
            out.copy_from_slice(&data_sector(lba, &[lba as u8, 0xEE]));
        }
    }
    let tracks = disc_data_only(8).tracks.clone();
    let disc = Disc::new(tracks, 8, Box::new(Counting(0)));
    assert_eq!(disc.read_data(5)[..2], [5, 0xEE]);
    assert_eq!(disc.read_data(8), [0; 2048]); // outside: the source is not asked
}

#[test]
fn the_state_identity_is_the_references_digest_of_the_contents_table() {
    // pcenginecd-python v0.1.0: disc_audio_then_data(40, 20).state_identity() and
    // disc_data_only(10).state_identity()
    assert_eq!(
        disc_audio_then_data(40, 20).state_identity(),
        "e84fd006688f6b4c26f7ea13b562993687fad884"
    );
    assert_eq!(
        disc_data_only(10).state_identity(),
        "b66deaaa17a173d2238b5540ad2801a68361e13c"
    );
}

#[test]
fn seek_times_are_the_references_doubles() {
    // (start, target, pcenginecd-python's seek_time_ms, int(MASTER_HZ * ms / 1000))
    let cases: [(i64, i64, f64, i64); 12] = [
        (0, 0, 216.735, 4654876),
        (0, 3, 216.735, 4654876),
        (0, 100, 333.4016666666667, 7160558),
        (0, 1000, 433.4016666666667, 9308285),
        (100, 5000, 501.9159166666667, 10779784),
        (0, 12573, 652.3722051282051, 14011175),
        (1000, 200000, 1746.6215097435897, 37512665),
        (333000, 10, 2307.928521025641, 49568008),
        (-150, 40, 333.4016666666667, 7160558),
        (400000, 5, 2556.8032902564105, 54913159),
        (70000, 70500, 360.0966666666667, 7733894),
        (50000, 56000, 515.8966666666666, 11080053),
    ];
    for (start, target, ms, clocks) in cases {
        let ours = seek_time_ms(start, target);
        assert_eq!(ours.to_bits(), ms.to_bits(), "{start} to {target}");
        assert_eq!((21_477_272.0 * ours / 1000.0) as i64, clocks);
    }
}
