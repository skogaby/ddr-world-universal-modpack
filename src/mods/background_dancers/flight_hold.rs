//! The flight stages' READY hold — the engine side (game thread, every
//! frame before the DancePlaySequence update: `lifecycle::drive_live` runs
//! from the input-manager frame callback). The decisions are pure
//! (`flight_hold_logic.rs`); this file reads / writes the two game fields
//! they act on:
//!
//! * the DPS READY? dwell timer (`DPS+0x130` on every supported build —
//!   the offset published by `derive_ddr_sel_intro` as
//!   `ddr_sel_dps_ready_timer_off`; no other known reader; the write is
//!   range-checked and only made while the live DPS is in step 5);
//! * the stage panel's visibility (the ShutterActor's stage-kind CMovieClip
//!   layer, `afp_layer_set_attribute(id, 1, 0 / 1)` — the panel's own state
//!   machine keeps running underneath; re-asserted every hidden frame).
//!
//! Fail-open: without the timer offset nothing is held (the take-off plays
//! over the song as before); without the shutter the take-off plays under
//! the panel (logged once).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::log_warn;
use crate::services::{bm2d_api, shutter, song_reset};

use super::flight_hold_logic::PanelOp;

/// `afp_layer_set_attribute` visibility bit.
const ATTR_VISIBLE: u32 = 0x1;

static TIMER_OFF: AtomicUsize = AtomicUsize::new(0);
static PANEL_WARNED: AtomicBool = AtomicBool::new(false);
/// The layer the last hide touched (0 = none): a show restores exactly it,
/// even when the shutter's active kind moved on meanwhile.
static HIDDEN_LAYER: AtomicU32 = AtomicU32::new(0);

/// Resolve the dwell timer offset (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.ddr_sel_dps_ready_timer_off() {
        Some(off) => {
            TIMER_OFF.store(off, Ordering::Release);
            true
        }
        None => {
            log_warn!(
                "BackgroundDancers: READY? dwell timer unresolved (ddr_sel_dps_ready_dwell) -- flight take-offs play over the song's start"
            );
            false
        }
    }
}

/// The hold can act at all.
pub fn available() -> bool {
    TIMER_OFF.load(Ordering::Acquire) != 0
}

fn timer_field() -> Option<*mut u8> {
    let off = TIMER_OFF.load(Ordering::Acquire);
    if off == 0 {
        return None;
    }
    let dps = song_reset::live_dps()?;
    let field = unsafe { dps.add(off) };
    memory::is_readable(field, 4).then_some(field)
}

/// The live DPS (identity for the per-DPS hold).
pub fn live_dps() -> usize {
    song_reset::live_dps().map_or(0, |p| p as usize)
}

/// The live DPS step (vtable-verified).
pub fn dps_step() -> Option<i32> {
    song_reset::dps_step()
}

/// The live DPS's dwell timer (seconds since the DPS was created).
pub fn read_timer() -> Option<f32> {
    let f = timer_field()?;
    let v = unsafe { memory::read_f32(f) };
    v.is_finite().then_some(v)
}

/// Write the dwell timer — only while the live DPS is in its READY? step.
pub fn write_timer(value: f32) -> bool {
    if song_reset::dps_step() != Some(super::flight_hold_logic::STEP_READY) {
        return false;
    }
    let Some(f) = timer_field() else {
        return false;
    };
    unsafe { memory::write_f32(f, value) };
    true
}

/// Whether the stage panel has settled — ShutterActor state 4 (covered:
/// its `in` / cut-in done) or no panel at all (idle, no actor, unreadable) —
/// and the state read (logging).
pub fn panel_settled() -> (bool, Option<i32>) {
    match shutter::snapshot() {
        Ok(Some(s)) => (s.state == 0 || s.state == 4, Some(s.state)),
        Ok(None) | Err(_) => (true, None),
    }
}

/// Hide / show the stage panel. Returns the layer touched (logging).
pub fn panel(op: PanelOp) -> Option<u32> {
    let visible = match op {
        PanelOp::Keep => return None,
        PanelOp::Hide => 0,
        PanelOp::Show => ATTR_VISIBLE,
    };
    if op == PanelOp::Show {
        let hidden = HIDDEN_LAYER.swap(0, Ordering::AcqRel);
        if hidden != 0 && bm2d_api::layer_id_is_valid(hidden) {
            let _ = bm2d_api::layer_set_attribute_raw(hidden, ATTR_VISIBLE, ATTR_VISIBLE);
        }
    }
    let layer = (|| {
        let snap = shutter::snapshot().ok()??;
        let kind = shutter::stage_kind()?;
        if snap.active_kind != kind {
            return None;
        }
        shutter::kind_clip(snap.actor, kind).map(|c| c.layer)
    })();
    match layer {
        Some(l) if bm2d_api::layer_set_attribute_raw(l, ATTR_VISIBLE, visible) => {
            if op == PanelOp::Hide {
                HIDDEN_LAYER.store(l, Ordering::Release);
            }
            Some(l)
        }
        _ => {
            if op == PanelOp::Hide && !PANEL_WARNED.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "BackgroundDancers: flight take-off -- the stage panel could not be hidden (no active stage-kind layer); the take-off plays under it"
                );
            }
            None
        }
    }
}
