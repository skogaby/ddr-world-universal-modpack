//! `#SCROLLS` / `#SPEEDS` — StepMania's display-only scroll gimmicks, pure.
//!
//! Both tags live in StepMania's `TimingData` but change nothing about
//! *when* a note is judged — only where it is drawn:
//!
//! - `#SCROLLS: beat=factor,…` is a piecewise-constant scroll factor whose
//!   integral maps a song beat to a *display beat*. A `0` region freezes
//!   every later note on the receptors until the region ends; a negative
//!   factor sends them backwards. The identity is the empty table or any
//!   table whose factors are all `1` (only display-beat *differences* are
//!   used, so a constant shift is invisible).
//! - `#SPEEDS: beat=ratio=delay=unit,…` is a global distance multiplier
//!   that, at `beat`, ramps from the previous segment's ratio to `ratio`
//!   over `delay` beats (`unit` 0) or seconds (`unit` 1) and then holds.
//!
//! The arithmetic is ported from StepMania 5's `TimingData::GetDisplayedBeat`
//! and `TimingData::GetDisplayedSpeedPercent`, including their edge rules:
//! the first scroll segment's factor extrapolates backwards before its beat;
//! a speed ramp is interpolated in *time* even for beat units (the segment's
//! start/end beats are converted through the tempo map once, at table
//! build); before the first speed segment the multiplier is `1` when that
//! segment ramps, else its ratio; the ramp into the first segment starts
//! from `1`; a zero-length ramp is a step. Entries at the same beat: the
//! last one wins (StepMania replaces the segment at that row).
//!
//! The tables are consumed per note per frame from an `extern "C"` render
//! hook (`services::scroll_remap`): every query is allocation-free, lock-free
//! and panic-free (`get`/`partition_point`, no indexing, finite-guarded).
//!
//! Purity contract: `std` only, `crate::core::sm` imports only. Mounted by
//! `scripts/validate_stepmania.sh` and `tools/sm_pack_check` through
//! `core/sm/mod.rs`.

use super::model::{Beat, Rational, Timing, Warning};
use super::simfile::parse_number;

/// One `#SCROLLS` entry as authored: the factor applies from `beat` until
/// the next entry's beat.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScrollEntry {
    pub beat: Beat,
    pub factor: Rational,
}

/// `#SPEEDS` ramp duration unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeedUnit {
    Beats,
    Seconds,
}

impl SpeedUnit {
    /// The `unit` field value StepMania writes (`0` beats, `1` seconds).
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Beats => 0,
            Self::Seconds => 1,
        }
    }

    #[must_use]
    pub const fn from_tag(tag: u8) -> Self {
        if tag == 1 {
            Self::Seconds
        } else {
            Self::Beats
        }
    }
}

/// One `#SPEEDS` entry as authored.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpeedEntry {
    pub beat: Beat,
    pub ratio: Rational,
    pub delay: Rational,
    pub unit: SpeedUnit,
}

/// Parse a `#SCROLLS` value tolerantly: blank entries and trailing commas
/// are ignored, malformed entries warn and are skipped.
#[must_use]
pub fn parse_scrolls(s: &str, warnings: &mut Vec<Warning>) -> Vec<ScrollEntry> {
    let mut out = Vec::new();
    for (i, entry) in s.split(',').enumerate() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let fields: Vec<&str> = entry.split('=').collect();
        let parsed = match fields.as_slice() {
            [b, f] => parse_number(b).zip(parse_number(f)),
            _ => None,
        };
        match parsed {
            Some((beat, factor)) => out.push(ScrollEntry {
                beat: Beat::from_rational(beat),
                factor,
            }),
            None => warnings.push(Warning(format!(
                "#SCROLLS entry {i} {entry:?} is not `beat=factor`; skipped"
            ))),
        }
    }
    out
}

