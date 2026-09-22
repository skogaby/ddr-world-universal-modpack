//! The viewport-pass compositor (design §3.1 / §4.5): render the scene graph
//! into a sub-rectangle ABOVE every 2D layer without touching the stock
//! passes or camera slot 0.
//!
//! The engine draws each frame as an ordered list of target lists; a target
//! list is a plain `vector<{viewport*, prio}>` (attach = push + sort, detach
//! = erase — the game's own `FUN_1802666c0` / `FUN_1802667d0`). Before each
//! viewport's render callback the worker applies THAT viewport's D3D rect
//! and, unless its flags bit1 is set, uploads THAT viewport's view/proj. The
//! three MODEL passes are such viewports in RENDER-3D (the frame floor); this
//! module attaches, into RENDER_2D at priorities after the three AFP layer
//! lists (0x65..0x67):
//!
//! 1. a mod-owned [`ClearViewport`] (bit1 set) whose slot-0 callback appends
//!    one gd Clear record — D3D9 clears the CURRENT viewport = our box;
//! 2. a byte-clone of the stock OPACITY pass and one of the TRANS pass
//!    (`pass_size` bytes each): self back-pointer re-pointed, rect = the box
//!    in render-target pixels, node-mask FILTER = a private bit no stock pass
//!    uses (`0x08` P1 / `0x20` P2), view/proj written by the owner every
//!    frame. They share the graph's render-item list with the stock passes
//!    and draw only the items stamped with their bit.
//!
//! ## Threads
//!
//! `create` / `set_*` / `detach` / `reap` / `render_target_dims` are GAME
//! THREAD ONLY and must run from an `input_manager::on_frame` callback
//! (mid-frame: the frame-end dispatch drained the workers, so no viewport
//! object is being read while we mutate or unlink it). A detached object is
//! freed by [`reap`] two frames later. The Clear callback runs on the
//! engine's render worker under the `node_visit` rules: no engine API, no
//! allocation, no locks, no logging, `catch_unwind`-wrapped.
//!
//! Every offset comes from `Scene3dSites.viewport` (the optional
//! `scene3d_resolve_viewport` sub-group); without it [`is_available`] is
//! false and every entry point is an inert `None`.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::Scene3dViewportSites;
use crate::{log_info, log_warn};

use super::camera_math::Mat4;
use super::sites;
pub use super::viewport_pass_layout::{
    encode_clear_record, private_bits_free, ClearSpec, ClearViewport, RtRect, CLEAR_RECORD_SIZE,
    CLEAR_VIEWPORT_SIZE, FILTER_BIT, PRIO_BASE, VP_FLAG_DISABLED, VP_FLAG_SKIP_CAMERA,
};

/// `void attach(list, viewport, u32 prio)` — the engine's push + sort.
type AttachFn = unsafe extern "C" fn(list: *mut u8, viewport: *mut u8, prio: u32);
/// `void detach(list, viewport)` — the engine's erase.
type DetachFn = unsafe extern "C" fn(list: *mut u8, viewport: *mut u8);

/// Frames a detached set must survive before its blocks are freed (the
/// worker may still hold a pointer from the frame of detachment).
pub const REAPER_FRAMES: u64 = 2;

/// Tri-state availability cache: 0 unchecked, 1 available, 2 unavailable.
static AVAILABLE: AtomicU8 = AtomicU8::new(0);
/// The worker-ctx gd write-pointer offset for the render callback (which
/// must not call `sites()` — a plain load only).
static GD_WRITE_OFF: AtomicUsize = AtomicUsize::new(0);
/// `[COL, slot0 render, slot1 dtor]` — installed pointer = image + 8.
static VTABLE: OnceLock<usize> = OnceLock::new();
/// Frame counter for the reaper (advanced by [`reap`]).
static FRAME: AtomicU64 = AtomicU64::new(0);
/// Detached sets awaiting their two-frame grace: `(blocks, detached_at)`;
/// blocks are stored as addresses (mod-owned allocations, game-thread only).
static REAPER: Mutex<Vec<(Vec<usize>, u64)>> = Mutex::new(Vec::new());

fn vp_sites() -> Option<Scene3dViewportSites> {
    sites()?.viewport
}

