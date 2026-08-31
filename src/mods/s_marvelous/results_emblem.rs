//! Full-combo emblems (design §4.9, plan Step 9): an all-S-Marvelous MFC
//! shows a violet S-MFC emblem on BOTH results surfaces.
//!
//! Per-stage results: the emblem is the `fc_usr` clip
//! (`player_%dp_info_usr/fc_usr` — the self-contained fc timeline inside
//! the `result_root` template, sprite 243, shared by both panes), driven
//! ONCE at scene build by the results builder (`result_window_build`) via
//! `afp_mc_op(mc, 0xF09, "loop_" + suffix)` from the clear-kind table
//! ([10] = "mfc"). The AP2 patch clones `loop_mfc` (frames 417–549) as
//! `loop_smfc` with the word art re-pointed to the violet
//! `scre_fc_smarvelous` region, the word object's per-frame HSL-rotation
//! updates DROPPED (the rainbow flow would hue-cycle the violet art) and
//! the segment's `gotoAndPlay("loop_mfc")` loop DoAction RETARGETED at the
//! new label ([`assets::EMBLEM_CLONE_OPTS`]). Post-original detour: when a
//! side's record is S-MFC, re-drive `0xF09 loop_smfc` — with the
//! `mc_frame_by_label` (0x1012) pre-check as the resolve observable
//! (deploy-#6 lesson: 0xF09 swallows label misses).
//!
//! Total results: the populate (`total_result_populate`) loads the
//! clear-kind badge bitmap `scre_total_player_%s` ([10] = "fc_mfc") into
//! the `fullcombo_usr` leaves under `total_p%d_top_usr` per stage pane.
//! Post-original detour: for each S-MFC (side, stage), re-load the leaves
//! with the injected `scre_total_player_fc_smfc` (name-only texturelist
//! binding — no geo, the combo-digit precedent). Pane↔stage mapping
//! replicates the populate's own rule: one pane per stage whose PRIMARY
//! side's record is non-virgin, in stage order.
//!
//! S-MFC condition, computed from the record (never live counters):
//! `clear_kind(+0x54) == 10 && smarv_count == marv_count && marv > 0`,
//! with the side's last-armed window (0 ⇒ mod wasn't armed ⇒ stock).
//!
//! Fail-open everywhere: unresolved signatures ⇒ no detour (stock
//! emblems); unstaged assets / unpatched template ⇒ re-drives decline;
//! label missing at re-drive time ⇒ latched WARN + stock emblem.
//!
//! PANIC SAFETY: both detour callbacks wrap all post-original work in
//! `catch_unwind`; no unwrap/indexing on the hook path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

use retour::GenericDetour;

use crate::core::ap2::Ap2Doc;
use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::{afp_patcher, bm2d_api, stage_records};
use crate::{log_info, log_warn};

use super::assets::{self, StagedEmblemPatch};
use super::{records, results_score, state};

// ── Actor field offsets (display-side RE §4 + FUN_1800b8aa0 /
// FUN_1800cb090 decompiles, 20260721) ────────────────────────────────

/// Results-window actor: the scene's ONE "result_root" layer.
const RW_LAYER: usize = 0x108;
/// Results-window actor: current stage index (i32).
const RW_STAGE: usize = 0xEC;
/// Total-results actor: primary side (i32) — indexes the player work the
/// populate's pane-existence walk reads.
const TR_PRIMARY_SIDE: usize = 0x9C;
/// Total-results actor: per-pane "total_result" layer pointers (5 slots).
const TR_PANE_LAYERS: usize = 0x1B0;
const TR_PANE_MAX: usize = 5;
/// AGCS layer struct: the BM2D layer id lives at +8 (FUN_1800b84c0:
/// `afp_layer_play(*(layer + 8))`).
const LAYER_ID: usize = 0x8;
/// Stage record: clear kind (7=FC 8=GFC 9=PFC 10=MFC).
const REC_CLEAR_KIND: usize = 0x54;
const CLEAR_KIND_MFC: i32 = 10;

