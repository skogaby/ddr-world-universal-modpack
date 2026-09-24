//! The DDR SELECTION option row: one per-player `custom_options` SCALAR row
//! (0..=6 = OFF / AUTO / 1st-5th / MAX-EXTREME / SuperNOVA / X / 2013-A)
//! rendered through [`ScalarFormat::Dynamic`] (text values — no per-value
//! chip textures), persisted LOCALLY (`PersistMode::Local` — the JSON cache,
//! never the wire), shown in both the in-game options menu and the overlay's
//! PLAYER SETTINGS tab, and mirrored across players in versus
//! (`versus_mirror`: one skin per cabinet). Effective at the next song — the
//! arm edge reads [`row_value`].
//!
//! Fail-open: without the scalar-row machinery the row is absent (one WARN)
//! and every song stays stock unless the developer knob is set; a
//! `Duplicate` registration (mod re-enabled this boot) re-arms the row. The
//! row's label texture `seop_item_ddr_selection` ships with the
//! custom-options texture sets (`scripts/option_strings.py`) — a DLL-only
//! deploy shows a blank label.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::services::custom_options::{
    self, PersistMode, RegisterError, RegisterSpec, ScalarFormat,
};
use crate::services::versus_mirror;
use crate::{log_info, log_warn};

use super::trigger;

pub const OPT_ID: &str = "ddr_selection";

static VALUE: [AtomicI32; 2] = [
    AtomicI32::new(trigger::ROW_OFF),
    AtomicI32::new(trigger::ROW_OFF),
];
static ROW_LIVE: AtomicBool = AtomicBool::new(false);
static REGISTERED: AtomicBool = AtomicBool::new(false);

fn label(_option_id: &str, value: i32) -> Option<String> {
    trigger::row_label(value).map(str::to_string)
}

fn on_change(side: u8, value: i32) {
    if side >= 2 {
        return;
    }
    let value = trigger::clamp_row(value);
    let old = VALUE[side as usize].swap(value, Ordering::AcqRel);
    if old != value {
        log_info!(
            "DDR SELECTION: P{} row = {} (next song)",
            side + 1,
            trigger::row_label(value).unwrap_or("?")
        );
    }
    versus_mirror::mirror_edit(OPT_ID, side, value);
}

fn identity_transform(_id: &str, value: i32) -> i32 {
    value
}

fn clamp_load(_id: &str, value: i32) -> i32 {
    trigger::clamp_row(value)
}

/// Register (or re-show) the row. Returns whether it is live.
pub fn register() -> bool {
    if REGISTERED.load(Ordering::Acquire) {
        set_available(true);
        return true;
    }
    if !custom_options::row_injection_available() {
        log_warn!(
            "DDR SELECTION: scalar row machinery unavailable -- the DDR SELECTION option row is absent"
        );
        return false;
    }
    let spec = RegisterSpec::scalar(
        OPT_ID,
        trigger::ROW_OFF,
        trigger::ROW_MAX,
        1,
        ScalarFormat::Dynamic(label),
    )
    .default_value(trigger::ROW_OFF)
    .persist_mode(PersistMode::Local)
    .persist_transform(identity_transform, clamp_load)
    .display_name("DDR SELECTION")
    .description(
        "Play with the gameplay screen of a classic DDR era; AUTO picks each song's own era",
    )
    .on_change(on_change);
    let ok = match custom_options::register_option(spec) {
        Ok(_handle) => true,
        Err(RegisterError::Duplicate { .. }) => {
            for side in 0..2u8 {
                on_change(
                    side,
                    custom_options::get_value(side, OPT_ID).unwrap_or(trigger::ROW_OFF),
                );
            }
            custom_options::set_option_available(OPT_ID, true);
            true
        }
        Err(e) => {
            log_warn!("DDR SELECTION: option row registration failed: {e}");
            false
        }
    };
    if !ok {
        return false;
    }
    REGISTERED.store(true, Ordering::Release);
    versus_mirror::register(&[OPT_ID]);
    ROW_LIVE.store(true, Ordering::Release);
    log_info!(
        "DDR SELECTION: option row live (OFF / AUTO / 5 eras; per player, mirrored in versus; local persistence; next song)"
    );
    true
}

/// Mod disable / re-enable: hide or show the row (values stay).
pub fn set_available(available: bool) {
    if !REGISTERED.load(Ordering::Acquire) {
        return;
    }
    if available {
        versus_mirror::register(&[OPT_ID]);
    } else {
        versus_mirror::unregister(&[OPT_ID]);
    }
    custom_options::set_option_available(OPT_ID, available);
    ROW_LIVE.store(available, Ordering::Release);
}

/// `side`'s row value (OFF while the row is not live).
pub fn row_value(side: usize) -> i32 {
    if side >= 2 || !ROW_LIVE.load(Ordering::Acquire) {
        return trigger::ROW_OFF;
    }
    VALUE[side].load(Ordering::Acquire)
}