/// Slot 0 of the clear viewport's vtable: `render(viewport, workerCtx)`.
/// Render worker: append one Clear record at the ctx's gd write pointer.
unsafe extern "C" fn clear_render(vp: *mut ClearViewport, ctx: *mut u8) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if vp.is_null() || ctx.is_null() {
            return;
        }
        let off = GD_WRITE_OFF.load(Ordering::Relaxed);
        if off == 0 {
            return;
        }
        let slot = ctx.add(off) as *mut *mut u8;
        let p = *slot;
        if p.is_null() {
            return;
        }
        let v = &*vp;
        let rec = encode_clear_record(&ClearSpec {
            depth: v.clear_flags & super::viewport_pass_layout::D3DCLEAR_ZBUFFER != 0,
            color: (v.clear_flags & super::viewport_pass_layout::D3DCLEAR_TARGET != 0)
                .then_some(v.color_argb),
        });
        std::ptr::copy_nonoverlapping(rec.as_ptr(), p, CLEAR_RECORD_SIZE);
        *slot = p.add(CLEAR_RECORD_SIZE);
    }));
}

/// Slot 1: dtor — never invoked by the engine on our objects (we detach and
/// free them ourselves); a no-op for safety.
unsafe extern "C" fn clear_dtor(_vp: *mut ClearViewport, _free: u8) {}

fn vtable() -> *const *const u8 {
    let p = *VTABLE.get_or_init(|| {
        // SAFETY: fresh RWX block, written before publication.
        unsafe {
            let raw = memory::alloc_zeroed(3 * 8);
            if raw.is_null() {
                return 0;
            }
            let slots = raw as *mut *const u8;
            *slots = std::ptr::null(); // [-1] COL
            *slots.add(1) = clear_render as *const u8;
            *slots.add(2) = clear_dtor as *const u8;
            slots.add(1) as usize
        }
    });
    p as *const *const u8
}

/// Why a stock pass did not pass its gate.
enum PassGate {
    /// The pass global is still NULL — the engine's render-graph boot has
    /// not constructed the passes yet (mod enable runs before it). Not an
    /// error: ask again later.
    NotYet,
    /// The pointer / vftable / self back-pointer do not match the derivation.
    Mismatch,
}

/// The live stock pass object behind `global`, probed: `Ok(pass)` when the
/// pointer is readable for `pass_size` bytes and carries the pass vftable
/// at the viewport sub-object and its own address at the self slot.
unsafe fn stock_pass(v: &Scene3dViewportSites, global: *const u8) -> Result<*mut u8, PassGate> {
    if !memory::is_readable(global, 8) {
        return Err(PassGate::Mismatch);
    }
    let pass = memory::read_ptr(global) as *mut u8;
    if pass.is_null() {
        return Err(PassGate::NotYet);
    }
    if !memory::is_readable(pass, v.pass_size) {
        return Err(PassGate::Mismatch);
    }
    if memory::read_ptr(pass.add(v.sub_off)) != v.pass_vftable {
        return Err(PassGate::Mismatch);
    }
    if memory::read_ptr(pass.add(v.pass_self_off)) != pass as *const u8 {
        return Err(PassGate::Mismatch);
    }
    Ok(pass)
}

/// The compositor's availability this boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Derived, the four live stock passes verified, private bits free.
    Available,
    /// Structurally refused (one WARN, latched for the boot).
    Unavailable,
    /// The stock pass objects do not exist YET (the render-graph boot has
    /// not run — mod enable is earlier than that); not latched, no WARN.
    /// They exist long before the first scene is shown.
    NotYet,
}

/// Whether the viewport sub-group derived at all (the boot-time question;
/// the live pass check is [`availability`]).
pub fn derivation_present() -> bool {
    vp_sites().is_some()
}

/// Whether the compositor can run this boot: the viewport sub-group derived
/// AND the four live stock passes are what the derivation described AND
/// their filters leave the two private bits clear. A positive or a
/// structural negative is latched (one WARN on refusal); a "not yet"
/// (null pass globals during boot) is re-checked on the next call.
pub fn is_available() -> bool {
    availability() == Availability::Available
}

/// [`is_available`] with the transient state visible to the caller.
pub fn availability() -> Availability {
    match AVAILABLE.load(Ordering::Acquire) {
        1 => return Availability::Available,
        2 => return Availability::Unavailable,
        _ => {}
    }
    let a = check_available();
    match a {
        Availability::Available => AVAILABLE.store(1, Ordering::Release),
        Availability::Unavailable => AVAILABLE.store(2, Ordering::Release),
        Availability::NotYet => {}
    }
    a
}

/// The "not yet" INFO — once per boot.
static NOT_YET_LOGGED: AtomicU8 = AtomicU8::new(0);

