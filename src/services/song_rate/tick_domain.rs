//! Pure content→wall domain algebra for the assist-tick track (design
//! req 30 of the song-rate streaming redesign).
//!
//! The consumer (`src/mods/assist_tick.rs`) is not host-mounted; this module
//! is — the same split that put `RateSnapshot::is_non_identity_commit()` in
//! `clock_patch` (Step 4 task-04). No logging, no allocation beyond the
//! output vector, callable from any thread.
//!
//! ## Domain algebra
//!
//! Under a committed song rate the Q31 clock stub outputs CONTENT-domain
//! music counts, so the anchor `m0`, the commit-time `mc`, and the per-side
//! JUDGMENT TIMING (the game applies `timing_music` against that clock) are
//! all content-domain terms — they convert. The audible tick track plays in
//! WALL time, and the cabinet `sound_offset` is the real audio chain's
//! latency — wall-domain, applied UNSCALED after the conversion:
//!
//! ```text
//! wall_pos(t) = content_to_wall_ms(t + jt − m0) − sound_offset
//! skip        = content_to_wall_ms(mc − m0)
//! ```
//!
//! Identity and uncommitted snapshots take the LITERALLY unchanged legacy
//! arithmetic (design req 30's 100 % pin: bit-identical output). The
//! identity ratio's `content_to_wall_ms` is an exact 1:1, but the legacy
//! path is kept verbatim so the pin is structural, not arithmetic.

use super::clock_patch::RateSnapshot;

/// Each tick's mix position in the track, in track (wall) milliseconds.
///
/// `judgment_timing` arrives SIGN-APPLIED — the mod owns its
/// `JUDGMENT_TIMING_SIGN` constant and passes the product, exactly as the
/// legacy shift computation multiplied it in place.
#[must_use]
pub fn tick_track_positions(
    times: &[i32],
    judgment_timing: i32,
    sound_offset: i32,
    m0: i32,
    snapshot: &RateSnapshot,
) -> Vec<i32> {
    if !snapshot.is_non_identity_commit() {
        return times
            .iter()
            .map(|&t| legacy_position(t, judgment_timing, sound_offset, m0))
            .collect();
    }
    times
        .iter()
        .map(|&t| {
            let content = i64::from(t) + i64::from(judgment_timing) - i64::from(m0);
            match snapshot.effective_rate.content_to_wall_ms(content) {
                Ok(wall) => clamp_i32(wall - i64::from(sound_offset)),
                // Can't-happen behind the seqlock (published ratios are
                // validated at exposure); deterministic identity fallback —
                // never a panic on the judge path.
                Err(_) => legacy_position(t, judgment_timing, sound_offset, m0),
            }
        })
        .collect()
}

/// The commit/rewind shift (`mc − m0`) in track (wall) milliseconds. May be
/// negative (a rewind past the anchor); the caller's `.max(0)` guard is
/// downstream, unchanged.
#[must_use]
pub fn restart_skip_ms(music_count: i32, m0: i32, snapshot: &RateSnapshot) -> i32 {
    if !snapshot.is_non_identity_commit() {
        return music_count.saturating_sub(m0);
    }
    let content = i64::from(music_count) - i64::from(m0);
    match snapshot.effective_rate.content_to_wall_ms(content) {
        Ok(wall) => clamp_i32(wall),
        Err(_) => music_count.saturating_sub(m0),
    }
}

/// The literal legacy arithmetic (the mod's pre-conversion formula),
/// reproduced bit-identically: shift computed with the same saturating call
/// order, then one saturating add.
fn legacy_position(t: i32, judgment_timing: i32, sound_offset: i32, m0: i32) -> i32 {
    let shift = judgment_timing
        .saturating_sub(sound_offset)
        .saturating_sub(m0);
    t.saturating_add(shift)
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
