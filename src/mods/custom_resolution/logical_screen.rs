//! The "logical screen" — what the game's APP layer sees as the screen size
//! (design §4.9 v3 / R6, supersedes the root-7 re-canvas and the widget
//! write-scaling; cabinet-derived 2026-09-05, see the planning record).
//!
//! The engine keeps a per-display info block (`DAT_1806f20d8` on 20260616;
//! `{u32 w, u32 h, …, +0x30 swap chain, +0x38 back-buffer}`) whose pointer
//! global is read by ~38 sites. They fall into three families:
//!
//! - **Physical** — the renderer/device layer: surface creation, the
//!   letterbox rect, the SYSTEM/DEBUG list viewports, the per-frame
//!   back-buffer bind (`+0x38`), the gd device layer (`+0x30`, per-display
//!   `idx*0x40` indexing, the gd lock). These MUST see the real back-buffer.
//! - **Design** — what the app positions "on the screen" INSIDE the layer
//!   roots: the layer set-size loop (roots 1/3/5/6/7 ← `screen w/h` via the
//!   two getter functions), the footer/version/attract text (`screen × 0.5`,
//!   `screen − const`), the "NOW LOADING" text. On a stock 720p cabinet
//!   "screen" == the 1280×720 design space these were authored against; on
//!   a 640×480 SD cabinet the same code ran against a 640×480 screen and
//!   Konami shipped `_sd` layouts for it — World dropped those, so SD/1080p/
//!   4K all show design-authored content cropped or misplaced in
//!   screen-sized roots. Feeding this family a constant **1280×720** makes
//!   every screen root a 1280×720 canvas (the walker scales the canvas to
//!   the physical viewport) and every screen-px position a design-px
//!   position — the whole app layer becomes resolution-independent in one
//!   move, with zero per-object fixes.
//!   NOT in this family (physical, like the renderer): the ark draw-callback
//!   API (`drawFont`/`drawSprite`/`drawLine`/… — `screen × pct / 100`, the
//!   hardware-check / TEST-menu / error screens the ark drives) and the
//!   system font (`screen × x / design`) — both draw into a screen-sized
//!   bare list whose walker context IS the physical viewport, so they
//!   scale by construction and broke when fed 1280×720 (cabinet, 2026-09-05
//!   run 4: hardware check no longer centred).
//! - **Render** — the AFP callbacks (projection matrix, render-ctx rect,
//!   BM2DGroup rect): a half-pixel correction that must match the RENDER
//!   target (register D19), i.e. `render.w/h` — equal to the physical size
//!   in Tier B and to 1280×720 in Tier A / SD.
//!
//! Mechanism: every `MOV r64,[RIP+disp32]` load of the info POINTER is
//! classified by content (CONSERVATIVE — a misrouted physical reader is a
//! black screen or a crash, a missed app reader is a cosmetic misplacement)
//! and its disp32 redirected to a mod-owned pointer slot → a fake 0x40 block
//! carrying only `{w, h}`. Anchored exclusions: the letterbox load (exact),
//! the surface-ctor body, the list-viewport builder. Family rules verified
//! offline on 20250805 / 20260224 / 20260721 / 20260825 (17 physical loads
//! left untouched on every build; design 17–18; render 4).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::core::memory;
use crate::core::scanner;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::plan::{Dims, STOCK};

static DONE: AtomicBool = AtomicBool::new(false);

/// `4C 8B 05 disp32` (MOV R8,[RIP+disp]) at this offset inside
/// `letterbox_rect_fn` loads the info pointer — the derivation source and
/// the first anchored exclusion.
const LETTERBOX_INFO_LOAD_OFF: usize = 0x16;
/// The surface ctor spans ~0x1200 bytes around the `render_surface_hoist`
/// match (two physical loads: the display surface create and the present
/// quad's half-pixel maths — the latter converts to float and would
/// otherwise classify as design).
const SURFACE_CTOR_BACK: usize = 0x400;
const SURFACE_CTOR_FWD: usize = 0x1400;
/// The list-viewport builder around `list_viewport_table` (SYSTEM /
/// DEBUG_DIALOG dims — physical).
const LIST_VP_BACK: usize = 0x200;
const LIST_VP_FWD: usize = 0x400;
/// The layer-table builder sits right before `layer_dispatcher`; its one
/// info load is the BM2DGroup ctor rect (+0x24/+0x28) → render family.
const LAYER_BUILDER_BACK: usize = 0x600;
/// The AFP callback module follows `wrapper_render`; its three info loads
/// (projection matrix, get-screen-rect, ctx reset) → render family.
const AFP_MODULE_START: usize = 0x800;
const AFP_MODULE_END: usize = 0x2000;
/// Content windows.
const FLOAT_WINDOW: usize = 0x80;
const PAIR_WINDOW: usize = 0x40;
/// The ark draw callbacks reference the `100.0f` percentage divisor within
/// this window around the load (before or after — the compiler hoists it).
const PCT_BACK: usize = 0x60;
const PCT_FWD: usize = 0x80;

