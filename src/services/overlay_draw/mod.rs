//! Overlay Draw — DLL-owned drawing through the game's screen command list.
//!
//! The mod-menu overlay's animated theme backgrounds (overlay-menu rewrite
//! Step 8, design §4.7): while the menu is open with ANIMATED BACKGROUND on
//! and the active theme shader-backed, one scissored quad is appended to the
//! active command list per frame, bound to the synthesized DEFAULT
//! container's theme program, with the c48/c49 time/rect constant block.
//! Proven by the Step 2 spike (docs/overlay_draw_research.md — 18k POC
//! emissions, zero faults; z-sandwich game → quad → widgets verified).
//!
//! Structure:
//! - [`encode`] — pure, dependency-free record encoders (host-tested via
//!   `scripts/validate_overlay_draw.sh`).
//! - this file — the impure emitter: activation feed, gates, arena
//!   reservation/copy, per-scene diagnostics, the theme-program index
//!   export (published by `shader_synthesis`).
//!
//! ## Emission site (cabinet-settled 2026-08-25 — third iteration)
//!
//! Emission detours the game's per-frame LAYER DISPATCHER (`layer_dispatcher`
//! signature; called once per frame unconditionally from the render
//! orchestrator in EVERY scene). PRE-original, the detour replicates the
//! dispatcher's own per-entry walk conditions over the 11-entry layer table
//! (`layer_table` derived global; entries `{override_ptr, layer_object,
//! list_index}` stride 0x18; walked iff `byte[layer+0x10]==0 &&
//! byte[layer+0x12]!=0`, non-override) and appends the background block to
//! the WIDGET layer's list (identified by pointer identity with the render
//! list manager the DLL's widgets register into; fallback: the LAST walked
//! entry — the topmost-composed layer). The layer's own walk then records
//! its content AFTER our quad, so the quad sits beneath the menu's widgets
//! and above every lower layer — in every scene, once per frame, on the
//! dispatcher's own thread at the dispatcher's own moment (no torn-list
//! risk; the earlier wrapper-render spray and dirty-anchor mechanisms are
//! retired — see docs/overlay_draw_research.md for the full trail).
//!
//! Fail-open: every gate failure skips the frame and latches one WARN per
//! failure class; ≥ [`FAILURE_LATCH_THRESHOLD`] consecutive failures while
//! active latch the emitter off for the session (design §6). Inactive cost:
//! one relaxed atomic read per frame in the detour (plus the original call).
//!
pub mod encode;

use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::Mutex;
use std::time::Instant;

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::{render_notes_hook, scene_manager};
use crate::{log_info, log_warn};

use encode::{Quad, RecordWriter};

