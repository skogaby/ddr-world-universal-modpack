//! Movie-size override (design §4.3.7): while a background-dancer song
//! window is open, every entered side whose `Customize + 0x30` movie-size
//! field selects the FULLSCREEN movie layer (values 0/1) is switched to the
//! sized "ON" layer (2) so the background movie renders as the thumbnail
//! marker OVER the 3D scene instead of covering it; the original values are
//! written back at window exit.
//!
//! Why a write into the Customize object: the game reads the field once, at
//! `DancePlaySequence` step 2, through the governing side's movie-size
//! getter — there is no per-song input to intercept. The scene callback
//! that opens the window fires BEFORE `createNextSequence`, so the write
//! lands before that read; the VIDEO SIZE option row re-seeds from the
//! field only at SONG_SELECT entry and the logout customize write-back
//! happens at EAM_EXIT — both after the restore. Maintainer rule: the movie
//! is never suppressed, only resized.
//!
//! Fail-open: `player_work_table` / `customize_offset` unresolved ⇒ nothing
//! is written (one INFO at init); a side that is not carded in has no
//! Customize object and is skipped silently; a field that no longer reads 2
//! at restore time (the player changed VIDEO SIZE in between — impossible
//! mid-song, defensive) is left alone.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::log_info;
use crate::services::stage_records;

/// `Customize + 0x30` — the movie-size field (shared with
/// `movie_size_customization.rs`).
const CUSTOMIZE_MOVIE_SIZE_OFFSET: usize = 0x30;
/// Values selecting the fullscreen movie layer (0 = unset, 1 = FULLSCREEN).
const FULLSCREEN_VALUES: [u32; 2] = [0, 1];
/// The sized "ON" layer.
const SIZED_ON: u32 = 2;

/// `customize_offset` (0 = unavailable).
static CUSTOMIZE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Resolve the shared derivation once (mod init). `false` = the override is
/// unavailable for this boot (movies stay fullscreen and vanish under the
/// 2D hide on movie songs).
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.get_address("customize_offset") {
        Some(off) if off as usize != 0 => {
            CUSTOMIZE_OFFSET.store(off as usize, Ordering::Release);
            true
        }
        _ => {
            log_info!(
                "BackgroundDancers: customize_offset unresolved -- movie-size override unavailable (movie songs keep the fullscreen movie)"
            );
            false
        }
    }
}

pub fn is_available() -> bool {
    CUSTOMIZE_OFFSET.load(Ordering::Acquire) != 0 && stage_records::is_available()
}

/// One side's movie-size field pointer (probed), or `None` when the side has
/// no PlayerWork/Customize (not carded in) or the chain is unreadable.
fn field_ptr(side: usize) -> Option<*mut u32> {
    let off = CUSTOMIZE_OFFSET.load(Ordering::Acquire);
    if off == 0 {
        return None;
    }
    let work = stage_records::player_work(side)?;
    let p = unsafe { work.add(off + CUSTOMIZE_MOVIE_SIZE_OFFSET) };
    if !memory::is_readable(p, 4) {
        return None;
    }
    Some(p as *mut u32)
}

/// Window entry: for every entered side reading 0/1, write 2 and remember
/// the original. Returns what to hand back to [`restore`].
pub fn apply(entered: [bool; 2]) -> [Option<u32>; 2] {
    let mut saved = [None, None];
    for side in 0..2 {
        if !entered[side] {
            continue;
        }
        let Some(p) = field_ptr(side) else { continue };
        // SAFETY: probed readable; the Customize object is game-owned but
        // this field is a plain u32 the game itself rewrites from the menu.
        let v = unsafe { p.read_volatile() };
        if FULLSCREEN_VALUES.contains(&v) {
            unsafe { p.write_volatile(SIZED_ON) };
            saved[side] = Some(v);
        }
    }
    saved
}

/// Window exit: write the remembered values back where the field still
/// reads the value we wrote.
pub fn restore(saved: [Option<u32>; 2]) -> usize {
    let mut restored = 0;
    for side in 0..2 {
        let Some(v) = saved[side] else { continue };
        let Some(p) = field_ptr(side) else { continue };
        // SAFETY: as `apply`.
        unsafe {
            if p.read_volatile() == SIZED_ON {
                p.write_volatile(v);
                restored += 1;
            }
        }
    }
    restored
}

/// Pure value mapping (host-testable): what `apply` would write for a
/// current field value, if anything.
pub fn override_for(current: u32) -> Option<u32> {
    FULLSCREEN_VALUES.contains(&current).then_some(SIZED_ON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fullscreen_values_are_overridden() {
        assert_eq!(override_for(0), Some(2));
        assert_eq!(override_for(1), Some(2));
        assert_eq!(override_for(2), None);
        assert_eq!(override_for(3), None);
        assert_eq!(override_for(7), None);
    }
}
