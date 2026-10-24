//! Scroll Remap Service — the ONE owner of the `get_offset_y` detour.
//!
//! `get_offset_y(dBeatCount: i64, speed: i32 /*×100*/, boost: i32,
//! musicCount: i32) -> f32` is the renderer's pure scroll-Y function: the
//! pixel offset of a note from the receptor row given its beat delta
//! (`note.beat_count − renderer.beat_count`, 1024 ticks per beat), the
//! player's speed mod, the BOOST/BRAKE/WAVE mode and the music clock. It is
//! the only place a note's draw position is decided, and it is reached by
//! `CALL rel32` from exactly four sites (verified offline on every supported
//! gamemdx build, 20250805 → 20260915): twice in the note collector (head
//! and freeze tail), once in the tap+freeze pass (active-hold head
//! recompute) and once in the guideline draw; `note_types_expansion::
//! mine_render` calls it through the signature address, so it lands here
//! too. Judgement never reads it.
//!
//! This service reproduces StepMania's display-only `#SCROLLS` / `#SPEEDS`
//! gimmicks for injected songs (`core::sm::scroll`) by remapping the
//! function's input and scaling its output, so NORMAL/BOOST/BRAKE/WAVE and
//! every downstream transform (hallway perspective, playfield scale) keep
//! composing on the stock math:
//!
//! ```text
//! now      = snapshot(renderer+0xA8) / 1024            (beats)
//! note     = now + dBeatCount / 1024
//! d'       = (display(note) − display(now)) × 1024     (ticks, #SCROLLS)
//! y        = stock_get_offset_y(d', speed, boost, musicCount)
//! result   = y × speed_at(now, (musicCount − beat0_ms) / 1000)   (#SPEEDS)
//! ```
//!
//! `beat0_ms` is the committed SSQ's `tempo_data[0]` (the music count of beat
//! 0), which puts the renderer's clock into the speed table's elapsed-
//! seconds domain; StepMania interpolates speed ramps in time, even for
//! beat units. The current beat comes from a `render_notes_hook` PRE
//! subscriber (Early) that snapshots `renderer+0xA8` / `+0xAC` once per
//! side per frame — the detour has no renderer pointer. Both sides share
//! one song clock, so the guideline draw (outside `render_notes`) reading
//! the other side's snapshot is exact; its first frame after a song starts
//! can be one frame stale, which is invisible.
//!
//! ## Hot path
//!
//! The detour runs per note per frame. With no gimmick song active it is one
//! `AtomicPtr` load and a tail call of the trampoline. Active, it is pure
//! `f64` arithmetic over two small sorted tables (`partition_point`, no
//! allocation, no lock, no panic path — `core::sm::scroll` is written for
//! this). Tables are published behind an `AtomicPtr` to a boxed record; the
//! current and the previous record are both retained (`RETAINED`) so a
//! detour call that loaded the old pointer an instant before a publish
//! never dereferences freed memory. `clear` only nulls the pointer.
//!
//! ## Cull (plan Step 9 piece 5)
//!
//! The note collector's top cull is `get_offset_y(...) > bound` (the
//! `cull_window` slot, 720 stock) and it BREAKS its loop at the first note
//! past the bound. Because the collector calls this detour, it culls on the
//! *remapped, scaled* offset — exactly what will be drawn — so no extra
//! `cull_window` contribution is needed: a `#SCROLLS 0` region or a
//! `#SPEEDS` ramp towards 0 pulls far notes inside the window and the
//! collector collects them (StepMania draws them all too), and the bottom
//! cull sees the same remapped value. One accepted limitation: a *negative*
//! scroll factor makes offsets non-monotone in beat order, and the
//! collector's loop break then drops the notes after the first one past the
//! bound (StepMania walks a display-beat range instead). Stock and
//! non-gimmick songs are untouched (`ACTIVE` null ⇒ passthrough).
//!
//! Lifecycle: `init` (lib.rs, after `render_notes_hook`) installs the detour
//! only when the StepMania loader is armed and `stepmania_packs.
//! scroll_gimmicks` is on; `services::stepmania` publishes at gameplay load
//! (`taint::note_bank_request`, the full-bank serve) and clears on leaving
//! scene 28 or when a stock song's bank is requested. Fail-open throughout.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};