/// Animated-background activation feed (written by the mod-menu on
/// open/close/theme/animate changes; read on the render thread).
static BG_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Theme program index in the DEFAULT container.
static BG_PROGRAM: AtomicU32 = AtomicU32::new(0);
/// Modal rect packed `x<<48 | y<<32 | w<<16 | h` (pixel space).
static BG_RECT: AtomicU64 = AtomicU64::new(0);
/// Theme params (f32 bits) forwarded through c49.zw.
static BG_PARAM0: AtomicU32 = AtomicU32::new(0);
static BG_PARAM1: AtomicU32 = AtomicU32::new(0);
/// Quad vertex alpha (the MENU OPACITY mapping — the theme PS' master fade).
static BG_ALPHA: AtomicU32 = AtomicU32::new(0xFF);
/// The `layer_table` derived global (pointer to the 11-entry layer table
/// pointer) — the dispatcher detour walks it. Null ⇒ emitter unavailable.
static LAYER_TABLE_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Whether the dispatcher detour installed (the emitter's availability —
/// the menu greys ANIMATED BACKGROUND through this via `emitter_ready`).
static DISPATCHER_HOOKED: AtomicBool = AtomicBool::new(false);

type DispatcherFn = extern "C" fn();
static mut DISPATCHER_HOOK: Option<GenericDetour<DispatcherFn>> = None;
/// The EMISSION ANCHOR: the wrapper address of a menu-owned text widget
/// created FIRST in the menu's `allocate_widgets` (so it renders before
/// the panel and every menu text widget). Emitting the background block
/// pre-original at ITS `wrapper_render` puts the quad MID-WALK in the
/// widget layer's list: above everything the layer drew earlier — incl.
/// the full-screen loading art that buried the segment-start emissions
/// (cabinet Tests A–C, 2026-08-25) — and below the menu's own widgets.
/// Identity-gated: only THIS wrapper's render emits (the round-2 spray's
/// per-(list,frame) dedup let an EARLIER game wrapper claim the emission
/// below the art — that's why it failed on title screens).
static EMIT_ANCHOR: AtomicUsize = AtomicUsize::new(0);
/// The anchor widget's dirty-flag byte (render_state+0x68). Re-armed
/// post-render while the background is active so the walk keeps
/// dispatching the anchor every frame (the game's render pass clears it;
/// static text is otherwise served from a cached path — round-1 finding).
static ANCHOR_DIRTY: AtomicUsize = AtomicUsize::new(0);
/// Session failure latch: consecutive gate failures while active.
static FAIL_STREAK: AtomicU32 = AtomicU32::new(0);
static SESSION_LATCHED_OFF: AtomicBool = AtomicBool::new(false);
const FAILURE_LATCH_THRESHOLD: u32 = 60;
/// Animation clock epoch (first emission-path use).
static EPOCH: once_cell::sync::Lazy<Instant> = once_cell::sync::Lazy::new(Instant::now);
/// Wall wrap for the shader time constant — f32 precision degrades over
/// multi-day uptimes; theme shaders use wrap-seamless frequencies.
const TIME_WRAP_MS: u128 = 3_600_000;

/// Parameters for an active animated background.
#[derive(Clone, Copy, Debug)]
pub struct BackgroundParams {
    /// Program index in the synthesized DEFAULT container.
    pub program: u32,
    /// Modal rect (x, y, w, h) in 1280×720 pixel space.
    pub rect: (u16, u16, u16, u16),
    /// Quad vertex alpha (the theme PS' master fade — MENU OPACITY maps
    /// here so gameplay shows through the animation like it does through
    /// the static panel).
    pub alpha: u8,
    /// Theme knobs forwarded through c49.zw (reserved; 0.0 today).
    pub params: [f32; 2],
}

/// Activate (`Some`) or deactivate (`None`) the animated background.
/// Callable from any thread — atomics only. The mod-menu calls this on
/// open/close and on THEME / ANIMATED BACKGROUND edits.
pub fn set_background(params: Option<BackgroundParams>) {
    match params {
        Some(p) => {
            let (x, y, w, h) = p.rect;
            BG_RECT.store(
                (x as u64) << 48 | (y as u64) << 32 | (w as u64) << 16 | h as u64,
                Ordering::Relaxed,
            );
            BG_PROGRAM.store(p.program, Ordering::Relaxed);
            BG_PARAM0.store(p.params[0].to_bits(), Ordering::Relaxed);
            BG_PARAM1.store(p.params[1].to_bits(), Ordering::Relaxed);
            BG_ALPHA.store(p.alpha as u32, Ordering::Relaxed);
            BG_ACTIVE.store(true, Ordering::Release);
        }
        None => BG_ACTIVE.store(false, Ordering::Release),
    }
}

/// Whether the animated background is currently active (menu rendering
/// reads this to dim the gradient panel over a live animation).
pub fn is_background_active() -> bool {
    BG_ACTIVE.load(Ordering::Relaxed) && !SESSION_LATCHED_OFF.load(Ordering::Relaxed)
}

/// Whether the animated-background emitter is installed (dispatcher detour
/// live + layer table derived). The menu's availability gate reads this.
pub fn emitter_ready() -> bool {
    DISPATCHER_HOOKED.load(Ordering::Acquire)
}

/// The `default_shader` derived global (pointer to the boot-resident
/// `gs_screencommand_default` shader-object pointer), from the signature
/// store at init. Null ⇒ POC shader gates refuse.
static DEFAULT_SHADER_GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Scene id whose diagnostics were most recently considered (cheap gate —
/// the full once-per-scene-id set sits behind it under a mutex).
static LAST_DIAG_SCENE: AtomicI32 = AtomicI32::new(i32::MIN);

/// Scene ids already logged (bounded; a pathological scene churn can't spam).
static DIAG_SCENES: Mutex<Vec<i32>> = Mutex::new(Vec::new());
const DIAG_SCENE_CAP: usize = 64;

/// Emission counter (diagnostic heartbeat: one INFO per 600 emissions ≈ 10 s
/// at 60 fps, so the log shows the emitter is alive without spamming).
static EMIT_COUNT: AtomicUsize = AtomicUsize::new(0);
/// One-shot INFO on the first successful emission (cabinet validation aid).
static FIRST_EMIT_LOGGED: AtomicBool = AtomicBool::new(false);

/// Refuse to append when the arena has already grown past this (we cannot
/// know the true capacity; this bounds our contribution to runaway frames.
/// Diagnostics log real sizes so the spike can replace this guess with data).
const ARENA_SOFT_CAP: u32 = 8 * 1024 * 1024;

/// Sanity bound on a shader object's program count (a garbage pointer read
/// would otherwise pass the `>= 1` gate with e.g. 0x41414141).
const MAX_PLAUSIBLE_PROGRAMS: u32 = 64;

/// Theme program indices in the DEFAULT container (in
/// `mod_menu::theme::ThemeProgram::slot()` order — the mod-menu animated
/// backgrounds), published by `shader_synthesis` whenever the synthesized
/// default container is being served with theme programs aboard (both the
/// fresh-build and cache-hit paths). Unset ⇒ no shader path ⇒ the menu
/// degrades to its static gradient (design §6). Synthesis runs once per
/// boot, so a OnceCell fits.
static THEME_PROGRAMS: once_cell::sync::OnceCell<[u32; 11]> = once_cell::sync::OnceCell::new();

/// Called by `shader_synthesis` after the default container (with theme
/// programs) is built or cache-validated. Idempotent; first write wins.
pub fn publish_theme_programs(indices: [u32; 11]) {
    if THEME_PROGRAMS.set(indices).is_ok() {
        log_info!(
            "overlay_draw: theme programs published (first={}, count={})",
            indices[0],
            indices.len()
        );
    }
}

/// The published theme program indices (in `ThemeProgram::slot()` order),
/// or `None` when synthesis hasn't run / themes were degraded — consumers
/// (the emitter, the menu's ANIMATED BACKGROUND availability gate) treat
/// `None` as "static only".
pub fn theme_program_indices() -> Option<[u32; 11]> {
    THEME_PROGRAMS.get().copied()
}

/// Dev debug (`DDR_OVERLAY_DRAW_STOCK_BIND=1`, read once at init): bind
/// program 0 (stock) instead of the theme program and draw a plain
/// half-black quad — isolates "theme PS output invisible on some screen"
/// from "the whole draw is dropped there". Cabinet Test B (2026-08-25)
/// used this to prove the loading-screen invisibility was not
/// theme-shader-specific.
static STOCK_BIND_DEBUG: AtomicBool = AtomicBool::new(false);

/// Install the menu's emission anchor: `wrapper` is the anchor text
/// widget's WRAPPER address (`create_text_widget_with_wrapper`),
/// `dirty_addr` its dirty-flag byte (`TextWidget::dirty_flag_addr`).
/// Called from the menu's widget allocation (any thread — atomics).
pub fn set_emit_anchor(wrapper: usize, dirty_addr: usize) {
    ANCHOR_DIRTY.store(dirty_addr, Ordering::Relaxed);
    EMIT_ANCHOR.store(wrapper, Ordering::Release);
    log_info!(
        "overlay_draw: emission anchor set (wrapper=0x{:X}, dirty=0x{:X})",
        wrapper,
        dirty_addr
    );
}

/// Clear the emission anchor (menu widget teardown). Emission stops with
/// it — the anchor wrapper is about to be freed.
pub fn clear_emit_anchor() {
    EMIT_ANCHOR.store(0, Ordering::Release);
    ANCHOR_DIRTY.store(0, Ordering::Relaxed);
}

// ── Auxiliary quad anchor (SMX touch overlay) ────────────────────────
//
// A second, independent emission anchor for DLL features that draw plain
// untextured quads every frame (the SMX touchscreen overlay's buttons).
// Same mechanism as the menu background anchor: the owner creates a
// hidden text widget FIRST among its widgets, registers its wrapper +
// dirty byte here with an emitter fn, and the emitter appends records
// mid-walk at that wrapper's render (above earlier layer content, below
// the owner's own label widgets). The dirty byte is re-armed
// post-render so the walk keeps dispatching the anchor every frame.

/// The aux anchor wrapper address (0 = none).
static AUX_ANCHOR: AtomicUsize = AtomicUsize::new(0);
/// The aux anchor widget's dirty-flag byte.
static AUX_DIRTY: AtomicUsize = AtomicUsize::new(0);
/// Emitter fn pointer (`fn()`), called at the aux anchor's render.
/// Must be panic-free or panic-contained; typically calls
/// [`emit_overlay_quads`].
static AUX_EMITTER: AtomicUsize = AtomicUsize::new(0);

/// Install the aux anchor (see module docs). Callable from any thread.
pub fn set_aux_anchor(wrapper: usize, dirty_addr: usize, emitter: fn()) {
    AUX_DIRTY.store(dirty_addr, Ordering::Relaxed);
    AUX_EMITTER.store(emitter as usize, Ordering::Relaxed);
    AUX_ANCHOR.store(wrapper, Ordering::Release);
    log_info!(
        "overlay_draw: aux emission anchor set (wrapper=0x{:X}, dirty=0x{:X})",
        wrapper,
        dirty_addr
    );
}

/// Clear the aux anchor (owner teardown).
pub fn clear_aux_anchor() {
    AUX_ANCHOR.store(0, Ordering::Release);
    AUX_DIRTY.store(0, Ordering::Relaxed);
    AUX_EMITTER.store(0, Ordering::Relaxed);
}

// ── Topmost emission (post-dispatcher append) ────────────────────────
//
// The SMX touch overlay must draw ABOVE everything — the mod menu, the
// game's own widget-layer content (loading art), all of it (it stands in
// for physical cabinet hardware that sits "in front of" the screen).
// Widget z = registration order, so no widget-based approach can
// guarantee that. Instead: POST-original in the layer-dispatcher detour
// — after the dispatcher recorded every layer's content — append records
// to the WIDGET layer's private CommandList (the 11-entry layer table's
// override entry whose layer object IS the render-list manager the DLL's
// widgets register into; see docs/overlay_draw_research.md "The widget
// layer is an OVERRIDE entry"). Appended records are the last in the
// list ⇒ drawn last ⇒ topmost. The append happens before the render
// orchestrator's consumer kick (we're still inside its dispatcher call),
// so the list is not yet submitted — same-thread, same-frame, safe.

/// The registered topmost emitter (`fn()`), called once per frame after
/// the dispatcher runs. Inside the call, [`with_topmost_writer`] targets
/// the widget layer's private list.
static TOPMOST_EMITTER: AtomicUsize = AtomicUsize::new(0);
/// The widget layer's private CommandList — valid ONLY during the
/// emitter call (published before, cleared after).
static TOPMOST_LIST: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static WARNED_NO_WIDGET_LAYER: AtomicBool = AtomicBool::new(false);

/// Register the per-frame topmost emitter (SMX overlay). One consumer.
pub fn set_topmost_emitter(f: fn()) {
    TOPMOST_EMITTER.store(f as usize, Ordering::Release);
}

pub fn clear_topmost_emitter() {
    TOPMOST_EMITTER.store(0, Ordering::Release);
}

/// Whether topmost emission is available (dispatcher detour installed —
/// the same availability as the animated backgrounds).
pub fn topmost_ready() -> bool {
    DISPATCHER_HOOKED.load(Ordering::Acquire)
}

/// Resolve the widget layer's private CommandList from the layer table:
/// the override entry whose layer object == the render-list manager the
/// DLL's widgets live in, and whose walk flags say it was composed this
/// frame. Null on any failure (one WARN).
unsafe fn resolve_widget_layer_list() -> *mut u8 {
    let global = LAYER_TABLE_GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return std::ptr::null_mut();
    }
    let table = *(global as *const *const u8);
    if table.is_null() {
        return std::ptr::null_mut();
    }
    let widget_mgr = crate::services::widget_renderer::render_list_manager();
    if widget_mgr.is_null() {
        return std::ptr::null_mut();
    }
    for i in 0..11usize {
        let entry = table.add(i * 0x18);
        let override_ptr = *(entry as *const *mut u8);
        let layer = *(entry.add(8) as *const *const u8);
        if override_ptr.is_null() || layer.is_null() {
            continue;
        }
        if layer != widget_mgr as *const u8 {
            continue;
        }
        // Same walk conditions as the dispatcher: only append when the
        // layer was actually composed this frame.
        if *layer.add(0x10) == 0 && *layer.add(0x12) != 0 {
            return override_ptr;
        }
        return std::ptr::null_mut(); // found but not composed this frame
    }
    std::ptr::null_mut()
}

/// Build + append records to the widget layer's private list, topmost.
/// ONLY callable from inside the registered topmost emitter (the list is
/// published around that call). The closure receives a [`RecordWriter`]
/// whose base is the final destination (self-contained payload pointers
/// stay valid). Fail-open with the shared WARN classes; returns whether
/// the block was appended.
pub fn with_topmost_writer(build: impl FnOnce(&mut RecordWriter)) -> bool {
    unsafe {
        let cl = TOPMOST_LIST.load(Ordering::Acquire);
        if cl.is_null() {
            warn_once(
                &WARNED_NO_WIDGET_LAYER,
                "widget layer list unavailable -- topmost overlay not drawn",
            );
            return false;
        }
        let size = *(cl.add(0x0C) as *const u32);
        let write = *(cl.add(0x10) as *const *mut u8);
        let base = *(cl.add(0x18) as *const *const u8);
        if write.is_null() || base.is_null() {
            warn_once(&WARNED_NO_LIST, "arena pointers null (topmost)");
            return false;
        }
        if (write as usize) != (base as usize) + size as usize {
            warn_once(
                &WARNED_BUMP_MISMATCH,
                "arena bump invariant violated -- refusing to emit (topmost)",
            );
            return false;
        }
        if size > ARENA_SOFT_CAP {
            warn_once(
                &WARNED_ARENA_CAP,
                "arena size beyond soft cap -- refusing to emit (topmost)",
            );
            return false;
        }

        let mut w = RecordWriter::new(write as u64);
        build(&mut w);
        if w.is_empty() {
            return true;
        }
        let bytes = w.bytes();
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), write, bytes.len());
        let new_size = size + bytes.len() as u32;
        *(cl.add(0x0C) as *mut u32) = new_size;
        *(cl.add(0x10) as *mut *mut u8) = (base as *mut u8).add(new_size as usize);
        true
    }
}

