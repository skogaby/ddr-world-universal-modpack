//! Chart-vs-audio offset estimation.
//!
//! Scores every candidate shift `d` (whole ms) by summing the onset
//! envelope at each chart event moved by `d`, weighted by the event's
//! weight. The best shift, refined between samples, is the measured
//! offset. Three checks guard it — peak at the search edge, a rival
//! alignment, disagreeing halves — and a cap bounds the correction.

use super::onset::{self, Envelope};
use super::{
    AudioView, Outcome, Params, Refusal, SyncEstimate, SyncEvent, EDGE_MARGIN_SAMPLES,
    MAX_CORRECTION_LIMIT_MS, MIN_EVENTS, RIVAL_EXCLUSION_MS, RIVAL_REFUSE_RATIO, SEARCH_MARGIN_MS,
    SPLIT_REFUSE_MS, TARGET_OFFSET_MS,
};

/// Seconds of audio kept clear around each usable event beyond the
/// search half-width, so every scored sample lies inside the envelope.
const EVENT_MARGIN_S: f64 = 0.1;

fn refused_early(reason: Refusal, events_used: usize) -> SyncEstimate {
    SyncEstimate {
        outcome: Outcome::Refused(reason),
        measured_ms: None,
        correction_ms: None,
        rival: None,
        split_ms: None,
        events_used,
    }
}

/// Measure how far `events` sit from the onsets in `audio`, and decide
/// whether and how far to move the chart.
///
/// Never fails: inputs that cannot be trusted yield
/// [`Outcome::Refused`], which callers treat as "leave the sync alone".
/// See [`SyncEstimate`] for the sign conventions.
#[must_use]
pub fn estimate(audio: &AudioView<'_>, events: &[SyncEvent], params: &Params) -> SyncEstimate {
    let env = onset::envelope(audio);
    estimate_on(env.as_ref(), events, params)
}

/// [`estimate`] over an already computed envelope (`None` = the audio had
/// none: refused as `NoAudio`). One envelope serves every SSQ file of a
/// song.
#[must_use]
pub fn estimate_on(env: Option<&Envelope>, events: &[SyncEvent], params: &Params) -> SyncEstimate {
    let Some(env) = env else {
        return refused_early(Refusal::NoAudio, 0);
    };

    let cap = params.max_correction_ms.min(MAX_CORRECTION_LIMIT_MS);
    let half = cap + SEARCH_MARGIN_MS;
    let margin_s = f64::from(half) / 1000.0 + EVENT_MARGIN_S;
    let mut usable: Vec<SyncEvent> = events
        .iter()
        .copied()
        .filter(|e| {
            e.time_s.is_finite()
                && e.weight.is_finite()
                && e.time_s - margin_s >= env.start_s()
                && e.time_s + margin_s <= env.end_s()
        })
        .collect();
    if usable.len() < MIN_EVENTS {
        return refused_early(Refusal::TooFewEvents, usable.len());
    }
    usable.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));

    // Candidate shifts d ∈ [round(T) − half, round(T) + half] ms.
    let lo_ms = TARGET_OFFSET_MS.round() - f64::from(half);
    let len = 2 * half as usize + 1;
    let curve = score_curve(env, &usable, lo_ms, len);
    let (index, position) = peak(&curve);
    let measured = lo_ms + position;
    let at_edge = index < EDGE_MARGIN_SAMPLES || index + EDGE_MARGIN_SAMPLES >= curve.len();
    let rival = rival_ratio(&curve, index);

    let (first, second) = usable.split_at(usable.len() / 2);
    let half_peak = |part: &[SyncEvent]| lo_ms + peak(&score_curve(env, part, lo_ms, len)).1;
    let split = (half_peak(first) - half_peak(second)).abs();

    let correction = measured - TARGET_OFFSET_MS;
    let outcome = if at_edge {
        Outcome::Refused(Refusal::AtSearchEdge)
    } else if rival >= RIVAL_REFUSE_RATIO {
        Outcome::Refused(Refusal::AmbiguousPeak)
    } else if split > SPLIT_REFUSE_MS {
        Outcome::Refused(Refusal::HalvesDisagree)
    } else {
        verdict(correction, cap)
    };

    SyncEstimate {
        outcome,
        measured_ms: Some(measured),
        correction_ms: Some(correction),
        rival: Some(rival),
        split_ms: Some(split),
        events_used: usable.len(),
    }
}