use retour::GenericDetour;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::core::sm::scroll::ScrollGimmicks;
use crate::services::render_notes_hook::{self, Priority};
use crate::{log_info, log_warn};

/// Must match `GetOffsetYFn` in `note_types_expansion::mine_render`.
type GetOffsetYFn =
    unsafe extern "C" fn(d_beat_count: i64, speed: i32, boost: i32, music_count: i32) -> f32;

/// ArrowRenderer field offsets (shared with `mine_render::actor`).
const RENDERER_BEAT_COUNT: usize = 0xA8;
const RENDERER_MUSIC_COUNT: usize = 0xAC;

/// What the hot path reads for the active song.
pub struct ActiveTables {
    gimmicks: Arc<ScrollGimmicks>,
    beat0_ms: i32,
    label: String,
}

// Written only by `init` (before INSTALLED is set), read by the detour.
static mut DETOUR: Option<GenericDetour<GetOffsetYFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The published tables; null = passthrough.
static ACTIVE: AtomicPtr<ActiveTables> = AtomicPtr::new(std::ptr::null_mut());
/// Current + previous published records, kept alive for the hot path.
static RETAINED: Mutex<Vec<Box<ActiveTables>>> = Mutex::new(Vec::new());

/// Per-frame renderer snapshot (`+0xA8` beat ticks, `+0xAC` music ms).
static SNAP_BEAT: AtomicI32 = AtomicI32::new(0);
static SNAP_MUSIC: AtomicI32 = AtomicI32::new(0);

/// `render_notes` PRE @ Early: stash the renderer's clock for the detour.
/// `renderer` is the live `ArrowRenderer this` (read without probing, like
/// `mine_render`); the offsets are the same `actor::*` constants.
fn snapshot_pre(renderer: *mut u8) {
    if renderer.is_null() {
        return;
    }
    unsafe {
        SNAP_BEAT.store(
            memory::read_i32(renderer.add(RENDERER_BEAT_COUNT)),
            Ordering::Relaxed,
        );
        SNAP_MUSIC.store(
            memory::read_i32(renderer.add(RENDERER_MUSIC_COUNT)),
            Ordering::Relaxed,
        );
    }
}

/// The detour. `extern "C"`: no panics, no allocation, no locks.
unsafe extern "C" fn get_offset_y_detour(
    d_beat_count: i64,
    speed: i32,
    boost: i32,
    music_count: i32,
) -> f32 {
    let Some(hook) = (*std::ptr::addr_of!(DETOUR)).as_ref() else {
        return 0.0;
    };
    let active = ACTIVE.load(Ordering::Acquire);
    if active.is_null() {
        return hook.call(d_beat_count, speed, boost, music_count);
    }
    let tables = &*active;
    let now_ticks = SNAP_BEAT.load(Ordering::Relaxed);
    // `music_count` IS the renderer's `+0xAC` for this call (the stock
    // callers pass it straight through), so it is the clock of record.
    let elapsed_s = f64::from(music_count.wrapping_sub(tables.beat0_ms)) / 1000.0;
    let (d_remapped, scale) = tables.gimmicks.remap(now_ticks, d_beat_count, elapsed_s);
    // Defence in depth against "advance until off-screen" loops in callers
    // we did not identify: never let the multiplier reach zero. 1/64 keeps a
    // speed-0 squeeze visually indistinguishable (a note 40 beats out sits
    // where a note 0.6 beats out normally would) while every such loop still
    // terminates within ~64x its stock iteration count.
    let scale = if scale.abs() < MIN_SPEED_SCALE {
        if scale.is_sign_negative() {
            -MIN_SPEED_SCALE
        } else {
            MIN_SPEED_SCALE
        }
    } else {
        scale
    };
    hook.call(d_remapped, speed, boost, music_count) * (scale as f32)
}

