//! Strict borrowed parser and identity-preserving serializer for DDR World song
//! XWB v43 streaming banks.

use std::borrow::Cow;
use std::fmt;
use std::io::Write;

use super::{adpcm, WaveFormat};

const MAGIC: &[u8; 4] = b"WBND";
const VERSION: u32 = 43;
const HEADER_VERSION: u32 = 42;
const HEADER_SIZE: usize = 52;
const BANK_DATA_SIZE: usize = 96;
/// Every song bank has at least the `<code>` main wave and the `<code>_s`
/// preview. `goru` (GOLD RUSH) additionally ships `goru_ac` / `goru_cs`
/// lyric variants — 4 entries — so the profile is 2..=[`MAX_ENTRY_COUNT`]
/// entries; the extra waves are unreachable through the cues gamemdx
/// requests (`<code>` / `<code>_s`) and pass through verbatim.
pub const MIN_ENTRY_COUNT: usize = 2;
pub const MAX_ENTRY_COUNT: usize = 8;
const ENTRY_META_SIZE: usize = 24;
const NAME_SIZE: usize = 64;
const STREAMING_ALIGNMENT: usize = 2048;
/// `WAVEBANK_TYPE_STREAMING | WAVEBANK_FLAGS_SEEKTABLES` — the flag set every
/// DDR World song bank carries, with or without friendly entry names.
const BANK_FLAGS_BASE: u32 = 0x0008_0001;
/// `WAVEBANK_FLAGS_ENTRYNAMES` — set iff segment 3 carries the 2×64-byte name
/// table. Most stock banks have it; ~33 World-era songs (e.g. `acef`, `neut`,
/// `dais`, `rhyz`) were built WITHOUT it (segment 3 = offset 0 / length 0,
/// flags `0x0008_0001`) — the "song speed does nothing for specific songs"
/// bug report of 2026-09-02.
const FLAG_ENTRY_NAMES: u32 = 0x0001_0000;
const MAX_DURATION: u32 = (1 << 28) - 1;

#[derive(Debug)]
pub enum XwbError {
    UnexpectedEof {
        offset: usize,
        wanted: usize,
        file_len: usize,
    },
    BadMagic,
    UnsupportedVersion {
        actual: u32,
    },
    InvalidHeaderVersion {
        actual: u32,
    },
    SegmentOutOfBounds {
        index: usize,
    },
    InvalidSegment {
        index: usize,
        field: &'static str,
    },
    InvalidBankField {
        field: &'static str,
    },
    InvalidBankName,
    InvalidEntryName {
        index: usize,
    },
    InvalidEntryIdentity,
    InvalidEntryField {
        index: usize,
        field: &'static str,
    },
    EntryDataOutOfBounds {
        index: usize,
    },
    EntryDataOverlap,
    EntryCodec {
        index: usize,
        source: adpcm::AdpcmError,
    },
    ArithmeticOverflow {
        field: &'static str,
    },
    AllocationFailed,
    Write(std::io::Error),
}

impl fmt::Display for XwbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof {
                offset,
                wanted,
                file_len,
            } => write!(
                f,
                "unexpected end of XWB at {offset}: wanted {wanted} bytes, file has {file_len}"
            ),
            Self::BadMagic => write!(f, "invalid XWB magic"),
            Self::UnsupportedVersion { actual } => {
                write!(f, "unsupported XWB version {actual}")
            }
            Self::InvalidHeaderVersion { actual } => {
                write!(f, "unsupported XWB header version {actual}")
            }
            Self::SegmentOutOfBounds { index } => {
                write!(f, "XWB segment {index} extends beyond the file")
            }
            Self::InvalidSegment { index, field } => {
                write!(f, "invalid XWB segment {index} {field}")
            }
            Self::InvalidBankField { field } => write!(f, "invalid XWB bank field {field}"),
            Self::InvalidBankName => write!(f, "invalid XWB bank name"),
            Self::InvalidEntryName { index } => write!(f, "invalid XWB entry {index} name"),
            Self::InvalidEntryIdentity => {
                write!(f, "XWB entries do not match bank code identities")
            }
            Self::InvalidEntryField { index, field } => {
                write!(f, "invalid XWB entry {index} field {field}")
            }
            Self::EntryDataOutOfBounds { index } => {
                write!(f, "XWB entry {index} data extends beyond segment 4")
            }
            Self::EntryDataOverlap => write!(f, "XWB entry data ranges overlap"),
            Self::EntryCodec { index, source } => {
                write!(f, "invalid XWB entry {index} ADPCM data: {source}")
            }
            Self::ArithmeticOverflow { field } => {
                write!(f, "XWB arithmetic overflow while computing {field}")
            }
            Self::AllocationFailed => write!(f, "XWB output allocation failed"),
            Self::Write(error) => write!(f, "writing XWB output failed: {error}"),
        }
    }
}

