//! Route the legacy AFP clips' embedded sounds to the `dsel` bank.
//!
//! World's `bm2d::SoundCallback::play(this, label)` (signature
//! `afp_sound_callback_play`) picks ONE manager slot from the cue name (2 or
//! 3) and plays it through `se_play`; a miss is silent. The legacy DDR
//! SELECTION clips name A3 cues that only the `dsel` bank (slot 4) holds, so
//! while a legacy skin is armed this pre-original detour sends every `dsel`
//! cue there, with the stock pan rule — including the few World's own banks
//! also carry (`vo_ingame_ready`, `vo_stage_clear`, `vo_stage_*`: A3's takes
//! on A3 clips). The same holds while A3's stage panel is live before the
//! play edge arms the skin (it is hosted from the song-select confirm).
//! Otherwise the original always runs (stock World).
//!
//! Runs inside libafp's display pass (`afp_do_display`), not the game-logic
//! loop: lock-free, allocation-free, log-free. Counters are drained into one
//! INFO by the game thread ([`drain_counters`]).

use std::ffi::{c_char, CStr};
use std::ptr::addr_of;
use std::sync::atomic::{AtomicU32, Ordering};

use retour::GenericDetour;

use crate::core::hooks;
use crate::services::game_audio;
use crate::{log_info, log_warn};

use super::bank;

type PlayFn = unsafe extern "system" fn(*mut u8, *const c_char) -> u32;

static mut HOOK: Option<GenericDetour<PlayFn>> = None;
/// Per-cue plays from the era bank (index = `bank::lookup` index).
static PLAYED: [AtomicU32; bank::MAX_CUES] = [const { AtomicU32::new(0) }; bank::MAX_CUES];
/// Era cues the game's sound path refused (muted / cue table full).
static FAILED: AtomicU32 = AtomicU32::new(0);
/// AFP sound calls while armed that name no era cue (World's own cues).
static OTHER: AtomicU32 = AtomicU32::new(0);

/// `this+8`: the side the playing BM2D group belongs to (0, 1; 2 = none).
const CALLBACK_SIDE_OFF: usize = 0x08;

/// Install the detour (once). `false` = unavailable (legacy clips' own sounds
/// stay silent; nothing else changes).
pub fn install(play: *const u8) -> bool {
    if unsafe { (*addr_of!(HOOK)).is_some() } {
        return true;
    }
    let target: PlayFn = unsafe { std::mem::transmute(play) };
    match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, play_hook) } {
        Ok(()) => {
            log_info!("DDR SELECTION: AFP sound-callback route installed");
            true
        }
        Err(e) => {
            log_warn!("DDR SELECTION: AFP sound-callback detour failed: {e}");
            false
        }
    }
}

/// Whether the route is installed (the legacy clips' own sounds can play).
pub fn is_installed() -> bool {
    unsafe { (*addr_of!(HOOK)).is_some() }
}

unsafe extern "system" fn play_hook(this: *mut u8, label: *const c_char) -> u32 {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return u32::MAX;
    };
    let routed = std::panic::catch_unwind(|| unsafe { route(this, label) }).unwrap_or(None);
    match routed {
        Some(h) => h,
        None => hook.call(this, label),
    }
}

/// `Some(handle)` = played from `dsel` (the original must not run).
unsafe fn route(this: *mut u8, label: *const c_char) -> Option<u32> {
    if label.is_null() || !super::super::is_enabled() {
        return None;
    }
    // Armed, or A3's stage panel already live (the song-select stage-panel
    // request hosts it before the play edge arms the skin), or A3's end
    // banner still on screen (it outlives the armed window).
    if super::super::armed_skin() == 0
        && !super::super::panel::root_live()
        && !super::super::banner::live()
    {
        return None;
    }
    let slot = bank::slot()?;
    let Some((index, _shared)) = bank::lookup(CStr::from_ptr(label).to_bytes()) else {
        OTHER.fetch_add(1, Ordering::Relaxed);
        return None;
    };
    match game_audio::se_play_from_sound_callback(slot, label, stock_pan(this)) {
        Some(h) => {
            if let Some(c) = PLAYED.get(index) {
                c.fetch_add(1, Ordering::Relaxed);
            }
            Some(h)
        }
        None => {
            // Muted / cue table full: let the stock path try (World's take for
            // a shared cue, a silent miss otherwise — exactly stock).
            FAILED.fetch_add(1, Ordering::Relaxed);
            None
        }
    }
}

/// The stock rule: centre unless the audio manager's versus-pan byte is set,
/// then side 0 → −1.0, side 1 → +1.0, anything else centre.
unsafe fn stock_pan(this: *mut u8) -> f32 {
    if this.is_null() || game_audio::versus_pan().unwrap_or(0) == 0 {
        return 0.0;
    }
    match (this.add(CALLBACK_SIDE_OFF) as *const u32).read_unaligned() {
        0 => -1.0,
        1 => 1.0,
        _ => 0.0,
    }
}

/// Log and reset the routing counters (game thread; every scene change while
/// armed, so the summary survives a session that ends mid-window).
pub fn drain_counters() {
    let mut played = Vec::new();
    for (i, c) in PLAYED.iter().enumerate() {
        let n = c.swap(0, Ordering::Relaxed);
        if n > 0 {
            played.push(format!("{} x{}", bank::cue_name(i).unwrap_or("?"), n));
        }
    }
    let failed = FAILED.swap(0, Ordering::Relaxed);
    let other = OTHER.swap(0, Ordering::Relaxed);
    if played.is_empty() && failed == 0 && other == 0 {
        return;
    }
    log_info!(
        "DDR SELECTION: legacy clip sounds -- played from the era bank: [{}]; refused by the game's sound path: {}; World cues passed through: {}",
        played.join(", "),
        failed,
        other
    );
}
