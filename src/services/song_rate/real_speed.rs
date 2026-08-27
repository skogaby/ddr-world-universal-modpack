//! Real Speed × effective rate (song-rate streaming design req 33).
//!
//! At a committed non-identity song rate, the audible tempo is
//! `Core BPM × effective_rate`, so a player's REAL SPEED target (speed
//! type 0: "arrows travel at N regardless of the song") must derive its
//! normalized multiplier from that effective tempo — independent of the
//! Real Speed Fix mod's toggle, which only redirects the NATIVE setter's
//! divisor (Max→Core BPM) and never sees the rate. At 100 % nothing here
//! runs and the toggle keeps its stock-vs-fix meaning bit-identically.
//!
//! Mechanism (the predecessor design's §Real Speed Integration, imported as
//! a KEEPER; RE record in this task's context.md): no competing patch —
//! the native derivation chain is left intact, and the rate-adjusted value
//! is written over its OUTPUTS once per side per song at the first judge
//! dispatch (strictly after any loader-thread commit, after the gameplay
//! actor latched its own copy). The consumer chain, verified on 20260721
//! (+ 20260616 byte spot-check):
//!
//! - `ddr::player::Option` (per side, `*(ctx)+0xE0` — the assist-tick
//!   chain): speed TYPE @+0x8 (0 = Real Speed, 1 = fixed multiplier),
//!   fixed multiplier @+0xC, DERIVED multiplier ×100 @+0x10 (the native
//!   `SetScrollSpeed` output: `clamp(trunc(target·100/BPM), 25, 800)`),
//!   target @+0x14, Min/Core/Max BPM doubles @+0x80/+0x88/+0x90.
//! - The GamePlayActor latches the active multiplier at construction into
//!   `+0x29C` (int) and `+0x290`/`+0x294` (f32 /100), then re-writes the
//!   arrow/spot renderers from those floats EVERY frame — so the actor
//!   cluster is the effective write target; `Option+0x10` is also written
//!   for display consistency.
//!
//! The pure derivation below is host-tested; the `#[cfg(windows)]` glue is
//! a judge_hook subscriber + scene reset owned by the Song Playback Speed
//! mod (the rate feature) — deliberately NOT by the Real Speed Fix mod,
//! whose enable state must not gate this (req 33's "regardless of the
//! toggle"). Every failure leg skips the write: stock behavior (fail-open).

use super::clock_patch::RateSnapshot;

/// Upper bound of the trusted Core-BPM domain. The game's charts top out in
/// the hundreds; anything past this means the chain read garbage.
const CORE_BPM_LIMIT: f64 = 10_000.0;
/// Trusted real-speed target domain (the native setter accepts any int; a
/// zero/negative or absurd value here means an unset field or a misread —
/// skipping the write is the conservative direction).
const TARGET_LIMIT: i32 = 100_000;
/// The native multiplier clamp (DAT_18035a740/744 = 25/800, read from the
/// image: ×0.25 to ×8.00).
const MULTIPLIER_MIN: i32 = 25;
const MULTIPLIER_MAX: i32 = 800;

/// The rate-adjusted normalized multiplier (×100), or `None` when no write
/// must happen (identity/uncommitted snapshot, or an untrusted input —
/// fail-soft to stock behavior).
///
/// Replicates the native derivation byte-faithfully with the effective
/// divisor: `trunc((target·100) / (core_bpm × source/output))`, clamped to
/// the native [25, 800]. f64 division + truncating cast is exactly what the
/// stock setter compiles to (`(int)((double)(target*100) / divisor)`); the
/// Max-BPM cap sentinel belongs to the stock Max path and does not apply to
/// the Core path (same semantics the shipped R24 patch encodes). The Real
/// Speed Fix toggle is structurally absent: the derivation is a pure
/// function of its three inputs.
#[must_use]
pub fn rate_adjusted_multiplier(
    target_real_speed: i32,
    core_bpm: f64,
    snapshot: &RateSnapshot,
) -> Option<i32> {
    if !snapshot.is_non_identity_commit() {
        return None;
    }
    if !(target_real_speed > 0 && target_real_speed <= TARGET_LIMIT) {
        return None;
    }
    if !(core_bpm.is_finite() && core_bpm > 0.0 && core_bpm <= CORE_BPM_LIMIT) {
        return None;
    }
    let rate = snapshot.effective_rate;
    if rate.source_frames == 0 || rate.output_frames == 0 {
        return None;
    }
    let effective_bpm = core_bpm * (rate.source_frames as f64 / rate.output_frames as f64);
    if !(effective_bpm.is_finite() && effective_bpm > 0.0) {
        return None;
    }
    let multiplier = (f64::from(target_real_speed) * 100.0 / effective_bpm) as i32;
    Some(multiplier.clamp(MULTIPLIER_MIN, MULTIPLIER_MAX))
}