/// Parse a `#SPEEDS` value tolerantly. A 3-field entry means `unit` 0
/// (StepMania's own default); a negative delay is skipped with a warning
/// (StepMania refuses it too).
#[must_use]
pub fn parse_speeds(s: &str, warnings: &mut Vec<Warning>) -> Vec<SpeedEntry> {
    let mut out = Vec::new();
    for (i, entry) in s.split(',').enumerate() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let fields: Vec<&str> = entry.split('=').collect();
        let (b, r, d, u) = match fields.as_slice() {
            [b, r, d] => (*b, *r, *d, "0"),
            [b, r, d, u] => (*b, *r, *d, *u),
            _ => {
                warnings.push(Warning(format!(
                    "#SPEEDS entry {i} {entry:?} is not `beat=ratio=delay[=unit]`; skipped"
                )));
                continue;
            }
        };
        let (Some(beat), Some(ratio), Some(delay)) =
            (parse_number(b), parse_number(r), parse_number(d))
        else {
            warnings.push(Warning(format!(
                "#SPEEDS entry {i} {entry:?} has a non-numeric field; skipped"
            )));
            continue;
        };
        if delay.is_negative() {
            warnings.push(Warning(format!(
                "#SPEEDS entry {i} {entry:?} has a negative delay; skipped"
            )));
            continue;
        }
        let unit = if u.trim() == "1" {
            SpeedUnit::Seconds
        } else {
            SpeedUnit::Beats
        };
        out.push(SpeedEntry {
            beat: Beat::from_rational(beat),
            ratio,
            delay,
            unit,
        });
    }
    out
}

// ---------------------------------------------------------------------
// Beat → seconds (for the time-domain speed ramps)
// ---------------------------------------------------------------------

/// Seconds elapsed from beat 0 to `beat` under `timing`, ignoring
/// `#OFFSET` (StepMania's `GetElapsedTimeFromBeatNoOffset`): BPM segments
/// plus every stop at a beat *before* `beat` and every delay at a beat *at
/// or before* it (a delay precedes its beat's notes, a stop follows them).
/// Warps are ignored — charts with warps are never committed. Non-positive
/// BPMs contribute no time rather than infinities.
#[must_use]
pub fn elapsed_seconds(timing: &Timing, beat: f64) -> f64 {
    if !beat.is_finite() {
        return 0.0;
    }
    let mut bpms: Vec<(f64, f64)> = timing
        .bpms
        .iter()
        .map(|(b, v)| (b.as_rational().as_f64(), v.as_rational().as_f64()))
        .collect();
    bpms.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut t = 0.0;
    if let Some(&(_, first_bpm)) = bpms.first() {
        // StepMania treats the first segment as starting at beat 0 whatever
        // its authored beat.
        let mut cur_beat = 0.0f64;
        let mut cur_bpm = first_bpm;
        for &(seg_beat, seg_bpm) in bpms.iter().skip(1) {
            if seg_beat >= beat {
                break;
            }
            if seg_beat > cur_beat {
                t += beats_to_seconds(seg_beat - cur_beat, cur_bpm);
                cur_beat = seg_beat;
            }
            cur_bpm = seg_bpm;
        }
        if beat > cur_beat {
            t += beats_to_seconds(beat - cur_beat, cur_bpm);
        } else if beat < 0.0 {
            t -= beats_to_seconds(-beat, first_bpm);
        }
    }
    for (b, d) in &timing.stops {
        if b.as_rational().as_f64() < beat {
            t += d.as_f64();
        }
    }
    for (b, d) in &timing.delays {
        if b.as_rational().as_f64() <= beat {
            t += d.as_f64();
        }
    }
    t
}

fn beats_to_seconds(beats: f64, bpm: f64) -> f64 {
    if bpm > 0.0 && bpm.is_finite() {
        beats * 60.0 / bpm
    } else {
        0.0
    }
}

/// Total delay authored exactly at `beat` (`GetDelayAtBeat`).
fn delay_at(timing: &Timing, beat: f64) -> f64 {
    timing
        .delays
        .iter()
        .filter(|(b, _)| b.as_rational().as_f64() == beat)
        .map(|(_, d)| d.as_f64())
        .sum()
}

// ---------------------------------------------------------------------
// Scroll table
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct ScrollSeg {
    beat: f64,
    factor: f64,
    /// Display beat at `beat` (the integral of the earlier segments).
    cum: f64,
}

