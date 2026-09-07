//! Debug-UI (TEST menu / hardware check / error screen) size scaling.
//!
//! The ark drives those screens through a table of gamemdx draw callbacks
//! (`createFont` / `drawFont` / `createSprite` / `drawSprite` / `drawLine` /
//! `drawFillRect` …). Positions and rect/line extents are in **screen
//! percentages** (`screen × pct / 100`), so they follow the output for free
//! (see `logical_screen` — this family deliberately stays on the physical
//! block). The two things that do NOT follow the output are the font scale
//! and the sprite scale: `createFont` asks `arkMDXGetMachineType` and picks a
//! fixed pixel scale from a 480-line table (machine 0/1, SD cabinets) or a
//! 720-line table (everything else); `createSprite` does the same with
//! 0.8 / 1.0. Neither ever looks at the back-buffer, so at 640×480 the text
//! is 1.5× too big and at 4K it is a third of the intended size.
//!
//! Fix: two post-original detours multiplying the game's own choice by
//! `output_h / ref_h` (`ref_h` = the height of the table the game picked,
//! resolved through the same `arkMDXGetMachineType` export the original
//! consulted) times the operator's `resolution.test_menu_scale`
//! ([`plan::debug_ui_scale`], pure/host-tested). Nothing else about the debug
//! drawers changes; with the factor at 1.0 nothing is installed.
//!
//! The machine-type export is resolved LAZILY on the first font/sprite
//! creation (the ark is guaranteed loaded by then — it is the caller), never
//! at `early_apply` time. Unresolvable ⇒ assume the 720-line table, one WARN.
//! Both detours are fail-open: any oddity (null out-pointer, unreadable
//! object) leaves the game's value alone.

use std::ffi::CString;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use retour::GenericDetour;
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetProcAddress;

use crate::core::hooks;
use crate::core::memory;
use crate::core::module_resolver::resolve_ark_module;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::plan;

/// `(int size_class, float* sx, float* sy)`.
type FontScaleFn = unsafe extern "C" fn(i32, *mut f32, *mut f32);
/// `(const char* name, sprite** out)`.
type SpriteCreateFn = unsafe extern "C" fn(*const u8, *mut *mut u8);
/// `arkMDXGetMachineType(i32* out)`.
type GetTypeFn = unsafe extern "C" fn(*mut i32);

static mut FONT_DETOUR: Option<GenericDetour<FontScaleFn>> = None;
static mut SPRITE_DETOUR: Option<GenericDetour<SpriteCreateFn>> = None;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static OUTPUT_H: AtomicU32 = AtomicU32::new(0);
/// `resolution.test_menu_scale` as f32 bits.
static USER_SCALE_BITS: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// Machine type: `MT_UNRESOLVED` until the first detour call resolves it.
const MT_UNRESOLVED: i32 = i32::MIN;
static MACHINE_TYPE: AtomicI32 = AtomicI32::new(MT_UNRESOLVED);
static LOGGED_FONT: AtomicBool = AtomicBool::new(false);
static LOGGED_SPRITE: AtomicBool = AtomicBool::new(false);

/// Sprite object layout (`createSprite` allocates 0x10): `+0x00 texture*`,
/// `+0x08 f32 scale`.
const SPRITE_SCALE_OFF: usize = 0x08;

/// True once both detours are live (diagnostic for the boot summary).
pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Install for the given output height. Skipped (Ok) when the factor is
/// the identity for BOTH machine-type tables (720-line output at user scale
/// 1.0 — the only case where nothing can change). Failures are non-fatal
/// for the boot plan: the caller logs and continues.
pub fn install(sigs: &SignatureStore, output: plan::Dims, user_scale: f32) -> Result<(), String> {
    if INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    if plan::debug_ui_scale(output.h, false, user_scale).is_none()
        && plan::debug_ui_scale(output.h, true, user_scale).is_none()
    {
        return Ok(());
    }
    let font = sigs
        .get_address("debug_font_scale")
        .ok_or("debug_font_scale unresolved")?;
    let sprite = sigs
        .get_address("debug_sprite_create")
        .ok_or("debug_sprite_create unresolved")?;

    OUTPUT_H.store(output.h, Ordering::Release);
    USER_SCALE_BITS.store(user_scale.to_bits(), Ordering::Release);

    unsafe {
        let font: FontScaleFn = std::mem::transmute(font);
        hooks::install_enabled(
            std::ptr::addr_of_mut!(FONT_DETOUR),
            font,
            font_scale_detour as FontScaleFn,
        )
        .map_err(|e| format!("debug_font_scale detour install failed: {e}"))?;
        let sprite: SpriteCreateFn = std::mem::transmute(sprite);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(SPRITE_DETOUR),
            sprite,
            sprite_create_detour as SpriteCreateFn,
        ) {
            // Keep the font half: text readability is the point; sprites
            // are the hardware-check logos.
            log_warn!(
                "CustomResolution: debug_sprite_create detour install failed ({e}) -- TEST-menu text scales, sprites stay stock"
            );
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "CustomResolution: debug-UI scale detours installed (output {} lines, test_menu_scale {})",
        output.h,
        user_scale
    );
    Ok(())
}

