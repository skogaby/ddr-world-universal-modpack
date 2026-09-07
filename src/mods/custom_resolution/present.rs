//! The graphics-init detour and the PRESENT render-target fixup (design
//! §4.7). `graphics_init(display_struct*)` is the onBoot callee that builds
//! the D3D device, every render surface, the eight list viewports and the
//! present chain. We wrap it once, at `early_apply` time (before onBoot runs):
//!
//! - pre-original: write the plan's AA config into the display struct
//!   (`+0x18`; covers every onBoot branch — the `aa_config_imm` patch only
//!   reaches the pcType-2..4 `MOV [RSP+d],3` store, and 2×/4× MSAA need a
//!   value no imm patch can produce on the other branches) and log the
//!   struct's HD flag / AA config / FPS target;
//! - post-original: the PRESENT rt struct (`*(render_surfaces+0x80)`) was
//!   created with the ctor's 1280×720 immediates but is re-pointed at the
//!   OUTPUT-sized back-buffer every frame, so its `u16 w/h` become the
//!   output dims here. Its depth is left stock while the render covers the
//!   output (a depth surface larger than the colour target is legal — stock
//!   SD cabinets run exactly that); when the render is SMALLER than the
//!   output in either dimension (perf mode: 720p/1080p render on a 4K panel)
//!   D3D9 requires a depth at least as large as the colour target, so an
//!   output-sized depth is created and swapped in with the engine's own
//!   refcount idiom (plan Step 7 / R12): `new = surface_create(out.w, out.h,
//!   0x4b, 0, &{0,0})`; `old = rt+0x10`; `if old { rt+0x10 = 0; release(old) }`;
//!   `rt+0x10 = new; addref(new)` — byte-for-byte what the ctor does when it
//!   binds `render_depth` (20260825 `FUN_1801f10e0` @ +0xD13). A missing
//!   derivation nulls the depth instead (the PRESENT pass is a textured quad
//!   and needs no Z) with one WARN.
//!
//! Also the window fit: spice2x's `CreateWindowExW` hook (MDX, `-w`) replaces
//! the game's requested client size with a hard-coded 1280×720 (800×600 with
//! its `-o` SD flag) and its `SetWindowPos` IAT hook swallows every later
//! game resize, so a non-720p back-buffer would be stretched into a 720p
//! client. After init we compare the client rect with the output and, when
//! they differ, resize the client ourselves through `user32!SetWindowPos`
//! resolved by `GetProcAddress` (the real export, not the patched import).
//! Fullscreen and stock loaders already match and are left alone.
//!
//! Also the place the design's R14 check lives: after init the screen
//! globals must read the output dims; anything else means the game's
//! display-mode fallback (or spice2x) rewrote them, and that is worth one
//! WARN so a "wrong size" report is self-diagnosing.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::CustomResolutionAnchors;
use crate::{log_info, log_warn};

use super::plan::{AaPolicy, Dims, Plan, PresentDepth};

type GraphicsInitFn = unsafe extern "C" fn(*mut u8);

static mut DETOUR: Option<GenericDetour<GraphicsInitFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);
static FIXUP_DONE: AtomicBool = AtomicBool::new(false);

// Plan facts the detour needs, published before the hook is enabled.
static OUT_W: AtomicUsize = AtomicUsize::new(0);
static OUT_H: AtomicUsize = AtomicUsize::new(0);
static REN_W: AtomicUsize = AtomicUsize::new(0);
static REN_H: AtomicUsize = AtomicUsize::new(0);
static DEPTH_POLICY_CREATE: AtomicBool = AtomicBool::new(false);
/// AA config to force into the display struct pre-init; `AA_KEEP` = leave it.
static AA_FORCE: AtomicUsize = AtomicUsize::new(AA_KEEP);
const AA_KEEP: usize = usize::MAX;
static SURFACES_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static SURFACE_CREATE: AtomicUsize = AtomicUsize::new(0);
static DEPTH_RELEASE: AtomicUsize = AtomicUsize::new(0);
static DEPTH_ADDREF: AtomicUsize = AtomicUsize::new(0);
static SCREEN_W_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static SCREEN_H_GLOBAL: AtomicUsize = AtomicUsize::new(0);
/// HWND read from the display struct (+0x08) pre-original.
static GAME_HWND: AtomicUsize = AtomicUsize::new(0);

