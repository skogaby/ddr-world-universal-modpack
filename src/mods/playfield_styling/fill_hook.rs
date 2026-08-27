//! Fill hook — the shared per-quad transform (design §4.2).
//!
//! One detour on `render_sprite_final`, the ArrowSprite per-quad filler that
//! every lane renderer reaches via real `CALL`s: normal arrows, freeze
//! head/body/tail, shock + electric overlay (`ArrowRenderer`), the receptor
//! row (`SpotRenderer`), the expanding hit flash (`JudgeEffectRenderer`) —
//! and `mine_render`'s own mine quads, which call the same entry point and
//! therefore inherit the transform automatically.
//!
//! The fill's `(x, y)` args are **lane-relative** (x = 96·dir, y = scroll
//! offset from the receptor row); the original adds the lane origin
//! (`posX/posY @ this+0x30/+0x34`) and applies reverse mirroring, rotation,
//! and appearance alpha AFTER our transform runs. So:
//!
//!   x' = cx + s·(x − cx)   (cx = lane half-width → anchors on lane center)
//!   y' = s·y               (anchors on the receptor row; commutes with
//!                           reverse negation)
//!   w' = s·w,  h' = s·h
//!   color.a' = a·op        (copied to a stack local — NEVER mutate through
//!                           the game's pointer; the game's own appearance /
//!                           fade alphas multiply on top inside the original)
//!
//! ## Renderer registry
//!
//! Fixed 16-slot `static mut` array (render-thread only, `addr_of!` access,
//! `REGISTRY_LEN` early-out — the `overlay_element_styling::capture`
//! discipline). Instances are classified by vtable (offset-0 vftable
//! pointer vs. the three RTTI-resolved vtables) and bound to a side via the
//! presence read / posX-split; unknown vtables are forwarded untouched and
//! never tracked. The registry is cleared at GAMEPLAY enter/exit (renderers
//! are per-song objects).
//!
//! ## Hot-path budget
//!
//! Runs per quad (typically < 100/frame; a few hundred worst-case with the
//! extended cull window). Work per call: two atomic loads + one ≤16-slot
//! pointer scan + 4 float mults. No locks, no allocation, no logging.

use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use retour::GenericDetour;

use crate::{log_info, log_warn};

use super::MOD_ID;

// ── ArrowSprite / renderer field offsets (Ghidra-verified; research §2/§5) ──

/// Lane origin X on the ArrowSprite base (f32). Read for the versus side
/// split (posX < 640 → P1).
const OFF_POS_X: usize = 0x30;
/// Lane origin Y on the ArrowSprite base (f32) — the screen Y of the
/// receptor row (the fill's `y` args are offsets from it, so it is the
/// vertical fixed point of the `y' = s·y` transform). Exposed to
/// `lane_hook` as the receptor-flash reposition anchor.
const OFF_POS_Y: usize = 0x34;
/// Mode enum (0 = single, 1 = double) on `screen::ArrowRenderer`.
const OFF_ARROW_MODE: usize = 0xB0;
/// Mode enum on `screen::SpotRenderer`.
const OFF_SPOT_MODE: usize = 0x98;

/// Versus playfield midline (screen units, 1280-wide space).
const X_SPLIT: f32 = 640.0;

/// Lane half-widths: 4 panels × 96 / 2, 8 panels × 96 / 2. The transform's
/// center-X anchor (`cx`) — lane-relative, so a lane shifted by
/// `center_arrows_single` scales about its shifted center automatically.
const HALF_WIDTH_SINGLE: f32 = 192.0;
const HALF_WIDTH_DOUBLE: f32 = 384.0;

// ── Registry (game render thread only) ──────────────────────────────

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub(crate) enum RendererClass {
    Arrow = 0,
    Spot = 1,
    JudgeEffect = 2,
}

