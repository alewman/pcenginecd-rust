// SPDX-License-Identifier: MIT
//! The disc: a track table and sectors (`disc.py`).
//!
//! Addresses. A sector's **LBA** is its index in the disc image: LBA 0 is the first stored
//! sector, which is absolute time 00:02:00 (the two-second lead-in gap is not stored). So `msf =
//! lba + 150` frames. The SCSI READ command and every track start in this module are in this LBA,
//! and the `READ TOC` answers are MSF = LBA + 150 [doc: pcetech 8; the Hudson BIOS manual's
//! CD_DINFO; emulator tier: MAME `nec/pce_cd.cpp` (BSD-3) adds 150; OBSERVED, closed box:
//! Geargrafx answers a track at LBA 40 with 00:02:40]. A raw sector is 2352 bytes;
//! a Mode 1 data sector keeps its 2048 payload bytes at +16 (12 sync, 4 header).
//!
//! **This is the one module designed for the browser and not only transcribed.** The table of
//! contents and the LBA/MSF logic are the reference's; where the reference opens files (and
//! expands a CHD with `chdman`), this crate has three separate pieces:
//!
//! - [`SectorSource`]: the sector store as a trait, "raw sector `lba` into this buffer".
//!   [`MemorySource`] holds a whole image (the tests' synthetic discs); [`CueSource`] maps an
//!   LBA to a byte range of one of a cue sheet's files and asks an [`ImageReader`] for it, which
//!   a web page implements over slices of the user's file (a closure `(file, offset, buffer) ->
//!   bytes read` is one), [`MemoryImages`] over whole files in memory and, natively only,
//!   [`FileImages`] over open files.
//! - [`parse_cue`]: the cue sheet from its text plus a way to learn each named file's length, so
//!   it needs no filesystem. The grammar is the reference's (`FILE`, `TRACK` with `MODE1/2352` or
//!   `AUDIO`, `INDEX 00/01`; several FILEs laid end to end; `PREGAP` refused).
//! - [`Disc`]: tracks plus a source. `Disc::from_cue_path` (native) is the reference's
//!   `Disc.from_cue`.
//!
//! **CHD is not read here** (the reference shells out to `chdman`, which a browser has not, and a
//! CHD reader would be a separately licensed dependency): extract a CHD to bin/cue first, as the
//! gate does.
//!
//! Other differences forced by Rust: LBAs are `i64` and the divisions are Python's (an audio
//! command may name a position before LBA 0); `Disc` keeps its source behind a `RefCell` so the
//! drive and the audio player can share one disc (`Rc<Disc>`), as the reference's two objects
//! share one; `to_bcd` returns the low eight bits (the reference returns an unbounded integer
//! for a value past 159, which a register read could not hold).

use std::cell::RefCell;

use pcengine_rust::machine::py_floordiv;
use sha1::{Digest, Sha1};

pub const RAW_SECTOR: usize = 2352;
pub const DATA_SECTOR: usize = 2048;
pub const FRAMES_PER_SECOND: i64 = 75;
/// the two-second gap before LBA 0 [doc: Red Book; emu: all four]
pub const LEAD_IN_FRAMES: i64 = 150;
pub const SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

pub fn to_bcd(value: i64) -> u8 {
    ((py_floordiv(value, 10) << 4) | value.rem_euclid(10)) as u8
}

pub fn from_bcd(value: u8) -> i64 {
    (value >> 4) as i64 * 10 + (value & 15) as i64
}

/// The absolute MSF of an LBA (the +150 lead-in applied).
pub fn lba_to_msf(lba: i64) -> (i64, i64, i64) {
    relative_msf(lba + LEAD_IN_FRAMES)
}

pub fn msf_to_lba(minutes: i64, seconds: i64, frames: i64) -> i64 {
    (minutes * 60 + seconds) * FRAMES_PER_SECOND + frames - LEAD_IN_FRAMES
}

/// A track-relative position (no lead-in).
pub fn relative_msf(frames: i64) -> (i64, i64, i64) {
    (
        py_floordiv(frames, 60 * FRAMES_PER_SECOND),
        py_floordiv(frames, FRAMES_PER_SECOND).rem_euclid(60),
        frames.rem_euclid(75),
    )
}

