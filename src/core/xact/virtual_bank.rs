//! Pure virtual-bank planning for the streaming rate engine: per-entry rate
//! targeting, half-up loop-boundary mapping with the one-frame clamp rule,
//! whole-bank layout planning with canonical pre-data synthesis, and virtual
//! file-offset region resolution.
//!
//! The per-entry logic is relocated verbatim-in-behavior from the retired
//! whole-song transformer (`transform.rs`, deleted 2026-08-09): the plan
//! produced here is the spec the incremental generator must reproduce.
//! [`plan_virtual_bank`] composes it into the virtual file the read detour
//! serves (plan Step 4): a stock-shaped pre-data block emitted by the same
//! canonical `xwb` code path the streaming serializer runs, entry data at the
//! serializer's physical offsets, zero-filled alignment gaps, and the exact
//! stock EOF clamp against the virtual size.

use std::fmt;

use super::rate::{self, RateError, RateRatio};
use super::stretch::LoopContext;
use super::xwb::{self, StreamedEntry};
use super::WaveFormat;

/// One entry's stretched-layout plan: the streamed header values the virtual
/// bank advertises, the exact reduced rate, and the loop context the
/// stretcher consumes.
#[derive(Clone, Copy, Debug)]
pub struct EntryPlan {
    pub streamed: StreamedEntry,
    pub rate: RateRatio,
    pub source_frames: u64,
    pub loop_context: Option<LoopContext>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlanError {
    Rate(RateError),
    /// A rate refusal (28-bit ceiling, unsupported percent) surfaced by
    /// [`plan_virtual_bank`], carrying the offending entry's identity.
    EntryRate {
        index: usize,
        source: RateError,
    },
    /// The mapped loop region is degenerate (empty/inverted) or its source
    /// range overflows.
    InvalidMappedLoop {
        index: usize,
    },
    /// The canonical pre-data emission refused the planned layout (carries
    /// the `XwbError` display; structurally unreachable after a successful
    /// per-entry plan — every layout rule is already enforced by the plan).
    PreData(String),
    ArithmeticOverflow,
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rate(error) => write!(f, "rate error: {error}"),
            Self::EntryRate { index, source } => {
                write!(f, "entry {index} rate error: {source}")
            }
            Self::InvalidMappedLoop { index } => {
                write!(f, "entry {index} mapped loop is invalid")
            }
            Self::PreData(detail) => write!(f, "pre-data synthesis failed: {detail}"),
            Self::ArithmeticOverflow => write!(f, "plan arithmetic overflow"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<RateError> for PlanError {
    fn from(error: RateError) -> Self {
        Self::Rate(error)
    }
}

/// Which entry receives the stretched (rate-planned, ring-served) plan;
/// the other entry passes through verbatim from the resident source.
///
/// [`StretchTarget::Main`] is the gameplay model (shipped 2026-08-10):
/// the played `<code>` entry stretches and the never-played `<code>_s`
/// preview passes through. [`StretchTarget::Side`] is the exact inverse —
/// the song-select preview-rate feature's plan (design 2026-08-15): at
/// song select the game plays ONLY the `_s` entry, so it stretches and
/// the main entry passes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchTarget {
    /// Stretch the entry named exactly like the bank (gameplay).
    Main,
    /// Stretch the non-main (`_s` preview) entry (song-select preview).
    Side,
}

/// The planned virtual file the read detour serves for a bound bank:
/// pre-data bytes at `[0, pre_data.len())`, each entry's generated stream at
/// its virtual offset, zero fill between them, EOF at `virtual_size`.
#[derive(Debug)]
pub struct VirtualBankLayout {
    /// Every entry's plan, in the source bank's physical (wave-index) order —
    /// 2 for every song bank but `goru` (4: `goru_cs`/`goru`/`goru_ac`/`goru_s`).
    /// Exactly ONE entry (`target_entry_index`) is rate-planned; all others
    /// pass through verbatim.
    pub entries: Vec<EntryPlan>,
    /// Index of the entry named exactly like the bank (the `<code>` wave).
    pub main_entry_index: usize,
    /// Index of the `<code>_s` preview wave.
    pub preview_entry_index: usize,
    /// Index of the STRETCHED (rate-planned) entry — the one the ring and
    /// producer serve. Equals `main_entry_index` for [`StretchTarget::Main`]
    /// (and for the identity plan, where the distinction is inert);
    /// `preview_entry_index` for [`StretchTarget::Side`].
    pub target_entry_index: usize,
    /// Virtual file offset of each entry's data (entry 0 at 2048; each later
    /// entry at the next 2048-aligned offset after the previous entry's data
    /// — the stock packer's rule, so physical order == index order).
    pub entry_offsets: Vec<u64>,
    /// Synthesized stock-shaped pre-data block — exactly the first
    /// `wave-data offset` (2048) bytes of the virtual file, emitted by the
    /// canonical `xwb` streaming layout.
    pub pre_data: Vec<u8>,
    /// Total virtual file size; segment 4 ends exactly here.
    pub virtual_size: u64,
}

/// The serving region containing a resolved virtual file offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// Bytes from the synthesized pre-data block, starting at `offset`.
    PreData { offset: usize },
    /// Bytes from entry `entry`'s generated stream, starting at `offset`
    /// within that stream.
    EntryData { entry: usize, offset: u64 },
    /// The inter-entry alignment gap: zero fill.
    Gap,
    /// At or past `virtual_size`: zero bytes are served.
    Eof,
}

/// One resolved contiguous span: the region containing the requested offset
/// and the byte count servable from it without leaving the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedSpan {
    pub region: Region,
    pub len: u32,
}

