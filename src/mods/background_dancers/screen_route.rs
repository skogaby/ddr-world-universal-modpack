//! Background Movies = STAGE SCREENS — the one code byte and the fit writes
//! (design `.agents/planning/2026-09-23-background-dancers-monitor-movies/`
//! §4.2, RE `docs/background_dancers_research.md` §8).
//!
//! DDR A3 drew a "monitor" song's movie into layer-table entry 10, whose
//! private command list renders into the 1280 × 1280 OFFSCREEN1 render
//! target — registered at boot as the named texture `offscreen1`, which the
//! screen materials of the `monitor*` / `replicant*` stages (and any custom
//! stage that names its screen image `offscreen1`) sample. World kept every
//! piece but chooses `thumbnail ? 0 : 9` in `MovieActor::onInitialize`. For
//! a routed song this module:
//!
//! 1. [`arm`] — rewrites that `9` to `10` (checked `09 → 0A`, the
//!    `anytime_speedmod` pattern) BEFORE the song's MovieActor exists (the
//!    window entry, scene 25 → 26; the byte is read once per MovieActor, at
//!    DPS step 2 of scene 28). The caller also writes VIDEO SIZE as
//!    FULLSCREEN: a set thumbnail flag selects entry 0 whatever the byte says.
//! 2. [`on_frame`] — frames every MovieActor of the window with A3's monitor
//!    fit: origin (0, 0), size (1280, 1280) written into the actor's fit
//!    rectangle (`movie_fit_origin_off` / `movie_fit_size_off`) while its
//!    step is 0 / 1 / 2 — the actor's 0x1045 case applies the fit only at
//!    step 2, and nothing but its ctor writes the fields. Runs from the mod's
//!    frame callback (not the scene driver, which returns early until the 3D
//!    scene is built — the fit must be in place before the movie's 2 → 3).
//! 3. [`disarm`] — writes the byte back (checked `0A → 09`) at window exit
//!    and at mod disable.
//!
//! Every function runs on the game thread (scene callback, frame callback,
//! mod enable/disable). Fail-open: unresolved signatures ⇒ [`is_available`]
//! is false and STAGE SCREENS degrades to THUMBNAIL (one WARN, lifecycle); a
//! refused checked write ⇒ that song plays as THUMBNAIL; an unreadable actor
//! ⇒ no write that frame (the movie keeps the game's own framing).

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

use super::movie_backdrop;
use super::movie_mode::{self, ImmAction, MOVIE_STEP_READY};

/// `MovieActor + 0x138` → the `agcs::Movie` wrapper, `+0x18` → its state
/// block (20260825 `FUN_180216c70`), whose `u32 +0x2C/+0x30` hold the
/// movie's pixel size once opened (the texture allocator `FUN_180216170`;
/// `f32 +0x24/+0x28` is the DRAW size the fit rewrites). Diagnostic only,
/// read through probed pointers.
const MOVIE_WRAPPER_OFF: usize = 0x138;
const MOVIE_IMPL_OFF: usize = 0x18;
const MB_PX_W: usize = 0x2C;
const MB_PX_H: usize = 0x30;

/// Layer-table entry stride and fields (research §7.2 / §8.3): override
/// command list `+0`, layer object `+8`, list index `+0x10`.
const LAYER_ENTRY_STRIDE: usize = 0x18;
const LAYER_ENTRY_ROUTED: usize = movie_mode::RouteImm::ROUTED as usize;
const LAYER_ENTRY_LAYER_OFF: usize = 0x08;
const LAYER_ENTRY_LIST_OFF: usize = 0x10;
/// The layer object (ScreenRoot): walk-gate bytes `+0x10` (== 0) / `+0x12`
/// (!= 0), active node count `+0x3C`.
const LAYER_GATE_A_OFF: usize = 0x10;
const LAYER_GATE_B_OFF: usize = 0x12;
const LAYER_NODE_COUNT_OFF: usize = 0x3C;

/// The fit's two f64 pairs: (x, y) at the origin field, (w, h) at the size
/// field (the z / depth halves at +0x10 are left alone).
const FIT_PAIR_LEN: usize = 16;

