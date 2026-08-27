//! Host tests for the pure seek layer: block quantization, the back-dated
//! clock anchor, the rebuild-at-T expectation model, and the
//! spanning-freeze neutralization planner (training design §4.4, R14).

use super::seek::{self, NoteView, RecordWrite, SeekQuantization};
use crate::core::xact::rate::RateRatio;
use crate::services::song_rate::clock_patch::RateSnapshot;

// ── Fixture helpers ──────────────────────────────────────────────────

/// Panel-participation array from a panel index list.
fn panels(indices: &[usize]) -> [i32; 8] {
    let mut flags = [0i32; 8];
    for &index in indices {
        flags[index] = 1;
    }
    flags
}

/// Per-panel duration array from `(panel, duration)` pairs.
fn durations(pairs: &[(usize, i32)]) -> [i32; 8] {
    let mut out = [0i32; 8];
    for &(panel, value) in pairs {
        out[panel] = value;
    }
    out
}

/// One synthetic 0x60-stride note.
fn note_bytes(kind: i8, display: i32, raw: i32, flags: [i32; 8], durs: [i32; 8]) -> Vec<u8> {
    let mut bytes = vec![0u8; seek::NOTE_STRIDE];
    bytes[0] = kind as u8;
    bytes[seek::NOTE_DISPLAY_TIME_OFFSET..seek::NOTE_DISPLAY_TIME_OFFSET + 4]
        .copy_from_slice(&display.to_le_bytes());
    bytes[seek::NOTE_RAW_TIME_OFFSET..seek::NOTE_RAW_TIME_OFFSET + 4]
        .copy_from_slice(&raw.to_le_bytes());
    for panel in 0..seek::PANEL_COUNT {
        let flag_at = seek::NOTE_PANEL_FLAGS_OFFSET + panel * 4;
        bytes[flag_at..flag_at + 4].copy_from_slice(&flags[panel].to_le_bytes());
        let duration_at = seek::NOTE_PANEL_DURATIONS_OFFSET + panel * 4;
        bytes[duration_at..duration_at + 4].copy_from_slice(&durs[panel].to_le_bytes());
    }
    bytes
}

fn decode(chunks: &[Vec<u8>]) -> Vec<NoteView> {
    let bytes: Vec<u8> = chunks.iter().flatten().copied().collect();
    seek::decode_notes(&bytes).expect("fixture notes decode")
}

fn snapshot(source: u64, output: u64, committed: bool, percent: i32) -> RateSnapshot {
    RateSnapshot {
        generation: 1,
        requested_percent: percent,
        participant_mask: 1,
        effective_rate: RateRatio::new(source, output).expect("ratio"),
        committed,
    }
}

// ── Block quantization (AC-1) ────────────────────────────────────────

#[test]
fn quantization_floors_to_the_block_grid() {
    // 44.1 kHz, 128 samples/block: one block ≈ 2.9025 ms.
    const SPB: u32 = 128;
    const RATE: u32 = 44_100;
    const MAX: u64 = 100_000;

    // 0 and negative clamp to the origin.
    for t in [0i32, -5] {
        let q = seek::quantize_seek(t, SPB, RATE, MAX).expect("quantize");
        assert_eq!((q.blocks, q.t_q_ms), (0, 0), "t = {t}");
    }

    // Floor property across the boundary region: the chosen block's frame
    // start is ≤ the target position, the next block's is beyond it.
    for t in [1i32, 2, 3, 5, 60_000, 59_999, 60_001] {
        let q = seek::quantize_seek(t, SPB, RATE, MAX).expect("quantize");
        let frames_at_t = u128::from(t as u32) * u128::from(RATE);
        let block_start = u128::from(q.blocks) * u128::from(SPB) * 1_000;
        let next_start = (u128::from(q.blocks) + 1) * u128::from(SPB) * 1_000;
        assert!(block_start <= frames_at_t, "t = {t}");
        assert!(frames_at_t < next_start, "t = {t}");
        // The grid point's ms value: half-up of blocks·spb·1000/rate.
        let expected = (u128::from(q.blocks) * u128::from(SPB) * 1_000 + u128::from(RATE) / 2)
            / u128::from(RATE);
        assert_eq!(u128::from(q.t_q_ms as u32), expected, "t = {t}");
        assert!(
            q.t_q_ms <= t,
            "the grid point never passes the target (t = {t})"
        );
    }

    // Past-source-end clamps to the caller's block bound.
    let clamped = seek::quantize_seek(1_000_000, SPB, RATE, 10).expect("quantize");
    assert_eq!(clamped.blocks, 10);

    // Invalid grid parameters refuse.
    assert!(seek::quantize_seek(1_000, 0, RATE, MAX).is_none());
    assert!(seek::quantize_seek(1_000, SPB, 0, MAX).is_none());
}

