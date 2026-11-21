# Where Rust forced a difference

Each module's header says so; together:

- **The board owns the card and the unit** (`board.rs`). The reference's
  `PcEngineCd` keeps `card`, `unit`, `arcade` and `board` side by side; here
  `PcEngineCd` is the description (the reference's constructor arguments with
  its defaults, and every named switch) and `build()` hands back the base
  board's `System`. `board::unit`, `card` and `arcade` reach the devices
  inside a board. The reference's `**board_kwargs` is the `board` field, a
  `BoardOptions` whose card and port are replaced.
- **`BoardPins` arrives with each call** (the base board's rule), so there is
  no `bind`. A unit driven with no board behind it takes the base crate's
  `DetachedPins`.
- **Callbacks became lists.** The reference's SCSI bus and ADPCM unit call the
  unit (`_bus_irq`, `_adpcm_irq`) and its audio player calls the bus
  (`on_good_status`). Here the bus and the ADPCM unit list their `(mask,
  level)` changes in order (`irq_changes`) and the unit applies them after
  each call (`sync_irq`, which also sets its bit on IRQ2); the player counts
  the good statuses it owes (`good_status_due`) and the bus answers them
  right after the player ran. Nothing the callbacks read can change in
  between, so the order of effects is the reference's.
- **Who holds what.** The bus owns the audio player (`unit.bus.cdda`,
  `unit.cdda()`); the ADPCM unit is its own audio source (the reference's
  `AdpcmSource` object is three fields and the `AudioSource` impl of
  `Adpcm`); the two are what `PortDevice::audio_sources` hands the mixer,
  after the PSG, where the reference appends them to the board's list. The
  fader both sources ask (`$180F`) is one `Fader` value behind an
  `Rc<Cell<_>>`, and the disc the bus and the player share is an `Rc<Disc>`.
  The reference's ADPCM source first catches the unit up to the mixer's
  clock; the board has always done that already, and here the source cannot
  reach the unit.
- **The id registers** `$18C0-$18C7` are asked of `pins.card` when it is a
  `SystemCard` (the reference's unit keeps the card it was built with).
- **Switches.** The reference's class attributes are `ScsiSwitches`,
  `UnitSwitches`, `AdpcmSwitches`, `ArcadeSwitches` and the fade times of
  `Fader`, public fields of their devices and together `CdSwitches` in the
  builder; `CddaPlayer.SUBQ_STATUS` is `subq_status`. A subclass with other
  attributes there is `ArcadeCard::with(switches)` here.
- **The disc** (`disc.rs`): the sector store is a trait and the cue sheet is
  parsed from text (below); no CHD; no `close`. LBAs are `i64` with Python's
  floor division (an audio command may name a position before LBA 0);
  `to_bcd` answers the low eight bits where the reference's integer is
  unbounded; a `TRACK` before any `FILE`, which the reference attaches to the
  last file through a negative index, is refused; a track without `INDEX 01`
  is an error where the reference raises `KeyError`.
- **Python's arithmetic** is spelled out: the seek model's `int`/`float`
  mix in `i64` and `f64` in the reference's order (`tests/disc.rs` holds
  twelve of its doubles, bit for bit), `-(-a // b)` as `ceil_div`, `int()` as
  truncation, `round()` to even. The ADPCM step table is the 49 integers
  `floor(16 * 1.1 ** index)` written out, so it does not depend on a
  platform's `pow`.
- **Save states**: the base board's format, through `DeviceState`. The unit
  saves everything that changes as the console runs (the bus and the player,
  the ADPCM unit with its RAM, CD RAM, backup RAM, the Arcade Card with its
  2 MiB, the fader) as one bincode value; the System Card saves its RAM. The
  switches, the wiring and the drive's command log stay the machine's own,
  and the player's decoded sector is not saved. The unit's identity names the
  disc by `Disc::state_identity` (the reference's digest of the table of
  contents, the same hexadecimal string) and says whether an Arcade Card is
  fitted; the card's names its image, its region and whether it is a Super
  System Card. A state is not portable to the reference or back.
- **Smaller ones**: a `bram` image longer than 2 KiB is cut (the reference's
  slice assignment would grow the RAM); a sector asked for with no disc in
  the drive reads as zeros (the reference asserts); `Disc::first_track` of a
  disc without tracks is 0 (the reference raises).
- **Not carried over**: `Disc.from_chd`; the reference's scripts that boot a
  disc to pictures and drive Geargrafx and Beetle PCE (it has been compared
  with those; this crate is judged against it).
- **Added**: `CdUnit::bram`, `set_bram` (the host keeping backup RAM between
  sessions), `trace.rs`, the three binaries, `examples/probe_report.rs`.