// ── Windows glue: the once-per-side-per-song write ───────────────────

#[cfg(windows)]
mod glue {
    use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
    use std::sync::Mutex;

    use super::rate_adjusted_multiplier;
    use crate::core::signatures::SignatureStore;
    use crate::services::judge_hook::{self, CallbackHandle, Priority};
    use crate::services::scene_manager;
    use crate::services::song_rate::clock_patch;
    use crate::types::scenes::scene;
    use crate::{log_info, log_warn};

    /// Offset of the play-side enum on the gameplay actor (the documented
    /// constant assist_tick/autoplay use).
    const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;
    /// The actor's multiplier cluster (RE record: context.md; the actor
    /// latches these at construction and re-writes the renderers from the
    /// two floats every frame). `+0x290` current, `+0x294` lerp target
    /// (both f32, multiplier/100), `+0x29C` the int ×100 copy.
    const ACTOR_SPEED_CURRENT: usize = 0x290;
    const ACTOR_SPEED_TARGET: usize = 0x294;
    const ACTOR_SPEED_INT: usize = 0x29C;
    /// Offset of the embedded `ddr::player::Option` within a side's context
    /// object (`*(table[side]) + 0xE0` — the assist-tick chain).
    const CTX_OPTION_OFFSET: usize = 0xE0;
    /// `ddr::player::Option` fields (verified 20260721; layout stable since
    /// 20250805 per the bulk-hack RE record).
    const OPTION_SPEED_TYPE: usize = 0x8;
    const OPTION_DERIVED_MULTIPLIER: usize = 0x10;
    const OPTION_TARGET: usize = 0x14;
    const OPTION_CORE_BPM: usize = 0x88;
    /// Speed-type value meaning "Real Speed mode" (target-derived). Fixed
    /// multiplier mode (1) has no BPM derivation and is deliberately
    /// untouched.
    const SPEED_TYPE_REAL: i32 = 0;

    /// Per-side once-per-song latch, armed at GAMEPLAY entry and consumed
    /// by that side's first judge dispatch.
    static PENDING: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
    /// The derived per-side context table (assist_tick's chain), stashed at
    /// `init`. Null = recompute unavailable (warned once at activate).
    static PLAYER_OPTION_TABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    /// Latches the "chain read failed" warning (once per session).
    static CHAIN_WARNED: AtomicBool = AtomicBool::new(false);
    /// Registration handles for deactivate. Touched only from the init /
    /// mod-menu threads, never the judge path.
    static REGISTRATION: Mutex<Option<(CallbackHandle, usize)>> = Mutex::new(None);

    /// Resolve the option-table chain. Returns availability.
    pub fn init(signatures: &SignatureStore) -> bool {
        match signatures.get_address("player_option_table") {
            Some(table) => {
                PLAYER_OPTION_TABLE.store(table as *mut u8, Ordering::Release);
                true
            }
            None => false,
        }
    }

    /// Register the judge subscriber + scene reset. Called from the Song
    /// Playback Speed mod's `enable()` (the rate feature owns this — the
    /// Real Speed Fix toggle must play no part). Fail-soft: a missing
    /// prerequisite leaves Real Speed stock at every rate, warned once.
    pub fn activate() {
        let mut registration = match REGISTRATION.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if registration.is_some() {
            return;
        }
        if PLAYER_OPTION_TABLE.load(Ordering::Acquire).is_null() {
            log_warn!(
                "song_rate/real_speed: player_option_table unresolved -- Real Speed stays stock at non-identity rates"
            );
            return;
        }
        let Some(judge) = judge_hook::register_pre(Priority::Normal, on_judge) else {
            log_warn!(
                "song_rate/real_speed: judge dispatcher unavailable -- Real Speed stays stock at non-identity rates"
            );
            return;
        };
        let scene_id = scene_manager::on_scene_change(Box::new(|_prev, next| {
            if next == scene::GAMEPLAY {
                PENDING[0].store(true, Ordering::Release);
                PENDING[1].store(true, Ordering::Release);
            }
        }));
        *registration = Some((judge, scene_id));
        log_info!("song_rate/real_speed: rate-aware Real Speed recompute active");
    }

