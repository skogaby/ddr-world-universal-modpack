//! Premium Free Mod — Freezes the per-stage counter (unlimited stages).
//!
//! Gated by the overlay mod menu (controls whether the mod is active at all).
//! When active, registers a bool-toggle in the custom options framework so
//! players can enable/disable it from the in-game options screen. The stage
//! counter is cabinet-global, so the APPLIED freeze is resolved from the
//! ENTERED side(s)' values (P1 governs when both are in) and re-resolved on
//! every option change and scene change; in versus the row is mirrored via
//! `services/versus_mirror` (P1 seeds at song select, last writer wins) so
//! both players see one value. A profile load on one side never overwrites
//! the other side's preference (the pre-2026-09-01 unconditional cross-side
//! `set_value` did, so the freeze followed whichever 2P profile loaded LAST).
//!
//! Binary mechanism: the per-frame stage counter increment is a 3-byte
//! `INC dword [RCX+0xc]` instruction at the `premium_free_stage_inc` anchor
//! + 3. Replacing those 3 bytes with NOPs leaves the counter at its current
//! value forever.
//!
//! ## Stale-record fix (frozen stage index)
//!
//! Freezing the counter makes every play reuse the SAME per-stage play record
//! (`PlayerWork + rec_base + stage*rec_stride`). The game's song-select commit
//! initializes that record — `(mcode, difficulty, style)` plus a full score
//! wipe — only under an `if (new_mcode != rec->mcode)` guard. In vanilla the
//! guard always passes (a new stage's record is virgin, mcode == -1), but with
//! a frozen index, re-picking the SAME song at a DIFFERENT difficulty skips
//! the re-init: the record keeps the previous play's difficulty, and the
//! per-stage network save then submits the new score under the old difficulty
//! (cabinet-confirmed 2026-07-10 via CE write-watch on the record).
//!
//! Fix: on each transition into SONG_SELECT while the freeze is active, write
//! `mcode = -1` into the frozen-stage record of both players. That restores
//! the vanilla invariant ("the current stage's record is virgin during song
//! selection"), so the game's own commit path re-initializes the record with
//! the fresh difficulty. Timing is safe: the previous play's save payload is
//! marshaled from the record during the results screen, well before the
//! song-select transition. All layout constants come from the shared
//! `stage_records` service, which decodes them from the matched
//! `stage_record_accessor` signature bytes — nothing hardcoded. If that decode
//! failed, this mod fails closed (the freeze can never activate).
//!
//! ## Same-credit ghost cache (`ghost_cache`)
//!
//! The frozen slot breaks the game's local-slot pacemaker ghost (the score
//! DB points a same-credit PB at `record[stage]`, which the fix above
//! virginises and the game re-prepares — the ghost copy yields an empty
//! vector). `ghost_cache` snapshots each committed grade stream keyed by
//! chart and re-injects it when the game resolves nothing. World port of
//! the DDR A3 `pfree_ghost` hook; see the module docs.
//!
mod diag;
mod ghost_cache;

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::services::scene_manager;
use crate::services::stage_records;
use crate::services::versus_mirror;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

const INC_OFFSET: usize = 3;
const INC_SIZE: usize = 3;

/// Sentinel for "scene callback not registered".
const NO_CALLBACK: usize = usize::MAX;

static PATCH_ADDR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_BYTES: [std::sync::atomic::AtomicU8; INC_SIZE] = [
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
    std::sync::atomic::AtomicU8::new(0),
];
static PATCH_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Offset of the stage counter inside GameWork (disp8 of the INC we patch —
/// this mod owns that patch site, so the decode stays local; the rest of the
/// record layout comes from the shared `stage_records` service).
static STAGE_COUNTER_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// Scene-manager callback id (NO_CALLBACK when unregistered).
static SCENE_CB_ID: AtomicUsize = AtomicUsize::new(NO_CALLBACK);

/// Whether the stage-counter freeze is currently applied (the option is ON).
pub(super) fn freeze_active() -> bool {
    PATCH_ACTIVE.load(Ordering::Acquire)
}

fn apply_nop_patch() {
    let addr = PATCH_ADDR.load(Ordering::Acquire);
    if addr.is_null() || PATCH_ACTIVE.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        let old = memory::make_writable(addr as *const u8, INC_SIZE);
        for i in 0..INC_SIZE {
            memory::write_u8(addr.add(i), 0x90);
        }
        memory::restore_protection(addr as *const u8, INC_SIZE, old);
    }
    log_info!("PremiumFree: stage counter frozen");
}