impl VirtualBankLayout {
    /// Map a virtual file offset to its serving region, clamping the request
    /// to `min(len, virtual_size − offset)` — the exact stock EOF contract —
    /// and then to the region's remaining contiguous bytes. Requests at or
    /// past `virtual_size` resolve to [`Region::Eof`] with zero bytes; a
    /// read spanning regions (the engine's 0x1000 header read covers the
    /// pre-data block AND the start of entry-0 data) is served by repeated
    /// calls, each advancing past the span it received.
    #[must_use]
    pub fn resolve(&self, offset: u64, len: u32) -> ResolvedSpan {
        if offset >= self.virtual_size {
            return ResolvedSpan {
                region: Region::Eof,
                len: 0,
            };
        }
        let clamped = u64::from(len).min(self.virtual_size - offset);
        let pre_data_end = self.pre_data.len() as u64;
        let (region, region_end) = if offset < pre_data_end {
            (
                Region::PreData {
                    offset: offset as usize,
                },
                pre_data_end,
            )
        } else {
            // Entries are laid out in index order (see `entry_offsets`);
            // walk them: inside an entry's data → EntryData, before the next
            // entry's start → Gap. Past the last entry's data there is no
            // gap (segment 4 ends exactly at the last byte).
            let mut resolved = None;
            for (entry, (&start, plan)) in self.entry_offsets.iter().zip(&self.entries).enumerate()
            {
                let end = start + plan.streamed.data_len as u64;
                if offset < start {
                    resolved = Some((Region::Gap, start));
                    break;
                }
                if offset < end {
                    resolved = Some((
                        Region::EntryData {
                            entry,
                            offset: offset - start,
                        },
                        end,
                    ));
                    break;
                }
            }
            // Unreachable for a consistent layout (virtual_size == last
            // entry end); serve zeros to EOF rather than panic in detour
            // context.
            resolved.unwrap_or((Region::Gap, self.virtual_size))
        };
        ResolvedSpan {
            region,
            // The span never exceeds the u32 request, so the cast is exact.
            len: clamped.min(region_end - offset) as u32,
        }
    }
}

