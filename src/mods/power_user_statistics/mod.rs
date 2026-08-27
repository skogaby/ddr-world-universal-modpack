pub mod calorie_feed;
pub mod csv_export;
pub mod data_feed;
pub mod pacemaker_swap;
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
                1,
                50,
                1,
                ScalarFormat::Unit { unit: "ms" },
            )
            .display_name("White Threshold")
            .description("Largest ms error that still displays white instead of colored")
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
