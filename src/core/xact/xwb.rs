//! Strict borrowed parser and identity-preserving serializer for DDR World song
//! XWB v43 streaming banks.

use std::fmt;
use std::io::Write;

use super::{adpcm, WaveFormat};

const MAGIC: &[u8; 4] = b"WBND";
const VERSION: u32 = 43;
const HEADER_VERSION: u32 = 42;
const HEADER_SIZE: usize = 52;
const BANK_DATA_SIZE: usize = 96;
const ENTRY_COUNT: usize = 2;
const ENTRY_META_SIZE: usize = 24;
const NAME_SIZE: usize = 64;
const STREAMING_ALIGNMENT: usize = 2048;
const BANK_FLAGS: u32 = 0x0009_0001;
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
    pub entries: [SongEntry<'a>; ENTRY_COUNT],
    name_bytes: &'a [u8; NAME_SIZE],
    name: &'a str,
}

impl SongBank<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        self.name
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
    name_bytes: &'a [u8; NAME_SIZE],
    name: &'a str,
}

impl SongEntry<'_> {
    #[must_use]
    pub fn name(&self) -> &str {
        self.name
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
    expect_segment(
        segments[1],
        1,
        HEADER_SIZE + BANK_DATA_SIZE,
        ENTRY_COUNT * ENTRY_META_SIZE,
    )?;
    let metadata_end = segments[1].offset + segments[1].length;
    if segments[2].length != 0 || segments[2].offset < metadata_end {
        return Err(XwbError::InvalidSegment {
            index: 2,
            field: "order or length",
        });
    }
    if segments[3].length != ENTRY_COUNT * NAME_SIZE
        || segments[3].offset < segments[2].offset
        || segments[3].offset < metadata_end
    {
        return Err(XwbError::InvalidSegment {
            index: 3,
            field: "order or length",
        });
    }
    let names_end =
        segments[3]
            .offset
            .checked_add(segments[3].length)
            .ok_or(XwbError::ArithmeticOverflow {
                field: "name segment end",
            })?;
    if segments[4].offset < names_end {
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

    if read_u32(bytes, segments[0].offset)? != BANK_FLAGS {
        return Err(XwbError::InvalidBankField { field: "flags" });
    }
    if read_u32(bytes, segments[0].offset + 4)? != ENTRY_COUNT as u32 {
        return Err(XwbError::InvalidBankField {
            field: "entry count",
        });
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

    let entries = [
        parse_entry(bytes, &segments, 0)?,
        parse_entry(bytes, &segments, 1)?,
    ];
    validate_identity(name, &entries)?;
    validate_ranges(&entries)?;

    Ok(SongBank {
        header_version,
        flags: BANK_FLAGS,
        alignment,
        compact_format,
        build_time,
        entries,
        name_bytes,
        name,
    })
}

#[derive(Clone, Copy)]
struct WriteLayout {
    segment_zero_offset: usize,
    segment_one_offset: usize,
    segment_two_offset: usize,
    segment_three_offset: usize,
    segment_four_offset: usize,
    data_offsets: [usize; ENTRY_COUNT],
    segment_four_length: usize,
    total_length: usize,
}

pub fn serialize_song_bank(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>; ENTRY_COUNT],
) -> Result<Vec<u8>, XwbError> {
    let layout = validate_write_layout(bank, replacements)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(layout.total_length)
        .map_err(|_| XwbError::AllocationFailed)?;
    write_song_bank_with_layout(bank, replacements, layout, &mut output)?;
    debug_assert_eq!(output.len(), layout.total_length);
    Ok(output)
}

/// Write one canonical song bank without materializing a second full bank.
pub fn write_song_bank(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>; ENTRY_COUNT],
    output: &mut impl Write,
) -> Result<(), XwbError> {
    let layout = validate_write_layout(bank, replacements)?;
    write_song_bank_with_layout(bank, replacements, layout, output)
}

pub fn serialized_song_bank_len(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry; ENTRY_COUNT],
) -> Result<usize, XwbError> {
    Ok(validate_stream_write_layout(bank, entries)?.total_length)
}

