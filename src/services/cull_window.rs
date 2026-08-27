//! Cull Window Service — multi-contributor extension of the note collector's
//! (and guideline draw's) top culling window.
//!
//! Promoted from `playfield_styling::cull_patch` (which shipped the
//! mechanism) so that BOTH playfield_styling (scaled playfields need
//! `720/scale`) and player_perspective (hallway needs its draw distance)
//! can widen the window independently of each other's config/enable state.
//!
//! ## Mechanism (unchanged from the shipped patch)
//!
//! Both functions stop collecting at a 720.0f screen bound loaded by one
//! `MOVSS xmm, [RIP+disp32]` instruction each. The 720.0 **constant is
//! shared by 14 unrelated readers and is NEVER patched**; instead each
//! instruction's disp32 is redirected (4-byte write, byte-verified first)
//! to a mod-owned float slot.
//!
//! The float slot is a 4-byte slot that must be RIP-reachable (±2 GB) from
//! both patch sites. Preferred home: an int3 (`0xCC`) function-alignment
//! cave inside the module near the collector; fallback: `memory::alloc_near`.
//!
//! The disp32 rewrite happens ONCE per process (lazily, at the first
//! contributor's `ensure_installed`) and is never unpatched — "off" is the
//! slot holding exactly 720.0, which is semantically stock.
//!
//! ## Contributor semantics
//!
//! The two contributors act in DIFFERENT spaces, and their effects compose
//! multiplicatively — the collector culls in pre-scale lane space, the
//! playfield-styling fill then scales the lane by `s`, and the perspective
//! map consumes post-scale SCREEN distance:
//!
//! - playfield_styling contributes its latched min scale (`s < 1` widens);
//! - player_perspective contributes its hallway draw distance in
//!   (post-scale) screen pixels.
//!
//! Effective pre-scale bound = `max(720, distance) / min(scale, 1)`. Each
//! component alone reduces to the shipped behavior (`720/s` and `distance`
//! respectively); both together give `distance/s` — the lane offset whose
//! scaled screen distance reaches the hallway draw distance. Both only ever
//! WIDEN, and over-collection is safe (extra notes render off-screen or at
//! the horizon).

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// The stock top cull bound (the render height).
pub const RENDER_HEIGHT: f32 = 720.0;

/// `MOVSS XMM15, [RIP+disp32]` — the collector's cull-bound load.
const CULL_PREFIX_XMM15: [u8; 5] = [0xF3, 0x44, 0x0F, 0x10, 0x3D];
/// `MOVSS XMM9, [RIP+disp32]` — the guideline draw's cull-bound load.
const CULL_PREFIX_XMM9: [u8; 5] = [0xF3, 0x44, 0x0F, 0x10, 0x0D];
/// Both instructions are prefix (5) + disp32 (4) bytes long.
const INSN_LEN: usize = 9;

/// playfield_styling's latched min scale (f32 bits; 1.0 = identity).
static SCALE_CONTRIB: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0f
/// player_perspective's draw distance in screen px (f32 bits; 720 = identity).
static DIST_CONTRIB: AtomicU32 = AtomicU32::new(0x4434_0000); // 720.0f

/// The mod-owned float slot. Null until install succeeds. Never freed —
/// patched game code points at it for the process lifetime.
static FLOAT_SLOT: AtomicPtr<f32> = AtomicPtr::new(std::ptr::null_mut());

/// True once the disp32 rewrites are live (once per process; never undone).
static PATCHED: AtomicBool = AtomicBool::new(false);

// Stashed at `init` (service block) for the lazy `ensure_installed`.
static COLLECTOR_SITE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static GUIDELINE_SITE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static MODULE_BASE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static MODULE_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Stash the two derived patch sites + module bounds. Called once from the
/// lib.rs service block (no code is patched here — patching is lazy, driven
/// by the first contributor's `ensure_installed`).
pub fn init(signatures: &SignatureStore, module_base: *const u8, module_size: usize) {
    if let Some(s) = signatures.get_address("collector_cull_site") {
        COLLECTOR_SITE.store(s as *mut u8, Ordering::Release);
    }
    if let Some(s) = signatures.get_address("guideline_cull_site") {
        GUIDELINE_SITE.store(s as *mut u8, Ordering::Release);
    }
    MODULE_BASE.store(module_base as *mut u8, Ordering::Release);
    MODULE_SIZE.store(module_size, Ordering::Release);
}

