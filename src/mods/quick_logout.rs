//! Quick Logout Mod — triple-press numpad 9 at song select to end the session.
//!
//! A triple-press of numpad **9** on either pinpad during plain music
//! selection (0-idx scene 25) immediately runs the game's own end-of-session
//! tail: TOTAL RESULTS → e-amusement logout save → THANK YOU FOR PLAYING →
//! attract. No confirmation, no on-screen UI, no configuration — the player no
//! longer has to play out the remaining stages to end a session (the
//! motivating case being Premium Free, whose frozen stage counter otherwise
//! keeps the session alive indefinitely). Either side may trigger; it ends the
//! session for both.
//!
//! ## Mechanism (one function call, zero new detours)
//!
//! The engine drives every scene transition with a single primitive,
//! `agcs::Sequence::finish(this, nextSceneId_1INDEXED)`, and nothing in the
//! end-of-session tail checks whether the session ended legitimately. The
//! trigger arms a one-shot scene redirect `30 → 32` (0-indexed), then calls
//! `finish(active_child, 30₁ᵢₙdₑₓ)` — the 0-idx **29 loader**, the only loader
//! that loads the `scene_result` BM2D package TOTAL RESULTS dereferences
//! without a null check. Resulting chain: 29 loader → (redirect) 32 TOTAL
//! RESULTS → 33 loader → 34 `EAmExitRootSequence` (credit expire + logout
//! save) → 35 THANK YOU → attract.
//!
//! Two invariants (see `docs/quick_logout_research.md`):
//! - **Never jump directly to scene 32** — the package is not resident at song
//!   select; the 29-loader hop is the entire mechanism.
//! - **Never close the shutter before triggering** — TOTAL RESULTS' only exit
//!   gate waits for shutter==closed via its OWN close request, which works
//!   only because the shutter is open on entry.
//!
//! The redirect relies on scene_manager's `advance_to_scene` m_currentID
//! repair: without it, the tail after TOTAL RESULTS would run
//! `getNextID(0x1F) = 0x20` (the stage-bump Wait sequence back to song select)
//! instead of the logout — hence the `redirect_repair_available()` enable
//! gate.
//!
//! ## Indexing convention (load-bearing)
//!
//! `finish` takes a **1-indexed** scene id; `scene_manager`, `types::scenes`,
//! and every log line are **0-indexed**. The only 1-indexed constant is
//! `POST_SONG_LOADER_1IDX = 30` (= 0-idx 29).
//!
//! Quick Logout itself writes no taint and no game state. Tainted sides'
//! logout saves are handled by the logout-save sanitiser in
//! `custom_options_persistence` (score-stripped, profile forwarded).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{input_manager, scene_manager, score_guard, stage_records};
use crate::types::buttons::*;
use crate::types::scenes::{get_scene_name, scene};
use crate::{log_info, log_warn};

const GESTURE_WINDOW: Duration = Duration::from_millis(1500);
const GESTURE_COUNT: usize = 3;

/// Offset of the active gosub-child slot on the TransitionSequence.
/// `*(ts + ACTIVE_CHILD_OFFSET)` is the running scene sequence (the
/// MusicSelectSequence when the scene is SONG_SELECT).
const ACTIVE_CHILD_OFFSET: usize = 0x58;

/// Offset of the actor tree-flags dword. Mask 0x24 = dying/destroyed —
/// a child with either bit set must not be `finish`ed.
const TREE_FLAGS_OFFSET: usize = 0x20;
const TREE_FLAGS_DEAD_MASK: u32 = 0x24;

/// Side-entered byte inside PlayerWork (non-zero once the side has joined).
const PLAYER_WORK_ENTERED_OFFSET: usize = 0x4;

/// The value passed to `sequence_finish` — **1-indexed** scene 30 = the
/// 0-indexed 29 post-song LoadingSequence (loads `scene_result`).
const POST_SONG_LOADER_1IDX: i32 = 30;