impl std::error::Error for XwbError {}

#[derive(Clone, Copy, Debug)]
struct Segment {
    offset: usize,
    length: usize,
}

#[derive(Debug)]
pub struct SongBank<'a> {
    pub header_version: u32,
    pub flags: u32,
    pub alignment: u32,
    pub compact_format: u32,
    pub build_time: u64,
    /// All entries in physical (index) order — the wave indices the XSB's
    /// cues reference. `MIN_ENTRY_COUNT..=MAX_ENTRY_COUNT` of them.
    pub entries: Vec<SongEntry<'a>>,
    /// Index of the entry that is the `<code>` main wave. Resolved from the
    /// entry-name table when the bank has one, otherwise from the durations
    /// (see [`EntryIdentitySource`]).
    main_entry_index: usize,
    /// Index of the `<code>_s` preview wave.
    preview_entry_index: usize,
    identity_source: EntryIdentitySource,
    name_bytes: &'a [u8; NAME_SIZE],
    name: &'a str,
}

/// How a parsed bank told its two entries apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryIdentitySource {
    /// Segment 3 carried `<code>` / `<code>_s` names (flags bit 0x10000 set).
    Names,
    /// No entry-name table. The main wave is the LONGER entry: every stock
    /// preview is a ~15 s clip (~650–680 k frames) while the shortest main
    /// wave in the World corpus is ~95 s, so the durations are never close.
    Duration,
}

impl SongBank<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        self.name
    }

    /// Index of the `<code>` main entry.
    #[must_use]
    pub fn main_entry_index(&self) -> usize {
        self.main_entry_index
    }

    /// Index of the `<code>_s` preview entry.
    #[must_use]
    pub fn preview_entry_index(&self) -> usize {
        self.preview_entry_index
    }

    /// Number of wave entries (2 for every song bank but `goru`).
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn identity_source(&self) -> EntryIdentitySource {
        self.identity_source
    }

    /// Whether the stock bank carried a segment-3 entry-name table. The
    /// writers reproduce the stock shape either way.
    #[must_use]
    pub fn has_entry_names(&self) -> bool {
        self.flags & FLAG_ENTRY_NAMES != 0
    }
}

#[derive(Debug)]
pub struct SongEntry<'a> {
    pub format: WaveFormat,
    pub data: &'a [u8],
    pub data_offset: usize,
    pub duration: u32,
    pub loop_start: u32,
    pub loop_length: u32,
    /// Stock name bytes when the bank has a name table; `None` for nameless
    /// banks (nothing to write back — the serializers emit no segment 3).
    name_bytes: Option<&'a [u8; NAME_SIZE]>,
    /// The entry's role name: borrowed from the file when present, otherwise
    /// synthesized (`<code>` / `<code>_s`) so every consumer keying on names
    /// (`entry.name() == bank.name()`) keeps working for nameless banks.
    name: Cow<'a, str>,
}

impl SongEntry<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EntryReplacement<'a> {
    pub data: &'a [u8],
    pub duration: u32,
    pub loop_start: u32,
    pub loop_length: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct StreamedEntry {
    pub data_len: usize,
    pub duration: u32,
    pub loop_start: u32,
    pub loop_length: u32,
}

#[derive(Debug)]
pub enum StreamWriteError<E> {
    Format(XwbError),
    Entry(E),
}

