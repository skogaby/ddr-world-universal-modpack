//! Classic Difficulty Adjustment — dance-pad double-taps change difficulty
//! at song select, like every DDR release before World.
//!
//! DDR World moved the on-wheel difficulty change to pinpad keys (2 lowers,
//! 5 raises) and dropped the classic double-tap-UP/-DOWN pad gesture. This
//! mod restores it by translation: at plain music selection (0-idx scene
//! 25), a double-tap of a side's UP panel within [`DOUBLE_TAP_WINDOW_MS`]
//! synthesizes that side's pinpad **5** (difficulty up); a double-tap of
//! DOWN synthesizes pinpad **2** (difficulty down). The game's own pinpad
//! handler does the rest — no menu state is touched directly.
//!
//! ## Mechanism (zero new detours, zero signatures)
//!
//! - **Panel reads:** `input_manager`'s opt-in stage-panel polling
//!   (`set_panel_polling`) reports the pad panels as `button::PANEL_*`
//!   InputEvents via the stable `arkMDXGetPanel*` exports.
//! - **Pinpad synthesis:** `input_manager::request_pinpad_pulse` — a
//!   ~120 ms one-shot OR'd into the ark's 10-key vtable impl (the single
//!   funnel every pinpad consumer reads through), the same seam the SMX
//!   touch overlay's pinpad rides. `request_pinpad_injection` at enable
//!   arms the lazy vtable-detour install even when the SMX mod is off.
//!
//! Scene-gated in the handler (leaving scene 25 clears the tap state, so a
//! stale tap can't pair with a later one); tap state also self-expires via
//! the window. Both sides are independent. Single taps pass through to
//! whatever the game natively does with them — only the *second* tap of a
//! pair adds the pinpad pulse.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use once_cell::sync::Lazy;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{input_manager, scene_manager};
use crate::types::buttons::*;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// Max gap between the two taps of a pair.
const DOUBLE_TAP_WINDOW_MS: u64 = 400;

/// 10-key buffer indices the game maps to difficulty changes at the wheel.
const PINPAD_DIFFICULTY_UP: usize = 5;
const PINPAD_DIFFICULTY_DOWN: usize = 2;

/// Millisecond epoch for tap timestamps (Instant isn't atomic-friendly).
static TAP_EPOCH: Lazy<Instant> = Lazy::new(Instant::now);

fn now_ms() -> u64 {
    // +1 so a press at t=0 is distinguishable from the "no tap" sentinel 0.
    TAP_EPOCH.elapsed().as_millis() as u64 + 1
}

/// Last press timestamp per `[side][direction]` (0 = UP, 1 = DOWN);
/// 0 = no pending first tap.
static LAST_TAP_MS: [[AtomicU64; 2]; 2] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const Z: AtomicU64 = AtomicU64::new(0);
    [[Z; 2], [Z; 2]]
};

fn clear_taps(side: usize) {
    for dir in &LAST_TAP_MS[side] {
        dir.store(0, Ordering::Release);
    }
}

pub struct ClassicDifficultyMod {
    input_cb: Option<usize>,
}

impl ClassicDifficultyMod {
    pub fn new() -> Self {
        Self { input_cb: None }
    }
}

impl Mod for ClassicDifficultyMod {
    fn id(&self) -> &str {
        "classic-difficulty-adjustment"
    }
    fn name(&self) -> &str {
        "Classic Difficulty Adjustment"
    }
    fn description(&self) -> &str {
        "Double-tap pad UP/DOWN at song select to raise/lower difficulty (pre-World behavior)"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        if !input_manager::is_available() {
            log_warn!("ClassicDifficulty: input_manager unavailable -- gesture inactive");
            return;
        }
        // Arm the lazy 10-key vtable-detour install (pinpad pulses need it)
        // and turn on stage-panel polling (PANEL_* events).
        input_manager::request_pinpad_injection();
        input_manager::set_panel_polling(true);

        let id = input_manager::on_input_event(Arc::new(|event: &InputEvent| {
            on_input_event(event);
        }));
        self.input_cb = Some(id);
        log_info!("ClassicDifficulty: enabled (double-tap pad UP/DOWN at song select)");
    }

    fn disable(&mut self) {
        if let Some(id) = self.input_cb.take() {
            input_manager::remove_callback(id);
        }
        // Sole panel-events consumer today (see set_panel_polling's doc).
        input_manager::set_panel_polling(false);
        input_manager::clear_pinpad_pulses();
        clear_taps(0);
        clear_taps(1);
        log_info!("ClassicDifficulty: disabled");
    }
}

/// Input callback (render/game thread, panic-free, O(1)).
fn on_input_event(event: &InputEvent) {
    if event.event_type != InputEventType::Pressed {
        return;
    }
    let (dir, pinpad_key) = match event.button {
        button::PANEL_UP => (0usize, PINPAD_DIFFICULTY_UP),
        button::PANEL_DOWN => (1usize, PINPAD_DIFFICULTY_DOWN),
        _ => return,
    };
    let side = match event.player {
        Player::P1 => 0usize,
        Player::P2 => 1usize,
    };

    // Taps only pair at plain music selection; leaving the scene clears the
    // side's pending taps so a stale press can't combine with a later one.
    if scene_manager::current_scene() != scene::SONG_SELECT {
        clear_taps(side);
        return;
    }

    let now = now_ms();
    let prev = LAST_TAP_MS[side][dir].swap(now, Ordering::AcqRel);
    if prev != 0 && now.saturating_sub(prev) <= DOUBLE_TAP_WINDOW_MS {
        // Pair complete: consume it (a third tap starts a fresh pair
        // instead of chaining) and synthesize the pinpad press.
        LAST_TAP_MS[side][dir].store(0, Ordering::Release);
        input_manager::request_pinpad_pulse(side, pinpad_key);
        log_info!(
            "ClassicDifficulty: P{} double-tap {} -> pinpad {}",
            side + 1,
            if dir == 0 { "UP" } else { "DOWN" },
            pinpad_key
        );
    }
}
