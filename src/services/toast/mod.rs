//! Shared toast service: a short text notice at the bottom-center of the
//! screen. Promoted from Training Mode's gesture-feedback toast so any mod
//! can use it; grown for the auto-calibration feature with owned-`String`
//! text, a caller-specified flash hold, and a pulsing persistent mode
//! ("Calibrating..." breathing for a whole song).
//!
//! One native `TextWidget`, created lazily on the render thread and kept
//! (hidden) for the process lifetime — the autoplay-watermark precedent.
//! The animation is a generation-tokened, self-requeueing render-thread
//! callback (the shipped driver pattern): a newer toast supersedes the
//! running one mid-fade, and `dismiss` hides everything. No locks are held
//! across the re-queue schedule; the render callback is panic-free (no
//! unwraps).
//!
//! `dismiss()` is deliberately UNCONDITIONAL and shared: whoever calls it
//! hides whatever toast is up. The one known cross-mod edge (disabling
//! Training Mode mid-calibration-song hides the pulsing toast) is benign —
//! the calibration measurement is unaffected.

pub mod curve;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use once_cell::sync::Lazy;

use crate::services::widget_renderer;
use crate::widgets::text_widget::{TextAlignment, TextWidget};

use curve::ToastMode;

/// Bottom-center placement (1280×720; comfortably above the screen
/// edge). Centered by the NATIVE per-line horizontal alignment
/// (`desc+0xA8` — the renderer offsets each line by its own pre-measured
/// width × −0.5, exact glyph metrics), so the text can change freely
/// without repositioning.
const TOAST_CENTER_X: f32 = 640.0;
const TOAST_Y: f32 = 630.0;
const TOAST_SCALE: f32 = 1.2;

struct ToastState {
    widget: Option<TextWidget>,
    /// The text to display, its envelope, and when its animation started.
    text: String,
    mode: ToastMode,
    started: Instant,
    /// The generation whose text/style has been applied to the widget.
    applied_generation: usize,
}

// SAFETY: the widget's native pointer is game memory valid for the
// process lifetime; all mutation happens on the render thread.
unsafe impl Send for ToastState {}

static STATE: Lazy<Mutex<ToastState>> = Lazy::new(|| {
    Mutex::new(ToastState {
        widget: None,
        text: String::new(),
        mode: ToastMode::Flash {
            hold_ms: curve::FLASH_DEFAULT_HOLD_MS,
        },
        started: Instant::now(),
        applied_generation: 0,
    })
});

/// Supersession token: each `show`/`dismiss` bumps it and orphans any
/// in-flight animation callback.
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// Show (or restart) a flash toast with the classic short hold (the
/// original gesture-toast envelope). Callable from the frame thread;
/// everything widget-facing runs on the render thread.
pub fn flash(text: impl Into<String>) {
    show(
        text.into(),
        ToastMode::Flash {
            hold_ms: curve::FLASH_DEFAULT_HOLD_MS,
        },
    );
}

/// Show (or restart) a flash toast holding at full brightness for
/// `hold_ms` (e.g. 3000 for calibration refusals, 5000 for the result).
pub fn flash_with_hold(text: impl Into<String>, hold_ms: u64) {
    show(text.into(), ToastMode::Flash { hold_ms });
}

/// Show a pulsing persistent toast: a slow breathing fade loop that runs
/// until `dismiss()` or a newer toast supersedes it.
pub fn show_pulsing(text: impl Into<String>) {
    show(text.into(), ToastMode::Pulse);
}

fn show(text: String, mode: ToastMode) {
    if !widget_renderer::is_available() {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    if let Ok(mut state) = STATE.lock() {
        state.text = text;
        state.mode = mode;
        state.started = Instant::now();
    }
    widget_renderer::run_on_render_thread(move || tick(generation));
}

/// Hide the toast and orphan any in-flight animation. Unconditional —
/// hides whatever toast is currently up, whoever showed it.
pub fn dismiss() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    if !widget_renderer::is_available() {
        return;
    }
    widget_renderer::run_on_render_thread(|| {
        if let Ok(state) = STATE.lock() {
            if let Some(ref widget) = state.widget {
                widget.hide();
            }
        }
    });
}

/// One animation frame (render thread): lazily create the widget, apply
/// the current generation's text once, evaluate the envelope, re-queue.
fn tick(generation: usize) {
    if GENERATION.load(Ordering::Acquire) != generation {
        return; // superseded by a newer toast or a dismiss
    }
    let Ok(mut state) = STATE.lock() else {
        return;
    };
    if state.widget.is_none() {
        let Some(widget) = widget_renderer::create_text_widget() else {
            return; // renderer refused; drop this toast silently
        };
        widget.set_alignment(TextAlignment::Center);
        widget.set_scale(TOAST_SCALE, TOAST_SCALE);
        widget.set_position(TOAST_CENTER_X, TOAST_Y);
        widget.set_outline(0.0, 0.0, 0.0, 0.8, 2);
        widget.hide();
        state.widget = Some(widget);
    }
    if state.applied_generation != generation {
        state.applied_generation = generation;
        if let Some(ref widget) = state.widget {
            widget.set_text(&state.text);
        }
    }
    let elapsed_ms = state.started.elapsed().as_millis() as u64;
    let alpha = curve::alpha_at(state.mode, elapsed_ms);
    if let Some(ref widget) = state.widget {
        match alpha {
            Some(alpha) => {
                widget.set_color(1.0, 0.85, 0.0, alpha);
                widget.show();
            }
            None => {
                widget.hide();
                return;
            }
        }
    }
    // Re-queue for the next frame — AFTER the state lock is released
    // (never hold a state mutex across a render-thread schedule).
    drop(state);
    widget_renderer::run_on_render_thread(move || tick(generation));
}
