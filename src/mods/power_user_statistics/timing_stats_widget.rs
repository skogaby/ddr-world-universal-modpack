//! Timing Stats Widget — per-player text widget during gameplay showing
//! EX loss, Current, Max, Abs Mean, Mean ms-error values and live calories.
//!
//! Layout (scale + a mirrored horizontal/vertical offset) is cabinet-wide:
//! seeded from the `power_user_statistics` config section at enable and
//! live-editable from the overlay menu's POWER USER STATISTICS rows on the
//! GLOBAL SETTINGS tab, which persist the whole section back. Edits re-lay
//! out any already-created widgets immediately (render thread).

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::log_info;
use crate::mods::config;
use crate::services::{custom_options, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::text_widget::{TextAlignment, TextWidget};

use super::{calorie_feed, data_feed};

/// Stock widget scale (== 100 % on the scale row).
const STOCK_SCALE: f32 = 0.5;

/// Horizontal CENTRE of each side's stat block, mirrored about the 1280-wide
/// screen centre so P1 and P2 read as the same layout. The native renderer
/// centres each `\n` line about this x independently (`desc+0xA8`, see
/// `TextWidget::set_alignment`), so the whole block is centre-aligned.
const P1_CENTER_X: f32 = 80.0;
const P2_CENTER_X: f32 = 1280.0 - P1_CENTER_X;
const BASE_Y: f32 = 425.0;

// Layout row ranges. Scale is a percentage of `STOCK_SCALE`; the offsets are
// px on the 1280x720 logical canvas. Horizontal: POSITIVE = inward (toward
// the screen centre) on BOTH sides — the one mirrored value moves the two
// blocks symmetrically. Vertical: POSITIVE = down (screen y grows downward).
pub const SCALE_MIN: i32 = 50;
pub const SCALE_MAX: i32 = 150;
pub const SCALE_DEFAULT: i32 = 100;
pub const OFFSET_X_MIN: i32 = -50;
pub const OFFSET_X_MAX: i32 = 400;
pub const OFFSET_Y_MIN: i32 = -400;
pub const OFFSET_Y_MAX: i32 = 200;

/// Overlay row keys (GLOBAL SETTINGS, grouped under this mod's header;
/// registration order = display order).
const SCALE_ROW_KEY: &str = "pus_widget_scale";
const OFFSET_X_ROW_KEY: &str = "pus_widget_offset_x";
const OFFSET_Y_ROW_KEY: &str = "pus_widget_offset_y";
const OWNING_MOD_ID: &str = "power-user-statistics";

/// Live layout values (already clamped). Written by config seeding and the
/// overlay rows; read on every (re)layout.
static LIVE_SCALE_PERCENT: AtomicI32 = AtomicI32::new(SCALE_DEFAULT);
static LIVE_OFFSET_X: AtomicI32 = AtomicI32::new(0);
static LIVE_OFFSET_Y: AtomicI32 = AtomicI32::new(0);

struct TimingStatsState {
    p1: Option<TextWidget>,
    p2: Option<TextWidget>,
    visible: bool,
    /// Whether the widgets were raised to the top of the widget render
    /// list this song (once, at the first show — see `raise_above_hud`).
    raised: bool,
}

static STATE: OnceLock<Arc<Mutex<TimingStatsState>>> = OnceLock::new();

fn state() -> &'static Arc<Mutex<TimingStatsState>> {
    STATE.get_or_init(|| {
        Arc::new(Mutex::new(TimingStatsState {
            p1: None,
            p2: None,
            visible: false,
            raised: false,
        }))
    })
}

/// Resolved (x, y, scale) for a side from the live layout values. Side 0 is
/// P1 (inward = +x), side 1 is P2 (inward = -x).
fn layout_for_side(side: usize) -> (f32, f32, f32) {
    let scale = STOCK_SCALE * LIVE_SCALE_PERCENT.load(Ordering::Relaxed) as f32 / 100.0;
    let dx = LIVE_OFFSET_X.load(Ordering::Relaxed) as f32;
    let dy = LIVE_OFFSET_Y.load(Ordering::Relaxed) as f32;
    let (base_x, inward) = if side == 0 {
        (P1_CENTER_X, 1.0)
    } else {
        (P2_CENTER_X, -1.0)
    };
    (base_x + inward * dx, BASE_Y + dy, scale)
}

fn apply_layout(w: &TextWidget, side: usize) {
    let (x, y, scale) = layout_for_side(side);
    w.set_position(x, y);
    w.set_scale(scale, scale);
}

/// Re-apply the live layout to any already-created widgets (render thread).
fn relayout_widgets() {
    let st = state().clone();
    widget_renderer::run_on_render_thread(move || {
        let Ok(s) = st.lock() else { return };
        if let Some(ref w) = s.p1 {
            apply_layout(w, 0);
        }
        if let Some(ref w) = s.p2 {
            apply_layout(w, 1);
        }
    });
}

