//! Per-Song Judgement Offsets — per-side, per-song overrides of the stock
//! JUDGEMENT OFFSET (`ddr::player::Option+0x24`), keyed by the song
//! highlighted on the song wheel.
//!
//! Planning: `.agents/planning/2026-08-17-per-song-judgement-offsets/`
//! (design `design/detailed-design.md`). Built up over plan Steps 1–6; this
//! module gate grows as steps land:
//!
//! - `csv` — pure CSV layer for `judgement_offsets.csv` (Step 1).
//! - `store` — per-side baseline/session state, merge semantics, wire codec
//!   (Step 1).
//! - `musicdb_scan` — pure `<basename>` scanner for the boot crawl (Step 3).
//! - `bootstrap` — boot crawl thread + CSV writer + baseline load (Step 3).
//! - `ui` — option rows, wheel-selection poll, edit capture (Step 4).
//! - `override_hook` — gameplay override write/restore lifecycle (Step 5).
//! - `persistence` — the `mod_judge_offsets` network wire (Step 6).
//!
//! Fully-inert rule (design requirement 9): everything is gated on
//! `custom_options::row_injection_available()` at `enable()` — without the
//! option rows the mod does nothing at all.

pub mod bootstrap;
pub mod csv;
pub mod musicdb_scan;
pub mod override_hook;
pub mod persistence;
pub mod store;
pub mod ui;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options;
use crate::{log_info, log_warn};
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether `enable()` completed (row injection present, bootstrap spawned).
/// Later steps' hook callbacks gate on this.
static MOD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// True while the mod is enabled and its machinery is live.
pub fn is_active() -> bool {
    MOD_ACTIVE.load(Ordering::Acquire)
}

pub struct PerSongJudgementOffsetsMod;

impl PerSongJudgementOffsetsMod {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PerSongJudgementOffsetsMod {
    fn default() -> Self {
        Self::new()
    }
}

impl Mod for PerSongJudgementOffsetsMod {
    fn id(&self) -> &str {
        "per-song-judgement-offsets"
    }

    fn name(&self) -> &str {
        "Per-Song Judgement Offsets"
    }

    fn description(&self) -> &str {
        "Per-player judgement offsets that follow the selected song, overriding the stock JUDGEMENT OFFSET for that song only"
    }

    fn required_signatures(&self) -> &[&str] {
        // player_option_table: the Option+0x24 write target (Step 5).
        // selectmusic_model: the wheel-selection poll anchor (Step 4).
        &["player_option_table", "selectmusic_model"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        ui::init(ctx);
        override_hook::init(ctx);
        true
    }

    fn enable(&mut self) {
        // Design requirement 9 / register D20: no UI ⇒ fully inert.
        if !custom_options::row_injection_available() {
            log_warn!(
                "PerSongJudgementOffsets: option-row injection unavailable -- mod fully inert"
            );
            return;
        }
        if !ui::enable() {
            return; // row registration failed — inert (WARN already logged)
        }
        bootstrap::start();
        override_hook::enable();
        persistence::register();
        MOD_ACTIVE.store(true, Ordering::Release);
        log_info!("PerSongJudgementOffsets: enabled (rows registered, bootstrap crawl started)");
    }

    fn disable(&mut self) {
        MOD_ACTIVE.store(false, Ordering::Release);
        override_hook::on_mod_disable();
        log_info!("PerSongJudgementOffsets: disabled");
    }

    fn is_active(&self) -> bool {
        is_active()
    }
}
