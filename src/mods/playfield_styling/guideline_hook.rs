//! Guideline styling + perspective — the measure guideline draws through its
//! own plain-sprite batch path, NOT the shared quad fill, so it gets its own
//! pair of detours:
//!
//! 1. **Capture detour** on the guideline draw (`fn(this)`): derives the
//!    side (mode@+0x78 doubles → side 0; presence read; versus →
//!    posX@+0x80 < 640 → P1) and builds the pass style:
//!    - playfield_styling's latched `(scale, opacity)` for that side (when
//!      that mod is enabled), pre-scaling `Ybase@+0x84` to `Y/s` around the
//!      original call (restored after);
//!    - player_perspective's latched preset for that side (when that side
//!      is in a perspective mode), resolved through the shared
//!      `compute_constants` into the same c48/c49 block the perspective VS
//!      receives — anchored on the original `Ybase@+0x84`, lane center from
//!      `Xbase@+0x80`, plus the guideline object's own reverse flag
//!      (virtual base @ +0x20 — the exact read the draw itself performs).
//!
//!    Why `Y/s`: the draw computes each line's emitted y as
//!    `±(offset + adj) + Ybase`. With Ybase pre-divided, the emitter-side
//!    transform `y' = s·y` reconstructs `±s·(offset+adj) + Ybase` — the
//!    exact receptor-anchored scale for BOTH scroll directions — while the
//!    draw's own cull bounds (patched via services::cull_window) naturally
//!    cover the extended window.
//!
//! 2. **Transform detour** on the bulk emitter (`fn(cmdlist, count,
//!    records)` — exactly ONE caller module-wide, verified at derivation):
//!    when the pass state is active, rewrites each 0x14-byte record
//!    `{x, y, w, h, color}` in place — scale first (x about the line's own
//!    center `cx = x + w/2`, y/w/h scaled, alpha MSB multiplied), then the
//!    generalized perspective map on the scaled screen coords
//!    (`d = clamp((y − anchor)·dir, d_min)`, `sp = z0·k/(k+d)`, y/x/w/h
//!    converged about `(cx, anchor)`) — the identical transform the
//!    perspective VS applies to same-offset arrows, so lines land exactly
//!    on their notes (guidelines bind the DEFAULT shader with tag-1/no-UV
//!    records, unreachable by the shader rewrite; 3 px tall → affine error
//!    irrelevant).
//!
//! ## Shared ownership (install decoupling)
//!
//! Both playfield_styling AND player_perspective need these detours, and
//! either may be config-disabled. `acquire(consumer, targets)` installs on
//! first use; `release(consumer)` removes only when no consumer remains.

use std::cell::Cell;
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::{log_info, log_warn};

use super::MOD_ID;

// ── Guideline object field offsets (Ghidra-verified; research §8) ───

/// Virtual-base pointer used by the draw's own reverse-flag lookup.
const OFF_VBPTR: usize = 0x20;
/// Mode enum (1 = double → 8 panels) on the guideline object.
const OFF_MODE: usize = 0x78;
/// X base — lane left edge, screen-space f32.
const OFF_X_BASE: usize = 0x80;
/// Y base — receptor row screen Y, f32. The capture detour's pre-scale target.
const OFF_Y_BASE: usize = 0x84;

/// Versus playfield midline (screen units, 1280-wide space).
const X_SPLIT: f32 = 640.0;

/// Guideline record stride: `{x f32, y f32, w f32, h f32, color u32}`.
const RECORD_STRIDE: usize = 0x14;

// ── Pass state ──────────────────────────────────────────────────────
// Set by the capture detour around the original call, consumed by the
// emitter detour. Strictly thread-synchronous (the emitter is invoked
// inside the guideline draw on the same thread), so a thread-local Cell is
// the exact-fit publication mechanism.

/// Combined per-pass style. `persp` is the same resolved constant block the
/// perspective VS receives (one source of truth: `compute_constants`).
#[derive(Clone, Copy)]
struct PassStyle {
    scale: f32,
    opacity: f32,
    persp: Option<crate::mods::player_perspective::PerspConstants>,
}

thread_local! {
    static PASS_STATE: Cell<Option<PassStyle>> = const { Cell::new(None) };
}

static PANIC_WARNED: AtomicBool = AtomicBool::new(false);

/// Consumers of the shared guideline detours.
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum Consumer {
    PlayfieldStyling = 0,
    PlayerPerspective = 1,
}