const LABEL_SMFC: &std::ffi::CStr = c"loop_smfc";

/// Both fns are `fn(this)`; the u64 return mirrors RAX through untouched.
type BuildFn = unsafe extern "C" fn(*mut u8) -> u64;

static BUILD_DETOUR: once_cell::sync::OnceCell<GenericDetour<BuildFn>> =
    once_cell::sync::OnceCell::new();
static TOTAL_DETOUR: once_cell::sync::OnceCell<GenericDetour<BuildFn>> =
    once_cell::sync::OnceCell::new();

/// Assets staged + patch registered + mod enabled (deactivate clears).
static ASSETS_READY: AtomicBool = AtomicBool::new(false);
/// The patch fn produced a patched result_root this session (latched — a
/// template already loaded stays patched across disable).
static PATCH_APPLIED: AtomicBool = AtomicBool::new(false);

static STAGED: Mutex<Option<StagedEmblemPatch>> = Mutex::new(None);
static REGISTER_ONCE: Once = Once::new();

static WARN_LABEL: AtomicBool = AtomicBool::new(false);
static WARN_VARIANT: AtomicBool = AtomicBool::new(false);
static WARN_TRANSFORM: AtomicBool = AtomicBool::new(false);
static FIRST_STAGE_LOGGED: AtomicBool = AtomicBool::new(false);
static FIRST_TOTAL_LOGGED: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

// ── Install (mod init) ───────────────────────────────────────────────

/// Install the two emblem detours, each best-effort (per-surface
/// fail-open). Returns true when at least one installed — the caller
/// gates asset staging on it.
pub fn install(signatures: &SignatureStore) -> bool {
    let mut any = false;
    match signatures.get_address("result_window_build") {
        Some(target) => {
            let target: BuildFn = unsafe { std::mem::transmute(target) };
            match unsafe { GenericDetour::new(target, window_build_hook) } {
                Ok(d) if unsafe { d.enable() }.is_ok() => {
                    let _ = BUILD_DETOUR.set(d);
                    any = true;
                }
                Ok(_) | Err(_) => {
                    log_warn!("SMarvelous: emblem build detour failed -- stage emblem stays stock")
                }
            }
        }
        None => log_warn!("SMarvelous: result_window_build unresolved -- stage emblem stays stock"),
    }
    match signatures.get_address("total_result_populate") {
        Some(target) => {
            let target: BuildFn = unsafe { std::mem::transmute(target) };
            match unsafe { GenericDetour::new(target, total_populate_hook) } {
                Ok(d) if unsafe { d.enable() }.is_ok() => {
                    let _ = TOTAL_DETOUR.set(d);
                    any = true;
                }
                Ok(_) | Err(_) => {
                    log_warn!("SMarvelous: emblem total detour failed -- total emblem stays stock")
                }
            }
        }
        None => {
            log_warn!("SMarvelous: total_result_populate unresolved -- total emblem stays stock")
        }
    }
    if any {
        log_info!("SMarvelous: FC emblem detour(s) installed");
    }
    any
}

// ── Activate / deactivate (mod enable/disable) ───────────────────────

/// Stage the emblem assets + register the result_root patch. Called from
/// enable (only when [`install`] succeeded).
pub fn activate() {
    let mut staged = match STAGED.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if staged.is_none() {
        *staged = assets::stage_emblems();
        if staged.is_none() {
            return; // WARN already emitted (fail-open)
        }
        REGISTER_ONCE.call_once(|| {
            afp_patcher::register_patch(assets::EMBLEM_TEMPLATE, Box::new(patch_result_root));
        });
    }
    ASSETS_READY.store(true, Ordering::Release);
}

/// Disable: the patch fn goes inert (future template loads stream stock)
/// and both re-drives decline. Texture/template state already mounted
/// this session stays (consistent: patched template ⇔ injected textures —
/// both are session-resident, neither is a stock-name replacement).
pub fn deactivate() {
    ASSETS_READY.store(false, Ordering::Release);
}