/// `movie_layer_select_imm` (the MovieActor layer-choice imm8).
static IMM_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// `movie_fit_origin_off` / `movie_fit_size_off` (0 = unresolved).
static ORIGIN_OFF: AtomicUsize = AtomicUsize::new(0);
static SIZE_OFF: AtomicUsize = AtomicUsize::new(0);
/// `layer_table` (the global holding the table pointer) — diagnostic only.
static LAYER_TABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// The byte currently reads `0A` because [`arm`] wrote it.
static ROUTED: AtomicBool = AtomicBool::new(false);
/// Layer entry 10's state logged once per boot.
static ENTRY_LOGGED: AtomicBool = AtomicBool::new(false);

/// Per-window MovieActor bookkeeping (a quick restart / course stage makes
/// a new actor; the window stays open across it).
struct Tracked {
    actor: usize,
    /// Frames the fit was written for this actor.
    writes: u32,
    /// The per-actor INFO (or the unframed WARN) was emitted.
    logged: bool,
    /// The step seen last frame (a lower one = a new actor at a reused address).
    last_step: i32,
}

static TRACKED: Mutex<Tracked> = Mutex::new(Tracked {
    actor: 0,
    writes: 0,
    logged: false,
    last_step: 0,
});

/// Resolve the route's addresses (mod init). `false` = STAGE SCREENS is
/// unavailable this boot.
pub fn init(signatures: &SignatureStore) -> bool {
    if let Some(t) = signatures.get_address("layer_table") {
        LAYER_TABLE.store(t as *mut u8, Ordering::Release);
    }
    let imm = signatures.get_address("movie_layer_select_imm");
    let origin = signatures.movie_fit_origin_off();
    let size = signatures.movie_fit_size_off();
    match (imm, origin, size) {
        (Some(i), Some(o), Some(s)) if !i.is_null() && o != 0 && s != 0 => {
            IMM_ADDR.store(i as *mut u8, Ordering::Release);
            ORIGIN_OFF.store(o, Ordering::Release);
            SIZE_OFF.store(s, Ordering::Release);
            true
        }
        _ => {
            log_info!(
                "BackgroundDancers: movie screen route unresolved (movie_layer_select_imm {}, fit offsets {}) -- Background Movies = STAGE SCREENS falls back to THUMBNAIL this boot",
                if imm.is_some() { "ok" } else { "MISSING" },
                if origin.is_some() && size.is_some() {
                    "ok"
                } else {
                    "MISSING"
                }
            );
            false
        }
    }
}

/// Both route signatures resolved.
pub fn is_available() -> bool {
    !IMM_ADDR.load(Ordering::Acquire).is_null()
        && ORIGIN_OFF.load(Ordering::Acquire) != 0
        && SIZE_OFF.load(Ordering::Acquire) != 0
}

/// Whether the current window is routed.
pub fn is_routed() -> bool {
    ROUTED.load(Ordering::Acquire)
}

/// Checked write of the imm8 toward `want_routed`. `Ok(true)` = written,
/// `Ok(false)` = it already held the wanted value, `Err(b)` = it reads the
/// unknown byte `b` (nothing written).
fn write_imm(want_routed: bool) -> Result<bool, u8> {
    let p = IMM_ADDR.load(Ordering::Acquire);
    if p.is_null() {
        return Err(0);
    }
    // SAFETY: `p` is inside gamemdx's code section (derived + gated at
    // boot); code bytes are always readable. The write goes through
    // VirtualProtect and is read back.
    unsafe {
        let current = memory::read_u8(p);
        match movie_mode::imm_action(current, want_routed) {
            ImmAction::Already => Ok(false),
            ImmAction::Refuse => Err(current),
            ImmAction::Write(v) => {
                let old = memory::make_writable(p, 1);
                memory::write_u8(p, v);
                memory::restore_protection(p, 1, old);
                let back = memory::read_u8(p);
                if back == v {
                    Ok(true)
                } else {
                    Err(back)
                }
            }
        }
    }
}