static WANTED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Resolve the guideline object's side: doubles → 0; single active side →
/// that side; versus → X-split; none present → `None`.
fn guideline_side(this: *mut u8) -> Option<u8> {
    unsafe {
        let mode = (this.add(OFF_MODE) as *const i32).read_unaligned();
        if mode == 1 {
            return Some(0);
        }
        let x = (this.add(OFF_X_BASE) as *const f32).read_unaligned();
        match super::fill_hook::read_presence() {
            (true, false) => Some(0),
            (false, true) => Some(1),
            (true, true) => Some(if x < X_SPLIT { 0 } else { 1 }),
            (false, false) => None,
        }
    }
}

/// The guideline object's reverse flag, read exactly the way the draw
/// itself reads it: `*(u8*)(this + 0x20 + *(i32*)(*(u64*)(this+0x20)+4))`.
unsafe fn guideline_reverse(this: *mut u8) -> bool {
    let vb = *(this.add(OFF_VBPTR) as *const *const u8);
    if vb.is_null() {
        return false;
    }
    let disp = *(vb.add(4) as *const i32);
    *this.add(OFF_VBPTR + disp as usize) != 0
}

// ── Capture detour (guideline draw) ─────────────────────────────────

type GuidelineDrawFn = unsafe extern "C" fn(this: *mut u8);

static mut DRAW_HOOK: Option<GenericDetour<GuidelineDrawFn>> = None;

unsafe extern "C" fn guideline_draw_cb(this: *mut u8) {
    let hook = match &*addr_of!(DRAW_HOOK) {
        Some(h) => h,
        None => return,
    };

    if this.is_null() {
        return hook.call(this);
    }

    // Derive the style for this object's side. Any anomaly → stock call.
    let plan = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let side = guideline_side(this)?;
        let (s, op) = if super::is_enabled() {
            super::latched(side as usize)
        } else {
            (1.0, 1.0)
        };
        let persp = crate::mods::player_perspective::latched_params(side).map(|p| {
            let mode = (this.add(OFF_MODE) as *const i32).read_unaligned();
            let x_base = (this.add(OFF_X_BASE) as *const f32).read_unaligned();
            crate::mods::player_perspective::compute_constants(
                &p,
                (this.add(OFF_Y_BASE) as *const f32).read_unaligned(),
                crate::mods::player_perspective::lane_center(mode == 1, x_base),
                if guideline_reverse(this) { -1.0 } else { 1.0 },
            )
        });
        if s == 1.0 && op == 1.0 && persp.is_none() {
            None
        } else {
            Some(PassStyle {
                scale: s,
                opacity: op,
                persp,
            })
        }
    }))
    .unwrap_or_else(|_| {
        if !PANIC_WARNED.swap(true, Ordering::Relaxed) {
            crate::log_error!("{MOD_ID}: guideline capture panicked — stock call");
        }
        None
    });

    let style = match plan {
        Some(p) => p,
        None => return hook.call(this),
    };

    // Pre-scale Ybase (restore after) + publish the pass state for the
    // emitter transform. `s` is clamped ≥ 0.25 by the option range, so the
    // divide is well-defined.
    let y_base_ptr = this.add(OFF_Y_BASE) as *mut f32;
    let y_base = y_base_ptr.read_unaligned();
    if style.scale != 1.0 {
        y_base_ptr.write_unaligned(y_base / style.scale);
    }
    PASS_STATE.with(|c| c.set(Some(style)));

    hook.call(this);

    PASS_STATE.with(|c| c.set(None));
    y_base_ptr.write_unaligned(y_base);
}

// ── Transform detour (bulk emitter) ─────────────────────────────────

/// `fn(cmdlist, count, records) -> cmd_header*` — writes a tag-0x01
/// DRAWSPRITES command and memcpys `count * 0x14` record bytes.
type BulkEmitterFn =
    unsafe extern "C" fn(cmdlist: *mut u8, count: i64, records: *mut u8) -> *mut u8;

static mut EMITTER_HOOK: Option<GenericDetour<BulkEmitterFn>> = None;

