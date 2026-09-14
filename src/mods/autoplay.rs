//! Autoplay Mod — Per-player autoplay toggled through the custom options UI.
//!
//! Registers a bool-toggle option on the Assist (Page5) and Mods (Page6)
//! tabs via the custom options framework. When a player enables autoplay,
//! the mod asks the shared `foot_panel_swap` service for the `Perfect`
//! controller on that side: the service's pre-judge callback swaps the side's
//! IFootPanel pointer for the game's built-in AutoFootPanel (populating the
//! Results vector with perfect auto-inputs before the native judgeNotes runs)
//! and its post-judge callback restores it. This mod owns no judge callbacks
//! and no panel object of its own — `services/foot_panel_swap` is the single
//! owner of that seam (the multiplayer bot is its other client, and a bot
//! armed on a side outranks this mod's request for that side).
//!
//! Per-player isolation: each side's autoplay request is independent. P1
//! toggling autoplay on does not affect P2's state.
//!
//! Anti-fake watermark: while a song is being autoplayed, a bouncing
//! rainbow "Autoplay Enabled" label (the Hello World mod's DVD-screensaver
//! behavior) is rendered over gameplay and kept up through the results
//! screen, so captured footage/screenshots of autoplayed scores are
//! identifiable. See the watermark section below for the exact rules.