/// Diagnostic threshold: scene 34 exiting faster than this suggests the
/// logout save was no-oped (assumption A1 failure mode).
const EAM_EXIT_MIN_DWELL: Duration = Duration::from_millis(500);

/// `agcs::Sequence::finish(this, nextSceneId_1INDEXED)`.
type SequenceFinishFn = unsafe extern "C" fn(*mut u8, i32);

/// Resolved `sequence_finish` fn ptr (null until init).
static SEQUENCE_FINISH: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Per-session trigger latch. Set at trigger; cleared on the next transition
/// into SONG_SELECT (new session or aborted chain — either way, re-arm).
static FIRED: AtomicBool = AtomicBool::new(false);
/// One-time WARN latch for the degraded session gate (stage_records down).
static SESSION_GATE_WARNED: AtomicBool = AtomicBool::new(false);
/// Diagnostics epoch: when the trigger fired.
static TRIGGER_AT: Mutex<Option<Instant>> = Mutex::new(None);
/// Tail diagnostics: when 0-idx scene 34 (EAmExit) was entered, if seen.
static SEEN_34_AT: Mutex<Option<Instant>> = Mutex::new(None);

#[derive(Default)]
struct GestureBuffer {
    presses: VecDeque<Instant>,
}

impl GestureBuffer {
    fn record(&mut self) -> bool {
        let now = Instant::now();
        self.presses.push_back(now);
        while let Some(front) = self.presses.front() {
            if now.duration_since(*front) > GESTURE_WINDOW {
                self.presses.pop_front();
            } else {
                break;
            }
        }
        if self.presses.len() >= GESTURE_COUNT {
            self.presses.clear();
            true
        } else {
            false
        }
    }

    fn clear(&mut self) {
        self.presses.clear();
    }
}

/// Per-side triple-9 buffers (`[P1, P2]`).
static GESTURE: Lazy<Mutex<[GestureBuffer; 2]>> =
    Lazy::new(|| Mutex::new([GestureBuffer::default(), GestureBuffer::default()]));

fn clear_gestures() {
    if let Ok(mut buffers) = GESTURE.lock() {
        for b in buffers.iter_mut() {
            b.clear();
        }
    }
}

pub struct QuickLogoutMod {
    input_cb: Option<usize>,
    scene_cb: Option<usize>,
}

impl QuickLogoutMod {
    pub fn new() -> Self {
        Self {
            input_cb: None,
            scene_cb: None,
        }
    }
}

