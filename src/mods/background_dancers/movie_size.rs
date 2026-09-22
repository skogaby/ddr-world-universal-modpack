//! Movie-size override (design §4.3.7, Background Movies 2026-09-22):
//! while a background-dancer song window is open, every entered side whose
//! `Customize + 0x30` movie-size field shows a movie is switched to what the
//! GLOBAL SETTINGS row "Background Movies" asks for (`movie_mode.rs`):
//! OFF → 3 (the game's own VIDEO SIZE OFF), THUMBNAIL → 2 (the sized "ON"
//! window over the 3D scene — the original behaviour), FULLSCREEN → 1 (the
//! movie under the 3D pass, stage hidden per song by `lifecycle.rs`). The
//! original values are written back at window exit.
//!
//! Why a write into the Customize object: the game reads the field once, at
//! `DancePlaySequence` step 2, through the governing side's movie-size
//! getter — there is no per-song input to intercept. The scene callback
//! that opens the window fires BEFORE `createNextSequence`, so the write
//! lands before that read; the VIDEO SIZE option row re-seeds from the
//! field only at SONG_SELECT entry and the logout customize write-back
//! happens at EAM_EXIT — both after the restore.
//!
//! Fail-open: `player_work_table` / `customize_offset` unresolved ⇒ nothing
//! is written (one INFO at init); a side that is not carded in has no
//! Customize object and is skipped silently; a field that no longer reads
//! the value written at restore time (the player changed VIDEO SIZE in
//! between — impossible mid-song, defensive) is left alone.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::log_info;
use crate::services::stage_records;

use super::movie_mode::{self, MovieMode};

/// `Customize + 0x30` — the movie-size field (shared with
/// `movie_size_customization.rs`).
const CUSTOMIZE_MOVIE_SIZE_OFFSET: usize = 0x30;

/// `customize_offset` (0 = unavailable).
static CUSTOMIZE_OFFSET: AtomicUsize = AtomicUsize::new(0);

/// One side's override: the value found at window entry and the value
/// written over it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Saved {
    pub original: u32,
    pub written: u32,
}

/// Resolve the shared derivation once (mod init). `false` = the override is
/// unavailable for this boot (movies keep the player's VIDEO SIZE; a
/// fullscreen movie vanishes under the 3D stage).
pub fn init(signatures: &SignatureStore) -> bool {
    match signatures.get_address("customize_offset") {
        Some(off) if off as usize != 0 => {
            CUSTOMIZE_OFFSET.store(off as usize, Ordering::Release);
            true
        }
        _ => {
            log_info!(
                "BackgroundDancers: customize_offset unresolved -- movie-size override unavailable (movies keep the player's VIDEO SIZE)"
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

/// Window entry: for every entered side whose value `mode` overrides
/// (`movie_mode::size_override`), write the override and remember both
/// values. Returns what to hand back to [`restore`].
pub fn apply(entered: [bool; 2], mode: MovieMode) -> [Option<Saved>; 2] {
    let mut saved = [None, None];
    for side in 0..2 {
        if !entered[side] {
            continue;
        }
        let Some(p) = field_ptr(side) else { continue };
        // SAFETY: probed readable; the Customize object is game-owned but
        // this field is a plain u32 the game itself rewrites from the menu.
        let v = unsafe { p.read_volatile() };
        if let Some(w) = movie_mode::size_override(mode, v) {
            unsafe { p.write_volatile(w) };
            saved[side] = Some(Saved {
                original: v,
                written: w,
            });
        }
    }
    saved
}

/// Window exit: write the remembered values back where the field still
/// reads the value we wrote.
pub fn restore(saved: [Option<Saved>; 2]) -> usize {
    let mut restored = 0;
    for side in 0..2 {
        let Some(s) = saved[side] else { continue };
        let Some(p) = field_ptr(side) else { continue };
        // SAFETY: as `apply`.
        unsafe {
            if p.read_volatile() == s.written {
                p.write_volatile(s.original);
                restored += 1;
            }
        }
    }
    restored
}