pub fn msf_frames(minutes: i64, seconds: i64, frames: i64) -> i64 {
    (minutes * 60 + seconds) * FRAMES_PER_SECOND + frames
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    pub number: i64,
    pub is_data: bool,
    /// LBA of INDEX 01
    pub start: i64,
    /// LBA one past the track's last sector
    pub end: i64,
    /// LBA of INDEX 00 (the pregap), if the cue gives one
    pub index0: Option<i64>,
}

// --- the sector store -----------------------------------------------------------------------------

/// Where a disc's sectors come from. `lba` is inside the disc (`0 <= lba < sector_count`: `Disc`
/// answers zeros outside by itself); the source fills all 2352 bytes, with zeros for whatever
/// it cannot read.
pub trait SectorSource {
    fn read_sector(&mut self, lba: i64, out: &mut [u8; RAW_SECTOR]);
}

/// A whole disc image in memory, the sectors end to end.
pub struct MemorySource {
    pub data: Vec<u8>,
}

impl SectorSource for MemorySource {
    fn read_sector(&mut self, lba: i64, out: &mut [u8; RAW_SECTOR]) {
        let from = lba as usize * RAW_SECTOR;
        let have = self.data.len().saturating_sub(from).min(RAW_SECTOR);
        out[..have].copy_from_slice(&self.data[from..from + have]);
        out[have..].fill(0);
    }
}

/// Bytes of the files a cue sheet names: `out.len()` bytes of file `file` (its index in
/// `CueSheet::files`) from `offset`; returns how many were read (fewer at the end of a file).
/// A web page implements this over `Blob.slice` of the user's file, or hands in a closure.
pub trait ImageReader {
    fn read_at(&mut self, file: usize, offset: u64, out: &mut [u8]) -> usize;
}

impl<F: FnMut(usize, u64, &mut [u8]) -> usize> ImageReader for F {
    fn read_at(&mut self, file: usize, offset: u64, out: &mut [u8]) -> usize {
        self(file, offset, out)
    }
}

/// The cue sheet's files, each whole in memory.
pub struct MemoryImages(pub Vec<Vec<u8>>);

impl ImageReader for MemoryImages {
    fn read_at(&mut self, file: usize, offset: u64, out: &mut [u8]) -> usize {
        let Some(data) = self.0.get(file) else {
            return 0;
        };
        let from = (offset as usize).min(data.len());
        let count = (data.len() - from).min(out.len());
        out[..count].copy_from_slice(&data[from..from + count]);
        count
    }
}

