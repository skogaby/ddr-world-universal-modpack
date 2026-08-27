//! Pure seek math + judge-record transforms (Training Mode v1, Step 2).
//!
//! The host-testable half of seek-to-T (design §4.4): block quantization of
//! the seek target, the back-dated clock-anchor value, the
//! rebuild-at-playhead-T expectation model, and the spanning-freeze
//! neutralization planner (R14) — pure functions over the 0x60-stride note
//! and 0x40-stride judge-record layouts (`run_state_re.md` §3/§4, training
//! research §3). No logging, no game reads: the engine-facing caller
//! (`super::request_reset`'s nonzero-T arm, task-03) supplies the bytes and
//! applies the planned writes.
//!
//! Layout constants live HERE, once — the engine caller addresses live
//! records through them (no duplicated magic numbers, per task req 4).

use crate::core::xact::rate::round_half_up_u128;
use crate::services::song_rate::clock_patch::RateSnapshot;

// ── Note layout (0x60 stride, chart vector at actor+0x90) ────────────
/// One chart note record.
pub const NOTE_STRIDE: usize = 0x60;
/// Kind byte (i8): 0 = tap/jump/shock/freeze-head, 1 = armed marker,
/// 2 = freeze-END marker; negatives are control notes (no judge record).
pub const NOTE_KIND_OFFSET: usize = 0x00;
/// Display/chart-offset-domain time (i32, beat-proportional).
pub const NOTE_DISPLAY_TIME_OFFSET: usize = 0x04;
/// Raw music-count milliseconds (i32) — the judge domain and the seek
/// playhead's domain.
pub const NOTE_RAW_TIME_OFFSET: usize = 0x08;
/// i32[8] per-panel participation flags (1 participates; 4 also seen).
pub const NOTE_PANEL_FLAGS_OFFSET: usize = 0x1C;
/// i32[8] per-panel freeze durations.
pub const NOTE_PANEL_DURATIONS_OFFSET: usize = 0x3C;
/// Panels per note (doubles-wide, both layouts).
pub const PANEL_COUNT: usize = 8;

// ── Judge record layout (0x40 stride, vector at actor+0xB0) ──────────
/// One judge record.
pub const RECORD_STRIDE: usize = 0x40;
/// judgedAt music count (i32, −1 = pending).
pub const RECORD_JUDGED_AT_OFFSET: usize = 0x08;
/// Grade (i32; see the `GRADE_*` values).
pub const RECORD_GRADE_OFFSET: usize = 0x0C;
/// i32[8] per-panel freeze hold progress.
pub const RECORD_HOLD_PROGRESS_OFFSET: usize = 0x14;
/// u8[8] per-panel freeze wobble counters (0xFF = not a freeze participant).
pub const RECORD_WOBBLE_OFFSET: usize = 0x34;

// ── Grade values (rebuild semantics, training research §3.2) ─────────
/// Unjudged/pending sentinel.
pub const GRADE_PENDING: i32 = 0xFF;
/// Consumed pre-playhead tap/jump/freeze-head.
pub const GRADE_CONSUMED: i32 = 0;
/// Armed marker (kind 1, non-shock shape).
pub const GRADE_ARMED: i32 = 5;
/// Passed/OK — the engine's freeze-completion (and pre-playhead shock)
/// value.
pub const GRADE_OK: i32 = 6;
/// Armed shock shape / freeze-END marker.
pub const GRADE_ARMED_SHOCK: i32 = 7;

// ── Block quantization (design §4.4 step 2, research §6) ─────────────

/// A seek target quantized to the source ADPCM block grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeekQuantization {
    /// The content shift in whole source blocks — B(T).
    pub blocks: u64,
    /// The grid point's raw-ms value (half-up) — the clock and rebuild
    /// playhead T_q. Never exceeds the requested target.
    pub t_q_ms: i32,
}

