//! Lane styling — sizing of the lane elements that do NOT flow through the
//! shared quad fill (`render_sprite_final`):
//!
//!   - **Lane filter** (the translucent darkening band, FILTER option):
//!     created via the pool-slot Create wrapper with the name
//!     `dance_filter_%s` (cabinet-diagnosed). Layer id at slot +0x08.
//!   - **Lane cover** (the SUDDEN/HIDDEN panel): same wrapper, names
//!     `hidden_cover_%s` / `sudden_cover_%s`.
//!   - **Danger flash** (red low-life lane overlay): same wrapper,
//!     `danger_single` / `danger_double`.
//!   - **Receptor hit flash** (`dance_effect`): created by NoteResultActor
//!     via `afp_layer_create_with_property` (bypasses the wrapper) and
//!     captured from the actor's clip vector by the `note_result_setup`
//!     detour. SHARED with `player_perspective` (consumer-refcounted like
//!     `guideline_hook` — either mod may be config-disabled): the flash is
//!     an AFP clip the perspective VS never touches, so tracking the mapped
//!     receptors (scale AND position) is done here, composing the published
//!     `PerspConstants` map after the playfield-styling step.
//!
//! The visible per-lane background art needs NO handling here: it scales
//! correctly through the fill hook + filter band alone (cabinet-confirmed).
//! The `1p_lane_usr`/`2p_lane_usr` find-child clips once captured for it
//! are the HUD LAYOUT CONTAINERS (children = the `judge_usr`/`arrow_usr`…
//! position markers), not lane art — that path (and its find-child detour)
//! was removed after every apply attempt failed harmlessly on transient ids.
//!
//! ## Mechanism (per the maintainer's spec: bands horizontal-only+scale-only)
//!
//! Capture points queue matched clips; the transform is **DEFERRED to the
//! first `render_sprite_final` call of the song** (via
//! [`has_pending`]/[`apply_pending`] from the fill hook) — by then the HUD
//! build (which reads marker positions) is complete and all clip positions
//! are final.
//!
//! Apply (all read-modify-write, position-preserving):
//!   - Filter/cover/danger (layer id): read the 2×3 matrix `{a,b,c,d,tx,ty}`
//!     (`afp_layer_get_matrix`) and write back `{s·a, b, s·c, d, tx, ty}` —
//!     horizontal-only about the layer origin, preserving the game's own
//!     scale (the filter may be a width-scaled unit quad) and translation.
//!   - Receptor flash: translation-only matrix RMW — first the playfield
//!     step toward the fill's fixed points (`tx' = cx + s·(tx−cx)`,
//!     `ty' = posY + s·(ty−posY)`), then the side's perspective map
//!     (`PerspConstants::map_point`, identical to the lane-pass VS) — plus
//!     a uniform component scale on the clip's root MC (see [`apply_one`]).
//!
//! No opacity is composed (scale-only). Side attribution: filter/cover from
//! the presence read (single/doubles) or the layer matrix `tx < 640` split
//! (versus); flash from mode/presence.
//!
//! ## Degradation
//!
//! Best-effort and NON-load-bearing: missing signatures / API misses log a
//! warning and skip lane styling; the core playfield styling is unaffected.
//! Both detours are inert outside the gameplay window.

use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicBool, Ordering};

use retour::GenericDetour;

use crate::services::bm2d_api;
use crate::{log_info, log_warn};

use super::MOD_ID;

/// Pool-create slot: the created clip's AFP **layer** id.
const OFF_LAYER_ID: usize = 0x08;

/// `afp_mc_set_param`/`afp_mc_get_param` table index of the scale pair.
const MC_PARAM_SCALE: i32 = 0x1003;

/// Versus playfield midline (screen units, 1280-wide space).
const X_SPLIT: f32 = 640.0;

#[derive(Clone, Copy, PartialEq)]
enum LaneKind {
    Filter,
    Cover,
    /// The red low-life flash overlay drawn over the lane (`danger_%s`).
    Danger,
    /// Per-panel receptor hit flash (`dance_effect`). Unlike the others this
    /// must also REPOSITION (converge toward the lane center) to track the
    /// scaled receptors — not just shrink in place.
    ReceptorFlash,
}