// ── The result_root template patch ───────────────────────────────────

fn patch_result_root(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !ASSETS_READY.load(Ordering::Acquire) {
        return None; // disabled/unstaged — stock, no warn (normal state)
    }
    let guard = STAGED.lock().ok()?;
    let staged = guard.as_ref()?;

    if afp != staged.stock_bytes.as_slice() {
        warn_once(
            &WARN_VARIANT,
            "SMarvelous: result_root variant differs from the staged template -- streaming stock",
        );
        return None;
    }

    let run = || -> Option<Vec<u8>> {
        let mut doc = Ap2Doc::parse(afp)?;
        let ids = doc.clone_segment_with_new_shapes_ex(
            assets::EMBLEM_SRC_LABEL,
            assets::EMBLEM_NEW_LABEL,
            &staged.shape_ids,
            assets::EMBLEM_CLONE_OPTS,
        )?;
        if ids != staged.expected {
            warn_once(
                &WARN_TRANSFORM,
                "SMarvelous: result_root allocated ids diverged from staging -- streaming stock",
            );
            return None;
        }
        doc.serialize()
    };
    match run() {
        Some(out) => {
            PATCH_APPLIED.store(true, Ordering::Release);
            log_info!(
                "SMarvelous: result_root patched ({} -> {} bytes, {})",
                afp.len(),
                out.len(),
                assets::EMBLEM_NEW_LABEL
            );
            Some((out, vec![0u8; 2]))
        }
        None => {
            warn_once(
                &WARN_TRANSFORM,
                "SMarvelous: result_root transform failed at stream time -- streaming stock",
            );
            None
        }
    }
}

// ── The S-MFC predicate ──────────────────────────────────────────────

/// From the record, never live counters: MFC clear kind + every marvelous
/// inside the side's armed window + at least one. Virgin records (mcode
/// -1, the marshal's skip key) and unreadable streams fail closed.
unsafe fn is_smfc(record: *const u8, window: i32) -> bool {
    if record.is_null() || window <= 0 {
        return false;
    }
    if memory::read_i32(record as *mut u8) == -1 {
        return false;
    }
    if memory::read_i32((record as *mut u8).add(REC_CLEAR_KIND)) != CLEAR_KIND_MFC {
        return false;
    }
    let Some(smarv) = records::smarv_count_from_record(record, window) else {
        return false;
    };
    let Some(marv) = records::marv_count_from_record(record) else {
        return false;
    };
    marv > 0 && smarv == marv
}

/// Resolve the record the per-stage results screen displays for `side`
/// (the builder's own course-gate branch, shared with results_score).
unsafe fn stage_screen_record(side: usize, stage: i32) -> Option<*mut u8> {
    if results_score::course_active() {
        stage_records::course_record(side)
    } else if stage >= 0 {
        stage_records::stage_record(side, stage as usize)
    } else {
        None
    }
}

// ── Per-stage results detour ─────────────────────────────────────────

unsafe extern "C" fn window_build_hook(actor: *mut u8) -> u64 {
    let ret = match BUILD_DETOUR.get() {
        Some(d) => d.call(actor),
        None => 0,
    };
    if !actor.is_null()
        && ASSETS_READY.load(Ordering::Acquire)
        && PATCH_APPLIED.load(Ordering::Acquire)
    {
        if let Err(e) = std::panic::catch_unwind(|| redrive_stage_emblems(actor)) {
            let _ = e;
        }
    }
    ret
}