/// Quantize a raw-ms seek target to the source block grid: FLOOR in the
/// frame domain (the block containing T), clamped to `[0, max_blocks]`
/// (the caller's source-length clamp input). Quantizing FIRST keeps chart
/// clock, claps, and audio mutually exact — all three consume the same
/// B(T) grid. `None` on degenerate grid parameters.
#[must_use]
pub fn quantize_seek(
    t_ms: i32,
    samples_per_block: u32,
    sample_rate: u32,
    max_blocks: u64,
) -> Option<SeekQuantization> {
    if samples_per_block == 0 || sample_rate == 0 {
        return None;
    }
    let target_ms = u128::from(i64::from(t_ms).max(0) as u64);
    let block_ms_numerator = u128::from(samples_per_block) * 1_000;
    let blocks = (target_ms * u128::from(sample_rate)) / block_ms_numerator;
    let blocks = blocks.min(u128::from(max_blocks));
    let t_q = round_half_up_u128(blocks * block_ms_numerator, u128::from(sample_rate)).ok()?;
    Some(SeekQuantization {
        blocks: u64::try_from(blocks).ok()?,
        t_q_ms: i32::try_from(t_q).ok()?,
    })
}

/// The wall-domain millisecond position of a served-stream block boundary
/// — the exact inverse of [`quantize_seek`]'s `t_q_ms` for its own
/// `blocks` output (same round-half-up expression). The Step-3 silent
/// start derives its adjust target from the LIVE binding's applied
/// mapping (`shift_blocks`), so the clock anchors at precisely the grid
/// point the audio serves — the desired-vs-committed rate epsilon of the
/// arm-time conversion never reaches the anchor. `None` on degenerate
/// grid parameters or overflow.
#[must_use]
pub fn blocks_to_wall_ms(blocks: u64, samples_per_block: u32, sample_rate: u32) -> Option<i32> {
    if samples_per_block == 0 || sample_rate == 0 {
        return None;
    }
    let block_ms_numerator = u128::from(samples_per_block) * 1_000;
    let ms = round_half_up_u128(
        u128::from(blocks) * block_ms_numerator,
        u128::from(sample_rate),
    )
    .ok()?;
    i32::try_from(ms).ok()
}

/// The back-dated timing-anchor tick for a seek to `t_q_ms` (research §6):
/// `now + delay − wall(T_q)`. `delay_ms` is the shipped future-dating term
/// (the countdown lead — 0 for an instant seek). Identity and uncommitted
/// snapshots take the literal legacy arithmetic (the `tick_domain`
/// selector, bit-identically); a committed rate converts through the exact
/// ratio. Conversion failure falls back to identity — never a panic on the
/// seek path.
#[must_use]
pub fn anchor_tick(now_tick: u64, delay_ms: u64, t_q_ms: i32, snapshot: &RateSnapshot) -> u64 {
    let content = i64::from(t_q_ms).max(0);
    let wall = if snapshot.is_non_identity_commit() {
        snapshot
            .effective_rate
            .content_to_wall_ms(content)
            .unwrap_or(content)
            .max(0)
    } else {
        content
    };
    now_tick.wrapping_add(delay_ms).wrapping_sub(wall as u64)
}

/// Content→wall milliseconds under the snapshot (research §6). Identity
/// and uncommitted snapshots return the literal input (the `tick_domain`
/// selector); a committed rate converts through the exact ratio, with an
/// identity fallback on conversion failure (never a panic).
#[must_use]
pub fn wall_ms(content_ms_value: i32, snapshot: &RateSnapshot) -> i32 {
    if !snapshot.is_non_identity_commit() {
        return content_ms_value;
    }
    snapshot
        .effective_rate
        .content_to_wall_ms(i64::from(content_ms_value))
        .ok()
        .and_then(|wall| i32::try_from(wall).ok())
        .unwrap_or(content_ms_value)
}

/// Wall→content milliseconds — the inverse of [`wall_ms`], via the exact
/// ratio with its terms swapped (`RateRatio` fields are a reduced
/// fraction; the inverse is the same fraction upside down). Same identity
/// pin and fallback discipline. Round-tripping through the integer ms
/// domains carries ≤ 1 ms of half-up slop — the anchor math's documented
/// tolerance.
#[must_use]
pub fn content_ms(wall_ms_value: i32, snapshot: &RateSnapshot) -> i32 {
    if !snapshot.is_non_identity_commit() {
        return wall_ms_value;
    }
    let inverse = crate::core::xact::rate::RateRatio {
        source_frames: snapshot.effective_rate.output_frames,
        output_frames: snapshot.effective_rate.source_frames,
    };
    inverse
        .content_to_wall_ms(i64::from(wall_ms_value))
        .ok()
        .and_then(|content| i32::try_from(content).ok())
        .unwrap_or(wall_ms_value)
}