impl LaneKind {
    fn name(self) -> &'static str {
        match self {
            LaneKind::Filter => "filter",
            LaneKind::Cover => "cover",
            LaneKind::Danger => "danger",
            LaneKind::ReceptorFlash => "receptor_flash",
        }
    }
}

/// Classify a clip name → (kind, side-from-name). Filter/cover names carry
/// the play MODE, not the side: `*_double` = doubles → side 0 (A7: doubles
/// uses P1's values); `*_single` is ambiguous (both versus sides create it)
/// → resolved at apply time via presence / position.
///
/// NOT matched: `1p_lane_usr`/`2p_lane_usr`/`double_lane_usr` — see the
/// module doc (HUD layout containers, not lane art).
fn classify(name: &str) -> Option<(LaneKind, Option<u8>)> {
    let mode_hint = if name.ends_with("_double") {
        Some(0)
    } else {
        None
    };
    match name {
        _ if name.starts_with("dance_filter") => Some((LaneKind::Filter, mode_hint)),
        _ if name.starts_with("hidden_cover") || name.starts_with("sudden_cover") => {
            Some((LaneKind::Cover, mode_hint))
        }
        // The red low-life lane flash (pool-create `danger_single` /
        // `danger_double` — cabinet-diagnosed). Matched EXACTLY: the HUD
        // builder also find-childs `danger_gauge_%dp_usr` (a gauge readout,
        // not the lane overlay), which must NOT match.
        "danger_single" => Some((LaneKind::Danger, None)),
        "danger_double" => Some((LaneKind::Danger, Some(0))),
        _ => None,
    }
}

/// Read a bounded, NUL-terminated, UTF-8 C string (≤63 bytes). `None` on
/// null / non-UTF8.
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
    std::str::from_utf8(&buf[..n]).ok()
}

// ── Pending queue (game thread only) ────────────────────────────────
// Captures queue here; the fill hook drains it on the next quad (i.e. the
// first rendered frame after the capture, when the HUD build is complete
// and all positions are final).

#[derive(Clone, Copy)]
struct PendingClip {
    kind: LaneKind,
    /// AFP layer id (all captures come from the pool-create slot / actor
    /// clip vector) → `afp_layer_get_matrix`/`set_matrix` rewrite.
    id: u32,
    side_hint: Option<u8>,
    /// Screen-space lane-center X for [`LaneKind::ReceptorFlash`] reposition
    /// (`tx' = anchor_x + s·(tx − anchor_x)`). `None` for the shrink-in-place
    /// lane bands (filter/cover/danger), which keep their `tx`.
    anchor_x: Option<f32>,
}

const PENDING_CAP: usize = 24;
static mut PENDING: [Option<PendingClip>; PENDING_CAP] = [None; PENDING_CAP];
/// Cross-path publication flag (capture → fill hook). The queue itself is
/// game-thread-only.
static LANE_PENDING: AtomicBool = AtomicBool::new(false);

/// Applied-id dedupe (captures can fire repeatedly for the same clip).
/// Reset per song.
const APPLIED_CAP: usize = 32;
static mut APPLIED: [u32; APPLIED_CAP] = [0; APPLIED_CAP];
static mut APPLIED_LEN: usize = 0;

static PANIC_WARNED: AtomicBool = AtomicBool::new(false);

/// Clear per-song state (called from the mod's scene callback at GAMEPLAY
/// enter/exit).
pub(crate) fn reset() {
    unsafe {
        let pending = &mut *addr_of_mut!(PENDING);
        for slot in pending.iter_mut() {
            *slot = None;
        }
        APPLIED_LEN = 0;
    }
    LANE_PENDING.store(false, Ordering::Release);
}

/// True if a captured clip is waiting for its deferred apply (read by the
/// fill hook each quad — one atomic load).
pub(crate) fn has_pending() -> bool {
    LANE_PENDING.load(Ordering::Acquire)
}

unsafe fn already_applied(id: u32) -> bool {
    let applied = &*addr_of!(APPLIED);
    applied[..APPLIED_LEN].contains(&id)
}

unsafe fn mark_applied(id: u32) {
    if APPLIED_LEN < APPLIED_CAP {
        let applied = &mut *addr_of_mut!(APPLIED);
        applied[APPLIED_LEN] = id;
        APPLIED_LEN += 1;
    }
}

