//! Clip capture — identifies every scoped overlay element per song by
//! detouring `CMovieClip::Create` (the template name arrives as a C string in
//! R8), and holds them in a fixed 64-slot registry.
//!
//! Later steps add side-binding + one-shot application (SetPosition detour,
//! Step 5) and opacity composition (color detours read the registry, Step 6).
//!
//! ## Threading
//!
//! All registry writers and readers run on the game thread: the Create /
//! SetPosition / SetColor detours during scene build + gameplay, and the
//! scene-change callback. The only cross-thread data is the option values,
//! mirrored into atomics in `mod.rs`. Hence the registry is a `static mut`
//! reached via `addr_of!` — no lock (design §4.3).

use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};

use retour::GenericDetour;

use crate::log_info;
use crate::services::bm2d_api;

use super::MOD_ID;

// ── Registry types (design §4.3, §5.3) ──────────────────────────────

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub(crate) enum ElementKind {
    Combo = 0,
    Judge = 1,
    FreezeJudge = 2,
    FastSlow = 3,
    Pacemaker = 4,
}

impl ElementKind {
    fn name(self) -> &'static str {
        match self {
            ElementKind::Combo => "combo",
            ElementKind::Judge => "judge",
            ElementKind::FreezeJudge => "freeze_judge",
            ElementKind::FastSlow => "fast_slow",
            ElementKind::Pacemaker => "pacemaker",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Side {
    Unbound,
    P1,
    P2,
}

#[derive(Clone, Copy)]
pub(crate) struct TrackedClip {
    /// Pool-wrapper pointer — the identity key. `null` = free slot.
    pub(crate) wrapper: *mut u8,
    /// The type-1 AFP layer id captured from `wrapper+0x08` at Create time.
    pub(crate) layer_id: u32,
    pub(crate) kind: ElementKind,
    pub(crate) side: Side,
    /// The element's original game-set position (the first SetPosition x/y).
    /// The anchored placement is computed from this, so it can be re-placed
    /// when the judgement anchor arrives without losing the true position.
    pub(crate) orig_x: i32,
    pub(crate) orig_y: i32,
    /// Bound: side + original position captured, opacity one-shot applied.
    /// (The scale/position matrix may be (re)written more than once — see
    /// `place` / `place_side` — but binding itself is one-shot.)
    pub(crate) bound: bool,
}

const EMPTY: TrackedClip = TrackedClip {
    wrapper: std::ptr::null_mut(),
    layer_id: 0,
    kind: ElementKind::Combo,
    side: Side::Unbound,
    orig_x: 0,
    orig_y: 0,
    bound: false,
};

/// 2 sides × 21 clips worst case (single: 3 combo + 1 judge + 7 freeze +
/// 1 fast_slow + 1 pacemaker = 13; double up to 21), rounded up.
pub(crate) const REGISTRY_CAP: usize = 64;

static mut REGISTRY: [TrackedClip; REGISTRY_CAP] = [EMPTY; REGISTRY_CAP];

/// Count of occupied registry slots. All registry access is single-threaded
/// (game thread), so this is really just a hot-path guard: the color compose
/// detour is engine-wide and always-on while the mod is enabled, so a `== 0`
/// early-out lets it skip the 64-slot scan entirely outside gameplay (design
/// §5.1 "defensive publication").
static REGISTRY_LEN: AtomicUsize = AtomicUsize::new(0);

/// Per-kind capture counter for this song's diagnostic summary (indexed by
/// `ElementKind as usize`). Reset on registry clear.
static CAPTURE_COUNT: [AtomicUsize; 5] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

/// One-shot registry-overflow warning latch (avoids log spam if a song ever
/// exceeds `REGISTRY_CAP`).
static OVERFLOW_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ── Cluster anchor (judgement text) for gap scaling ─────────────────
// Each scoped element scales about its own visual center. To also scale the
// SPACING between elements (so a shrunk cluster stays tight instead of drifting
// apart), we uniformly zoom every element's Y position about a common anchor —
// the judgement text — biasing the cluster toward the top of the playfield:
//
//   new_y = anchor_y + scale * (orig_y - anchor_y)   (X is left unchanged, so
//   per-panel freeze columns and lane-centered text keep their horizontal
//   layout). At 100 % this is identity; at 150 % the cluster zooms out from the
//   judge line, at 30 % it zooms in toward it.
//
// The anchor is the judge element's position, captured when it binds. Because
// element bind order isn't guaranteed, any element bound BEFORE the judge is
// re-placed once the judge sets the anchor (`place_side`). Per side; reset each
// song. All access is game-thread only (like REGISTRY).
static ANCHOR_X: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static ANCHOR_Y: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];
static ANCHOR_KNOWN: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

// ── Side binding (design §4.4) ──────────────────────────────────────

/// Versus playfield midline (layout units, 1280-wide space). Clips whose
/// first SetPosition x is `< X_SPLIT` bind to P1, else P2. Cabinet-validate
/// (design Appendix C.1) — the bind debug log records x per bind.
const X_SPLIT: i32 = 640;

/// Player-object array global (derived from `player_array_anchor` in
/// `mod.rs::init`). Two pointers, P1=`[0]`, P2=`[1]` at `+8`; each points at a
/// player object whose byte at `+0x4` is the authoritative "this side is
/// playing" flag. Reused verbatim from `center_arrows_single`. 0 = unresolved.
static PLAYER_ARRAY: AtomicU64 = AtomicU64::new(0);

/// True once the SetPosition side-binding detour is live. When false, the
/// Create detour applies the single-active-side fallback (design §4.4).
static SETPOS_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Shared-capture flag: another mod (s_marvelous' flash re-drive) relies on
/// this registry's `dance_judge` tracking even when the styling mod itself
/// is config-disabled. While set: the hook bodies TRACK (classify + bind)
/// regardless of `MOD_ENABLED`, the styling APPLY stays gated on the mod,
/// and `remove()` refuses to tear the detours down (styling's enable
/// rollback must not break the sharing consumer).
static SHARED_CAPTURE: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_shared_capture(on: bool) {
    SHARED_CAPTURE.store(on, Ordering::Release);
}

/// Whether the hook bodies should track clips at all.
fn tracking_enabled() -> bool {
    super::is_enabled() || SHARED_CAPTURE.load(Ordering::Acquire)
}

/// Store the derived player-object array global (called from `mod.rs::init`).
pub(crate) fn set_player_array(addr: *const u8) {
    PLAYER_ARRAY.store(addr as u64, Ordering::Release);
}

/// Read the two per-side "is playing" flags via the triple-deref the game's
/// own per-side accessors use: `*(*(*slot) + 4) != 0` (slot = array + side*8).
/// Returns `(false, false)` if the array is unresolved or any pointer is null.
/// Verbatim port of `center_arrows_single::read_presence` (cabinet-validated).
fn read_presence() -> (bool, bool) {
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

/// Resolve a clip's side at its first position write: single active side →
/// that side (x ignored); versus (both present) → x-threshold; neither
/// present → `None` (can't bind yet).
fn determine_side(x: i32) -> Option<Side> {
    match read_presence() {
        (true, false) => Some(Side::P1),
        (false, true) => Some(Side::P2),
        (true, true) => Some(if x < X_SPLIT { Side::P1 } else { Side::P2 }),
        (false, false) => None,
    }
}

// ── Name classification (design §5.3) ───────────────────────────────
// Exact matches first, then the combo prefix. `dance_judge` is matched only
// EXACTLY, so it can never swallow `dance_judge_for_freeze`, and the receptor
// flashes (`dance_effect`) match nothing → correctly excluded.

fn classify(name: &str) -> Option<ElementKind> {
    match name {
        "dance_judge" => Some(ElementKind::Judge),
        "dance_judge_for_freeze" => Some(ElementKind::FreezeJudge),
        "dance_fast_slow" => Some(ElementKind::FastSlow),
        "dance_score_compare" => Some(ElementKind::Pacemaker),
        _ if name.starts_with("dance_combo_root") => Some(ElementKind::Combo),
        _ => None,
    }
}

/// Read the R8 template name defensively: null-checked, bounded to 63 bytes +
/// NUL, UTF-8 validated. Returns `None` on null / no-NUL-in-bound / non-UTF8.
/// (The longest tracked name is `dance_judge_for_freeze` = 22 bytes, so 63 is
/// ample; an unterminated or oversized buffer can't match any known name.)
unsafe fn read_name<'a>(name: *const i8, buf: &'a mut [u8; 64]) -> Option<&'a str> {
    if name.is_null() {
        return None;
    }
    let mut n = 0usize;
    while n < 63 {
        let b = *name.add(n) as u8;
        if b == 0 {
            break;
        }
        buf[n] = b;
        n += 1;
    }
    // n == 63 with no NUL → over-long; it won't match a known name, but return
    // it anyway (classify will reject it). std::str validates UTF-8.
    std::str::from_utf8(&buf[..n]).ok()
}

// ── Registry operations (game thread only) ──────────────────────────

/// Remove any entry aliasing `wrapper` (slot-reuse eviction). Returns true if
/// one was removed.
unsafe fn evict(wrapper: *mut u8) -> bool {
    let reg = &mut *addr_of_mut!(REGISTRY);
    let mut removed = 0usize;
    for slot in reg.iter_mut() {
        if slot.wrapper == wrapper {
            *slot = EMPTY;
            removed += 1;
        }
    }
    if removed > 0 {
        REGISTRY_LEN.fetch_sub(removed, Ordering::Release);
    }
    removed > 0
}

/// Insert a freshly-created tracked clip. Assumes any stale entry for
/// `wrapper` was already evicted. Returns the slot index, or `None` on
/// overflow.
unsafe fn insert(wrapper: *mut u8, kind: ElementKind) -> Option<usize> {
    let layer_id = (wrapper.add(0x08) as *const u32).read_unaligned();
    let reg = &mut *addr_of_mut!(REGISTRY);
    for (i, slot) in reg.iter_mut().enumerate() {
        if slot.wrapper.is_null() {
            *slot = TrackedClip {
                wrapper,
                layer_id,
                kind,
                side: Side::Unbound,
                orig_x: 0,
                orig_y: 0,
                bound: false,
            };
            CAPTURE_COUNT[kind as usize].fetch_add(1, Ordering::Relaxed);
            REGISTRY_LEN.fetch_add(1, Ordering::Release);
            return Some(i);
        }
    }
    // No free slot: never overwrite a live entry — styling degrades to
    // partial for this song. Warn once.
    if !OVERFLOW_WARNED.swap(true, Ordering::Relaxed) {
        crate::log_warn!(
            "OverlayElementStyling: clip registry full ({} slots) — styling partial this song",
            REGISTRY_CAP
        );
    }
    None
}

/// Bind a tracked clip to `side` and apply the one-shots. Re-validates the
/// layer id against the wrapper (guards a slot recycled between Create and
/// SetPosition); evicts + skips on mismatch. `(x, y)` is the element's original
/// game-set position (the first SetPosition x/y; `(0,0)` on the Create-time
/// fallback). Binding is one-shot; the scale/position matrix is written by
/// [`place`], which is anchored on the judgement text so element SPACING scales
/// with size (see the anchor statics).
unsafe fn bind_and_apply(idx: usize, side: Side, x: i32, y: i32) {
    let side_i = match side {
        Side::P1 => 0usize,
        Side::P2 => 1usize,
        Side::Unbound => return,
    };

    let (kind, layer_id) = {
        let reg = &mut *addr_of_mut!(REGISTRY);
        let clip = &mut reg[idx];
        if clip.wrapper.is_null() || clip.bound {
            return;
        }
        // Layer-id revalidation: the wrapper's live layer id must still match
        // the one captured at Create (design §6). A mismatch means the pool
        // slot was recycled — evict and skip rather than styling an unrelated
        // clip.
        let live_layer = (clip.wrapper.add(0x08) as *const u32).read_unaligned();
        if live_layer == 0 || live_layer != clip.layer_id {
            *clip = EMPTY;
            REGISTRY_LEN.fetch_sub(1, Ordering::Release);
            return;
        }
        clip.side = side;
        clip.orig_x = x;
        clip.orig_y = y;
        clip.bound = true;
        (clip.kind, clip.layer_id)
    };

    // Styling application below is the MOD's behavior — gated on it being
    // enabled. Shared-capture-only mode (styling config-disabled) stops
    // here: the clip is tracked + side-bound for consumers like the
    // s_marvelous flash, with zero visual writes.
    if !super::is_enabled() {
        if matches!(kind, ElementKind::Judge) {
            ANCHOR_X[side_i].store(x, Ordering::Release);
            ANCHOR_Y[side_i].store(y, Ordering::Release);
            ANCHOR_KNOWN[side_i].store(true, Ordering::Release);
        }
        return;
    }

    // Opacity one-shot (once, design §4.5). Independent of the position matrix.
    //   - Combo: COMPOSE-ONLY. The game hides a <4 combo via SetColor(a=0); a
    //     one-shot here would un-hide a 0-combo counter. All combo alpha flows
    //     through the compose detour instead.
    //   - Judge / FreezeJudge / FastSlow: the game never colors these, so the
    //     one-shot is their sole opacity source.
    //   - Pacemaker: one-shot seeds pre-first-event opacity; the compose detour
    //     multiplies the game's later 1.0/0.5 writes.
    let op = super::opacity_pct(side_i as u8);
    if !matches!(kind, ElementKind::Combo) && op != 100 {
        let a = op as f32 / 100.0;
        bm2d_api::layer_set_color_raw(layer_id, 1.0, 1.0, 1.0, a);
    }

    // Scale + anchored placement. If this is the judgement text, it becomes the
    // cluster anchor — record it and (re)place every already-bound same-side
    // element so those bound before the judge get their gaps compressed too.
    if matches!(kind, ElementKind::Judge) {
        ANCHOR_X[side_i].store(x, Ordering::Release);
        ANCHOR_Y[side_i].store(y, Ordering::Release);
        ANCHOR_KNOWN[side_i].store(true, Ordering::Release);
        place_side(side_i);
    } else {
        place(idx);
    }

    let scale = super::scale_pct(side_i as u8);
    log_info!(
        "{MOD_ID}: bind kind={} pos=({},{}) side={} scale={} opacity={}",
        kind.name(),
        x,
        y,
        side_i,
        scale,
        op
    );
}

/// Write a bound clip's scale/position matrix: uniform scale `s` about the
/// element's own center, with the Y position zoomed about the judgement anchor
/// (`new_y = anchor_y + s*(orig_y - anchor_y)`) so element SPACING scales with
/// size. FreezeJudge is EXCLUDED from the anchor (its results are per-panel and
/// must stay at their original position — they still scale about their own
/// center). X is always left at the original (per-panel/lane layout preserved).
/// Skips at 100 % (stock — leave the game's own transform untouched). Safe to
/// call more than once (idempotent for a given scale/anchor). Must supply the
/// translation because `afp_layer_set_matrix` rewrites the whole 4×4 including
/// the translation row (`+0x130/0x134`); a later `afp_layer_set_position`
/// rewrites only that row, so the scale here survives game repositions.
unsafe fn place(idx: usize) {
    let reg = &*addr_of!(REGISTRY);
    let clip = reg[idx];
    if !clip.bound {
        return;
    }
    let side_i = match clip.side {
        Side::P1 => 0usize,
        Side::P2 => 1usize,
        Side::Unbound => return,
    };
    let scale = super::scale_pct(side_i as u8);
    if scale == 100 {
        return; // stock: leave the game's own transform in place
    }
    let s = scale as f32 / 100.0;
    let new_x = clip.orig_x as f32;
    // Freeze O.K./N.G. results are per-panel (anchored above each lane column),
    // so they must NOT be pulled toward the judgement anchor — they still scale
    // about their own center but keep their original position. Every other
    // scoped element gets the judge-anchored gap compression.
    let anchored = !matches!(clip.kind, ElementKind::FreezeJudge);
    let new_y = if anchored && ANCHOR_KNOWN[side_i].load(Ordering::Acquire) {
        let ay = ANCHOR_Y[side_i].load(Ordering::Acquire) as f32;
        ay + s * (clip.orig_y as f32 - ay)
    } else {
        clip.orig_y as f32
    };
    bm2d_api::layer_set_scale_translate_raw(clip.layer_id, s, s, new_x, new_y);
}

/// Re-place every bound clip on `side_i` (called when the judgement anchor is
/// established, so clips bound before the judge get their gaps compressed too).
/// Collects indices first to avoid aliasing the registry across `place`.
unsafe fn place_side(side_i: usize) {
    let want = if side_i == 0 { Side::P1 } else { Side::P2 };
    let mut idxs = [0usize; REGISTRY_CAP];
    let mut n = 0usize;
    {
        let reg = &*addr_of!(REGISTRY);
        for (i, slot) in reg.iter().enumerate() {
            if slot.bound && slot.side == want {
                idxs[n] = i;
                n += 1;
            }
        }
    }
    for &i in idxs.iter().take(n) {
        place(i);
    }
}

/// Find the registry index for `wrapper`, if tracked.
pub(crate) fn find(wrapper: *mut u8) -> Option<usize> {
    if wrapper.is_null() || REGISTRY_LEN.load(Ordering::Acquire) == 0 {
        return None;
    }
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        reg.iter().position(|s| s.wrapper == wrapper)
    }
}