/// Exact family sizes that must hold before ANY write (offline, all four
/// builds: design 4 = 2 getters + footer + loading text; render 4; the ark
/// percentage drawers 10–11 and the system font 3 stay physical).
const EXPECT_DESIGN: usize = 4;
const EXPECT_GETTERS: usize = 2;
const EXPECT_RENDER: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Design,
    Render,
}

struct Load {
    /// Address of the REX byte of the 7-byte `MOV r64,[RIP+disp32]`.
    insn: *const u8,
    family: Family,
    getter: bool,
}

/// Install the redirects for the given render size. Idempotent; any
/// classification anomaly ⇒ one WARN and NO writes.
pub fn install(sigs: &SignatureStore, module_base: *const u8, module_size: usize, render: Dims) {
    if DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(lb) = sigs.get_address("letterbox_rect_fn") else {
        log_warn!("CustomResolution: letterbox_rect_fn unresolved -- logical screen not installed (app content stays in screen pixels)");
        return;
    };
    let Some(info_global) = (unsafe { display_info_global(lb) }) else {
        log_warn!("CustomResolution: per-display-info global not decodable -- logical screen not installed");
        return;
    };
    let (Some(hoist), Some(lvt), Some(dispatcher), Some(wrapper_render)) = (
        sigs.get_address("render_surface_hoist"),
        sigs.get_address("list_viewport_table"),
        sigs.get_address("layer_dispatcher"),
        sigs.get_address("wrapper_render"),
    ) else {
        log_warn!("CustomResolution: an anchor for the logical-screen classifier is unresolved -- not installed");
        return;
    };

    let anchors = Anchors {
        letterbox_load: unsafe { lb.add(LETTERBOX_INFO_LOAD_OFF) } as usize,
        ctor: (
            (hoist as usize).saturating_sub(SURFACE_CTOR_BACK),
            hoist as usize + SURFACE_CTOR_FWD,
        ),
        list_vp: (
            (lvt as usize).saturating_sub(LIST_VP_BACK),
            lvt as usize + LIST_VP_FWD,
        ),
        layer_builder: (
            (dispatcher as usize).saturating_sub(LAYER_BUILDER_BACK),
            dispatcher as usize,
        ),
        afp: (
            wrapper_render as usize + AFP_MODULE_START,
            wrapper_render as usize + AFP_MODULE_END,
        ),
    };

    let loads = unsafe { classify(module_base, module_size, info_global, &anchors) };
    let getters = loads.iter().filter(|l| l.getter).count();
    let design = loads.iter().filter(|l| l.family == Family::Design).count();
    let render_n = loads.iter().filter(|l| l.family == Family::Render).count();
    if getters != EXPECT_GETTERS || render_n != EXPECT_RENDER || design != EXPECT_DESIGN {
        log_warn!(
            "CustomResolution: logical-screen classifier off-shape (getters {getters}/{EXPECT_GETTERS}, render {render_n}/{EXPECT_RENDER}, design {design}/{EXPECT_DESIGN}) -- not installed"
        );
        return;
    }

    // Fake blocks (only w/h are ever read through the redirected loads) and
    // the two pointer slots the redirected disp32s reach.
    let design_block = unsafe { memory::alloc_zeroed(0x40) };
    let render_block = unsafe { memory::alloc_zeroed(0x40) };
    let slots = unsafe { memory::alloc_near(module_base, 16) };
    if design_block.is_null() || render_block.is_null() || slots.is_null() {
        log_warn!("CustomResolution: logical-screen allocation failed -- not installed");
        return;
    }
    unsafe {
        memory::write_u32(design_block, STOCK.w);
        memory::write_u32(design_block.add(4), STOCK.h);
        memory::write_u32(render_block, render.w);
        memory::write_u32(render_block.add(4), render.h);
        memory::write_ptr(slots, design_block);
        memory::write_ptr(slots.add(8), render_block);
    }

    let mut n = 0usize;
    for l in &loads {
        let slot = match l.family {
            Family::Design => slots,
            Family::Render => unsafe { slots.add(8) },
        };
        let disp_addr = unsafe { l.insn.add(3) as *mut u8 };
        let rip = unsafe { l.insn.add(7) } as isize;
        let new_disp = (slot as isize) - rip;
        if new_disp > i32::MAX as isize || new_disp < i32::MIN as isize {
            log_warn!(
                "CustomResolution: logical-screen redirect out of rel32 range -- site skipped"
            );
            continue;
        }
        let old = unsafe { memory::read_u32(disp_addr) }.to_le_bytes();
        let new = (new_disp as i32).to_le_bytes();
        match unsafe { memory::apply_checked_patch(disp_addr, &old, &new) } {
            Ok(()) => n += 1,
            Err(e) => log_warn!("CustomResolution: logical-screen redirect failed: {e:?}"),
        }
    }
    log_info!(
        "CustomResolution: logical screen installed -- {} app-layer load(s) read 1280x720 (design), {} AFP load(s) read {}x{} (render); {} untouched physical",
        design,
        render_n,
        render.w,
        render.h,
        n_physical(module_base, module_size, info_global, &loads)
    );
    let _ = n;
}