/// Queue a captured clip for the deferred apply (game thread). Dedupes
/// against both the pending queue and the already-applied set.
unsafe fn queue_clip(kind: LaneKind, id: u32, side_hint: Option<u8>, anchor_x: Option<f32>) {
    if id == 0 || already_applied(id) {
        return;
    }
    let pending = &mut *addr_of_mut!(PENDING);
    if pending.iter().flatten().any(|p| p.id == id) {
        return;
    }
    match pending.iter_mut().find(|s| s.is_none()) {
        Some(slot) => {
            *slot = Some(PendingClip {
                kind,
                id,
                side_hint,
                anchor_x,
            });
            LANE_PENDING.store(true, Ordering::Release);
        }
        None => {
            if !PANIC_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!("{MOD_ID}: lane pending queue full — clip 0x{id:08X} not styled");
            }
        }
    }
}

/// Resolve a queued clip's side: name hint → presence (single/doubles) →
/// layer-matrix `tx < 640` split (versus). `None` = undeterminable (skip).
unsafe fn resolve_side(p: &PendingClip) -> Option<u8> {
    if let Some(s) = p.side_hint {
        return Some(s);
    }
    match super::fill_hook::read_presence() {
        (true, false) => Some(0),
        (false, true) => Some(1),
        (true, true) => {
            bm2d_api::layer_get_matrix_raw(p.id).map(|m| if m[4] < X_SPLIT { 0 } else { 1 })
        }
        (false, false) => None,
    }
}

/// Outcome of one apply attempt.
enum ApplyOutcome {
    /// Handled (successfully or terminally failed) — do not revisit.
    Done,
    /// Prerequisite not ready (e.g. renderer not bound yet) — re-queue and
    /// retry on a later fill call this song.
    Retry,
}

