//! XWB (XACT Wave Bank) v43 writer — **fixed-header, one-entry tick profile**.
//!
//! Port of the sibling `ddr-chart-tools`' `xwb::container::write`, reduced to
//! the one shape this crate synthesizes: an in-memory (`TYPE_BUFFER`) bank
//! holding a single MS-ADPCM mono 44.1 kHz entry declared at the tick track's
//! fixed capacity. Every constant below is either a hard requirement of
//! `xactengine2_10.dll`'s bank validator or a value copied from the game's own
//! shipped in-memory banks (see the shipped feature's RE record,
//! `.agents/planning/20260725-assist-tick/research/xact-bank-format.md`).
//!
//! ## The rewrite contract (design §"Data Models")
//!
//! The engine parses the header/entry table once at `CreateInMemoryWaveBank`
//! and then reads **sample bytes** lazily from the client-owned buffer for the
//! bank's lifetime (it never copies). Declaring the one entry at the *maximum*
//! duration makes the header immutable for the process lifetime: per song only
//! the bytes in `[sample_seg_offset, sample_seg_offset + sample_seg_len)` are
//! rewritten (with an encoded-silence tail), and nothing the engine validated
//! ever changes. [`TickXwb`] carries that segment's offset/length so the
//! game-audio layer can capture a raw pointer to it at registration.
//!
//! File layout:
//!
//! ```text
//! [Header]      0x00..0x34  "WBND" + version + 5 segment descriptors
//! [Segment 0]   0x34..0x94  bank data (flags, count, name, format info)
//! [Segment 1]   0x94..0xAC  entry metadata (24 bytes, 1 entry)
//! [Segment 2]   empty       seek tables (must be empty for ADPCM)
//! [Segment 3]   0xAC..0xEC  entry name (64-byte null-padded)
//! [Segment 4]   0xEC..EOF   wave data — THE REWRITABLE SAMPLE SEGMENT
//! ```
//!
//! Deterministic and pure CPU — safe on any thread.

const MAGIC: &[u8; 4] = b"WBND";
const VERSION: u32 = 43;
const HEADER_SIZE: usize = 52;
/// Bank data segment is a fixed 96 bytes in XACT2 `header_version` 42.
const BANK_DATA_SIZE: usize = 96;
const ENTRY_META_SIZE: usize = 24;

/// `TYPE_BUFFER` (bit 0 clear) + `ENTRYNAMES` (bit 16) + bit 19 — byte for
/// byte the value both stock in-memory banks carry.
const BANK_FLAGS: u32 = 0x0009_0000;
/// The engine requires exactly 42.
const HEADER_VERSION: u32 = 42;
/// Required unconditionally, even though nothing looks the names up.
const ENTRY_NAME_ELEMENT_SIZE: usize = 64;
/// The validator's minimum, and what both stock in-memory banks use.
const ALIGNMENT: usize = 4;

/// Packed `WAVEBANKMINIWAVEFORMAT`: MS-ADPCM (codec 2), 1 channel, 44100 Hz,
/// `block_align_raw` 48 — the only codec configuration DDR's authoring tool
/// ever emits. Derived: `block_align` 70 bytes, 128 samples/block (the
/// constants in [`super::adpcm`]).
const FORMAT_BITS: u32 = 2 | (1 << 2) | (44_100 << 5) | (48 << 23);

/// A built tick wave bank: the container bytes plus the location of the
/// rewritable sample segment within them.
pub struct TickXwb {
    pub bytes: Vec<u8>,
    /// Byte offset of segment 4 (the wave data) within `bytes`.
    pub sample_seg_offset: usize,
    /// Length of the wave data — always `blocks * BLOCK_ALIGN`, and always
    /// runs exactly to the end of `bytes` (an engine validator rule).
    pub sample_seg_len: usize,
}