/// For the color compose detours (Step 6): the bound side (0/1) of a tracked
/// wrapper, or `None` if untracked or still unbound. When unbound, the caller
/// forwards the color write unchanged (the only pre-bind write is combo's
/// create-time `a=0` hide, which is opacity-invariant — design §4.6).
pub(crate) fn tracked_bound_side(wrapper: *mut u8) -> Option<u8> {
    let idx = find(wrapper)?;
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        match reg[idx].side {
            Side::P1 => Some(0),
            Side::P2 => Some(1),
            Side::Unbound => None,
        }
    }
}

/// The side-bound `dance_judge` clip's pool wrapper, if captured this song.
/// GAME-THREAD-ONLY (lock-free registry). The layer id is revalidated
/// against the live wrapper so a recycled pool slot is never handed out.
/// Consumed cross-mod via `overlay_element_styling::judge_clip`.
pub(crate) fn judge_wrapper_for_side(side: u8) -> Option<*mut u8> {
    let want = match side {
        0 => Side::P1,
        1 => Side::P2,
        _ => return None,
    };
    unsafe {
        let reg = &*addr_of!(REGISTRY);
        for clip in reg.iter() {
            if !clip.wrapper.is_null()
                && clip.bound
                && clip.kind == ElementKind::Judge
                && clip.side == want
            {
                let live_layer = (clip.wrapper.add(0x08) as *const u32).read_unaligned();
                if live_layer != 0 && live_layer == clip.layer_id {
                    return Some(clip.wrapper);
                }
                return None;
            }
        }
    }
    None
}