/// The cue sheet's files on disk (native only: nothing else in the crate touches a file).
#[cfg(not(target_arch = "wasm32"))]
pub struct FileImages {
    files: Vec<std::fs::File>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileImages {
    /// Open every file of `sheet`, named relative to `directory` (the cue's own).
    pub fn open(sheet: &CueSheet, directory: &std::path::Path) -> Result<FileImages, String> {
        let mut files = Vec::new();
        for file in &sheet.files {
            let path = directory.join(&file.name);
            files.push(
                std::fs::File::open(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?,
            );
        }
        Ok(FileImages { files })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ImageReader for FileImages {
    fn read_at(&mut self, file: usize, offset: u64, out: &mut [u8]) -> usize {
        use std::io::{Read, Seek, SeekFrom};
        let Some(handle) = self.files.get_mut(file) else {
            return 0;
        };
        if handle.seek(SeekFrom::Start(offset)).is_err() {
            return 0;
        }
        let mut done = 0;
        while done < out.len() {
            match handle.read(&mut out[done..]) {
                Ok(0) | Err(_) => break,
                Ok(count) => done += count,
            }
        }
        done
    }
}

/// The sectors of a cue sheet's files, through an `ImageReader`.
pub struct CueSource<R: ImageReader> {
    /// (first LBA, sectors) of each file
    spans: Vec<(i64, i64)>,
    pub reader: R,
}

impl<R: ImageReader> CueSource<R> {
    pub fn new(sheet: &CueSheet, reader: R) -> CueSource<R> {
        CueSource {
            spans: sheet
                .files
                .iter()
                .map(|file| (file.first_lba, file.sectors))
                .collect(),
            reader,
        }
    }
}

impl<R: ImageReader> SectorSource for CueSource<R> {
    fn read_sector(&mut self, lba: i64, out: &mut [u8; RAW_SECTOR]) {
        let mut done = 0;
        for (file, &(first, sectors)) in self.spans.iter().enumerate() {
            if first <= lba && lba < first + sectors {
                let offset = (lba - first) as u64 * RAW_SECTOR as u64;
                done = self.reader.read_at(file, offset, out).min(RAW_SECTOR);
                break;
            }
        }
        out[done..].fill(0);
    }
}

// --- the cue sheet ----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueFile {
    /// as the cue names it (relative to the cue)
    pub name: String,
    /// whole raw sectors in the file
    pub sectors: i64,
    /// the LBA of the file's first sector: the files lie end to end, as one disc
    pub first_lba: i64,
}

/// A parsed cue sheet: where each file lies on the disc, and the track table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueSheet {
    pub files: Vec<CueFile>,
    pub tracks: Vec<Track>,
    pub sector_count: i64,
}

impl CueSheet {
    /// The file and the byte offset in it that hold sector `lba`.
    pub fn locate(&self, lba: i64) -> Option<(usize, u64)> {
        self.files.iter().enumerate().find_map(|(index, file)| {
            (file.first_lba <= lba && lba < file.first_lba + file.sectors)
                .then(|| (index, (lba - file.first_lba) as u64 * RAW_SECTOR as u64))
        })
    }
}

struct Entry {
    number: i64,
    is_data: bool,
    file: usize,
    idx0: Option<i64>,
    idx1: Option<i64>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `KEYWORD\s+rest` at the start of `line`, case-insensitive: the rest, its leading whitespace
/// removed.
fn after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let head = line.get(..keyword.len())?;
    if !head.eq_ignore_ascii_case(keyword) {
        return None;
    }
    let rest = &line[keyword.len()..];
    let trimmed = rest.trim_start();
    (trimmed.len() < rest.len()).then_some(trimmed)
}

/// The reference's `FILE\s+"?(.*?)"?\s+\w+$`: the name between optional quotes, before the
/// last word of the line.
fn file_name(line: &str) -> Option<&str> {
    let rest = after_keyword(line, "FILE")?;
    let before = rest.trim_end_matches(is_word);
    if before.len() == rest.len() {
        return None; // no closing word (BINARY, WAVE, ...)
    }
    let name = before.trim_end();
    if name.len() == before.len() {
        return None; // the word is not set off by whitespace
    }
    let name = name.strip_prefix('"').unwrap_or(name);
    Some(name.strip_suffix('"').unwrap_or(name))
}

/// A leading run of ASCII digits as a number, and what follows it.
fn number(text: &str) -> Option<(i64, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let value = text[..end].parse().ok()?;
    Some((value, &text[end..]))
}

/// The reference's `TRACK\s+(\d+)\s+(\S+)`.
fn track_line(line: &str) -> Option<(i64, &str)> {
    let rest = after_keyword(line, "TRACK")?;
    let (value, rest) = number(rest)?;
    let mode = rest.trim_start();
    if mode.len() == rest.len() || mode.is_empty() {
        return None;
    }
    Some((value, mode.split_whitespace().next()?))
}

/// The reference's `INDEX\s+(\d+)\s+(\d+):(\d+):(\d+)`.
fn index_line(line: &str) -> Option<(i64, i64)> {
    let rest = after_keyword(line, "INDEX")?;
    let (index, rest) = number(rest)?;
    let time = rest.trim_start();
    if time.len() == rest.len() {
        return None;
    }
    let (minutes, rest) = number(time)?;
    let (seconds, rest) = number(rest.strip_prefix(':')?)?;
    let (frames, _) = number(rest.strip_prefix(':')?)?;
    Some((index, msf_frames(minutes, seconds, frames)))
}

/// A cue sheet from its text. `file_bytes` answers the length in bytes of a file the sheet
/// names (a page asks the `File` the user chose; `Disc::from_cue_path` asks the filesystem).
///
/// Handles `FILE`, `TRACK` (MODE1/2352, AUDIO) and `INDEX 00/01`; several FILEs are laid end to
/// end, as one disc. Cue times are positions in their file (chdman writes them so, with the
/// pregap stored). `PREGAP` (an unstored gap) is not supported.
pub fn parse_cue(
    text: &str,
    file_bytes: &mut dyn FnMut(&str) -> Result<u64, String>,
) -> Result<CueSheet, String> {
    let mut files: Vec<(String, i64)> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    for raw in text.split(['\n', '\r']) {
        let line = raw.trim();
        if let Some(name) = file_name(line) {
            let sectors = (file_bytes(name)? / RAW_SECTOR as u64) as i64;
            files.push((name.to_string(), sectors));
        } else if let Some((number, mode)) = track_line(line) {
            let mode = mode.to_uppercase();
            if mode != "AUDIO" && mode != "MODE1/2352" {
                return Err(format!("cue: unsupported track mode {mode}"));
            }
            if files.is_empty() {
                return Err("cue: a TRACK before any FILE".to_string());
            }
            entries.push(Entry {
                number,
                is_data: mode != "AUDIO",
                file: files.len() - 1,
                idx0: None,
                idx1: None,
            });
        } else if let Some((index, frames)) = index_line(line) {
            let entry = entries.last_mut().ok_or("cue: an INDEX before any TRACK")?;
            match index {
                0 => entry.idx0 = Some(frames),
                1 => entry.idx1 = Some(frames),
                _ => {} // the reference keeps them and never reads them
            }
        } else if line.to_uppercase().starts_with("PREGAP") {
            return Err("cue: PREGAP (an unstored gap) is not supported".to_string());
        }
    }
    if files.is_empty() || entries.is_empty() {
        return Err("cue: no FILE or TRACK".to_string());
    }
    let mut lba = 0;
    let mut placed = Vec::new();
    for (name, sectors) in files {
        placed.push(CueFile {
            name,
            sectors,
            first_lba: lba,
        });
        lba += sectors;
    }
    let mut tracks = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let missing = || format!("cue: track {} has no INDEX 01", e.number);
        let base = placed[e.file].first_lba;
        let start = base + e.idx1.ok_or_else(missing)?;
        let index0 = e.idx0.map(|frames| base + frames);
        let end = match entries.get(i + 1) {
            Some(n) => {
                let nbase = placed[n.file].first_lba;
                match n.idx0 {
                    Some(frames) => nbase + frames,
                    None => {
                        nbase
                            + n.idx1
                                .ok_or_else(|| format!("cue: track {} has no INDEX 01", n.number))?
                    }
                }
            }
            None => lba,
        };
        tracks.push(Track {
            number: e.number,
            is_data: e.is_data,
            start,
            end,
            index0,
        });
    }
    Ok(CueSheet {
        files: placed,
        tracks,
        sector_count: lba,
    })
}

// --- the disc ---------------------------------------------------------------------------------------

/// A sector store plus a TOC. `sector_count` is the number of LBAs.
///
/// Media, not state: a save state names the disc (`state_identity`), it does not hold it.
pub struct Disc {
    pub tracks: Vec<Track>,
    pub sector_count: i64,
    source: RefCell<Box<dyn SectorSource>>,
}

impl Disc {
    pub fn new(tracks: Vec<Track>, sector_count: i64, source: Box<dyn SectorSource>) -> Disc {
        Disc {
            tracks,
            sector_count,
            source: RefCell::new(source),
        }
    }