/// The default-shader pointer + program count for topmost consumers
/// (binding program 0 around textured draws).
pub fn default_shader_ptr() -> *const u8 {
    let (shader, progs) = unsafe { read_default_shader() };
    if shader.is_null() || progs < 1 || progs > MAX_PLAUSIBLE_PROGRAMS {
        std::ptr::null()
    } else {
        shader
    }
}

/// Append a batch of untextured quads to the ACTIVE command list, bound
/// to the stock default-shader program 0 (mid-walk the current shader
/// binding is arbitrary — an explicit stock bind keeps the quads
/// correct and doubles as the state the layer's later records expect).
/// ONLY legal from inside an aux-emitter call (the active list is the
/// engine's own installation for the walk in progress). Fail-open:
/// every gate failure skips the frame with one latched WARN class.
/// Returns whether the batch was emitted.
pub fn emit_overlay_quads(quads: &[encode::Quad]) -> bool {
    if quads.is_empty() {
        return true;
    }
    unsafe {
        let (shader, progs) = read_default_shader();
        if shader.is_null() || progs < 1 || progs > MAX_PLAUSIBLE_PROGRAMS {
            warn_once(
                &WARNED_NO_SHADER,
                "default shader unresolved -- overlay quads unavailable",
            );
            return false;
        }
        let cl = render_notes_hook::active_command_list();
        if cl.is_null() {
            warn_once(&WARNED_NO_LIST, "active command list null at aux anchor");
            return false;
        }
        let size = *(cl.add(0x0C) as *const u32);
        let write = *(cl.add(0x10) as *const *mut u8);
        let base = *(cl.add(0x18) as *const *const u8);
        if write.is_null() || base.is_null() {
            warn_once(&WARNED_NO_LIST, "arena pointers null (aux)");
            return false;
        }
        if (write as usize) != (base as usize) + size as usize {
            warn_once(
                &WARNED_BUMP_MISMATCH,
                "arena bump invariant violated -- refusing to emit (aux)",
            );
            return false;
        }
        if size > ARENA_SOFT_CAP {
            warn_once(
                &WARNED_ARENA_CAP,
                "arena size beyond soft cap -- refusing to emit (aux)",
            );
            return false;
        }

        let mut w = RecordWriter::new(write as u64);
        w.set_context_2d(1280.0, 720.0, 0.0, 0.0);
        w.set_shader(shader as u64, 0);
        w.quads_untextured(quads);
        let bytes = w.bytes();

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), write, bytes.len());
        let new_size = size + bytes.len() as u32;
        *(cl.add(0x0C) as *mut u32) = new_size;
        *(cl.add(0x10) as *mut *mut u8) = (base as *mut u8).add(new_size as usize);
        true
    }
}