struct Anchors {
    letterbox_load: usize,
    ctor: (usize, usize),
    list_vp: (usize, usize),
    layer_builder: (usize, usize),
    afp: (usize, usize),
}

unsafe fn display_info_global(lb: *const u8) -> Option<*const u8> {
    let p = lb.add(LETTERBOX_INFO_LOAD_OFF);
    if !memory::is_readable(p, 7) || *p != 0x4C || *p.add(1) != 0x8B || *p.add(2) != 0x05 {
        return None;
    }
    Some(scanner::decode_rip_relative(p.add(3)))
}

/// Any sign the window belongs to the device layer: `SHL r,6` (per-display
/// `idx*0x40` indexing), `LOCK XADD` (the gd lock idiom), or a `[reg+0x30]`
/// / `[reg+0x38]` field access (swap chain / back-buffer handle).
fn has_device_marker(w: &[u8]) -> bool {
    for i in 0..w.len().saturating_sub(2) {
        if w[i] == 0xC1 && (0xE0..=0xE7).contains(&w[i + 1]) && w[i + 2] == 0x06 {
            return true;
        }
        if w[i] == 0xF0 && w[i + 1] == 0x0F && w[i + 2] == 0xC1 {
            return true;
        }
    }
    for i in 1..w.len().saturating_sub(1) {
        let op = w[i - 1];
        let modrm = w[i];
        let disp = w[i + 1];
        let field = disp == 0x30 || disp == 0x38;
        if field && (modrm & 0xC0) == 0x40 && matches!(op, 0x8B | 0x89 | 0x39 | 0x3B) {
            return true;
        }
    }
    false
}

/// A RIP-relative scalar-SSE operand (`F3 0F {10,58,59,5C,5E} modrm(rm=101)
/// disp32`) whose f32 target reads `100.0` — the ark draw callbacks' percentage
/// divisor. `w` starts `back` bytes before the load; `w_addr` is its address.
unsafe fn references_percent_divisor(w: &[u8], w_addr: *const u8) -> bool {
    for i in 0..w.len().saturating_sub(8) {
        if w[i] == 0xF3
            && w[i + 1] == 0x0F
            && matches!(w[i + 2], 0x10 | 0x58 | 0x59 | 0x5C | 0x5E)
            && (w[i + 3] & 0xC7) == 0x05
        {
            let disp = i32::from_le_bytes([w[i + 4], w[i + 5], w[i + 6], w[i + 7]]);
            let target = w_addr.add(i + 8).offset(disp as isize);
            if memory::is_readable(target, 4) && memory::read_f32(target) == 100.0 {
                return true;
            }
        }
    }
    false
}