    /// What a save state records of the disc: its table of contents (not its sectors, so two
    /// discs with one layout are not told apart). The same digest as the reference's: the SHA-1
    /// of Python's `repr(([(number, is_data, start, end), ...], sector_count))`.
    pub fn state_identity(&self) -> String {
        let toc: Vec<String> = self
            .tracks
            .iter()
            .map(|t| {
                let data = if t.is_data { "True" } else { "False" };
                format!("({}, {}, {}, {})", t.number, data, t.start, t.end)
            })
            .collect();
        let text = format!("([{}], {})", toc.join(", "), self.sector_count);
        Sha1::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    // -- construction -------------------------------------------------------------------------

    /// The disc of a parsed cue sheet, its sectors from `source` (a `CueSource` over the
    /// sheet's files, or anything else that answers the sheet's LBAs).
    pub fn from_cue(sheet: &CueSheet, source: Box<dyn SectorSource>) -> Disc {
        Disc::new(sheet.tracks.clone(), sheet.sector_count, source)
    }

    /// A bin/cue pair on disk, read in place (the reference's `Disc.from_cue`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_cue_path(cue_path: impl AsRef<std::path::Path>) -> Result<Disc, String> {
        let cue_path = cue_path.as_ref();
        let named = |error: String| format!("{}: {error}", cue_path.display());
        let bytes = std::fs::read(cue_path).map_err(|error| named(error.to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        let directory = cue_path.parent().unwrap_or(std::path::Path::new("."));
        let sheet = parse_cue(&text, &mut |name| {
            let path = directory.join(name);
            std::fs::metadata(&path)
                .map(|meta| meta.len())
                .map_err(|error| format!("{}: {error}", path.display()))
        })
        .map_err(named)?;
        let images = FileImages::open(&sheet, directory)?;
        Ok(Disc::from_cue(
            &sheet,
            Box::new(CueSource::new(&sheet, images)),
        ))
    }

    /// A disc whose tracks are raw 2352-byte sectors in memory: `(is_data, bytes)` each.
    /// Used by the tests (`tests/discs.rs` builds the bytes).
    pub fn from_memory(tracks: Vec<(bool, Vec<u8>)>) -> Result<Disc, String> {
        let mut out = Vec::new();
        let mut data = Vec::new();
        let mut lba = 0;
        for (index, (is_data, payload)) in tracks.into_iter().enumerate() {
            if payload.len() % RAW_SECTOR != 0 {
                return Err("a memory track is whole raw sectors".to_string());
            }
            let count = (payload.len() / RAW_SECTOR) as i64;
            out.push(Track {
                number: index as i64 + 1,
                is_data,
                start: lba,
                end: lba + count,
                index0: None,
            });
            data.extend_from_slice(&payload);
            lba += count;
        }
        Ok(Disc::new(out, lba, Box::new(MemorySource { data })))
    }

    // -- reading ------------------------------------------------------------------------------

    /// 2352 bytes of sector `lba`; zeros outside the disc.
    pub fn read_raw(&self, lba: i64) -> [u8; RAW_SECTOR] {
        let mut out = [0u8; RAW_SECTOR];
        if 0 <= lba && lba < self.sector_count {
            self.source.borrow_mut().read_sector(lba, &mut out);
        }
        out
    }

    /// The 2048-byte user data of a Mode 1 sector (zeros if it is not a data sector).
    pub fn read_data(&self, lba: i64) -> [u8; DATA_SECTOR] {
        let mut out = [0u8; DATA_SECTOR];
        if self.track_at(lba).is_some_and(|track| track.is_data) {
            out.copy_from_slice(&self.read_raw(lba)[16..16 + DATA_SECTOR]);
        }
        out
    }

    // -- the table of contents ----------------------------------------------------------------

    /// The track whose range (INDEX 00 .. end) holds `lba`.
    pub fn track_at(&self, lba: i64) -> Option<&Track> {
        self.tracks.iter().find(|t| {
            let first = t.index0.unwrap_or(t.start);
            first <= lba && lba < t.end
        })
    }

    pub fn track(&self, number: i64) -> Option<&Track> {
        self.tracks.iter().find(|t| t.number == number)
    }

    /// The first track's number (0 for a disc without tracks, which the reference cannot
    /// answer).
    pub fn first_track(&self) -> i64 {
        self.tracks.first().map_or(0, |t| t.number)
    }

    pub fn last_track(&self) -> i64 {
        self.tracks.last().map_or(0, |t| t.number)
    }

    pub fn end_lba(&self) -> i64 {
        self.sector_count
    }
}