// Latched one-shot WARN classes (fail-open diagnostics, never spam).
static WARNED_NO_LIST: AtomicBool = AtomicBool::new(false);
static WARNED_BUMP_MISMATCH: AtomicBool = AtomicBool::new(false);
static WARNED_ARENA_CAP: AtomicBool = AtomicBool::new(false);
static WARNED_NO_SHADER: AtomicBool = AtomicBool::new(false);
static WARNED_PROG_COUNT: AtomicBool = AtomicBool::new(false);

fn warn_once(flag: &AtomicBool, msg: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        log_warn!("overlay_draw: {}", msg);
    }
}

/// Resolve the derived globals and install the layer-dispatcher detour.
/// Call once from lib.rs init after signature resolution (fail-open:
/// missing pieces only disable the corresponding gates — the menu then
/// greys ANIMATED BACKGROUND via [`emitter_ready`]).
pub fn init(signatures: &SignatureStore) {
    if std::env::var("DDR_OVERLAY_DRAW_STOCK_BIND").as_deref() == Ok("1") {
        STOCK_BIND_DEBUG.store(true, Ordering::Relaxed);
        log_info!("overlay_draw: STOCK-BIND debug active (program 0, plain half-black quad)");
    }
    match signatures.get_address("default_shader") {
        Some(a) => DEFAULT_SHADER_GLOBAL.store(a as *mut u8, Ordering::Release),
        None => log_warn!(
            "overlay_draw: default_shader global not resolved -- shader binds unavailable"
        ),
    }

    let table = signatures.get_address("layer_table");
    let dispatcher = signatures.get_address("layer_dispatcher");
    match (table, dispatcher) {
        (Some(t), Some(d)) => {
            LAYER_TABLE_GLOBAL.store(t as *mut u8, Ordering::Release);
            unsafe {
                let target: DispatcherFn = std::mem::transmute(d);
                match crate::core::hooks::install_enabled(
                    std::ptr::addr_of_mut!(DISPATCHER_HOOK),
                    target,
                    dispatcher_hook,
                ) {
                    Ok(()) => {
                        DISPATCHER_HOOKED.store(true, Ordering::Release);
                        log_info!("overlay_draw: layer-dispatcher detour installed");
                    }
                    Err(e) => log_warn!(
                        "overlay_draw: layer-dispatcher hook failed ({}) -- animated backgrounds unavailable",
                        e
                    ),
                }
            }
        }
        _ => log_warn!(
            "overlay_draw: layer_dispatcher/layer_table unresolved -- animated backgrounds unavailable"
        ),
    }
}

