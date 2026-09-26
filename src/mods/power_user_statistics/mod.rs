//! Power User Statistics Mod — per-player timing statistics for players who want the numbers:
//! a live ms-error / EX / calorie readout during gameplay, the pacemaker readout swapped for
//! the latest step's ms error, and a per-song CSV of every step's timing.
//!
//! Mod id `power-user-statistics`, default ON (not in `DEFAULT_OFF_MODS`). Every feature is
//! gated per player by its own option row, so enabling the mod shows nothing until a player
//! turns a row on.
//!
//! ## Features
//!
//! - **Realtime Gameplay Statistics** (`timing_stats`): one native `TextWidget` per side,
//!   created at the first GAMEPLAY entry, shown at that side's first judgement of the song
//!   and kept through the post-song loader and the stage results (0-idx 29 → 30); any other
//!   destination hides it. Content is DETAILED (EX loss, current / max / abs-mean / mean ms
//!   error, live calories) or STREAMLINED (Δ, Max Δ, EX loss, then per-grade counts), in a
//!   SIDE COLUMN, BOTTOM LINE or TOP LINE layout.
//! - **Pacemaker → MS Error** (`pacemaker_to_mserror` + child `pacemaker_threshold`): an
//!   11-byte JMP patch in the `NoteResultActor` pacemaker render case (msg 0x1036) replaces
//!   the score delta with the side's latest ms error, forces the readout visible even with no
//!   ghost / rival data, and draws it white while `|error| < threshold` (0 = always colored).
//! - **Export Step Data (CSV)** (`step_data_export`): per-step expected / actual / delta rows,
//!   written when the song leaves GAMEPLAY.
//! - **Realtime calories**: the `Cal:` line of the DETAILED readout, read from the game's own
//!   per-stage kcal accumulator.
//!
//! Every user-facing signed ms value shows POSITIVE = FAST, NEGATIVE = SLOW; the feed keeps
//! the raw `actual − expected` delta and negates only at the display boundary
//! (`readout::display_ms`, re-exported as `data_feed::display_ms`). Miss and O.K. are not
//! timing samples — they count toward EX loss and the grade tallies only, and the pacemaker
//! readout keeps the last real step's error across a Miss.
//!
//! ## Submodules
//!
//! - `data_feed.rs` — owns the ONE `judge_submit` detour (see below) and the per-side
//!   `MsErrorAccum` buffers + lock-free `latest_ms_error`.
//! - `timing_stats_widget.rs` — the widgets, their scene lifecycle, the GLOBAL SETTINGS layout
//!   rows, the config seed / whole-section persist, and the `bottom_text` hide.
//! - `readout.rs` — pure layout / content / geometry / alignment / composition model and the
//!   display sign helpers (host-tested).
//! - `pacemaker_swap.rs` — the 0x1036 patch, its hand-assembled stub and the white-zone color
//!   redirect.
//! - `csv_export.rs` — song identity snapshot and the CSV writer.
//! - `calorie_feed.rs` — detour on the `CalcCalorieActor` tick (`calc_calorie_tick`) caching
//!   each side's live kcal.
//!
//! ## The shared `judge_submit` detour
//!
//! `data_feed::install` is the only installer of the `judge_submit` detour and is idempotent
//! (returns `true` if already installed). This mod, `timing_offsets` (auto-calibration) and
//! `s_marvelous` all call it from their `init`, so each of those features works with this mod
//! disabled. The detour body is the only place the per-step ms error exists, so it also hosts
//! the other features' hot-path taps: the calibration accumulator (`calibration_arm` /
//! `calibration_reset` / `calibration_take`) and the S-Marvelous classification feed
//! (pre-original) plus its display fan-out (post-original — after the stock handler's
//! `in_marvelous` play). Never add a second detour on `judge_submit`; add a tap here instead.
//! The detour is never removed: disabling this mod stops the widget, the pacemaker patch and
//! the CSV flush, not the feed.
//!
//! ## Invariants
//!
//! - The feed and the widget text update run inside the judge hot path: buffer access there
//!   is `try_lock` only (a contended lock drops that sample's buffer update, never blocks),
//!   and the S-Marv / calibration taps are lock-free so contention can never drop them.
//! - Widget creation, show / hide and re-layout happen on the render thread
//!   (`widget_renderer::run_on_render_thread`).
//! - Buffers reset at GAMEPLAY entry and on an in-place `song_reset` (the aborted attempt must
//!   not reach the CSV); the cached kcal resets only at GAMEPLAY entry — the
//!   `CalcCalorieActor` survives an in-place reset and keeps accumulating.
//! - **Calibration suppression** (`set_calibration_suppress`, set / cleared per song by
//!   `timing_offsets::calibration`): while set, the widget never shows and the pacemaker swap
//!   behaves as if its option were OFF, because the live ms error is the very signal being
//!   calibrated. Data collection (buffers, CSV) is unaffected.
//! - **`bottom_text`:** in the BOTTOM LINE layout the widgets sit where the stock CREDIT /
//!   PASELI / ONLINE text draws, so during the widget phase this mod hides it under its own
//!   `HideReason::PowerUserStatistics` bit (independent of the operator's `hide-bottom-text`
//!   mod); released at disable. The TOP LINE layout never hides it.
//! - **`s_marvelous`:** the STREAMLINED content shows an S-Marv count only while that mod is
//!   enabled (`s_marvelous::is_enabled()`), and Marv is then exclusive of S-Marv.
//!
//! ## Degradation
//!
//! No required signatures; `init` always succeeds. Missing `judge_submit` disables the
//! widget (nothing to feed it); missing `pacemaker_render_input` disables the swap (a missing
//! `note_result_actor_vtable` only disables the force-visible write, and underivable color
//! loads fall back to zeroing the value in the white zone); missing `calc_calorie_tick` leaves
//! the calorie line at 0. If `custom_options` is unavailable `enable` registers nothing and
//! starts neither the widget nor the swap.
//!
//! ## Config and option rows
//!
//! Per-player rows (`custom_options`, all `PersistMode::Full`): `timing_stats`,
//! `pacemaker_to_mserror`, `pacemaker_threshold` (0..=50 ms, default 10, shown only when the
//! parent is ON) and `step_data_export`; labels come from `scripts/option_strings.py`.
//! Cabinet-wide widget geometry lives in the DLL-owned `power_user_statistics` section of
//! mod-config.json (`widget_scale_percent`, `widget_layout`, `widget_content`,
//! `widget_offset_x` / `_y`, `widget_alignment`, `horizontal_offset_x` / `_y`,
//! `top_line_offset_x` / `_y`), seeded at enable and edited from the GLOBAL SETTINGS
//! `pus_widget_*` rows, which rewrite the whole section on every edit.
//!
//! RE notes: `docs/pacemaker_display_research.md`, `docs/calorie_weight_profile_research.md`.
//! Host tests: `scripts/validate_power_user_statistics.sh` (mounts `readout.rs`).