/// The current effective cull bound (mirrors the slot; readable without a
/// slot deref for the mine integration). 720.0 when never installed.
pub fn cull_bound() -> f32 {
    let slot = FLOAT_SLOT.load(Ordering::Acquire);
    if slot.is_null() {
        RENDER_HEIGHT
    } else {
        unsafe { *slot }
    }
}

/// Whether the disp32 rewrites are live.
pub fn is_patched() -> bool {
    PATCHED.load(Ordering::Acquire)
}

/// Set playfield_styling's contribution: the latched minimum playfield
/// scale (clamped to (0, 1] — only shrinking widens the window).
pub fn set_scale_contribution(scale: f32) {
    let v = if scale.is_finite() && scale > 0.0 {
        scale.min(1.0)
    } else {
        1.0
    };
    SCALE_CONTRIB.store(v.to_bits(), Ordering::Release);
    recompute();
}

/// Reset playfield_styling's contribution to identity.
pub fn clear_scale_contribution() {
    set_scale_contribution(1.0);
}

/// Set player_perspective's contribution: the hallway draw distance in
/// screen pixels (floored at 720 = identity).
pub fn set_distance_contribution(distance_px: f32) {
    let v = if distance_px.is_finite() {
        distance_px.max(RENDER_HEIGHT)
    } else {
        RENDER_HEIGHT
    };
    DIST_CONTRIB.store(v.to_bits(), Ordering::Release);
    recompute();
}

/// Reset player_perspective's contribution to identity.
pub fn clear_distance_contribution() {
    set_distance_contribution(RENDER_HEIGHT);
}

/// Effective pre-scale bound = `max(720, distance) / min(scale, 1)`.
/// The scale divide converts the (post-scale) screen distance back into
/// the lane space the collector culls in.
fn recompute() {
    let s = f32::from_bits(SCALE_CONTRIB.load(Ordering::Acquire));
    let d = f32::from_bits(DIST_CONTRIB.load(Ordering::Acquire));
    set_slot(d.max(RENDER_HEIGHT) / s.clamp(f32::MIN_POSITIVE, 1.0));
}

/// Write `v` into the float slot (protection-toggled: the slot may live in
/// an int3 cave inside the module's RX .text section). No-op if the slot
/// was never created.
fn set_slot(v: f32) {
    let slot = FLOAT_SLOT.load(Ordering::Acquire);
    if slot.is_null() {
        return;
    }
    unsafe {
        let p = slot as *mut u8;
        let old = memory::make_writable(p, 4);
        memory::write_f32(p, v);
        memory::restore_protection(p, 4, old);
    }
}

/// Find a 4-byte, 4-aligned slot inside an int3 alignment cave within
/// ±`range` bytes of `near`, staying inside the module. Requires a run of
/// at least `MIN_RUN` consecutive `0xCC` bytes and returns a slot in its
/// middle (never the first/last byte, so partial overwrites of neighboring
/// padding semantics are impossible).
fn find_int3_cave_slot(
    module_base: *const u8,
    module_size: usize,
    near: *const u8,
    range: usize,
) -> Option<*mut f32> {
    const MIN_RUN: usize = 12;
    let lo = (near as usize)
        .saturating_sub(range)
        .max(module_base as usize);
    let hi = (near as usize + range).min(module_base as usize + module_size);
    if hi <= lo {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(lo as *const u8, hi - lo) };

    let mut run_start = 0usize;
    let mut run_len = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == 0xCC {
            if run_len == 0 {
                run_start = i;
            }
            run_len += 1;
            if run_len >= MIN_RUN {
                // Take a 4-aligned slot at least 2 bytes into the run.
                let cave = lo + run_start;
                let slot = (cave + 2 + 3) & !3;
                if slot + 4 <= cave + run_len {
                    return Some(slot as *mut f32);
                }
            }
        } else {
            run_len = 0;
        }
    }
    None
}