// ── Anchor value (AC-2) ──────────────────────────────────────────────

#[test]
fn anchor_is_now_minus_wall_of_t_q() {
    const NOW: u64 = 5_000_000;

    // Identity: bit-identical to the legacy arithmetic (now + delay − t).
    let identity = RateSnapshot::IDENTITY;
    assert_eq!(seek::anchor_tick(NOW, 0, 0, &identity), NOW);
    assert_eq!(seek::anchor_tick(NOW, 0, 30_000, &identity), NOW - 30_000);
    assert_eq!(
        seek::anchor_tick(NOW, 2_500, 30_000, &identity),
        NOW + 2_500 - 30_000,
        "the delay composes exactly like the shipped future-dating"
    );

    // Committed 50 %: wall(t) = 2·t (exact via content_to_wall_ms).
    let half = snapshot(1, 2, true, 50);
    let wall = half
        .effective_rate
        .content_to_wall_ms(30_000)
        .expect("conversion");
    assert_eq!(wall, 60_000);
    assert_eq!(seek::anchor_tick(NOW, 0, 30_000, &half), NOW - 60_000);
    assert_eq!(
        seek::anchor_tick(NOW, 2_500, 30_000, &half),
        NOW + 2_500 - 60_000
    );

    // An uncommitted non-identity snapshot takes the legacy path — the
    // tick_domain selector, verbatim.
    let uncommitted = snapshot(1, 2, false, 50);
    assert_eq!(
        seek::anchor_tick(NOW, 0, 30_000, &uncommitted),
        NOW - 30_000
    );
}

#[test]
fn domain_conversions_are_exact_and_identity_pinned() {
    // Identity / uncommitted: both directions are the literal input.
    let identity = RateSnapshot::IDENTITY;
    assert_eq!(seek::wall_ms(12_345, &identity), 12_345);
    assert_eq!(seek::content_ms(12_345, &identity), 12_345);
    let uncommitted = snapshot(1, 2, false, 50);
    assert_eq!(seek::wall_ms(12_345, &uncommitted), 12_345);
    assert_eq!(seek::content_ms(12_345, &uncommitted), 12_345);

    // Committed 50 % (source 1, output 2): wall = 2·content exactly, and
    // the inverse halves it.
    let half = snapshot(1, 2, true, 50);
    assert_eq!(seek::wall_ms(30_000, &half), 60_000);
    assert_eq!(seek::content_ms(60_000, &half), 30_000);
    // Round trip on exact values.
    assert_eq!(seek::content_ms(seek::wall_ms(4_321, &half), &half), 4_321);

    // Committed 175 % (source 7, output 4): the round trip through the
    // integer ms domains stays within 1 ms of the origin — the documented
    // quantization slop the anchor math tolerates.
    let fast = snapshot(7, 4, true, 175);
    for t in [1_000i32, 12_345, 59_999] {
        let round_trip = seek::content_ms(seek::wall_ms(t, &fast), &fast);
        assert!(
            (round_trip - t).abs() <= 1,
            "175% round trip drifted: {t} -> {round_trip}"
        );
    }
}

// ── Rebuild-at-T expectation model ───────────────────────────────────

