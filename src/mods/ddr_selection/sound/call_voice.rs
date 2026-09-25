//! A3's in-game announcer and crowd for the legacy skins, on World's
//! `CallVoiceActor` (the override of `services::call_voice_hooks`).
//!
//! While a legacy skin is armed and the era bank holds every cue
//! [`rules::all_cues`] names, each gameplay frame of World's actor runs A3's
//! rules ([`rules::step`]) over the fields World's own `onMessage` keeps
//! filling, instead of World's `onUpdate`: skin 1 `ACT6` / `2nd_BIG2`, skins
//! 2–3 `sn2_dgm*` + `2nd_KANSEI_B` / `STG_APP03`, skins 4–5 A3's own
//! `vo_ingame_*` announcer + `STG_APP02`. Every play goes through the game's
//! `se_play` façade into the era bank's slot (the game's mute filter + AVS
//! lock, the handle registered in the same cue table World's guard reads);
//! guarded plays use World's own is-playing check. Otherwise (disarmed, bank
//! missing, a cue missing, an unreadable actor) World's announcer runs.
//! announcer_mute's predicate still silences everything first.
//!
//! Game thread (the actor's update), no allocation, no lock; one INFO per
//! new actor. RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! announcer-crowd.md`.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use crate::core::memory;
use crate::services::{call_voice_hooks, game_audio};
use crate::{log_info, log_warn};

use super::{bank, rules};

// A3's field layout (checked instruction by instruction by
// `derive_call_voice` on every build).
const STEP_STATE: usize = 0x58;
const STEP_INDEX: usize = 0x82;
const STEP_COUNT: usize = 5;
const HANDLE: usize = 0x84;
const TIME: usize = 0x88;
const TIME2: usize = 0x8C;
const NEXT_STATE: usize = 0x90;
const NEXT_CROWD: usize = 0x94;
const MILESTONE: usize = 0x98;
const VOICE_OFF: usize = 0x9C;
const SE_OFF: usize = 0x9D;
const WAS_LOW: usize = 0x9E;
/// Per side (`+ side * 0xC`): gauge f32, combo, difficulty.
const SIDE: usize = 0xA0;
const SIDE_STRIDE: usize = 0xC;
const FIELDS_END: usize = 0xBC;

/// 0 unchecked, 1 every rule cue in the bank, 2 some missing.
static CUES: AtomicU8 = AtomicU8::new(0);
static LAST_ACTOR: AtomicUsize = AtomicUsize::new(0);

/// Register the override (mod init). `false` when the announcer hook is
/// unavailable (World's announcer on every skin).
pub fn init() -> bool {
    if !call_voice_hooks::is_available() {
        log_warn!("DDR SELECTION: announcer hook unavailable -- World's announcer on every skin");
        return false;
    }
    call_voice_hooks::set_override(Some(on_update));
    if !call_voice_hooks::acquire() {
        call_voice_hooks::set_override(None);
        log_warn!("DDR SELECTION: announcer detour failed -- World's announcer on every skin");
        return false;
    }
    log_info!("DDR SELECTION: A3 announcer / crowd rules ready");
    true
}

/// Remove the override (disable).
pub fn shutdown() {
    call_voice_hooks::set_override(None);
    LAST_ACTOR.store(0, Ordering::Relaxed);
}

fn cues_ready() -> bool {
    match CUES.load(Ordering::Acquire) {
        1 => true,
        2 => false,
        _ => {
            let missing = rules::all_cues()
                .into_iter()
                .find(|c| bank::lookup(c.trim_end_matches('\0').as_bytes()).is_none());
            match missing {
                None => {
                    CUES.store(1, Ordering::Release);
                    true
                }
                Some(c) => {
                    log_warn!(
                        "DDR SELECTION: era bank lacks announcer cue {} -- World's announcer on legacy skins",
                        c.trim_end_matches('\0')
                    );
                    CUES.store(2, Ordering::Release);
                    false
                }
            }
        }
    }
}

unsafe fn rd_i32(a: *mut u8, off: usize) -> i32 {
    (a.add(off) as *const i32).read_unaligned()
}

unsafe fn wr_i32(a: *mut u8, off: usize, v: i32) {
    (a.add(off) as *mut i32).write_unaligned(v)
}

fn play(slot: i32, name: &'static str) -> u32 {
    // `name` is NUL-terminated (rules' contract).
    unsafe { game_audio::se_play_from_sound_callback(slot, name.as_ptr().cast(), 0.0) }
        .unwrap_or(u32::MAX)
}

fn on_update(actor: *mut u8) -> bool {
    let skin = super::super::armed_skin();
    if skin == 0 {
        return false;
    }
    let Some(slot) = bank::slot() else {
        return false;
    };
    if !cues_ready() || !memory::is_readable(actor, FIELDS_END) {
        return false;
    }
    unsafe {
        let idx = (actor.add(STEP_INDEX) as *const u16).read_unaligned() as usize;
        if idx >= STEP_COUNT {
            return false;
        }
        let state = rd_i32(actor, STEP_STATE + idx * 8);
        if state == 0 || state == 3 {
            return true; // World's (and A3's) early return
        }
        let side = |s: usize, k: usize| actor.add(SIDE + s * SIDE_STRIDE + k * 4);
        let f = rules::Fields {
            gauge: [
                (side(0, 0) as *const f32).read_unaligned(),
                (side(1, 0) as *const f32).read_unaligned(),
            ],
            combo: [
                (side(0, 1) as *const i32).read_unaligned(),
                (side(1, 1) as *const i32).read_unaligned(),
            ],
            difficulty: [
                (side(0, 2) as *const i32).read_unaligned(),
                (side(1, 2) as *const i32).read_unaligned(),
            ],
            time: rd_i32(actor, TIME),
            time2: rd_i32(actor, TIME2),
            next_state: rd_i32(actor, NEXT_STATE),
            next_crowd: rd_i32(actor, NEXT_CROWD),
            milestone: rd_i32(actor, MILESTONE),
            voice_off: *actor.add(VOICE_OFF) != 0,
            se_off: *actor.add(SE_OFF) != 0,
            was_low: *actor.add(WAS_LOW) != 0,
        };
        let Some(s) = rules::step(&f, skin) else {
            return false;
        };
        if LAST_ACTOR.swap(actor as usize, Ordering::Relaxed) != actor as usize {
            log_info!(
                "DDR SELECTION: A3 announcer / crowd for skin {} ({})",
                skin,
                super::super::policy::skin_name(skin).unwrap_or("?")
            );
        }
        wr_i32(actor, MILESTONE, s.milestone);
        wr_i32(actor, NEXT_STATE, s.next_state);
        wr_i32(actor, NEXT_CROWD, s.next_crowd);
        *actor.add(WAS_LOW) = u8::from(s.was_low);
        for p in s.plays.iter().flatten() {
            match *p {
                rules::Play::Voice(n) => wr_i32(actor, HANDLE, play(slot, n) as i32),
                rules::Play::Guarded(n) => {
                    let h = rd_i32(actor, HANDLE) as u32;
                    if !call_voice_hooks::cue_is_playing(h) {
                        wr_i32(actor, HANDLE, play(slot, n) as i32);
                    }
                }
                rules::Play::Se(n) => {
                    let _ = play(slot, n);
                }
            }
        }
    }
    true
}