/// Clear the whole registry (belt-and-braces alongside Create-time eviction).
/// `log_counts` emits the per-song capture summary before zeroing.
pub(crate) fn clear(log_counts: bool) {
    if log_counts {
        let counts: [usize; 5] = std::array::from_fn(|i| CAPTURE_COUNT[i].load(Ordering::Relaxed));
        if counts.iter().any(|&c| c > 0) {
            log_info!(
                "OverlayElementStyling: song captures — combo={} judge={} freeze={} fast_slow={} pacemaker={}",
                counts[0], counts[1], counts[2], counts[3], counts[4]
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
    // Reset the per-side judgement anchor so next song re-establishes it.
    for i in 0..2 {
        ANCHOR_KNOWN[i].store(false, Ordering::Release);
    }
}

// ── Create detour ───────────────────────────────────────────────────

/// `CMovieClip::Create(this, package*, name: *const c_char /*R8*/,
/// priority: i32, mode: i32)`. Modeled as `-> u64` so the original's RAX is
/// forwarded verbatim regardless of the true (unused) return type.
type CreateFn = unsafe extern "C" fn(*mut u8, *mut u8, *const i8, i32, i32) -> u64;

static mut CREATE_HOOK: Option<GenericDetour<CreateFn>> = None;

unsafe extern "C" fn create_hook(
    this: *mut u8,
    package: *mut u8,
    name: *const i8,
    priority: i32,
    mode: i32,
) -> u64 {
    // Call the original FIRST — the wrapper's layer id (wrapper+0x08) is only
    // populated after Create runs.
    let ret = match &*addr_of!(CREATE_HOOK) {
        Some(h) => h.call(this, package, name, priority, mode),
        None => return 0,
    };

    // Our capture logic never propagates a panic across the FFI boundary.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !tracking_enabled() || this.is_null() {
            return;
        }
        // Slot-reuse eviction: any Create over a tracked wrapper ptr (matching
        // name or not) drops the stale entry first.
        evict(this);
        let mut buf = [0u8; 64];
        if let Some(s) = read_name(name, &mut buf) {
            if let Some(kind) = classify(s) {
                if let Some(idx) = insert(this, kind) {
                    // Fallback (design §4.4): with no SetPosition detour, a
                    // single active side is unambiguous here at Create time.
                    // Versus can't be disambiguated without a position, so it
                    // renders stock. The element isn't positioned yet, so pass
                    // (0,0) — the game's subsequent afp_layer_set_position
                    // fills the translation (it rewrites only the translation
                    // dwords, preserving the scale we set here).
                    if !SETPOS_INSTALLED.load(Ordering::Acquire) {
                        let side = match read_presence() {
                            (true, false) => Some(Side::P1),
                            (false, true) => Some(Side::P2),
                            _ => None,
                        };
                        if let Some(side) = side {
                            bind_and_apply(idx, side, 0, 0);
                        }
                    }
                }
            }
        }
    }));

    ret
}

