//! Auto-calibration session driver (timing-offsets sub-feature): the
//! "Calibrate next song?" overlay row, the arm/consume lifecycle, and the
//! end-of-song apply.
//!
//! Lifecycle (design: `.agents/planning/2026-08-26-auto-calibration/`):
//! - The arm is IN-MEMORY ONLY (never persisted; OFF at boot) and mirrors
//!   the contributed overlay row `timing_calibrate_next`.
//! - GAMEPLAY (scene 28) entry with the arm ON runs the guards: exactly one
//!   entered side (via `stage_records::side_entered`) and song rate 100 %
//!   (`song_rate` snapshot). Guards pass ⇒ a `Collecting` session + the
//!   pulsing "Calibrating..." toast; refusals ⇒ a `ConsumeOnly` session +
//!   a 3 s reason toast (2P / song speed) or a silent WARN (unreadable).
//! - GAMEPLAY exit (fires for every exit shape — natural end, quick
//!   restart/fail redirects, course inter-stage): dismiss the toast, run
//!   the apply (Step 3), and ALWAYS flip the arm OFF for any non-idle
//!   session — one rule: any song ending while the toggle is ON consumes it
//!   (D16). An armed toggle with no song played survives untouched.
//! - `song_reset` (in-place quick restarts, Training Mode scrubs/loops —
//!   scene 28 never exits) resets the sample accumulator; the session and
//!   toast persist.
//!
//! Fail-open everywhere: a missing service refuses `enable()` with one WARN
//! and the row never registers; the four offset rows are unaffected.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::mods::mod_menu::{self, EnumRowSpec, RowChangeCallback};
use crate::mods::power_user_statistics::data_feed;
use crate::services::song_rate::clock_patch;
use crate::services::{scene_manager, score_guard, song_reset, stage_records, toast};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::compute::{self, CalibStats, CensusOutcome, Outcome};

/// Stable contributed-row key (distinct from any registry mod id).
const ROW_KEY: &str = "timing_calibrate_next";

/// Refusal-toast hold (3 s); the result toast holds 5 s.
const REFUSAL_TOAST_MS: u64 = 3000;
const RESULT_TOAST_MS: u64 = 5000;

/// Canonical timing-offsets field index of SOUND_OFFSET.
const SOUND_IDX: usize = 0;

/// Operator intent: calibrate the next song. Mirrors the overlay row.
static ARMED: AtomicBool = AtomicBool::new(false);

/// The per-song session for the gameplay scene currently running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    /// No calibration song in flight.
    Idle,
    /// Guards passed — the tap is armed for `side`, the pulsing toast is up.
    Collecting { side: u8 },
    /// Armed but refused at entry (2P / rate / unreadable): the song still
    /// consumes the arm at exit, nothing is measured.
    ConsumeOnly,
}

/// Session + callback handles, touched only in scene/reset callbacks (never
/// on the judge hot path).
struct Lifecycle {
    session: Session,
    scene_cb: Option<usize>,
    reset_cb: Option<usize>,
}

static LIFECYCLE: Mutex<Lifecycle> = Mutex::new(Lifecycle {
    session: Session::Idle,
    scene_cb: None,
    reset_cb: None,
});

/// Register the row + callbacks. Called from the mod's `enable()` BEFORE
/// `register_overlay_rows()` so the calibrate row renders at the top of the
/// timing-offsets section (contributed rows render in insertion order).
pub(super) fn enable() {
    if !data_feed::is_installed() {
        log_warn!(
            "TimingOffsets/calibration: judge_submit data feed unavailable -- calibration disabled"
        );
        return;
    }
    if !scene_manager::is_available() {
        log_warn!("TimingOffsets/calibration: scene_manager unavailable -- calibration disabled");
        return;
    }
    if !song_reset::is_available() {
        log_warn!("TimingOffsets/calibration: song_reset unavailable -- calibration disabled");
        return;
    }

    let scene_cb = scene_manager::on_scene_change(Box::new(|prev, next| {
        on_scene_change(prev, next);
    }));
    let reset_cb = song_reset::on_song_reset(|_t_ms| on_song_reset());
    if let Ok(mut lc) = LIFECYCLE.lock() {
        lc.session = Session::Idle;
        lc.scene_cb = Some(scene_cb);
        lc.reset_cb = Some(reset_cb);
    }

    ARMED.store(false, Ordering::Release);
    register_row(0);
    log_info!("TimingOffsets/calibration: enabled (row registered, arm OFF)");
}