impl Mod for QuickLogoutMod {
    fn id(&self) -> &str {
        "quick-logout"
    }
    fn name(&self) -> &str {
        "Quick Logout"
    }
    fn description(&self) -> &str {
        "Triple-press 9 at song select to end the session (results + save + logout)"
    }
    fn required_signatures(&self) -> &[&str] {
        &["sequence_finish", "player_work_table"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let addr = ctx.signatures.require_address("sequence_finish");
        SEQUENCE_FINISH.store(addr as *mut u8, Ordering::Release);
        true
    }

    fn enable(&mut self) {
        // The 30→32 redirect hands control to the game's automatic tail,
        // which reads m_currentID. Without the advance_to_scene repair a
        // redirect leaves the stale pre-redirect id and TOTAL RESULTS would
        // exit into the stage-bump path back to song select instead of the
        // logout. Refuse to enable rather than mis-route.
        if !scene_manager::is_available() || !scene_manager::redirect_repair_available() {
            log_warn!(
                "QuickLogout: scene_manager or its m_currentID redirect repair unavailable -- refusing to enable"
            );
            return;
        }
        if !input_manager::is_available() {
            log_warn!("QuickLogout: input_manager unavailable -- gesture inactive");
            return;
        }

        let id = input_manager::on_input_event(Arc::new(|event: &InputEvent| {
            on_input_event(event);
        }));
        self.input_cb = Some(id);

        let id = scene_manager::on_scene_change(Box::new(on_scene_change));
        self.scene_cb = Some(id);

        log_info!("QuickLogout: enabled (triple-9 at song select ends the session)");
    }

    fn disable(&mut self) {
        if let Some(id) = self.input_cb.take() {
            input_manager::remove_callback(id);
        }
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        clear_gestures();
        FIRED.store(false, Ordering::Release);
        log_info!("QuickLogout: disabled");
    }
}

/// Input callback (frame thread, panic-free).
fn on_input_event(event: &InputEvent) {
    if event.event_type != InputEventType::Pressed || event.button != button::NUM_9 {
        return;
    }
    let side = match event.player {
        Player::P1 => 0usize,
        Player::P2 => 1usize,
    };

    // Gesture presses only count at plain music selection. Leaving the scene
    // clears the side's buffer so a stale press can't combine with a later
    // one. (The matching/battle song-select variants are different scene ids,
    // so the gesture is inert there by construction.)
    if scene_manager::current_scene() != scene::SONG_SELECT {
        if let Ok(mut buffers) = GESTURE.lock() {
            if let Some(b) = buffers.get_mut(side) {
                b.clear();
            }
        }
        return;
    }

    let triggered = {
        let Ok(mut buffers) = GESTURE.lock() else {
            return;
        };
        match buffers.get_mut(side) {
            Some(b) => b.record(),
            None => false,
        }
    };

    if triggered {
        try_trigger(side);
    }
}

/// Session gate: at least one side has entered (`PlayerWork+0x4 != 0`).
/// Returns the per-side entered flags, or `None` when stage_records is
/// unavailable (degrade to scene-gate-only with a one-time WARN).
fn entered_sides() -> Option<[bool; 2]> {
    if !stage_records::is_available() {
        if !SESSION_GATE_WARNED.swap(true, Ordering::AcqRel) {
            log_warn!(
                "QuickLogout: stage_records unavailable -- session gate degraded to scene gate only"
            );
        }
        return None;
    }
    let mut entered = [false; 2];
    for (side, flag) in entered.iter_mut().enumerate() {
        if let Some(work) = stage_records::player_work(side) {
            *flag = unsafe { memory::read_u8(work.add(PLAYER_WORK_ENTERED_OFFSET)) } != 0;
        }
    }
    Some(entered)
}

/// The trigger. Four gates (FIRED latch, session gate, live transition
/// sequence, live child), then: arm the one-shot 30→32 redirect and
/// `finish(child, 30₁ᵢₙdₑₓ)`. `finish` is synchronous — our scene hook runs
/// re-entrantly during the call (verified deadlock-free: the input manager
/// dispatches callbacks outside its lock) — so NO lock may be held across it.
fn try_trigger(trigger_side: usize) {
    // Gate 1: per-session latch.
    if FIRED.load(Ordering::Acquire) {
        return;
    }

    // Gate 2: session gate — at least one side entered. When stage_records is
    // down this degrades to "pass" (the scene gate alone still holds; scene 25
    // is unreachable without a session).
    let entered = entered_sides();
    if let Some(entered) = entered {
        if !entered[0] && !entered[1] {
            return;
        }
    }

    let finish_addr = SEQUENCE_FINISH.load(Ordering::Acquire);
    if finish_addr.is_null() {
        return;
    }

    // Gate 3: a live TransitionSequence.
    let Some(ts) = scene_manager::current_transition_sequence() else {
        log_warn!("QuickLogout: no TransitionSequence captured -- trigger ignored");
        return;
    };

    // Gate 4: a live, not-dying active child (the MusicSelectSequence).
    let (child, flags) = unsafe {
        let child = memory::read_ptr(ts.add(ACTIVE_CHILD_OFFSET)) as *mut u8;
        if child.is_null() {
            (child, 0u32)
        } else {
            (child, memory::read_u32(child.add(TREE_FLAGS_OFFSET)))
        }
    };
    if child.is_null() {
        log_warn!("QuickLogout: TransitionSequence has no active child -- trigger ignored");
        return;
    }
    if flags & TREE_FLAGS_DEAD_MASK != 0 {
        log_warn!(
            "QuickLogout: active child is dying (flags 0x{:X}) -- trigger ignored",
            flags
        );
        return;
    }

    // Trigger context: entered sides + per-side taint, so a "this side's
    // logout save will be sanitised/suppressed" situation is visible in the
    // log at the moment of trigger.
    let describe = |side: usize| -> &'static str {
        let entered = entered.map(|e| e[side]).unwrap_or(true);
        match (entered, score_guard::logout_taint(side)) {
            (false, _) => "not entered",
            (true, false) => "entered, clean",
            (true, true) => "entered, TAINTED",
        }
    };
    log_info!(
        "QuickLogout: triggered by P{} at song select -- P1: {}; P2: {}",
        trigger_side + 1,
        describe(0),
        describe(1)
    );