fn check_available() -> Availability {
    let Some(v) = vp_sites() else {
        log_warn!("viewport_pass: unavailable -- the scene3d viewport sub-group did not derive");
        return Availability::Unavailable;
    };
    // "Booted" = the display's RENDER_2D list exists (the same render-graph
    // boot constructs the passes and the lists). Before it, nothing below can
    // be judged.
    let booted = unsafe { render2d_list(&v).is_some() };
    let mut filters = [0u32; 4];
    for (i, g) in v.pass_globals.iter().enumerate() {
        // SAFETY: probed reads of the derived globals / pass objects.
        match unsafe { stock_pass(&v, *g) } {
            Ok(pass) => filters[i] = unsafe { memory::read_u32(pass.add(v.pass_filter_off)) },
            Err(gate) => {
                let not_yet = matches!(gate, PassGate::NotYet) || !booted;
                if not_yet {
                    if NOT_YET_LOGGED.swap(1, Ordering::AcqRel) == 0 {
                        log_info!(
                            "viewport_pass: stock pass {} not constructed yet (render-graph boot pending) -- availability re-checked at first use",
                            i
                        );
                    }
                    return Availability::NotYet;
                }
                log_warn!(
                    "viewport_pass: unavailable -- stock pass {} (DISTANT/OPACITY/LOWPRIO/TRANS order) failed its identity gate",
                    i
                );
                return Availability::Unavailable;
            }
        }
    }
    if filters[1] != 0x56 || filters[3] != 0x46 {
        log_warn!(
            "viewport_pass: unavailable -- OPACITY/TRANS filters {:#x}/{:#x} are not the stock 0x56/0x46",
            filters[1],
            filters[3]
        );
        return Availability::Unavailable;
    }
    if !private_bits_free(filters) {
        log_warn!(
            "viewport_pass: unavailable -- a stock pass filter uses a private bit (filters {:#x?})",
            filters
        );
        return Availability::Unavailable;
    }
    if vtable().is_null() {
        log_warn!("viewport_pass: unavailable -- vtable allocation failed");
        return Availability::Unavailable;
    }
    GD_WRITE_OFF.store(v.gd_write_off, Ordering::Release);
    log_info!(
        "viewport_pass: available -- stock filters {:#x?}, private bits {:#x}/{:#x} free, RENDER_2D prios {:#x}../{:#x}..",
        filters,
        FILTER_BIT[0],
        FILTER_BIT[1],
        PRIO_BASE[0],
        PRIO_BASE[1]
    );
    Availability::Available
}

/// The RENDER_2D target list object, probed.
unsafe fn render2d_list(v: &Scene3dViewportSites) -> Option<*mut u8> {
    if !memory::is_readable(v.display, 8) {
        return None;
    }
    let display = memory::read_ptr(v.display);
    if display.is_null() || !memory::is_readable(display, v.render2d_list_off + 8) {
        return None;
    }
    let list = memory::read_ptr(display.add(v.render2d_list_off)) as *mut u8;
    if list.is_null() || !memory::is_readable(list, v.list_flags_off + 4) {
        return None;
    }
    Some(list)
}

/// The RENDER_2D target's pixel size (the u16 dims of the list's target
/// surface — the display back buffer; output-sized under custom
/// resolution). `None` while unavailable / not yet created.
pub fn render_target_dims() -> Option<(u32, u32)> {
    let v = vp_sites()?;
    // SAFETY: every hop probed.
    unsafe {
        let list = render2d_list(&v)?;
        let target = memory::read_ptr(list.add(v.list_target_off));
        if target.is_null() || !memory::is_readable(target, v.target_h_off + 2) {
            return None;
        }
        let w = u16::from_le_bytes([
            memory::read_u8(target.add(v.target_w_off)),
            memory::read_u8(target.add(v.target_w_off + 1)),
        ]) as u32;
        let h = u16::from_le_bytes([
            memory::read_u8(target.add(v.target_h_off)),
            memory::read_u8(target.add(v.target_h_off + 1)),
        ]) as u32;
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }
}

/// A clear viewport + two pass clones attached into RENDER_2D.
pub struct PassSet {
    clear: *mut ClearViewport,
    opaque: *mut u8,
    trans: *mut u8,
    attached: bool,
    filter_bit: u32,
    base_prio: u32,
    enabled: bool,
}

// Raw pointers to mod-owned blocks; game-thread only by contract.
unsafe impl Send for PassSet {}

unsafe fn write_rect(p: *mut u8, rect: RtRect) {
    memory::write_i32(p, rect.x);
    memory::write_i32(p.add(4), rect.y);
    memory::write_i32(p.add(8), rect.w);
    memory::write_i32(p.add(12), rect.h);
}