impl RendererClass {
    fn name(self) -> &'static str {
        match self {
            RendererClass::Arrow => "arrow",
            RendererClass::Spot => "spot",
            RendererClass::JudgeEffect => "judge_effect",
        }
    }
}

#[derive(Clone, Copy)]
struct TrackedRenderer {
    /// Renderer `this` — the identity key. `null` = free slot.
    this: *mut u8,
    side: u8,
    half_width: f32,
    /// Screen Y of the receptor row (`posY @ +0x34`) — the fill transform's
    /// vertical fixed point.
    pos_y: f32,
    class: RendererClass,
}

const EMPTY: Option<TrackedRenderer> = None;

/// 2 sides × 3 renderer classes = 6 live entries worst case; 16 slots is
/// ample headroom.
const REGISTRY_CAP: usize = 16;

static mut REGISTRY: [Option<TrackedRenderer>; REGISTRY_CAP] = [EMPTY; REGISTRY_CAP];

/// Occupied-slot count. All registry access is single-threaded (render
/// thread); this is the hot-path `== 0` early-out plus a publication fence.
static REGISTRY_LEN: AtomicUsize = AtomicUsize::new(0);

/// Per-class bind counter for the per-song exit summary.
static CAPTURE_COUNT: [AtomicUsize; 3] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

/// One-shot latches for log-once warnings (reset per song via `clear`).
static OVERFLOW_WARNED: AtomicBool = AtomicBool::new(false);
static PANIC_WARNED: AtomicBool = AtomicBool::new(false);

/// True between GAMEPLAY entry and exit (set by the mod's scene callback).
/// Outside gameplay the callback forwards immediately — no registry work.
static IN_GAMEPLAY: AtomicBool = AtomicBool::new(false);

/// Player-object array global (presence read), set at install from the
/// mod's resolved targets. 0 = unresolved (side binding degrades to defer).
static PLAYER_ARRAY: AtomicU64 = AtomicU64::new(0);