    /// Unregister (mod disable). The current song's latches are already
    /// consumed; an in-flight attempt keeps whatever was applied.
    pub fn deactivate() {
        let taken = REGISTRATION.lock().ok().and_then(|mut guard| guard.take());
        if let Some((judge, scene_id)) = taken {
            judge_hook::unregister(judge);
            scene_manager::remove_callback(scene_id);
        }
        PENDING[0].store(false, Ordering::Release);
        PENDING[1].store(false, Ordering::Release);
    }

    /// Judge pre-callback. O(1) steady state (one consumed latch check);
    /// the once-per-song work is a handful of guarded reads + four stores.
    /// No locks, no allocation (the judge-path contract).
    fn on_judge(actor: *mut u8, _music_count: i32) {
        if actor.is_null() {
            return;
        }
        let side = unsafe { *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) };
        let side = if side == 1 { 1usize } else { 0usize };
        if !PENDING[side].swap(false, Ordering::AcqRel) {
            return;
        }
        // First dispatch of this side's song: strictly after any loader
        // thread commit (the same guarantee assist_tick's anchor uses), and
        // after the actor latched its own multiplier copy at construction.
        let snapshot = clock_patch::snapshot();
        if !snapshot.is_non_identity_commit() {
            // Identity song: nothing to do — the native (or fix-patched)
            // derivation stands untouched, both toggle states bit-identical.
            return;
        }

        let table = PLAYER_OPTION_TABLE.load(Ordering::Acquire);
        if table.is_null() {
            return;
        }
        unsafe {
            let holder = *(table.add(side * 8) as *const *const u8);
            if holder.is_null() {
                return warn_chain_unreadable("ctx-table entry is null", side);
            }
            let ctx = *(holder as *const *const u8);
            if ctx.is_null() {
                return warn_chain_unreadable("side context is null", side);
            }
            let option = ctx.add(CTX_OPTION_OFFSET);
            let speed_type = *(option.add(OPTION_SPEED_TYPE) as *const i32);
            if speed_type != SPEED_TYPE_REAL {
                // Fixed-multiplier mode: no BPM derivation exists; req 33
                // covers the Real Speed derivation only.
                return;
            }
            let target = *(option.add(OPTION_TARGET) as *const i32);
            let core_bpm = *(option.add(OPTION_CORE_BPM) as *const f64);
            let Some(multiplier) = rate_adjusted_multiplier(target, core_bpm, &snapshot) else {
                return warn_chain_unreadable("target/core BPM outside the trusted domain", side);
            };

            // The actor cluster is what the per-frame renderer copy reads;
            // Option+0x10 keeps the game's own displays consistent. Writing
            // both lerp endpoints collapses any in-flight speed transition
            // to the adjusted value.
            *(option.add(OPTION_DERIVED_MULTIPLIER) as *mut i32) = multiplier;
            *(actor.add(ACTOR_SPEED_INT) as *mut i32) = multiplier;
            let as_float = multiplier as f32 / 100.0;
            *(actor.add(ACTOR_SPEED_CURRENT) as *mut f32) = as_float;
            *(actor.add(ACTOR_SPEED_TARGET) as *mut f32) = as_float;
            log_info!(
                "song_rate/real_speed: side {} multiplier {} (target {} core {:.2} at {}%)",
                side,
                multiplier,
                target,
                core_bpm,
                snapshot.requested_percent
            );
        }
    }

    fn warn_chain_unreadable(why: &str, side: usize) {
        if !CHAIN_WARNED.swap(true, Ordering::Relaxed) {
            log_warn!(
                "song_rate/real_speed: option chain unreadable for side {} ({}) -- Real Speed stays stock this song (warned once)",
                side,
                why
            );
        }
    }
}

#[cfg(windows)]
pub use glue::{activate, deactivate, init};