/// Tear down: unregister callbacks, remove the row, disarm, dismiss any
/// live pulsing toast. Called from the mod's `disable()`.
pub(super) fn disable() {
    let (scene_cb, reset_cb, was_live) = match LIFECYCLE.lock() {
        Ok(mut lc) => {
            let live = matches!(lc.session, Session::Collecting { .. });
            lc.session = Session::Idle;
            (lc.scene_cb.take(), lc.reset_cb.take(), live)
        }
        Err(_) => (None, None, false),
    };
    if let Some(id) = scene_cb {
        scene_manager::remove_callback(id);
    }
    if let Some(id) = reset_cb {
        song_reset::remove_callback(id);
    }
    if was_live {
        toast::dismiss();
        clear_hides();
    }
    ARMED.store(false, Ordering::Release);
    mod_menu::remove_rows_for(&[ROW_KEY]);
    log_info!("TimingOffsets/calibration: disabled");
}

/// Register (or idempotently re-register) the overlay row with `value`.
/// Re-registration with 0 is also the programmatic flip-OFF mechanism:
/// `register_enum_row` replaces by key, and the tab list rebuilds from the
/// contributed-row store on every menu open.
fn register_row(value: i32) {
    let cb: RowChangeCallback = std::sync::Arc::new(|v| {
        ARMED.store(v != 0, Ordering::Release);
    });
    mod_menu::register_enum_row(EnumRowSpec {
        key: ROW_KEY.to_string(),
        label: "Calibrate next song?".to_string(),
        hint: "Measures your timing next song and adjusts Sound Offset. \
               Play one player and time your steps to the music."
            .to_string(),
        parent_row_key: Some(super::MOD_ID.to_string()),
        values: vec![0, 1],
        labels: vec!["OFF".to_string(), "ON".to_string()],
        initial_value: value,
        on_change: cb,
    });
}

/// Flip the arm OFF (state + row display). The one D16 rule: any song
/// ending while the toggle is ON clears it.
fn flip_off() {
    ARMED.store(false, Ordering::Release);
    register_row(0);
}

/// Scene-change dispatch (runs inside scene_manager's catch_unwind, outside
/// its lock).
fn on_scene_change(prev: i32, next: i32) {
    if next == scene::GAMEPLAY {
        on_gameplay_entry();
    } else if prev == scene::GAMEPLAY {
        on_gameplay_exit();
    }
}

/// GAMEPLAY entry: latch the arm into a session behind the guards.
fn on_gameplay_entry() {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }

    // Guard 1: exactly one entered side.
    let outcome = compute::census(
        stage_records::side_entered(0),
        stage_records::side_entered(1),
    );
    let side = match outcome {
        CensusOutcome::Single { side } => side,
        CensusOutcome::TwoPlayers => {
            log_warn!("calibration: 2P play detected -- calibration disabled for this song");
            toast::flash_with_hold("2P MODE DETECTED, CALIBRATION DISABLED", REFUSAL_TOAST_MS);
            set_session(Session::ConsumeOnly);
            return;
        }
        CensusOutcome::NonePlaying => {
            log_warn!(
                "calibration: entered sides unreadable/none -- calibration disabled for this song"
            );
            set_session(Session::ConsumeOnly);
            return;
        }
    };

    // Guard 2: song rate must be 100% (ms errors are content-domain; the
    // audio latency being calibrated is wall-domain). Re-checked at apply.
    if clock_patch::snapshot().is_non_identity_commit() {
        log_warn!("calibration: song rate != 100% -- calibration disabled for this song");
        toast::flash_with_hold("SONG SPEED ACTIVE, CALIBRATION DISABLED", REFUSAL_TOAST_MS);
        set_session(Session::ConsumeOnly);
        return;
    }

    start_collecting(side);
    set_session(Session::Collecting { side });
    toast::show_pulsing("Calibrating...");

    // D18: hide every judgement-feedback overlay so the player times to the
    // audio instead of chasing the current judgment windows. Fail-open: with
    // the styling mod disabled there is no hide path — calibrate anyway
    // (visible overlays only add noise, not error).
    if !crate::mods::overlay_element_styling::set_calibration_hide(true) {
        log_warn!(
            "calibration: overlay-element-styling inactive -- judgement overlays stay visible"
        );
    }
    // D19: suppress the PUS realtime timing readouts (they leak the signal
    // being calibrated). No-op when that mod is disabled.
    crate::mods::power_user_statistics::set_calibration_suppress(true);

    log_info!(
        "calibration: collecting side {} (arm consumed at song end)",
        side
    );
}

/// GAMEPLAY exit: apply (Step 3) and consume the arm.
fn on_gameplay_exit() {
    let session = match LIFECYCLE.lock() {
        Ok(mut lc) => {
            let s = lc.session;
            lc.session = Session::Idle;
            s
        }
        Err(_) => Session::Idle,
    };
    match session {
        Session::Idle => {}
        Session::ConsumeOnly => {
            flip_off();
        }
        Session::Collecting { side } => {
            toast::dismiss();
            clear_hides();
            finish_collecting(side);
            flip_off();
        }
    }
}