fn create_player_widget(side: usize) -> Option<TextWidget> {
    let w = widget_renderer::create_text_widget()?;
    apply_layout(&w, side);
    w.set_alignment(TextAlignment::Center);
    w.set_color(1.0, 1.0, 1.0, 1.0);
    w.set_system_outline();
    w.set_text("EX: -0\nCurrent: +0ms\nMax: 0ms\nAbs(μ): 0.00ms\nμ: +0.00ms\nCal: 0.00");
    w.hide();
    Some(w)
}

/// Read + clamp one layout value from the config section, logging when the
/// operator's value was out of range.
fn configured(
    name: &str,
    pick: impl Fn(&config::PowerUserStatisticsConfig) -> Option<i32>,
    default: i32,
    min: i32,
    max: i32,
) -> i32 {
    let raw = config::get()
        .and_then(|c| c.power_user_statistics.as_ref())
        .and_then(pick)
        .unwrap_or(default);
    let clamped = raw.clamp(min, max);
    if clamped != raw {
        log_info!(
            "timing_stats_widget: {} {} out of range -- clamped to {}",
            name,
            raw,
            clamped
        );
    }
    clamped
}

/// Write the whole `power_user_statistics` section from the live values —
/// `save_json_key` REPLACES the section, so every row edit must emit every
/// key or the others silently reset on the next boot.
fn persist_section() {
    config::save_json_key(
        "power_user_statistics",
        serde_json::json!({
            "widget_scale_percent": LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
            "widget_offset_x": LIVE_OFFSET_X.load(Ordering::Relaxed),
            "widget_offset_y": LIVE_OFFSET_Y.load(Ordering::Relaxed),
        }),
    );
}

/// Shared row-edit tail: clamp, store, persist, re-lay out live widgets.
fn on_layout_row_change(slot: &'static AtomicI32, min: i32, max: i32, what: &str, v: i32) {
    let clamped = v.clamp(min, max);
    slot.store(clamped, Ordering::Relaxed);
    persist_section();
    relayout_widgets();
    log_info!("timing_stats_widget: {} set to {}", what, clamped);
}

fn register_overlay_rows() {
    use crate::mods::mod_menu::{self, ScalarRowSpec};
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: SCALE_ROW_KEY.to_string(),
        label: "Stats Widget Scale (%)".to_string(),
        hint: "Size of the realtime gameplay statistics block, percent of stock (100).".to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: SCALE_MIN,
        max: SCALE_MAX,
        step_fine: 5,
        step_coarse: 25,
        initial: LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
        on_change: Arc::new(|v| {
            on_layout_row_change(&LIVE_SCALE_PERCENT, SCALE_MIN, SCALE_MAX, "scale %", v)
        }),
    });
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: OFFSET_X_ROW_KEY.to_string(),
        label: "Stats Widget H-Offset (px)".to_string(),
        hint:
            "Horizontal shift, mirrored for P1/P2: + moves both blocks inward, - toward the edges."
                .to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: OFFSET_X_MIN,
        max: OFFSET_X_MAX,
        step_fine: 5,
        step_coarse: 25,
        initial: LIVE_OFFSET_X.load(Ordering::Relaxed),
        on_change: Arc::new(|v| {
            on_layout_row_change(&LIVE_OFFSET_X, OFFSET_X_MIN, OFFSET_X_MAX, "h-offset px", v)
        }),
    });
    mod_menu::register_scalar_row(ScalarRowSpec {
        key: OFFSET_Y_ROW_KEY.to_string(),
        label: "Stats Widget V-Offset (px)".to_string(),
        hint: "Vertical shift for both blocks: + moves down, - moves up.".to_string(),
        parent_row_key: Some(OWNING_MOD_ID.to_string()),
        min: OFFSET_Y_MIN,
        max: OFFSET_Y_MAX,
        step_fine: 5,
        step_coarse: 25,
        initial: LIVE_OFFSET_Y.load(Ordering::Relaxed),
        on_change: Arc::new(|v| {
            on_layout_row_change(&LIVE_OFFSET_Y, OFFSET_Y_MIN, OFFSET_Y_MAX, "v-offset px", v)
        }),
    });
}

pub fn enable() {
    LIVE_SCALE_PERCENT.store(
        configured(
            "widget_scale_percent",
            |c| c.widget_scale_percent,
            SCALE_DEFAULT,
            SCALE_MIN,
            SCALE_MAX,
        ),
        Ordering::Relaxed,
    );
    LIVE_OFFSET_X.store(
        configured(
            "widget_offset_x",
            |c| c.widget_offset_x,
            0,
            OFFSET_X_MIN,
            OFFSET_X_MAX,
        ),
        Ordering::Relaxed,
    );
    LIVE_OFFSET_Y.store(
        configured(
            "widget_offset_y",
            |c| c.widget_offset_y,
            0,
            OFFSET_Y_MIN,
            OFFSET_Y_MAX,
        ),
        Ordering::Relaxed,
    );
    register_overlay_rows();
    log_info!(
        "timing_stats_widget: enabled (scale {}%, offset x {} y {}; widgets created on first gameplay entry)",
        LIVE_SCALE_PERCENT.load(Ordering::Relaxed),
        LIVE_OFFSET_X.load(Ordering::Relaxed),
        LIVE_OFFSET_Y.load(Ordering::Relaxed)
    );
}