use std::sync::{Arc, Mutex};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::services::foot_panel_swap::{self, Controller};
use crate::services::{scene_manager, score_guard, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::bounce::{hsv_to_rgb, Bouncer};
use crate::widgets::text_widget::TextWidget;
use crate::{log_info, log_warn};

fn autoplay_on_change(player_side: u8, new_value: i32) {
    if player_side < 2 {
        let enabled = new_value != 0;
        let side = player_side as usize;
        // The shared swap service applies the request per judge frame (a bot
        // armed on this side outranks it — see `foot_panel_swap`).
        foot_panel_swap::set_perfect(side, enabled);
        // Mirror the per-side state into the score guard so the profile-save
        // trampoline suppresses this side's score upload while autoplay is on.
        score_guard::set_autoplay_taint(side, enabled);
        log_info!(
            "Autoplay: side={} {}",
            player_side,
            if enabled { "ON" } else { "OFF" }
        );
    }
}

// ── "Autoplay Enabled" watermark ────────────────────────────────────
// Bouncing rainbow label (same math/behavior as Hello World's demo text,
// via the shared `widgets::bounce` helpers) shown whenever a song was
// autoplayed, kept up through the results screen so screenshots and
// videos of autoplayed scores are identifiable.
//
// Visibility is re-evaluated every tick from current state — never
// latched to a specific scene transition — so quick-fail redirects,
// in-place quick restarts, and session ends all behave correctly:
//   * Observing (scene == GAMEPLAY && an ENTERED side's autoplay ON) arms
//     it. Entered-gated because the per-side option values outlive the
//     player: the JSON cache primes BOTH sides at boot and a profile load
//     only overwrites the side that carded in, so a stale `p2.autoplay=1`
//     from an earlier 2P session lit the watermark over a solo P1 whose
//     own autoplay was OFF (cabinet, 2026-09-01). A non-entered side has
//     no GamePlayActor, so its flag never engages autoplay anyway. When
//     entered-state is unavailable the side counts as entered (fail toward
//     showing the anti-fake mark, never toward hiding it).
//   * Any scene outside {GAMEPLAY, STAGE_RESULT, RESULTS_DETAIL} disarms
//     it (STAGE_RESULT is the post-song loader between gameplay and the
//     results detail screen — included so the label doesn't blink off
//     during the transition).
//   * It renders whenever armed: toggling autoplay OFF mid-song or at the
//     results screen does NOT hide it — the label persists until the
//     player leaves the gameplay/results flow.

const WATERMARK_TICK_MS: u64 = 16;
const WATERMARK_SCALE: f32 = 1.5;
// ~14.8 px/char at scale 1.0 (measured from Hello World's 26-char string
// at 385 px), "Autoplay Enabled" = 16 chars.
const WATERMARK_W: f32 = 240.0 * WATERMARK_SCALE;
const WATERMARK_H: f32 = 32.0 * WATERMARK_SCALE;

struct WatermarkState {
    widget: Option<TextWidget>,
    bouncer: Bouncer,
    hue: f32,
    visible: bool,
    /// True once autoplay has been seen ON during the current song's
    /// gameplay; cleared on any scene outside the gameplay/results flow.
    armed: bool,
    running: bool,
}

unsafe impl Send for WatermarkState {}

fn new_watermark_state() -> WatermarkState {
    WatermarkState {
        widget: None,
        bouncer: Bouncer {
            x: 100.0,
            y: 100.0,
            dx: 2.0,
            dy: 1.5,
            w: WATERMARK_W,
            h: WATERMARK_H,
        },
        hue: 0.0,
        visible: false,
        armed: false,
        running: false,
    }
}

/// Whether `side` counts toward the watermark: the swap service's EFFECTIVE
/// controller for the side is `Perfect` (so a side driven by the multiplayer
/// bot — which outranks a cached `autoplay = ON` — never lights it) and the
/// side is entered (unknown entered-state ⇒ counts — see the rules above).
fn side_autoplay_engaged(side: usize) -> bool {
    foot_panel_swap::controller(side) == Controller::Perfect
        && stage_records::side_entered(side).unwrap_or(true)
}

fn watermark_tick(st: &Arc<Mutex<WatermarkState>>) {
    let current = if scene_manager::is_available() {
        scene_manager::current_scene()
    } else {
        -1
    };
    let autoplay_on = side_autoplay_engaged(0) || side_autoplay_engaged(1);
    let in_results_flow = matches!(
        current,
        scene::GAMEPLAY | scene::STAGE_RESULT | scene::RESULTS_DETAIL
    );

    let mut s = st.lock().unwrap();

    if current == scene::GAMEPLAY && autoplay_on {
        s.armed = true;
    }
    if !in_results_flow {
        s.armed = false;
    }

    let show = s.armed;
    if show != s.visible {
        s.visible = show;
        if show {
            s.bouncer.randomize();
        }
        if let Some(ref w) = s.widget {
            if show {
                w.show();
            } else {
                w.hide();
            }
        }
    }

    if s.visible {
        s.bouncer.tick();
        s.hue = (s.hue + 2.0) % 360.0;
        let (r, g, b) = hsv_to_rgb(s.hue, 1.0, 1.0);
        if let Some(ref w) = s.widget {
            w.set_position(s.bouncer.x, s.bouncer.y);
            w.set_color(r, g, b, 1.0);
        }
    }
}

/// Spawn the watermark tick thread. Creates the text widget lazily (on the
/// render thread) once the widget renderer is up, then evaluates
/// arm/visibility state and advances the bounce animation every tick.
fn spawn_watermark_thread(st: Arc<Mutex<WatermarkState>>) {
    st.lock().unwrap().running = true;
    std::thread::spawn(move || {
        let mut widget_requested = false;
        loop {
            {
                let s = st.lock().unwrap();
                if !s.running {
                    break;
                }
            }

            if !widget_requested && widget_renderer::is_available() {
                widget_requested = true;
                let st2 = st.clone();
                widget_renderer::run_on_render_thread(move || {
                    let mut s = st2.lock().unwrap();
                    if let Some(tw) = widget_renderer::create_text_widget() {
                        tw.set_text("Autoplay Enabled");
                        tw.set_scale(WATERMARK_SCALE, WATERMARK_SCALE);
                        tw.set_color(1.0, 0.0, 0.0, 1.0);
                        // Honor whatever visibility the tick loop already
                        // decided (e.g. autoplay armed before the renderer
                        // came up).
                        if s.visible {
                            tw.show();
                        } else {
                            tw.hide();
                        }
                        s.widget = Some(tw);
                    }
                });
            }

            watermark_tick(&st);
            std::thread::sleep(std::time::Duration::from_millis(WATERMARK_TICK_MS));
        }
    });
}

pub struct AutoplayMod {
    watermark: Arc<Mutex<WatermarkState>>,
}

unsafe impl Send for AutoplayMod {}

impl AutoplayMod {
    pub fn new() -> Self {
        Self {
            watermark: Arc::new(Mutex::new(new_watermark_state())),
        }
    }
}

impl Mod for AutoplayMod {
    fn id(&self) -> &str {
        "autoplay"
    }
    fn name(&self) -> &str {
        "Autoplay"
    }
    fn description(&self) -> &str {
        "Per-player auto-play toggle in the options menu"
    }
    fn required_signatures(&self) -> &[&str] {
        // The judge swap's signatures (`judge_notes`, `auto_foot_panel_vtable`,
        // `auto_foot_panel_update`) are owned by `foot_panel_swap`; this mod
        // only needs that service to be up.
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        if !foot_panel_swap::is_available() {
            log_warn!("Autoplay: foot_panel_swap service unavailable -- autoplay inactive");
            return false;
        }
        true
    }

    fn enable(&mut self) {
        // Fail closed: an autoplayed score is fabricated, so autoplay must not
        // be usable unless the score-submission guard can suppress its upload.
        // If the guard's save hook didn't install, refuse to enable entirely —
        // register no option row (so no request ever reaches the swap
        // service) and no watermark, so the player has no way to produce a
        // faked score that would reach the server.
        if !score_guard::is_available() {
            log_warn!(
                "Autoplay: score-submission guard unavailable -- refusing to enable (fail-closed)"
            );
            return;
        }

        // Register the custom option. The change callback forwards the
        // per-player request to the swap service; the initial dispatch (fired
        // by register_option for both sides with default_value=0) will set
        // both sides to OFF.
        if custom_options::is_available() {
            let spec = RegisterSpec::bool_toggle("autoplay")
                .display_name("Autoplay")
                .description(
                    "The game plays every step perfectly on its own; scores are never saved",
                )
                .default_value(0)
                .on_change(autoplay_on_change);
            match custom_options::register_option(spec) {
                Ok(_handle) => {
                    log_info!("Autoplay: registered custom option on Mods tab");
                }
                Err(e) => {
                    log_warn!("Autoplay: custom option registration failed: {e}");
                }
            }
        } else {
            log_warn!("Autoplay: custom_options service unavailable -- option row will not render");
        }

        // Start the "Autoplay Enabled" watermark. Only reached when the swap
        // service is up (init gate), i.e. autoplay can actually engage.
        spawn_watermark_thread(self.watermark.clone());

        log_info!("Autoplay: enabled (per-player, toggled via options menu)");
    }

    fn disable(&mut self) {
        foot_panel_swap::set_perfect(0, false);
        foot_panel_swap::set_perfect(1, false);
        {
            let mut s = self.watermark.lock().unwrap();
            s.running = false;
            s.armed = false;
            s.visible = false;
            if let Some(ref mut w) = s.widget {
                w.destroy();
            }
            s.widget = None;
        }
        log_info!("Autoplay: disabled");
    }
}
