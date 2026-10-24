//! Lead-in: the silence prepended to the audio (and the equal shift of the
//! chart, `ssq::writer::WriteOptions::lead_in_ms`) so a song never starts
//! with a note in the first frames.
//!
//! Design §4.2 asks for the smallest whole-ADPCM-block multiple such that
//! `tempo_data[0] = −#OFFSET·1000 + lead_in_ms ≥ 60` (auto-sync headroom)
//! and the first note sits ≥ 2 s into the audio (originally phrased as
//! "first note tick ≥ 4096 and ≥ 2 s of audio before the first note"; a
//! lead-in shifts time, not ticks, so the tick half is expressed purely as
//! the 2 s rule — a chart whose first note is at tick 0 simply gets ≥ 2 s
//! of silence in front).
//!
//! Blocks are 128 frames, which is not a whole number of milliseconds at
//! 44.1 kHz (2.9025 ms) or 48 kHz (2.6667 ms). The audio gets exactly
//! `frames`; the chart gets `ms`:
//! - at 48 kHz three blocks are exactly 8 ms, so the block count is bumped
//!   by at most two to land on a whole millisecond (`exact == true`);
//! - at 44.1 kHz a whole-millisecond multiple needs 441 blocks (1.28 s),
//!   so `ms` is rounded to the nearest millisecond instead — the chart and
//!   the audio disagree by at most 0.5 ms, below the auto-sync estimator's
//!   resolution and two orders under the Marvelous window.
//! Both constraints are re-checked in the rounded millisecond domain the
//! SSQ writer actually uses, bumping blocks until they hold.

use super::BLOCK_FRAMES;

/// Minimum `tempo_data[0]` in ms.
pub const MIN_TD0_MS: i64 = 60;
/// Minimum audio time of the first note in ms.
pub const MIN_FIRST_NOTE_MS: i64 = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadIn {
    /// What the SSQ writer shifts the chart by.
    pub ms: u32,
    /// What the decoder prepends; a multiple of 128.
    pub frames: u64,
    /// `frames` is exactly `ms` milliseconds at this rate.
    pub exact: bool,
}

impl LeadIn {
    pub const ZERO: Self = Self {
        ms: 0,
        frames: 0,
        exact: true,
    };

    /// A lead-in of `blocks` blocks at `rate`.
    #[must_use]
    pub fn from_blocks(blocks: u64, rate: u32) -> Self {
        let frames = blocks.saturating_mul(BLOCK_FRAMES as u64);
        let rate = u64::from(rate.max(1));
        let num = frames.saturating_mul(1000);
        let ms = u32::try_from((num + rate / 2) / rate).unwrap_or(u32::MAX);
        Self {
            ms,
            frames,
            exact: num % rate == 0,
        }
    }

    /// The smallest block multiple giving at least `ms` milliseconds.
    #[must_use]
    pub fn at_least_ms(ms: u32, rate: u32) -> Self {
        let frames = (u64::from(ms) * u64::from(rate.max(1))).div_ceil(1000);
        Self::from_blocks(frames.div_ceil(BLOCK_FRAMES as u64), rate)
    }
}