pub fn parse_song_bank(bytes: &[u8]) -> Result<SongBank<'_>, XwbError> {
    if read_array::<4>(bytes, 0)? != MAGIC {
        return Err(XwbError::BadMagic);
    }
    let version = read_u32(bytes, 4)?;
    if version != VERSION {
        return Err(XwbError::UnsupportedVersion { actual: version });
    }
    let header_version = read_u32(bytes, 8)?;
    if header_version != HEADER_VERSION {
        return Err(XwbError::InvalidHeaderVersion {
            actual: header_version,
        });
    }

    let mut segments = [Segment {
        offset: 0,
        length: 0,
    }; 5];
    for (index, segment) in segments.iter_mut().enumerate() {
        segment.offset = read_u32(bytes, 12 + index * 8)? as usize;
        segment.length = read_u32(bytes, 16 + index * 8)? as usize;
        let end =
            segment
                .offset
                .checked_add(segment.length)
                .ok_or(XwbError::ArithmeticOverflow {
                    field: "segment end",
                })?;
        if end > bytes.len() {
            return Err(XwbError::SegmentOutOfBounds { index });
        }
    }

    expect_segment(segments[0], 0, HEADER_SIZE, BANK_DATA_SIZE)?;
    // The entry count lives in BANKDATA and sizes segments 1 and 3.
    let entry_count = read_u32(bytes, segments[0].offset + 4)? as usize;
    if !(MIN_ENTRY_COUNT..=MAX_ENTRY_COUNT).contains(&entry_count) {
        return Err(XwbError::InvalidBankField {
            field: "entry count",
        });
    }
    expect_segment(
        segments[1],
        1,
        HEADER_SIZE + BANK_DATA_SIZE,
        entry_count * ENTRY_META_SIZE,
    )?;
    let metadata_end = segments[1].offset + segments[1].length;
    if segments[2].length != 0 || segments[2].offset < metadata_end {
        return Err(XwbError::InvalidSegment {
            index: 2,
            field: "order or length",
        });
    }
    // Segment 3 (entry names) is optional: stock banks either carry the full
    // N×64-byte table after the seek segment, or omit it entirely (offset 0,
    // length 0). Anything else is a shape we don't understand.
    let has_entry_names = match (segments[3].offset, segments[3].length) {
        (0, 0) => false,
        (offset, length)
            if length == entry_count * NAME_SIZE
                && offset >= segments[2].offset
                && offset >= metadata_end =>
        {
            true
        }
        _ => {
            return Err(XwbError::InvalidSegment {
                index: 3,
                field: "order or length",
            });
        }
    };
    let pre_data_end = if has_entry_names {
        segments[3]
            .offset
            .checked_add(segments[3].length)
            .ok_or(XwbError::ArithmeticOverflow {
                field: "name segment end",
            })?
    } else {
        segments[2].offset
    };
    if segments[4].offset < pre_data_end {
        return Err(XwbError::InvalidSegment {
            index: 4,
            field: "order",
        });
    }
    if segments[4].offset % STREAMING_ALIGNMENT != 0 {
        return Err(XwbError::InvalidSegment {
            index: 4,
            field: "alignment",
        });
    }
    if segments[4].offset + segments[4].length != bytes.len() {
        return Err(XwbError::InvalidSegment {
            index: 4,
            field: "EOF framing",
        });
    }

    let flags = read_u32(bytes, segments[0].offset)?;
    let expected_flags = if has_entry_names {
        BANK_FLAGS_BASE | FLAG_ENTRY_NAMES
    } else {
        BANK_FLAGS_BASE
    };
    if flags != expected_flags {
        return Err(XwbError::InvalidBankField { field: "flags" });
    }
    let name_bytes = read_array::<NAME_SIZE>(bytes, segments[0].offset + 8)?;
    let name = parse_bank_name(name_bytes)?;
    if read_u32(bytes, segments[0].offset + 72)? != ENTRY_META_SIZE as u32 {
        return Err(XwbError::InvalidBankField {
            field: "entry metadata size",
        });
    }
    if read_u32(bytes, segments[0].offset + 76)? != NAME_SIZE as u32 {
        return Err(XwbError::InvalidBankField {
            field: "entry name size",
        });
    }
    let alignment = read_u32(bytes, segments[0].offset + 80)?;
    if alignment != STREAMING_ALIGNMENT as u32 {
        return Err(XwbError::InvalidBankField { field: "alignment" });
    }
    let compact_format = read_u32(bytes, segments[0].offset + 84)?;
    if compact_format != 0 {
        return Err(XwbError::InvalidBankField {
            field: "compact format",
        });
    }
    let build_time = read_u64(bytes, segments[0].offset + 88)?;

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entry_count)
        .map_err(|_| XwbError::AllocationFailed)?;
    for index in 0..entry_count {
        entries.push(parse_entry(bytes, &segments, index, has_entry_names)?);
    }
    let (main_entry_index, preview_entry_index, identity_source) = if has_entry_names {
        let (main, preview) = resolve_identity_by_name(name, &entries)?;
        (main, preview, EntryIdentitySource::Names)
    } else {
        // Duration can only tell TWO entries apart (no way to know which of
        // several long waves the `<code>` cue plays) — and every nameless
        // stock bank is a plain 2-entry bank.
        if entries.len() != MIN_ENTRY_COUNT {
            return Err(XwbError::InvalidEntryIdentity);
        }
        let main = resolve_identity_by_duration(&entries)?;
        // Synthesize the role names so downstream `entry.name() == bank.name()`
        // consumers see the same contract a named bank offers.
        entries[main].name = Cow::Borrowed(name);
        entries[1 - main].name = Cow::Owned(format!("{name}_s"));
        (main, 1 - main, EntryIdentitySource::Duration)
    };
    validate_ranges(&entries)?;

    Ok(SongBank {
        header_version,
        flags,
        alignment,
        compact_format,
        build_time,
        entries,
        main_entry_index,
        preview_entry_index,
        identity_source,
        name_bytes,
        name,
    })
}