/// Byte-clone `stock` (probed + identity-gated) with our rect / filter /
/// flags; view/proj start as the stock's (overwritten by `set_camera`).
unsafe fn clone_pass(
    v: &Scene3dViewportSites,
    global: *const u8,
    rect: RtRect,
    filter_bit: u32,
) -> Option<*mut u8> {
    let stock = stock_pass(v, global).ok()?;
    let clone = memory::alloc_zeroed(v.pass_size);
    if clone.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(stock as *const u8, clone, v.pass_size);
    memory::write_ptr(clone.add(v.pass_self_off), clone);
    write_rect(clone.add(v.pass_rect_off), rect);
    memory::write_f32(clone.add(v.pass_minz_off), 0.0);
    memory::write_f32(clone.add(v.pass_maxz_off), 1.0);
    memory::write_u32(clone.add(v.pass_filter_off), filter_bit);
    // Enabled, camera upload ON (the worker uploads OUR view/proj).
    memory::write_u32(clone.add(v.pass_flags_off), 0);
    Some(clone)
}

/// Game thread (`on_frame`). Allocate the three viewports and attach them
/// to RENDER_2D at `base_prio`, `+1`, `+2`. `None` (one WARN per class)
/// when the compositor is unavailable or any allocation / probe fails —
/// nothing is left attached.
pub fn create(filter_bit: u32, rect: RtRect, clear: ClearSpec, base_prio: u32) -> Option<PassSet> {
    if !is_available() {
        return None;
    }
    let v = vp_sites()?;
    let vt = vtable();
    if vt.is_null() {
        return None;
    }
    // SAFETY: every engine pointer probed; the blocks are ours.
    unsafe {
        let Some(list) = render2d_list(&v) else {
            log_warn!("viewport_pass: RENDER_2D target list unreadable -- not attaching");
            return None;
        };
        if v.vp_rect_off != super::viewport_pass_layout::VP_RECT_OFF
            || v.vp_flags_off != super::viewport_pass_layout::VP_FLAGS_OFF
        {
            log_warn!(
                "viewport_pass: derived viewport layout (rect +0x{:X}, flags +0x{:X}) disagrees with ClearViewport -- refusing",
                v.vp_rect_off,
                v.vp_flags_off
            );
            return None;
        }
        let clear_vp = memory::alloc_zeroed(CLEAR_VIEWPORT_SIZE) as *mut ClearViewport;
        if clear_vp.is_null() {
            log_warn!("viewport_pass: clear viewport allocation failed");
            return None;
        }
        std::ptr::write(
            clear_vp,
            ClearViewport {
                vtable: vt as usize,
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                min_z: 0.0,
                max_z: 1.0,
                name_hash: 0,
                flags: VP_FLAG_SKIP_CAMERA,
                clear_flags: clear.d3d_flags(),
                color_argb: clear.color.unwrap_or(0),
                z: 1.0,
                stencil: 0,
                _pad: [0; 2],
            },
        );
        let opaque = clone_pass(&v, v.pass_globals[1], rect, filter_bit);
        let trans = clone_pass(&v, v.pass_globals[3], rect, filter_bit);
        let (Some(opaque), Some(trans)) = (opaque, trans) else {
            log_warn!("viewport_pass: pass clone failed (stock pass gate / allocation)");
            memory::free_alloc(clear_vp as *mut u8);
            if let Some(p) = opaque {
                memory::free_alloc(p);
            }
            if let Some(p) = trans {
                memory::free_alloc(p);
            }
            return None;
        };
        let attach: AttachFn = std::mem::transmute::<*const u8, AttachFn>(v.attach);
        attach(list, clear_vp as *mut u8, base_prio);
        attach(list, opaque.add(v.sub_off), base_prio + 1);
        attach(list, trans.add(v.sub_off), base_prio + 2);
        log_info!(
            "viewport_pass: attached clear@{:#x} opaque@{:#x} trans@{:#x} filter={:#x} rect=({},{},{},{})",
            base_prio,
            base_prio + 1,
            base_prio + 2,
            filter_bit,
            rect.x,
            rect.y,
            rect.w,
            rect.h
        );
        Some(PassSet {
            clear: clear_vp,
            opaque,
            trans,
            attached: true,
            filter_bit,
            base_prio,
            enabled: true,
        })
    }
}

impl PassSet {
    pub fn filter_bit(&self) -> u32 {
        self.filter_bit
    }

