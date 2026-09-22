//! Dance-cut and camera sequencing — PURE, seed-deterministic functions of
//! song time (design §4.3.3), porting A3's `SceneManageActor` /
//! `CharaActor` / `CameraActor` rules (research `a3-runtime-rules.md` §1, §4):
//!
//! - Every dancer plays its shuffled playlist in lock-step SEGMENTS: segment
//!   `k` plays `playlist_i[k mod n_i]` for every dancer `i` and lasts
//!   `min_i dur(playlist_i[k mod n_i]) − CUT_LEAD` (the most-urgent dancer's
//!   `remaining < 1.5 s` broadcasts the cut to everyone — a hard cut, no
//!   blend; the last 1.5 s of every clip are never shown). Segments chain
//!   from 0 forever.
//! - The camera cycles its shuffled MAIN list on each clip's finish, is
//!   FROZEN (no main switch) while a cut lies within `CAMERA_FREEZE_LEAD`
//!   ahead, cuts to the next `_non` shot AT the dance cut and holds it
//!   `NON_HOLD_BASE + U_k` seconds (`U_k ∈ [0,1)` from the per-song seed ⊕
//!   the segment index), then resumes with the NEXT main clip. No beat gate
//!   (v1 deviation, design §9).
//!
//! Purity: `advance(advance(s, a, b), b, c) == advance(s, a, c)` and
//! `at(t) == advance(initial, 0, t)`, so a rewind or scrub re-simulates from
//! 0 and lands exactly where forward stepping would have.

use super::selection::Rng;

/// A3 `DAT_1802647b8`: cut when `remaining < 1.5 s`.
pub const CUT_LEAD: f32 = 1.5;
/// A3 `DAT_1802624d8`: camera switching frozen when `remaining < 2.0 s`.
pub const CAMERA_FREEZE_LEAD: f32 = 2.0;
/// A sub-`CUT_LEAD` clip cannot stall the chain (design).
pub const MIN_SEGMENT: f32 = 0.05;
/// `_non` hold = `1.0 + U[0,1)` seconds (A3 `FUN_18005aed0`).
pub const NON_HOLD_BASE: f32 = 1.0;
/// Hard cap on segment walks (a NaN/inf `t` or a pathological table must not
/// spin the game thread).
const MAX_SEGMENT_WALK: usize = 200_000;

/// A parsed clip the schedules reason about (name + header facts).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRef {
    pub name: String,
    pub duration_s: f32,
    pub loops: bool,
}

impl ClipRef {
    pub fn new(name: impl Into<String>, duration_s: f32, loops: bool) -> ClipRef {
        ClipRef {
            name: name.into(),
            duration_s,
            loops,
        }
    }
}

/// Where a dancer is at time `t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DancePos {
    /// Index into that dancer's playlist (`segment mod n_i`).
    pub clip: usize,
    /// Seconds into the clip (`t − segment_start`; negative before the edge).
    pub local_t: f32,
    pub segment: usize,
    pub segment_start: f32,
    pub segment_end: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DanceSchedule {
    per_dancer: Vec<Vec<ClipRef>>,
    /// Segment lengths are snapped DOWN to a multiple of this (BPM-sync
    /// mode: one beat of dance time, so every cut lands on a beat); `None`
    /// = unsnapped (A3).
    quantum: Option<f32>,
}

impl DanceSchedule {
    /// `None` when there is no dancer or any dancer has an empty playlist.
    pub fn new(per_dancer: Vec<Vec<ClipRef>>) -> Option<DanceSchedule> {
        if per_dancer.is_empty() || per_dancer.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(DanceSchedule {
            per_dancer,
            quantum: None,
        })
    }

    /// Snap every segment length down to whole multiples of `q` (≥ `q`).
    pub fn with_quantum(mut self, q: f32) -> DanceSchedule {
        self.quantum = if q > 0.0 && q.is_finite() {
            Some(q)
        } else {
            None
        };
        self
    }

    pub fn quantum(&self) -> Option<f32> {
        self.quantum
    }

    pub fn dancer_count(&self) -> usize {
        self.per_dancer.len()
    }