/// `DIVSS xmm,[reg+0x74]` / `[reg+0x78]` — the system-font manager's design
/// size divisor (`screen × x / design`).
fn divides_by_sysfont_design(w: &[u8]) -> bool {
    w.windows(5).any(|x| {
        x[0] == 0xF3
            && x[1] == 0x0F
            && x[2] == 0x5E
            && (x[3] & 0xC0) == 0x40
            && (x[4] == 0x74 || x[4] == 0x78)
    })
}

fn has_float_use(w: &[u8]) -> bool {
    w.windows(2).any(|x| x == [0x0F, 0x2A] || x == [0x0F, 0x5B])
}

/// `MOV r32,[reg+4]` — the second half of a direct `{w, h}` pair read.
fn has_pair_read(w: &[u8]) -> bool {
    w.windows(3)
        .any(|x| x[0] == 0x8B && (0x40..=0x47).contains(&x[1]) && x[2] == 0x04)
}

unsafe fn classify(base: *const u8, size: usize, info: *const u8, a: &Anchors) -> Vec<Load> {
    let mut out = Vec::new();
    if !memory::is_readable(base, size) {
        return out;
    }
    let b = std::slice::from_raw_parts(base, size);
    let mut i = 0;
    while i + 7 <= b.len() {
        let rex = b[i];
        if (rex & 0xF8) != 0x48 || b[i + 1] != 0x8B || (b[i + 2] & 0xC7) != 0x05 {
            i += 1;
            continue;
        }
        let disp = i32::from_le_bytes([b[i + 3], b[i + 4], b[i + 5], b[i + 6]]);
        let target = base.add(i + 7).offset(disp as isize);
        if target != info {
            i += 1;
            continue;
        }
        let addr = base as usize + i;
        let win = &b[i + 7..(i + 7 + FLOAT_WINDOW).min(b.len())];
        let w40 = &win[..win.len().min(PAIR_WINDOW)];
        let getter = win.len() >= 4
            && (win[..3] == [0x8B, 0x00, 0xC3] || win[..4] == [0x8B, 0x40, 0x04, 0xC3]);
        let in_range = |r: (usize, usize)| r.0 <= addr && addr < r.1;
        let anchored_physical = addr == a.letterbox_load || in_range(a.ctor) || in_range(a.list_vp);

        let family = if getter {
            Some(Family::Design)
        } else if anchored_physical {
            None
        } else if in_range(a.layer_builder) {
            // The BM2DGroup ctor: `+0x24 = w; +0x28 = h` — a plain pair read
            // (its `+0x30 = prio` store trips the device-marker heuristic,
            // which the anchor makes unnecessary here).
            if has_pair_read(w40) {
                Some(Family::Render)
            } else {
                None
            }
        } else if in_range(a.afp) {
            // Render family only when the content is a plain w/h consumer.
            if !has_device_marker(w40) && (has_float_use(win) || has_pair_read(w40)) {
                Some(Family::Render)
            } else {
                None
            }
        } else if {
            let back = i.min(PCT_BACK);
            let pw = &b[i - back..(i + 7 + PCT_FWD).min(b.len())];
            references_percent_divisor(pw, base.add(i - back))
        } {
            None // ark draw callback (percentage drawer) — physical canvas
        } else if divides_by_sysfont_design(w40) {
            None // system font — physical canvas
        } else if has_float_use(win) && !has_device_marker(w40) {
            Some(Family::Design)
        } else {
            None
        };
        if let Some(family) = family {
            out.push(Load {
                insn: base.add(i),
                family,
                getter,
            });
        }
        i += 7;
    }
    out
}

/// Diagnostic count of info loads NOT redirected.
fn n_physical(base: *const u8, size: usize, info: *const u8, redirected: &[Load]) -> usize {
    let mut total = 0usize;
    unsafe {
        let b = std::slice::from_raw_parts(base, size);
        let mut i = 0;
        while i + 7 <= b.len() {
            if (b[i] & 0xF8) == 0x48 && b[i + 1] == 0x8B && (b[i + 2] & 0xC7) == 0x05 {
                let disp = i32::from_le_bytes([b[i + 3], b[i + 4], b[i + 5], b[i + 6]]);
                // After patching, redirected loads no longer point at `info`;
                // count those that still do.
                if base.add(i + 7).offset(disp as isize) == info {
                    total += 1;
                }
                i += 7;
                continue;
            }
            i += 1;
        }
    }
    let _ = redirected;
    total
}