/// Clear the D18 overlay hide and the D19 PUS suppression (Collecting
/// teardown + mod disable).
fn clear_hides() {
    crate::mods::overlay_element_styling::set_calibration_hide(false);
    crate::mods::power_user_statistics::set_calibration_suppress(false);
}

/// Song reset (in-place quick restart / Training Mode scrub or loop): the
/// song re-measures from scratch; the session and pulsing toast persist.
fn on_song_reset() {
    let collecting = LIFECYCLE
        .lock()
        .map(|lc| matches!(lc.session, Session::Collecting { .. }))
        .unwrap_or(false);
    if collecting {
        reset_collection();
        log_info!("calibration: song reset -- sample accumulator cleared");
    }
}

fn set_session(session: Session) {
    if let Ok(mut lc) = LIFECYCLE.lock() {
        lc.session = session;
    }
}

// ── Measurement + apply ─────────────────────────────────────────────────

/// Arm the per-step sample tap for `side`.
fn start_collecting(side: u8) {
    data_feed::calibration_arm(side as usize);
}

/// Clear the sample accumulator mid-song (in-place restart / scrub).
fn reset_collection() {
    data_feed::calibration_reset();
}

/// Take the samples and run the apply pipeline: autoplay guard → rate
/// re-check → `compute` → `set_offset(SOUND)` + toasts + logs. Runs in the
/// scene callback (catch_unwind context provided by scene_manager); every
/// branch is a WARN/INFO + toast, never a panic.
fn finish_collecting(side: u8) {
    let (sum, sum_sq, count) = data_feed::calibration_take();
    let stats = CalibStats { sum, sum_sq, count };

    // Belt-and-braces rate re-check (also guarded at entry): a non-identity
    // rate makes the content-domain samples meaningless.
    if clock_patch::snapshot().is_non_identity_commit() {
        log_warn!("calibration: song rate went non-identity mid-flight -- discarded");
        toast::flash_with_hold("SONG SPEED ACTIVE, CALIBRATION DISABLED", REFUSAL_TOAST_MS);
        return;
    }

    // Autoplay ALONE invalidates the measurement (machine-timed ~0 ms
    // steps). Deliberately NOT `is_stage_suppressed`: that ORs in the
    // quick-fail / training / assist-tick / rate score taints, whose steps
    // are humanly real — a quick-exited calibration song measures honestly
    // (cabinet-observed misattribution, first deploy 2026-08-26).
    let autoplay = score_guard::is_autoplay_tainted(side as usize);
    let old = super::get_offset(SOUND_IDX);
    match compute::compute(&stats, old, autoplay) {
        Outcome::Apply {
            new_offset,
            mean,
            stddev,
        } => {
            super::set_offset(SOUND_IDX, new_offset);
            // Sync the overlay SOUND OFFSET row's DISPLAYED value — the row
            // store only updates through menu edits, and a stale display
            // would both mislead the operator and let a later edit step from
            // the pre-calibration value.
            super::refresh_overlay_row(SOUND_IDX);
            let delta = new_offset - old;
            log_info!(
                "calibration: old={} mean={:+.1} count={} stddev={:.1} -> new={}",
                old,
                mean,
                count,
                stddev,
                new_offset
            );
            toast::flash_with_hold(
                format!("CALIBRATED: {old} -> {new_offset} ({delta:+} MS)"),
                RESULT_TOAST_MS,
            );
        }
        Outcome::RefuseAutoplay => {
            log_warn!("calibration: side {} autoplay-tainted -- discarded", side);
            toast::flash_with_hold("AUTOPLAY ACTIVE, CALIBRATION DISCARDED", REFUSAL_TOAST_MS);
        }
        Outcome::RefuseTooFewSamples { count } => {
            log_warn!(
                "calibration: only {} valid samples (need {}) -- not applied",
                count,
                compute::MIN_SAMPLES
            );
            toast::flash_with_hold("CALIBRATION FAILED: NOT ENOUGH STEPS", REFUSAL_TOAST_MS);
        }
        Outcome::RefuseMeanOutOfRange { mean } => {
            log_warn!(
                "calibration: mean {:+.1} ms beyond credibility bound ({} ms) -- not applied",
                mean,
                compute::MAX_ABS_MEAN_MS
            );
            toast::flash_with_hold(
                "CALIBRATION FAILED: TIMING TOO INCONSISTENT",
                REFUSAL_TOAST_MS,
            );
        }
    }
}