/// Smallest |speed multiplier| the detour applies (see the detour body).
const MIN_SPEED_SCALE: f64 = 1.0 / 64.0;

/// Route the guideline (beat-line) drawer's `CALL get_offset_y` around the
/// remap. That function loops "next beat line until its offset passes the
/// 720 px bound"; under a speed-0 segment or a negative scroll factor the
/// remapped offset never passes it and the game thread spins forever
/// (cabinet 2026-10-23: total freeze at Matt Silver's beat 160). The notes
/// are collected and drawn per note (bounded loops), so only this caller
/// needs the stock function. Each `E8 rel32` in the drawer that targets
/// `get_offset_y` is rewritten to a near 14-byte stub (`JMP [RIP+0]` +
/// absolute address) that lands on the detour's trampoline, i.e. the
/// original body. Checked patch; a miss leaves the remap installed but
/// logs that gimmick songs would be unsafe, and the caller then refuses to
/// publish tables.
unsafe fn bypass_guideline_caller(signatures: &SignatureStore, get_offset_y: *const u8) -> bool {
    let Some(draw) = signatures.get_address("guideline_draw") else {
        log_warn!("ScrollRemap: guideline_draw unresolved -- cannot bypass its get_offset_y call");
        return false;
    };
    let Some(hook) = (*std::ptr::addr_of!(DETOUR)).as_ref() else {
        return false;
    };
    let trampoline = hook.trampoline() as *const () as usize;
    // Same window the derivation classified the drawer with (its
    // `DRAW_WINDOW`): the CALL sits after the 720.0f cull load, past the
    // first 0x338 bytes on 20260915 (cabinet 2026-10-23).
    const DRAW_WINDOW: usize = 0x800;
    let span = {
        let draw_end = (draw as usize).saturating_add(DRAW_WINDOW);
        let module_end = crate::core::module_resolver::get_game_module()
            .map(|m| m.base as usize + m.size)
            .unwrap_or(usize::MAX);
        draw_end.min(module_end).saturating_sub(draw as usize)
    };
    let sites = crate::core::scanner::scan_xrefs_to(draw, span, get_offset_y);
    if sites.is_empty() {
        log_warn!(
            "ScrollRemap: no CALL get_offset_y found in guideline_draw (+0x{:X}, span 0x{:X})",
            draw as usize,
            span
        );
        return false;
    }
    let mut patched = 0usize;
    for site in sites {
        let stub = match memory::alloc_near_rel32(site, 16) {
            Ok(p) => p,
            Err(e) => {
                log_warn!("ScrollRemap: near stub allocation failed: {:?}", e);
                return false;
            }
        };
        // FF 25 00 00 00 00 = JMP QWORD PTR [RIP+0]; the 8-byte target follows.
        let mut stub_bytes = [0u8; 14];
        stub_bytes[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
        stub_bytes[6..].copy_from_slice(&(trampoline as u64).to_le_bytes());
        std::ptr::copy_nonoverlapping(stub_bytes.as_ptr(), stub, stub_bytes.len());
        let after = site as usize + 5;
        let Ok(new_rel) = crate::core::memory_patch::rel32_displacement(after, stub as usize)
        else {
            log_warn!("ScrollRemap: stub out of rel32 range (should not happen)");
            return false;
        };
        let expected = std::slice::from_raw_parts(site.add(1), 4).to_vec();
        if let Err(e) =
            memory::apply_checked_patch(site.add(1) as *mut u8, &expected, &new_rel.to_le_bytes())
        {
            log_warn!("ScrollRemap: guideline call-site patch failed: {:?}", e);
            return false;
        }
        patched += 1;
    }
    log_info!(
        "ScrollRemap: guideline_draw's {} CALL get_offset_y site(s) routed to the stock function (beat lines ignore gimmicks)",
        patched
    );
    true
}

/// Install the detour and the per-frame snapshot subscriber. Returns
/// `false` (service idle, `publish` a no-op) when `get_offset_y` or the
/// `render_notes` dispatcher is unavailable or the install fails.
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(addr) = signatures.get_address("get_offset_y") else {
        log_warn!(
            "ScrollRemap: get_offset_y signature not resolved -- scroll gimmicks unavailable"
        );
        return false;
    };
    if !render_notes_hook::is_available() {
        log_warn!(
            "ScrollRemap: render_notes dispatcher unavailable -- scroll gimmicks unavailable"
        );
        return false;
    }
    if render_notes_hook::register_pre(Priority::Early, snapshot_pre).is_none() {
        log_warn!(
            "ScrollRemap: could not subscribe to render_notes -- scroll gimmicks unavailable"
        );
        return false;
    }
    unsafe {
        let target: GetOffsetYFn = std::mem::transmute(addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            get_offset_y_detour,
        ) {
            Ok(()) => {
                if !bypass_guideline_caller(signatures, addr) {
                    // The remap itself is harmless while passthrough; what is
                    // unsafe is publishing tables with the beat-line loop
                    // still remapped. Leave the detour installed (passthrough)
                    // and refuse to activate.
                    log_warn!(
                        "ScrollRemap: guideline bypass unavailable -- scroll gimmicks DISABLED this boot (freeze hazard)"
                    );
                    return false;
                }
                INSTALLED.store(true, Ordering::Release);
                log_info!("ScrollRemap: installed get_offset_y detour @ {:p} (passthrough until a gimmick song loads)", addr);
                true
            }
            Err(e) => {
                log_warn!("ScrollRemap: failed to install get_offset_y detour: {}", e);
                false
            }
        }
    }
}