/// Apply the styling to one queued clip. Read-modify-write so the game's
/// own scale/translation components are preserved.
unsafe fn apply_one(p: &PendingClip) -> ApplyOutcome {
    let side = match resolve_side(p) {
        Some(s) => s,
        None => return ApplyOutcome::Done,
    };
    // Playfield-styling scale (identity when that mod is config-disabled —
    // the guideline capture's pattern).
    let (s, _op) = if super::is_enabled() {
        super::latched(side as usize)
    } else {
        (1.0, 1.0)
    };
    // The side's perspective map (receptor flash only): latched at song
    // start, but the RESOLVED constants publish on the side's first lane
    // pass — retry until they have.
    let persp = if p.kind == LaneKind::ReceptorFlash
        && crate::mods::player_perspective::latched_params(side).is_some()
    {
        match crate::mods::player_perspective::published_constants(side) {
            Some(c) => Some(c),
            None => return ApplyOutcome::Retry,
        }
    } else {
        None
    };
    if s == 1.0 && persp.is_none() {
        mark_applied(p.id); // identity: nothing to do, don't revisit
        return ApplyOutcome::Done;
    }

    // The playfield-scale flash reposition needs the side's receptor-row
    // anchor from the fill registry; that renderer binds on its first quad
    // — which may be later in the same frame than our first drain. Retry
    // until bound. (Not needed when s == 1: the perspective step carries
    // its own anchor in the published constants.)
    let anchor_y = if p.kind == LaneKind::ReceptorFlash && s != 1.0 {
        match super::fill_hook::side_anchor_y(side) {
            Some(y) => y,
            None => return ApplyOutcome::Retry,
        }
    } else {
        0.0
    };

    let ok = match bm2d_api::layer_get_matrix_raw(p.id) {
        Some(m) => match (p.kind, p.anchor_x) {
            // Receptor flash: track the styled/mapped receptors — position
            // AND size. Composition order matches the lane quads: the fill's
            // playfield scale first (converge toward the lane center X and
            // the receptor row Y — the fill's fixed points,
            // cabinet-verified), then the side's perspective map applied to
            // the resulting point (the identical transform the perspective
            // VS applies to the receptor quads). The shrink itself goes
            // through the clip's ROOT MC component scale (param 0x1003 —
            // scales about the registration point, position untouched;
            // scaling the layer matrix a/d displaced the art by its
            // internal offset, cabinet-diagnosed).
            (LaneKind::ReceptorFlash, Some(cx)) => {
                let mut tx = m[4];
                let mut ty = m[5];
                let mut comp = s;
                if s != 1.0 {
                    tx = cx + s * (tx - cx);
                    ty = anchor_y + s * (ty - anchor_y);
                }
                if let Some(pc) = persp {
                    let (px, py, sp) = pc.map_point(tx, ty);
                    tx = px;
                    ty = py;
                    comp *= sp;
                }
                let new_m: [f32; 6] = [m[0], m[1], m[2], m[3], tx, ty];
                let moved = bm2d_api::layer_set_matrix_raw(p.id, &new_m);
                let scaled = match bm2d_api::layer_find_child(p.id, "/") {
                    Some(root_mc) => match bm2d_api::mc_get_vec2(root_mc, MC_PARAM_SCALE) {
                        Some((sx, sy)) => {
                            let ok = bm2d_api::mc_set_scale(root_mc, sx * comp, sy * comp);
                            log_info!(
                                        "{MOD_ID}: lane {} layer=0x{:08X} root_mc=0x{:08X} side={} s={:.2} persp={} comp={:.2} tx={:.1}→{:.1} ty={:.1}→{:.1} scale=({:.2},{:.2})→({:.2},{:.2}) ok={}",
                                        p.kind.name(),
                                        p.id,
                                        root_mc,
                                        side,
                                        s,
                                        persp.is_some(),
                                        comp,
                                        m[4],
                                        new_m[4],
                                        m[5],
                                        new_m[5],
                                        sx,
                                        sy,
                                        sx * comp,
                                        sy * comp,
                                        ok
                                    );
                            ok
                        }
                        None => {
                            log_warn!(
                                "{MOD_ID}: lane {} root_mc=0x{root_mc:08X} get-scale failed",
                                p.kind.name()
                            );
                            false
                        }
                    },
                    None => {
                        log_warn!(
                            "{MOD_ID}: lane {} layer=0x{:08X} root MC (\"/\") not found",
                            p.kind.name(),
                            p.id
                        );
                        false
                    }
                };
                moved && scaled
            }
            // Lane bands (filter/cover/danger): shrink content in place —
            // `{s·a, b, s·c, d, tx, ty}` (horizontal-only, position preserved).
            _ => {
                let new_m: [f32; 6] = [s * m[0], m[1], s * m[2], m[3], m[4], m[5]];
                let ok = bm2d_api::layer_set_matrix_raw(p.id, &new_m);
                log_info!(
                    "{MOD_ID}: lane {} layer=0x{:08X} side={} s={:.2} m={:?}→{:?} ok={}",
                    p.kind.name(),
                    p.id,
                    side,
                    s,
                    m,
                    new_m,
                    ok
                );
                ok
            }
        },
        None => false,
    };

    if !ok {
        log_warn!(
            "{MOD_ID}: lane {} apply failed (id=0x{:08X}) — left stock",
            p.kind.name(),
            p.id
        );
    }
    mark_applied(p.id);
    ApplyOutcome::Done
}

/// Drain the pending queue (called from the fill hook on the game thread —
/// by then the HUD build is complete and all clip positions are final).
/// Clips whose prerequisites aren't ready ([`ApplyOutcome::Retry`]) stay
/// queued for the next fill call.
pub(crate) fn apply_pending() {
    if !LANE_PENDING.swap(false, Ordering::AcqRel) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let pending = &mut *addr_of_mut!(PENDING);
        let mut any_retry = false;
        for slot in pending.iter_mut() {
            if let Some(p) = slot.take() {
                match apply_one(&p) {
                    ApplyOutcome::Done => {}
                    ApplyOutcome::Retry => {
                        *slot = Some(p);
                        any_retry = true;
                    }
                }
            }
        }
        if any_retry {
            LANE_PENDING.store(true, Ordering::Release);
        }
    }));
}

// ── pool-create detour (lane filter + covers + danger) ──────────────

type PoolCreateFn = unsafe extern "C" fn(
    pool: *mut u8,
    package: *mut u8,
    name: *const i8,
    priority: i32,
    mode: i32,
) -> *mut u8;

static mut POOL_CREATE_HOOK: Option<GenericDetour<PoolCreateFn>> = None;