/// The layer-dispatcher detour: forward first, then run the registered
/// topmost emitter (SMX overlay) against the widget layer's private
/// list — records appended after the dispatcher's own recording draw
/// LAST (topmost), and the orchestrator's consumer kick hasn't run yet
/// (we're inside its dispatcher call), so the append is same-frame-safe.
/// Installing this detour also proves the layer machinery resolved on
/// this build (`emitter_ready` / `topmost_ready`).
extern "C" fn dispatcher_hook() {
    unsafe {
        if let Some(ref hook) = *std::ptr::addr_of!(DISPATCHER_HOOK) {
            hook.call();
        }
        let emitter = TOPMOST_EMITTER.load(Ordering::Acquire);
        if emitter != 0 {
            TOPMOST_LIST.store(resolve_widget_layer_list(), Ordering::Release);
            // SAFETY: stored from a valid `fn()` pointer.
            let f: fn() = std::mem::transmute(emitter);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            TOPMOST_LIST.store(std::ptr::null_mut(), Ordering::Release);
        }
    }
}

/// Per-wrapper-render tick — per-scene diagnostics only (emission moved to
/// the layer-dispatcher detour). Panic-contained here (extern "C" caller).
pub fn on_wrapper_render() {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(diag_tick));
}

/// A gate failed while the background was active: count toward the session
/// latch (design §6 — repeated failure must not spam retries forever).
fn note_gate_failure() {
    let streak = FAIL_STREAK.fetch_add(1, Ordering::Relaxed) + 1;
    if streak >= FAILURE_LATCH_THRESHOLD && !SESSION_LATCHED_OFF.swap(true, Ordering::Relaxed) {
        log_warn!(
            "overlay_draw: {} consecutive emission failures -- animated background latched off for the session",
            streak
        );
    }
}