#[derive(Clone)]
struct WriteLayout {
    segment_zero_offset: usize,
    segment_one_offset: usize,
    segment_two_offset: usize,
    segment_three_offset: usize,
    segment_four_offset: usize,
    data_offsets: Vec<usize>,
    segment_four_length: usize,
    total_length: usize,
}

/// `replacements` / `entries` must carry exactly one element per bank entry.
fn expect_entry_count(bank: &SongBank<'_>, provided: usize) -> Result<(), XwbError> {
    if provided != bank.entries.len() {
        return Err(XwbError::InvalidBankField {
            field: "entry count",
        });
    }
    Ok(())
}

pub fn serialize_song_bank(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>],
) -> Result<Vec<u8>, XwbError> {
    let layout = validate_write_layout(bank, replacements)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(layout.total_length)
        .map_err(|_| XwbError::AllocationFailed)?;
    write_song_bank_with_layout(bank, replacements, &layout, &mut output)?;
    debug_assert_eq!(output.len(), layout.total_length);
    Ok(output)
}

/// Write one canonical song bank without materializing a second full bank.
pub fn write_song_bank(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>],
    output: &mut impl Write,
) -> Result<(), XwbError> {
    let layout = validate_write_layout(bank, replacements)?;
    write_song_bank_with_layout(bank, replacements, &layout, output)
}

pub fn serialized_song_bank_len(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry],
) -> Result<usize, XwbError> {
    Ok(validate_stream_write_layout(bank, entries)?.total_length)
}

pub fn write_song_bank_streaming<E>(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry],
    output: &mut impl Write,
    mut write_entry: impl FnMut(usize, &mut dyn Write) -> Result<(), E>,
) -> Result<(), StreamWriteError<E>> {
    let layout = validate_stream_write_layout(bank, entries).map_err(StreamWriteError::Format)?;
    write_stream_header(bank, entries, &layout, output).map_err(StreamWriteError::Format)?;

    let mut wave_cursor = 0usize;
    for index in 0..entries.len() {
        if layout.data_offsets[index] > wave_cursor {
            write_zeros(output, layout.data_offsets[index] - wave_cursor)
                .map_err(StreamWriteError::Format)?;
        }
        let mut exact = ExactLengthWriter {
            inner: output,
            remaining: entries[index].data_len,
        };
        write_entry(index, &mut exact).map_err(StreamWriteError::Entry)?;
        if exact.remaining != 0 {
            return Err(StreamWriteError::Format(XwbError::InvalidEntryField {
                index,
                field: "streamed payload length",
            }));
        }
        wave_cursor = layout.data_offsets[index]
            .checked_add(entries[index].data_len)
            .ok_or(StreamWriteError::Format(XwbError::ArithmeticOverflow {
                field: "streamed payload end",
            }))?;
    }
    Ok(())
}

/// The canonical streaming pre-data block plus the physical layout facts a
/// virtual bank serves against.
///
/// `bytes` is exactly the serializer's prefix `[0, wave_data_offset)`:
/// 52-byte header, BANKDATA, entry metadata, entry names, zero pad.
#[derive(Debug)]
pub struct StreamPreData {
    /// Synthesized bytes `[0, wave_data_offset)` of the streamed bank.
    pub bytes: Vec<u8>,
    /// Per-entry data offsets within the wave-data segment.
    pub data_offsets: Vec<usize>,
    /// Segment-4 (wave data) file offset — the pre-data length.
    pub wave_data_offset: usize,
    /// Total streamed file length (`== serialized_song_bank_len`).
    pub total_length: usize,
}

/// Emit the streaming pre-data block through the same private
/// `validate_stream_write_layout` + `write_stream_header` path
/// [`write_song_bank_streaming`] runs, so the block is byte-identical to the
/// serializer's prefix by construction (one canonical emitter, no
/// duplication).
pub fn stream_pre_data(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry],
) -> Result<StreamPreData, XwbError> {
    let layout = validate_stream_write_layout(bank, entries)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(layout.segment_four_offset)
        .map_err(|_| XwbError::AllocationFailed)?;
    write_stream_header(bank, entries, &layout, &mut bytes)?;
    debug_assert_eq!(bytes.len(), layout.segment_four_offset);
    Ok(StreamPreData {
        bytes,
        data_offsets: layout.data_offsets,
        wave_data_offset: layout.segment_four_offset,
        total_length: layout.total_length,
    })
}

