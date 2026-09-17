//! Dance time from the chart's tempo map (A3 `FUN_18003a1b0`, the
//! DancePlaySequence's per-frame graph-rate setter — RE record in
//! `docs/background_dancers_research.md` §3.5). Pure, std-only, harness-
//! mounted.
//!
//! A3 scaled the WHOLE 3D scene's animation clock (dancers, stage loops,
//! camera clips — `SceneGraphManager+0x38` multiplies the update dt) by
//! ```text
//! rate = minBpm < 10 && ConfigBank["MOTION_STOP_SLOW"]      ? 1/12
//!      : ConfigBank["MOTION_BPM_DEPENDENCY"]                 ? maxBpm / 120
//!      : 1.0
//! ```
//! with `bpm` = each live side's current chart BPM. Retail shipped
//! `MOTION_STOP_SLOW = TRUE`, `MOTION_BPM_DEPENDENCY = FALSE`. The modpack
//! turns BOTH on by default (`background_dancers.bpm_sync` /
//! `.stop_slow` in `mod-config.json`).
//!
//! Instead of integrating a rate per frame (which cannot follow a training
//! rewind), the scene time `τ` is a PURE FUNCTION of the content-domain
//! music count `mc` through the chart's tempo map: over a tempo segment of
//! `Δtick` measure-ticks and `Δms` milliseconds,
//! - `rate = 1/12` (stop-slow) contributes `Δms / 12000` s,
//! - `rate = bpm/120` contributes `(bpm/120)·Δms/1000 = Δtick/2048` s
//!   EXACTLY (`bpm = 60000·Δtick/(1024·Δms)`) — dance time is half a second
//!   per beat, independent of the bpm value: the choreography clips are
//!   authored at 120 BPM (`mc_*_ne01_loop` = 242 frames ≈ 8 beats),
//! - `rate = 1` contributes `Δms / 1000` s.
//!
//! **Beat phase.** A3's option only matched the tempo (the clip started at
//! the song-start edge with whatever phase). In BPM-sync mode `τ = 0` is
//! pinned to the chart MEASURE boundary (tick multiple of 4096) nearest to
//! music time 0, so the clips' 120-BPM grid lands on the chart's beats and
//! downbeats; before that point `τ < 0` and one-shot clips hold frame 0.
//! The dance schedule additionally snaps its segment lengths to whole
//! beats ([`BEAT_TAU`]) so every cut lands on a beat.

/// Measure ticks per whole note (`docs/ssq_format.md` §1).
pub const TICKS_PER_MEASURE: i64 = 4096;
/// Measure ticks per beat (quarter note).
pub const TICKS_PER_BEAT: i64 = 1024;
/// Dance seconds per beat: the clips are authored at 120 BPM.
pub const BEAT_TAU: f32 = 0.5;
/// A3 `DAT_180264a58`: below this BPM the STOP slow-motion rate applies.
pub const STOP_BPM: f64 = 10.0;
/// A3 `0x3daaaaab`: the STOP slow-motion rate.
pub const STOP_RATE: f64 = 1.0 / 12.0;
/// A3 `DAT_180265258`: the BPM the clips are authored at.
pub const REFERENCE_BPM: f64 = 120.0;

/// The two A3 ConfigBank switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoOptions {
    /// `MOTION_BPM_DEPENDENCY`: scene rate = bpm / 120 (+ beat-phase pin).
    pub bpm_sync: bool,
    /// `MOTION_STOP_SLOW`: scene rate = 1/12 while the chart BPM < 10.
    pub stop_slow: bool,
}

impl TempoOptions {
    /// The modpack default: both on.
    pub const DEFAULT: TempoOptions = TempoOptions {
        bpm_sync: true,
        stop_slow: true,
    };
    /// A3 retail.
    pub const A3_RETAIL: TempoOptions = TempoOptions {
        bpm_sync: false,
        stop_slow: true,
    };
    /// Wall clock (no tempo influence at all).
    pub const REAL_TIME: TempoOptions = TempoOptions {
        bpm_sync: false,
        stop_slow: false,
    };
}

