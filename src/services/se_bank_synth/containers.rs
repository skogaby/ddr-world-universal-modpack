//! The pure container-synthesis API: constants, one-time container build,
//! and per-song track synthesis.
//!
//! **Zero crate dependencies** (only `super::{adpcm,xsb,xwb}`, which are
//! equally pure) — deliberately, so the whole format layer can be compiled
//! stand-alone on a host machine for offline validation against the sibling
//! `ddr-se-bank` tool, without the DLL crate's Windows-only dependencies.
//! Everything here is deterministic byte-work, callable from any thread.

use super::{adpcm, xsb, xwb};

/// Tick track sample rate. Matches the clap asset and the only sample rate
/// DDR's own banks use; there is no resampling anywhere in this pipeline.
pub const TICK_RATE_HZ: u32 = 44_100;
/// Fixed track capacity (D15 of the song-rate streaming design,
/// maintainer-accepted): 1200 s WALL keeps 300 s of chart content covered at
/// the slowest supported song rate (25 %) — and 300 s covers every standard
/// chart (≲ 2:00) with headroom. Ticks beyond it are dropped with a count
/// (FR-8). Raising it is this one constant.
pub const TICK_CAPACITY_MS: u32 = 1_200_000;
/// The bank's/cue's internal name. Deliberately unrelated to any file name —
/// the engine pairs banks and resolves cues by *internal* name only. Equally
/// deliberately NOT the shipped per-tick bank's name (`asti`): the engine's
/// name-keyed pairing is global, so while both banks coexist (plan Steps
/// 2–3) a shared name cross-pairs the tick sound bank with the shipped
/// 214 ms clap wave bank — live-confirmed on the cabinet as "one clap, then
/// silence" (deploy log 2026-07-29).
pub const BANK_NAME: &str = "astk";

/// Raw capacity in samples (exact: `TICK_CAPACITY_MS` × 44.1).
const CAPACITY_RAW_SAMPLES: usize = (TICK_CAPACITY_MS as u64 * TICK_RATE_HZ as u64 / 1000) as usize;
/// Capacity rounded up to whole ADPCM blocks — the fixed size everything is
/// declared and encoded at.
const CAPACITY_BLOCKS: usize = CAPACITY_RAW_SAMPLES.div_ceil(adpcm::SAMPLES_PER_BLOCK);
/// Decoded length of the fixed entry, in samples (= the declared duration).
const CAPACITY_PADDED_SAMPLES: usize = CAPACITY_BLOCKS * adpcm::SAMPLES_PER_BLOCK;
/// Encoded length of the fixed sample segment, in bytes (~28.9 MB —
/// allocated lazily, only when Assist Tick is actually used).
const SAMPLE_SEG_LEN: usize = CAPACITY_BLOCKS * adpcm::BLOCK_ALIGN;

/// The one-time containers: a sound bank and a wave bank whose single ADPCM
/// entry is declared at [`TICK_CAPACITY_MS`], plus the location of the wave
/// bank's rewritable sample segment.
pub struct TickContainers {
    pub xsb_bytes: Vec<u8>,
    pub xwb_bytes: Vec<u8>,
    /// Byte offset of the sample segment within `xwb_bytes`.
    pub sample_seg_offset: usize,
    /// Length of the sample segment — every [`synthesize_track`] output is
    /// exactly this many bytes, and the segment runs exactly to the end of
    /// `xwb_bytes` (an engine validator rule; asserted).
    pub sample_seg_len: usize,
}

/// Build the one-cue SE-profile XSB and the fixed-header XWB. Built once
/// (boot); the XWB's sample segment starts as encoded silence.
pub fn build_tick_containers() -> TickContainers {
    let xsb_bytes = xsb::build_se(BANK_NAME);
    let wave = xwb::build(BANK_NAME, CAPACITY_PADDED_SAMPLES as u32, SAMPLE_SEG_LEN);
    debug_assert_eq!(
        wave.sample_seg_offset + wave.sample_seg_len,
        wave.bytes.len()
    );
    TickContainers {
        xsb_bytes,
        xwb_bytes: wave.bytes,
        sample_seg_offset: wave.sample_seg_offset,
        sample_seg_len: wave.sample_seg_len,
    }
}