/// Plan the whole virtual bank for `source` at `percent`: the TARGET entry
/// through [`plan_entry`] (rate refusals gain their entry identity as
/// [`PlanError::EntryRate`]; loop refusals already carry it), the other
/// entry as a verbatim PASSTHROUGH (stock duration/loops/data_len, served
/// unstretched from the resident source). [`StretchTarget::Main`] is the
/// gameplay plan: the never-played preview passes through, keeping bank
/// prepare — and therefore the loading screen — off the DSP's critical
/// path (maintainer-approved 2026-08-10, step05-fix).
/// [`StretchTarget::Side`] is the song-select preview plan: the `_s` entry
/// the game actually plays stretches, and the never-played main entry
/// passes through (song-preview-rate design, 2026-08-15). Physical entry
/// order preserved; pre-data synthesized through `xwb`'s canonical
/// streaming emission — the layout the engine already parses in the proven
/// pipeline.
pub fn plan_virtual_bank(
    source: &xwb::SongBank<'_>,
    percent: u32,
    target: StretchTarget,
) -> Result<VirtualBankLayout, PlanError> {
    // `parse_song_bank` resolves which entry is the `<code>` main wave —
    // from the entry-name table when the bank has one, from the durations
    // for the nameless World-era banks (`acef`, `neut`, `dais`, `rhyz`, …).
    let main_entry_index = source.main_entry_index();
    debug_assert_eq!(source.entries[main_entry_index].name(), source.name());
    let preview_entry_index = source.preview_entry_index();
    let target_entry_index = match target {
        StretchTarget::Main => main_entry_index,
        StretchTarget::Side => preview_entry_index,
    };
    let mut entries = Vec::with_capacity(source.entries.len());
    for (index, entry) in source.entries.iter().enumerate() {
        entries.push(if index == target_entry_index {
            plan_entry(index, entry, percent).map_err(|error| match error {
                PlanError::Rate(rate_error) => PlanError::EntryRate {
                    index,
                    source: rate_error,
                },
                other => other,
            })?
        } else {
            passthrough_plan(entry)
        });
    }
    finish_layout(source, entries, target_entry_index)
}

/// Emit the pre-data for a set of entry plans and assemble the layout.
fn finish_layout(
    source: &xwb::SongBank<'_>,
    entries: Vec<EntryPlan>,
    target_entry_index: usize,
) -> Result<VirtualBankLayout, PlanError> {
    let streamed: Vec<StreamedEntry> = entries.iter().map(|plan| plan.streamed).collect();
    let pre_data = xwb::stream_pre_data(source, &streamed)
        .map_err(|error| PlanError::PreData(error.to_string()))?;
    let entry_offsets = pre_data
        .data_offsets
        .iter()
        .map(|&offset| (pre_data.wave_data_offset + offset) as u64)
        .collect();
    Ok(VirtualBankLayout {
        entries,
        main_entry_index: source.main_entry_index(),
        preview_entry_index: source.preview_entry_index(),
        target_entry_index,
        entry_offsets,
        virtual_size: pre_data.total_length as u64,
        pre_data: pre_data.bytes,
    })
}

/// Plan the whole virtual bank at IDENTITY: BOTH entries pass through
/// verbatim — the training-mode identity arm (training design §4.5). The
/// MAIN entry MUST go through [`passthrough_plan`], never
/// `plan_entry(…, 100)`: the rate path block-quantizes the output
/// (`rate::target_for_percent`), so a stock entry whose real duration sits
/// inside its final block would advertise a slightly different duration
/// and a ≈1-but-not-1 rate (`docs/training_mode_research.md` §5.3). The
/// resulting layout reproduces the stock file exactly; content shifting
/// happens at serve time in the binding, never in the plan (the engine
/// parses this header once per bank — research §5.1).
pub fn plan_identity_bank(source: &xwb::SongBank<'_>) -> Result<VirtualBankLayout, PlanError> {
    let main_entry_index = source.main_entry_index();
    debug_assert_eq!(source.entries[main_entry_index].name(), source.name());
    let entries = source.entries.iter().map(passthrough_plan).collect();
    // Every entry is verbatim; the target distinction is inert. The field is
    // populated coherently (== main) for downstream consumers.
    finish_layout(source, entries, main_entry_index)
}