#[test]
fn rebuild_expectations_mirror_the_engine_per_kind_semantics() {
    const T: i32 = 10_000;
    let notes = decode(&[
        // Control note: no record at all.
        note_bytes(-6, 100, 100, panels(&[]), durations(&[])),
        // Tap before T: consumed (grade 0, judgedAt = raw).
        note_bytes(0, 4_000, 4_000, panels(&[1]), durations(&[])),
        // Shock (all four panels of side 0) before T: consumed grade 6.
        note_bytes(0, 5_000, 5_000, panels(&[0, 1, 2, 3]), durations(&[])),
        // Armed marker: playhead-independent grade 5, judgedAt pre-filled.
        note_bytes(1, 20_000, 20_000, panels(&[2]), durations(&[])),
        // Freeze fully before T: head consumed + kind-2 back-patches the
        // head's hold progress to the full durations.
        note_bytes(0, 6_000, 6_000, panels(&[3]), durations(&[(3, 900)])),
        note_bytes(2, 6_900, 6_900, panels(&[3]), durations(&[])),
        // Tap after T: pending.
        note_bytes(0, 15_000, 15_000, panels(&[0]), durations(&[])),
    ]);
    let records = seek::rebuild_expectations(&notes, T);

    // One record per non-control note, in note order.
    assert_eq!(records.len(), 6);
    assert_eq!(records[0].note_index, 1);

    // Tap before T.
    assert_eq!((records[0].judged_at, records[0].grade), (4_000, 0));
    // Shock before T.
    assert_eq!((records[1].judged_at, records[1].grade), (5_000, 6));
    // Armed marker.
    assert_eq!(
        (records[2].judged_at, records[2].grade),
        (20_000, seek::GRADE_ARMED)
    );
    // Freeze head before T: consumed, freeze participant, hold progress
    // back-patched to the full durations by the kind-2 walk.
    assert_eq!((records[3].judged_at, records[3].grade), (6_000, 0));
    assert!(records[3].freeze_participant);
    assert_eq!(records[3].hold_progress[3], 900);
    // Freeze end marker: always armed grade 7 with judgedAt pre-filled.
    assert_eq!(
        (records[4].judged_at, records[4].grade),
        (6_900, seek::GRADE_ARMED_SHOCK)
    );
    // Tap after T: pending.
    assert_eq!(
        (records[5].judged_at, records[5].grade),
        (-1, seek::GRADE_PENDING)
    );
    assert!(!records[5].freeze_participant);
}

// ── Spanning-freeze neutralization (AC-3) ────────────────────────────

fn write_set(writes: &[RecordWrite]) -> Vec<(usize, i32)> {
    writes
        .iter()
        .map(|write| (write.byte_offset, write.value))
        .collect()
}

#[test]
fn neutralization_emits_nothing_without_a_spanning_freeze() {
    const T: i32 = 10_000;
    // No freezes at all.
    let plain = decode(&[
        note_bytes(0, 4_000, 4_000, panels(&[1]), durations(&[])),
        note_bytes(0, 15_000, 15_000, panels(&[0]), durations(&[])),
    ]);
    assert!(seek::neutralization_writes(&plain, T).is_empty());

    // Freeze fully before T (engine's own back-patch covers it).
    let before = decode(&[
        note_bytes(0, 6_000, 6_000, panels(&[3]), durations(&[(3, 900)])),
        note_bytes(2, 6_900, 6_900, panels(&[3]), durations(&[])),
    ]);
    assert!(seek::neutralization_writes(&before, T).is_empty());

    // Freeze fully after T.
    let after = decode(&[
        note_bytes(0, 15_000, 15_000, panels(&[2]), durations(&[(2, 800)])),
        note_bytes(2, 15_800, 15_800, panels(&[2]), durations(&[])),
    ]);
    assert!(seek::neutralization_writes(&after, T).is_empty());
}