// ── Note decode ──────────────────────────────────────────────────────

/// One decoded chart note (the 0x60-stride layout's judged fields).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteView {
    pub kind: i8,
    pub display_time: i32,
    pub raw_time: i32,
    pub panel_flags: [i32; PANEL_COUNT],
    pub durations: [i32; PANEL_COUNT],
}

impl NoteView {
    /// Whether this note participates in freeze processing (any per-panel
    /// duration > 0 — the rebuild's wobble/hold sentinel rule).
    #[must_use]
    pub fn freeze_participant(&self) -> bool {
        self.durations.iter().any(|&duration| duration > 0)
    }

    /// The engine's shock shape: all four panels of either side
    /// participating with flag value 1.
    #[must_use]
    pub fn shock_shaped(&self) -> bool {
        self.panel_flags[..4].iter().all(|&flag| flag == 1)
            || self.panel_flags[4..].iter().all(|&flag| flag == 1)
    }

    /// Panel-participation mask (nonzero-ness per panel) — the freeze
    /// head↔end pairing key (flag VALUES vary: 1 and 4 both participate).
    fn participation_mask(&self) -> u8 {
        self.panel_flags
            .iter()
            .enumerate()
            .fold(
                0u8,
                |mask, (panel, &flag)| {
                    if flag != 0 {
                        mask | (1 << panel)
                    } else {
                        mask
                    }
                },
            )
    }
}

/// Decode a 0x60-stride note vector from raw bytes. `None` on a partial
/// stride (layout drift — the caller refuses the seek).
#[must_use]
pub fn decode_notes(bytes: &[u8]) -> Option<Vec<NoteView>> {
    if bytes.len() % NOTE_STRIDE != 0 {
        return None;
    }
    let read_i32 = |chunk: &[u8], offset: usize| {
        i32::from_le_bytes([
            chunk[offset],
            chunk[offset + 1],
            chunk[offset + 2],
            chunk[offset + 3],
        ])
    };
    Some(
        bytes
            .chunks_exact(NOTE_STRIDE)
            .map(|chunk| {
                let mut panel_flags = [0i32; PANEL_COUNT];
                let mut durations = [0i32; PANEL_COUNT];
                for panel in 0..PANEL_COUNT {
                    panel_flags[panel] = read_i32(chunk, NOTE_PANEL_FLAGS_OFFSET + panel * 4);
                    durations[panel] = read_i32(chunk, NOTE_PANEL_DURATIONS_OFFSET + panel * 4);
                }
                NoteView {
                    kind: chunk[NOTE_KIND_OFFSET] as i8,
                    display_time: read_i32(chunk, NOTE_DISPLAY_TIME_OFFSET),
                    raw_time: read_i32(chunk, NOTE_RAW_TIME_OFFSET),
                    panel_flags,
                    durations,
                }
            })
            .collect(),
    )
}

// ── Display⇄raw end-domain converters (Step 4, design §4.2) ──────────
//
// The chart's display (+0x04) and raw (+0x08) note times are two views
// of one monotone chart-wide mapping (both driven by the beat
// structure); every note — control notes included — carries a point on
// it. The game's own converter brackets a query between consecutive
// notes and linearly interpolates (research §4.4's `FUN_1801c8d50`
// equivalent); these replicate it for the CMA end thresholds: `+0x94`
// is display-domain, so a raw-ms section end needs `display_for_raw`,
// and the loop driver's fire bound needs the stock `+0x94` back in raw
// ms via `raw_for_display`.