/// Score of each candidate shift `lo_ms + i` for `i ∈ 0..len`.
fn score_curve(env: &Envelope, events: &[SyncEvent], lo_ms: f64, len: usize) -> Vec<f64> {
    (0..len)
        .map(|i| {
            let d = (lo_ms + i as f64) / 1000.0;
            events
                .iter()
                .map(|e| e.weight * env.sample(e.time_s + d))
                .sum()
        })
        .collect()
}

/// Index of the curve's maximum and its position refined through a
/// parabola over the neighboring samples.
fn peak(curve: &[f64]) -> (usize, f64) {
    let index = curve
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    let mut position = index as f64;
    if index > 0 && index + 1 < curve.len() {
        let (a, b, c) = (curve[index - 1], curve[index], curve[index + 1]);
        let den = a - 2.0 * b + c;
        if den.abs() > 1e-12 {
            position += 0.5 * (a - c) / den;
        }
    }
    (index, position)
}

/// Median of `values` (0 for an empty slice).
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    match n {
        0 => 0.0,
        _ if n % 2 == 1 => sorted[n / 2],
        _ => 0.5 * (sorted[n / 2 - 1] + sorted[n / 2]),
    }
}

/// Height of the strongest local maximum more than
/// [`RIVAL_EXCLUSION_MS`] from the peak, relative to the peak, both
/// measured above the curve's median. 1.0 when the peak is not above
/// the median (a flat or featureless curve); 0.0 with no rival.
fn rival_ratio(curve: &[f64], index: usize) -> f64 {
    let med = median(curve);
    let top = curve.get(index).copied().unwrap_or(med) - med;
    if top <= 0.0 {
        return 1.0;
    }
    (1..curve.len().saturating_sub(1))
        .filter(|&j| j.abs_diff(index) > RIVAL_EXCLUSION_MS)
        .filter(|&j| curve[j] >= curve[j - 1] && curve[j] >= curve[j + 1])
        .map(|j| (curve[j] - med) / top)
        .fold(0.0, f64::max)
}