pub mod calorie_feed;
pub mod csv_export;
pub mod data_feed;
pub mod pacemaker_swap;
pub mod readout;
pub mod timing_stats_widget;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec, ScalarFormat, ShowWhen};
use crate::services::scene_manager;
use crate::services::song_reset;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use std::sync::atomic::{AtomicBool, Ordering};

/// Calibration suppression (timing-offsets auto-calibration): while set, the
/// realtime timing readouts stay hidden — they leak the exact signal being
/// calibrated (the live ms error). Song-scoped, set/cleared by
/// `timing_offsets::calibration`; data collection (buffers, CSV export) is
/// unaffected. No-op when this mod is disabled (widget destroyed, patch
/// restored — nothing to suppress).
static CALIBRATION_SUPPRESS: AtomicBool = AtomicBool::new(false);

/// Set/clear the calibration suppression flag.
pub fn set_calibration_suppress(on: bool) {
    CALIBRATION_SUPPRESS.store(on, Ordering::Release);
}

/// Whether the realtime timing readouts are calibration-suppressed (read
/// per judge dispatch by the widget and the pacemaker swap).
pub(crate) fn calibration_suppressed() -> bool {
    CALIBRATION_SUPPRESS.load(Ordering::Acquire)
}

pub struct PowerUserStatisticsMod {
    data_feed_installed: bool,
    pacemaker_swap_ready: bool,
    calorie_feed_installed: bool,
    scene_cb_id: Option<usize>,
    reset_cb_id: Option<usize>,
}

impl PowerUserStatisticsMod {
    pub fn new() -> Self {
        Self {
            data_feed_installed: false,
            pacemaker_swap_ready: false,
            calorie_feed_installed: false,
            scene_cb_id: None,
            reset_cb_id: None,
        }
    }
}