unsafe extern "C" fn emitter_cb(cmdlist: *mut u8, count: i64, records: *mut u8) -> *mut u8 {
    let hook = match &*addr_of!(EMITTER_HOOK) {
        Some(h) => h,
        None => return std::ptr::null_mut(),
    };

    let state = PASS_STATE.with(|c| c.get());
    if let Some(style) = state {
        if !records.is_null() && count > 0 && count < 0x10000 {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let s = style.scale;
                let op = style.opacity;
                for i in 0..count as usize {
                    let r = records.add(i * RECORD_STRIDE);
                    let x = (r as *const f32).read_unaligned();
                    let y = (r.add(4) as *const f32).read_unaligned();
                    let w = (r.add(8) as *const f32).read_unaligned();
                    let h = (r.add(12) as *const f32).read_unaligned();

                    // 1. Playfield scale, about the line's own center
                    //    (identity when s == 1).
                    let cx = x + w * 0.5;
                    let mut nx = cx - s * w * 0.5;
                    let mut ny = s * y;
                    let mut nw = s * w;
                    let mut nh = s * h;

                    // 2. The generalized perspective map on the scaled
                    //    screen coords — the identical transform the
                    //    perspective VS applies (same resolved constants,
                    //    shared `map_point`). For a full-lane-width
                    //    guideline record the record center equals the
                    //    constants' cx (live-verified by the hallway
                    //    deploy: lines land on their notes).
                    if let Some(p) = style.persp {
                        let (px, py, sp) = p.map_point(nx, ny);
                        nx = px;
                        ny = py;
                        nw *= sp;
                        nh *= sp;
                    }

                    (r as *mut f32).write_unaligned(nx);
                    (r.add(4) as *mut f32).write_unaligned(ny);
                    (r.add(8) as *mut f32).write_unaligned(nw);
                    (r.add(12) as *mut f32).write_unaligned(nh);
                    if op != 1.0 {
                        // Alpha is the color u32's MSB (memory byte 3).
                        let a_ptr = r.add(16 + 3);
                        let a = a_ptr.read_unaligned() as f32 * op;
                        a_ptr.write_unaligned(a.round().clamp(0.0, 255.0) as u8);
                    }
                }
            }));
        }
    }

    hook.call(cmdlist, count, records)
}

// ── Acquire / release (shared install) ──────────────────────────────

/// Declare interest in the guideline detours, installing them on first use.
/// Returns false if the install failed (caller decides how fatal that is).
/// `player_array` seeds the presence read used for side attribution — it is
/// stored only if not already set (playfield_styling's fill install also
/// seeds it; either consumer may come first or be disabled).
pub(crate) fn acquire(
    consumer: Consumer,
    draw: *const u8,
    emitter: *const u8,
    player_array: *const u8,
) -> bool {
    WANTED[consumer as usize].store(true, Ordering::Release);
    super::fill_hook::seed_player_array(player_array);
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    if draw.is_null() || emitter.is_null() {
        log_warn!("{MOD_ID}: guideline targets unresolved — hooks unavailable");
        return false;
    }
    unsafe {
        let draw_fn: GuidelineDrawFn = std::mem::transmute(draw);
        if let Err(e) =
            crate::core::hooks::install_enabled(addr_of_mut!(DRAW_HOOK), draw_fn, guideline_draw_cb)
        {
            log_warn!("{MOD_ID}: guideline draw hook install failed: {e}");
            return false;
        }
        let emitter_fn: BulkEmitterFn = std::mem::transmute(emitter);
        if let Err(e) =
            crate::core::hooks::install_enabled(addr_of_mut!(EMITTER_HOOK), emitter_fn, emitter_cb)
        {
            log_warn!("{MOD_ID}: guideline emitter hook install failed: {e}");
            if let Some(d) = (*addr_of_mut!(DRAW_HOOK)).take() {
                let _ = d.disable();
            }
            return false;
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "{MOD_ID}: guideline hooks installed (draw @ {:p}, emitter @ {:p})",
        draw,
        emitter
    );
    true
}

/// Drop a consumer's interest; tears the detours down only when no
/// consumer remains.
pub(crate) fn release(consumer: Consumer) {
    WANTED[consumer as usize].store(false, Ordering::Release);
    if WANTED.iter().any(|w| w.load(Ordering::Acquire)) {
        return;
    }
    if !INSTALLED.swap(false, Ordering::AcqRel) {
        return;
    }
    unsafe {
        if let Some(d) = (*addr_of_mut!(EMITTER_HOOK)).take() {
            let _ = d.disable();
        }
        if let Some(d) = (*addr_of_mut!(DRAW_HOOK)).take() {
            let _ = d.disable();
        }
    }
    PASS_STATE.with(|c| c.set(None));
    PANIC_WARNED.store(false, Ordering::Relaxed);
}