    pub fn playlist(&self, dancer: usize) -> &[ClipRef] {
        self.per_dancer
            .get(dancer)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The clip dancer `i` plays in segment `k`.
    pub fn clip_index(&self, dancer: usize, k: usize) -> usize {
        let n = self.per_dancer[dancer].len();
        k % n
    }

    /// `max(MIN_SEGMENT, min_i dur_i(k) − CUT_LEAD)`, snapped down to the
    /// quantum when one is set.
    pub fn segment_len(&self, k: usize) -> f32 {
        let mut shortest = f32::INFINITY;
        for (i, pl) in self.per_dancer.iter().enumerate() {
            let d = pl[self.clip_index(i, k)].duration_s;
            if d < shortest {
                shortest = d;
            }
        }
        let len = shortest - CUT_LEAD;
        let len = if !(len > MIN_SEGMENT) {
            MIN_SEGMENT
        } else {
            len
        };
        match self.quantum {
            Some(q) => ((len / q).floor().max(1.0)) * q,
            None => len,
        }
    }

    /// `(segment index, start, end)` of the segment containing `t`
    /// (segment 0 for `t < 0` / non-finite `t`).
    pub fn segment_at(&self, t: f32) -> (usize, f32, f32) {
        let mut k = 0usize;
        let mut start = 0.0f32;
        let mut end = self.segment_len(0);
        if !t.is_finite() || t < 0.0 {
            return (0, start, end);
        }
        let mut walked = 0usize;
        while t >= end && walked < MAX_SEGMENT_WALK {
            k += 1;
            start = end;
            end = start + self.segment_len(k);
            walked += 1;
        }
        (k, start, end)
    }

    pub fn at(&self, dancer: usize, t: f32) -> DancePos {
        let (k, start, end) = self.segment_at(t);
        DancePos {
            clip: self.clip_index(dancer, k),
            local_t: t - start,
            segment: k,
            segment_start: start,
            segment_end: end,
        }
    }

    /// Segment ends (the camera's cut events) strictly below `until`, with
    /// their segment index.
    pub fn cuts_until(&self, until: f32) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        let mut k = 0usize;
        let mut end = self.segment_len(0);
        while end < until && k < MAX_SEGMENT_WALK {
            out.push((k, end));
            k += 1;
            end += self.segment_len(k);
        }
        out
    }

    /// Design §4.3.3 `cut_times`.
    pub fn cut_times(&self, until: f32) -> Vec<f32> {
        self.cuts_until(until).into_iter().map(|(_, c)| c).collect()
    }

    /// The first cut strictly after `t`: `(segment index, cut time)`.
    /// `segment_at` returns the segment with `start <= t < end`, so its end
    /// is the first cut `> t` (a `t` sitting exactly on a cut already
    /// belongs to the next segment).
    pub fn next_cut_after(&self, t: f32) -> (usize, f32) {
        let (k, _, end) = self.segment_at(t);
        (k, end)
    }

    /// The first cut at or after `t`: `(segment index, cut time)` with
    /// `cut >= t` (equal when `t` is exactly a cut instant).
    pub fn cut_at_or_after(&self, t: f32) -> (usize, f32) {
        let (k, start, end) = self.segment_at(t);
        if k > 0 && start == t {
            (k - 1, start)
        } else {
            (k, end)
        }
    }
}

