//! FF/RW scrub indicator (Training Mode Step 7, maintainer request
//! 2026-08-15): a music-player-style symbol flash confirming each
//! dispatched scrub — the RW double-triangle on the LEFT side of the
//! screen, FF on the RIGHT — with the toast's quick fade-in/hold/fade-out
//! envelope (the "Marker Set/Clear" feedback class, as an image).
//!
//! Two repo-shipped 128×128 PNGs (`data_mods/training_mode/tex/
//! training_scrub_{rw,ff}.png`, regenerate via
//! `scripts/gen_training_scrub_icons.py`) loaded through `asset_loader`
//! (the strip-marker model: requested once, never released —
//! process-lifetime chrome), one native `ImageWidget` per side, created
//! lazily on the render thread once its texture resolves and kept hidden
//! between flashes. The animation is the toast's generation-tokened,
//! self-requeueing render-thread callback verbatim: a newer flash
//! supersedes the running one mid-fade (and hides the other side's
//! icon), `dismiss` (mod disable) hides everything. Every failure is
//! fail-open: an unresolved texture or refused widget simply means no
//! flash — the scrub itself already happened.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;

use crate::services::{asset_loader, widget_renderer};
use crate::widgets::image_widget::{ImageWidget, ImageWidgetConfig};

/// The toast's fade envelope (visual parity with the marker feedback).
const FADE_IN_MS: u64 = 100;
const HOLD_MS: u64 = 250;
const FADE_OUT_MS: u64 = 300;

/// Icon geometry (1280×720): mid-height, inset from the screen edges far
/// enough to clear the chart-strip timeline at either placement (strip:
/// 8 px margin + ≤ 120 px wide).
const ICON_SIZE_PX: f32 = 96.0;
const ICON_CENTER_Y: f32 = 360.0;
const RW_CENTER_X: f32 = 180.0;
const FF_CENTER_X: f32 = 1100.0;

/// Repo-shipped icon assets (never released once loaded — the
/// strip-marker / mine-texture model).
const RW_TEX_PATH: &str = "./data_mods/training_mode/tex/training_scrub_rw.png";
const RW_TEX_STEM: &str = "training_scrub_rw";
const FF_TEX_PATH: &str = "./data_mods/training_mode/tex/training_scrub_ff.png";
const FF_TEX_STEM: &str = "training_scrub_ff";

/// One direction's icon: load-requested latch, resolved texture id, and
/// the lazily created widget.
#[derive(Default)]
struct Icon {
    load_requested: bool,
    texture: Option<i32>,
    widget: Option<ImageWidget>,
}

struct IndicatorState {
    /// `[0] = RW, [1] = FF`.
    icons: [Icon; 2],
    /// Which icon the current generation flashes (0 RW / 1 FF).
    active: usize,
    started: Instant,
}

// SAFETY: the widgets' native pointers are game memory valid for the
// process lifetime; all mutation happens on the render thread.
unsafe impl Send for IndicatorState {}

static STATE: Lazy<Mutex<IndicatorState>> = Lazy::new(|| {
    Mutex::new(IndicatorState {
        icons: Default::default(),
        active: 0,
        started: Instant::now(),
    })
});

/// Supersession token: each `show`/`dismiss` bumps it and orphans any
/// in-flight animation callback (the toast pattern).
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// The toast's fade curve: alpha at `elapsed_ms`, `None` once done.
fn fade_alpha(elapsed_ms: u64) -> Option<f32> {
    if elapsed_ms < FADE_IN_MS {
        return Some(elapsed_ms as f32 / FADE_IN_MS as f32);
    }
    let after_in = elapsed_ms - FADE_IN_MS;
    if after_in < HOLD_MS {
        return Some(1.0);
    }
    let after_hold = after_in - HOLD_MS;
    if after_hold < FADE_OUT_MS {
        return Some(1.0 - after_hold as f32 / FADE_OUT_MS as f32);
    }
    None
}