/// One tempo node: chart position in measure ticks, music count in ms
/// (the game's normalized `tempo_data·1000/TPS`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoNode {
    pub tick: i64,
    pub ms: f64,
}

/// The chart's tempo map with the dance-time prefix sums.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    nodes: Vec<TempoNode>,
    /// Un-anchored dance time at every node (`F(node.ms)`).
    tau_at: Vec<f64>,
    /// `F(mc_anchor)` — subtracted so `τ(anchor) = 0`.
    tau_anchor: f64,
    /// The music count `τ = 0` is pinned to (diagnostics).
    anchor_ms: f64,
    opts: TempoOptions,
}

/// Segment rate class per the A3 rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rate {
    StopSlow,
    BeatLinear,
    RealTime,
}

fn classify(dtick: i64, dms: f64, opts: TempoOptions) -> Rate {
    // bpm = 60000·Δtick/(1024·Δms); Δms == 0 ⇒ a warp (infinite BPM).
    let bpm = if dms > 0.0 {
        60000.0 * dtick as f64 / (TICKS_PER_BEAT as f64 * dms)
    } else {
        f64::INFINITY
    };
    if bpm < STOP_BPM && opts.stop_slow {
        Rate::StopSlow
    } else if opts.bpm_sync {
        Rate::BeatLinear
    } else {
        Rate::RealTime
    }
}

/// Dance time gained over a whole segment.
fn segment_tau(dtick: i64, dms: f64, rate: Rate) -> f64 {
    match rate {
        Rate::StopSlow => dms / 1000.0 * STOP_RATE,
        Rate::BeatLinear => dtick as f64 / TICKS_PER_BEAT as f64 * BEAT_TAU as f64,
        Rate::RealTime => dms / 1000.0,
    }
}

impl TempoMap {
    /// Build from the tempo nodes (file order; ticks and ms non-decreasing —
    /// nodes that violate that or fewer than 2 nodes ⇒ `None`).
    pub fn new(nodes: Vec<TempoNode>, opts: TempoOptions) -> Option<TempoMap> {
        if nodes.len() < 2 {
            return None;
        }
        for w in nodes.windows(2) {
            if w[1].tick < w[0].tick || w[1].ms < w[0].ms || !w[0].ms.is_finite() {
                return None;
            }
        }
        if !nodes[nodes.len() - 1].ms.is_finite() {
            return None;
        }
        let mut tau_at = Vec::with_capacity(nodes.len());
        tau_at.push(0.0);
        for i in 1..nodes.len() {
            let dtick = nodes[i].tick - nodes[i - 1].tick;
            let dms = nodes[i].ms - nodes[i - 1].ms;
            let r = classify(dtick, dms, opts);
            tau_at.push(tau_at[i - 1] + segment_tau(dtick, dms, r));
        }
        let mut map = TempoMap {
            nodes,
            tau_at,
            tau_anchor: 0.0,
            anchor_ms: 0.0,
            opts,
        };
        let anchor_ms = if opts.bpm_sync {
            // The measure boundary nearest to music time 0.
            let tick0 = map.tick_at(0.0);
            let measure = (tick0 / TICKS_PER_MEASURE as f64).round() as i64 * TICKS_PER_MEASURE;
            map.ms_at_tick(measure)
        } else {
            0.0
        };
        map.anchor_ms = anchor_ms;
        map.tau_anchor = map.raw_tau(anchor_ms);
        Some(map)
    }

    pub fn options(&self) -> TempoOptions {
        self.opts
    }

