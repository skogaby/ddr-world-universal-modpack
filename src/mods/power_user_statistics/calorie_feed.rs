//! Real-time calorie feed — detours the `CalcCalorieActor` per-frame tick
//! (vtable slot 6) to cache each side's live per-stage kcal for the
//! Power User Statistics gameplay overlay.
//!
//! The game maintains a running per-stage kcal accumulator at `actor+0x94`
//! (RE doc §3.1): the tick reads the current measurement-window flag and, when
//! a window closes, adds the per-window increment into `+0x94`. That same value
//! is committed to the profile at stage finalize and summed into the
//! result-screen calorie total — so it is exactly "kcal burned this song so
//! far," in the unit the game itself displays and saves.
//!
//! We read it from **inside the actor's own live tick** (never holding the
//! actor pointer across frames), so there is no lifetime/dangling concern — we
//! cache only the integer into a per-side atomic that the widget reads.
//!
//! The `CalcCalorieActor` is constructed unconditionally in normal gameplay
//! (gamemdx `FUN_18005be50`; gated only by a HUD-suppression flag, NOT by the
//! profile's `is_disp_weight`), so the live value is available regardless of
//! whether the player has Konami's own calorie display turned on.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// Player side on the CalcCalorieActor (`+0x58`, i32).
const ACTOR_PLAY_SIDE_OFFSET: usize = 0x58;
/// Running per-stage kcal accumulator on the CalcCalorieActor (`+0x94`, i32).
const ACTOR_KCAL_OFFSET: usize = 0x94;

/// `void CalcCalorieActor::tick(this)` — the disassembled slot 6 reads only
/// `RCX` (the actor), so a single-arg signature is sufficient both to receive
/// the game's call and to re-enter the original via the trampoline.
type CalorieTickFn = unsafe extern "C" fn(actor: *mut u8);

static DETOUR: OnceLock<GenericDetour<CalorieTickFn>> = OnceLock::new();

/// Latest `actor+0x94` per side, refreshed every frame by the tick hook and
/// read by the timing-stats widget. Reset to 0 at gameplay entry so a previous
/// song's total can't flash before the first tick.
static REALTIME_KCAL: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

/// Current cached live kcal for `side` (0 = P1, 1 = P2).
pub fn latest(side: usize) -> i32 {
    REALTIME_KCAL[side & 1].load(Ordering::Acquire)
}

/// Zero both sides' cached kcal. Called on gameplay entry (the game constructs
/// a fresh actor per song, so its `+0x94` restarts at 0 too).
pub fn reset() {
    REALTIME_KCAL[0].store(0, Ordering::Release);
    REALTIME_KCAL[1].store(0, Ordering::Release);
}

/// Install the detour on the calorie-actor tick. Returns false on failure (the
/// caller then leaves the realtime-calorie line disabled).
pub fn install(signatures: &SignatureStore) -> bool {
    let Some(addr) = signatures.get_address("calc_calorie_tick") else {
        log_warn!("calorie_feed: calc_calorie_tick signature not resolved");
        return false;
    };

    unsafe {
        let target: CalorieTickFn = std::mem::transmute(addr);
        let detour = match GenericDetour::new(target, calorie_tick_hook) {
            Ok(d) => d,
            Err(e) => {
                log_warn!("calorie_feed: failed to create detour: {}", e);
                return false;
            }
        };
        // Store before enable: once the prologue is patched any thread can
        // enter the hook, which needs `.get()` to reach the original.
        if DETOUR.set(detour).is_err() {
            log_warn!("calorie_feed: detour slot already populated");
            return false;
        }
        if let Some(Err(e)) = DETOUR.get().map(|d| d.enable()) {
            log_warn!("calorie_feed: failed to enable detour: {}", e);
            return false;
        }
    }

    log_info!(
        "calorie_feed: installed calc_calorie_tick detour @ {:p}",
        addr
    );
    true
}

unsafe extern "C" fn calorie_tick_hook(actor: *mut u8) {
    if !actor.is_null() {
        let side = *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32);
        if (0..=1).contains(&side) {
            let kcal = *(actor.add(ACTOR_KCAL_OFFSET) as *const i32);
            REALTIME_KCAL[side as usize].store(kcal, Ordering::Release);
        }
    }

    // Always run the original so the game's own accumulation proceeds.
    if let Some(detour) = DETOUR.get() {
        detour.call(actor);
    }
}