/// The non-main entry's verbatim plan: the virtual header advertises the
/// STOCK values and the serving layer copies the stock bytes straight from
/// the resident source — no stretch, no producer involvement, and (if any
/// path ever cues it) real audio at normal speed, which is exactly what a
/// preview should be. [`plan_identity_bank`] applies the same plan to the
/// MAIN entry for the training-mode identity arm.
fn passthrough_plan(entry: &xwb::SongEntry<'_>) -> EntryPlan {
    EntryPlan {
        streamed: StreamedEntry {
            data_len: entry.data.len(),
            duration: entry.duration,
            loop_start: entry.loop_start,
            loop_length: entry.loop_length,
        },
        rate: RateRatio::IDENTITY,
        source_frames: u64::from(entry.duration),
        loop_context: None,
    }
}

/// Plan one parsed song-bank entry at `percent`.
pub fn plan_entry(
    index: usize,
    entry: &xwb::SongEntry<'_>,
    percent: u32,
) -> Result<EntryPlan, PlanError> {
    plan_entry_values(
        index,
        entry.format,
        entry.duration,
        entry.loop_start,
        entry.loop_length,
        percent,
    )
}

/// Plan an entry from its raw metadata: exact whole-block rate targeting
/// (28-bit XWB duration ceiling enforced by `rate::target_for_percent`) plus
/// loop mapping. This is the value-level surface the host tests drive.
pub fn plan_entry_values(
    index: usize,
    format: WaveFormat,
    duration: u32,
    loop_start: u32,
    loop_length: u32,
    percent: u32,
) -> Result<EntryPlan, PlanError> {
    let target =
        rate::target_for_percent(u64::from(duration), format.samples_per_block(), percent)?;
    let output_frames =
        u32::try_from(target.output_frames).map_err(|_| PlanError::ArithmeticOverflow)?;
    let data_len = usize::try_from(target.output_blocks)
        .ok()
        .and_then(|blocks| blocks.checked_mul(format.block_align() as usize))
        .ok_or(PlanError::ArithmeticOverflow)?;
    let (mapped_start, mapped_length, loop_context) =
        map_loop(index, duration, loop_start, loop_length, output_frames)?;
    Ok(EntryPlan {
        streamed: StreamedEntry {
            data_len,
            duration: output_frames,
            loop_start: mapped_start,
            loop_length: mapped_length,
        },
        rate: target.rate,
        source_frames: u64::from(duration),
        loop_context,
    })
}

/// Map a source loop region onto the target frame count with the exact
/// half-up boundary rules (load-bearing: the generator and the header
/// synthesis must agree on these values to the frame).
pub fn map_loop(
    index: usize,
    source_frames: u32,
    loop_start: u32,
    loop_length: u32,
    output_frames: u32,
) -> Result<(u32, u32, Option<LoopContext>), PlanError> {
    if loop_length == 0 {
        return Ok((0, 0, None));
    }
    let source_end = loop_start
        .checked_add(loop_length)
        .ok_or(PlanError::InvalidMappedLoop { index })?;
    let mapped_start = map_boundary(loop_start, source_frames, output_frames)?;
    let mapped_end = map_boundary(source_end, source_frames, output_frames)?;
    if mapped_end <= mapped_start {
        return Err(PlanError::InvalidMappedLoop { index });
    }
    let mapped_length = mapped_end - mapped_start;
    Ok((
        mapped_start,
        mapped_length,
        Some(LoopContext {
            source_start: loop_start as usize,
            source_end: source_end as usize,
            output_start: mapped_start as usize,
            output_end: mapped_end as usize,
        }),
    ))
}

