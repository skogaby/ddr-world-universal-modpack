//! The fullscreen-movie probe behind Background Movies = FULLSCREEN (NO
//! STAGE) (`movie_mode.rs`): is the live song's background movie actually
//! being drawn?
//!
//! Walks the game's own actor tree every frame (game thread, O(children)):
//! the live DancePlaySequence (vtable-verified by `song_reset::dps_step`) →
//! its `sequence::dance::SceneManageActor` child (created at DPS step 2,
//! `FUN_18007d480` on 20260825, added with the actor tree's add-child) →
//! that actor's `sequence::dance::MovieActor` child (created in the
//! SceneManageActor's onInitialize only when the song has a movie and the
//! governing side's VIDEO SIZE is FULLSCREEN or ON) → the MovieActor's
//! StackStep (0 opening, 1 ready, 2 waiting for its offset, 3 playing, 4 no
//! movie). Both classes are identified by their RTTI vtables
//! (`scene_manage_actor_vtable` / `movie_actor_vtable`); the only offsets
//! used are the `agcs::Actor` tree links (`song_reset::FIRST_CHILD_OFFSET`
//! / `NEXT_SIBLING_OFFSET`) and the Actor's embedded StackStep (`+0x58`
//! values, `+0x82` depth index — the same shape `song_reset` reads on the
//! GamePlayActor and the Combo actor, and `quick_restart_or_fail` on the
//! ShutterActor).
//!
//! Timing: the SceneManageActor answers the DPS step-3 readiness poll
//! (`0x1001`) as not-ready until its own step 2, which it only reaches once
//! the MovieActor left step 0 — so by DPS step 5 (the dancers' visibility
//! edge) the answer is final. A faked open (suppressed / failed fallback
//! graph) is filtered through `movie_policy::last_build`.
//!
//! Fail-open: vtables unresolved ⇒ [`is_available`] is false and the mode
//! degrades to THUMBNAIL at window entry (one WARN, `lifecycle.rs`).

use std::sync::atomic::{AtomicPtr, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::log_info;
use crate::services::movie_policy::{self, LastBuild};
use crate::services::song_reset;

use super::movie_mode::{self, Backdrop};

/// `agcs::Actor` embedded StackStep: `i32` values at `+0x58 + idx·8`, the
/// `u16` depth index at `+0x82` (below the 5-entry capacity).
const ACTOR_STEP_BASE: usize = 0x58;
const ACTOR_STEP_INDEX: usize = 0x82;
const ACTOR_STEP_CAPACITY: usize = 5;
/// A child list longer than this is corrupt (a DPS has ~10–20 children).
const MAX_CHILDREN: usize = 256;

static SCENE_MANAGE_VT: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static MOVIE_ACTOR_VT: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Resolve the two RTTI vtables (mod init). `false` = the probe is
/// unavailable this boot.
pub fn init(signatures: &SignatureStore) -> bool {
    let sma = signatures.get_address("scene_manage_actor_vtable");
    let movie = signatures.get_address("movie_actor_vtable");
    match (sma, movie) {
        (Some(s), Some(m)) if !s.is_null() && !m.is_null() => {
            SCENE_MANAGE_VT.store(s as *mut u8, Ordering::Release);
            MOVIE_ACTOR_VT.store(m as *mut u8, Ordering::Release);
            true
        }
        _ => {
            log_info!(
                "BackgroundDancers: SceneManageActor/MovieActor vtables unresolved -- Background Movies = FULLSCREEN falls back to THUMBNAIL this boot"
            );
            false
        }
    }
}

/// The probe can answer: both vtables, the DPS identity gate and the shared
/// BuildGraph hook (which tells a real movie from a faked one).
pub fn is_available() -> bool {
    !SCENE_MANAGE_VT.load(Ordering::Acquire).is_null()
        && !MOVIE_ACTOR_VT.load(Ordering::Acquire).is_null()
        && song_reset::dps_identity_available()
        && movie_policy::is_available()
}

/// The first direct child of `parent` whose vtable is `vt`. The links are
/// the engine's own actor tree, walked on the game thread that owns it
/// (the same unprobed walk as `song_reset::gameplay_actors`; the probe runs
/// every visible frame, and `memory::is_readable` is a VirtualQuery per
/// call) — bounded against a corrupt list.
///
/// # Safety
/// `parent` must be a live `agcs::Actor` (the caller verified its identity).
unsafe fn child_with_vtable(parent: *const u8, vt: *const u8) -> Option<*const u8> {
    let mut child = memory::read_ptr(parent.add(song_reset::FIRST_CHILD_OFFSET));
    let mut n = 0usize;
    while !child.is_null() && n < MAX_CHILDREN {
        n += 1;
        if memory::read_ptr(child) == vt {
            return Some(child);
        }
        child = memory::read_ptr(child.add(song_reset::NEXT_SIBLING_OFFSET));
    }
    None
}

/// The live song's MovieActor and its StackStep, `None` when there is no
/// verified DPS, no SceneManageActor yet, no MovieActor, or the step is
/// unreadable. Game thread only (the actor tree is the game thread's). Also
/// the STAGE SCREENS fit writer's accessor (`screen_route.rs`).
pub fn live_movie_actor() -> Option<(*mut u8, i32)> {
    let sma_vt = SCENE_MANAGE_VT.load(Ordering::Acquire) as *const u8;
    let movie_vt = MOVIE_ACTOR_VT.load(Ordering::Acquire) as *const u8;
    if sma_vt.is_null() || movie_vt.is_null() {
        return None;
    }
    // `dps_step` answers only for a vtable-verified DancePlaySequence.
    song_reset::dps_step()?;
    let dps = song_reset::live_dps()? as *const u8;
    // SAFETY: the DPS is live (verified above, game thread); the tree links
    // are engine-maintained, and the one field read outside the Actor base
    // (the MovieActor's step) is probed first.
    unsafe {
        let sma = child_with_vtable(dps, sma_vt)?;
        let movie = child_with_vtable(sma, movie_vt)?;
        if !memory::is_readable(movie, ACTOR_STEP_INDEX + 2) {
            return None;
        }
        let idx = *(movie.add(ACTOR_STEP_INDEX) as *const u16) as usize;
        if idx >= ACTOR_STEP_CAPACITY {
            return None;
        }
        let at = movie.add(ACTOR_STEP_BASE + idx * 8);
        if !memory::is_readable(at, 4) {
            return None;
        }
        Some((movie as *mut u8, memory::read_i32(at)))
    }
}

/// The live MovieActor's StackStep (see [`live_movie_actor`]).
fn movie_step() -> Option<i32> {
    live_movie_actor().map(|(_, step)| step)
}

/// The live song's fullscreen-movie state (game thread). `Backdrop::None`
/// whenever the probe is unavailable.
pub fn probe() -> Backdrop {
    if !is_available() {
        return Backdrop::None;
    }
    movie_mode::classify(
        movie_step(),
        movie_policy::should_suppress(),
        movie_policy::last_build() == LastBuild::RealOpened,
    )
}

/// The raw inputs of the last classification, for the one-shot INFO.
pub fn describe() -> String {
    format!(
        "movie actor step {:?}, graphs suppressed {}, last build {:?}",
        movie_step(),
        movie_policy::should_suppress(),
        movie_policy::last_build()
    )
}