fn restore_patch() {
    let addr = PATCH_ADDR.load(Ordering::Acquire);
    if addr.is_null() || !PATCH_ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }
    unsafe {
        let old = memory::make_writable(addr as *const u8, INC_SIZE);
        for i in 0..INC_SIZE {
            memory::write_u8(addr.add(i), ORIGINAL_BYTES[i].load(Ordering::Acquire));
        }
        memory::restore_protection(addr as *const u8, INC_SIZE, old);
    }
    log_info!("PremiumFree: stage counter restored");
}

/// Reset the frozen-stage play record of both players to "virgin"
/// (`mcode = -1`) so the next song-select commit re-initializes it with the
/// freshly chosen (mcode, difficulty) instead of being skipped by the game's
/// `new_mcode != rec->mcode` guard. Runs on the render thread (scene hook),
/// panic-isolated by scene_manager.
fn virginize_frozen_stage_records() {
    let game_work = match stage_records::game_work() {
        Some(p) => p,
        None => return,
    };

    unsafe {
        // Course mode plays through a separate record whose init is
        // unconditional — leave it alone.
        if memory::read_u64(game_work.add(stage_records::course_field_offset())) != 0 {
            return;
        }
        let stage = memory::read_i32(game_work.add(STAGE_COUNTER_OFFSET.load(Ordering::Acquire)));
        if !(0..stage_records::MAX_STAGE_RECORDS as i32).contains(&stage) {
            return;
        }
        for side in 0..2usize {
            let rec = match stage_records::stage_record(side, stage as usize) {
                Some(r) => r,
                None => continue,
            };
            let mcode = memory::read_i32(rec);
            if mcode != -1 {
                memory::write_i32(rec, -1);
                log_info!(
                    "PremiumFree: reset frozen stage-{} record for P{} (was mcode {}) so the next pick commits a fresh difficulty",
                    stage + 1,
                    side + 1,
                    mcode
                );
            }
        }
    }
}

/// Option id (shared with the versus mirror registration).
const OPT_ID: &str = "premium_free";
const MIRRORED_OPTIONS: [&str; 1] = [OPT_ID];

/// Per-side desired state (the option row's value as last delivered by an
/// edit or a profile load). The FREEZE is cabinet-global, so the applied
/// state is resolved from these by [`refresh_freeze`] — never written by a
/// side that isn't in the session.
static DESIRED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Cabinet-wide effective freeze. Entered side governs; P1 wins when both
/// are in (versus rows are mirrored anyway, so they agree). With entered
/// state unknown fall back to `p1 || p2` (a stray profile load can never
/// silently DROP a freeze another player relies on).
fn effective_freeze() -> bool {
    let p1 = DESIRED[0].load(Ordering::Acquire);
    let p2 = DESIRED[1].load(Ordering::Acquire);
    match (
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    ) {
        (Some(true), _) => p1,
        (Some(false), Some(true)) => p2,
        (Some(false), Some(false)) => false,
        _ => p1 || p2,
    }
}

/// Re-resolve and apply the freeze. Called from every option change AND from
/// every scene change (a side entering/leaving changes who governs, without
/// any option edit).
fn refresh_freeze() {
    if effective_freeze() {
        apply_nop_patch();
    } else {
        restore_patch();
    }
}

fn premium_free_on_change(player_side: u8, new_value: i32) {
    if player_side >= 2 {
        return;
    }
    DESIRED[player_side as usize].store(new_value != 0, Ordering::Release);
    log_info!(
        "PremiumFree: side={} desired {}",
        player_side,
        if new_value != 0 { "ON" } else { "OFF" }
    );
    refresh_freeze();
    // Versus: one cabinet-wide freeze ⇒ the row is mirrored across both
    // entered sides through the shared service (P1 seeds at song select,
    // last writer wins). Solo: the other side's cached preference is left
    // alone — a profile load must never overwrite another player's setting
    // (the former unconditional cross-side `set_value` did exactly that,
    // making the freeze follow whichever 2P profile loaded LAST).
    versus_mirror::mirror_edit(OPT_ID, player_side, new_value);
}

pub struct PremiumFreeMod;