/// Pre-request the icon texture loads (mod enable): by the first scrub
/// the assets are normally resident, so even the first flash shows.
pub(super) fn prime() {
    if !widget_renderer::is_available() {
        return;
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(mut state) = STATE.lock() {
            for idx in 0..2 {
                ensure_load_requested(&mut state, idx);
            }
        }
    });
}

/// Flash the indicator for a dispatched scrub (`ff` = pinpad 9, shown
/// right; else RW, shown left). Callable from the frame thread.
pub(super) fn show(ff: bool) {
    if !widget_renderer::is_available() {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    if let Ok(mut state) = STATE.lock() {
        state.active = usize::from(ff);
        state.started = Instant::now();
    }
    widget_renderer::run_on_render_thread(move || tick(generation));
}

/// Hide both icons and orphan any in-flight animation (mod disable).
pub(super) fn dismiss() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    if !widget_renderer::is_available() {
        return;
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(state) = STATE.lock() {
            for icon in &state.icons {
                if let Some(ref widget) = icon.widget {
                    widget.hide();
                }
            }
        }
    });
}

/// Request one icon's asset load (render thread; once — never released,
/// the strip-marker model). A failed request stays latched: no retry
/// storm, no flash for the process lifetime (fail-open).
fn ensure_load_requested(state: &mut IndicatorState, idx: usize) {
    let (path, stem) = if idx == 0 {
        (RW_TEX_PATH, RW_TEX_STEM)
    } else {
        (FF_TEX_PATH, FF_TEX_STEM)
    };
    let icon = &mut state.icons[idx];
    if !icon.load_requested {
        icon.load_requested = true;
        if asset_loader::load(path, stem).is_none() {
            crate::log_warn!("ScrubIndicator: {} load failed -- no flash", stem);
        }
    }
    if icon.texture.is_none() {
        icon.texture = asset_loader::resolve(stem).map(|t| t.handle as i32);
    }
}

/// One animation frame (render thread): poll the asset, lazily create
/// the widget, hide the other side, apply the fade, re-queue.
fn tick(generation: usize) {
    if GENERATION.load(Ordering::Acquire) != generation {
        return; // superseded by a newer flash or a dismiss
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    let active = state.active;
    ensure_load_requested(&mut state, active);
    let other = &state.icons[1 - active];
    if let Some(ref widget) = other.widget {
        widget.hide();
    }
    if state.icons[active].widget.is_none() {
        if let Some(texture) = state.icons[active].texture {
            let center_x = if active == 0 {
                RW_CENTER_X
            } else {
                FF_CENTER_X
            };
            if let Some(widget) = widget_renderer::create_image_widget(&ImageWidgetConfig {
                x: center_x - ICON_SIZE_PX / 2.0,
                y: ICON_CENTER_Y - ICON_SIZE_PX / 2.0,
                width: ICON_SIZE_PX,
                height: ICON_SIZE_PX,
                texture_name: None,
                ..Default::default()
            }) {
                widget.set_texture_id(texture);
                widget.hide();
                state.icons[active].widget = Some(widget);
            }
        }
    }
    let elapsed_ms = state.started.elapsed().as_millis() as u64;
    let alpha = fade_alpha(elapsed_ms);
    let mut done = false;
    if let Some(ref widget) = state.icons[active].widget {
        match alpha {
            Some(alpha) => {
                widget.set_color(0xFFFFFFFF);
                widget.set_opacity(alpha);
                widget.show();
            }
            None => {
                widget.hide();
                done = true;
            }
        }
    } else if alpha.is_none() {
        // Texture never resolved within the flash window: give up quietly
        // (the load stays latched; the NEXT flash will find it resolved).
        done = true;
    }
    if done {
        return;
    }
    // Re-queue for the next frame — AFTER the state lock is released
    // (never hold a state mutex across a render-thread schedule).
    drop(state);
    widget_renderer::run_on_render_thread(move || tick(generation));
}