/// Pick the lead-in for a chart.
///
/// `offset_s` is the simfile `#OFFSET` (tick 0 is at `−offset_s` seconds
/// in the audio). `first_note_s` is the audio time of the earliest declared
/// note with no lead-in — read it back from the SSQ written with
/// `lead_in_ms = 0` (`TempoConverter::beat_to_music_count(first_tick)` /
/// 1000) so stops, delays and per-chart timing are accounted for. `rate` is
/// the normalised audio rate.
#[must_use]
pub fn choose_lead_in(offset_s: f64, first_note_s: f64, rate: u32) -> LeadIn {
    let rate = if rate == 0 { 44_100 } else { rate };
    let offset_s = if offset_s.is_finite() { offset_s } else { 0.0 };
    let first_note_s = if first_note_s.is_finite() {
        first_note_s
    } else {
        0.0
    };
    let td0_ms = (-offset_s * 1000.0).round() as i64;
    let first_note_ms = (first_note_s * 1000.0).round() as i64;

    // Continuous requirement as a starting point, then the smallest block
    // count whose ROUNDED millisecond value satisfies both rules — the
    // millisecond domain is the one the SSQ writer uses.
    let need_s = (MIN_TD0_MS as f64 / 1000.0 + offset_s)
        .max(MIN_FIRST_NOTE_MS as f64 / 1000.0 - first_note_s)
        .max(0.0);
    let need_frames = (need_s * f64::from(rate)).floor().max(0.0) as u64;
    let mut blocks = need_frames / BLOCK_FRAMES as u64;

    let satisfied = |l: &LeadIn| {
        let ms = i64::from(l.ms);
        td0_ms + ms >= MIN_TD0_MS && first_note_ms + ms >= MIN_FIRST_NOTE_MS
    };

    // Bounded: the floor is at most one block short and rounding is
    // < 0.5 ms against blocks of > 2.6 ms.
    let mut lead = LeadIn::from_blocks(blocks, rate);
    for _ in 0..4 {
        if satisfied(&lead) {
            break;
        }
        blocks += 1;
        lead = LeadIn::from_blocks(blocks, rate);
    }
    if lead.frames == 0 {
        return LeadIn::ZERO;
    }
    // Cheap exactness: a whole-millisecond block count within two blocks.
    if !lead.exact {
        for extra in 1..=2 {
            let candidate = LeadIn::from_blocks(blocks + extra, rate);
            if candidate.exact {
                return candidate;
            }
        }
    }
    lead
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_lead_in_when_both_rules_already_hold() {
        // Big Money: #OFFSET −0.241, first note at beat 8 of 140 BPM.
        let l = choose_lead_in(-0.241, 0.241 + 8.0 * 60.0 / 140.0, 44_100);
        assert_eq!(l, LeadIn::ZERO);
    }

    #[test]
    fn first_note_at_beat_zero_gets_two_seconds() {
        let l = choose_lead_in(-0.241, 0.241, 44_100);
        assert_eq!(l.frames % 128, 0);
        // 606 blocks = 77 568 frames = 1758.78 ms → rounds to 1759, and
        // 241 + 1759 = 2000 holds in the millisecond domain.
        assert_eq!(l.frames, 606 * 128);
        assert_eq!(l.ms, 1759);
        assert!(!l.exact);
        assert_eq!(241 + i64::from(l.ms), MIN_FIRST_NOTE_MS);
        // One block less would break the 2 s rule.
        let less = LeadIn::from_blocks(605, 44_100);
        assert!(241 + i64::from(less.ms) < MIN_FIRST_NOTE_MS);

        let l = choose_lead_in(-0.241, 0.241, 48_000);
        assert_eq!(l.frames, 660 * 128);
        assert_eq!(l.ms, 1760);
        assert!(l.exact);
    }

    #[test]
    fn td0_rule_dominates_for_positive_offsets() {
        // Chart starts 0.5 s before the audio; first note far away.
        let l = choose_lead_in(0.5, 2.93, 44_100);
        assert_eq!(l.frames, 193 * 128);
        assert_eq!(l.ms, 560);
        assert_eq!(-500 + i64::from(l.ms), MIN_TD0_MS);
        let l48 = choose_lead_in(0.5, 2.93, 48_000);
        assert!(l48.exact);
        assert_eq!(l48.frames % 384, 0);
        assert!(-500 + i64::from(l48.ms) >= MIN_TD0_MS);
    }

    #[test]
    fn rounding_never_violates_the_millisecond_rules() {
        for rate in [44_100u32, 48_000] {
            for tenth in -30i32..30 {
                let offset = f64::from(tenth) / 10.0;
                for note_tenth in 0..40 {
                    let first = f64::from(note_tenth) / 10.0;
                    let l = choose_lead_in(offset, first, rate);
                    let td0 = (-offset * 1000.0).round() as i64 + i64::from(l.ms);
                    let note = (first * 1000.0).round() as i64 + i64::from(l.ms);
                    assert!(td0 >= MIN_TD0_MS, "{rate} {offset} {first} {l:?}");
                    assert!(note >= MIN_FIRST_NOTE_MS, "{rate} {offset} {first} {l:?}");
                    assert_eq!(l.frames % 128, 0);
                    let ms_f = l.frames as f64 * 1000.0 / f64::from(rate);
                    assert!((ms_f - f64::from(l.ms)).abs() <= 0.5);
                    if rate == 48_000 && l.frames > 0 {
                        assert!(l.exact);
                    }
                }
            }
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(LeadIn::from_blocks(3, 48_000).ms, 8);
        assert!(LeadIn::from_blocks(3, 48_000).exact);
        assert_eq!(LeadIn::at_least_ms(1000, 48_000).frames, 48_000);
        assert_eq!(LeadIn::at_least_ms(1000, 44_100).frames, 44_160);
        assert!(choose_lead_in(f64::NAN, f64::INFINITY, 0).frames > 0);
    }
}
