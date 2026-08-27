//! Pure replay arithmetic for the ultrafast-boot cache
//! (design §Data Models → Replay write set).
//!
//! [`compute_slot`] transcribes, 1:1, the arithmetic
//! `CheckStepDataActor::onUpdate` performs after each Analyze call — the
//! per-slot music-DB values and flags plus the song-wide u16 BPM
//! accumulator contributions — from one cached [`SlotPayload`].
//! [`fold_radar`] reproduces the actor's five radar accumulator updates
//! (with the `sota.ssq`/`thr8.ssq` special cases decided by the caller).
//!
//! Exactness notes (from the decompiled onUpdate,
//! `docs/ultrafast_boot_research.md` §3.8/§5.3):
//! - The BPM doubles were already rounded by the game at Analyze exit; the
//!   DB stores them C-cast to int (truncation toward zero).
//! - The song-wide u16 min/max accumulate SKIPS zero values (`if != 0`).
//! - The variable-BPM flag compares `|max − min| > threshold` strictly.
//! - The corruption trigger sums result[0]+result[2] (steps + shocks) —
//!   NOT freezes. The replay writes only the flag byte; the game's error
//!   reporter is never invoked from replay (design FR-3).
//!
//! Dependency-free and unsafe-free on purpose — host-tested via
//! `scripts/validate_fast_bootup.sh`. The unsafe applier that writes these
//! into game memory is a later plan step.

// `super::cache` resolves in both mount contexts: the real crate
// (`mods::fast_bootup::cache`) and the validation harness (top-level
// `cache`, where `super` is the crate root).
use super::cache::SlotPayload;

/// Everything onUpdate derives from one (difficulty, mode) analysis, ready
/// for the applier to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotWrites {
    /// `entry+0x98 + idx*4` — max BPM as int.
    pub max_bpm: i32,
    /// `entry+0xC0 + idx*4` — core BPM as int.
    pub core_bpm: i32,
    /// `entry+0xE8 + idx*4` — min BPM as int.
    pub min_bpm: i32,
    /// `entry+0x11A + idx` — has shock arrows.
    pub shock: bool,
    /// `entry+0x124 + idx` — BPM varies beyond the game's threshold.
    pub variable_bpm: bool,
    /// `entry+0x12E + idx` — result[4] > 0 (semantics unresolved; replayed
    /// verbatim).
    pub flag_12e: bool,
    /// `entry+0x1B4 + idx*4` — EX score = (steps + freezes + shocks) × 3.
    pub ex_score: i32,
    /// `entry+0x1B0` — per-song corruption flag contribution (sticky OR
    /// across slots; flag byte only, never the game's reporter).
    pub corrupt: bool,
    /// `entry+0x94` max-accumulate contribution; `None` = skip (zero rule).
    pub song_max_bpm: Option<u16>,
    /// `entry+0x96` min-accumulate contribution; `None` = skip (zero rule).
    pub song_min_bpm: Option<u16>,
}

/// Reconstruct an f64 stored in the result block as two little-endian i32s.
fn f64_from_pair(lo: i32, hi: i32) -> f64 {
    f64::from_bits((lo as u32 as u64) | ((hi as u32 as u64) << 32))
}

/// C-style double→int conversion as the game performs it (`cvttsd2si` —
/// truncation toward zero). Values are game-rounded BPMs (small, positive),
/// so Rust's saturating `as` matches on the whole realistic domain.
fn as_int(v: f64) -> i32 {
    v as i32
}