/// The `#SCROLLS` integral, ready for per-frame queries.
#[derive(Debug, Clone, Default)]
pub struct ScrollTable {
    segments: Vec<ScrollSeg>,
}

impl PartialEq for ScrollTable {
    fn eq(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self.segments.iter().zip(&other.segments).all(|(a, b)| {
                a.beat.to_bits() == b.beat.to_bits() && a.factor.to_bits() == b.factor.to_bits()
            })
    }
}

impl Eq for ScrollTable {}

impl ScrollTable {
    /// Build from authored entries: sorted by beat, same-beat duplicates
    /// resolved last-wins, non-finite values dropped.
    #[must_use]
    pub fn new(entries: &[ScrollEntry]) -> Self {
        let pairs: Vec<(f64, f64)> = entries
            .iter()
            .map(|e| (e.beat.as_rational().as_f64(), e.factor.as_f64()))
            .collect();
        Self::from_pairs(&pairs)
    }

    /// Build from `(beat, factor)` pairs (the index's stored form).
    #[must_use]
    pub fn from_pairs(pairs: &[(f64, f64)]) -> Self {
        let mut pairs: Vec<(f64, f64)> = pairs
            .iter()
            .copied()
            .filter(|(b, f)| b.is_finite() && f.is_finite())
            .collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut dedup: Vec<(f64, f64)> = Vec::with_capacity(pairs.len());
        for p in pairs {
            match dedup.last_mut() {
                Some(last) if last.0 == p.0 => *last = p,
                _ => dedup.push(p),
            }
        }
        let mut segments = Vec::with_capacity(dedup.len());
        let mut cum = 0.0;
        let mut prev: Option<(f64, f64)> = None;
        for (beat, factor) in dedup {
            if let Some((pb, pf)) = prev {
                cum += (beat - pb) * pf;
            }
            segments.push(ScrollSeg { beat, factor, cum });
            prev = Some((beat, factor));
        }
        Self { segments }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// `(beat, factor)` pairs in table order.
    pub fn entries(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.segments.iter().map(|s| (s.beat, s.factor))
    }

    /// True when the table cannot move anything: no segments, or every
    /// factor is exactly `1`.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.segments.iter().all(|s| s.factor == 1.0)
    }

    /// `GetDisplayedBeat`: the integral of the factor from the first
    /// segment's beat to `beat` (identity when empty; the first factor
    /// extrapolates backwards).
    #[must_use]
    pub fn display_beat(&self, beat: f64) -> f64 {
        if self.segments.is_empty() {
            return beat;
        }
        let idx = self
            .segments
            .partition_point(|s| s.beat <= beat)
            .saturating_sub(1);
        match self.segments.get(idx) {
            Some(seg) => seg.cum + (beat - seg.beat) * seg.factor,
            None => beat,
        }
    }
}

// ---------------------------------------------------------------------
// Speed table
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct SpeedSeg {
    beat: f64,
    ratio: f64,
    delay: f64,
    /// Ramp start / end in elapsed seconds (no offset). Precomputed through
    /// the tempo map so the per-frame query is a comparison and a lerp.
    start_s: f64,
    end_s: f64,
}

/// The `#SPEEDS` multiplier, ready for per-frame queries.
#[derive(Debug, Clone, Default)]
pub struct SpeedTable {
    segments: Vec<SpeedSeg>,
}

impl PartialEq for SpeedTable {
    fn eq(&self, other: &Self) -> bool {
        self.segments.len() == other.segments.len()
            && self.segments.iter().zip(&other.segments).all(|(a, b)| {
                a.beat.to_bits() == b.beat.to_bits()
                    && a.ratio.to_bits() == b.ratio.to_bits()
                    && a.delay.to_bits() == b.delay.to_bits()
                    && a.start_s.to_bits() == b.start_s.to_bits()
                    && a.end_s.to_bits() == b.end_s.to_bits()
            })
    }
}

impl Eq for SpeedTable {}