fn validate_write_layout(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>],
) -> Result<WriteLayout, XwbError> {
    expect_entry_count(bank, replacements.len())?;
    for (index, replacement) in replacements.iter().enumerate() {
        if replacement.duration > MAX_DURATION {
            return Err(XwbError::InvalidEntryField {
                index,
                field: "duration",
            });
        }
        adpcm::validate_generated_layout(
            replacement.data.len(),
            bank.entries[index].format,
            replacement.duration,
        )
        .map_err(|source| XwbError::EntryCodec { index, source })?;
        validate_loop(
            index,
            replacement.duration,
            replacement.loop_start,
            replacement.loop_length,
        )?;
    }
    let mut data_lens = Vec::new();
    data_lens
        .try_reserve_exact(replacements.len())
        .map_err(|_| XwbError::AllocationFailed)?;
    data_lens.extend(
        replacements
            .iter()
            .map(|replacement| replacement.data.len()),
    );
    compute_write_layout(bank, &data_lens)
}

fn validate_stream_write_layout(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry],
) -> Result<WriteLayout, XwbError> {
    expect_entry_count(bank, entries.len())?;
    for (index, entry) in entries.iter().enumerate() {
        if entry.duration > MAX_DURATION {
            return Err(XwbError::InvalidEntryField {
                index,
                field: "duration",
            });
        }
        // The PARSER's layout rule, not the generated-content whole-block
        // rule: the stream header may advertise a verbatim stock entry (the
        // preview passthrough), whose duration sits INSIDE its final block —
        // the emission contract must accept exactly what `parse_song_bank`
        // accepts (live HeaderSynth refusal, 2026-08-10). Generated streams
        // (whole blocks, frames == blocks × samples) pass the same rule.
        adpcm::validate_encoded_layout(entry.data_len, bank.entries[index].format, entry.duration)
            .map_err(|source| XwbError::EntryCodec { index, source })?;
        validate_loop(index, entry.duration, entry.loop_start, entry.loop_length)?;
    }
    let mut data_lens = Vec::new();
    data_lens
        .try_reserve_exact(entries.len())
        .map_err(|_| XwbError::AllocationFailed)?;
    data_lens.extend(entries.iter().map(|entry| entry.data_len));
    compute_write_layout(bank, &data_lens)
}

/// The canonical streamed layout for `data_lens.len()` entries: header,
/// BANKDATA, N metadata rows, (N name rows when the stock bank had them),
/// wave data at the next 2048 boundary; each entry's data at the next 2048
/// boundary after the previous entry's data (entry 0 at 0) — the stock
/// packer's rule, so physical index order == file order.
fn compute_write_layout(bank: &SongBank<'_>, data_lens: &[usize]) -> Result<WriteLayout, XwbError> {
    let entry_count = data_lens.len();
    let segment_zero_offset = HEADER_SIZE;
    let segment_one_offset = segment_zero_offset + BANK_DATA_SIZE;
    let segment_two_offset = segment_one_offset + entry_count * ENTRY_META_SIZE;
    let segment_three_offset = segment_two_offset;
    let pre_data_end = if bank.has_entry_names() {
        segment_three_offset + entry_count * NAME_SIZE
    } else {
        segment_three_offset
    };
    let segment_four_offset = checked_round_up(pre_data_end, STREAMING_ALIGNMENT)?;

    let mut data_offsets = Vec::new();
    data_offsets
        .try_reserve_exact(entry_count)
        .map_err(|_| XwbError::AllocationFailed)?;
    let mut cursor = 0usize;
    for (index, &len) in data_lens.iter().enumerate() {
        let offset = if index == 0 {
            0
        } else {
            checked_round_up(cursor, STREAMING_ALIGNMENT)?
        };
        let _ = to_u32(offset, "entry data offset")?;
        let _ = to_u32(len, "entry data length")?;
        data_offsets.push(offset);
        cursor = offset
            .checked_add(len)
            .ok_or(XwbError::ArithmeticOverflow {
                field: "wave segment length",
            })?;
    }
    let segment_four_length = cursor;
    let total_length = segment_four_offset.checked_add(segment_four_length).ok_or(
        XwbError::ArithmeticOverflow {
            field: "output length",
        },
    )?;
    for (field, value) in [
        ("segment offset", segment_four_offset),
        ("segment length", segment_four_length),
        ("output length", total_length),
    ] {
        let _ = to_u32(value, field)?;
    }
    Ok(WriteLayout {
        segment_zero_offset,
        segment_one_offset,
        segment_two_offset,
        segment_three_offset,
        segment_four_offset,
        data_offsets,
        segment_four_length,
        total_length,
    })
}