/// Window entry of a routed song (game thread, before the song's MovieActor
/// exists): `09 → 0A`. `false` + WARN when the byte is not the stock value
/// (someone else's patch) or the write did not stick — the caller then plays
/// the song as THUMBNAIL before writing anything else.
pub fn arm() -> bool {
    if !is_available() {
        return false;
    }
    if let Ok(mut t) = TRACKED.lock() {
        *t = Tracked {
            actor: 0,
            writes: 0,
            logged: false,
            last_step: 0,
        };
    }
    match write_imm(true) {
        Ok(written) => {
            ROUTED.store(true, Ordering::Release);
            log_info!(
                "BackgroundDancers: stage screens -- movie layer select {} (entry 9 -> 10, OFFSCREEN1)",
                if written {
                    "patched 09 -> 0A"
                } else {
                    "already 0A"
                }
            );
            true
        }
        Err(b) => {
            log_warn!(
                "BackgroundDancers: stage screens -- movie layer select reads 0x{:02X} (expected 09) -- not patched, the song plays as THUMBNAIL",
                b
            );
            false
        }
    }
}

/// Window exit / mod disable (game thread): `0A → 09` when this module
/// routed the window. Idempotent.
pub fn disarm() {
    if !ROUTED.swap(false, Ordering::AcqRel) {
        return;
    }
    let (actor, writes) = TRACKED
        .lock()
        .map(|t| (t.actor, t.writes))
        .unwrap_or((0, 0));
    match write_imm(false) {
        Ok(written) => log_info!(
            "BackgroundDancers: stage screens -- movie layer select {} at song-window exit (last actor {}, {} fit frame(s))",
            if written {
                "restored 0A -> 09"
            } else {
                "already 09"
            },
            if actor != 0 { "framed" } else { "none seen" },
            writes
        ),
        Err(b) => log_warn!(
            "BackgroundDancers: stage screens -- movie layer select reads 0x{:02X} at restore (expected 0A) -- left alone",
            b
        ),
    }
}

/// Per frame (game thread). O(1) when the window is not routed.
pub fn on_frame() {
    if !ROUTED.load(Ordering::Acquire) {
        return;
    }
    let Some((actor, step)) = movie_backdrop::live_movie_actor() else {
        return;
    };
    let Ok(mut t) = TRACKED.lock() else { return };
    // A new actor — or one reallocated at the same address (its step starts
    // over) — gets its own framing + INFO.
    if t.actor != actor as usize || step < t.last_step {
        *t = Tracked {
            actor: actor as usize,
            writes: 0,
            logged: false,
            last_step: step,
        };
    }
    t.last_step = step;
    if movie_mode::fit_writable(step) {
        if write_fit(actor) {
            t.writes = t.writes.saturating_add(1);
        }
        // Once the movie opened its size is known: one INFO per actor.
        if step >= MOVIE_STEP_READY && !t.logged && t.writes > 0 {
            t.logged = true;
            log_framed(actor, step, t.writes);
        }
    } else if !t.logged {
        t.logged = true;
        match step {
            movie_mode::MOVIE_STEP_PLAYING if t.writes == 0 => log_warn!(
                "BackgroundDancers: stage screens -- MovieActor {:p} reached step {} before its fit could be written -- it keeps the game's own framing inside the square",
                actor,
                step
            ),
            movie_mode::MOVIE_STEP_PLAYING => log_framed(actor, step, t.writes),
            _ => log_info!(
                "BackgroundDancers: stage screens -- MovieActor {:p} at step {} (no movie drawn) -- nothing to frame",
                actor,
                step
            ),
        }
    }
}

/// Write origin (0, 0) / size (1280, 1280). `false` when a field is not
/// readable (nothing written).
fn write_fit(actor: *mut u8) -> bool {
    let origin = ORIGIN_OFF.load(Ordering::Acquire);
    let size = SIZE_OFF.load(Ordering::Acquire);
    if origin == 0 || size == 0 {
        return false;
    }
    // SAFETY: `actor` is the live MovieActor (vtable-verified child walk,
    // game thread — the actor's own thread); both f64 pairs are probed
    // before the write. The offsets are sweep-verified (0x108 / 0x120) and
    // below the 0x150 allocation.
    unsafe {
        let o = actor.add(origin);
        let s = actor.add(size);
        if !memory::is_readable(o, FIT_PAIR_LEN) || !memory::is_readable(s, FIT_PAIR_LEN) {
            return false;
        }
        let e = movie_mode::SCREEN_RT_EXTENT;
        (o as *mut f64).write_unaligned(0.0);
        (o.add(8) as *mut f64).write_unaligned(0.0);
        (s as *mut f64).write_unaligned(e);
        (s.add(8) as *mut f64).write_unaligned(e);
    }
    true
}