/// PRESENT rt struct = `*(surfaces + 0x80)`; struct layout `{+0x08 colour id,
/// +0x10 depth id, +0x14 u16 w, +0x16 u16 h, +0x18 u8 msaa}` (0x1C bytes).
const PRESENT_RT_OFF: usize = 0x80;
const RT_DEPTH_OFF: usize = 0x10;
const RT_W_OFF: usize = 0x14;
const RT_H_OFF: usize = 0x16;
const RT_LEN: usize = 0x1C;

/// The engine's depth-stencil surface format id (`render_depth` /
/// `depth A/B` in the surface ctor: `surface_create(0x500, 0x2d0, 0x4b)`).
const DEPTH_FORMAT: u32 = 0x4b;

/// `surface_create(u16 w, u16 h, u32 format, u32 msaa, opts*) -> u32 id`;
/// `opts` = `{u32, u8}` — the ctor passes a zeroed stack block for every
/// surface it creates (NULL would substitute a global default block whose
/// contents are not verified), so we pass the same zeroed block.
type SurfaceCreateFn = unsafe extern "C" fn(u32, u32, u32, u32, *const u8) -> u32;
/// `release(u32 id)` / `addref(u32 id)` — the surface refcount pair.
type SurfaceRefFn = unsafe extern "C" fn(u32);

/// Display-struct fields (0x20 bytes on onBoot's stack).
const DS_HWND: usize = 0x08;
const DS_HD_FLAG: usize = 0x12;
const DS_AA: usize = 0x18;
const DS_FPS: usize = 0x1C;

/// Install the detour. Requires `graphics_init` and `render_surfaces_global`;
/// the screen globals are optional (they only feed the R14 WARN).
pub fn install(anchors: &CustomResolutionAnchors, plan: &Plan) -> Result<(), String> {
    if INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let target = anchors.graphics_init.ok_or("graphics_init unresolved")?;
    let surfaces = anchors
        .render_surfaces_global
        .ok_or("render_surfaces_global unresolved")?;

    OUT_W.store(plan.output.w as usize, Ordering::Release);
    OUT_H.store(plan.output.h as usize, Ordering::Release);
    REN_W.store(plan.render.w as usize, Ordering::Release);
    REN_H.store(plan.render.h as usize, Ordering::Release);
    DEPTH_POLICY_CREATE.store(
        plan.present_depth == PresentDepth::CreateOutputSized,
        Ordering::Release,
    );
    AA_FORCE.store(
        match plan.aa {
            AaPolicy::Stock => AA_KEEP,
            AaPolicy::Force(v) => v as usize,
        },
        Ordering::Release,
    );
    SURFACES_GLOBAL.store(surfaces as usize, Ordering::Release);
    SURFACE_CREATE.store(
        anchors.surface_create.map_or(0, |p| p as usize),
        Ordering::Release,
    );
    DEPTH_RELEASE.store(
        anchors.present_depth_release.map_or(0, |p| p as usize),
        Ordering::Release,
    );
    DEPTH_ADDREF.store(
        anchors.present_depth_addref.map_or(0, |p| p as usize),
        Ordering::Release,
    );
    SCREEN_W_GLOBAL.store(
        anchors.screen_w_global.map_or(0, |p| p as usize),
        Ordering::Release,
    );
    SCREEN_H_GLOBAL.store(
        anchors.screen_h_global.map_or(0, |p| p as usize),
        Ordering::Release,
    );

    unsafe {
        let target: GraphicsInitFn = std::mem::transmute(target);
        hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            graphics_init_detour as GraphicsInitFn,
        )
        .map_err(|e| format!("graphics_init detour install failed: {e}"))?;
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!("CustomResolution: graphics_init detour installed");
    Ok(())
}

/// True once the post-init fixup ran (diagnostic for the mod's status line).
pub fn fixup_done() -> bool {
    FIXUP_DONE.load(Ordering::Acquire)
}

