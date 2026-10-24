//! From-scratch DDR song wave bank (XWB v43, streaming) — layout first,
//! blocks appended later.
//!
//! The shape is the stock packer's, as `xwb::parse_song_bank` enforces it
//! (and as the sibling `ddr-chart-tools`' `build_xwb_bank` + `xwb::write`
//! emit it): 52-byte header, 96-byte BANKDATA (flags `0x0009_0001`,
//! header version 42, 64-byte names, alignment 2048, build time 0), two
//! 24-byte entry rows, an EMPTY seek-table segment (ADPCM has none — the
//! `SEEKTABLES` flag bit is stock), two 64-byte entry names, zero pad to
//! `0x800`, then the wave data: entry 0 at 0, entry 1 at the next 2048
//! boundary. Entries are **main = 0 (`<name>`), preview = 1 (`<name>_s`)**
//! — the stock order; the tool writes preview first, so a tool bank and
//! ours differ in entry order only (the harness checks per-entry bytes
//! against the tool and whole-file bytes against the tool's container
//! writer fed main-first). The matching XSB is `xsb::write_song_profile`.
//!
//! Every entry is MS-ADPCM, stereo, 140-byte / 128-frame blocks
//! (`WaveFormat` raw block align 48, `adpcm.rs`); durations are whole
//! blocks (`blocks · 128`), `loop_start 0`, `loop_length duration − 1` like
//! the tool and most stock entries. The plan is complete before any audio
//! is decoded, so a progressive writer can `set_len` the file, publish the
//! header and append blocks in file order (design §5.6): [`SongBankWriter`]
//! tracks the contiguous `written_watermark()` the read hook enforces.
//!
//! Pure; `scripts/validate_stepmania.sh` mounts it.

use std::fmt;
use std::io::{Seek, SeekFrom, Write};

use super::adpcm::{self, AdpcmError};
use super::WaveFormat;

const MAGIC: &[u8; 4] = b"WBND";
const VERSION: u32 = 43;
const HEADER_VERSION: u32 = 42;
const HEADER_SIZE: u64 = 52;
const BANK_DATA_SIZE: u64 = 96;
const ENTRY_META_SIZE: u64 = 24;
const NAME_SIZE: usize = 64;
const ALIGNMENT: u64 = 2048;
/// `TYPE_STREAMING | SEEKTABLES | ENTRYNAMES`.
const FLAGS: u32 = 0x0009_0001;
/// 28-bit duration field.
const MAX_DURATION: u64 = (1 << 28) - 1;

pub const ENTRY_COUNT: usize = 2;
pub const MAIN_ENTRY: usize = 0;
pub const PREVIEW_ENTRY: usize = 1;
pub const BLOCK_FRAMES: u64 = 128;
pub const BLOCK_BYTES: usize = 140;
/// Longest entry name: 63 bytes + NUL, and `<name>_s` must fit too.
pub const MAX_NAME_LEN: usize = NAME_SIZE - 3;

/// The DDR song-bank wave format: ADPCM, 2 channels, `rate`, raw align 48.
#[must_use]
pub fn ddr_wave_format(rate: u32) -> WaveFormat {
    WaveFormat::from_packed(2 | (2 << 2) | ((rate & 0x3_FFFF) << 5) | (48 << 23))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// Empty, > 61 bytes, or not `[a-z0-9_]`.
    BadName(String),
    BadRate(u32),
    /// An entry has zero frames.
    EmptyEntry(usize),
    /// Duration exceeds the 28-bit field.
    TooLong {
        entry: usize,
        frames: u64,
    },
    ArithmeticOverflow,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(n) => write!(f, "bad bank name {n:?}"),
            Self::BadRate(r) => write!(f, "bad sample rate {r}"),
            Self::EmptyEntry(e) => write!(f, "entry {e} has no frames"),
            Self::TooLong { entry, frames } => {
                write!(f, "entry {entry} too long ({frames} frames)")
            }
            Self::ArithmeticOverflow => f.write_str("song bank layout overflow"),
        }
    }
}

impl std::error::Error for PlanError {}