unsafe extern "C" fn pool_create_cb(
    pool: *mut u8,
    package: *mut u8,
    name: *const i8,
    priority: i32,
    mode: i32,
) -> *mut u8 {
    let hook = match &*addr_of!(POOL_CREATE_HOOK) {
        Some(h) => h,
        None => return std::ptr::null_mut(),
    };
    let ret = hook.call(pool, package, name, priority, mode);

    if super::is_enabled() && super::fill_hook::in_gameplay() && !ret.is_null() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut buf = [0u8; 64];
            if let Some(s) = read_name(name, &mut buf) {
                let layer_id = (ret.add(OFF_LAYER_ID) as *const u32).read_unaligned();
                if layer_id != 0 {
                    if let Some((kind, side_hint)) = classify(s) {
                        queue_clip(kind, layer_id, side_hint, None);
                    }
                }
            }
        }));
    }
    ret
}

// ── NoteResultActor setup detour (receptor hit flashes) ─────────────

/// NoteResultActor field offsets (from the setup decompile).
const OFF_NRA_MODE: usize = 0x90; // 0 = single (4 panels), else double (8)
const OFF_NRA_FLASH_BEGIN: usize = 0xE8; // vector<CMovieClip*> begin
const OFF_NRA_FLASH_END: usize = 0xF0; // vector end

type NoteResultSetupFn = unsafe extern "C" fn(this: *mut u8);

static mut NOTE_RESULT_HOOK: Option<GenericDetour<NoteResultSetupFn>> = None;

unsafe extern "C" fn note_result_setup_cb(this: *mut u8) {
    let hook = match &*addr_of!(NOTE_RESULT_HOOK) {
        Some(h) => h,
        None => return,
    };
    // Run the original FIRST — it creates + positions the flash clips.
    hook.call(this);

    // Capture when EITHER consumer needs the flashes this song: playfield
    // styling enabled, or a side latched a perspective preset (the latch
    // precedes actor setup — both happen after the GAMEPLAY scene-entry
    // callbacks).
    let wanted = super::is_enabled() || crate::mods::player_perspective::any_side_latched();
    if !wanted || !super::fill_hook::in_gameplay() || this.is_null() {
        return;
    }

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // The per-panel flash clips live in a vector<CMovieClip*> at
        // actor+0xE8..+0xF0; each element's AFP layer id is at clip+0x08.
        //
        // NOTE: `begin` must be typed as a pointer-to-pointer so `add(i)`
        // strides 8 bytes per element. (The first cabinet build walked it as
        // `*const u8` — 1-byte stride — producing garbage "clip" pointers
        // that passed the null check and faulted on the +0x08 read.)
        let begin = (this.add(OFF_NRA_FLASH_BEGIN) as *const *const *const u8).read_unaligned();
        let end = (this.add(OFF_NRA_FLASH_END) as *const *const *const u8).read_unaligned();
        if begin.is_null() || end.is_null() || end <= begin {
            return;
        }
        // Sanity: a real vector's bounds are 8-aligned and 8-divisible.
        let (b, e) = (begin as usize, end as usize);
        if b % 8 != 0 || e % 8 != 0 {
            return;
        }
        let count = (e - b) / 8;
        if count == 0 || count > 16 {
            return;
        }

        // Collect (layer_id, tx). Positions are final (SetPosition ran in the
        // original). The lane-center anchor is their centroid — the flash set
        // spans the lane symmetrically about it.
        let mut ids = [0u32; 16];
        let mut n = 0usize;
        let mut min_tx = f32::MAX;
        let mut max_tx = f32::MIN;
        for i in 0..count {
            let clip = begin.add(i).read();
            if clip.is_null() || (clip as usize) % 8 != 0 {
                continue;
            }
            let layer_id = (clip.add(OFF_LAYER_ID) as *const u32).read_unaligned();
            if layer_id == 0 {
                continue;
            }
            if let Some(m) = bm2d_api::layer_get_matrix_raw(layer_id) {
                min_tx = min_tx.min(m[4]);
                max_tx = max_tx.max(m[4]);
            } else {
                continue;
            }
            ids[n] = layer_id;
            n += 1;
        }
        if n == 0 || min_tx > max_tx {
            return;
        }
        let cx = (min_tx + max_tx) * 0.5;

        // Side: doubles (mode != 0) → 0 (A7). Single → defer to
        // `resolve_side` (presence read; per-clip tx split only in true
        // versus). NOTE: do NOT use the centroid split here — with
        // center-arrows-1P the single lane is centered EXACTLY at 640.0,
        // and `cx < 640` misclassified it as side 1 (cabinet-diagnosed).
        let mode = (this.add(OFF_NRA_MODE) as *const i32).read_unaligned();
        let side_hint = if mode != 0 { Some(0) } else { None };

        for &id in ids.iter().take(n) {
            queue_clip(LaneKind::ReceptorFlash, id, side_hint, Some(cx));
        }
    }));
}