impl PremiumFreeMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for PremiumFreeMod {
    fn id(&self) -> &str {
        "premium-free"
    }
    fn name(&self) -> &str {
        "Premium Free"
    }
    fn description(&self) -> &str {
        "Freeze the stage counter (unlimited stages)"
    }
    fn required_signatures(&self) -> &[&str] {
        &[
            "premium_free_stage_inc",
            "stage_record_accessor",
            "player_work_table",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let match_addr = ctx.signatures.require_address("premium_free_stage_inc");
        let addr = unsafe { match_addr.add(INC_OFFSET) as *mut u8 };

        unsafe {
            if *addr != 0xFF || *addr.add(1) != 0x41 || *addr.add(2) != 0x0C {
                log_warn!("PremiumFree: expected INC [RCX+0xc] (FF 41 0C) at patch address");
                return false;
            }
            for i in 0..INC_SIZE {
                ORIGINAL_BYTES[i].store(*addr.add(i), Ordering::Release);
            }
            // The INC's disp8 IS the stage-counter offset inside GameWork.
            STAGE_COUNTER_OFFSET.store(*addr.add(2) as usize, Ordering::Release);
        }

        // ── Stale-record-fix layout (shared `stage_records` decode) ─────
        // The service decodes and validates the whole record layout from the
        // accessor bytes at boot (out-of-range constants, out-of-module
        // globals, and a player_work_table mismatch all fail it closed). The
        // fix cannot arm without it — fail the whole mod closed rather than
        // risk poisoning score submissions.
        if !stage_records::is_available() {
            log_warn!("PremiumFree: stage_records layout unavailable -- disabling");
            return false;
        }
        log_info!(
            "PremiumFree: stale-record fix armed (records at work+0x{:X}, stride 0x{:X}, course field +0x{:X})",
            stage_records::record_base(),
            stage_records::record_stride(),
            stage_records::course_field_offset()
        );

        PATCH_ADDR.store(addr, Ordering::Release);

        // Same-credit ghost cache — fail-open sub-feature (missing anchors
        // only cost the ghost repair; the freeze + stale-record fix stand).
        ghost_cache::install(ctx.signatures);
        true
    }

    fn enable(&mut self) {
        // The stale-record fix is load-bearing for save integrity: without
        // the scene hook, a frozen stage index submits scores under the
        // previous play's difficulty. Fail closed (no option row -> the
        // freeze can never activate) if scenes are unavailable.
        if !scene_manager::is_available() {
            log_warn!(
                "PremiumFree: scene_manager unavailable -- stale-record fix cannot arm, refusing to enable"
            );
            return;
        }
        if SCENE_CB_ID.load(Ordering::Acquire) == NO_CALLBACK {
            let id = scene_manager::on_scene_change(Box::new(|prev, next| {
                // Who governs the freeze can change without an option edit
                // (a side entering or leaving) — re-resolve on every scene.
                refresh_freeze();
                if next == scene::SONG_SELECT && PATCH_ACTIVE.load(Ordering::Acquire) {
                    virginize_frozen_stage_records();
                }
                // Session end: drop the same-credit ghost cache.
                if next == scene::EAM_EXIT || next == scene::ATTRACT_DEMO {
                    ghost_cache::clear();
                }
                if PATCH_ACTIVE.load(Ordering::Acquire) {
                    diag::on_scene_enter(prev, next);
                }
            }));
            SCENE_CB_ID.store(id, Ordering::Release);
        }
        ghost_cache::set_enabled(true);

        if custom_options::is_available() {
            let spec = RegisterSpec::bool_toggle(OPT_ID)
                .display_name("Premium Free")
                .description("Runs every session with PREMIUM PLAY features at the standard price")
                .default_value(0)
                .on_change(premium_free_on_change);
            match custom_options::register_option(spec) {
                Ok(_handle) => {
                    log_info!("PremiumFree: registered custom option on Mods tab");
                    // One cabinet-wide freeze ⇒ mirror the row in versus.
                    versus_mirror::register(&MIRRORED_OPTIONS);
                }
                Err(e) => {
                    log_warn!("PremiumFree: custom option registration failed: {e}");
                }
            }
        } else {
            log_warn!(
                "PremiumFree: custom_options service unavailable -- option row will not render"
            );
        }
        log_info!("PremiumFree: enabled (toggled via in-game options menu)");
    }

    fn disable(&mut self) {
        versus_mirror::unregister(&MIRRORED_OPTIONS);
        DESIRED[0].store(false, Ordering::Release);
        DESIRED[1].store(false, Ordering::Release);
        restore_patch();
        ghost_cache::set_enabled(false);
        let cb = SCENE_CB_ID.swap(NO_CALLBACK, Ordering::AcqRel);
        if cb != NO_CALLBACK {
            scene_manager::remove_callback(cb);
        }
        log_info!("PremiumFree: disabled");
    }
}