    // Arm diagnostics BEFORE the finish call so the re-entrant scene hook
    // logs the very first hop (25 -> 29 loader) as part of the tail. Leaf
    // locks only; both released before finish.
    if let Ok(mut t) = TRIGGER_AT.lock() {
        *t = Some(Instant::now());
    }
    if let Ok(mut s) = SEEN_34_AT.lock() {
        *s = None;
    }
    FIRED.store(true, Ordering::Release);

    // One-shot redirect: the loader's natural successor 30 (RESULTS_DETAIL)
    // becomes 32 (TOTAL RESULTS). 0-indexed, like all scene_manager ids.
    scene_manager::add_redirect_once(scene::RESULTS_DETAIL, scene::FINAL_RESULTS);

    // finish(child, 30₁ᵢₙdₑₓ) — 1-INDEXED: this is the 0-idx 29 post-song
    // loader, whose unconditional load mask makes `scene_result` resident for
    // TOTAL RESULTS. Synchronous; frees nothing (the reaper runs next frame).
    let finish: SequenceFinishFn = unsafe { std::mem::transmute(finish_addr) };
    unsafe { finish(child, POST_SONG_LOADER_1IDX) };
}

/// Scene callback: tail diagnostics while FIRED + latch reset at song select.
fn on_scene_change(prev: i32, next: i32) {
    if !FIRED.load(Ordering::Acquire) {
        return;
    }

    let elapsed_ms = TRIGGER_AT
        .lock()
        .ok()
        .and_then(|t| *t)
        .map(|t| t.elapsed().as_millis())
        .unwrap_or(0);
    log_info!(
        "QuickLogout: tail {} ({}) -> {} ({}) (+{} ms)",
        prev,
        get_scene_name(prev),
        next,
        get_scene_name(next),
        elapsed_ms
    );

    if next == scene::EAM_EXIT {
        if let Ok(mut s) = SEEN_34_AT.lock() {
            *s = Some(Instant::now());
        }
    }

    if next == scene::THANK_YOU {
        // The two FR4 diagnostics for assumption A1 (does a forced
        // EAmExitRootSequence actually perform the logout save?).
        let seen_34 = SEEN_34_AT.lock().ok().and_then(|s| *s);
        match seen_34 {
            None => log_warn!(
                "QuickLogout: scene 34 (EAM_EXIT) never appeared before THANK_YOU -- logout save skipped (eam offline or ark entry-flow failure)"
            ),
            Some(at) => {
                let dwell = at.elapsed();
                if dwell < EAM_EXIT_MIN_DWELL {
                    log_warn!(
                        "QuickLogout: EAM_EXIT exited suspiciously fast ({} ms) -- verify the logout save reached the backend",
                        dwell.as_millis()
                    );
                }
            }
        }
    }

    // Back at song select: new session or aborted chain — either way, re-arm.
    if next == scene::SONG_SELECT {
        FIRED.store(false, Ordering::Release);
        clear_gestures();
        if let Ok(mut t) = TRIGGER_AT.lock() {
            *t = None;
        }
        if let Ok(mut s) = SEEN_34_AT.lock() {
            *s = None;
        }
    }
}