fn ensure_widgets_created(s: &mut TimingStatsState) {
    if !widget_renderer::is_available() {
        return;
    }
    if s.p1.is_none() {
        s.p1 = create_player_widget(0);
    }
    if s.p2.is_none() {
        s.p2 = create_player_widget(1);
    }
}

pub fn disable() {
    crate::mods::mod_menu::remove_rows_for(&[SCALE_ROW_KEY, OFFSET_X_ROW_KEY, OFFSET_Y_ROW_KEY]);
    let st = state().clone();
    widget_renderer::run_on_render_thread(move || {
        let mut s = st.lock().unwrap();
        if let Some(mut w) = s.p1.take() {
            w.destroy();
        }
        if let Some(mut w) = s.p2.take() {
            w.destroy();
        }
        s.visible = false;
    });

    log_info!("timing_stats_widget: disabled");
}

/// Called from scene_manager callback to show/hide based on gameplay state.
pub fn on_scene_change(_prev: i32, next: i32) {
    let entering_gameplay = next == scene::GAMEPLAY;
    let st = state().clone();
    widget_renderer::run_on_render_thread(move || {
        let mut s = st.lock().unwrap();

        if entering_gameplay {
            ensure_widgets_created(&mut s);
            // Don't show yet — widgets become visible on first update_text
            // call, which only fires for players that are actually playing.
            s.visible = true;
            s.raised = false;
            if let Some(ref w) = s.p1 {
                w.hide();
            }
            if let Some(ref w) = s.p2 {
                w.hide();
            }
        } else if s.visible {
            s.visible = false;
            if let Some(ref w) = s.p1 {
                w.hide();
            }
            if let Some(ref w) = s.p2 {
                w.hide();
            }
        }
    });
}

/// Once per song, relink both stat widgets to the TAIL of the widget render
/// list so they draw above every other gameplay HUD widget — in particular
/// the training strip, whose widgets allocate lazily at gameplay entry
/// (render-list z = list order; whichever mod created its widgets later
/// won). Deferred to frame start; the first judgement is comfortably after
/// the strip's texture resolve. Skipped while the overlay menu is open: the
/// menu must stay topmost, and it re-raises itself on every open anyway.
fn raise_above_hud(s: &TimingStatsState) {
    if s.raised || crate::mods::mod_menu::is_open() {
        return;
    }
    let wrappers = [
        s.p1.as_ref().map_or(0, |w| w.render_wrapper()),
        s.p2.as_ref().map_or(0, |w| w.render_wrapper()),
    ];
    let st = state().clone();
    widget_renderer::run_on_render_thread(move || {
        widget_renderer::bring_to_front(&wrappers);
        if let Ok(mut s) = st.lock() {
            s.raised = true;
        }
    });
}

/// Update widget text with current buffer values. Called from the
/// judge_submit detour (runs on the game's main/render thread).
/// Shows the widget on first call for this song (lazy show — only
/// players that are actually playing get their widget displayed).
pub fn update_text(player_side: usize) {
    let Ok(s) = state().try_lock() else { return };
    if !s.visible {
        return;
    }

    let widget = match player_side {
        0 => s.p1.as_ref(),
        1 => s.p2.as_ref(),
        _ => return,
    };
    let Some(w) = widget else { return };

    let option_on = custom_options::get_value(player_side as u8, "timing_stats").unwrap_or(0) != 0;
    if !option_on {
        return;
    }

    // Auto-calibration: the live ms-error readout leaks the exact signal
    // being calibrated. Suppression is set at GAMEPLAY entry (before the
    // first judgment), so the widget never becomes visible during a
    // calibration song. (Early-return BEFORE the show below.)
    if super::calibration_suppressed() {
        return;
    }

    // Make visible on first judgment for this player this song.
    w.show();
    raise_above_hud(&s);

    let bufs = data_feed::buffers();
    let Ok(b) = bufs[player_side].try_lock() else {
        return;
    };

    let current = b.current as f64;
    let max_abs = b.max_abs as f64;
    let (abs_mean, mean) = if b.count > 0 {
        (
            b.sum_abs as f64 / b.count as f64,
            b.sum as f64 / b.count as f64,
        )
    } else {
        (0.0, 0.0)
    };

    // Live calories burned this song. The game's accumulator (`actor+0x94`,
    // via the calorie tick hook) counts small-calories (cal); the game's own
    // display shows kcal, so divide by 1000 and show 2 decimals to match.
    // Part of the same "REALTIME GAMEPLAY STATISTICS" block — no separate gate.
    let kcal = calorie_feed::latest(player_side) as f32 / 1000.0;

    use std::fmt::Write;
    let mut buf_str = String::with_capacity(96);
    let _ = write!(
        buf_str,
        "EX: -{}\nCurrent: {:+.0}ms\nMax: {:.0}ms\nAbs(μ): {:.2}ms\nμ: {:+.2}ms\nCal: {:.2}",
        b.ex_loss, current, max_abs, abs_mean, mean, kcal
    );
    w.set_text(&buf_str);
}