/// Decide the applied correction: refuse beyond the cap, else round to
/// whole ms (half away from zero); a correction that rounds to 0 leaves
/// the chart unchanged.
fn verdict(correction_ms: f64, cap_ms: u32) -> Outcome {
    if !correction_ms.is_finite() || correction_ms.abs() > f64::from(cap_ms) {
        return Outcome::Refused(Refusal::BeyondCap);
    }
    // |correction| ≤ cap ≤ MAX_CORRECTION_LIMIT_MS, so this fits i32.
    match correction_ms.round() as i32 {
        0 => Outcome::Unchanged,
        delta_ms => Outcome::Apply { delta_ms },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sync::{Outcome, Params, Refusal, SyncEvent, TARGET_OFFSET_MS};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const RATE: u32 = 44_100;
    const SECONDS: f64 = 8.0;

    /// Deterministic xorshift64* in [-1, 1).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            let x = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
            (x >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        }
    }

    /// Irregular chart events: a 0.1 s grid from 0.5 s to 7.5 s, about
    /// 60% occupied, weights 1 or 2.
    fn events() -> Vec<SyncEvent> {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        (0..70)
            .filter_map(|i| {
                let keep = rng.next() < 0.2;
                let heavy = rng.next() > 0.3;
                keep.then(|| SyncEvent {
                    time_s: 0.5 + 0.1 * f64::from(i),
                    weight: if heavy { 2.0 } else { 1.0 },
                })
            })
            .collect()
    }

    /// Stereo samples with a 20 ms decaying noise burst at every event
    /// time plus `shift_ms`, the second half of the events moved a further
    /// `split_ms`, over low-level background noise.
    fn audio(events: &[SyncEvent], shift_ms: f64, split_ms: f64) -> Vec<i16> {
        let frames = (SECONDS * f64::from(RATE)) as usize;
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        let mut mono: Vec<f64> = (0..frames).map(|_| 0.01 * rng.next()).collect();
        let burst = (0.020 * f64::from(RATE)) as usize;
        let half = events.len() / 2;
        for (i, e) in events.iter().enumerate() {
            let extra = if i >= half { split_ms } else { 0.0 };
            let at = e.time_s + (shift_ms + extra) / 1000.0;
            let start = (at * f64::from(RATE)).round() as usize;
            for k in 0..burst {
                if let Some(s) = mono.get_mut(start + k) {
                    let decay = (-(k as f64) / (0.005 * f64::from(RATE))).exp();
                    *s += 0.6 * decay * rng.next();
                }
            }
        }
        mono.iter()
            .flat_map(|&s| {
                let v = (s.clamp(-1.0, 1.0) * 32_767.0) as i16;
                [v, v]
            })
            .collect()
    }

    fn view(samples: &[i16]) -> AudioView<'_> {
        AudioView {
            samples,
            sample_rate: RATE,
            channels: 2,
        }
    }

    fn shifted(events: &[SyncEvent], by_ms: f64) -> Vec<SyncEvent> {
        events
            .iter()
            .map(|e| SyncEvent {
                time_s: e.time_s + by_ms / 1000.0,
                weight: e.weight,
            })
            .collect()
    }

    fn measured(samples: &[i16], events: &[SyncEvent], params: &Params) -> Result<f64, String> {
        estimate(&view(samples), events, params)
            .measured_ms
            .ok_or_else(|| "estimate should measure".to_string())
    }

    fn baseline() -> Result<f64, String> {
        let ev = events();
        measured(&audio(&ev, 0.0, 0.0), &ev, &Params::default())
    }

    #[test]
    fn fixture_has_enough_events() {
        assert!(events().len() >= 40, "got {}", events().len());
    }

    #[test]
    fn recovers_audio_shifts() -> TestResult {
        let ev = events();
        let m0 = baseline()?;
        for x in [-55.0, -30.0, -7.0, 1.0, 13.0, 40.0, 55.0] {
            let m = measured(&audio(&ev, x, 0.0), &ev, &Params::default())?;
            assert!(
                (m - m0 - x).abs() <= 0.5,
                "shift {x}: measured {m}, baseline {m0}"
            );
        }
        Ok(())
    }

    #[test]
    fn refuses_too_few_events() {
        let ev: Vec<SyncEvent> = events().into_iter().take(20).collect();
        let est = estimate(&view(&audio(&ev, 0.0, 0.0)), &ev, &Params::default());
        assert_eq!(est.outcome, Outcome::Refused(Refusal::TooFewEvents));
        assert_eq!(est.measured_ms, None);
        assert_eq!(est.events_used, 20);
    }

    #[test]
    fn refuses_without_audio() {
        let est = estimate(&view(&[]), &events(), &Params::default());
        assert_eq!(est.outcome, Outcome::Refused(Refusal::NoAudio));
        assert_eq!(est.measured_ms, None);
    }

    #[test]
    fn refuses_peak_at_search_edge() -> TestResult {
        let ev = events();
        let m0 = baseline()?;
        let params = Params {
            max_correction_ms: 30,
        };
        // The search is round(T) ± 40 ms = −42..=38 ms. A true peak just
        // past its end leaves the curve still rising at the last sample.
        // (Much further out, the curve is featureless and the estimate is
        // refused as ambiguous instead.)
        let est = estimate(&view(&audio(&ev, 40.0 - m0, 0.0)), &ev, &params);
        assert_eq!(
            est.outcome,
            Outcome::Refused(Refusal::AtSearchEdge),
            "{est:?}"
        );
        assert!(est.measured_ms.is_some());
        Ok(())
    }

    #[test]
    fn refuses_beyond_cap() -> TestResult {
        let ev = events();
        let m0 = baseline()?;
        let params = Params {
            max_correction_ms: 30,
        };
        let est = estimate(
            &view(&audio(&ev, 35.0 + TARGET_OFFSET_MS - m0, 0.0)),
            &ev,
            &params,
        );
        assert_eq!(est.outcome, Outcome::Refused(Refusal::BeyondCap), "{est:?}");
        let correction = est.correction_ms.ok_or("correction")?;
        assert!((correction - 35.0).abs() <= 0.5, "correction {correction}");
        Ok(())
    }

    #[test]
    fn refuses_two_equal_alignments() {
        // Every event duplicated 30 ms later with the same weight: the
        // audio fits both alignments equally. (30 ms, not less, so the
        // duplicates fall after each 20 ms burst rather than on its
        // cutoff, which would make one alignment score lower.)
        let ev = events();
        let mut doubled = ev.clone();
        doubled.extend(shifted(&ev, 30.0));
        let est = estimate(&view(&audio(&ev, 0.0, 0.0)), &doubled, &Params::default());
        assert_eq!(
            est.outcome,
            Outcome::Refused(Refusal::AmbiguousPeak),
            "{est:?}"
        );
        assert!(est.rival.is_some_and(|r| r >= 0.9));
    }

    #[test]
    fn refuses_when_halves_disagree() {
        let ev = events();
        let est = estimate(&view(&audio(&ev, 0.0, 10.0)), &ev, &Params::default());
        assert_eq!(
            est.outcome,
            Outcome::Refused(Refusal::HalvesDisagree),
            "{est:?}"
        );
        assert!(
            est.split_ms.is_some_and(|s| (s - 10.0).abs() <= 1.0),
            "{est:?}"
        );
    }

    #[test]
    fn in_sync_chart_is_unchanged() -> TestResult {
        let ev = events();
        let m0 = baseline()?;
        let chart = shifted(&ev, m0 - TARGET_OFFSET_MS);
        let est = estimate(&view(&audio(&ev, 0.0, 0.0)), &chart, &Params::default());
        assert_eq!(est.outcome, Outcome::Unchanged, "{est:?}");
        Ok(())
    }

    #[test]
    fn offset_chart_gets_rounded_correction() -> TestResult {
        let ev = events();
        let m0 = baseline()?;
        // Chart 7 ms early relative to in-sync: it should move 7 ms later.
        let chart = shifted(&ev, m0 - TARGET_OFFSET_MS - 7.0);
        let est = estimate(&view(&audio(&ev, 0.0, 0.0)), &chart, &Params::default());
        assert_eq!(est.outcome, Outcome::Apply { delta_ms: 7 }, "{est:?}");
        Ok(())
    }

    #[test]
    fn estimate_on_shared_envelope_matches_estimate() {
        let ev = events();
        let samples = audio(&ev, 13.0, 0.0);
        let env = onset::envelope(&view(&samples));
        assert_eq!(
            estimate_on(env.as_ref(), &ev, &Params::default()),
            estimate(&view(&samples), &ev, &Params::default())
        );
        assert_eq!(
            estimate_on(None, &ev, &Params::default()).outcome,
            Outcome::Refused(Refusal::NoAudio)
        );
    }

    #[test]
    fn verdict_rounds_half_away_and_caps() {
        assert_eq!(verdict(0.49, 60), Outcome::Unchanged);
        assert_eq!(verdict(0.5, 60), Outcome::Apply { delta_ms: 1 });
        assert_eq!(verdict(-0.5, 60), Outcome::Apply { delta_ms: -1 });
        assert_eq!(verdict(-1.49, 60), Outcome::Apply { delta_ms: -1 });
        assert_eq!(verdict(60.0, 60), Outcome::Apply { delta_ms: 60 });
        assert_eq!(verdict(60.2, 60), Outcome::Refused(Refusal::BeyondCap));
        assert_eq!(verdict(-60.2, 60), Outcome::Refused(Refusal::BeyondCap));
        assert_eq!(verdict(f64::NAN, 60), Outcome::Refused(Refusal::BeyondCap));
    }

    #[test]
    fn median_and_rival_helpers() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        // Peak at 2 (value 10), rival local max at 20 (value 6), median 0.
        let mut curve = vec![0.0; 25];
        curve[2] = 10.0;
        curve[20] = 6.0;
        assert!((rival_ratio(&curve, 2) - 0.6).abs() < 1e-12);
        // A neighbor within the exclusion is not a rival.
        curve[20] = 0.0;
        curve[10] = 9.0;
        assert!((rival_ratio(&curve, 2) - 0.0).abs() < 1e-12);
    }
}