pub fn write_song_bank_streaming<E>(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry; ENTRY_COUNT],
    output: &mut impl Write,
    mut write_entry: impl FnMut(usize, &mut dyn Write) -> Result<(), E>,
) -> Result<(), StreamWriteError<E>> {
    let layout = validate_stream_write_layout(bank, entries).map_err(StreamWriteError::Format)?;
    write_stream_header(bank, entries, layout, output).map_err(StreamWriteError::Format)?;

    let mut wave_cursor = 0usize;
    for index in 0..ENTRY_COUNT {
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
    pub data_offsets: [usize; ENTRY_COUNT],
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
    entries: &[StreamedEntry; ENTRY_COUNT],
) -> Result<StreamPreData, XwbError> {
    let layout = validate_stream_write_layout(bank, entries)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(layout.segment_four_offset)
        .map_err(|_| XwbError::AllocationFailed)?;
    write_stream_header(bank, entries, layout, &mut bytes)?;
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
    replacements: &[EntryReplacement<'_>; ENTRY_COUNT],
) -> Result<WriteLayout, XwbError> {
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

    let segment_zero_offset = HEADER_SIZE;
    let segment_one_offset = segment_zero_offset + BANK_DATA_SIZE;
    let segment_two_offset = segment_one_offset + ENTRY_COUNT * ENTRY_META_SIZE;
    let segment_three_offset = segment_two_offset;
    let segment_four_offset = checked_round_up(
        segment_three_offset + ENTRY_COUNT * NAME_SIZE,
        STREAMING_ALIGNMENT,
    )?;
    let data_offsets = [
        0,
        checked_round_up(replacements[0].data.len(), STREAMING_ALIGNMENT)?,
    ];
    let segment_four_length = data_offsets[1]
        .checked_add(replacements[1].data.len())
        .ok_or(XwbError::ArithmeticOverflow {
            field: "wave segment length",
        })?;
    let total_length = segment_four_offset.checked_add(segment_four_length).ok_or(
        XwbError::ArithmeticOverflow {
            field: "output length",
        },
    )?;

    for (field, value) in [
        ("segment offset", segment_four_offset),
        ("segment length", segment_four_length),
        ("output length", total_length),
        ("entry zero length", replacements[0].data.len()),
        ("entry one length", replacements[1].data.len()),
        ("entry one offset", data_offsets[1]),
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

fn validate_stream_write_layout(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry; ENTRY_COUNT],
) -> Result<WriteLayout, XwbError> {
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

    let segment_zero_offset = HEADER_SIZE;
    let segment_one_offset = segment_zero_offset + BANK_DATA_SIZE;
    let segment_two_offset = segment_one_offset + ENTRY_COUNT * ENTRY_META_SIZE;
    let segment_three_offset = segment_two_offset;
    let segment_four_offset = checked_round_up(
        segment_three_offset + ENTRY_COUNT * NAME_SIZE,
        STREAMING_ALIGNMENT,
    )?;
    let data_offsets = [
        0,
        checked_round_up(entries[0].data_len, STREAMING_ALIGNMENT)?,
    ];
    let segment_four_length =
        data_offsets[1]
            .checked_add(entries[1].data_len)
            .ok_or(XwbError::ArithmeticOverflow {
                field: "wave segment length",
            })?;
    let total_length = segment_four_offset.checked_add(segment_four_length).ok_or(
        XwbError::ArithmeticOverflow {
            field: "output length",
        },
    )?;
    for (field, value) in [
        ("segment offset", segment_four_offset),
        ("segment length", segment_four_length),
        ("output length", total_length),
        ("entry zero length", entries[0].data_len),
        ("entry one length", entries[1].data_len),
        ("entry one offset", data_offsets[1]),
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

fn write_stream_header(
    bank: &SongBank<'_>,
    entries: &[StreamedEntry; ENTRY_COUNT],
    layout: WriteLayout,
    output: &mut impl Write,
) -> Result<(), XwbError> {
    write_bytes(output, MAGIC)?;
    write_u32(output, VERSION)?;
    write_u32(output, bank.header_version)?;
    for (offset, length) in [
        (layout.segment_zero_offset, BANK_DATA_SIZE),
        (layout.segment_one_offset, ENTRY_COUNT * ENTRY_META_SIZE),
        (layout.segment_two_offset, 0),
        (layout.segment_three_offset, ENTRY_COUNT * NAME_SIZE),
        (layout.segment_four_offset, layout.segment_four_length),
    ] {
        write_u32(output, to_u32(offset, "segment offset")?)?;
        write_u32(output, to_u32(length, "segment length")?)?;
    }
    write_u32(output, bank.flags)?;
    write_u32(output, ENTRY_COUNT as u32)?;
    write_bytes(output, bank.name_bytes)?;
    write_u32(output, ENTRY_META_SIZE as u32)?;
    write_u32(output, NAME_SIZE as u32)?;
    write_u32(output, bank.alignment)?;
    write_u32(output, bank.compact_format)?;
    write_bytes(output, &bank.build_time.to_le_bytes())?;
    for index in 0..ENTRY_COUNT {
        write_u32(output, entries[index].duration << 4)?;
        write_u32(output, bank.entries[index].format.packed())?;
        write_u32(
            output,
            to_u32(layout.data_offsets[index], "entry data offset")?,
        )?;
        write_u32(
            output,
            to_u32(entries[index].data_len, "entry data length")?,
        )?;
        write_u32(output, entries[index].loop_start)?;
        write_u32(output, entries[index].loop_length)?;
    }
    for entry in &bank.entries {
        write_bytes(output, entry.name_bytes)?;
    }
    let names_end = layout.segment_three_offset + ENTRY_COUNT * NAME_SIZE;
    write_zeros(output, layout.segment_four_offset - names_end)
}

fn write_song_bank_with_layout(
    bank: &SongBank<'_>,
    replacements: &[EntryReplacement<'_>; ENTRY_COUNT],
    layout: WriteLayout,
    output: &mut impl Write,
) -> Result<(), XwbError> {
    write_bytes(output, MAGIC)?;
    write_u32(output, VERSION)?;
    write_u32(output, bank.header_version)?;
    for (offset, length) in [
        (layout.segment_zero_offset, BANK_DATA_SIZE),
        (layout.segment_one_offset, ENTRY_COUNT * ENTRY_META_SIZE),
        (layout.segment_two_offset, 0),
        (layout.segment_three_offset, ENTRY_COUNT * NAME_SIZE),
        (layout.segment_four_offset, layout.segment_four_length),
    ] {
        write_u32(output, to_u32(offset, "segment offset")?)?;
        write_u32(output, to_u32(length, "segment length")?)?;
    }

    write_u32(output, bank.flags)?;
    write_u32(output, ENTRY_COUNT as u32)?;
    write_bytes(output, bank.name_bytes)?;
    write_u32(output, ENTRY_META_SIZE as u32)?;
    write_u32(output, NAME_SIZE as u32)?;
    write_u32(output, bank.alignment)?;
    write_u32(output, bank.compact_format)?;
    write_bytes(output, &bank.build_time.to_le_bytes())?;

    for index in 0..ENTRY_COUNT {
        let replacement = replacements[index];
        write_u32(output, replacement.duration << 4)?;
        write_u32(output, bank.entries[index].format.packed())?;
        write_u32(
            output,
            to_u32(layout.data_offsets[index], "entry data offset")?,
        )?;
        write_u32(output, to_u32(replacement.data.len(), "entry data length")?)?;
        write_u32(output, replacement.loop_start)?;
        write_u32(output, replacement.loop_length)?;
    }

    for entry in &bank.entries {
        write_bytes(output, entry.name_bytes)?;
    }
    let names_end = layout.segment_three_offset + ENTRY_COUNT * NAME_SIZE;
    write_zeros(output, layout.segment_four_offset - names_end)?;

    write_bytes(output, replacements[0].data)?;
    write_zeros(output, layout.data_offsets[1] - replacements[0].data.len())?;
    write_bytes(output, replacements[1].data)?;
    Ok(())
}

fn parse_entry<'a>(
    bytes: &'a [u8],
    segments: &[Segment; 5],
    index: usize,
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

    let name_bytes = read_array::<NAME_SIZE>(bytes, segments[3].offset + index * NAME_SIZE)?;
    let name = parse_entry_name(name_bytes, index)?;
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

fn validate_identity(name: &str, entries: &[SongEntry<'_>; 2]) -> Result<(), XwbError> {
    let expected_preview_len = name.len() + 2;
    let is_main = |entry: &SongEntry<'_>| entry.name() == name;
    let is_preview = |entry: &SongEntry<'_>| {
        let bytes = entry.name().as_bytes();
        bytes.len() == expected_preview_len
            && bytes.starts_with(name.as_bytes())
            && bytes[name.len()..] == *b"_s"
    };
    if !((is_main(&entries[0]) && is_preview(&entries[1]))
        || (is_preview(&entries[0]) && is_main(&entries[1])))
    {
        return Err(XwbError::InvalidEntryIdentity);
    }
    Ok(())
}

fn validate_ranges(entries: &[SongEntry<'_>; 2]) -> Result<(), XwbError> {
    let first_end = entries[0]
        .data_offset
        .checked_add(entries[0].data.len())
        .ok_or(XwbError::ArithmeticOverflow {
            field: "first entry range",
        })?;
    let second_end = entries[1]
        .data_offset
        .checked_add(entries[1].data.len())
        .ok_or(XwbError::ArithmeticOverflow {
            field: "second entry range",
        })?;
    if entries[0].data_offset < second_end && entries[1].data_offset < first_end {
        return Err(XwbError::EntryDataOverlap);
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