/// What one per-song synthesis produced.
pub struct SynthResult {
    /// Encoded sample segment — exactly [`TickContainers::sample_seg_len`]
    /// bytes, silence-padded past the last clap.
    pub encoded: Vec<u8>,
    /// Claps mixed into the track (including clipped ones).
    pub mixed: usize,
    /// `content_ms < 0` inputs, clipped to position 0 (still audible, once) —
    /// possible for an early first note under large negative shifts.
    pub clipped: usize,
    /// `content_ms ≥ TICK_CAPACITY_MS` inputs, dropped (FR-8 truncation).
    pub dropped: usize,
}

/// Scale mono PCM by a linear-amplitude percentage with i32 headroom,
/// saturating to i16 (the mixer's own convention — overlapping claps already
/// saturate the same way). Truncation toward zero is deliberate: symmetric
/// around zero and inaudible at these gain levels.
///
/// 100 returns an identical copy; callers should skip the call entirely at
/// 100 — the assist-tick identity path does (design R4: a default-valued
/// TICK EFFECT VOLUME row must produce a byte-identical track).
pub fn scale_pcm(pcm: &[i16], percent: i32) -> Vec<i16> {
    pcm.iter()
        .map(|&s| ((s as i32 * percent) / 100).clamp(-32768, 32767) as i16)
        .collect()
}

/// Mix claps into a fixed-capacity mono buffer and MS-ADPCM-encode it.
///
/// Each `content_ms` value places one copy of `clap_pcm` with its first
/// sample at `round(ms × 44.1)` — sample-exact by construction, which is the
/// entire point of the pre-mixed track. Overlapping claps (16th bursts — the
/// clap is ~214 ms) sum with i32 headroom and saturate to i16. A clap
/// running past the capacity is truncated at the buffer end.
///
/// Pure CPU, no allocation surprises beyond the one ~106 MB mix buffer —
/// sized for a background thread (design NFR-1), not the judge dispatch.
pub fn synthesize_track(clap_pcm: &[i16], content_ms: &[i32]) -> SynthResult {
    let mut buf = vec![0i16; CAPACITY_PADDED_SAMPLES];
    let mut mixed = 0usize;
    let mut clipped = 0usize;
    let mut dropped = 0usize;

    for &ms in content_ms {
        let pos = if ms < 0 {
            clipped += 1;
            0usize
        } else if ms >= TICK_CAPACITY_MS as i32 {
            dropped += 1;
            continue;
        } else {
            // round(ms / 1000 × rate); i64 because 300 000 × 44 100 > u32.
            ((ms as i64 * TICK_RATE_HZ as i64 + 500) / 1000) as usize
        };
        let room = buf.len().saturating_sub(pos);
        for (slot, &s) in buf[pos..]
            .iter_mut()
            .zip(&clap_pcm[..clap_pcm.len().min(room)])
        {
            *slot = (*slot as i32 + s as i32).clamp(-32768, 32767) as i16;
        }
        mixed += 1;
    }

    let encoded = adpcm::encode_mono(&buf);
    debug_assert_eq!(encoded.len(), SAMPLE_SEG_LEN);
    SynthResult {
        encoded,
        mixed,
        clipped,
        dropped,
    }
}

/// Convert a content shift in milliseconds to a **whole-block** byte offset
/// into the encoded sample segment, rounding to the nearest block.
///
/// This is the replacement for `SoundBank::Play`'s `timeOffset`, which was
/// live-refuted as a seek (2026-07-29): the engine's only use of it is
/// fast-forwarding the cue's *event* timeline, and an already-due wave
/// starts at sample 0 (`Wave_StartNow_NoSampleOffset` — there is no
/// sample-offset start primitive in this engine). Instead the track content
/// itself is shifted: MS-ADPCM blocks are fully self-contained, so dropping
/// the first k blocks of the encoded track shifts every clap earlier by
/// exactly k × 128/44.1 ms (2.90 ms granularity, ≤ 1.45 ms rounding error —
/// one constant per song, inside the start-instant budget).
///
/// Clamped to the segment: shifting more than the whole track yields the
/// full segment length (an all-silence rewrite — every clap is in the past).
pub fn shift_bytes_for_ms(shift_ms: i32) -> usize {
    if shift_ms <= 0 {
        return 0;
    }
    // blocks = round(ms × rate / 1000 / samples_per_block); i64 headroom.
    let samples = shift_ms as i64 * TICK_RATE_HZ as i64;
    let denom = 1000 * adpcm::SAMPLES_PER_BLOCK as i64;
    let blocks = ((samples + denom / 2) / denom) as usize;
    blocks.min(CAPACITY_BLOCKS) * adpcm::BLOCK_ALIGN
}
