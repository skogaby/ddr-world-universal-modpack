//! Background hide (design §4.3.6) — the 2D `bg_root` background clip is
//! made transparent for the duration of a 3D scene so the engine's model
//! passes are what the player sees behind the lane.
//!
//! Mechanism (maintainer override: NEVER a placeholder arc, NEVER a movie
//! suppression): every frame while armed, find the LIVE `bg_root`
//! `CMovieClip` through `BgMovieActor (singleton) → BackgroundFrame
//! (+bgframe_off) → clip slot (+bg_clip_slot_off)`, validate it against the
//! engine's 0x400-slot CMovieClip pool (inside the pool, on a slot boundary,
//! readable — the game may tear the frame/clip down under us), read its AFP
//! layer id (`clip+0x08` — the same field `overlay_element_styling` reads on
//! its captured clips) and set the layer's multiplicative colour to alpha 0.
//! On disarm the last layer gets alpha 1 back once. Every write is
//! idempotent-per-frame and paired with the restore (design §6 principle).
//!
//! Every engine offset comes from `scene3d::sites()` (RE §1.6); `clip+0x08`
//! is the CMovieClip wrapper's layer-id field that `cmovieclip_create`'s
//! consumers already rely on crate-wide.
//!
//! GAME THREAD ONLY (`on_frame` from the mod's frame callback).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::core::memory;
use crate::services::{bm2d_api, scene3d};
use crate::{log_info, log_warn};

/// CMovieClip wrapper: `u32` type-1 AFP layer id.
const CLIP_LAYER_ID_OFF: usize = 0x08;
/// Consecutive frames without a live clip before the one-per-arm WARN.
const MISS_WARN_THRESHOLD: u32 = 60;

static ARMED: AtomicBool = AtomicBool::new(false);
/// The layer id written alpha-0 most recently (0 = none).
static LAST_LAYER: AtomicU32 = AtomicU32::new(0);
static MISSES: AtomicU32 = AtomicU32::new(0);
static MISS_WARNED: AtomicBool = AtomicBool::new(false);
/// Log the first successful hide once per arm.
static HIDDEN_LOGGED: AtomicBool = AtomicBool::new(false);

/// Start hiding on the next frame. Idempotent. One WARN + inert when the
/// libafp colour setter is unavailable.
pub fn arm() {
    if !bm2d_api::layer_color_available() {
        log_warn!("bg-hide: afp_layer_set_color unavailable -- 2D background stays visible");
        return;
    }
    if !scene3d::is_available() {
        log_warn!("bg-hide: scene3d sites unavailable -- 2D background stays visible");
        return;
    }
    MISSES.store(0, Ordering::Relaxed);
    MISS_WARNED.store(false, Ordering::Relaxed);
    HIDDEN_LOGGED.store(false, Ordering::Relaxed);
    ARMED.store(true, Ordering::Release);
}

/// Stop hiding and restore the last hidden layer's alpha once. Idempotent.
pub fn disarm() {
    if !ARMED.swap(false, Ordering::AcqRel) {
        return;
    }
    let layer = LAST_LAYER.swap(0, Ordering::AcqRel);
    if layer != 0 {
        let ok = bm2d_api::layer_set_color_raw(layer, 1.0, 1.0, 1.0, 1.0);
        log_info!(
            "bg-hide: disarmed (layer 0x{:X} alpha restored{})",
            layer,
            if ok { "" } else { " -- set_color FAILED" }
        );
    } else {
        log_info!("bg-hide: disarmed (no layer was hidden)");
    }
}

pub fn is_armed() -> bool {
    ARMED.load(Ordering::Acquire)
}

/// Per-frame driver (game thread). O(1) when disarmed.
pub fn on_frame() {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    match live_bg_layer() {
        Some(layer) => {
            MISSES.store(0, Ordering::Relaxed);
            let prev = LAST_LAYER.swap(layer, Ordering::AcqRel);
            if prev != 0 && prev != layer {
                // The clip was recreated: give the old layer its alpha back
                // (it may be gone — the setter just fails harmlessly then).
                let _ = bm2d_api::layer_set_color_raw(prev, 1.0, 1.0, 1.0, 1.0);
            }
            let ok = bm2d_api::layer_set_color_raw(layer, 1.0, 1.0, 1.0, 0.0);
            if !HIDDEN_LOGGED.swap(true, Ordering::AcqRel) {
                log_info!(
                    "bg-hide: bg_root layer 0x{:X} alpha 0{}",
                    layer,
                    if ok { "" } else { " -- set_color FAILED" }
                );
            }
        }
        None => {
            let n = MISSES.fetch_add(1, Ordering::Relaxed) + 1;
            if n == MISS_WARN_THRESHOLD && !MISS_WARNED.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "bg-hide: no live bg_root clip for {} frames (actor/frame/clip chain unreadable or clip not in the pool) -- 2D background stays visible",
                    n
                );
            }
        }
    }
}

/// The live `bg_root` clip's AFP layer id, or `None` when any link of the
/// chain is unreadable / the clip is not a pool slot / the layer is 0.
fn live_bg_layer() -> Option<u32> {
    let s = scene3d::sites()?;
    if !memory::is_readable(s.bgmovie_actor, 8) {
        return None;
    }
    // SAFETY: every pointer is probed before the read that follows it.
    unsafe {
        let actor = memory::read_ptr(s.bgmovie_actor);
        if actor.is_null() || !memory::is_readable(actor, s.bgframe_off + 8) {
            return None;
        }
        let frame = memory::read_ptr(actor.add(s.bgframe_off));
        if frame.is_null() || !memory::is_readable(frame, s.bg_clip_slot_off + 8) {
            return None;
        }
        let clip = memory::read_ptr(frame.add(s.bg_clip_slot_off));
        if clip.is_null() {
            return None;
        }
        let pool = s.cmovieclip_pool as usize;
        let stride = s.cmovieclip_pool_stride;
        let count = s.cmovieclip_pool_count;
        let c = clip as usize;
        if stride == 0 || c < pool || c >= pool + count * stride || (c - pool) % stride != 0 {
            return None;
        }
        if !memory::is_readable(clip, CLIP_LAYER_ID_OFF + 4) {
            return None;
        }
        let layer = memory::read_u32(clip.add(CLIP_LAYER_ID_OFF));
        if layer == 0 {
            None
        } else {
            Some(layer)
        }
    }
}
