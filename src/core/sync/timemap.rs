//! Beat → audio-time mapping for analysis.
//!
//! A [`TimeMap`] is the piecewise-linear timeline an output chart will
//! actually play with, built from the SSQ tempo pairs a materialised file
//! carries. It is `f64` and used only to place chart events for
//! correlation; nothing here is written to disk. (The tool's `from_song`
//! constructor is not ported: the runtime always measures the file it
//! wrote.)

use crate::core::sm::model::Beat;

/// Measure ticks per beat in an SSQ tempo chunk (`docs/ssq_format.md` §3).
const TICKS_PER_BEAT: f64 = Beat::TICKS_PER_BEAT as f64;

/// Piecewise-linear beat → audio-seconds mapping.
///
/// Anchors are `(beat, seconds)` sorted by beat. A stop is two anchors
/// at the same beat; a note exactly on a stop plays at the stop's start.
/// Outside the anchors the map extrapolates with the nearest tempo.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeMap {
    anchors: Vec<(f64, f64)>,
    /// Seconds per beat of the first segment with a nonzero beat span.
    first_slope: f64,
    /// Seconds per beat of the last segment with a nonzero beat span.
    last_slope: f64,
}

impl TimeMap {
    /// Build from SSQ `(measure_tick, tempo_data)` pairs at `tps` ticks
    /// per second (`docs/ssq_format.md` §3): beat = tick / 1024,
    /// seconds = tempo_data / tps.
    ///
    /// Returns `None` for `tps == 0`, fewer than two pairs, ticks that
    /// decrease, or pairs that never advance in beat.
    #[must_use]
    pub fn from_tempo_pairs(pairs: &[(i32, i32)], tps: u32) -> Option<TimeMap> {
        if tps == 0 || pairs.windows(2).any(|w| w[1].0 < w[0].0) {
            return None;
        }
        let tps = f64::from(tps);
        let anchors = pairs
            .iter()
            .map(|&(tick, td)| (f64::from(tick) / TICKS_PER_BEAT, f64::from(td) / tps))
            .collect();
        Self::from_anchors(anchors)
    }

    fn from_anchors(anchors: Vec<(f64, f64)>) -> Option<TimeMap> {
        let slope =
            |w: &[(f64, f64)]| (w[1].0 > w[0].0).then(|| (w[1].1 - w[0].1) / (w[1].0 - w[0].0));
        let first_slope = anchors.windows(2).find_map(slope)?;
        let last_slope = anchors.windows(2).rev().find_map(slope)?;
        Some(TimeMap {
            anchors,
            first_slope,
            last_slope,
        })
    }

    /// Audio time of `beat`, in seconds.
    #[must_use]
    pub fn seconds_at(&self, beat: f64) -> f64 {
        let n = self.anchors.len();
        let k = self.anchors.partition_point(|&(b, _)| b < beat);
        if let Some(&(b, s)) = self.anchors.get(k) {
            if b == beat {
                return s;
            }
        }
        let (Some(&(b0, s0)), Some(&(bn, sn))) = (self.anchors.first(), self.anchors.last()) else {
            return 0.0;
        };
        if k == 0 {
            return s0 + (beat - b0) * self.first_slope;
        }
        if k == n {
            return sn + (beat - bn) * self.last_slope;
        }
        // anchors[k-1].0 < beat < anchors[k].0, so the span is nonzero.
        match (self.anchors.get(k - 1), self.anchors.get(k)) {
            (Some(&(b0, s0)), Some(&(b1, s1))) => s0 + (s1 - s0) * (beat - b0) / (b1 - b0),
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    fn map(pairs: &[(i32, i32)], tps: u32) -> Result<TimeMap, Box<dyn std::error::Error>> {
        TimeMap::from_tempo_pairs(pairs, tps).ok_or_else(|| "map should build".into())
    }

    #[test]
    fn pairs_single_bpm_interpolates() -> TestResult {
        let m = map(&[(0, 0), (4096, 2000)], 1000)?;
        assert!(close(m.seconds_at(0.0), 0.0, 1e-12));
        assert!(close(m.seconds_at(2.0), 1.0, 1e-12));
        assert!(close(m.seconds_at(4.0), 2.0, 1e-12));
        Ok(())
    }

    #[test]
    fn pairs_stop_note_on_stop_takes_stop_start() -> TestResult {
        let m = map(&[(0, 0), (8192, 4000), (8192, 4500), (12288, 6500)], 1000)?;
        assert!(
            close(m.seconds_at(8.0), 4.0, 1e-12),
            "a note on a stop plays at its start"
        );
        assert!(
            close(m.seconds_at(9.0), 5.0, 1e-12),
            "notes after a stop include it"
        );
        Ok(())
    }

    #[test]
    fn pairs_extrapolate_before_and_after() -> TestResult {
        let m = map(&[(0, 0), (8192, 4000), (8192, 4500), (12288, 6500)], 1000)?;
        assert!(close(m.seconds_at(-1.0), -0.5, 1e-12));
        assert!(close(m.seconds_at(13.0), 7.0, 1e-12));
        Ok(())
    }

    #[test]
    fn pairs_extrapolate_past_trailing_stop() -> TestResult {
        // The last anchor closes a stop; extrapolation continues from it
        // with the last real tempo.
        let m = map(&[(0, 0), (4096, 2000), (4096, 2500)], 1000)?;
        assert!(close(m.seconds_at(5.0), 3.0, 1e-12));
        Ok(())
    }

    #[test]
    fn pairs_tps_scales_seconds() -> TestResult {
        let m = map(&[(0, 0), (4096, 300)], 150)?;
        assert!(close(m.seconds_at(4.0), 2.0, 1e-12));
        Ok(())
    }

    #[test]
    fn pairs_reject_malformed() {
        assert!(
            TimeMap::from_tempo_pairs(&[(0, 0)], 1000).is_none(),
            "one pair"
        );
        assert!(
            TimeMap::from_tempo_pairs(&[(0, 0), (4096, 2000)], 0).is_none(),
            "TPS 0"
        );
        assert!(
            TimeMap::from_tempo_pairs(&[(0, 0), (4096, 2000), (2048, 3000)], 1000).is_none(),
            "decreasing ticks"
        );
        assert!(
            TimeMap::from_tempo_pairs(&[(0, 0), (0, 500), (0, 900)], 1000).is_none(),
            "no beat span"
        );
    }
}