#[test]
fn neutralization_covers_the_spanning_freeze_exactly() {
    const T: i32 = 10_000;
    // Record indices: 0 = pre-T tap, 1 = spanning head, 2 = its end
    // marker (a control note in between must not shift record indexing).
    let notes = decode(&[
        note_bytes(0, 4_000, 4_000, panels(&[0]), durations(&[])),
        note_bytes(0, 9_000, 9_000, panels(&[1]), durations(&[(1, 3_000)])),
        note_bytes(-7, 9_500, 9_500, panels(&[]), durations(&[])),
        note_bytes(2, 12_000, 12_000, panels(&[1]), durations(&[])),
    ]);
    let writes = seek::neutralization_writes(&notes, T);

    let head_base = 1 * seek::RECORD_STRIDE;
    let end_base = 2 * seek::RECORD_STRIDE;
    let mut expected: Vec<(usize, i32)> = (0..seek::PANEL_COUNT)
        .map(|panel| {
            (
                head_base + seek::RECORD_HOLD_PROGRESS_OFFSET + panel * 4,
                if panel == 1 { 3_000 } else { 0 },
            )
        })
        .collect();
    expected.push((end_base + seek::RECORD_GRADE_OFFSET, seek::GRADE_OK));
    expected.push((end_base + seek::RECORD_JUDGED_AT_OFFSET, 12_000));
    assert_eq!(write_set(&writes), expected);
}

#[test]
fn neutralization_handles_multi_panel_and_back_to_back_freezes() {
    const T: i32 = 10_000;
    // A multi-panel (jump) freeze spanning T, then a back-to-back second
    // freeze on the same panels entirely after T: only the first emits.
    let notes = decode(&[
        note_bytes(
            0,
            8_000,
            8_000,
            panels(&[1, 2]),
            durations(&[(1, 4_000), (2, 4_000)]),
        ),
        note_bytes(2, 12_000, 12_000, panels(&[1, 2]), durations(&[])),
        note_bytes(
            0,
            12_500,
            12_500,
            panels(&[1, 2]),
            durations(&[(1, 1_000), (2, 1_000)]),
        ),
        note_bytes(2, 13_500, 13_500, panels(&[1, 2]), durations(&[])),
    ]);
    let writes = seek::neutralization_writes(&notes, T);

    let head_base = 0;
    let end_base = seek::RECORD_STRIDE;
    let mut expected: Vec<(usize, i32)> = (0..seek::PANEL_COUNT)
        .map(|panel| {
            (
                head_base + seek::RECORD_HOLD_PROGRESS_OFFSET + panel * 4,
                if panel == 1 || panel == 2 { 4_000 } else { 0 },
            )
        })
        .collect();
    expected.push((end_base + seek::RECORD_GRADE_OFFSET, seek::GRADE_OK));
    expected.push((end_base + seek::RECORD_JUDGED_AT_OFFSET, 12_000));
    assert_eq!(write_set(&writes), expected);
}

#[test]
fn decode_notes_refuses_partial_strides() {
    assert!(seek::decode_notes(&[0u8; 0x60 + 1]).is_none());
    assert!(seek::decode_notes(&[]).map(|notes| notes.is_empty()) == Some(true));
    let _ = SeekQuantization {
        blocks: 0,
        t_q_ms: 0,
    };
}

#[test]
fn blocks_to_wall_ms_round_trips_quantize_seek() {
    // The production-like grid: 128 samples/block at 44 100 Hz (~2.90 ms).
    let (spb, rate, max_blocks) = (128u32, 44_100u32, 1_000_000u64);
    for target in [0, 1, 2_903, 34_813, 60_000, 599_000] {
        let quantized = seek::quantize_seek(target, spb, rate, max_blocks).expect("grid sane");
        assert_eq!(
            seek::blocks_to_wall_ms(quantized.blocks, spb, rate),
            Some(quantized.t_q_ms),
            "block {} of target {} ms",
            quantized.blocks,
            target
        );
    }
}

#[test]
fn blocks_to_wall_ms_explicit_values() {
    // 128 samples @ 8 kHz = exactly 16 ms per block.
    assert_eq!(seek::blocks_to_wall_ms(0, 128, 8_000), Some(0));
    assert_eq!(seek::blocks_to_wall_ms(1, 128, 8_000), Some(16));
    assert_eq!(seek::blocks_to_wall_ms(1_000, 128, 8_000), Some(16_000));
    // Degenerate grids refuse.
    assert_eq!(seek::blocks_to_wall_ms(1, 0, 8_000), None);
    assert_eq!(seek::blocks_to_wall_ms(1, 128, 0), None);
}

// ── Display⇄raw end-domain converters (Step 4, design §4.2) ──────────