fn redrive_stage_emblems(actor: *mut u8) {
    unsafe {
        let layer = memory::read_ptr(actor.add(RW_LAYER));
        if layer.is_null() {
            return;
        }
        let layer_id = memory::read_u32(layer.add(LAYER_ID));
        if layer_id == 0 {
            return;
        }
        let stage = memory::read_i32(actor.add(RW_STAGE));
        for side in 0..2usize {
            let window = state::last_armed_window(side);
            if window <= 0 {
                continue;
            }
            let Some(record) = stage_screen_record(side, stage) else {
                continue;
            };
            if !is_smfc(record, window) {
                continue;
            }
            let path = if side == 0 {
                "player_1p_info_usr/fc_usr"
            } else {
                "player_2p_info_usr/fc_usr"
            };
            let Some(mc) = bm2d_api::layer_find_child(layer_id, path) else {
                continue; // pane hidden/absent — stock behavior
            };
            // Resolve observable (0xF09's internal miss is SWALLOWED —
            // deploy-#6 lesson): confirm the label exists on this clip.
            let Some(frame) = bm2d_api::mc_frame_by_label(mc, LABEL_SMFC) else {
                warn_once(
                    &WARN_LABEL,
                    "SMarvelous: loop_smfc missing on fc_usr -- stage emblem stays stock",
                );
                continue;
            };
            if bm2d_api::mc_op_str(mc, 0xF09, LABEL_SMFC)
                && !FIRST_STAGE_LOGGED.swap(true, Ordering::Relaxed)
            {
                log_info!(
                    "SMarvelous: S-MFC stage emblem re-drive (side {}, mc {:08X}, label frame {})",
                    side,
                    mc,
                    frame
                );
            }
        }
    }
}

// ── Total results detour ─────────────────────────────────────────────

unsafe extern "C" fn total_populate_hook(actor: *mut u8) -> u64 {
    let ret = match TOTAL_DETOUR.get() {
        Some(d) => d.call(actor),
        None => 0,
    };
    if !actor.is_null() && ASSETS_READY.load(Ordering::Acquire) {
        if let Err(e) = std::panic::catch_unwind(|| redrive_total_emblems(actor)) {
            let _ = e;
        }
    }
    ret
}

fn redrive_total_emblems(actor: *mut u8) {
    unsafe {
        let primary = memory::read_i32(actor.add(TR_PRIMARY_SIDE));
        if !(0..=1).contains(&primary) {
            return;
        }
        // Pane ↔ stage mapping (the populate's own rule): one pane per
        // stage whose PRIMARY-side record is non-virgin, in stage order.
        let mut pane = 0usize;
        for stage in 0..TR_PANE_MAX {
            let Some(primary_rec) = stage_records::stage_record(primary as usize, stage) else {
                continue;
            };
            if memory::read_i32(primary_rec) == -1 {
                continue; // no pane created for this stage
            }
            if pane >= TR_PANE_MAX {
                break;
            }
            let pane_layer = memory::read_ptr(actor.add(TR_PANE_LAYERS + pane * 8));
            pane += 1;
            if pane_layer.is_null() {
                continue;
            }
            let pane_layer_id = memory::read_u32(pane_layer.add(LAYER_ID));
            if pane_layer_id == 0 {
                continue;
            }
            for side in 0..2usize {
                let window = state::last_armed_window(side);
                if window <= 0 {
                    continue;
                }
                let Some(record) = stage_records::stage_record(side, stage) else {
                    continue;
                };
                if !is_smfc(record, window) {
                    continue;
                }
                let path = if side == 0 {
                    "total_p1_top_usr/fullcombo_usr"
                } else {
                    "total_p2_top_usr/fullcombo_usr"
                };
                let Some(mc) = bm2d_api::layer_find_child(pane_layer_id, path) else {
                    continue;
                };
                // Re-load every leaf clip with the violet badge (the stock
                // loader's own walk shape: traversal depth 6).
                let mut id = Some(mc);
                let mut loaded = 0u32;
                while let Some(cur) = id {
                    if bm2d_api::mc_load_bitmap(cur, assets::EMBLEM_TOTAL_TEXTURE) {
                        loaded += 1;
                    }
                    id = bm2d_api::mc_traversal(cur, 6);
                }
                if loaded > 0 && !FIRST_TOTAL_LOGGED.swap(true, Ordering::Relaxed) {
                    log_info!(
                        "SMarvelous: S-MFC total badge re-drive (side {}, stage {}, {} leaf(s))",
                        side,
                        stage,
                        loaded
                    );
                }
            }
        }
    }
}
