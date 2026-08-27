//! Opacity composition — detours the wrapper SetColor methods and multiplies
//! the game's own alpha writes by the per-side opacity, so opacity composes
//! WITH the game's alpha semantics (combo visibility gating 0/1, pacemaker
//! negative-delta dim 0.5) instead of fighting them (design §4.6).
//!
//! - Float form (vtable +0x90): `fn(this, a: f32, r: f32, g: f32, b: f32)` —
//!   **alpha is the FIRST float argument** (RE §3; XMM1). This is the
//!   load-bearing detour — it covers every observed color write on the scoped
//!   elements (combo gate, digit tint via the array-form +0x98 which dispatches
//!   into +0x90 virtually, and the pacemaker dim). Its install failure refuses
//!   the mod's enable.
//! - Int form (vtable +0xB0): `fn(this, a_pct: i32, r: f32, g: f32, b: f32)` —
//!   integer alpha percent (param 1). Non-fatal: the float form covers every
//!   observed write; this closes the only other multiplicative-color path and
//!   logs once if it ever fires on a tracked clip (design Appendix C.2).
//!
//! For a tracked, side-bound clip: `a' = a * opacity[side]`. Untracked or
//! still-unbound clips forward unchanged — the hooks are engine-wide, so
//! correctness for every other pool clip is non-negotiable.

use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::log_info;

use super::{capture, is_enabled, MOD_ID};

/// Wrapper SetColor float form (vtable +0x90):
/// `void(this, a: f32, r: f32, g: f32, b: f32)` — alpha first.
type SetColorFloatFn = unsafe extern "C" fn(*mut u8, f32, f32, f32, f32);

static mut COLOR_FLOAT_HOOK: Option<GenericDetour<SetColorFloatFn>> = None;

/// Compute the composed alpha for a color write on `this`. Tracked + bound →
/// `a * opacity/100`; otherwise `a` unchanged. Kept panic-free itself (pure
/// arithmetic + a registry read), but the caller still wraps it defensively.
fn compose_alpha(this: *mut u8, a: f32) -> f32 {
    if !is_enabled() {
        return a;
    }
    match capture::tracked_bound_side(this) {
        Some(side) => {
            let op = super::opacity_pct_fast(side as usize);
            a * (op as f32 / 100.0)
        }
        // Untracked, or tracked-but-unbound (only pre-bind write is combo's
        // create-time a=0 hide, which is opacity-invariant) → unchanged.
        None => a,
    }
}

unsafe extern "C" fn set_color_float_hook(this: *mut u8, a: f32, r: f32, g: f32, b: f32) {
    let a_prime =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compose_alpha(this, a))) {
            Ok(v) => v,
            Err(_) => a,
        };
    if let Some(h) = &*addr_of!(COLOR_FLOAT_HOOK) {
        h.call(this, a_prime, r, g, b);
    }
}

/// Install the +0x90 float compose detour (load-bearing). Returns false on
/// failure; the mod refuses to enable in that case.
pub(crate) fn install_float(addr: *const u8) -> bool {
    unsafe {
        let target: SetColorFloatFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            addr_of_mut!(COLOR_FLOAT_HOOK),
            target,
            set_color_float_hook,
        ) {
            Ok(()) => {
                log_info!(
                    "{MOD_ID}: SetColor(+0x90) compose hook installed @ {:p}",
                    addr
                );
                true
            }
            Err(e) => {
                crate::log_warn!("{MOD_ID}: SetColor(+0x90) hook install failed: {e}");
                false
            }
        }
    }
}

/// Tear down the color compose detours.
pub(crate) fn remove() {
    unsafe {
        if let Some(d) = (*addr_of_mut!(COLOR_FLOAT_HOOK)).take() {
            let _ = d.disable();
        }
        if let Some(d) = (*addr_of_mut!(COLOR_INT_HOOK)).take() {
            let _ = d.disable();
        }
    }
    INT_HIT_LOGGED.store(false, Ordering::Relaxed);
}

// ── Int-percent form (vtable +0xB0) ─────────────────────────────────
// `void(this, a_pct: i32, r: f32, g: f32, b: f32)` — the int alpha is param 1
// (EDX), which the wrapper divides by 100.0 before forwarding. Non-fatal
// (design §4.6, Q9): the float form covers every color write observed on the
// scoped elements; this closes the only other multiplicative-color path. A
// one-shot debug log on the first tracked hit records whether +0xB0 ever
// actually fires on our clips (open question, design Appendix C.2).

type SetColorIntFn = unsafe extern "C" fn(*mut u8, i32, f32, f32, f32);

static mut COLOR_INT_HOOK: Option<GenericDetour<SetColorIntFn>> = None;

/// Latches the first observed tracked int-color write (diagnostic).
static INT_HIT_LOGGED: AtomicBool = AtomicBool::new(false);

fn compose_alpha_int(this: *mut u8, a_pct: i32) -> i32 {
    if !is_enabled() {
        return a_pct;
    }
    match capture::tracked_bound_side(this) {
        Some(side) => {
            let op = super::opacity_pct_fast(side as usize);
            if !INT_HIT_LOGGED.swap(true, Ordering::Relaxed) {
                log_info!(
                    "{MOD_ID}: +0xB0 int color fired on a tracked clip (a_pct={} side={} op={})",
                    a_pct,
                    side,
                    op
                );
            }
            // Integer alpha math, clamped ≥ 0.
            ((a_pct * op) / 100).max(0)
        }
        None => a_pct,
    }
}

unsafe extern "C" fn set_color_int_hook(this: *mut u8, a_pct: i32, r: f32, g: f32, b: f32) {
    let a_prime = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compose_alpha_int(this, a_pct)
    })) {
        Ok(v) => v,
        Err(_) => a_pct,
    };
    if let Some(h) = &*addr_of!(COLOR_INT_HOOK) {
        h.call(this, a_prime, r, g, b);
    }
}

/// Install the +0xB0 int-percent compose detour (NON-fatal). Returns false if
/// the address is missing or the install fails; the mod continues without it.
pub(crate) fn install_int(addr: *const u8) -> bool {
    unsafe {
        let target: SetColorIntFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            addr_of_mut!(COLOR_INT_HOOK),
            target,
            set_color_int_hook,
        ) {
            Ok(()) => {
                log_info!(
                    "{MOD_ID}: SetColor(+0xB0) compose hook installed @ {:p}",
                    addr
                );
                true
            }
            Err(e) => {
                crate::log_warn!("{MOD_ID}: SetColor(+0xB0) hook install failed (non-fatal): {e}");
                false
            }
        }
    }
}
