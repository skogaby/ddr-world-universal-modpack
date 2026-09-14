//! Dev-mode self-test: arm the bot controller on the HUMAN's own entered
//! side(s) so the cabinet proves the load-bearing controller fact — the judge
//! grades a DLL-owned `IFootPanel` exactly where the planner's `event_mc`
//! says — before player entry is ever touched. Also the build-portability
//! probe for old builds (design §7.3 item 10).
//!
//! Gate: `layeredfs.developer_mode` in `mod-config.json` AND the environment
//! variable `DDR_BOT_SELF_TEST=<level 1..=10>`, both read once at `enable`.
//! Never active on a player's cabinet. Mutually exclusive per side with the
//! impersonation (plan Step 4): a side already driven by a bot is left alone.
//!
//! Lifecycle (scene callback, 0-indexed scenes): entering GAMEPLAY arms every
//! entered side (`filler::start_song` + `foot_panel_swap::arm_bot` + the
//! autoplay score taint); leaving {GAMEPLAY, STAGE_RESULT, RESULTS_DETAIL}
//! disarms and logs the tally; an in-place `song_reset` re-rolls the seed.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::{filler, skill};
use crate::services::{foot_panel_swap, score_guard, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// The armed self-test level (0 = inactive).
static LEVEL: AtomicU8 = AtomicU8::new(0);
/// Which sides THIS module armed (so it never disarms someone else's bot).
static ARMED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

#[link(name = "kernel32")]
extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
}

/// `QueryPerformanceCounter` as a seed source (0 on failure). Shared with
/// `impersonation.rs`.
pub(super) fn qpc() -> u64 {
    let mut v = 0i64;
    if unsafe { QueryPerformanceCounter(&mut v) } == 0 {
        0
    } else {
        v as u64
    }
}

/// Read the gate once. Returns the armed level.
pub fn configure() -> u8 {
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    let level = match std::env::var("DDR_BOT_SELF_TEST") {
        Ok(v) => v.trim().parse::<u8>().unwrap_or(0),
        Err(_) => 0,
    };
    let armed = if dev_mode && (1..=10).contains(&level) {
        level
    } else {
        if level != 0 && !dev_mode {
            log_warn!(
                "MultiplayerBot: DDR_BOT_SELF_TEST set but layeredfs.developer_mode is off -- self-test ignored"
            );
        }
        0
    };
    LEVEL.store(armed, Ordering::Release);
    if armed != 0 {
        let c = skill::curve(armed);
        log_info!(
            "MultiplayerBot: SELF-TEST armed at LV{} (sigma={:.1}ms p_miss={:.2}%) -- the bot will play EVERY entered side's own lane",
            armed,
            c.sigma_ms,
            c.p_miss * 100.0
        );
    }
    armed
}

pub fn is_active() -> bool {
    LEVEL.load(Ordering::Acquire) != 0
}

/// Scene-change hook (called from the mod's single scene callback).
pub fn on_scene_change(_prev: i32, next: i32) {
    let level = LEVEL.load(Ordering::Acquire);
    if level == 0 {
        return;
    }
    if next == scene::GAMEPLAY {
        for side in 0..2 {
            if stage_records::side_entered(side) != Some(true) {
                continue;
            }
            if foot_panel_swap::controller(side) == foot_panel_swap::Controller::Bot
                && !ARMED[side].load(Ordering::Acquire)
            {
                continue; // someone else's bot (impersonation) — leave it
            }
            arm(side, level);
        }
        return;
    }
    let in_window = matches!(
        next,
        scene::GAMEPLAY | scene::STAGE_RESULT | scene::RESULTS_DETAIL
    );
    if !in_window {
        for side in 0..2 {
            if ARMED[side].load(Ordering::Acquire) {
                disarm(side);
            }
        }
    }
}

fn arm(side: usize, level: u8) {
    let seed = skill::seed(qpc(), 0, 0, level);
    filler::start_song(side, level, seed);
    if !foot_panel_swap::arm_bot(side, filler::fill) {
        log_warn!(
            "MultiplayerBot: SELF-TEST could not arm the bot controller on side {} (bot objects unavailable?)",
            side
        );
        filler::reset(side);
        return;
    }
    score_guard::set_autoplay_taint(side, true);
    ARMED[side].store(true, Ordering::Release);
    log_info!(
        "MultiplayerBot: SELF-TEST bot armed on side {} at LV{}",
        side,
        level
    );
}

fn disarm(side: usize) {
    if let Some(s) = filler::summary(side) {
        log_info!(
            "MultiplayerBot: SELF-TEST tally side={} LV{} seed={:#x} planned marv={} perf={} great={} good={} miss={} | judged marv={} perf={} great={} good={} miss={} other={} | mismatch={} frames={}",
            side,
            s.level,
            s.seed,
            s.planned[0],
            s.planned[1],
            s.planned[2],
            s.planned[3],
            s.planned[5],
            s.judged[0],
            s.judged[1],
            s.judged[2],
            s.judged[3],
            s.judged[5],
            s.judged[4],
            s.mismatches,
            s.frames
        );
    }
    foot_panel_swap::disarm_bot(side);
    score_guard::set_autoplay_taint(side, false);
    filler::reset(side);
    ARMED[side].store(false, Ordering::Release);
}

/// `song_reset` subscriber: an in-place restart rebuilds the Results, so the
/// bot re-rolls with a fresh seed on every armed side.
pub fn on_song_reset(_t_ms: i32) {
    let level = LEVEL.load(Ordering::Acquire);
    if level == 0 {
        return;
    }
    for side in 0..2 {
        if ARMED[side].load(Ordering::Acquire) {
            filler::start_song(side, level, skill::seed(qpc(), 0, 0, level));
            log_info!(
                "MultiplayerBot: SELF-TEST re-rolled side {} after song reset",
                side
            );
        }
    }
}

/// Disarm everything (mod disable).
pub fn shutdown() {
    for side in 0..2 {
        if ARMED[side].load(Ordering::Acquire) {
            disarm(side);
        }
    }
    LEVEL.store(0, Ordering::Release);
}