impl SpeedTable {
    /// Build from authored entries under `timing` (the chart's effective
    /// tempo map — ramps are interpolated in seconds).
    #[must_use]
    pub fn new(entries: &[SpeedEntry], timing: &Timing) -> Self {
        Self::with_clock(entries, &|b| elapsed_seconds(timing, b), &|b| {
            delay_at(timing, b)
        })
    }

    /// `new` with an explicit beat → elapsed-seconds mapping and a
    /// delay-at-beat lookup.
    #[must_use]
    pub fn with_clock(
        entries: &[SpeedEntry],
        beat_to_seconds: &dyn Fn(f64) -> f64,
        delay_at_beat: &dyn Fn(f64) -> f64,
    ) -> Self {
        let mut raw: Vec<(f64, f64, f64, SpeedUnit)> = entries
            .iter()
            .map(|e| {
                (
                    e.beat.as_rational().as_f64(),
                    e.ratio.as_f64(),
                    e.delay.as_f64(),
                    e.unit,
                )
            })
            .filter(|(b, r, d, _)| b.is_finite() && r.is_finite() && d.is_finite() && *d >= 0.0)
            .collect();
        raw.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut dedup: Vec<(f64, f64, f64, SpeedUnit)> = Vec::with_capacity(raw.len());
        for e in raw {
            match dedup.last_mut() {
                Some(last) if last.0 == e.0 => *last = e,
                _ => dedup.push(e),
            }
        }
        let segments: Vec<(f64, f64, f64, f64, f64)> = dedup
            .into_iter()
            .map(|(beat, ratio, delay, unit)| {
                let start_s = beat_to_seconds(beat) - delay_at_beat(beat);
                let end_s = match unit {
                    SpeedUnit::Seconds => start_s + delay,
                    SpeedUnit::Beats => beat_to_seconds(beat + delay) - delay_at_beat(beat + delay),
                };
                (beat, ratio, delay, start_s, end_s)
            })
            .collect();
        Self::from_segments(&segments)
    }

    /// Build from resolved `(beat, ratio, delay, start_s, end_s)` segments
    /// (the index's stored form). Sorted by beat, same-beat last wins,
    /// non-finite values dropped, a non-finite end collapses to a step.
    #[must_use]
    pub fn from_segments(segments: &[(f64, f64, f64, f64, f64)]) -> Self {
        let mut raw: Vec<(f64, f64, f64, f64, f64)> = segments
            .iter()
            .copied()
            .filter(|(b, r, d, s, _)| {
                b.is_finite() && r.is_finite() && d.is_finite() && *d >= 0.0 && s.is_finite()
            })
            .collect();
        raw.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut dedup: Vec<(f64, f64, f64, f64, f64)> = Vec::with_capacity(raw.len());
        for e in raw {
            match dedup.last_mut() {
                Some(last) if last.0 == e.0 => *last = e,
                _ => dedup.push(e),
            }
        }
        let segments = dedup
            .into_iter()
            .map(|(beat, ratio, delay, start_s, end_s)| SpeedSeg {
                beat,
                ratio,
                delay,
                start_s,
                end_s: if end_s.is_finite() { end_s } else { start_s },
            })
            .collect();
        Self { segments }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// `(beat, ratio, delay, start_s, end_s)` in table order.
    pub fn entries(&self) -> impl Iterator<Item = (f64, f64, f64, f64, f64)> + '_ {
        self.segments
            .iter()
            .map(|s| (s.beat, s.ratio, s.delay, s.start_s, s.end_s))
    }