/// Once-per-scene-id diagnostics: active list pointer + arena fields +
/// default-shader program count. The spike's stage-2 evidence.
fn diag_tick() {
    let scene = scene_manager::current_scene();
    if scene == LAST_DIAG_SCENE.load(Ordering::Relaxed) {
        return;
    }
    LAST_DIAG_SCENE.store(scene, Ordering::Relaxed);

    {
        let Ok(mut seen) = DIAG_SCENES.lock() else {
            return;
        };
        if seen.contains(&scene) || seen.len() >= DIAG_SCENE_CAP {
            return;
        }
        seen.push(scene);
    }

    unsafe {
        let cl = render_notes_hook::active_command_list();
        if cl.is_null() {
            log_info!(
                "overlay_draw diag: scene={} active_command_list=null",
                scene
            );
            return;
        }
        let size = *(cl.add(0x0C) as *const u32);
        let write = *(cl.add(0x10) as *const *const u8);
        let base = *(cl.add(0x18) as *const *const u8);
        let bump_ok = !write.is_null()
            && !base.is_null()
            && (write as usize) == (base as usize) + size as usize;

        let (shader, progs) = read_default_shader();

        log_info!(
            "overlay_draw diag: scene={} cl={:p} size=0x{:X} write={:p} base={:p} bump_ok={} default_shader={:p} progs={}",
            scene, cl, size, write, base, bump_ok, shader, progs
        );
    }
}