/// Install the Create detour (load-bearing). Returns false on failure; the
/// mod refuses to enable in that case.
pub(crate) fn install_create(addr: *const u8) -> bool {
    unsafe {
        // Idempotent: the shared-capture consumer and the styling mod's
        // enable may both request the install (whichever runs first wins).
        if (*addr_of!(CREATE_HOOK)).is_some() {
            return true;
        }
        let target: CreateFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(addr_of_mut!(CREATE_HOOK), target, create_hook) {
            Ok(()) => {
                log_info!("{MOD_ID}: CMovieClip::Create hook installed @ {:p}", addr);
                true
            }
            Err(e) => {
                crate::log_warn!("{MOD_ID}: Create hook install failed: {e}");
                false
            }
        }
    }
}

/// Tear down the Create detour and clear the registry. No-op while a
/// shared-capture consumer holds the registry (styling's enable rollback
/// must not break the sharing mod; the detours are passive when unused).
pub(crate) fn remove() {
    if SHARED_CAPTURE.load(Ordering::Acquire) {
        log_info!("{MOD_ID}: capture remove skipped — shared consumer active");
        return;
    }
    unsafe {
        if let Some(d) = (*addr_of_mut!(SETPOS_HOOK)).take() {
            let _ = d.disable();
        }
        if let Some(d) = (*addr_of_mut!(CREATE_HOOK)).take() {
            let _ = d.disable();
        }
    }
    SETPOS_INSTALLED.store(false, Ordering::Release);
    clear(false);
    OVERFLOW_WARNED.store(false, Ordering::Relaxed);
}