/// One entry's metadata row as the two writers see it.
#[derive(Clone, Copy)]
struct EntryHeader {
    duration: u32,
    data_len: usize,
    loop_start: u32,
    loop_length: u32,
}

/// Emit everything before segment 4: header + segment table, BANKDATA, entry
/// metadata, then — ONLY when the stock bank had one — the entry-name table,
/// then zero pad up to the wave data. Nameless stock banks are reproduced
/// nameless (segment 3 = 0/0, no ENTRYNAMES flag) so the served header keeps
/// the stock shape apart from the durations/lengths/offsets we intend to change.
fn write_pre_data(
    bank: &SongBank<'_>,
    entries: impl ExactSizeIterator<Item = EntryHeader>,
    layout: &WriteLayout,
    output: &mut impl Write,
) -> Result<(), XwbError> {
    let entry_count = bank.entries.len();
    debug_assert_eq!(entries.len(), entry_count);
    let has_names = bank.has_entry_names();
    let (names_offset, names_len) = if has_names {
        (layout.segment_three_offset, entry_count * NAME_SIZE)
    } else {
        (0, 0)
    };
    write_bytes(output, MAGIC)?;
    write_u32(output, VERSION)?;
    write_u32(output, bank.header_version)?;
    for (offset, length) in [
        (layout.segment_zero_offset, BANK_DATA_SIZE),
        (layout.segment_one_offset, entry_count * ENTRY_META_SIZE),
        (layout.segment_two_offset, 0),
        (names_offset, names_len),
        (layout.segment_four_offset, layout.segment_four_length),
    ] {
        write_u32(output, to_u32(offset, "segment offset")?)?;
        write_u32(output, to_u32(length, "segment length")?)?;
    }
    write_u32(output, bank.flags)?;
    write_u32(output, to_u32(entry_count, "entry count")?)?;
    write_bytes(output, bank.name_bytes)?;
    write_u32(output, ENTRY_META_SIZE as u32)?;
    write_u32(output, NAME_SIZE as u32)?;
    write_u32(output, bank.alignment)?;
    write_u32(output, bank.compact_format)?;
    write_bytes(output, &bank.build_time.to_le_bytes())?;
    for (index, header) in entries.enumerate() {
        write_u32(output, header.duration << 4)?;
        write_u32(output, bank.entries[index].format.packed())?;
        write_u32(
            output,
            to_u32(layout.data_offsets[index], "entry data offset")?,
        )?;
        write_u32(output, to_u32(header.data_len, "entry data length")?)?;
        write_u32(output, header.loop_start)?;
        write_u32(output, header.loop_length)?;
    }
    let mut pre_data_end = layout.segment_two_offset;
    if has_names {
        for (index, entry) in bank.entries.iter().enumerate() {
            let name_bytes = entry
                .name_bytes
                .ok_or(XwbError::InvalidEntryName { index })?;
            write_bytes(output, name_bytes)?;
        }
        pre_data_end = layout.segment_three_offset + entry_count * NAME_SIZE;
    }
    write_zeros(output, layout.segment_four_offset - pre_data_end)
}

fn write_stream_header(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry],
    layout: &WriteLayout,
    output: &mut impl Write,
) -> Result<(), XwbError> {
    let headers = entries.iter().map(|entry| EntryHeader {
        duration: entry.duration,
        data_len: entry.data_len,
        loop_start: entry.loop_start,
        loop_length: entry.loop_length,
    });
    write_pre_data(bank, headers, layout, output)
}

fn write_song_bank_with_layout(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>],
    layout: &WriteLayout,
    output: &mut impl Write,
) -> Result<(), XwbError> {
    let headers = replacements.iter().map(|replacement| EntryHeader {
        duration: replacement.duration,
        data_len: replacement.data.len(),
        loop_start: replacement.loop_start,
        loop_length: replacement.loop_length,
    });
    write_pre_data(bank, headers, layout, output)?;

    let mut cursor = 0usize;
    for (index, replacement) in replacements.iter().enumerate() {
        write_zeros(output, layout.data_offsets[index] - cursor)?;
        write_bytes(output, replacement.data)?;
        cursor = layout.data_offsets[index] + replacement.data.len();
    }
    Ok(())
}