/// The complete layout of a two-entry song bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SongBankPlan {
    name: String,
    rate: u32,
    format: WaveFormat,
    blocks: [u64; ENTRY_COUNT],
    /// Entry data offsets relative to the wave-data segment.
    data_offsets: [u64; ENTRY_COUNT],
    wave_data_offset: u64,
    wave_data_len: u64,
}

impl SongBankPlan {
    /// Plan a bank named `name` (`<name>` / `<name>_s` entries) at `rate`
    /// with `frames_main` / `frames_preview` frames; frame counts are
    /// rounded UP to whole blocks (the decoder pads the last block).
    pub fn new(
        name: &str,
        rate: u32,
        frames_main: u64,
        frames_preview: u64,
    ) -> Result<Self, PlanError> {
        if !is_valid_name(name) {
            return Err(PlanError::BadName(name.to_string()));
        }
        if rate == 0 || rate > 0x3_FFFF {
            return Err(PlanError::BadRate(rate));
        }
        let mut blocks = [0u64; ENTRY_COUNT];
        for (entry, frames) in [frames_main, frames_preview].into_iter().enumerate() {
            if frames == 0 {
                return Err(PlanError::EmptyEntry(entry));
            }
            let b = frames.div_ceil(BLOCK_FRAMES);
            let duration = b
                .checked_mul(BLOCK_FRAMES)
                .ok_or(PlanError::ArithmeticOverflow)?;
            if duration > MAX_DURATION {
                return Err(PlanError::TooLong { entry, frames });
            }
            blocks[entry] = b;
        }
        let pre_data_end = HEADER_SIZE
            + BANK_DATA_SIZE
            + ENTRY_COUNT as u64 * ENTRY_META_SIZE
            + ENTRY_COUNT as u64 * NAME_SIZE as u64;
        let wave_data_offset = round_up(pre_data_end, ALIGNMENT);
        let main_len = blocks[MAIN_ENTRY]
            .checked_mul(BLOCK_BYTES as u64)
            .ok_or(PlanError::ArithmeticOverflow)?;
        let preview_len = blocks[PREVIEW_ENTRY]
            .checked_mul(BLOCK_BYTES as u64)
            .ok_or(PlanError::ArithmeticOverflow)?;
        let preview_offset = round_up(main_len, ALIGNMENT);
        let wave_data_len = preview_offset
            .checked_add(preview_len)
            .ok_or(PlanError::ArithmeticOverflow)?;
        let total = wave_data_offset
            .checked_add(wave_data_len)
            .ok_or(PlanError::ArithmeticOverflow)?;
        if total > u64::from(u32::MAX) {
            return Err(PlanError::ArithmeticOverflow);
        }
        Ok(Self {
            name: name.to_string(),
            rate,
            format: ddr_wave_format(rate),
            blocks,
            data_offsets: [0, preview_offset],
            wave_data_offset,
            wave_data_len,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    #[must_use]
    pub fn wave_format(&self) -> WaveFormat {
        self.format
    }

    /// Encoded blocks in `entry`.
    #[must_use]
    pub fn blocks(&self, entry: usize) -> u64 {
        self.blocks.get(entry).copied().unwrap_or(0)
    }

    /// Declared duration of `entry` in frames (`blocks · 128`).
    #[must_use]
    pub fn frames(&self, entry: usize) -> u64 {
        self.blocks(entry) * BLOCK_FRAMES
    }

    /// Encoded byte length of `entry`.
    #[must_use]
    pub fn data_len(&self, entry: usize) -> u64 {
        self.blocks(entry) * BLOCK_BYTES as u64
    }

    /// Absolute file offset of the wave-data segment (`0x800`).
    #[must_use]
    pub fn wave_data_offset(&self) -> u64 {
        self.wave_data_offset
    }

    /// Absolute file offset of `entry`'s first block.
    #[must_use]
    pub fn entry_data_offset(&self, entry: usize) -> u64 {
        self.wave_data_offset + self.data_offsets.get(entry).copied().unwrap_or(0)
    }

    /// Absolute file offset of block `block_index` of `entry`; `None` past
    /// the entry's end (index == blocks is the one-past-the-end offset).
    #[must_use]
    pub fn block_offset(&self, entry: usize, block_index: u64) -> Option<u64> {
        if entry >= ENTRY_COUNT || block_index > self.blocks[entry] {
            return None;
        }
        Some(self.entry_data_offset(entry) + block_index * BLOCK_BYTES as u64)
    }

    /// Total file length.
    #[must_use]
    pub fn total_len(&self) -> u64 {
        self.wave_data_offset + self.wave_data_len
    }

    /// Everything before the wave data: header, BANKDATA, entry rows,
    /// names, zero pad — exactly `wave_data_offset()` bytes.
    #[must_use]
    pub fn header_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.wave_data_offset as usize);
        let seg0 = HEADER_SIZE;
        let seg1 = seg0 + BANK_DATA_SIZE;
        let seg2 = seg1 + ENTRY_COUNT as u64 * ENTRY_META_SIZE;
        let seg3 = seg2;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&HEADER_VERSION.to_le_bytes());
        for (offset, length) in [
            (seg0, BANK_DATA_SIZE),
            (seg1, ENTRY_COUNT as u64 * ENTRY_META_SIZE),
            (seg2, 0),
            (seg3, ENTRY_COUNT as u64 * NAME_SIZE as u64),
            (self.wave_data_offset, self.wave_data_len),
        ] {
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&(length as u32).to_le_bytes());
        }
        out.extend_from_slice(&FLAGS.to_le_bytes());
        out.extend_from_slice(&(ENTRY_COUNT as u32).to_le_bytes());
        out.extend_from_slice(&name_field(&self.name));
        out.extend_from_slice(&(ENTRY_META_SIZE as u32).to_le_bytes());
        out.extend_from_slice(&(NAME_SIZE as u32).to_le_bytes());
        out.extend_from_slice(&(ALIGNMENT as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // compact format
        out.extend_from_slice(&0u64.to_le_bytes()); // build time
        for entry in 0..ENTRY_COUNT {
            let duration = self.frames(entry) as u32;
            out.extend_from_slice(&(duration << 4).to_le_bytes());
            out.extend_from_slice(&self.format.packed().to_le_bytes());
            out.extend_from_slice(&(self.data_offsets[entry] as u32).to_le_bytes());
            out.extend_from_slice(&(self.data_len(entry) as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes()); // loop start
            out.extend_from_slice(&duration.saturating_sub(1).to_le_bytes());
        }
        out.extend_from_slice(&name_field(&self.name));
        out.extend_from_slice(&name_field(&format!("{}_s", self.name)));
        out.resize(self.wave_data_offset as usize, 0);
        out
    }
}

fn round_up(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

fn name_field(name: &str) -> [u8; NAME_SIZE] {
    let mut field = [0u8; NAME_SIZE];
    let bytes = name.as_bytes();
    let n = bytes.len().min(NAME_SIZE - 1);
    field[..n].copy_from_slice(&bytes[..n]);
    field
}

/// `[a-z0-9_]{1,61}` — what the XSB and the game's basename buffer accept.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME_LEN
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

#[derive(Debug)]
pub enum WriteError {
    Io(std::io::Error),
    Encode(AdpcmError),
    /// A block of the wrong size, or more blocks than planned.
    BlockShape {
        entry: usize,
        len: usize,
    },
    /// `finish` before both entries were complete.
    Incomplete {
        entry: usize,
        written: u64,
        planned: u64,
    },
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "song bank write failed: {e}"),
            Self::Encode(e) => write!(f, "song bank encode failed: {e}"),
            Self::BlockShape { entry, len } => {
                write!(
                    f,
                    "entry {entry}: block of {len} bytes does not fit the plan"
                )
            }
            Self::Incomplete {
                entry,
                written,
                planned,
            } => write!(f, "entry {entry}: {written} of {planned} blocks written"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<std::io::Error> for WriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Appends encoded blocks to a sink in file order: header, entry 0's
/// blocks, the alignment gap (zeros), entry 1's blocks.
pub struct SongBankWriter<'p, W: Write + Seek> {
    sink: W,
    plan: &'p SongBankPlan,
    entry: usize,
    block: u64,
    watermark: u64,
    silence: Option<Vec<u8>>,
}

impl<'p, W: Write + Seek> SongBankWriter<'p, W> {
    /// Write the header and position the sink at entry 0's first block.
    pub fn new(mut sink: W, plan: &'p SongBankPlan) -> Result<Self, WriteError> {
        sink.seek(SeekFrom::Start(0))?;
        let header = plan.header_bytes();
        sink.write_all(&header)?;
        Ok(Self {
            sink,
            plan,
            entry: 0,
            block: 0,
            watermark: header.len() as u64,
            silence: None,
        })
    }

    /// Bytes from offset 0 that are final (header, then every block
    /// appended so far, gap included once entry 1 starts).
    #[must_use]
    pub fn written_watermark(&self) -> u64 {
        self.watermark
    }

    /// `(entry, block)` the next append lands in; `entry == 2` when done.
    #[must_use]
    pub fn position(&self) -> (usize, u64) {
        (self.entry, self.block)
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.entry >= ENTRY_COUNT
    }

    /// Append one encoded 140-byte block.
    pub fn append_encoded(&mut self, block: &[u8]) -> Result<(), WriteError> {
        if block.len() != BLOCK_BYTES || self.entry >= ENTRY_COUNT {
            return Err(WriteError::BlockShape {
                entry: self.entry,
                len: block.len(),
            });
        }
        self.sink.write_all(block)?;
        self.block += 1;
        self.watermark += BLOCK_BYTES as u64;
        if self.block == self.plan.blocks(self.entry) {
            self.advance_entry()?;
        }
        Ok(())
    }

    /// Encode one 128-frame stereo PCM block and append it.
    pub fn append_pcm(&mut self, pcm: &[i16]) -> Result<(), WriteError> {
        let mut encoded = Vec::with_capacity(BLOCK_BYTES);
        adpcm::encode_block(pcm, self.plan.format, &mut encoded).map_err(WriteError::Encode)?;
        self.append_encoded(&encoded)
    }

    /// Fill the rest of the current entry with encoded silence (a decoder
    /// that ran short) so the file still matches the plan.
    pub fn pad_entry_with_silence(&mut self) -> Result<(), WriteError> {
        if self.entry >= ENTRY_COUNT {
            return Ok(());
        }
        let silence = match &self.silence {
            Some(s) => s.clone(),
            None => {
                let mut s = Vec::with_capacity(BLOCK_BYTES);
                adpcm::encode_block(&[0i16; 256], self.plan.format, &mut s)
                    .map_err(WriteError::Encode)?;
                self.silence = Some(s.clone());
                s
            }
        };
        while self.entry < ENTRY_COUNT && self.block < self.plan.blocks(self.entry) {
            let entry_before = self.entry;
            self.append_encoded(&silence)?;
            if self.entry != entry_before {
                break;
            }
        }
        Ok(())
    }

    /// Verify both entries are complete and flush.
    pub fn finish(mut self) -> Result<W, WriteError> {
        if self.entry < ENTRY_COUNT {
            return Err(WriteError::Incomplete {
                entry: self.entry,
                written: self.block,
                planned: self.plan.blocks(self.entry),
            });
        }
        self.sink.flush()?;
        Ok(self.sink)
    }

    fn advance_entry(&mut self) -> Result<(), WriteError> {
        self.entry += 1;
        self.block = 0;
        if self.entry < ENTRY_COUNT {
            // Zero the alignment gap explicitly so plain sinks match a
            // pre-sized file byte for byte.
            let next = self.plan.entry_data_offset(self.entry);
            let gap = next.saturating_sub(self.watermark);
            if gap > 0 {
                let zeros = [0u8; ALIGNMENT as usize];
                let mut left = gap;
                while left > 0 {
                    let n = left.min(ALIGNMENT) as usize;
                    self.sink.write_all(&zeros[..n])?;
                    left -= n as u64;
                }
            }
            self.watermark = next;
        }
        Ok(())
    }
}

/// Encode and write a whole bank in one go: `main` then `preview` PCM
/// blocks (each `&[i16; 256]`-shaped); short iterators are padded with
/// encoded silence, long ones truncated to the plan.
pub fn write_complete<W, M, P>(
    sink: W,
    plan: &SongBankPlan,
    main: M,
    preview: P,
) -> Result<W, WriteError>
where
    W: Write + Seek,
    M: IntoIterator,
    M::Item: AsRef<[i16]>,
    P: IntoIterator,
    P::Item: AsRef<[i16]>,
{
    let mut writer = SongBankWriter::new(sink, plan)?;
    append_entry(&mut writer, main)?;
    append_entry(&mut writer, preview)?;
    writer.finish()
}

fn append_entry<W, I>(writer: &mut SongBankWriter<'_, W>, blocks: I) -> Result<(), WriteError>
where
    W: Write + Seek,
    I: IntoIterator,
    I::Item: AsRef<[i16]>,
{
    let entry = writer.entry;
    if entry >= ENTRY_COUNT {
        return Ok(());
    }
    for pcm in blocks {
        if writer.entry != entry {
            break;
        }
        writer.append_pcm(pcm.as_ref())?;
    }
    if writer.entry == entry {
        writer.pad_entry_with_silence()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::xwb;
    use super::*;
    use std::io::Cursor;

    fn plan() -> SongBankPlan {
        SongBankPlan::new("zab12c", 44_100, 120 * 44_100, 15 * 44_100).expect("plan")
    }

    #[test]
    fn layout_is_stock_shaped() {
        let p = plan();
        assert_eq!(p.wave_data_offset(), 0x800);
        assert_eq!(p.blocks(0), (120u64 * 44_100).div_ceil(128));
        assert_eq!(p.frames(0), p.blocks(0) * 128);
        assert_eq!(p.entry_data_offset(0), 0x800);
        assert_eq!(p.entry_data_offset(1) % 2048, 0);
        assert!(p.entry_data_offset(1) >= 0x800 + p.data_len(0));
        assert!(p.entry_data_offset(1) < 0x800 + p.data_len(0) + 2048);
        assert_eq!(p.total_len(), p.entry_data_offset(1) + p.data_len(1));
        assert_eq!(p.block_offset(0, 0), Some(0x800));
        assert_eq!(p.block_offset(0, 3), Some(0x800 + 3 * 140));
        assert_eq!(p.block_offset(0, p.blocks(0)), Some(0x800 + p.data_len(0)));
        assert_eq!(p.block_offset(0, p.blocks(0) + 1), None);
        assert_eq!(p.block_offset(2, 0), None);
        let h = p.header_bytes();
        assert_eq!(h.len(), 0x800);
        assert_eq!(&h[0..4], b"WBND");
        assert_eq!(&h[52 + 8..52 + 8 + 6], b"zab12c");
        assert_eq!(&h[52 + 96 + 48..52 + 96 + 48 + 6], b"zab12c");
        assert_eq!(&h[52 + 96 + 48 + 64..52 + 96 + 48 + 64 + 8], b"zab12c_s");
        assert_eq!(ddr_wave_format(44_100).samples_per_block(), 128);
        assert_eq!(ddr_wave_format(48_000).block_align(), 140);
    }

    #[test]
    fn plan_rejects_bad_inputs() {
        assert!(matches!(
            SongBankPlan::new("ZAB", 44_100, 1, 1),
            Err(PlanError::BadName(_))
        ));
        assert!(matches!(
            SongBankPlan::new("zab", 0, 1, 1),
            Err(PlanError::BadRate(0))
        ));
        assert!(matches!(
            SongBankPlan::new("zab", 44_100, 0, 1),
            Err(PlanError::EmptyEntry(0))
        ));
        assert!(matches!(
            SongBankPlan::new("zab", 44_100, 1, 1 << 29),
            Err(PlanError::TooLong { entry: 1, .. })
        ));
        assert!(SongBankPlan::new(&"a".repeat(61), 44_100, 1, 1).is_ok());
        assert!(SongBankPlan::new(&"a".repeat(62), 44_100, 1, 1).is_err());
    }

    #[test]
    fn complete_bank_parses_strictly_with_stock_identity() {
        let p = SongBankPlan::new("zab12c", 44_100, 10 * 128 + 5, 3 * 128).expect("plan");
        let main = (0..p.blocks(0)).map(|i| [((i * 97) % 2000) as i16 - 1000; 256]);
        let preview = (0..p.blocks(1)).map(|_| [123i16; 256]);
        let out = write_complete(Cursor::new(Vec::new()), &p, main, preview)
            .expect("write")
            .into_inner();
        assert_eq!(out.len() as u64, p.total_len());
        let bank = xwb::parse_song_bank(&out).expect("strict parse");
        assert_eq!(bank.main_entry_index(), 0);
        assert_eq!(bank.preview_entry_index(), 1);
        assert_eq!(bank.name(), "zab12c");
        assert_eq!(bank.entries[0].name(), "zab12c");
        assert_eq!(bank.entries[1].name(), "zab12c_s");
        assert_eq!(bank.entries[0].duration as u64, 11 * 128);
        assert_eq!(bank.entries[1].duration as u64, 3 * 128);
        assert_eq!(bank.entries[0].data.len(), 11 * 140);
        assert_eq!(bank.entries[1].data.len(), 3 * 140);
        assert_eq!(bank.entries[0].loop_length as u64, 11 * 128 - 1);
        assert_eq!(bank.entries[0].format.sample_rate(), 44_100);
        assert!(bank.has_entry_names());
    }

    #[test]
    fn progressive_watermark_tracks_block_offsets() {
        let p = SongBankPlan::new("abcd", 48_000, 5 * 128, 2 * 128).expect("plan");
        let mut w = SongBankWriter::new(Cursor::new(Vec::new()), &p).expect("writer");
        assert_eq!(w.written_watermark(), 0x800);
        let block = [7u8; 140];
        for k in 1..=3u64 {
            w.append_encoded(&block).expect("append");
            assert_eq!(w.written_watermark(), p.block_offset(0, k).unwrap());
            assert_eq!(w.position(), (0, k));
        }
        w.append_encoded(&block).expect("append");
        w.append_encoded(&block).expect("append");
        // Entry 0 complete: the gap is written and the watermark jumps to
        // entry 1's start.
        assert_eq!(w.position(), (1, 0));
        assert_eq!(w.written_watermark(), p.entry_data_offset(1));
        w.append_encoded(&block).expect("append");
        assert_eq!(w.written_watermark(), p.block_offset(1, 1).unwrap());
        assert!(matches!(
            w.append_encoded(&[0u8; 139]),
            Err(WriteError::BlockShape { entry: 1, len: 139 })
        ));
        w.append_encoded(&block).expect("append");
        assert!(w.is_complete());
        assert_eq!(w.written_watermark(), p.total_len());
        assert!(matches!(
            w.append_encoded(&block),
            Err(WriteError::BlockShape { entry: 2, .. })
        ));
        let bytes = w.finish().expect("finish").into_inner();
        assert_eq!(bytes.len() as u64, p.total_len());
        assert!(bytes[0x800 + 5 * 140..p.entry_data_offset(1) as usize]
            .iter()
            .all(|&b| b == 0));
    }

    #[test]
    fn finish_before_completion_is_an_error() {
        let p = SongBankPlan::new("abcd", 48_000, 128, 128).expect("plan");
        let w = SongBankWriter::new(Cursor::new(Vec::new()), &p).expect("writer");
        assert!(matches!(
            w.finish(),
            Err(WriteError::Incomplete {
                entry: 0,
                written: 0,
                planned: 1
            })
        ));
    }

    #[test]
    fn short_and_long_iterators_are_padded_and_truncated() {
        let p = SongBankPlan::new("abcd", 44_100, 4 * 128, 2 * 128).expect("plan");
        let main = vec![[1000i16; 256]; 2]; // short by two blocks
        let preview = vec![[-1000i16; 256]; 9]; // seven too many
        let out = write_complete(Cursor::new(Vec::new()), &p, main, preview)
            .expect("write")
            .into_inner();
        let bank = xwb::parse_song_bank(&out).expect("parse");
        let mut silence = Vec::new();
        adpcm::encode_block(&[0i16; 256], p.wave_format(), &mut silence).unwrap();
        assert_eq!(&bank.entries[0].data[2 * 140..3 * 140], &silence[..]);
        assert_eq!(&bank.entries[0].data[3 * 140..4 * 140], &silence[..]);
        assert_eq!(bank.entries[1].data.len(), 2 * 140);
    }
}