/// Seed the player-object array without a full install (used by the shared
/// guideline hooks when player_perspective acquires them while
/// playfield_styling is config-disabled). Keeps an existing value.
pub(super) fn seed_player_array(player_array: *const u8) {
    if !player_array.is_null() {
        let _ = PLAYER_ARRAY.compare_exchange(
            0,
            player_array as u64,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// The three RTTI-resolved offset-0 vftable addresses (Arrow, Spot,
/// JudgeEffect), set at install. 0 = unresolved.
static VTABLE_ARROW: AtomicU64 = AtomicU64::new(0);
static VTABLE_SPOT: AtomicU64 = AtomicU64::new(0);
static VTABLE_JUDGE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn set_in_gameplay(v: bool) {
    IN_GAMEPLAY.store(v, Ordering::Release);
}

/// Whether the GAMEPLAY scene is currently active (set by the mod's scene
/// callback). Used by the lane hook to confine its engine-wide pool-create /
/// note-result detours to the gameplay window.
pub(super) fn in_gameplay() -> bool {
    IN_GAMEPLAY.load(Ordering::Acquire)
}

/// Read the two per-side "is playing" flags via the triple-deref the game's
/// own per-side accessors use: `*(*(*slot) + 4) != 0` (slot = array +
/// side*8). Returns `(false, false)` if the array is unresolved or any
/// pointer is null. (Cabinet-validated port from `center_arrows_single`;
/// also used by the guideline capture detour.)
pub(super) fn read_presence() -> (bool, bool) {
    let array = PLAYER_ARRAY.load(Ordering::Acquire) as *const *const *const u8;
    if array.is_null() {
        return (false, false);
    }
    unsafe {
        let present = |slot_index: usize| -> bool {
            let p1 = array.add(slot_index).read_unaligned();
            if p1.is_null() {
                return false;
            }
            let player = p1.read_unaligned();
            !player.is_null() && player.add(0x4).read_unaligned() != 0
        };
        (present(0), present(1))
    }
}

/// Clear the registry (GAMEPLAY enter/exit, mod disable). `log_stats`
/// emits the per-song capture summary before zeroing.
pub(crate) fn clear_registry(log_stats: bool) {
    if log_stats {
        let counts: [usize; 3] = std::array::from_fn(|i| CAPTURE_COUNT[i].load(Ordering::Relaxed));
        if counts.iter().any(|&c| c > 0) {
            log_info!(
                "{MOD_ID}: song captures — arrow={} spot={} judge_effect={}",
                counts[0],
                counts[1],
                counts[2]
            );
        }
    }
    for c in &CAPTURE_COUNT {
        c.store(0, Ordering::Relaxed);
    }
    unsafe {
        let reg = &mut *addr_of_mut!(REGISTRY);
        for slot in reg.iter_mut() {
            *slot = EMPTY;
        }
    }
    REGISTRY_LEN.store(0, Ordering::Release);
    OVERFLOW_WARNED.store(false, Ordering::Relaxed);
}

/// Find a tracked renderer by `this`. Render-thread only.
fn find(this: *mut u8) -> Option<TrackedRenderer> {
    if REGISTRY_LEN.load(Ordering::Acquire) == 0 {
        return None;
    }
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        reg.iter().flatten().find(|t| t.this == this).copied()
    }
}

/// Look up the half-width of the Arrow/Spot renderer bound to `side`, if
/// any (JudgeEffect inherits it — it has no verified mode field).
fn side_half_width(side: u8) -> Option<f32> {
    if REGISTRY_LEN.load(Ordering::Acquire) == 0 {
        return None;
    }
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        reg.iter()
            .flatten()
            .find(|t| t.side == side && !matches!(t.class, RendererClass::JudgeEffect))
            .map(|t| t.half_width)
    }
}

/// Screen Y of `side`'s receptor row (the fill transform's vertical fixed
/// point) — from the bound Spot renderer (the receptor row itself),
/// falling back to Arrow (same lane origin). Used by `lane_hook` to
/// reposition the receptor hit flash. `None` until a renderer binds.
pub(crate) fn side_anchor_y(side: u8) -> Option<f32> {
    if REGISTRY_LEN.load(Ordering::Acquire) == 0 {
        return None;
    }
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        reg.iter()
            .flatten()
            .filter(|t| t.side == side && !matches!(t.class, RendererClass::JudgeEffect))
            .min_by_key(|t| t.class as u8 != RendererClass::Spot as u8)
            .map(|t| t.pos_y)
    }
}

/// Resolve a renderer's side from the presence read + posX split:
/// single active side → that side; versus → `posX < 640` → P1; neither
/// present → `None` (defer binding).
fn determine_side(pos_x: f32) -> Option<u8> {
    match read_presence() {
        (true, false) => Some(0),
        (false, true) => Some(1),
        (true, true) => Some(if pos_x < X_SPLIT { 0 } else { 1 }),
        (false, false) => None,
    }
}

/// Classify an unknown `this` by its offset-0 vftable pointer. Unknown
/// vtable → `None` (forward untouched, never track).
fn classify(this: *mut u8) -> Option<RendererClass> {
    let vptr = unsafe { (this as *const u64).read_unaligned() };
    if vptr == VTABLE_ARROW.load(Ordering::Relaxed) {
        Some(RendererClass::Arrow)
    } else if vptr == VTABLE_SPOT.load(Ordering::Relaxed) {
        Some(RendererClass::Spot)
    } else if vptr == VTABLE_JUDGE.load(Ordering::Relaxed) {
        Some(RendererClass::JudgeEffect)
    } else {
        None
    }
}