unsafe extern "C" fn graphics_init_detour(display: *mut u8) {
    let _ = std::panic::catch_unwind(|| {
        if !display.is_null() && memory::is_readable(display, 0x20) {
            GAME_HWND.store(
                memory::read_u64(display.add(DS_HWND)) as usize,
                Ordering::Release,
            );
            let aa_before = memory::read_u32(display.add(DS_AA));
            let force = AA_FORCE.load(Ordering::Acquire);
            if force != AA_KEEP && aa_before != force as u32 {
                memory::write_u32(display.add(DS_AA) as *mut u8, force as u32);
            }
            log_info!(
                "CustomResolution: graphics_init display struct: hd_flag={} aa_config={} (onBoot chose {}) fps={}",
                memory::read_u8(display.add(DS_HD_FLAG)),
                memory::read_u32(display.add(DS_AA)),
                aa_before,
                memory::read_u32(display.add(DS_FPS))
            );
        }
    });

    if let Some(hook) = (*addr_of!(DETOUR)).as_ref() {
        hook.call(display);
    }

    let _ = std::panic::catch_unwind(|| fixup());
    let _ = std::panic::catch_unwind(|| fit_window());
}

/// Resize the game window's CLIENT area to the output dims when a loader
/// (spice2x `-w`) pinned it elsewhere. No-op when it already matches (the
/// fullscreen / stock-loader case) or the HWND is unknown.
#[cfg(windows)]
unsafe fn fit_window() {
    use windows::core::PCSTR;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, GetClientRect, GetMenu, GetWindowLongPtrW, GWL_EXSTYLE, GWL_STYLE,
        SET_WINDOW_POS_FLAGS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
        WINDOW_EX_STYLE, WINDOW_STYLE,
    };

    let hwnd = GAME_HWND.load(Ordering::Acquire);
    if hwnd == 0 {
        return;
    }
    let hwnd = HWND(hwnd as *mut _);
    let out = Dims::new(
        OUT_W.load(Ordering::Acquire) as u32,
        OUT_H.load(Ordering::Acquire) as u32,
    );
    let mut client = RECT::default();
    if GetClientRect(hwnd, &mut client).is_err() {
        return;
    }
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);
    if cw == out.w as i32 && ch == out.h as i32 {
        log_info!("CustomResolution: window client already {}x{}", cw, ch);
        return;
    }

    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    let has_menu = !GetMenu(hwnd).0.is_null();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: out.w as i32,
        bottom: out.h as i32,
    };
    if AdjustWindowRectEx(
        &mut rect,
        WINDOW_STYLE(style),
        has_menu,
        WINDOW_EX_STYLE(ex_style),
    )
    .is_err()
    {
        log_warn!(
            "CustomResolution: AdjustWindowRectEx failed -- window left at {}x{}",
            cw,
            ch
        );
        return;
    }
    let (ww, wh) = (rect.right - rect.left, rect.bottom - rect.top);

    // Real user32 export, bypassing any loader's IAT hook on our import.
    type SetWindowPosFn = unsafe extern "system" fn(HWND, HWND, i32, i32, i32, i32, u32) -> i32;
    let Ok(user32) = GetModuleHandleA(PCSTR(b"user32.dll\0".as_ptr())) else {
        return;
    };
    let Some(proc_addr) = GetProcAddress(user32, PCSTR(b"SetWindowPos\0".as_ptr())) else {
        return;
    };
    let set_window_pos: SetWindowPosFn = std::mem::transmute(proc_addr);
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
    let ok = set_window_pos(hwnd, HWND::default(), 0, 0, ww, wh, flags.0);
    let mut after = RECT::default();
    let _ = GetClientRect(hwnd, &mut after);
    log_info!(
        "CustomResolution: window client {}x{} -> requested {}x{} (SetWindowPos={}, now {}x{}) -- a loader pinned the window size",
        cw,
        ch,
        out.w,
        out.h,
        ok,
        after.right - after.left,
        after.bottom - after.top
    );
}

#[cfg(not(windows))]
unsafe fn fit_window() {}