// ── Install / remove ────────────────────────────────────────────────

/// Consumers of the shared note-result (receptor hit flash) detour —
/// mirrors `guideline_hook`'s refcounted install (either mod may be
/// config-disabled independently).
#[derive(Clone, Copy)]
#[repr(usize)]
pub(crate) enum FlashConsumer {
    PlayfieldStyling = 0,
    PlayerPerspective = 1,
}

static NR_WANTED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static NR_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Declare interest in the receptor-flash capture, installing the
/// note-result detour on first use. Best-effort for both consumers.
pub(crate) fn flash_acquire(consumer: FlashConsumer, note_result_setup: Option<*const u8>) -> bool {
    if !bm2d_api::afp_layers_available() {
        log_warn!("{MOD_ID}: AFP-layer API unavailable — receptor-flash styling skipped");
        return false;
    }
    NR_WANTED[consumer as usize].store(true, Ordering::Release);
    if NR_INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let addr = match note_result_setup {
        Some(a) => a,
        None => {
            log_warn!("{MOD_ID}: note_result_setup unresolved — receptor hit flash not styled");
            return false;
        }
    };
    unsafe {
        let target: NoteResultSetupFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            addr_of_mut!(NOTE_RESULT_HOOK),
            target,
            note_result_setup_cb,
        ) {
            Ok(()) => {
                log_info!("{MOD_ID}: receptor-flash hook installed @ {addr:p}");
                NR_INSTALLED.store(true, Ordering::Release);
                true
            }
            Err(e) => {
                log_warn!("{MOD_ID}: note-result hook install failed: {e}");
                false
            }
        }
    }
}

/// Drop a consumer's interest; tears the detour down only when no consumer
/// remains.
pub(crate) fn flash_release(consumer: FlashConsumer) {
    NR_WANTED[consumer as usize].store(false, Ordering::Release);
    if NR_WANTED.iter().any(|w| w.load(Ordering::Acquire)) {
        return;
    }
    if !NR_INSTALLED.swap(false, Ordering::AcqRel) {
        return;
    }
    unsafe {
        if let Some(d) = (*addr_of_mut!(NOTE_RESULT_HOOK)).take() {
            let _ = d.disable();
        }
    }
}

/// Install the lane-capture detours (playfield_styling's enable path).
/// Best-effort (NON-load-bearing): a missing signature or unavailable AFP
/// API logs a warning and skips lane styling without affecting the core
/// playfield styling. Returns true if at least one detour installed.
pub(crate) fn install(t: &super::ResolvedTargets) -> bool {
    if !bm2d_api::afp_layers_available() {
        log_warn!("{MOD_ID}: AFP-layer API unavailable — lane styling skipped");
        return false;
    }

    let mut any = false;

    match t.pool_create {
        Some(addr) => unsafe {
            let target: PoolCreateFn = std::mem::transmute(addr);
            match crate::core::hooks::install_enabled(
                addr_of_mut!(POOL_CREATE_HOOK),
                target,
                pool_create_cb,
            ) {
                Ok(()) => {
                    log_info!("{MOD_ID}: lane-filter/cover hook installed @ {:p}", addr);
                    any = true;
                }
                Err(e) => log_warn!("{MOD_ID}: pool-create hook install failed: {e}"),
            }
        },
        None => {
            log_warn!("{MOD_ID}: cmovieclip_pool_create unresolved — lane filter/cover not scaled")
        }
    }

    any |= flash_acquire(FlashConsumer::PlayfieldStyling, t.note_result_setup);

    any
}

/// Tear down playfield_styling's lane detours (the note-result detour
/// survives while player_perspective still holds it).
pub(crate) fn remove() {
    unsafe {
        if let Some(d) = (*addr_of_mut!(POOL_CREATE_HOOK)).take() {
            let _ = d.disable();
        }
    }
    flash_release(FlashConsumer::PlayfieldStyling);
    reset();
    PANIC_WARNED.store(false, Ordering::Relaxed);
}