/// Verify a patch site's instruction bytes and 720.0f RIP target, then
/// rewrite its disp32 to point at `slot`. Returns false (without writing)
/// on ANY mismatch.
unsafe fn verify_and_patch(site: *const u8, prefix: &[u8; 5], slot: *mut f32, label: &str) -> bool {
    // Byte verification: exact opcode prefix.
    let bytes = std::slice::from_raw_parts(site, INSN_LEN);
    if &bytes[..5] != prefix {
        log_warn!(
            "cull_window: {label} opcode mismatch (got {:02X?}) — refusing to patch",
            &bytes[..5]
        );
        return false;
    }
    // Content verification: the current RIP target must read 720.0f.
    let target = crate::core::scanner::decode_rip_relative(site.add(5));
    if memory::read_f32(target) != RENDER_HEIGHT {
        log_warn!("cull_window: {label} RIP target does not read 720.0 — refusing to patch");
        return false;
    }
    // RIP-reachability of the new displacement.
    let insn_end = site as i64 + INSN_LEN as i64;
    let disp = slot as i64 - insn_end;
    if disp > i32::MAX as i64 || disp < i32::MIN as i64 {
        log_warn!("cull_window: {label} float slot not RIP-reachable — refusing to patch");
        return false;
    }

    let disp_addr = site.add(5) as *mut u8;
    let old = memory::make_writable(disp_addr, 4);
    memory::write_i32(disp_addr, disp as i32);
    memory::restore_protection(disp_addr, 4, old);

    log_info!(
        "cull_window: {label} patched @ {:p} → slot {:p}",
        site,
        slot
    );
    true
}

/// Install the cull-window patches: create/locate the float slot (init
/// 720.0) and redirect BOTH verified sites (collector + guideline) to it.
/// Callable by ANY contributor's enable path; idempotent — once patched,
/// later calls return true immediately (the rewrites are never undone;
/// "off" is every contribution at identity → slot = 720.0).
///
/// All-or-nothing: if either site fails verification the other is NOT
/// patched and the whole install reports failure. (Site order: collector
/// first; a collector failure leaves everything stock.)
pub fn ensure_installed() -> bool {
    if PATCHED.load(Ordering::Acquire) {
        return true;
    }

    let collector_site = COLLECTOR_SITE.load(Ordering::Acquire) as *const u8;
    let guideline_site = GUIDELINE_SITE.load(Ordering::Acquire) as *const u8;
    let module_base = MODULE_BASE.load(Ordering::Acquire) as *const u8;
    let module_size = MODULE_SIZE.load(Ordering::Acquire);
    if collector_site.is_null() || guideline_site.is_null() || module_base.is_null() {
        log_warn!("cull_window: patch sites unresolved — cull extension unavailable");
        return false;
    }

    // Pre-verify BOTH sites before writing anything, so a mismatch on the
    // second site can't leave a half-patched pair.
    unsafe {
        for (site, prefix, label) in [
            (collector_site, &CULL_PREFIX_XMM15, "collector cull site"),
            (guideline_site, &CULL_PREFIX_XMM9, "guideline cull site"),
        ] {
            let bytes = std::slice::from_raw_parts(site, INSN_LEN);
            let target = crate::core::scanner::decode_rip_relative(site.add(5));
            if &bytes[..5] != prefix.as_slice() || memory::read_f32(target) != RENDER_HEIGHT {
                log_warn!("cull_window: {label} pre-verification failed — cull patch unavailable");
                return false;
            }
        }
    }

    // The float slot: int3 cave near the collector, else near-VirtualAlloc.
    let slot = match FLOAT_SLOT.load(Ordering::Acquire) {
        s if !s.is_null() => s,
        _ => {
            let slot = find_int3_cave_slot(module_base, module_size, collector_site, 0x20000)
                .or_else(|| {
                    let p = unsafe { memory::alloc_near(collector_site, 4) } as *mut f32;
                    if p.is_null() {
                        None
                    } else {
                        log_info!("cull_window: float slot via alloc_near (no int3 cave found)");
                        Some(p)
                    }
                });
            match slot {
                Some(s) => {
                    FLOAT_SLOT.store(s, Ordering::Release);
                    s
                }
                None => {
                    log_warn!("cull_window: no RIP-reachable float slot — cull patch unavailable");
                    return false;
                }
            }
        }
    };

    // Initialize from the current contributions (identity ⇒ 720.0) BEFORE
    // any site points at the slot.
    recompute();
    log_info!("cull_window: float slot @ {slot:p} = {:.1}", cull_bound());

    unsafe {
        if !verify_and_patch(
            collector_site,
            &CULL_PREFIX_XMM15,
            slot,
            "collector cull site",
        ) {
            return false;
        }
        if !verify_and_patch(
            guideline_site,
            &CULL_PREFIX_XMM9,
            slot,
            "guideline cull site",
        ) {
            // Pre-verification makes this unreachable in practice; if it
            // ever fires, keep the slot stock-equivalent and report failure.
            set_slot(RENDER_HEIGHT);
            return false;
        }
    }

    PATCHED.store(true, Ordering::Release);
    true
}
