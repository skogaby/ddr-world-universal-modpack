//! Scissor-record rescale detour (design §4.8 / research §4.3).
//!
//! The ScreenCommandList walker's tag-0x0C handler `(walker_ctx**, record*)`
//! copies the record's `{u16 x, y, w, h}` VERBATIM into the gd 0x18 record
//! that becomes `IDirect3DDevice9::SetScissorRect` — render-target pixels,
//! no canvas scaling. Every other 2D handler converts canvas → NDC through
//! the walker's 2D context (`ndc = (x·scale + offset)·2 − 1` with
//! `scale = 1/canvas`, `offset = origin/rt`), so with a render target that is
//! not 1280×720 every scissored layer (options-menu lists, the song wheel,
//! any `root+0x60 != 0` group, the DLL's own `overlay_draw` scissors) would
//! clip to the top-left 1280×720 of the surface.
//!
//! The fix is the one walker-level piece a resolution mod needs: before the
//! original runs, rewrite the record's rect into RT pixels with exactly the
//! math the draw handlers use (`plan::scissor_scale`, host-tested), call the
//! original, then RESTORE the record bytes — lists are re-walked, and the
//! restore keeps them byte-identical for the next walk (and for the stock
//! path should the detour ever be disabled). Installed only when the plan's
//! render is not 1280×720 (stock canvas == RT is already exact).
//!
//! Struct facts (AOB-pinned prologue + research §4.1, identical on all four
//! builds): `ctx = *walker` — `+0x00` offset vec `{ox/rt_w, oy/rt_h}`,
//! `+0x10` scale vec `{1/canvas_w, 1/canvas_h}` (written by the tag-0x07
//! set-context handler); `gd = walker[1]` — `+0x144`/`+0x146` u16 viewport
//! w/h (set by the segment header); record `+4` u16 enable, `+6/+8/+A/+C`
//! u16 x, y, w, h.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::plan::{scissor_scale, Dims};

type ScissorFn = unsafe extern "C" fn(*mut *mut u8, *mut u8);

static mut DETOUR: Option<GenericDetour<ScissorFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Scaled records logged so far (the first few, for the cabinet check).
static LOGGED: AtomicU32 = AtomicU32::new(0);
const LOG_FIRST_N: u32 = 3;
/// One-shot WARN when the walker context looks unusable (stale/zero scale).
static WARNED_CTX: AtomicBool = AtomicBool::new(false);
/// One-shot INFO on the very first dispatch (scaled or not) — three 4K
/// cabinet runs through song select, the options modals and gameplay never
/// produced a scissor record (2026-09-07), so "is the handler ever called"
/// needs its own line.
static FIRST_CALL: AtomicBool = AtomicBool::new(false);

const CTX_OFFSET_X: usize = 0x00;
const CTX_OFFSET_Y: usize = 0x04;
const CTX_SCALE_X: usize = 0x10;
const CTX_SCALE_Y: usize = 0x14;
const GD_RT_W: usize = 0x144;
const GD_RT_H: usize = 0x146;
const REC_ENABLE: usize = 0x4;
const REC_RECT: usize = 0x6;
const REC_RECT_LEN: usize = 8;