/// Transcribe onUpdate's post-Analyze write set for one slot.
///
/// * `has_chart` — the music-DB entry's own "chart exists at (mode,
///   difficulty)" answer (vfunc +0x70), evaluated LIVE at replay time so
///   musicdb edits keep stock semantics.
/// * `threshold` — the game's variable-BPM threshold global
///   (`DAT_180393F40`), read at replay time.
pub fn compute_slot(payload: &SlotPayload, has_chart: bool, threshold: f64) -> SlotWrites {
    let steps = payload.result[0];
    let freezes = payload.result[1];
    let shocks = payload.result[2];

    let min_f = f64_from_pair(payload.result[8], payload.result[9]);
    let core_f = f64_from_pair(payload.result[10], payload.result[11]);
    let max_f = f64_from_pair(payload.result[12], payload.result[13]);

    let min_bpm = as_int(min_f);
    let core_bpm = as_int(core_f);
    let max_bpm = as_int(max_f);

    SlotWrites {
        max_bpm,
        core_bpm,
        min_bpm,
        shock: shocks > 0,
        variable_bpm: (max_f - min_f).abs() > threshold,
        flag_12e: payload.result[4] > 0,
        ex_score: (steps + freezes + shocks) * 3,
        corrupt: has_chart && (payload.ret == 0 || steps + shocks == 0),
        song_max_bpm: (max_bpm != 0).then_some(max_bpm as u16),
        song_min_bpm: (min_bpm != 0).then_some(min_bpm as u16),
    }
}

/// Which hardcoded filename special case applies to a payload's file
/// (decided by the caller from the cached game path's filename).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialFile {
    None,
    /// `sota.ssq` — the only file whose radar[0] feeds the actor's +0xA8
    /// accumulator.
    Sota,
    /// `thr8.ssq` — the only file whose radar[1] feeds +0xAC.
    Thr8,
}

/// Fold one payload's radar block into the actor-accumulator image
/// (indices 0..4 ↔ actor +0xA8/+0xAC/+0xB0/+0xB4/+0xB8). Indices 2..4
/// always max-accumulate; 0 and 1 only under their matching special file.
pub fn fold_radar(acc: &mut [i32; 5], radar: &[i32; 5], special: SpecialFile) {
    if special == SpecialFile::Sota {
        acc[0] = acc[0].max(radar[0]);
    }
    if special == SpecialFile::Thr8 {
        acc[1] = acc[1].max(radar[1]);
    }
    for i in 2..5 {
        acc[i] = acc[i].max(radar[i]);
    }
}