    /// The music count (ms) `τ = 0` is pinned to.
    pub fn anchor_ms(&self) -> f64 {
        self.anchor_ms
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Index `i ≥ 1` of the segment `(i−1, i)` containing `ms` (the first
    /// segment before the map, the last after it).
    fn segment_for_ms(&self, ms: f64) -> usize {
        let n = self.nodes.len();
        // partition_point: first node with node.ms > ms
        let idx = self.nodes.partition_point(|nd| nd.ms <= ms);
        idx.clamp(1, n - 1)
    }

    /// Chart position (measure ticks, fractional) at music count `ms`;
    /// extrapolates with the outer segments, flat inside a stop.
    pub fn tick_at(&self, ms: f64) -> f64 {
        let i = self.segment_for_ms(ms);
        let (a, b) = (self.nodes[i - 1], self.nodes[i]);
        let dms = b.ms - a.ms;
        if dms <= 0.0 {
            // warp or degenerate: tick jumps at this ms
            return if ms >= b.ms {
                b.tick as f64
            } else {
                a.tick as f64
            };
        }
        a.tick as f64 + (b.tick - a.tick) as f64 * (ms - a.ms) / dms
    }

    /// Music count at a chart position (the forward map; a stop's ms range
    /// maps its tick to the stop's START).
    pub fn ms_at_tick(&self, tick: i64) -> f64 {
        let n = self.nodes.len();
        let idx = self
            .nodes
            .partition_point(|nd| nd.tick < tick)
            .clamp(1, n - 1);
        let (a, b) = (self.nodes[idx - 1], self.nodes[idx]);
        let dtick = b.tick - a.tick;
        if dtick <= 0 {
            return a.ms;
        }
        a.ms + (b.ms - a.ms) * (tick - a.tick) as f64 / dtick as f64
    }

    /// Un-anchored dance time `F(ms)`.
    fn raw_tau(&self, ms: f64) -> f64 {
        let i = self.segment_for_ms(ms);
        let (a, b) = (self.nodes[i - 1], self.nodes[i]);
        let dtick = b.tick - a.tick;
        let dms = b.ms - a.ms;
        let r = classify(dtick, dms, self.opts);
        let base = self.tau_at[i - 1];
        if dms <= 0.0 {
            // A warp: the whole segment's dance time lands at once.
            return if ms >= b.ms { self.tau_at[i] } else { base };
        }
        // Linear inside the segment (also extrapolates outside the map).
        let frac = (ms - a.ms) / dms;
        base + segment_tau(dtick, dms, r) * frac
    }

    /// Dance time (seconds of the 120-BPM clip clock) at music count `ms`.
    pub fn tau(&self, ms: f64) -> f32 {
        (self.raw_tau(ms) - self.tau_anchor) as f32
    }

    /// The chart BPM of the segment containing `ms` (diagnostics; 0 inside
    /// a stop, `inf` across a warp).
    pub fn bpm_at(&self, ms: f64) -> f64 {
        let i = self.segment_for_ms(ms);
        let (a, b) = (self.nodes[i - 1], self.nodes[i]);
        let dms = b.ms - a.ms;
        if dms <= 0.0 {
            return f64::INFINITY;
        }
        60000.0 * (b.tick - a.tick) as f64 / (TICKS_PER_BEAT as f64 * dms)
    }
}

/// Nodes from the SSQ tempo chunk's raw pairs: `(time_offset ticks,
/// tempo_data seconds-ticks)` + the file's TPS, normalized to ms the way the
/// game does (`round(td·1000/TPS + 0.5)`, `docs/ssq_format.md` §3.4).
pub fn nodes_from_ssq_pairs(pairs: &[(i32, i32)], tps: i32) -> Vec<TempoNode> {
    if tps <= 0 {
        return Vec::new();
    }
    pairs
        .iter()
        .map(|&(tick, td)| TempoNode {
            tick: tick as i64,
            ms: ((td as f64) * 1000.0 / tps as f64 + 0.5).round(),
        })
        .collect()
}

/// Snap a dance-time length DOWN to whole beats (≥ one beat).
pub fn snap_to_beats(len: f32) -> f32 {
    let beats = (len / BEAT_TAU).floor().max(1.0);
    beats * BEAT_TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(bpm: f64, beats: i64) -> Vec<TempoNode> {
        // one node per beat, 0..=beats
        (0..=beats)
            .map(|b| TempoNode {
                tick: b * TICKS_PER_BEAT,
                ms: b as f64 * 60000.0 / bpm,
            })
            .collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn at_120_bpm_dance_time_is_music_time() {
        let m = TempoMap::new(constant(120.0, 64), TempoOptions::DEFAULT).unwrap();
        assert_eq!(m.anchor_ms(), 0.0);
        for ms in [0.0, 500.0, 1234.5, 20_000.0, 31_500.0] {
            assert!(close(m.tau(ms), ms as f32 / 1000.0), "{ms}");
        }
        // extrapolation beyond the map keeps the rate
        assert!(close(m.tau(40_000.0), 40.0));
        assert!(close(m.tau(-1_000.0), -1.0));
    }

    #[test]
    fn at_180_bpm_dance_time_runs_one_and_a_half_times() {
        let m = TempoMap::new(constant(180.0, 64), TempoOptions::DEFAULT).unwrap();
        // one beat = 333.33 ms of music = 0.5 s of dance
        assert!(close(m.tau(60000.0 / 180.0), 0.5));
        assert!(close(m.tau(10_000.0), 15.0));
        assert!((m.bpm_at(500.0) - 180.0).abs() < 1e-9);
    }

    #[test]
    fn at_60_bpm_dance_time_runs_at_half() {
        let m = TempoMap::new(constant(60.0, 64), TempoOptions::DEFAULT).unwrap();
        assert!(close(m.tau(10_000.0), 5.0));
    }

    #[test]
    fn real_time_options_ignore_the_tempo() {
        let m = TempoMap::new(constant(180.0, 64), TempoOptions::REAL_TIME).unwrap();
        assert!(close(m.tau(10_000.0), 10.0));
        assert_eq!(m.anchor_ms(), 0.0);
    }

    fn with_stop() -> Vec<TempoNode> {
        // 120 BPM, a 2 s stop at beat 8, then 120 BPM again
        let mut v = constant(120.0, 8);
        v.push(TempoNode {
            tick: 8 * TICKS_PER_BEAT,
            ms: 4000.0 + 2000.0,
        });
        for b in 9..=16 {
            v.push(TempoNode {
                tick: b * TICKS_PER_BEAT,
                ms: 6000.0 + (b - 8) as f64 * 500.0,
            });
        }
        v
    }

    #[test]
    fn stop_slows_to_one_twelfth_and_resumes() {
        let m = TempoMap::new(with_stop(), TempoOptions::DEFAULT).unwrap();
        assert!(close(m.tau(4000.0), 4.0));
        // halfway through the stop: 1 s of music → 1/12 s of dance
        assert!(close(m.tau(5000.0), 4.0 + 1.0 / 12.0));
        assert!(close(m.tau(6000.0), 4.0 + 2.0 / 12.0));
        // after the stop the beat clock resumes (+0.5 s per beat)
        assert!(close(m.tau(6500.0), 4.0 + 2.0 / 12.0 + 0.5));
        assert_eq!(m.bpm_at(5000.0), 0.0);
        // inside the stop the chart position is flat
        assert!((m.tick_at(5000.0) - (8 * TICKS_PER_BEAT) as f64).abs() < 1e-9);
    }

    #[test]
    fn a3_retail_options_freeze_only_the_stop() {
        let m = TempoMap::new(with_stop(), TempoOptions::A3_RETAIL).unwrap();
        assert!(close(m.tau(4000.0), 4.0));
        assert!(close(m.tau(6000.0), 4.0 + 2.0 / 12.0));
        // real time afterwards
        assert!(close(m.tau(7000.0), 4.0 + 2.0 / 12.0 + 1.0));
    }

    #[test]
    fn stop_without_stop_slow_freezes_dance_time() {
        let opts = TempoOptions {
            bpm_sync: true,
            stop_slow: false,
        };
        let m = TempoMap::new(with_stop(), opts).unwrap();
        assert!(close(m.tau(4000.0), 4.0));
        assert!(close(m.tau(5000.0), 4.0));
        assert!(close(m.tau(6000.0), 4.0));
        assert!(close(m.tau(6500.0), 4.5));
    }

    #[test]
    fn warp_jumps_dance_time_instantly() {
        // 120 BPM, then 4 beats warp at 2 s, then 120 BPM
        let v = vec![
            TempoNode { tick: 0, ms: 0.0 },
            TempoNode {
                tick: 4 * TICKS_PER_BEAT,
                ms: 2000.0,
            },
            TempoNode {
                tick: 8 * TICKS_PER_BEAT,
                ms: 2000.0,
            },
            TempoNode {
                tick: 12 * TICKS_PER_BEAT,
                ms: 4000.0,
            },
        ];
        let m = TempoMap::new(v, TempoOptions::DEFAULT).unwrap();
        assert!(close(m.tau(1999.0), 1.999));
        assert!(close(m.tau(2000.0), 4.0));
        assert!(close(m.tau(3000.0), 5.0));
    }

    #[test]
    fn beat_phase_pins_tau_zero_to_the_nearest_measure() {
        // tick 0 sits 300 ms into the music (a +300 ms sync offset) at 120 BPM:
        // the nearest measure boundary to music 0 is tick 0 (300 ms < 1 s)
        let v: Vec<TempoNode> = (0..=16)
            .map(|b| TempoNode {
                tick: b * TICKS_PER_BEAT,
                ms: 300.0 + b as f64 * 500.0,
            })
            .collect();
        let m = TempoMap::new(v, TempoOptions::DEFAULT).unwrap();
        assert!((m.anchor_ms() - 300.0).abs() < 1e-9);
        assert!(close(m.tau(300.0), 0.0));
        assert!(close(m.tau(0.0), -0.3));
        assert!(close(m.tau(2300.0), 2.0));
        // tick 0 at 1.5 s → the nearest measure to music 0 is tick −4096 at
        // −0.5 s → τ(0) = +0.5
        let v: Vec<TempoNode> = (0..=16)
            .map(|b| TempoNode {
                tick: b * TICKS_PER_BEAT,
                ms: 1500.0 + b as f64 * 500.0,
            })
            .collect();
        let m = TempoMap::new(v, TempoOptions::DEFAULT).unwrap();
        assert!((m.anchor_ms() - -500.0).abs() < 1e-9);
        assert!(close(m.tau(0.0), 0.5));
        assert!(close(m.tau(1500.0), 2.0));
    }

    #[test]
    fn ssq_pairs_normalize_like_the_game() {
        // TPS 150: 94 ticks → 626.67 ms → round(627.17) = 627
        let n = nodes_from_ssq_pairs(&[(0, 0), (4096, 94)], 150);
        assert_eq!(n[1].tick, 4096);
        assert_eq!(n[1].ms, 627.0);
        let n = nodes_from_ssq_pairs(&[(0, 0), (4096, 2000)], 1000);
        assert_eq!(n[1].ms, 2001.0);
        assert!(nodes_from_ssq_pairs(&[(0, 0)], 0).is_empty());
    }

    #[test]
    fn map_rejects_short_or_unsorted_input() {
        assert!(
            TempoMap::new(vec![TempoNode { tick: 0, ms: 0.0 }], TempoOptions::DEFAULT).is_none()
        );
        assert!(TempoMap::new(
            vec![
                TempoNode { tick: 0, ms: 0.0 },
                TempoNode {
                    tick: 1024,
                    ms: -1.0
                }
            ],
            TempoOptions::DEFAULT
        )
        .is_none());
    }

    #[test]
    fn snap_to_beats_floors_to_half_seconds() {
        assert_eq!(snap_to_beats(19.67), 19.5);
        assert_eq!(snap_to_beats(20.0), 20.0);
        assert_eq!(snap_to_beats(0.2), 0.5);
        assert_eq!(snap_to_beats(0.5), 0.5);
    }
}