// ── SetPosition detour (side binding) ───────────────────────────────

/// Wrapper SetPosition (vtable +0x38): `fn(this, x: i32, y: i32)` (void).
type SetPositionFn = unsafe extern "C" fn(*mut u8, i32, i32);

static mut SETPOS_HOOK: Option<GenericDetour<SetPositionFn>> = None;

unsafe extern "C" fn set_position_hook(this: *mut u8, x: i32, y: i32) {
    // Call the original FIRST (position is the game's; we only observe it).
    if let Some(h) = &*addr_of!(SETPOS_HOOK) {
        h.call(this, x, y);
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !tracking_enabled() {
            return;
        }
        let idx = match find(this) {
            Some(i) => i,
            None => return,
        };
        let bound = {
            let reg = &*addr_of!(REGISTRY);
            reg[idx].bound
        };
        if bound {
            // Subsequent game reposition (e.g. FAST/SLOW per judgement, msg
            // 0x1035): the original call just overwrote the layer translation
            // with the raw position, so re-apply our anchored placement using
            // the new position to keep the gap scaling through the move.
            reposition(idx, x, y);
        } else if let Some(side) = determine_side(x) {
            // First position write: bind + apply.
            bind_and_apply(idx, side, x, y);
        }
    }));
}

/// Re-apply the anchored placement for an already-bound clip whose game
/// position just changed (updates the stored original to the new raw position,
/// then re-places). If the judgement text itself moves, the cluster anchor
/// moves with it and every same-side clip is re-placed.
unsafe fn reposition(idx: usize, x: i32, y: i32) {
    let kind = {
        let reg = &mut *addr_of_mut!(REGISTRY);
        let clip = &mut reg[idx];
        if !clip.bound {
            return;
        }
        clip.orig_x = x;
        clip.orig_y = y;
        clip.kind
    };
    let side_i = match {
        let reg = &*addr_of!(REGISTRY);
        reg[idx].side
    } {
        Side::P1 => 0usize,
        Side::P2 => 1usize,
        Side::Unbound => return,
    };
    if matches!(kind, ElementKind::Judge) {
        ANCHOR_X[side_i].store(x, Ordering::Release);
        ANCHOR_Y[side_i].store(y, Ordering::Release);
        ANCHOR_KNOWN[side_i].store(true, Ordering::Release);
        place_side(side_i);
    } else {
        place(idx);
    }
}

/// Install the SetPosition side-binding detour (NON-fatal — versus degrades to
/// stock, single/double still styled via the Create-time fallback). Returns
/// false if the address is missing or the install fails.
pub(crate) fn install_set_position(addr: *const u8) -> bool {
    unsafe {
        // Idempotent (see install_create).
        if (*addr_of!(SETPOS_HOOK)).is_some() {
            return true;
        }
        let target: SetPositionFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            addr_of_mut!(SETPOS_HOOK),
            target,
            set_position_hook,
        ) {
            Ok(()) => {
                SETPOS_INSTALLED.store(true, Ordering::Release);
                log_info!(
                    "{MOD_ID}: CMovieClip::SetPosition hook installed @ {:p}",
                    addr
                );
                true
            }
            Err(e) => {
                crate::log_warn!("{MOD_ID}: SetPosition hook install failed: {e}");
                false
            }
        }
    }
}