/// Linearly map `x` through the piecewise mapping the note vector's
/// `(key, value)` pairs define: bracket between consecutive
/// distinct-key notes and interpolate; outside the covered range,
/// EXTRAPOLATE from the nearest distinct-key pair (monotonicity is
/// preserved — clamping would fabricate equality with the boundary
/// value, which the threshold clamp math must never see). Assumes
/// chart-ordered (non-decreasing) keys. `None` when fewer than two
/// distinct keys exist (no slope). i64 arithmetic, round half away
/// from zero — round-trip slop ≤ 1 unit per direction.
fn interpolate_notes(
    notes: &[NoteView],
    x: i32,
    key: fn(&NoteView) -> i32,
    value: fn(&NoteView) -> i32,
) -> Option<i32> {
    let first = notes.first()?;
    let last = notes.last()?;
    // The bracketing pair: (lo, hi) with key(lo) < key(hi) and
    // key(lo) <= x <= key(hi) where possible; otherwise the nearest
    // distinct-key pair for extrapolation.
    let (lo, hi) = if x <= key(first) {
        // Before (or at) the first note: the first distinct-key pair.
        let hi = notes.iter().find(|note| key(note) > key(first))?;
        (first, hi)
    } else if x >= key(last) {
        // Past (or at) the last note: the last distinct-key pair.
        let lo = notes.iter().rev().find(|note| key(note) < key(last))?;
        (lo, last)
    } else {
        // In range: an exact-key hit returns its value; otherwise the
        // enclosing distinct-key bracket.
        let mut lo = first;
        let mut hi = None;
        for note in notes {
            if key(note) == x {
                return Some(value(note));
            }
            if key(note) < x {
                lo = note;
            } else {
                hi = Some(note);
                break;
            }
        }
        (lo, hi?)
    };
    let (x0, y0) = (i64::from(key(lo)), i64::from(value(lo)));
    let (x1, y1) = (i64::from(key(hi)), i64::from(value(hi)));
    debug_assert!(x1 > x0, "bracket keys must be distinct");
    let numerator = (i64::from(x) - x0) * (y1 - y0);
    let denominator = x1 - x0;
    let half = denominator / 2;
    let rounded = if numerator >= 0 {
        (numerator + half) / denominator
    } else {
        (numerator - half) / denominator
    };
    i32::try_from(y0 + rounded).ok()
}

/// Raw-ms → display-domain time through the chart's note mapping (the
/// game's converter, design §4.2): the CMA `+0x94` value for a raw-ms
/// section end. `None` on degenerate vectors (empty/single-note/zero
/// slope) — the caller's WARN-once natural-end ladder.
#[must_use]
pub fn display_for_raw(notes: &[NoteView], raw_ms: i32) -> Option<i32> {
    interpolate_notes(
        notes,
        raw_ms,
        |note| note.raw_time,
        |note| note.display_time,
    )
}

/// Display-domain → raw-ms — the exact inverse of [`display_for_raw`]
/// (same bracketing over `display_time`): the stock CMA `+0x94`
/// threshold expressed in raw ms, the loop driver's fire-bound term.
/// Round-trips within ±1 unit on monotone vectors.
#[must_use]
pub fn raw_for_display(notes: &[NoteView], display_ms: i32) -> Option<i32> {
    interpolate_notes(
        notes,
        display_ms,
        |note| note.display_time,
        |note| note.raw_time,
    )
}

// ── Rebuild-at-T expectation model (research §3.2) ───────────────────

/// The expected post-rebuild state of one judge record — the oracle the
/// engine's rebuild is verified against, and the neutralization walker's
/// input model. Records are index-aligned with the non-control notes
/// (kind ≥ 0), in note order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordExpectation {
    /// Index into the FULL note vector (control notes included).
    pub note_index: usize,
    pub judged_at: i32,
    pub grade: i32,
    pub hold_progress: [i32; PANEL_COUNT],
    /// Whether the wobble/hold sentinel pair is zeroed (freeze processing
    /// participant).
    pub freeze_participant: bool,
}