/// Half-up boundary mapping with the one-frame clamp rule: a boundary that
/// rounds at most one frame past the output length clamps to it (rounding
/// tolerance); anything further refuses (malformed loop metadata).
fn map_boundary(boundary: u32, source_frames: u32, output_frames: u32) -> Result<u32, PlanError> {
    let numerator = u128::from(boundary)
        .checked_mul(u128::from(output_frames))
        .ok_or(PlanError::ArithmeticOverflow)?;
    let mapped = rate::round_half_up_u128(numerator, u128::from(source_frames))?;
    let maximum = u128::from(output_frames);
    let clamped = mapped.min(maximum);
    if mapped - clamped > 1 {
        return Err(PlanError::ArithmeticOverflow);
    }
    u32::try_from(clamped).map_err(|_| PlanError::ArithmeticOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The transform-era synthetic profile: stereo, 8 kHz, block align raw 48
    /// (140-byte blocks, 128 samples per block).
    fn stereo_format() -> WaveFormat {
        WaveFormat::from_packed(2 | (2 << 2) | (8_000 << 5) | (48 << 23))
    }

    #[test]
    fn plan_targets_match_the_retired_transformer_vectors() {
        let format = stereo_format();
        assert_eq!(format.samples_per_block(), 128);
        assert_eq!(format.block_align(), 140);

        // 1024 frames at 75%: 8 blocks -> round_half_up(102400/9600) = 11
        // blocks = 1408 frames (the retired transformer's loop-test shape).
        let plan = plan_entry_values(0, format, 1_024, 0, 0, 75).unwrap();
        assert_eq!(plan.streamed.duration, 1_408);
        assert_eq!(plan.streamed.data_len, 11 * 140);
        assert_eq!(plan.source_frames, 1_024);
        assert_eq!(plan.rate, RateRatio::new(1_024, 1_408).unwrap());
        assert_eq!(plan.loop_context, None);
        assert_eq!(
            (plan.streamed.loop_start, plan.streamed.loop_length),
            (0, 0)
        );

        // 1024 frames at 125%: round_half_up(102400/16000) = 6 blocks = 768.
        let plan = plan_entry_values(0, format, 1_024, 0, 0, 125).unwrap();
        assert_eq!(plan.streamed.duration, 768);
        assert_eq!(plan.streamed.data_len, 6 * 140);
        assert_eq!(plan.rate, RateRatio::new(1_024, 768).unwrap());
    }

    #[test]
    fn interior_loop_maps_half_up_exactly_like_the_retired_transformer() {
        // The deleted transformer test's exact assertion: duration 1024,
        // loop 128+768 at 75% -> loop_start 176, loop_length 1056.
        let plan = plan_entry_values(1, stereo_format(), 1_024, 128, 768, 75).unwrap();
        assert_eq!(plan.streamed.duration, 1_408);
        assert_eq!(plan.streamed.loop_start, 176);
        assert_eq!(plan.streamed.loop_length, 1_056);
        assert_eq!(
            plan.loop_context,
            Some(LoopContext {
                source_start: 128,
                source_end: 896,
                output_start: 176,
                output_end: 1_232,
            })
        );
    }

    #[test]
    fn zero_length_loops_plan_as_unlooped() {
        assert_eq!(map_loop(0, 1_024, 500, 0, 1_408).unwrap(), (0, 0, None));
    }

    #[test]
    fn degenerate_mapped_loops_refuse() {
        // A one-frame source loop that maps to an empty output region:
        // 1 frame of 100_000 at 25% of a tiny output collapses start == end.
        let result = map_loop(1, 100_000, 50_000, 1, 128);
        assert_eq!(result, Err(PlanError::InvalidMappedLoop { index: 1 }));

        // Source range overflow refuses with the same identity.
        let result = map_loop(0, 1_024, u32::MAX, 2, 1_408);
        assert_eq!(result, Err(PlanError::InvalidMappedLoop { index: 0 }));
    }

    #[test]
    fn boundary_clamp_tolerates_one_frame_and_refuses_beyond() {
        // Boundary half a frame past the source end rounds one past the
        // output and clamps onto it (the one-frame tolerance).
        let (start, length, _) = map_loop(0, 1_000, 0, 1_050, 10).unwrap();
        assert_eq!((start, length), (0, 10));

        // A boundary far past the source maps well beyond output+1: refused.
        let result = map_loop(0, 1_024, 0, 2_048, 1_408);
        assert_eq!(result, Err(PlanError::ArithmeticOverflow));
    }

    #[test]
    fn oversized_targets_refuse_via_the_28_bit_ceiling() {
        // MAX_XWB_DURATION source frames at 25% targets ~4x the ceiling.
        let result = plan_entry_values(
            0,
            stereo_format(),
            u32::try_from(rate::MAX_XWB_DURATION).unwrap(),
            0,
            0,
            25,
        );
        assert!(matches!(
            result,
            Err(PlanError::Rate(RateError::DurationOutOfRange { .. }))
        ));
    }
}