/// The machine type the game's own chooser consulted (cached after the
/// first resolve). `None` when the ark export is unavailable.
fn machine_type() -> Option<i32> {
    let cached = MACHINE_TYPE.load(Ordering::Acquire);
    if cached != MT_UNRESOLVED {
        return (cached >= 0).then_some(cached);
    }
    let resolved = resolve_machine_type();
    // -1 = "export unavailable", sticky so the WARN fires once.
    MACHINE_TYPE.store(resolved.unwrap_or(-1), Ordering::Release);
    if resolved.is_none() {
        log_warn!(
            "CustomResolution: arkMDXGetMachineType unavailable -- debug-UI scale assumes the 720-line table"
        );
    }
    resolved
}

fn resolve_machine_type() -> Option<i32> {
    let ark = resolve_ark_module()?;
    let cname = CString::new("arkMDXGetMachineType").ok()?;
    let addr = unsafe { GetProcAddress(ark.handle, PCSTR(cname.as_ptr() as *const u8)) }?;
    let getter: GetTypeFn = unsafe { std::mem::transmute(addr) };
    let mut out: i32 = -1;
    unsafe { getter(&mut out) };
    (out >= 0).then_some(out)
}

/// The multiplier for this boot, or `None` when it is the identity.
fn factor() -> Option<f32> {
    let output_h = OUTPUT_H.load(Ordering::Acquire);
    let user = f32::from_bits(USER_SCALE_BITS.load(Ordering::Acquire));
    let is_sd = matches!(machine_type(), Some(0) | Some(1));
    plan::debug_ui_scale(output_h, is_sd, user)
}

unsafe extern "C" fn font_scale_detour(class: i32, sx: *mut f32, sy: *mut f32) {
    if let Some(hook) = (*addr_of!(FONT_DETOUR)).as_ref() {
        hook.call(class, sx, sy);
    } else {
        log_warn!(
            "CustomResolution: debug font-scale detour called without its original -- skipped"
        );
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        let Some(f) = factor() else { return };
        if sx.is_null()
            || sy.is_null()
            || !memory::is_readable(sx as *const u8, 4)
            || !memory::is_readable(sy as *const u8, 4)
        {
            return;
        }
        let (ox, oy) = (*sx, *sy);
        if !ox.is_finite() || !oy.is_finite() {
            return;
        }
        *sx = ox * f;
        *sy = oy * f;
        if !LOGGED_FONT.swap(true, Ordering::AcqRel) {
            log_info!(
                "CustomResolution: debug font class {} scale {:.3}x{:.3} -> {:.3}x{:.3} (factor {:.3}); further fonts are silent",
                class,
                ox,
                oy,
                *sx,
                *sy,
                f
            );
        }
    });
}

unsafe extern "C" fn sprite_create_detour(name: *const u8, out: *mut *mut u8) {
    if let Some(hook) = (*addr_of!(SPRITE_DETOUR)).as_ref() {
        hook.call(name, out);
    } else {
        log_warn!(
            "CustomResolution: debug sprite-create detour called without its original -- skipped"
        );
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        let Some(f) = factor() else { return };
        if out.is_null() || !memory::is_readable(out as *const u8, 8) {
            return;
        }
        let sprite = *out;
        if sprite.is_null() || !memory::is_readable(sprite, SPRITE_SCALE_OFF + 4) {
            return;
        }
        let slot = sprite.add(SPRITE_SCALE_OFF) as *mut f32;
        let old = *slot;
        if !old.is_finite() {
            return;
        }
        *slot = old * f;
        if !LOGGED_SPRITE.swap(true, Ordering::AcqRel) {
            log_info!(
                "CustomResolution: debug sprite scale {:.3} -> {:.3} (factor {:.3}); further sprites are silent",
                old,
                *slot,
                f
            );
        }
    });
}