fn parse_entry<'a>(
    bytes: &'a [u8],
    segments: &[Segment; 5],
    index: usize,
    has_entry_names: bool,
) -> Result<SongEntry<'a>, XwbError> {
    let metadata = segments[1].offset + index * ENTRY_META_SIZE;
    let flags_and_duration = read_u32(bytes, metadata)?;
    if flags_and_duration & 0xf != 0 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "flags",
        });
    }
    let duration = flags_and_duration >> 4;
    if duration == 0 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "duration",
        });
    }
    let format = WaveFormat::from_packed(read_u32(bytes, metadata + 4)?);
    validate_song_format(index, format)?;
    let data_offset = read_u32(bytes, metadata + 8)? as usize;
    if index > 0 && data_offset % STREAMING_ALIGNMENT != 0 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "data alignment",
        });
    }
    let data_length = read_u32(bytes, metadata + 12)? as usize;
    let data_end = data_offset
        .checked_add(data_length)
        .ok_or(XwbError::ArithmeticOverflow {
            field: "entry data end",
        })?;
    if data_end > segments[4].length {
        return Err(XwbError::EntryDataOutOfBounds { index });
    }
    let absolute_data =
        segments[4]
            .offset
            .checked_add(data_offset)
            .ok_or(XwbError::ArithmeticOverflow {
                field: "entry absolute data offset",
            })?;
    let data = slice(bytes, absolute_data, data_length)?;
    adpcm::validate_encoded_layout(data_length, format, duration)
        .map_err(|source| XwbError::EntryCodec { index, source })?;

    let loop_start = read_u32(bytes, metadata + 16)?;
    let loop_length = read_u32(bytes, metadata + 20)?;
    validate_loop(index, duration, loop_start, loop_length)?;

    let (name_bytes, name) = if has_entry_names {
        let name_bytes = read_array::<NAME_SIZE>(bytes, segments[3].offset + index * NAME_SIZE)?;
        let name = parse_entry_name(name_bytes, index)?;
        (Some(name_bytes), Cow::Borrowed(name))
    } else {
        // Placeholder until the bank-level identity pass assigns the role
        // name (`parse_song_bank` overwrites it for both entries).
        (None, Cow::Borrowed(""))
    };
    Ok(SongEntry {
        format,
        data,
        data_offset,
        duration,
        loop_start,
        loop_length,
        name_bytes,
        name,
    })
}

fn validate_song_format(index: usize, format: WaveFormat) -> Result<(), XwbError> {
    if format.codec() != WaveFormat::CODEC_ADPCM {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "codec",
        });
    }
    if format.channels() != 2 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "channels",
        });
    }
    if format.sample_rate() == 0 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "sample rate",
        });
    }
    if format.block_align_raw() != 48 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "block alignment",
        });
    }
    if format.bits_per_sample_flag() != 0 {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "bits flag",
        });
    }
    Ok(())
}

fn validate_loop(
    index: usize,
    duration: u32,
    loop_start: u32,
    loop_length: u32,
) -> Result<(), XwbError> {
    let loop_end = loop_start
        .checked_add(loop_length)
        .ok_or(XwbError::InvalidEntryField {
            index,
            field: "loop overflow",
        })?;
    if loop_end > duration {
        return Err(XwbError::InvalidEntryField {
            index,
            field: "loop extent",
        });
    }
    Ok(())
}

/// Named banks: exactly one entry is `<code>` and exactly one `<code>_s`;
/// any further entries (`goru_ac` / `goru_cs`) are variants the engine's
/// cues never reach and pass through verbatim. Returns `(main, preview)`.
fn resolve_identity_by_name(
    name: &str,
    entries: &[SongEntry<'_>],
) -> Result<(usize, usize), XwbError> {
    let expected_preview_len = name.len() + 2;
    let is_preview = |entry: &SongEntry<'_>| {
        let bytes = entry.name().as_bytes();
        bytes.len() == expected_preview_len
            && bytes.starts_with(name.as_bytes())
            && bytes[name.len()..] == *b"_s"
    };
    let mut main = None;
    let mut preview = None;
    for (index, entry) in entries.iter().enumerate() {
        if entry.name() == name {
            if main.replace(index).is_some() {
                return Err(XwbError::InvalidEntryIdentity);
            }
        } else if is_preview(entry) && preview.replace(index).is_some() {
            return Err(XwbError::InvalidEntryIdentity);
        }
    }
    match (main, preview) {
        (Some(main), Some(preview)) => Ok((main, preview)),
        _ => Err(XwbError::InvalidEntryIdentity),
    }
}

