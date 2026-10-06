//! Ownership of the DancePlaySequence's pre-song READY? dwell (`DPS+0x130`,
//! `docs/quick_restart_fail_speedup_research.md` §12.2) between the mods
//! that touch it.
//!
//! Two kinds of writers exist: SKIPPERS seed the timer past the threshold so
//! the song starts as soon as it is ready (quick restart's fresh-DPS path,
//! DDR SELECTION while its legacy stage panel is hosted — A3 had no dwell),
//! and the one HOLDER keeps it below the threshold (Background Dancers'
//! flight take-off, which plays before the song as in the zan games). A
//! skipper and the holder in the same frame would each undo the other —
//! DDR SELECTION seeds from the ShutterActor update, AFTER Background
//! Dancers' input-poll write and before the DPS update reads the timer.
//!
//! The holder announces the next song's hold from its song-window start
//! (long before the DPS exists) and withdraws it when the hold stands down
//! or ends; DDR SELECTION's seeder stands down while it is announced. Quick
//! restart keeps seeding (a quick restart is meant to be quick): the holder
//! sees its seed and stands down.

use std::sync::atomic::{AtomicBool, Ordering};

static HOLD_WANTED: AtomicBool = AtomicBool::new(false);

/// The holder: announce (`true`) / withdraw (`false`) a READY hold for the
/// current song.
pub fn set_hold_wanted(on: bool) {
    HOLD_WANTED.store(on, Ordering::Release);
}

/// A READY hold is announced: dwell skippers that defer must not seed.
pub fn hold_wanted() -> bool {
    HOLD_WANTED.load(Ordering::Acquire)
}