/// Whether the detour is live.
pub fn is_available() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Whether a song's tables are currently applied.
pub fn is_active() -> bool {
    !ACTIVE.load(Ordering::Acquire).is_null()
}

/// Publish `gimmicks` for the song about to play. Identity tables are
/// treated as `clear`; re-publishing the song that is already active (the
/// resolver reports one load several times) is a no-op. Any thread.
pub fn publish(label: &str, gimmicks: Arc<ScrollGimmicks>, beat0_ms: i32) {
    if !is_available() {
        return;
    }
    if gimmicks.is_identity() {
        clear();
        return;
    }
    let current = ACTIVE.load(Ordering::Acquire);
    if !current.is_null() {
        let cur = unsafe { &*current };
        if cur.label == label && cur.beat0_ms == beat0_ms && *cur.gimmicks == *gimmicks {
            return;
        }
    }
    let record = Box::new(ActiveTables {
        gimmicks,
        beat0_ms,
        label: label.to_string(),
    });
    let ptr = &*record as *const ActiveTables as *mut ActiveTables;
    {
        let mut retained = RETAINED.lock().unwrap_or_else(|p| p.into_inner());
        retained.push(record);
        // Keep the newest two: the one being published and the one a detour
        // call in flight may still be reading.
        while retained.len() > 2 {
            retained.remove(0);
        }
    }
    ACTIVE.store(ptr, Ordering::Release);
    let (scroll_n, speed_n, scroll_id, speed_id) = {
        let g = unsafe { &(*ptr).gimmicks };
        (
            g.scroll.len(),
            g.speed.len(),
            g.scroll.is_identity(),
            g.speed.is_identity(),
        )
    };
    log_info!(
        "ScrollRemap: active for '{}' -- #SCROLLS {} segment(s){}, #SPEEDS {} segment(s){}, beat0 {} ms",
        label,
        scroll_n,
        if scroll_id { " (identity)" } else { "" },
        speed_n,
        if speed_id { " (identity)" } else { "" },
        beat0_ms
    );
}

/// Back to passthrough. Idempotent; logs only when something was active.
pub fn clear() {
    let prev = ACTIVE.swap(std::ptr::null_mut(), Ordering::AcqRel);
    if prev.is_null() {
        return;
    }
    // `prev` stays alive in RETAINED until two later publishes.
    let label = unsafe { (*prev).label.clone() };
    log_info!("ScrollRemap: cleared ('{}') -- passthrough", label);
}