/// Nameless 2-entry banks: the main wave is the strictly longer entry.
/// Refuses when the two durations don't separate clearly (equal, or within a
/// factor of 2 — real previews are ~15 s against ≥ ~90 s mains, so a near tie
/// means this is not a `<code>`/`<code>_s` song bank and we must not guess).
fn resolve_identity_by_duration(entries: &[SongEntry<'_>]) -> Result<usize, XwbError> {
    let (short, long, main) = if entries[0].duration > entries[1].duration {
        (entries[1].duration, entries[0].duration, 0)
    } else {
        (entries[0].duration, entries[1].duration, 1)
    };
    if short == 0 || long / short < 2 {
        return Err(XwbError::InvalidEntryIdentity);
    }
    Ok(main)
}

/// No two entries' data ranges may overlap (any physical order is fine).
fn validate_ranges(entries: &[SongEntry<'_>]) -> Result<(), XwbError> {
    let mut ranges = Vec::new();
    ranges
        .try_reserve_exact(entries.len())
        .map_err(|_| XwbError::AllocationFailed)?;
    for entry in entries {
        let end = entry.data_offset.checked_add(entry.data.len()).ok_or(
            XwbError::ArithmeticOverflow {
                field: "entry range",
            },
        )?;
        ranges.push((entry.data_offset, end));
    }
    for (index, &(start, end)) in ranges.iter().enumerate() {
        for &(other_start, other_end) in &ranges[index + 1..] {
            if start < other_end && other_start < end {
                return Err(XwbError::EntryDataOverlap);
            }
        }
    }
    Ok(())
}

fn parse_bank_name(bytes: &[u8; NAME_SIZE]) -> Result<&str, XwbError> {
    let name = parse_name(bytes).ok_or(XwbError::InvalidBankName)?;
    if name.is_empty() || name.len() > NAME_SIZE - 3 {
        return Err(XwbError::InvalidBankName);
    }
    Ok(name)
}

fn parse_entry_name(bytes: &[u8; NAME_SIZE], index: usize) -> Result<&str, XwbError> {
    parse_name(bytes).ok_or(XwbError::InvalidEntryName { index })
}

fn parse_name(bytes: &[u8; NAME_SIZE]) -> Option<&str> {
    if bytes[NAME_SIZE - 1] != 0 {
        return None;
    }
    let end = bytes.iter().position(|&byte| byte == 0)?;
    let name = std::str::from_utf8(&bytes[..end]).ok()?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    Some(name)
}

fn expect_segment(
    segment: Segment,
    index: usize,
    offset: usize,
    length: usize,
) -> Result<(), XwbError> {
    if segment.offset != offset {
        return Err(XwbError::InvalidSegment {
            index,
            field: "offset",
        });
    }
    if segment.length != length {
        return Err(XwbError::InvalidSegment {
            index,
            field: "length",
        });
    }
    Ok(())
}

fn checked_round_up(value: usize, alignment: usize) -> Result<usize, XwbError> {
    let adjusted = value
        .checked_add(alignment - 1)
        .ok_or(XwbError::ArithmeticOverflow { field: "alignment" })?;
    Ok(adjusted / alignment * alignment)
}

fn to_u32(value: usize, field: &'static str) -> Result<u32, XwbError> {
    u32::try_from(value).map_err(|_| XwbError::ArithmeticOverflow { field })
}

struct ExactLengthWriter<'a, W: Write + ?Sized> {
    inner: &'a mut W,
    remaining: usize,
}

impl<W: Write + ?Sized> Write for ExactLengthWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "streamed XWB payload exceeded declared length",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn write_u32(output: &mut impl Write, value: u32) -> Result<(), XwbError> {
    write_bytes(output, &value.to_le_bytes())
}

fn write_bytes(output: &mut impl Write, bytes: &[u8]) -> Result<(), XwbError> {
    output.write_all(bytes).map_err(XwbError::Write)
}

fn write_zeros(output: &mut impl Write, mut length: usize) -> Result<(), XwbError> {
    const ZEROS: [u8; STREAMING_ALIGNMENT] = [0; STREAMING_ALIGNMENT];
    while length > 0 {
        let chunk = length.min(ZEROS.len());
        write_bytes(output, &ZEROS[..chunk])?;
        length -= chunk;
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, XwbError> {
    Ok(u32::from_le_bytes(*read_array::<4>(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, XwbError> {
    Ok(u64::from_le_bytes(*read_array::<8>(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Result<&[u8; N], XwbError> {
    slice(bytes, offset, N)?
        .try_into()
        .map_err(|_| XwbError::UnexpectedEof {
            offset,
            wanted: N,
            file_len: bytes.len(),
        })
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], XwbError> {
    let end = offset
        .checked_add(length)
        .ok_or(XwbError::ArithmeticOverflow { field: "slice end" })?;
    bytes.get(offset..end).ok_or(XwbError::UnexpectedEof {
        offset,
        wanted: length,
        file_len: bytes.len(),
    })
}