/// The Movie's state block (see the `MB_*` offsets), probed.
fn movie_block(actor: *mut u8) -> Option<*const u8> {
    // SAFETY: every pointer read out of the actor is probed first.
    unsafe {
        let w = actor.add(MOVIE_WRAPPER_OFF);
        if !memory::is_readable(w, 8) {
            return None;
        }
        let wrapper = memory::read_ptr(w);
        if !memory::is_readable(wrapper.wrapping_add(MOVIE_IMPL_OFF), 8) {
            return None;
        }
        let block = memory::read_ptr(wrapper.add(MOVIE_IMPL_OFF));
        memory::is_readable(block, MB_PX_H + 4).then_some(block)
    }
}

/// The movie's pixel size (the texture allocator's), once opened.
fn movie_px(actor: *mut u8) -> Option<(f32, f32)> {
    let block = movie_block(actor)?;
    // SAFETY: probed by `movie_block`.
    let (w, h) = unsafe {
        (
            memory::read_u32(block.add(MB_PX_W)),
            memory::read_u32(block.add(MB_PX_H)),
        )
    };
    (w > 0 && h > 0 && w <= 8192 && h <= 8192).then_some((w as f32, h as f32))
}

fn log_framed(actor: *mut u8, step: i32, writes: u32) {
    let px = match movie_px(actor) {
        Some((w, h)) if w > 0.0 && h > 0.0 => {
            let s = (movie_mode::SCREEN_RT_EXTENT / w as f64)
                .min(movie_mode::SCREEN_RT_EXTENT / h as f64);
            format!(
                ", movie {}x{} -> {:.0}x{:.0} in the square",
                w,
                h,
                w as f64 * s,
                h as f64 * s
            )
        }
        _ => String::new(),
    };
    log_info!(
        "BackgroundDancers: stage screens -- MovieActor {:p} framed: origin (0,0) size (1280,1280) (step {}, {} frame(s){})",
        actor,
        step,
        writes,
        px
    );
    log_entry_once();
}

/// One-shot per boot: layer-table entry 10's state after the first routed
/// registration (a routed Movie increments the layer's active-node count).
fn log_entry_once() {
    if ENTRY_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }
    let global = LAYER_TABLE.load(Ordering::Acquire) as *const u8;
    if global.is_null() {
        log_info!(
            "BackgroundDancers: stage screens -- layer_table unresolved, entry 10 not inspected"
        );
        return;
    }
    // SAFETY: every pointer read is probed first (diagnostic only).
    unsafe {
        if !memory::is_readable(global, 8) {
            return;
        }
        let table = memory::read_ptr(global);
        let entry = table.wrapping_add(LAYER_ENTRY_ROUTED * LAYER_ENTRY_STRIDE);
        if !memory::is_readable(entry, LAYER_ENTRY_STRIDE) {
            log_info!(
                "BackgroundDancers: stage screens -- layer table entry 10 unreadable (table {:p})",
                table
            );
            return;
        }
        let over = memory::read_ptr(entry);
        let layer = memory::read_ptr(entry.add(LAYER_ENTRY_LAYER_OFF));
        let list = memory::read_i32(entry.add(LAYER_ENTRY_LIST_OFF));
        if !memory::is_readable(layer, LAYER_NODE_COUNT_OFF + 4) {
            log_info!(
                "BackgroundDancers: stage screens -- layer entry 10: override {:p}, layer {:p} (unreadable), list {}",
                over,
                layer,
                list
            );
            return;
        }
        log_info!(
            "BackgroundDancers: stage screens -- layer entry 10: override {:p}, layer {:p}, list {}, walk gate +0x10={} +0x12={}, active nodes {} (>= 1 = the movie registered)",
            over,
            layer,
            list,
            memory::read_u8(layer.add(LAYER_GATE_A_OFF)),
            memory::read_u8(layer.add(LAYER_GATE_B_OFF)),
            memory::read_u32(layer.add(LAYER_NODE_COUNT_OFF))
        );
    }
}