    pub fn base_prio(&self) -> u32 {
        self.base_prio
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Move the box (all three viewports). Game thread.
    pub fn set_rect(&mut self, rect: RtRect) {
        let Some(v) = vp_sites() else { return };
        // SAFETY: our own blocks.
        unsafe {
            write_rect(
                (self.clear as *mut u8).add(super::viewport_pass_layout::VP_RECT_OFF),
                rect,
            );
            write_rect(self.opaque.add(v.pass_rect_off), rect);
            write_rect(self.trans.add(v.pass_rect_off), rect);
        }
    }

    /// Write the camera into both clones (the worker uploads them before
    /// each clone's render). Game thread, once per frame.
    pub fn set_camera(&mut self, view: &Mat4, proj: &Mat4) {
        let Some(v) = vp_sites() else { return };
        // SAFETY: our own blocks; the matrices are 0x40 bytes inside pass_size.
        unsafe {
            for clone in [self.opaque, self.trans] {
                std::ptr::copy_nonoverlapping(
                    view.as_ptr() as *const u8,
                    clone.add(v.pass_view_off),
                    64,
                );
                std::ptr::copy_nonoverlapping(
                    proj.as_ptr() as *const u8,
                    clone.add(v.pass_proj_off),
                    64,
                );
            }
        }
    }

    /// Show / hide the whole set via the DISABLED bit (the dispatcher skips
    /// disabled viewports; nothing is detached). Game thread.
    pub fn set_enabled(&mut self, on: bool) {
        let Some(v) = vp_sites() else { return };
        self.enabled = on;
        let disabled = if on { 0 } else { VP_FLAG_DISABLED };
        // SAFETY: our own blocks.
        unsafe {
            let flags = (self.clear as *mut u8).add(super::viewport_pass_layout::VP_FLAGS_OFF);
            memory::write_u32(flags, VP_FLAG_SKIP_CAMERA | disabled);
            memory::write_u32(self.opaque.add(v.pass_flags_off), disabled);
            memory::write_u32(self.trans.add(v.pass_flags_off), disabled);
        }
    }

    /// Erase the three viewports from RENDER_2D and queue the blocks for
    /// the reaper (freed ≥ [`REAPER_FRAMES`] later). Game thread.
    pub fn detach(mut self) {
        self.detach_inner();
    }

    fn detach_inner(&mut self) {
        if !self.attached {
            return;
        }
        self.attached = false;
        let Some(v) = vp_sites() else { return };
        // SAFETY: the list is probed; the viewports are the ones we attached.
        unsafe {
            if let Some(list) = render2d_list(&v) {
                let detach: DetachFn = std::mem::transmute::<*const u8, DetachFn>(v.detach);
                detach(list, self.clear as *mut u8);
                detach(list, self.opaque.add(v.sub_off));
                detach(list, self.trans.add(v.sub_off));
            } else {
                log_warn!(
                    "viewport_pass: RENDER_2D list unreadable at detach -- viewports left attached (blocks leaked)"
                );
                return;
            }
        }
        let blocks = vec![
            self.clear as usize,
            self.opaque as usize,
            self.trans as usize,
        ];
        let now = FRAME.load(Ordering::Relaxed);
        if let Ok(mut r) = REAPER.lock() {
            r.push((blocks, now));
        }
        log_info!(
            "viewport_pass: detached set @{:#x}.. (filter {:#x}); blocks reaped in {} frames",
            self.base_prio,
            self.filter_bit,
            REAPER_FRAMES
        );
    }
}

impl Drop for PassSet {
    /// A dropped-but-attached set is detached (never leave an engine list
    /// pointing at memory we are about to lose track of).
    fn drop(&mut self) {
        if self.attached {
            self.detach_inner();
        }
    }
}

/// Game thread, once per frame by the owner: advance the frame counter and
/// free detached sets that are ≥ [`REAPER_FRAMES`] old.
pub fn reap() {
    let now = FRAME.fetch_add(1, Ordering::AcqRel) + 1;
    let Ok(mut r) = REAPER.lock() else { return };
    if r.is_empty() {
        return;
    }
    let mut freed = 0usize;
    r.retain(|(blocks, at)| {
        if now.saturating_sub(*at) >= REAPER_FRAMES {
            // SAFETY: detached ≥ 2 frames ago; no worker holds the pointers.
            for b in blocks {
                unsafe { memory::free_alloc(*b as *mut u8) };
            }
            freed += 1;
            false
        } else {
            true
        }
    });
    if freed > 0 {
        log_info!("viewport_pass: reaped {} set(s)", freed);
    }
}