/// Read the default shader object pointer + its program count (0 when any
/// link is null/implausible).
unsafe fn read_default_shader() -> (*const u8, u32) {
    let global = DEFAULT_SHADER_GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return (std::ptr::null(), 0);
    }
    let shader = *(global as *const *const u8);
    if shader.is_null() {
        return (std::ptr::null(), 0);
    }
    let progs = *(shader.add(4) as *const u32);
    (shader, progs)
}

/// The menu anchor's `wrapper_render` fired (pre-original, from
/// `widget_renderer::wrapper_render_hook`). Identity-gated one atomic
/// compare for every non-anchor wrapper. Panic-contained here (the caller
/// is an extern "C" frame).
pub fn on_anchor_render(wrapper: *mut u8) {
    // Aux anchor (SMX touch overlay quads).
    if wrapper as usize == AUX_ANCHOR.load(Ordering::Acquire) {
        let emitter = AUX_EMITTER.load(Ordering::Relaxed);
        if emitter != 0 {
            // SAFETY: stored from a valid `fn()` pointer.
            let f: fn() = unsafe { std::mem::transmute(emitter) };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        }
        return;
    }
    if wrapper as usize != EMIT_ANCHOR.load(Ordering::Acquire) {
        return;
    }
    if !BG_ACTIVE.load(Ordering::Relaxed) || SESSION_LATCHED_OFF.load(Ordering::Relaxed) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(emit_background));
}

/// The menu anchor's `wrapper_render` returned (post-original). Re-arm the
/// anchor's dirty flag while the background is active so the game's walk
/// keeps dispatching it every frame (the render pass clears the flag; a
/// clean static wrapper is served from a cached path — round-1 finding).
pub fn on_anchor_rendered(wrapper: *mut u8) {
    // Aux anchor: re-arm unconditionally while installed (the emitter's
    // own gates make an idle frame O(1)).
    if wrapper as usize == AUX_ANCHOR.load(Ordering::Acquire) {
        let dirty = AUX_DIRTY.load(Ordering::Relaxed);
        if dirty != 0 {
            unsafe { *(dirty as *mut u8) = 1 };
        }
        return;
    }
    if wrapper as usize != EMIT_ANCHOR.load(Ordering::Acquire) {
        return;
    }
    if !BG_ACTIVE.load(Ordering::Relaxed) || SESSION_LATCHED_OFF.load(Ordering::Relaxed) {
        return;
    }
    let dirty = ANCHOR_DIRTY.load(Ordering::Relaxed);
    if dirty != 0 {
        unsafe { *(dirty as *mut u8) = 1 };
    }
}