impl Mod for PowerUserStatisticsMod {
    fn id(&self) -> &str {
        "power-user-statistics"
    }
    fn name(&self) -> &str {
        "Power User Statistics"
    }
    fn description(&self) -> &str {
        "Per-player ms-error stats, pacemaker swap, CSV export"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.data_feed_installed = data_feed::install(ctx.signatures);
        if !self.data_feed_installed {
            log_warn!("PowerUserStatistics: data feed unavailable — sub-features disabled");
        }
        self.pacemaker_swap_ready = pacemaker_swap::init(ctx.signatures);
        if !self.pacemaker_swap_ready {
            log_warn!("PowerUserStatistics: pacemaker swap signature missing — feature disabled");
        }
        self.calorie_feed_installed = calorie_feed::install(ctx.signatures);
        if !self.calorie_feed_installed {
            log_warn!(
                "PowerUserStatistics: calorie tick signature missing — realtime calories disabled"
            );
        }
        true
    }

    fn enable(&mut self) {
        if !custom_options::is_available() {
            log_warn!("PowerUserStatistics: custom_options unavailable — options will not render");
            return;
        }

        let specs = [
            RegisterSpec::bool_toggle("timing_stats")
                .display_name("Realtime Gameplay Statistics")
                .description(
                    "Live timing readout during gameplay: ms error, fast/slow counts, calories",
                ),
            RegisterSpec::bool_toggle("pacemaker_to_mserror")
                .display_name("Pacemaker -> MS Error")
                .description("Replaces the pacemaker readout with your latest step's ms error"),
            RegisterSpec::scalar(
                "pacemaker_threshold",
                0,
                50,
                1,
                ScalarFormat::Unit { unit: "ms" },
            )
            .display_name("White Threshold")
            .description("Largest ms error shown white instead of colored (0 = always colored)")
            .default_value(10)
            .show_when(ShowWhen::Equals {
                parent_id: "pacemaker_to_mserror".into(),
                value: 1,
            }),
            RegisterSpec::bool_toggle("step_data_export")
                .display_name("Export Step Data (CSV)")
                .description("Writes a per-song CSV of every step's timing to the export folder"),
        ];

        for spec in specs {
            match custom_options::register_option(spec) {
                Ok(_) => {}
                Err(e) => {
                    log_warn!("PowerUserStatistics: option registration failed: {:?}", e);
                }
            }
        }

        if self.data_feed_installed {
            timing_stats_widget::enable();
        }
        if self.pacemaker_swap_ready {
            pacemaker_swap::enable();
        }

        if scene_manager::is_available() {
            let id = scene_manager::on_scene_change(Box::new(|prev, next| {
                if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
                    // Leaving gameplay normally (song end, quick fail) — flush CSV.
                    csv_export::flush();
                }
                if next == scene::GAMEPLAY {
                    // Entering gameplay — either fresh start or quick restart.
                    // Reset buffers (discards incomplete attempt on restart).
                    let csv_p1 = custom_options::get_value(0, "step_data_export").unwrap_or(0) != 0;
                    let csv_p2 = custom_options::get_value(1, "step_data_export").unwrap_or(0) != 0;
                    data_feed::reset_buffers(csv_p1, csv_p2);
                    // Zero the cached live kcal so the previous song's total
                    // can't show before the new actor's first tick.
                    calorie_feed::reset();
                }
                timing_stats_widget::on_scene_change(prev, next);
            }));
            self.scene_cb_id = Some(id);
        }

        // In-place song reset (quick restart's instant path): no scene
        // transition fires, so mirror the gameplay-entry buffer reset —
        // the aborted attempt's ms-error samples and song identity must
        // not pollute the replay's CSV row. The calorie feed is
        // deliberately NOT reset: the CalcCalorieActor survives the reset
        // and its per-stage kcal keeps accumulating (calories were
        // physically burned; design §6 decision).
        if song_reset::is_available() {
            self.reset_cb_id = Some(song_reset::on_song_reset(|_t_ms| {
                let csv_p1 = custom_options::get_value(0, "step_data_export").unwrap_or(0) != 0;
                let csv_p2 = custom_options::get_value(1, "step_data_export").unwrap_or(0) != 0;
                data_feed::reset_buffers(csv_p1, csv_p2);
                log_info!("PowerUserStatistics: song reset -- ms-error buffers cleared");
            }));
        }

        log_info!("PowerUserStatistics: enabled");
    }

    fn disable(&mut self) {
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        if let Some(id) = self.reset_cb_id.take() {
            song_reset::remove_callback(id);
        }
        timing_stats_widget::disable();
        pacemaker_swap::disable();
        log_info!("PowerUserStatistics: disabled");
    }
}