/// Bind an unseen renderer: classify → side → half-width → insert. Returns
/// the bound entry, or `None` when the quad should pass through unstyled
/// (unknown class, deferred bind, or registry overflow).
fn bind(this: *mut u8) -> Option<TrackedRenderer> {
    let class = classify(this)?;

    let pos_x = unsafe { (this.add(OFF_POS_X) as *const f32).read_unaligned() };
    let pos_y = unsafe { (this.add(OFF_POS_Y) as *const f32).read_unaligned() };

    let (side, half_width) = match class {
        RendererClass::Arrow | RendererClass::Spot => {
            let mode_off = if matches!(class, RendererClass::Arrow) {
                OFF_ARROW_MODE
            } else {
                OFF_SPOT_MODE
            };
            let mode = unsafe { (this.add(mode_off) as *const i32).read_unaligned() };
            if mode == 1 {
                // Doubles: one renderer set spans 8 panels as side 0 (A7).
                (0u8, HALF_WIDTH_DOUBLE)
            } else {
                (determine_side(pos_x)?, HALF_WIDTH_SINGLE)
            }
        }
        RendererClass::JudgeEffect => {
            // No verified mode field — inherit the side's half-width from
            // the Arrow/Spot renderer already bound to that side (they bind
            // first in the frame's draw order; defer until one exists).
            let side = determine_side(pos_x)?;
            match side_half_width(side) {
                Some(hw) => (side, hw),
                None => {
                    // Doubles binds Arrow/Spot to side 0 regardless of
                    // which player carded in (mode==1 short-circuits the
                    // presence read above). If the presence read attributed
                    // this side but only the other side carries a bound
                    // DOUBLE-width renderer, follow it.
                    let other = 1 - side;
                    match side_half_width(other) {
                        Some(hw) if hw == HALF_WIDTH_DOUBLE => (other, hw),
                        _ => return None, // defer: retry on a later quad
                    }
                }
            }
        }
    };

    let entry = TrackedRenderer {
        this,
        side,
        half_width,
        pos_y,
        class,
    };
    unsafe {
        let reg = &mut *addr_of_mut!(REGISTRY);
        match reg.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(entry);
                CAPTURE_COUNT[class as usize].fetch_add(1, Ordering::Relaxed);
                REGISTRY_LEN.fetch_add(1, Ordering::Release);
            }
            None => {
                if !OVERFLOW_WARNED.swap(true, Ordering::Relaxed) {
                    log_warn!(
                        "{MOD_ID}: renderer registry full ({REGISTRY_CAP} slots) — styling partial this song"
                    );
                }
                return None;
            }
        }
    }

    log_info!(
        "{MOD_ID}: bind class={} side={} half_width={} posX={:.0} posY={:.0}",
        class.name(),
        side,
        half_width,
        pos_x,
        pos_y
    );
    Some(entry)
}

// ── Detour ──────────────────────────────────────────────────────────

/// `render_sprite_final` — ArrowSprite per-quad filler (final overload).
/// `(this, &sprite /*0x34-byte ROTATESPRITE*/, x, y, w, h, &uv[4], twist,
/// &color /*COLOR4B*/)`.
type RenderSpriteFinalFn = unsafe extern "C" fn(
    this: *mut u8,
    sprite: *mut u8,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: *const f32,
    twist: f32,
    color: *const u8,
);

static mut FILL_HOOK: Option<GenericDetour<RenderSpriteFinalFn>> = None;

/// The computed replacement args for one quad: geometry + the copied,
/// opacity-composed color (owned locally — the incoming `color` may point
/// into game memory and is never written through) + optionally an inset
/// copy of the UV rect (`None` = pass the original pointer through).
struct QuadPlan {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [u8; 4],
}