/// Classify a game path's filename for [`fold_radar`].
pub fn special_file(game_path: &str) -> SpecialFile {
    match game_path.rsplit('/').next() {
        Some("sota.ssq") => SpecialFile::Sota,
        Some("thr8.ssq") => SpecialFile::Thr8,
        _ => SpecialFile::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The game's stock threshold on current builds is a small double; the
    /// exact value is read at runtime. Tests use a stand-in.
    const THRESHOLD: f64 = 10.0;

    fn payload(
        steps: i32,
        freezes: i32,
        shocks: i32,
        ret: u8,
        min: f64,
        core: f64,
        max: f64,
    ) -> SlotPayload {
        let mut result = [0i32; 14];
        result[0] = steps;
        result[1] = freezes;
        result[2] = shocks;
        let put = |r: &mut [i32; 14], at: usize, v: f64| {
            let bits = v.to_bits();
            r[at] = (bits & 0xFFFF_FFFF) as u32 as i32;
            r[at + 1] = (bits >> 32) as u32 as i32;
        };
        put(&mut result, 8, min);
        put(&mut result, 10, core);
        put(&mut result, 12, max);
        SlotPayload {
            difficulty: 3,
            mode: 0,
            ret,
            result,
            radar: [0; 5],
        }
    }

    #[test]
    fn normal_chart() {
        let w = compute_slot(&payload(250, 12, 3, 1, 65.0, 175.0, 180.0), true, THRESHOLD);
        assert_eq!(w.min_bpm, 65);
        assert_eq!(w.core_bpm, 175);
        assert_eq!(w.max_bpm, 180);
        assert!(w.shock);
        assert!(w.variable_bpm); // |180-65| > 10
        assert!(!w.flag_12e);
        assert_eq!(w.ex_score, (250 + 12 + 3) * 3);
        assert!(!w.corrupt);
        assert_eq!(w.song_max_bpm, Some(180));
        assert_eq!(w.song_min_bpm, Some(65));
    }

    #[test]
    fn zeroed_failed_payload() {
        let w = compute_slot(&payload(0, 0, 0, 0, 0.0, 0.0, 0.0), false, THRESHOLD);
        assert_eq!((w.min_bpm, w.core_bpm, w.max_bpm), (0, 0, 0));
        assert!(!w.shock && !w.variable_bpm && !w.flag_12e);
        assert_eq!(w.ex_score, 0);
        assert!(!w.corrupt); // no chart expected → no corruption
        assert_eq!(w.song_max_bpm, None); // skip-zero rule
        assert_eq!(w.song_min_bpm, None);
    }

    #[test]
    fn corruption_truth_table() {
        // (has_chart, ret, steps, shocks, freezes) → corrupt
        let cases = [
            (true, 0u8, 100, 0, 0, true),   // chart expected, parse failed
            (true, 1u8, 0, 0, 0, true),     // parsed but no steps/shocks
            (true, 1u8, 0, 0, 50, true),    // freezes alone do NOT count
            (true, 1u8, 0, 1, 0, false),    // shocks alone suffice
            (true, 1u8, 1, 0, 0, false),    // steps alone suffice
            (false, 0u8, 0, 0, 0, false),   // no chart expected
            (false, 1u8, 100, 0, 0, false), // fine either way
        ];
        for (has_chart, ret, steps, shocks, freezes, want) in cases {
            let w = compute_slot(
                &payload(steps, freezes, shocks, ret, 100.0, 100.0, 100.0),
                has_chart,
                THRESHOLD,
            );
            assert_eq!(
                w.corrupt, want,
                "has_chart={has_chart} ret={ret} steps={steps} shocks={shocks} freezes={freezes}"
            );
        }
    }

    #[test]
    fn variable_bpm_strictly_greater() {
        let at = compute_slot(
            &payload(1, 0, 0, 1, 100.0, 100.0, 110.0),
            true,
            10.0, // |110-100| == threshold → NOT variable
        );
        assert!(!at.variable_bpm);
        let above = compute_slot(&payload(1, 0, 0, 1, 100.0, 100.0, 110.5), true, 10.0);
        assert!(above.variable_bpm);
        let below = compute_slot(&payload(1, 0, 0, 1, 100.0, 100.0, 105.0), true, 10.0);
        assert!(!below.variable_bpm);
    }

    #[test]
    fn truncation_toward_zero() {
        let w = compute_slot(
            &payload(1, 0, 0, 1, 65.9, 200.0, 400.5),
            true,
            f64::INFINITY,
        );
        assert_eq!(w.min_bpm, 65);
        assert_eq!(w.max_bpm, 400);
        assert_eq!(w.song_min_bpm, Some(65));
        assert_eq!(w.song_max_bpm, Some(400));
    }

    #[test]
    fn flag_12e_from_result4() {
        let mut p = payload(1, 0, 0, 1, 100.0, 100.0, 100.0);
        p.result[4] = 7;
        assert!(compute_slot(&p, true, THRESHOLD).flag_12e);
        p.result[4] = 0;
        assert!(!compute_slot(&p, true, THRESHOLD).flag_12e);
    }

    #[test]
    fn radar_fold_specials() {
        let mut acc = [0i32; 5];
        fold_radar(&mut acc, &[100, 200, 30, 40, 50], SpecialFile::None);
        assert_eq!(acc, [0, 0, 30, 40, 50]); // 0/1 untouched without special

        fold_radar(&mut acc, &[100, 200, 10, 10, 10], SpecialFile::Sota);
        assert_eq!(acc, [100, 0, 30, 40, 50]); // sota feeds [0] only

        fold_radar(&mut acc, &[500, 200, 60, 10, 10], SpecialFile::Thr8);
        assert_eq!(acc, [100, 200, 60, 40, 50]); // thr8 feeds [1]; [2] grew

        assert_eq!(special_file("data/mdb_apx/ssq/sota.ssq"), SpecialFile::Sota);
        assert_eq!(special_file("data/mdb_apx/ssq/thr8.ssq"), SpecialFile::Thr8);
        assert_eq!(special_file("data/mdb_apx/ssq/puty.ssq"), SpecialFile::None);
    }
}