/// The converter fixture: a leading kind −6 control note (control notes
/// carry valid time pairs and stay IN the converter's domain) plus real
/// notes over two different slopes, so interpolation is observable:
/// raw {−2_000, 0, 10_000, 20_000, 40_000} ↔
/// display {−1_600, 0, 8_000, 16_000, 48_000}
/// (slope 0.8 up to raw 20_000, then slope 1.6).
fn converter_notes() -> Vec<NoteView> {
    decode(&[
        note_bytes(-6, -1_600, -2_000, panels(&[]), durations(&[])),
        note_bytes(0, 0, 0, panels(&[0]), durations(&[])),
        note_bytes(0, 8_000, 10_000, panels(&[1]), durations(&[])),
        note_bytes(0, 16_000, 20_000, panels(&[2]), durations(&[])),
        note_bytes(0, 48_000, 40_000, panels(&[3]), durations(&[])),
    ])
}

#[test]
fn display_for_raw_is_exact_at_note_points_and_interpolates_midpoints() {
    let notes = converter_notes();
    // Note points return the exact display values.
    for (raw, display) in [(-2_000, -1_600), (0, 0), (10_000, 8_000), (40_000, 48_000)] {
        assert_eq!(seek::display_for_raw(&notes, raw), Some(display));
    }
    // Bracket midpoints return the linear midpoint of each slope.
    assert_eq!(seek::display_for_raw(&notes, 5_000), Some(4_000));
    assert_eq!(seek::display_for_raw(&notes, 15_000), Some(12_000));
    assert_eq!(seek::display_for_raw(&notes, 30_000), Some(32_000));
}

#[test]
fn display_for_raw_extrapolates_past_the_edges() {
    let notes = converter_notes();
    // Below the first note: the FIRST distinct pair's slope extends
    // (slope 0.8 through (−2_000, −1_600)).
    assert_eq!(seek::display_for_raw(&notes, -4_000), Some(-3_200));
    // Past the last note: the LAST pair's slope extends (slope 1.6
    // through (40_000, 48_000)) — a b_ms just past the last note still
    // maps monotonically (strictly above the last display value).
    assert_eq!(seek::display_for_raw(&notes, 45_000), Some(56_000));
    let last_display = seek::display_for_raw(&notes, 40_000).unwrap();
    assert!(seek::display_for_raw(&notes, 40_001).unwrap() > last_display);
}

#[test]
fn raw_for_display_inverts_and_round_trips() {
    let notes = converter_notes();
    // The inverse at note points and midpoints.
    for (display, raw) in [(0, 0), (8_000, 10_000), (12_000, 15_000), (48_000, 40_000)] {
        assert_eq!(seek::raw_for_display(&notes, display), Some(raw));
    }
    // Round-trip within the documented ±1 interpolation slop.
    for raw in (-3_000..=45_000).step_by(777) {
        let display = seek::display_for_raw(&notes, raw).expect("forward");
        let back = seek::raw_for_display(&notes, display).expect("inverse");
        assert!(
            (back - raw).abs() <= 1,
            "round-trip drifted: raw {raw} -> display {display} -> {back}"
        );
    }
}

#[test]
fn converters_refuse_degenerate_vectors() {
    // Empty and single-note vectors have no bracket.
    let empty: Vec<NoteView> = Vec::new();
    assert_eq!(seek::display_for_raw(&empty, 1_000), None);
    assert_eq!(seek::raw_for_display(&empty, 1_000), None);
    let single = decode(&[note_bytes(0, 8_000, 10_000, panels(&[0]), durations(&[]))]);
    assert_eq!(seek::display_for_raw(&single, 1_000), None);
    assert_eq!(seek::raw_for_display(&single, 1_000), None);
    // All-equal keys (zero slope) cannot interpolate — refuse rather
    // than divide by zero.
    let flat = decode(&[
        note_bytes(0, 8_000, 10_000, panels(&[0]), durations(&[])),
        note_bytes(0, 9_000, 10_000, panels(&[1]), durations(&[])),
    ]);
    assert_eq!(seek::display_for_raw(&flat, 12_000), None);
    // The inverse over the same vector still works where ITS keys are
    // distinct (display 8_000/9_000 bracket fine).
    assert_eq!(seek::raw_for_display(&flat, 8_500), Some(10_000));
}
