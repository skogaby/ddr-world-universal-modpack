//! DDR SELECTION era sounds: the mod-owned `dsel` XACT bank (every A3 cue a
//! legacy skin plays, copied out of the A3-generation `_n` banks World ships
//! but never loads) and the routing that makes the legacy AFP clips' embedded
//! sounds play from it.
//!
//! * [`cues`] (pure) — the manifest.
//! * [`bank_build`] (pure) — subset + merge of the source XSB/XWB pairs.
//! * [`bank`] — background build at enable, registration into manager slot 4
//!   on the game thread, lock-free name lookup.
//! * [`afp_route`] — pre-original detour on `bm2d::SoundCallback::play`.
//! * [`code_se`] — one-byte branch flips that silence World's code-played
//!   sounds a legacy clip already plays itself (`se_game_fullcombo`).

pub mod afp_route;
pub mod bank;
pub mod bank_build;
pub mod code_se;
pub mod cues;

use std::ffi::CString;

use crate::services::game_audio;

/// Play an era-bank cue from code (centre pan), behind the game's own mute
/// filter. `false` when the bank is not registered, the cue is not in it, or
/// the game's sound path refused. Game thread.
pub fn play_era_cue(cue: &str) -> bool {
    let Some(slot) = bank::slot() else {
        return false;
    };
    if bank::lookup(cue.as_bytes()).is_none() {
        return false;
    }
    let Ok(c) = CString::new(cue) else {
        return false;
    };
    unsafe { game_audio::se_play_from_sound_callback(slot, c.as_ptr(), 0.0) }.is_some()
}

/// Stop every playing instance of an era-bank cue. Game thread.
pub fn stop_era_cue(cue: &str) -> bool {
    let (Some(slot), Ok(c)) = (bank::slot(), CString::new(cue)) else {
        return false;
    };
    game_audio::stop_cue_in_slot(slot, &c)
}
