//! Timing Stats Widget — per-player text widget during gameplay showing
//! EX loss, Current, Max, Abs Mean, and Mean ms-error values.

use std::sync::{Arc, Mutex, OnceLock};

use crate::log_info;
use crate::services::{custom_options, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::text_widget::TextWidget;

use super::{calorie_feed, data_feed};

const WIDGET_SCALE: f32 = 0.5;

const P1_X: f32 = 20.0;
const P2_X: f32 = 1155.0;
const BASE_Y: f32 = 425.0;

struct TimingStatsState {
    p1: Option<TextWidget>,
    p2: Option<TextWidget>,
    visible: bool,
}

static STATE: OnceLock<Arc<Mutex<TimingStatsState>>> = OnceLock::new();

fn state() -> &'static Arc<Mutex<TimingStatsState>> {
    STATE.get_or_init(|| {
        Arc::new(Mutex::new(TimingStatsState {
            p1: None,
            p2: None,
            visible: false,
        }))
    })
}

fn create_player_widget(x: f32) -> Option<TextWidget> {
    let w = widget_renderer::create_text_widget()?;
    w.set_position(x, BASE_Y);
    w.set_scale(WIDGET_SCALE, WIDGET_SCALE);
    w.set_color(1.0, 1.0, 1.0, 1.0);
    w.set_outline(0.0, 0.0, 0.0, 1.0, 1);
    w.set_text("EX: -0\nCurrent: +0ms\nMax: 0ms\nAbs(μ): 0.00ms\nμ: +0.00ms\nCal: 0.00");
    w.hide();
    Some(w)
}

pub fn enable() {
    log_info!("timing_stats_widget: enabled (widgets created on first gameplay entry)");
}

fn ensure_widgets_created(s: &mut TimingStatsState) {
    if !widget_renderer::is_available() {
        return;
    }
    if s.p1.is_none() {
        s.p1 = create_player_widget(P1_X);
    }
    if s.p2.is_none() {
        s.p2 = create_player_widget(P2_X);
    }
}

pub fn disable() {
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