    /// True when the multiplier is `1` everywhere: no segments, or every
    /// ratio is exactly `1` (a ramp from 1 to 1 is flat).
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.segments.iter().all(|s| s.ratio == 1.0)
    }

    /// `GetDisplayedSpeedPercent(beat, elapsed_s)`. `elapsed_s` is the
    /// song clock in the same no-offset domain as [`elapsed_seconds`].
    #[must_use]
    pub fn speed_at(&self, beat: f64, elapsed_s: f64) -> f64 {
        let Some(first) = self.segments.first() else {
            return 1.0;
        };
        let index = self
            .segments
            .partition_point(|s| s.beat <= beat)
            .saturating_sub(1);
        let Some(seg) = self.segments.get(index) else {
            return 1.0;
        };
        if index == 0 && first.delay > 0.0 && elapsed_s < seg.start_s {
            return 1.0;
        }
        if seg.end_s >= elapsed_s && (index > 0 || first.delay > 0.0) {
            let prior = match index.checked_sub(1).and_then(|i| self.segments.get(i)) {
                Some(p) => p.ratio,
                None => 1.0,
            };
            let used = elapsed_s - seg.start_s;
            let duration = seg.end_s - seg.start_s;
            let ratio_used = if duration == 0.0 {
                1.0
            } else {
                used / duration
            };
            let v = prior + ratio_used * (seg.ratio - prior);
            return if v.is_finite() { v } else { seg.ratio };
        }
        seg.ratio
    }
}

// ---------------------------------------------------------------------
// Combined record
// ---------------------------------------------------------------------

/// One chart's (or song's) scroll gimmicks, as the renderer seam consumes
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScrollGimmicks {
    pub scroll: ScrollTable,
    pub speed: SpeedTable,
}

impl ScrollGimmicks {
    #[must_use]
    pub fn new(scrolls: &[ScrollEntry], speeds: &[SpeedEntry], timing: &Timing) -> Self {
        Self {
            scroll: ScrollTable::new(scrolls),
            speed: SpeedTable::new(speeds, timing),
        }
    }

