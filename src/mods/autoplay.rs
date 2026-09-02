//! Autoplay Mod — Per-player autoplay toggled through the custom options UI.
//!
//! Registers a bool-toggle option on the Assist (Page5) and Mods (Page6)
//! tabs via the custom options framework. When a player enables autoplay,
//! the pre-judge callback swaps that side's IFootPanel pointer for the
//! game's built-in AutoFootPanel, which populates the Results vector with
//! perfect auto-inputs before the native judgeNotes runs.
//!
//! Per-player isolation: each side's autoplay flag is an independent
//! AtomicBool. P1 toggling autoplay on does not affect P2's state.
//!
//! Judge-hook integration:
//!   * Pre-judge callback (`Priority::Late`): reads the per-side atomic;
//!     if false, no-ops. If true, stashes the original foot panel, writes
//!     AutoFootPanel into the slot, and calls AutoFootPanel::update.
//!   * Post-judge callback (`Priority::Early`): restores the stashed
//!     foot panel pointer for the relevant side.
//!
//! Anti-fake watermark: while a song is being autoplayed, a bouncing
//! rainbow "Autoplay Enabled" label (the Hello World mod's DVD-screensaver
//! behavior) is rendered over gameplay and kept up through the results
//! screen, so captured footage/screenshots of autoplayed scores are
//! identifiable. See the watermark section below for the exact rules.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::services::judge_hook::{self, CallbackHandle, Priority};
use crate::services::{scene_manager, score_guard, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::bounce::{hsv_to_rgb, Bouncer};
use crate::widgets::text_widget::TextWidget;
use crate::{log_info, log_warn};

const NOTE_LIST_PTR: usize = 0x0B0;
const NOTE_COUNT: usize = 0x168;
const AUTO_PANEL_SIZE: usize = 0x40;

type AutoUpdateFn = unsafe extern "C" fn(*mut u8, *const u8, i32, i32);

// ── Shared state read by the callbacks ──────────────────────────────
static mut AUTO_PANEL: *mut u8 = std::ptr::null_mut();
static mut AUTO_UPDATE: Option<AutoUpdateFn> = None;
static FOOT_PANEL_OFFSET: AtomicUsize = AtomicUsize::new(0);

// ── Per-player autoplay enable flags ────────────────────────────────
// Written by the custom-options change callback; read by the pre-judge
// callback to decide whether to swap the foot panel for this side.
static AUTOPLAY_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

// ── Per-frame scratch state ─────────────────────────────────────────
// One stash per side so P1 and P2 can restore independently when both
// are in gameplay simultaneously (double-play or versus).
static ORIGINAL_FOOT_PANEL: [AtomicPtr<u8>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

/// Offset of the play-side enum (0=left/P1, 1=right/P2) within the
/// gameplay actor struct. Located immediately before the play-mode
/// field at +0x88. In doubles mode the value is 0 (left side owns
/// both pads).
const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;

fn autoplay_on_change(player_side: u8, new_value: i32) {
    if player_side < 2 {
        let enabled = new_value != 0;
        AUTOPLAY_ENABLED[player_side as usize].store(enabled, Ordering::Release);
        // Mirror the per-side state into the score guard so the profile-save
        // trampoline suppresses this side's score upload while autoplay is on.
        score_guard::set_autoplay_taint(player_side as usize, enabled);
        log_info!(
            "Autoplay: side={} {}",
            player_side,
            if enabled { "ON" } else { "OFF" }
        );
    }
}

fn autoplay_pre_judge(actor: *mut u8, music_count: i32) {
    let fp_offset = FOOT_PANEL_OFFSET.load(Ordering::Acquire);
    if fp_offset == 0 {
        return;
    }

    let side = unsafe { *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) };
    // In singles/doubles, side is 0 (left). In versus, side 1 (right) for P2.
    // Treat any value outside 0..=1 as side 0 to handle doubles gracefully
    // (doubles player controls both pads from the LEFT side slot).
    let side_idx = if side == 1 { 1usize } else { 0usize };

    if !AUTOPLAY_ENABLED[side_idx].load(Ordering::Acquire) {
        return;
    }

    unsafe {
        let fp_slot = actor.add(fp_offset);
        let original_fp = *(fp_slot as *const *mut u8);
        ORIGINAL_FOOT_PANEL[side_idx].store(original_fp, Ordering::Release);

        memory::write_ptr(fp_slot, AUTO_PANEL as *const u8);

        if let Some(update_fn) = AUTO_UPDATE {
            let note_count = memory::read_i32(actor.add(NOTE_COUNT) as *const u8);
            update_fn(
                AUTO_PANEL,
                actor.add(NOTE_LIST_PTR) as *const u8,
                note_count,
                music_count,
            );
        }
    }
}

fn autoplay_post_judge(actor: *mut u8, _music_count: i32) {
    let fp_offset = FOOT_PANEL_OFFSET.load(Ordering::Acquire);
    if fp_offset == 0 {
        return;
    }

    let side = unsafe { *(actor.add(ACTOR_PLAY_SIDE_OFFSET) as *const i32) };
    let side_idx = if side == 1 { 1usize } else { 0usize };

    if !AUTOPLAY_ENABLED[side_idx].load(Ordering::Acquire) {
        return;
    }

    unsafe {
        let fp_slot = actor.add(fp_offset);
        let original_fp =
            ORIGINAL_FOOT_PANEL[side_idx].swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !original_fp.is_null() {
            memory::write_ptr(fp_slot, original_fp);
        }
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

/// Whether `side`'s autoplay flag should count toward the watermark: ON and
/// the side is entered (unknown entered-state ⇒ counts — see the rules
/// above).
fn side_autoplay_engaged(side: usize) -> bool {
    AUTOPLAY_ENABLED[side].load(Ordering::Acquire)
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
    judge_notes_addr: *const u8,
    pre_handle: Option<CallbackHandle>,
    post_handle: Option<CallbackHandle>,
    watermark: Arc<Mutex<WatermarkState>>,
}

unsafe impl Send for AutoplayMod {}

impl AutoplayMod {
    pub fn new() -> Self {
        Self {
            judge_notes_addr: std::ptr::null(),
            pre_handle: None,
            post_handle: None,
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
        &[
            "judge_notes",
            "auto_foot_panel_vtable",
            "auto_foot_panel_update",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.judge_notes_addr = ctx.signatures.require_address("judge_notes");
        let vtable = ctx.signatures.require_address("auto_foot_panel_vtable");
        let update_addr = ctx.signatures.require_address("auto_foot_panel_update");

        match judge_hook::foot_panel_offset() {
            Some(off) => {
                FOOT_PANEL_OFFSET.store(off, Ordering::Release);
                log_info!("Autoplay: using foot panel offset 0x{:X}", off);
            }
            None => {
                log_warn!(
                    "Autoplay: judge_hook did not detect foot panel offset -- autoplay inactive"
                );
                return false;
            }
        }

        unsafe {
            AUTO_PANEL = memory::alloc_zeroed(AUTO_PANEL_SIZE);
            if AUTO_PANEL.is_null() {
                log_warn!("Autoplay: failed to allocate AutoFootPanel buffer");
                return false;
            }
            memory::write_ptr(AUTO_PANEL, vtable);
            AUTO_UPDATE = Some(std::mem::transmute::<
                *const u8,
                unsafe extern "C" fn(*mut u8, *const u8, i32, i32),
            >(update_addr));
        }
        true
    }

    fn enable(&mut self) {
        // Fail closed: an autoplayed score is fabricated, so autoplay must not
        // be usable unless the score-submission guard can suppress its upload.
        // If the guard's save hook didn't install, refuse to enable entirely —
        // register no judge callbacks and no option row, so the player has no
        // way to produce a faked score that would reach the server.
        if !score_guard::is_available() {
            log_warn!(
                "Autoplay: score-submission guard unavailable -- refusing to enable (fail-closed)"
            );
            return;
        }

        // Register the judge-hook callbacks so we can intercept judgeNotes.
        self.pre_handle = judge_hook::register_pre(Priority::Late, autoplay_pre_judge);
        self.post_handle = judge_hook::register_post(Priority::Early, autoplay_post_judge);

        if self.pre_handle.is_none() || self.post_handle.is_none() {
            log_warn!(
                "Autoplay: judge_hook service unavailable (pre={}, post={}) -- autoplay inactive",
                self.pre_handle.is_some(),
                self.post_handle.is_some()
            );
            return;
        }

        // Register the custom option. The change callback updates the
        // per-player AtomicBool; the initial dispatch (fired by
        // register_option for both sides with default_value=0) will set
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

        // Start the "Autoplay Enabled" watermark. Only reached when the
        // judge hooks registered, i.e. autoplay can actually engage.
        spawn_watermark_thread(self.watermark.clone());

        log_info!("Autoplay: enabled (per-player, toggled via options menu)");
    }

    fn disable(&mut self) {
        if let Some(h) = self.pre_handle.take() {
            judge_hook::unregister(h);
        }
        if let Some(h) = self.post_handle.take() {
            judge_hook::unregister(h);
        }
        AUTOPLAY_ENABLED[0].store(false, Ordering::Release);
        AUTOPLAY_ENABLED[1].store(false, Ordering::Release);
        ORIGINAL_FOOT_PANEL[0].store(std::ptr::null_mut(), Ordering::Release);
        ORIGINAL_FOOT_PANEL[1].store(std::ptr::null_mut(), Ordering::Release);
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