/// Model `FUN_180060d40` at playhead T: per-kind consumed/pending/armed
/// state, including the kind-2 pre-T back-patch of the matching head's
/// hold progress.
#[must_use]
pub fn rebuild_expectations(notes: &[NoteView], playhead_ms: i32) -> Vec<RecordExpectation> {
    let mut records: Vec<RecordExpectation> = Vec::new();
    for (note_index, note) in notes.iter().enumerate() {
        if note.kind < 0 {
            continue; // control notes emit no record
        }
        let mut record = RecordExpectation {
            note_index,
            judged_at: -1,
            grade: GRADE_PENDING,
            hold_progress: [0; PANEL_COUNT],
            freeze_participant: note.freeze_participant(),
        };
        match note.kind {
            0 => {
                if note.raw_time < playhead_ms {
                    record.judged_at = note.raw_time;
                    record.grade = if note.shock_shaped() {
                        GRADE_OK
                    } else {
                        GRADE_CONSUMED
                    };
                }
            }
            1 => {
                // Armed markers are playhead-independent.
                record.judged_at = note.raw_time;
                record.grade = if note.shock_shaped() {
                    GRADE_ARMED_SHOCK
                } else {
                    GRADE_ARMED
                };
            }
            2 => {
                record.judged_at = note.raw_time;
                record.grade = GRADE_ARMED_SHOCK;
                if note.raw_time < playhead_ms {
                    // Freeze fully before T: back-patch the LATEST earlier
                    // freeze-participant record whose participation mask
                    // matches — pre-T freezes are marked fully held.
                    let mask = note.participation_mask();
                    if let Some(head) = records.iter_mut().rev().find(|candidate| {
                        candidate.freeze_participant
                            && notes[candidate.note_index].participation_mask() == mask
                    }) {
                        head.hold_progress = notes[head.note_index].durations;
                    }
                }
            }
            _ => {}
        }
        records.push(record);
    }
    records
}

// ── Spanning-freeze neutralization planner (R14, design §4.4) ────────

/// One planned i32 write into the rebuilt judge-record vector,
/// byte-addressed from the vector's base. The engine-facing caller
/// applies these verbatim after the rebuild-at-T.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordWrite {
    pub byte_offset: usize,
    pub value: i32,
}

/// Plan the neutralization of every freeze SPANNING the seek target
/// (`head +0x08 < T_q < end +0x08`, strict): copy the head note's full
/// per-panel durations into the head record's hold progress and mark the
/// kind-2 end record consumed (grade OK + judgedAt) — mirroring the
/// engine's own pre-T treatment. Non-spanning freezes emit nothing (the
/// rebuild already handles fully-before; fully-after stays pending).
#[must_use]
pub fn neutralization_writes(notes: &[NoteView], t_q_ms: i32) -> Vec<RecordWrite> {
    // Record index per note (None for control notes).
    let mut record_indices: Vec<Option<usize>> = Vec::with_capacity(notes.len());
    let mut next_record = 0usize;
    for note in notes {
        if note.kind < 0 {
            record_indices.push(None);
        } else {
            record_indices.push(Some(next_record));
            next_record += 1;
        }
    }

    let mut writes = Vec::new();
    for (head_index, head) in notes.iter().enumerate() {
        if head.kind != 0 || !head.freeze_participant() || head.raw_time >= t_q_ms {
            continue;
        }
        let mask = head.participation_mask();
        let Some((end_index, end)) = notes
            .iter()
            .enumerate()
            .skip(head_index + 1)
            .find(|(_, candidate)| candidate.kind == 2 && candidate.participation_mask() == mask)
        else {
            continue;
        };
        if end.raw_time <= t_q_ms {
            continue; // fully before T — the engine's own back-patch covers it
        }
        let head_record = record_indices[head_index].expect("kind-0 notes have records");
        let end_record = record_indices[end_index].expect("kind-2 notes have records");
        for panel in 0..PANEL_COUNT {
            writes.push(RecordWrite {
                byte_offset: head_record * RECORD_STRIDE + RECORD_HOLD_PROGRESS_OFFSET + panel * 4,
                value: head.durations[panel],
            });
        }
        writes.push(RecordWrite {
            byte_offset: end_record * RECORD_STRIDE + RECORD_GRADE_OFFSET,
            value: GRADE_OK,
        });
        writes.push(RecordWrite {
            byte_offset: end_record * RECORD_STRIDE + RECORD_JUDGED_AT_OFFSET,
            value: end.raw_time,
        });
    }
    writes
}