    /// Nothing to remap: both tables are identity.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.scroll.is_identity() && self.speed.is_identity()
    }

    /// The renderer remap in one place (pure, so the harness can pin it):
    /// given the current beat and clock and a note's beat delta in ticks,
    /// the remapped delta (ticks, rounded half away from zero, saturating)
    /// and the multiplier to scale the stock result by.
    ///
    /// `now_ticks` and `d_beat_ticks` use `Beat::TICKS_PER_BEAT` per beat;
    /// `elapsed_s` is the no-offset song clock.
    #[must_use]
    pub fn remap(&self, now_ticks: i32, d_beat_ticks: i64, elapsed_s: f64) -> (i64, f64) {
        let tpb = Beat::TICKS_PER_BEAT as f64;
        let now = f64::from(now_ticks) / tpb;
        let note = now + (d_beat_ticks as f64) / tpb;
        let d = (self.scroll.display_beat(note) - self.scroll.display_beat(now)) * tpb;
        // `as i64` saturates and maps NaN to 0 — never panics.
        let d_ticks = if d.is_finite() {
            d.round() as i64
        } else {
            d_beat_ticks
        };
        let s = self.speed.speed_at(now, elapsed_s);
        (d_ticks, if s.is_finite() { s } else { 1.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::model::Bpm;

    fn r(n: i64, d: i64) -> Rational {
        Rational::new(n, d).unwrap()
    }

    fn timing(bpm: i64) -> Timing {
        Timing {
            bpms: vec![(Beat::zero(), Bpm::from_rational(r(bpm, 1)))],
            ..Timing::default()
        }
    }

    fn scrolls(text: &str) -> Vec<ScrollEntry> {
        let mut w = Vec::new();
        let v = parse_scrolls(text, &mut w);
        assert!(w.is_empty(), "{w:?}");
        v
    }

    fn speeds(text: &str) -> Vec<SpeedEntry> {
        let mut w = Vec::new();
        let v = parse_speeds(text, &mut w);
        assert!(w.is_empty(), "{w:?}");
        v
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Tech Spectrum Super `260-MISC.[MASTER] Matt Silver` (the acceptance
    /// chart), 0-factor regions at 116.0–116.5, 171.5–171.75, 264.167–282.667
    /// alternating 0 / 1.5, 293 = 0.
    const MATT_SILVER_SCROLLS: &str = "0.000=1.000,116.000=0.000,116.500=1.000,171.500=0.000,171.750=1.000,264.167=0.000,265.000=1.500,266.000=0.000,267.000=1.500,282.667=1.000,293.000=0.000";
    const MATT_SILVER_SPEEDS: &str = "0.000=1.000=0.000=0,160.000=0.000=0.000=0,160.021=1.000=6.000=0,196.000=0.500=0.000=0,196.021=1.000=16.000=0";

    #[test]
    fn parses_tolerantly() {
        let mut w = Vec::new();
        let s = parse_scrolls("0=1,,8.5=0.5,\n,junk,4=x,1=2=3", &mut w);
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].beat, Beat::from_rational(r(17, 2)));
        assert_eq!(s[1].factor, r(1, 2));
        assert_eq!(w.len(), 3, "{w:?}");

        let mut w = Vec::new();
        let p = parse_speeds("0=1=0=0,8=2=1.5=1,12=0.5=2,16=1=-1=0,bad,20=1=0=7", &mut w);
        assert_eq!(p.len(), 4, "{p:?}");
        assert_eq!(p[1].unit, SpeedUnit::Seconds);
        assert_eq!(p[2].unit, SpeedUnit::Beats);
        assert_eq!(p[2].delay, r(2, 1));
        assert_eq!(p[3].unit, SpeedUnit::Beats, "unknown unit tag → beats");
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn identity_tables() {
        let t = ScrollTable::new(&[]);
        assert!(t.is_identity());
        assert!(close(t.display_beat(12.5), 12.5));
        let t = ScrollTable::new(&scrolls("0=1"));
        assert!(t.is_identity());
        assert!(close(t.display_beat(12.5), 12.5));
        assert!(close(t.display_beat(-3.0), -3.0));
        let t = ScrollTable::new(&scrolls("4=1"));
        assert!(t.is_identity(), "a constant shift is invisible");
        assert!(!ScrollTable::new(&scrolls("0=1,8=0")).is_identity());

        assert!(SpeedTable::new(&[], &timing(120)).is_identity());
        assert!(SpeedTable::new(&speeds("0=1=0=0"), &timing(120)).is_identity());
        assert!(SpeedTable::new(&speeds("0=1=0=0,8=1=4=0"), &timing(120)).is_identity());
        assert!(!SpeedTable::new(&speeds("0=1=0=0,8=2=4=0"), &timing(120)).is_identity());
        assert!(ScrollGimmicks::new(&[], &[], &timing(120)).is_identity());
    }

    #[test]
    fn zero_factor_freezes_display_beat() {
        let t = ScrollTable::new(&scrolls("0=1,8=0,12=1"));
        assert!(close(t.display_beat(8.0), 8.0));
        assert!(close(t.display_beat(10.0), 8.0));
        assert!(close(t.display_beat(12.0), 8.0));
        assert!(
            close(t.display_beat(13.5), 9.5),
            "resumes with the lost 4 beats"
        );
    }

    #[test]
    fn negative_factor_and_backward_extrapolation() {
        let t = ScrollTable::new(&scrolls("0=1,4=-1,6=1"));
        assert!(close(t.display_beat(4.0), 4.0));
        assert!(close(t.display_beat(5.0), 3.0));
        assert!(close(t.display_beat(6.0), 2.0));
        assert!(close(t.display_beat(7.0), 3.0));
        // First segment at beat 4 with factor 2: StepMania extrapolates backwards.
        let t = ScrollTable::new(&scrolls("4=2"));
        assert!(close(t.display_beat(0.0), -8.0));
        assert!(close(t.display_beat(5.0), 2.0));
    }

    #[test]
    fn same_beat_last_wins_and_unsorted_input_is_sorted() {
        let t = ScrollTable::new(&scrolls("8=0,0=1,8=2"));
        assert_eq!(t.len(), 2);
        assert!(close(t.display_beat(9.0), 10.0));
    }

    #[test]
    fn matt_silver_scrolls() {
        let t = ScrollTable::new(&scrolls(MATT_SILVER_SCROLLS));
        assert!(!t.is_identity());
        assert!(close(t.display_beat(116.25), t.display_beat(116.0)));
        assert!(close(t.display_beat(116.5), t.display_beat(116.0)));
        assert!(close(
            t.display_beat(116.5 + 0.75),
            t.display_beat(116.0) + 0.75
        ));
        assert!(close(t.display_beat(100.0), 100.0));
        // 171.5–171.75 frozen; 0.75 lost in total by 172.
        assert!(close(t.display_beat(172.0), 172.0 - 0.5 - 0.25));
        // 264.167–265 frozen, 265–266 ×1.5, 266–267 frozen, 267–282.667 ×1.5.
        let d264 = t.display_beat(264.167);
        assert!(close(t.display_beat(265.0), d264));
        assert!(close(t.display_beat(266.0), d264 + 1.5));
        assert!(close(t.display_beat(267.0), d264 + 1.5));
        assert!(close(t.display_beat(268.0), d264 + 3.0));
        // 293 = 0 to the end.
        assert!(close(t.display_beat(400.0), t.display_beat(293.0)));
    }

    #[test]
    fn matt_silver_speeds() {
        let tm = timing(120);
        let t = SpeedTable::new(&speeds(MATT_SILVER_SPEEDS), &tm);
        let at = |beat: f64| t.speed_at(beat, elapsed_seconds(&tm, beat));
        assert!(close(at(0.0), 1.0));
        assert!(close(at(159.9), 1.0));
        assert!(close(at(160.0), 0.0), "{}", at(160.0));
        assert!(close(at(160.01), 0.0));
        // 160.021 ramps 0 → 1 over 6 beats.
        assert!(close(at(160.021), 0.0));
        assert!(close(at(163.021), 0.5), "{}", at(163.021));
        assert!((at(163.0) - 0.5).abs() < 0.01);
        assert!(close(at(166.021), 1.0));
        assert!(close(at(170.0), 1.0));
        // 196 steps to 0.5, 196.021 ramps 0.5 → 1 over 16 beats.
        assert!(close(at(196.0), 0.5));
        assert!(close(at(196.021 + 8.0), 0.75), "{}", at(204.021));
        assert!(close(at(196.021 + 16.0), 1.0));
        assert!(close(at(300.0), 1.0));
    }

    #[test]
    fn seconds_unit_ramps_in_time() {
        // 120 BPM: beat 8 = 4.0 s; a 1.5 s ramp ends at 5.5 s (beat 11).
        let tm = timing(120);
        let t = SpeedTable::new(&speeds("0=1=0=0,8=2=1.5=1"), &tm);
        assert!(close(t.speed_at(7.9, 3.95), 1.0));
        assert!(close(t.speed_at(8.0, 4.0), 1.0));
        assert!(close(t.speed_at(9.5, 4.75), 1.5));
        assert!(close(t.speed_at(11.0, 5.5), 2.0));
        assert!(close(t.speed_at(20.0, 10.0), 2.0));
        // Beat units follow the tempo map: at 60 BPM the same 1.5 → 1.5 beats.
        let tm60 = timing(60);
        let t = SpeedTable::new(&speeds("0=1=0=0,8=2=1.5=0"), &tm60);
        assert!(close(t.speed_at(8.75, elapsed_seconds(&tm60, 8.75)), 1.5));
    }

    #[test]
    fn first_segment_rules_follow_stepmania() {
        let tm = timing(120);
        // A single stepping segment: its ratio applies even before its beat.
        let t = SpeedTable::new(&speeds("8=2=0=0"), &tm);
        assert!(close(t.speed_at(0.0, 0.0), 2.0));
        assert!(close(t.speed_at(9.0, 4.5), 2.0));
        // A ramping first segment: 1 before it, ramp from 1 across it.
        let t = SpeedTable::new(&speeds("8=3=4=0"), &tm);
        assert!(close(t.speed_at(0.0, 0.0), 1.0));
        assert!(close(t.speed_at(10.0, 5.0), 2.0));
        assert!(close(t.speed_at(12.0, 6.0), 3.0));
        assert!(close(t.speed_at(13.0, 6.5), 3.0));
    }

    #[test]
    fn elapsed_seconds_counts_stops_and_delays_like_stepmania() {
        let mut tm = timing(120);
        tm.bpms
            .push((Beat::from_integer(8), Bpm::from_rational(r(60, 1))));
        tm.stops.push((Beat::from_integer(4), r(1, 2)));
        tm.delays.push((Beat::from_integer(8), r(1, 4)));
        assert!(close(elapsed_seconds(&tm, 0.0), 0.0));
        assert!(close(elapsed_seconds(&tm, 4.0), 2.0), "stop AT 4 not yet");
        assert!(close(elapsed_seconds(&tm, 4.5), 2.75));
        assert!(
            close(elapsed_seconds(&tm, 8.0), 4.0 + 0.5 + 0.25),
            "delay AT 8 counts"
        );
        assert!(close(elapsed_seconds(&tm, 9.0), 4.75 + 1.0));
        assert!(close(elapsed_seconds(&tm, -2.0), -1.0));
        // Degenerate tempo maps never produce infinities.
        let bad = Timing {
            bpms: vec![(Beat::zero(), Bpm::from_rational(r(0, 1)))],
            ..Timing::default()
        };
        assert!(elapsed_seconds(&bad, 10.0).is_finite());
        assert!(elapsed_seconds(&Timing::default(), 10.0).is_finite());
    }

    #[test]
    fn raw_constructors_round_trip_the_resolved_tables() {
        let tm = timing(120);
        let g = ScrollGimmicks::new(
            &scrolls(MATT_SILVER_SCROLLS),
            &speeds(MATT_SILVER_SPEEDS),
            &tm,
        );
        let pairs: Vec<(f64, f64)> = g.scroll.entries().collect();
        let segs: Vec<(f64, f64, f64, f64, f64)> = g.speed.entries().collect();
        let back = ScrollGimmicks {
            scroll: ScrollTable::from_pairs(&pairs),
            speed: SpeedTable::from_segments(&segs),
        };
        assert_eq!(back, g);
        assert_ne!(back, ScrollGimmicks::default());
        assert!(close(
            back.speed.speed_at(163.021, elapsed_seconds(&tm, 163.021)),
            0.5
        ));
    }

    #[test]
    fn remap_arithmetic() {
        let tm = timing(120);
        let g = ScrollGimmicks::new(
            &scrolls(MATT_SILVER_SCROLLS),
            &speeds(MATT_SILVER_SPEEDS),
            &tm,
        );
        let tpb = Beat::TICKS_PER_BEAT;
        // Plain region: delta unchanged, speed 1.
        let now = 100 * tpb as i32;
        let (d, s) = g.remap(now, 2 * tpb, elapsed_seconds(&tm, 100.0));
        assert_eq!(d, 2 * tpb);
        assert!(close(s, 1.0));
        // now = 115, note at 117: the 116.0–116.5 freeze removes half a beat.
        let now = 115 * tpb as i32;
        let (d, _) = g.remap(now, 2 * tpb, elapsed_seconds(&tm, 115.0));
        assert_eq!(d, (1.5 * tpb as f64) as i64);
        // now inside the freeze: notes inside it sit on the receptors.
        let now = (116.25 * tpb as f64) as i32;
        let (d, _) = g.remap(now, (0.2 * tpb as f64) as i64, elapsed_seconds(&tm, 116.25));
        assert_eq!(d, 0);
        // Passed notes (negative delta) remap symmetrically.
        let now = 117 * tpb as i32;
        let (d, _) = g.remap(now, -2 * tpb, elapsed_seconds(&tm, 117.0));
        assert_eq!(d, -(1.5 * tpb as f64) as i64);
        // Speed scaling at the squeeze.
        let now = 160 * tpb as i32;
        let (d, s) = g.remap(now, 4 * tpb, elapsed_seconds(&tm, 160.0));
        assert_eq!(d, 4 * tpb);
        assert!(close(s, 0.0));
        let now = (163.021 * tpb as f64) as i32;
        let (_, s) = g.remap(now, 4 * tpb, elapsed_seconds(&tm, 163.021));
        assert!((s - 0.5).abs() < 1e-3, "{s}");
        // Identity tables: delta passes through exactly.
        let id = ScrollGimmicks::default();
        assert_eq!(id.remap(12345, 777, 0.0), (777, 1.0));
        assert_eq!(id.remap(12345, 1 << 40, 0.0).0, 1 << 40);
    }
}