/// Emit the animated-background block into the ACTIVE command list at the
/// menu anchor's render — mid-walk in the widget layer, above everything
/// the layer drew earlier this frame (incl. full-screen loading art) and
/// below the panel + text widgets registered after the anchor. The active
/// list at a wrapper render is the engine's own installation for the walk
/// in progress (the verified-safe append surface — the Step 2 spike's
/// site). Gate ladder unchanged: every read range-validated, every
/// failure fail-open + one WARN class + a tick toward the session latch.
fn emit_background() {
    let (shader, progs) = unsafe { read_default_shader() };
    if shader.is_null() {
        warn_once(&WARNED_NO_SHADER, "default shader unresolved");
        note_gate_failure();
        return;
    }
    let program = BG_PROGRAM.load(Ordering::Relaxed);
    // MANDATORY: the SetShader handler has no bounds check — never bind a
    // program the container doesn't carry.
    if progs < program + 1 || progs > MAX_PLAUSIBLE_PROGRAMS {
        warn_once(
            &WARNED_PROG_COUNT,
            "default shader program count below the theme index -- refusing to bind",
        );
        note_gate_failure();
        return;
    }
    let packed = BG_RECT.load(Ordering::Relaxed);
    let (rx, ry, rw, rh) = (
        (packed >> 48) as u16,
        (packed >> 32) as u16,
        (packed >> 16) as u16,
        packed as u16,
    );
    if rw == 0 || rh == 0 {
        note_gate_failure();
        return;
    }

    unsafe {
        let cl = render_notes_hook::active_command_list();
        if cl.is_null() {
            warn_once(&WARNED_NO_LIST, "active command list null at anchor render");
            note_gate_failure();
            return;
        }

        let size = *(cl.add(0x0C) as *const u32);
        let write = *(cl.add(0x10) as *const *mut u8);
        let base = *(cl.add(0x18) as *const *const u8);
        if write.is_null() || base.is_null() {
            warn_once(&WARNED_NO_LIST, "arena pointers null");
            note_gate_failure();
            return;
        }
        if (write as usize) != (base as usize) + size as usize {
            warn_once(
                &WARNED_BUMP_MISMATCH,
                "arena bump invariant violated (write != base+size) -- refusing to emit",
            );
            note_gate_failure();
            return;
        }
        if size > ARENA_SOFT_CAP {
            warn_once(
                &WARNED_ARENA_CAP,
                "arena size beyond soft cap -- refusing to emit",
            );
            note_gate_failure();
            return;
        }

        let time_s = (EPOCH.elapsed().as_millis() % TIME_WRAP_MS) as f32 / 1000.0;
        let p0 = f32::from_bits(BG_PARAM0.load(Ordering::Relaxed));
        let p1 = f32::from_bits(BG_PARAM1.load(Ordering::Relaxed));

        // Build the block against the exact destination address (payload
        // pointers are absolute), then copy + bump once.
        let stock_debug = STOCK_BIND_DEBUG.load(Ordering::Relaxed);
        let (bind_program, quad_color) = if stock_debug {
            // Test-B debug: stock program 0 + a plain 50%-black quad (the
            // Step 2 POC shape) — no theme PS in the path.
            (0u32, 0x8000_0000u32)
        } else {
            (program, (BG_ALPHA.load(Ordering::Relaxed) & 0xFF) << 24)
        };
        let mut w = RecordWriter::new(write as u64);
        w.set_context_2d(1280.0, 720.0, 0.0, 0.0);
        // NO scissor records: the quad's corners already trace the exact
        // modal rect (the theme PS rounds the corners via the SDF mask),
        // so the scissor added nothing (and complicates state restore
        // mid-walk — the layer's own records follow ours).
        w.set_vs_const_f(
            0,
            &[
                [time_s, rx as f32, ry as f32, 0.0],
                [rw as f32, rh as f32, p0, p1],
            ],
        );
        w.set_shader(shader as u64, bind_program);
        w.quads_untextured(&[Quad {
            corners: [
                [rx as f32, ry as f32],
                [(rx + rw) as f32, ry as f32],
                [(rx + rw) as f32, (ry + rh) as f32],
                [rx as f32, (ry + rh) as f32],
            ],
            // Black base; the alpha byte is the MENU OPACITY mapping — the
            // theme PS multiplies its output alpha by this vertex alpha,
            // so gameplay shows through the animation exactly like it does
            // through the static panel.
            color: quad_color,
        }]);
        // Full state restore: back to the stock program.
        w.set_shader(shader as u64, 0);
        let bytes = w.bytes();

        std::ptr::copy_nonoverlapping(bytes.as_ptr(), write, bytes.len());
        let new_size = size + bytes.len() as u32;
        *(cl.add(0x0C) as *mut u32) = new_size;
        *(cl.add(0x10) as *mut *mut u8) = (base as *mut u8).add(new_size as usize);

        FAIL_STREAK.store(0, Ordering::Relaxed);

        if !FIRST_EMIT_LOGGED.swap(true, Ordering::Relaxed) {
            log_info!(
                "overlay_draw: animated background emitting at anchor (program={}, rect={},{},{},{}, list={:p}, pre_size=0x{:X})",
                program, rx, ry, rw, rh, cl, size
            );
        }
        let n = EMIT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if n % 600 == 0 {
            log_info!(
                "overlay_draw: background alive -- {} emissions (scene={}, program={}, list={:p}, arena size 0x{:X})",
                n,
                scene_manager::current_scene(),
                program,
                cl,
                new_size
            );
        }
    }
}