/// A dance schedule with NO dancers behind it — one pseudo-dancer playing
/// one non-looping `period_s` clip forever, so a cut lands every
/// `max(MIN_SEGMENT, period_s − CUT_LEAD)` seconds. Only the cut times are
/// consumed (the camera event loop of a stage-only preview, design §4.6);
/// `director::produce` iterates the parsed dancers, of which there are none.
pub fn synthetic_schedule(period_s: f32) -> Option<DanceSchedule> {
    DanceSchedule::new(vec![vec![ClipRef::new("synthetic", period_s, false)]])
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipSel {
    Main(usize),
    Non(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub clip: ClipSel,
    /// Song time the current clip started (its frame 0).
    pub clip_start: f32,
    /// While in a `_non` shot: when the hold ends and the main list resumes.
    pub hold_until: Option<f32>,
    /// A dance cut is within `CAMERA_FREEZE_LEAD` ahead (main switching off).
    pub frozen: bool,
    /// How many `_non` shots have been shown (next = `non_rotation % non.len()`).
    pub non_rotation: usize,
    /// Position in the main list (the clip shown, or resumed after a hold).
    pub main_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CameraSchedule {
    main: Vec<ClipRef>,
    non: Vec<ClipRef>,
    rng_seed: u64,
}

impl CameraSchedule {
    /// `None` when the main list is empty; an empty `non` list disables the
    /// cut-away shots (the camera stays frozen through the cut, then resumes
    /// its own cycle).
    pub fn new(main: Vec<ClipRef>, non: Vec<ClipRef>, rng_seed: u64) -> Option<CameraSchedule> {
        if main.is_empty() {
            return None;
        }
        Some(CameraSchedule {
            main,
            non,
            rng_seed,
        })
    }

    pub fn main(&self) -> &[ClipRef] {
        &self.main
    }
    pub fn non(&self) -> &[ClipRef] {
        &self.non
    }

    /// The clip a state refers to.
    pub fn clip_of(&self, st: &CameraState) -> &ClipRef {
        match st.clip {
            ClipSel::Main(i) => &self.main[i % self.main.len()],
            ClipSel::Non(i) => {
                if self.non.is_empty() {
                    &self.main[st.main_index % self.main.len()]
                } else {
                    &self.non[i % self.non.len()]
                }
            }
        }
    }

    pub fn initial(&self) -> CameraState {
        CameraState {
            clip: ClipSel::Main(0),
            clip_start: 0.0,
            hold_until: None,
            frozen: false,
            non_rotation: 0,
            main_index: 0,
        }
    }

    /// `U_k ∈ [0,1)` for the cut ending segment `k` — per-song seed ⊕ k.
    pub fn hold_jitter(&self, k: usize) -> f32 {
        Rng::new(self.rng_seed ^ (k as u64 + 1)).next_f32()
    }

    /// Frozen at `t`: some cut `c` with `0 ≤ c − t < CAMERA_FREEZE_LEAD`.
    pub fn frozen_at(&self, t: f32, dance: &DanceSchedule) -> bool {
        if !t.is_finite() {
            return false;
        }
        let (_, c) = dance.cut_at_or_after(t.max(0.0));
        let ahead = c - t;
        (0.0..CAMERA_FREEZE_LEAD).contains(&ahead)
    }

    /// When a main-clip finish at `f` actually takes effect: deferred to the
    /// cut instant when it falls inside the freeze window.
    fn effective_finish(&self, f: f32, dance: &DanceSchedule) -> f32 {
        let (_, c) = dance.cut_at_or_after(f.max(0.0));
        let ahead = c - f;
        if (0.0..CAMERA_FREEZE_LEAD).contains(&ahead) {
            c
        } else {
            f
        }
    }

    /// Apply every event in `(from, to]` in time order.
    pub fn advance(
        &self,
        st: &CameraState,
        from: f32,
        to: f32,
        dance: &DanceSchedule,
    ) -> CameraState {
        let mut s = *st;
        if !(to > from) || !to.is_finite() {
            s.frozen = to.is_finite() && self.frozen_at(to, dance);
            return s;
        }
        let n_main = self.main.len();
        let mut t_cur = from;
        let mut guard = 0usize;
        loop {
            guard += 1;
            if guard > MAX_SEGMENT_WALK {
                break;
            }
            // Candidate events strictly after t_cur, at or before `to`.
            let (cut_k, cut_t) = dance.next_cut_after(t_cur);
            let cut = if cut_t <= to {
                Some((cut_k, cut_t))
            } else {
                None
            };

            let hold_end = match s.hold_until {
                Some(h) if h > t_cur && h <= to => Some(h),
                _ => None,
            };

            let finish = if s.hold_until.is_none() {
                if let ClipSel::Main(i) = s.clip {
                    let f = s.clip_start + self.main[i % n_main].duration_s;
                    let e = self.effective_finish(f, dance);
                    if e > t_cur && e <= to {
                        Some(e)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            // Earliest event; ties: cut before finish (the `_non` takes over
            // the very instant the deferred finish would fire), hold-end
            // before either (it cannot coincide with a cut in practice).
            let mut next: Option<(u8, f32)> = None;
            if let Some(h) = hold_end {
                next = Some((0, h));
            }
            if let Some((_, c)) = cut {
                if next.map_or(true, |(_, t)| c < t) {
                    next = Some((1, c));
                }
            }
            if let Some(f) = finish {
                if next.map_or(true, |(_, t)| f < t) {
                    next = Some((2, f));
                }
            }
            let Some((kind, t_ev)) = next else { break };
            match kind {
                0 => {
                    // hold end → resume with the next main clip
                    s.main_index = (s.main_index + 1) % n_main;
                    s.clip = ClipSel::Main(s.main_index);
                    s.clip_start = t_ev;
                    s.hold_until = None;
                }
                1 => {
                    let (k, c) = cut.unwrap_or((0, t_ev));
                    if !self.non.is_empty() {
                        s.clip = ClipSel::Non(s.non_rotation % self.non.len());
                        s.non_rotation += 1;
                        s.clip_start = c;
                        s.hold_until = Some(c + NON_HOLD_BASE + self.hold_jitter(k));
                    } else if let ClipSel::Main(i) = s.clip {
                        // no cut-aways: a finish deferred by the freeze fires now
                        let f = s.clip_start + self.main[i % n_main].duration_s;
                        if f <= c {
                            s.main_index = (i + 1) % n_main;
                            s.clip = ClipSel::Main(s.main_index);
                            s.clip_start = c;
                        }
                    }
                }
                _ => {
                    if let ClipSel::Main(i) = s.clip {
                        s.main_index = (i + 1) % n_main;
                        s.clip = ClipSel::Main(s.main_index);
                        s.clip_start = t_ev;
                    }
                }
            }
            t_cur = t_ev;
        }
        s.frozen = self.frozen_at(to, dance);
        s
    }

    /// Re-simulate from 0 (rewinds / first frame).
    pub fn at(&self, t: f32, dance: &DanceSchedule) -> CameraState {
        let init = self.initial();
        if !(t > 0.0) {
            let mut s = init;
            s.frozen = t.is_finite() && self.frozen_at(t.max(0.0), dance);
            return s;
        }
        self.advance(&init, 0.0, t, dance)
    }

    /// Seconds into the current camera clip at `t`.
    pub fn local_t(&self, st: &CameraState, t: f32) -> f32 {
        t - st.clip_start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantum_snaps_segment_lengths_down_to_beats() {
        let d = DanceSchedule::new(vec![vec![
            ClipRef::new("a", 21.167, false),
            ClipRef::new("b", 19.767, false),
        ]])
        .unwrap()
        .with_quantum(0.5);
        // 21.167 − 1.5 = 19.667 → 19.5; 19.767 − 1.5 = 18.267 → 18.0
        assert!((d.segment_len(0) - 19.5).abs() < 1e-6);
        assert!((d.segment_len(1) - 18.0).abs() < 1e-6);
        // never below one quantum
        let tiny = DanceSchedule::new(vec![vec![ClipRef::new("t", 1.6, false)]])
            .unwrap()
            .with_quantum(0.5);
        assert!((tiny.segment_len(0) - 0.5).abs() < 1e-6);
        // unsnapped stays exact
        let plain = DanceSchedule::new(vec![vec![ClipRef::new("a", 21.167, false)]]).unwrap();
        assert!((plain.segment_len(0) - 19.667).abs() < 1e-5);
        assert_eq!(plain.quantum(), None);
    }

    fn clips(durs: &[f32]) -> Vec<ClipRef> {
        durs.iter()
            .enumerate()
            .map(|(i, d)| ClipRef::new(format!("c{i}"), *d, false))
            .collect()
    }

    fn two_dancers() -> DanceSchedule {
        DanceSchedule::new(vec![
            clips(&[21.2, 22.0, 19.8]),
            clips(&[20.9, 22.5, 18.9, 21.4]),
        ])
        .unwrap()
    }

    fn cams(seed: u64) -> CameraSchedule {
        CameraSchedule::new(
            (0..6)
                .map(|i| ClipRef::new(format!("st_{i}"), 7.5, false))
                .collect(),
            (0..4)
                .map(|i| ClipRef::new(format!("non_{i}"), 7.5, false))
                .collect(),
            seed,
        )
        .unwrap()
    }

    #[test]
    fn segment_lengths_min_minus_lead_with_wrap() {
        let d = two_dancers();
        // k: A[k%3], B[k%4]
        let exp = [
            (21.2f32).min(20.9) - 1.5, // k0: 20.9−1.5
            (22.0f32).min(22.5) - 1.5, // k1: 22.0
            (19.8f32).min(18.9) - 1.5, // k2: 18.9
            (21.2f32).min(21.4) - 1.5, // k3: 21.2
            (22.0f32).min(20.9) - 1.5, // k4: 20.9
            (19.8f32).min(22.5) - 1.5, // k5: 19.8
        ];
        for (k, e) in exp.iter().enumerate() {
            assert!(
                (d.segment_len(k) - e).abs() < 1e-6,
                "k{k}: {} vs {e}",
                d.segment_len(k)
            );
        }
        // clamp
        let short = DanceSchedule::new(vec![clips(&[1.0]), clips(&[30.0])]).unwrap();
        assert_eq!(short.segment_len(0), MIN_SEGMENT);
        assert_eq!(short.segment_len(7), MIN_SEGMENT);
        // constructor refusals
        assert!(DanceSchedule::new(vec![]).is_none());
        assert!(DanceSchedule::new(vec![clips(&[5.0]), vec![]]).is_none());
    }

    #[test]
    fn dancers_share_cut_instants() {
        let d = two_dancers();
        let mut t = 0.0f32;
        let mut last_seg = usize::MAX;
        let mut segs_seen = 0;
        while t < 300.0 {
            let a = d.at(0, t);
            let b = d.at(1, t);
            assert_eq!(a.segment, b.segment, "t={t}");
            assert_eq!(a.segment_start, b.segment_start);
            assert_eq!(a.segment_end, b.segment_end);
            assert_eq!(a.clip, a.segment % 3);
            assert_eq!(b.clip, b.segment % 4);
            assert!((a.local_t - (t - a.segment_start)).abs() < 1e-6);
            assert!(a.local_t >= 0.0 && a.local_t < a.segment_end - a.segment_start + 1e-6);
            if a.segment != last_seg {
                segs_seen += 1;
                last_seg = a.segment;
            }
            t += 0.25;
        }
        assert!(segs_seen >= 14, "{segs_seen}");
        // cut list equals the segment ends
        let cuts = d.cut_times(100.0);
        assert!((cuts[0] - 19.4).abs() < 1e-5);
        assert!((cuts[1] - (19.4 + 20.5)).abs() < 1e-4);
        assert!(cuts.iter().all(|c| *c < 100.0));
        assert_eq!(cuts.len(), d.cuts_until(100.0).len());
        // pre-edge / non-finite
        let pre = d.at(0, -3.0);
        assert_eq!(pre.segment, 0);
        assert!((pre.local_t + 3.0).abs() < 1e-6);
        assert_eq!(d.at(1, f32::NAN).segment, 0);
        // next_cut_after at exactly a cut instant returns the following one
        let (k0, c0) = d.next_cut_after(0.0);
        assert_eq!(k0, 0);
        let (k1, c1) = d.next_cut_after(c0);
        assert_eq!(k1, 1);
        assert!(c1 > c0);
    }

    #[test]
    fn camera_timeline_follows_a3_rules() {
        let d = two_dancers();
        let cam = cams(0xABCD);
        let c0 = d.cut_times(100.0)[0]; // 19.4
                                        // main index advances on finish while not frozen
        let s = cam.at(7.4, &d);
        assert_eq!(s.clip, ClipSel::Main(0));
        assert!(!s.frozen);
        let s = cam.at(7.5, &d);
        assert_eq!(s.clip, ClipSel::Main(1));
        assert_eq!(s.clip_start, 7.5);
        let s = cam.at(15.0, &d);
        assert_eq!(s.clip, ClipSel::Main(2));
        assert_eq!(s.clip_start, 15.0);
        // frozen from cut − 2.0 (exclusive) up to the cut
        assert!(!cam.at(c0 - 2.0, &d).frozen);
        assert!(cam.at(c0 - 1.99, &d).frozen);
        assert!(cam.at(c0 - 0.01, &d).frozen);
        // still Main(2) right before the cut (its finish at 22.5 is after the cut anyway)
        assert_eq!(cam.at(c0 - 0.01, &d).clip, ClipSel::Main(2));
        // at the cut: _non shot 0 starts exactly at the cut
        let s = cam.at(c0, &d);
        assert_eq!(s.clip, ClipSel::Non(0));
        assert_eq!(s.clip_start, c0);
        assert_eq!(s.non_rotation, 1);
        let hold = s.hold_until.unwrap();
        assert!(hold >= c0 + 1.0 && hold < c0 + 2.0, "hold {hold}");
        assert!((hold - (c0 + 1.0 + cam.hold_jitter(0))).abs() < 1e-6);
        // still in the shot just before the hold ends
        let s = cam.at(hold - 0.01, &d);
        assert_eq!(s.clip, ClipSel::Non(0));
        // resume with the NEXT main clip at the hold end
        let s = cam.at(hold, &d);
        assert_eq!(s.clip, ClipSel::Main(3));
        assert_eq!(s.clip_start, hold);
        assert_eq!(s.hold_until, None);
        assert_eq!(s.main_index, 3);
        // second cut rotates the _non list
        let c1 = d.cut_times(100.0)[1];
        let s = cam.at(c1, &d);
        assert_eq!(s.clip, ClipSel::Non(1));
        assert_eq!(s.non_rotation, 2);
        // rotation wraps modulo the list
        let c4 = d.cut_times(200.0)[4];
        assert_eq!(cam.at(c4, &d).clip, ClipSel::Non(0));
        assert_eq!(cam.at(c4, &d).non_rotation, 5);
        // seeds change the hold, never the cut instants
        let cam2 = cams(0x1234);
        assert_eq!(cam2.at(c0, &d).clip_start, c0);
        assert_ne!(cam2.at(c0, &d).hold_until, cam.at(c0, &d).hold_until);
        // clip lookup
        assert_eq!(cam.clip_of(&cam.at(c0, &d)).name, "non_0");
        assert_eq!(cam.clip_of(&cam.at(7.5, &d)).name, "st_1");
        assert!((cam.local_t(&cam.at(9.0, &d), 9.0) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn deferred_finish_inside_the_freeze_window() {
        // Main clips of 6.0 s: finishes at 6, 12, 18 — 18.0 lies inside
        // (17.4, 19.4] → deferred to the cut; with a _non list the cut takes
        // over at 19.4 and the resume picks Main(next).
        let d = two_dancers();
        let c0 = d.cut_times(100.0)[0];
        let main: Vec<ClipRef> = (0..3)
            .map(|i| ClipRef::new(format!("m{i}"), 6.0, false))
            .collect();
        let non = vec![ClipRef::new("n0", 7.5, false)];
        let cam = CameraSchedule::new(main.clone(), non, 1).unwrap();
        assert_eq!(cam.at(12.0, &d).clip, ClipSel::Main(2));
        assert_eq!(
            cam.at(18.0, &d).clip,
            ClipSel::Main(2),
            "finish at 18.0 is frozen"
        );
        assert_eq!(cam.at(18.5, &d).clip, ClipSel::Main(2));
        let s = cam.at(c0, &d);
        assert_eq!(s.clip, ClipSel::Non(0));
        let h = s.hold_until.unwrap();
        assert_eq!(
            cam.at(h, &d).clip,
            ClipSel::Main(0),
            "resume = next after Main(2) wraps to 0"
        );

        // Without cut-aways the deferred finish fires AT the cut instant.
        let cam0 = CameraSchedule::new(main, vec![], 1).unwrap();
        assert_eq!(cam0.at(18.5, &d).clip, ClipSel::Main(2));
        assert!(cam0.at(18.5, &d).frozen);
        let s = cam0.at(c0, &d);
        assert_eq!(s.clip, ClipSel::Main(0));
        assert_eq!(s.clip_start, c0);
        assert_eq!(s.hold_until, None);
        // then its own cycle: next finish at c0 + 6
        assert_eq!(cam0.at(c0 + 5.99, &d).clip, ClipSel::Main(0));
        assert_eq!(cam0.at(c0 + 6.0, &d).clip, ClipSel::Main(1));
        // a finish AFTER the cut but outside any freeze is not deferred
        let cam_long = CameraSchedule::new(
            vec![
                ClipRef::new("m", 20.0, false),
                ClipRef::new("m2", 20.0, false),
            ],
            vec![],
            1,
        )
        .unwrap();
        assert_eq!(cam_long.at(19.99, &d).clip, ClipSel::Main(0));
        assert_eq!(cam_long.at(20.0, &d).clip, ClipSel::Main(1));
    }

    #[test]
    fn at_equals_incremental_stepping_and_rewind() {
        let d = two_dancers();
        for seed in [1u64, 0xDEAD_BEEF, 77] {
            let cam = cams(seed);
            let mut s = cam.initial();
            let mut t = 0.0f32;
            let dt = 1.0 / 60.0;
            let mut n = 0;
            while t < 200.0 {
                let t2 = t + dt;
                s = cam.advance(&s, t, t2, &d);
                let direct = cam.at(t2, &d);
                assert_eq!(s, direct, "seed {seed} t {t2}");
                // dance purity is trivial (stateless) — spot-check monotone segments
                let a = d.at(0, t2);
                assert!(a.segment_start <= t2 && t2 < a.segment_end);
                t = t2;
                n += 1;
            }
            assert!(n > 11_000);
            // coarse steps land on the same states
            let mut s2 = cam.initial();
            let mut t = 0.0f32;
            while t < 200.0 {
                let t2 = (t + 3.3).min(200.0);
                s2 = cam.advance(&s2, t, t2, &d);
                assert_eq!(s2, cam.at(t2, &d));
                t = t2;
            }
            // rewind: re-simulating to an earlier time equals the forward path
            let fwd = cam.at(45.0, &d);
            let _late = cam.at(150.0, &d);
            assert_eq!(cam.at(45.0, &d), fwd);
            // stepping backwards through advance is a no-op (events only in (from, to])
            let back = cam.advance(&fwd, 45.0, 30.0, &d);
            assert_eq!(back.clip, fwd.clip);
        }
    }

    #[test]
    fn empty_lists_and_short_segments() {
        assert!(CameraSchedule::new(vec![], vec![], 1).is_none());
        // segments shorter than the hold: a new cut while holding re-cuts
        let d = DanceSchedule::new(vec![clips(&[2.0]), clips(&[2.0])]).unwrap();
        assert_eq!(d.segment_len(0), 0.5);
        let cam = cams(3);
        let s = cam.at(0.5, &d);
        assert_eq!(s.clip, ClipSel::Non(0));
        let s = cam.at(1.0, &d);
        assert_eq!(s.clip, ClipSel::Non(1), "second cut re-cuts while holding");
        assert_eq!(s.non_rotation, 2);
        assert!(s.hold_until.unwrap() >= 2.0);
        // still pure
        let mut st = cam.initial();
        let mut t = 0.0;
        while t < 10.0 {
            st = cam.advance(&st, t, t + 0.1, &d);
            assert_eq!(st, cam.at(t + 0.1, &d));
            t += 0.1;
        }
    }

    /// The plan's Step 6 demo: `cargo test -- --nocapture print_example_timeline`.
    #[test]
    fn print_example_timeline() {
        let d = two_dancers();
        let cam = cams(0x5EED);
        println!("segment | start   | end     | A clip | B clip");
        for k in 0..8 {
            let a = d.at(0, d.cuts_until(1e9)[k].1 - 0.01);
            println!(
                "{k:>7} | {:>7.2} | {:>7.2} | {:>6} | {:>6}",
                a.segment_start,
                a.segment_end,
                d.playlist(0)[a.clip].name,
                d.playlist(1)[d.at(1, a.segment_start).clip].name
            );
        }
        println!("camera events (first 60 s):");
        let mut prev = cam.initial();
        let mut t = 0.0f32;
        while t < 60.0 {
            let s = cam.advance(&prev, t, t + 1.0 / 60.0, &d);
            if s.clip != prev.clip || s.frozen != prev.frozen {
                println!(
                    "  t={:>6.2}  {:?} start={:.2} hold_until={:?} frozen={}",
                    t + 1.0 / 60.0,
                    s.clip,
                    s.clip_start,
                    s.hold_until,
                    s.frozen
                );
            }
            prev = s;
            t += 1.0 / 60.0;
        }
    }

    #[test]
    fn synthetic_schedule_cuts_every_period_minus_lead() {
        let s = synthetic_schedule(9.0).expect("one pseudo-dancer");
        assert_eq!(s.dancer_count(), 1);
        let cuts = s.cut_times(23.0);
        assert_eq!(cuts.len(), 3);
        for (c, want) in cuts.iter().zip([7.5f32, 15.0, 22.5]) {
            assert!((c - want).abs() < 1e-4, "{cuts:?}");
        }
        assert_eq!(s.at(0, 8.0).segment, 1);
        assert_eq!(s.at(0, 8.0).clip, 0);
        // A degenerate period still yields MIN_SEGMENT segments (no stall).
        let z = synthetic_schedule(0.0).unwrap();
        assert_eq!(z.segment_len(0), MIN_SEGMENT);
        assert_eq!(z.segment_len(7), MIN_SEGMENT);
    }
}