/// Registry lookup/bind + transform math. Panic-free by construction, but
/// the caller still wraps it in `catch_unwind`. Returns `None` when the
/// quad should be forwarded untouched.
fn plan_quad(this: *mut u8, x: f32, y: f32, w: f32, h: f32, color: *const u8) -> Option<QuadPlan> {
    if this.is_null() || color.is_null() {
        return None;
    }
    let tracked = match find(this) {
        Some(t) => t,
        None => bind(this)?,
    };
    let (s, op) = super::latched(tracked.side as usize);
    if s == 1.0 && op == 1.0 {
        return None; // identity fast-path: zero behavioral delta
    }

    let cx = tracked.half_width;
    let mut c = [0u8; 4];
    unsafe {
        std::ptr::copy_nonoverlapping(color, c.as_mut_ptr(), 4);
    }
    if op != 1.0 {
        // Alpha is byte 3 of the COLOR4B; the game's own appearance / fade
        // alphas multiply on top inside the original.
        c[3] = (c[3] as f32 * op).round().clamp(0.0, 255.0) as u8;
    }
    Some(QuadPlan {
        x: cx + s * (x - cx),
        y: s * y,
        w: s * w,
        h: s * h,
        color: c,
    })
}

unsafe extern "C" fn fill_hook_cb(
    this: *mut u8,
    sprite: *mut u8,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    uv: *const f32,
    twist: f32,
    color: *const u8,
) {
    let hook = match &*addr_of!(FILL_HOOK) {
        Some(h) => h,
        None => return,
    };

    // Fast path: mod off or outside gameplay → forward untouched.
    if !super::is_enabled() || !IN_GAMEPLAY.load(Ordering::Acquire) {
        return hook.call(this, sprite, x, y, w, h, uv, twist, color);
    }

    // Deferred lane-clip styling: captured lane clips are applied on the
    // first quad AFTER their capture — by then the HUD build (which reads
    // marker positions out of the lane clip) is complete. One atomic load
    // per quad when idle.
    if super::lane_hook::has_pending() {
        super::lane_hook::apply_pending();
    }

    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        plan_quad(this, x, y, w, h, color)
    })) {
        Ok(Some(p)) => hook.call(
            this,
            sprite,
            p.x,
            p.y,
            p.w,
            p.h,
            uv,
            twist,
            p.color.as_ptr(),
        ),
        Ok(None) => hook.call(this, sprite, x, y, w, h, uv, twist, color),
        Err(_) => {
            if !PANIC_WARNED.swap(true, Ordering::Relaxed) {
                crate::log_error!("{MOD_ID}: fill callback panicked — forwarding untouched");
            }
            hook.call(this, sprite, x, y, w, h, uv, twist, color)
        }
    }
}

// ── Install / remove ────────────────────────────────────────────────

/// Install the fill detour (load-bearing). Publishes the vtables + player
/// array for the bind path first, then installs via `install_enabled`
/// (store-before-enable). Returns false on failure.
pub(crate) fn install(t: &super::ResolvedTargets) -> bool {
    VTABLE_ARROW.store(t.arrow_renderer_vtable as u64, Ordering::Relaxed);
    VTABLE_SPOT.store(t.spot_renderer_vtable as u64, Ordering::Relaxed);
    VTABLE_JUDGE.store(t.judge_effect_renderer_vtable as u64, Ordering::Relaxed);
    PLAYER_ARRAY.store(t.player_array as u64, Ordering::Release);

    unsafe {
        let target: RenderSpriteFinalFn = std::mem::transmute(t.fill);
        match crate::core::hooks::install_enabled(addr_of_mut!(FILL_HOOK), target, fill_hook_cb) {
            Ok(()) => {
                log_info!(
                    "{MOD_ID}: render_sprite_final hook installed @ {:p}",
                    t.fill
                );
                true
            }
            Err(e) => {
                log_warn!("{MOD_ID}: render_sprite_final hook install failed: {e}");
                false
            }
        }
    }
}

/// Tear down the fill detour and clear the registry.
pub(crate) fn remove() {
    unsafe {
        if let Some(d) = (*addr_of_mut!(FILL_HOOK)).take() {
            let _ = d.disable();
        }
    }
    clear_registry(false);
    PANIC_WARNED.store(false, Ordering::Relaxed);
}