/// Swap the PRESENT rt's depth for an output-sized one (render < output).
/// Returns the text for the fixup INFO. On a missing derivation or a failed
/// create the depth is NULLED (safe: the PRESENT pass draws a textured quad
/// with Z disabled) and one WARN names the reason.
unsafe fn replace_depth(rt10: *mut u8, out: Dims) -> String {
    let depth_slot = rt10.add(RT_DEPTH_OFF) as *mut u32;
    let old = *depth_slot;
    let create = SURFACE_CREATE.load(Ordering::Acquire);
    let release = DEPTH_RELEASE.load(Ordering::Acquire);
    let addref = DEPTH_ADDREF.load(Ordering::Acquire);
    let missing = [
        (create, "surface_create"),
        (release, "present_depth_release"),
        (addref, "present_depth_addref"),
    ]
    .iter()
    .filter(|(p, _)| *p == 0)
    .map(|(_, n)| *n)
    .collect::<Vec<_>>();
    if !missing.is_empty() {
        *depth_slot = 0;
        log_warn!(
            "CustomResolution: PRESENT depth NULLED (was id 0x{old:X}) -- {} unresolved, no output-sized depth",
            missing.join("/")
        );
        return format!("nulled -- {} unresolved", missing.join("/"));
    }
    let create: SurfaceCreateFn = std::mem::transmute(create);
    let release: SurfaceRefFn = std::mem::transmute(release);
    let addref: SurfaceRefFn = std::mem::transmute(addref);

    let opts = [0u8; 8];
    let new = create(out.w, out.h, DEPTH_FORMAT, 0, opts.as_ptr());
    if new == 0 {
        *depth_slot = 0;
        log_warn!(
            "CustomResolution: surface_create({}x{}, depth) returned 0 -- PRESENT depth NULLED (was id 0x{old:X})",
            out.w,
            out.h
        );
        return "nulled -- output-sized create failed".to_string();
    }
    if old != 0 && old != new {
        *depth_slot = 0;
        release(old);
    }
    *depth_slot = new;
    addref(new);
    format!(
        "replaced: id 0x{old:X} (render-sized) -> 0x{new:X} ({}x{} output-sized)",
        out.w, out.h
    )
}

/// Post-init: PRESENT rt dims → output; screen-global sanity check.
unsafe fn fixup() {
    if FIXUP_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let out = Dims::new(
        OUT_W.load(Ordering::Acquire) as u32,
        OUT_H.load(Ordering::Acquire) as u32,
    );
    let ren = Dims::new(
        REN_W.load(Ordering::Acquire) as u32,
        REN_H.load(Ordering::Acquire) as u32,
    );

    let surfaces_global = SURFACES_GLOBAL.load(Ordering::Acquire) as *const *const u8;
    let surfaces = if memory::is_readable(surfaces_global as *const u8, 8) {
        *surfaces_global
    } else {
        std::ptr::null()
    };
    if surfaces.is_null() || !memory::is_readable(surfaces, PRESENT_RT_OFF + 8) {
        log_warn!("CustomResolution: render-surface object unreadable after graphics_init -- PRESENT rt not fixed (top-left 1280x720 present expected)");
    } else {
        let rt10 = *(surfaces.add(PRESENT_RT_OFF) as *const *mut u8);
        if rt10.is_null() || !memory::is_readable(rt10, RT_LEN) {
            log_warn!("CustomResolution: PRESENT rt struct unreadable -- not fixed");
        } else {
            let w = *(rt10.add(RT_W_OFF) as *const u16) as u32;
            let h = *(rt10.add(RT_H_OFF) as *const u16) as u32;
            if w == ren.w && h == ren.h {
                if out != ren {
                    *(rt10.add(RT_W_OFF) as *mut u16) = out.w as u16;
                    *(rt10.add(RT_H_OFF) as *mut u16) = out.h as u16;
                }
                let depth = if DEPTH_POLICY_CREATE.load(Ordering::Acquire) {
                    replace_depth(rt10, out)
                } else {
                    "stock, render covers output".to_string()
                };
                log_info!(
                    "CustomResolution: PRESENT rt dims {}x{} -> {}x{} (depth {})",
                    w,
                    h,
                    out.w,
                    out.h,
                    depth
                );
            } else {
                log_warn!(
                    "CustomResolution: PRESENT rt reads {}x{}, expected render {}x{} -- layout gate failed, not fixed",
                    w,
                    h,
                    ren.w,
                    ren.h
                );
            }
        }
    }

    let sw = SCREEN_W_GLOBAL.load(Ordering::Acquire) as *const u8;
    let sh = SCREEN_H_GLOBAL.load(Ordering::Acquire) as *const u8;
    if !sw.is_null() && !sh.is_null() && memory::is_readable(sw, 4) && memory::is_readable(sh, 4) {
        let (gw, gh) = (memory::read_u32(sw), memory::read_u32(sh));
        if gw == out.w && gh == out.h {
            log_info!("CustomResolution: screen globals confirm {}x{}", gw, gh);
        } else {
            log_warn!(
                "CustomResolution: screen globals read {}x{} but {}x{} was requested -- the game's display-mode fallback (or a spice2x -forceres/-windowresize override) changed the back-buffer",
                gw,
                gh,
                out.w,
                out.h
            );
        }
    }
}