/// Build the fixed-capacity tick wave bank.
///
/// `name` is the bank's internal name (must byte-match the XSB's wave-bank
/// name field — the engine pairs banks by name). `total_samples` is the
/// declared entry duration and must be `blocks * SAMPLES_PER_BLOCK` for
/// `sample_seg_len == blocks * BLOCK_ALIGN`; the duration field is 28 bits
/// wide (asserted). The sample segment is filled with **encoded silence**
/// (repeated silence blocks), so the bank is playable — as silence — even
/// before the first per-song rewrite.
pub fn build(name: &str, total_samples: u32, sample_seg_len: usize) -> TickXwb {
    assert!(
        total_samples < (1 << 28),
        "XWB entry duration field is 28 bits; {total_samples} samples does not fit"
    );
    let blocks = sample_seg_len / super::adpcm::BLOCK_ALIGN;
    assert_eq!(
        blocks * super::adpcm::BLOCK_ALIGN,
        sample_seg_len,
        "sample segment must be whole ADPCM blocks"
    );
    assert_eq!(
        blocks * super::adpcm::SAMPLES_PER_BLOCK,
        total_samples as usize,
        "declared duration must equal the segment's decoded sample count"
    );

    // Segment layout. Segment 2 (seek tables) is empty; segment 4 starts at
    // the first ALIGNMENT boundary after the entry-name segment.
    let seg0_off = HEADER_SIZE;
    let seg1_off = seg0_off + BANK_DATA_SIZE;
    let seg2_off = seg1_off + ENTRY_META_SIZE;
    let seg3_off = seg2_off; // seg2 is empty
    let seg4_off = round_up(seg3_off + ENTRY_NAME_ELEMENT_SIZE, ALIGNMENT);

    let mut buf = Vec::with_capacity(seg4_off + sample_seg_len);

    // -- Header --
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&HEADER_VERSION.to_le_bytes());
    for (off, len) in [
        (seg0_off, BANK_DATA_SIZE),
        (seg1_off, ENTRY_META_SIZE),
        (seg2_off, 0),
        (seg3_off, ENTRY_NAME_ELEMENT_SIZE),
        (seg4_off, sample_seg_len),
    ] {
        buf.extend_from_slice(&(off as u32).to_le_bytes());
        buf.extend_from_slice(&(len as u32).to_le_bytes());
    }

    // -- Segment 0: bank data --
    buf.extend_from_slice(&BANK_FLAGS.to_le_bytes());
    buf.extend_from_slice(&1u32.to_le_bytes()); // entry count
    buf.extend_from_slice(&fixed_name(name));
    buf.extend_from_slice(&(ENTRY_META_SIZE as u32).to_le_bytes());
    buf.extend_from_slice(&(ENTRY_NAME_ELEMENT_SIZE as u32).to_le_bytes());
    buf.extend_from_slice(&(ALIGNMENT as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // compact_format
    buf.extend_from_slice(&0u64.to_le_bytes()); // build_time (0 = reproducible)

    // -- Segment 1: the one entry's metadata --
    // The low nibble is the entry-flags field and must be zero; the upper 28
    // bits are the duration in samples. `Duration >= loop_start + loop_length`
    // is the engine's only loop constraint; equality is what stock entries do.
    buf.extend_from_slice(&(total_samples << 4).to_le_bytes());
    buf.extend_from_slice(&FORMAT_BITS.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // data offset within segment 4
    buf.extend_from_slice(&(sample_seg_len as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes()); // loop_start
    buf.extend_from_slice(&total_samples.to_le_bytes()); // loop_length

    // -- Segment 3: the one entry's name (named after the bank) --
    buf.extend_from_slice(&fixed_name(name));

    // -- Padding to the segment-4 alignment boundary --
    buf.resize(seg4_off, 0);

    // -- Segment 4: encoded silence, one block repeated --
    let silence = super::adpcm::silence_block();
    for _ in 0..blocks {
        buf.extend_from_slice(silence);
    }
    debug_assert_eq!(buf.len(), seg4_off + sample_seg_len);

    TickXwb {
        bytes: buf,
        sample_seg_offset: seg4_off,
        sample_seg_len,
    }
}

/// Copy `name` into a fixed 64-byte null-padded field.
fn fixed_name(name: &str) -> [u8; 64] {
    let mut field = [0u8; 64];
    for (slot, &b) in field.iter_mut().zip(name.as_bytes()) {
        *slot = b;
    }
    field
}

fn round_up(value: usize, alignment: usize) -> usize {
    value.div_ceil(alignment) * alignment
}