/// Install the detour on `scissor_handler`. Idempotent.
pub fn install(sigs: &SignatureStore) -> Result<(), String> {
    if INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let target = sigs
        .get_address("scissor_handler")
        .ok_or("scissor_handler unresolved")?;
    unsafe {
        let target: ScissorFn = std::mem::transmute(target);
        hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            scissor_detour as ScissorFn,
        )
        .map_err(|e| format!("scissor_handler detour install failed: {e}"))?;
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!("CustomResolution: scissor detour installed (canvas -> render-target px)");
    Ok(())
}

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// The rect rewrite, or `None` when the record should pass through untouched
/// (disabled record, unusable walker context, degenerate dims).
unsafe fn scaled_rect(walker: *mut *mut u8, record: *mut u8) -> Option<[u8; REC_RECT_LEN]> {
    if walker.is_null() || record.is_null() {
        return None;
    }
    if *(record.add(REC_ENABLE) as *const u16) == 0 {
        return None;
    }
    let ctx = *walker;
    let gd = *walker.add(1);
    if ctx.is_null() || gd.is_null() {
        return None;
    }
    let rt = Dims::new(
        *(gd.add(GD_RT_W) as *const u16) as u32,
        *(gd.add(GD_RT_H) as *const u16) as u32,
    );
    if rt.w == 0 || rt.h == 0 {
        return None;
    }
    let sx = *(ctx.add(CTX_SCALE_X) as *const f32);
    let sy = *(ctx.add(CTX_SCALE_Y) as *const f32);
    if !(sx.is_finite() && sy.is_finite() && sx > 0.0 && sy > 0.0) {
        if !WARNED_CTX.swap(true, Ordering::AcqRel) {
            log_warn!(
                "CustomResolution: scissor record before any 2D context (scale {sx}/{sy}) -- passed through raw"
            );
        }
        return None;
    }
    let canvas = (1.0 / sx, 1.0 / sy);
    let offset_px = (
        *(ctx.add(CTX_OFFSET_X) as *const f32) * rt.w as f32,
        *(ctx.add(CTX_OFFSET_Y) as *const f32) * rt.h as f32,
    );
    if !(offset_px.0.is_finite() && offset_px.1.is_finite()) {
        return None;
    }
    let rect = record.add(REC_RECT) as *const u16;
    let (x, y, w, h) = (*rect, *rect.add(1), *rect.add(2), *rect.add(3));
    let (nx, ny, nw, nh) = scissor_scale(x, y, w, h, rt, canvas, offset_px);
    if (nx, ny, nw, nh) == (x, y, w, h) {
        return None;
    }
    let n = LOGGED.fetch_add(1, Ordering::AcqRel);
    if n < LOG_FIRST_N {
        log_info!(
            "CustomResolution: scissor {}x{}+{}+{} (canvas {:.0}x{:.0}) -> {}x{}+{}+{} (rt {}x{}, origin {:.1},{:.1}){}",
            w,
            h,
            x,
            y,
            canvas.0,
            canvas.1,
            nw,
            nh,
            nx,
            ny,
            rt.w,
            rt.h,
            offset_px.0,
            offset_px.1,
            if n + 1 == LOG_FIRST_N {
                " -- further rescales are silent"
            } else {
                ""
            }
        );
    }
    let mut out = [0u8; REC_RECT_LEN];
    out[0..2].copy_from_slice(&nx.to_le_bytes());
    out[2..4].copy_from_slice(&ny.to_le_bytes());
    out[4..6].copy_from_slice(&nw.to_le_bytes());
    out[6..8].copy_from_slice(&nh.to_le_bytes());
    Some(out)
}

unsafe extern "C" fn scissor_detour(walker: *mut *mut u8, record: *mut u8) {
    let Some(hook) = (*addr_of!(DETOUR)).as_ref() else {
        log_warn!("CustomResolution: scissor detour called without its original -- skipped");
        return;
    };
    if !FIRST_CALL.swap(true, Ordering::AcqRel) {
        let _ = std::panic::catch_unwind(|| {
            let enable = if record.is_null() {
                u16::MAX
            } else {
                *(record.add(REC_ENABLE) as *const u16)
            };
            let (rt_w, rt_h) = if walker.is_null() || (*walker.add(1)).is_null() {
                (0, 0)
            } else {
                let gd = *walker.add(1);
                (
                    *(gd.add(GD_RT_W) as *const u16),
                    *(gd.add(GD_RT_H) as *const u16),
                )
            };
            log_info!(
                "CustomResolution: scissor handler first dispatch (enable={enable}, viewport {rt_w}x{rt_h})"
            );
        });
    }
    // Any panic in the rewrite path ⇒ the original runs on the untouched record.
    let rewrite = std::panic::catch_unwind(|| scaled_rect(walker, record)).unwrap_or(None);
    match rewrite {
        Some(new) => {
            let rect = record.add(REC_RECT);
            let mut saved = [0u8; REC_RECT_LEN];
            std::ptr::copy_nonoverlapping(rect, saved.as_mut_ptr(), REC_RECT_LEN);
            std::ptr::copy_nonoverlapping(new.as_ptr(), rect, REC_RECT_LEN);
            hook.call(walker, record);
            std::ptr::copy_nonoverlapping(saved.as_ptr(), rect, REC_RECT_LEN);
        }
        None => hook.call(walker, record),
    }
}
