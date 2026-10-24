//! Fail-closed hiding (design §4.13, F12).
//!
//! Injected songs are only ever exposed under the guarantee that their plays
//! cannot produce a score save: the per-stage save is suppressed by the
//! injected taint and the card-out logout save is sanitised by the EAM_EXIT
//! whole-side wipe. That machinery is assembled by services that come up
//! AFTER the music DB was built from `startup.arc`, so the injection decision
//! cannot wait for it. Instead, once the mods enable (every service is up),
//! [`enforce`] asks `score_guard::is_full_sanitization_available()`; if the
//! answer is no, every injected entry gets its corruption flag set.
//!
//! The corruption flag (`music::Info + 0x1B0`, the byte `fast_bootup` writes
//! for a zero-step chart) makes the game's "is chart active" predicate drop
//! the entry at the next active-list rebuild — which happens on every sequence
//! transition, i.e. before any player reaches the wheel. The flag hides; it
//! never reports (`ME1529` is the reporter, and only `CheckStepDataActor`
//! calls it).
//!
//! Belt and braces: the taint in `taint.rs` still fires for any injected bank
//! the game loads regardless of visibility, so even a song that slipped
//! through (a rebuild that has not happened yet) suppresses its save.
//!
//! Resolution: `find_music_by_mcode` is a derived signature (`signatures.rs`);
//! when it is missing nothing can be hidden and one WARN says so — the taint
//! remains the sole guard.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::score_guard;
use crate::{log_info, log_warn};

/// `music::Info + 0x1B0`: the per-song corruption flag byte (see
/// `fast_bootup`'s `ENTRY_CORRUPT_FLAG`; stable across every supported build,
/// anchored by the unique `MOV byte [RAX+0x1B0],1` in `CheckStepDataActor`).
const ENTRY_CORRUPT_FLAG: usize = 0x1B0;

type FindMcodeFn = unsafe extern "C" fn(i32) -> *mut u8;

static FIND_MCODE: AtomicUsize = AtomicUsize::new(0);
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// Resolve the lookup once (mod `init`, where the signature store is at hand).
pub fn resolve(signatures: &SignatureStore) {
    if let Some(a) = signatures.get_address("find_music_by_mcode") {
        FIND_MCODE.store(a as usize, Ordering::Release);
    }
}

/// Whether this boot hid the injected songs for safety.
pub fn hidden_for_safety() -> bool {
    HIDDEN.load(Ordering::Acquire)
}

/// Apply the fail-closed rule for `mcodes`: hide them all when full score
/// sanitisation is unavailable. Returns the number of entries hidden.
pub fn enforce(mcodes: &[u32]) -> usize {
    if mcodes.is_empty() {
        return 0;
    }
    if score_guard::is_full_sanitization_available() {
        log_info!(
            "StepMania: score sanitisation available -- {} injected song(s) stay visible",
            mcodes.len()
        );
        return 0;
    }
    let hidden = hide_all(mcodes);
    if hidden > 0 {
        HIDDEN.store(true, Ordering::Release);
    }
    log_warn!(
        "StepMania: score sanitisation UNAVAILABLE -- hid {}/{} injected song(s) (corruption flag; any that still load are save-suppressed by the taint)",
        hidden,
        mcodes.len()
    );
    hidden
}

/// Hide `mcodes` unconditionally (corruption flag) — for injected songs that
/// cannot be served this boot (e.g. split-timing files without the
/// `ssq_path` chain). Returns the number of entries hidden.
pub fn hide_unservable(mcodes: &[u32], why: &str) -> usize {
    if mcodes.is_empty() {
        return 0;
    }
    let hidden = hide_all(mcodes);
    log_warn!(
        "StepMania: {} -- hid {}/{} injected song(s) (corruption flag)",
        why,
        hidden,
        mcodes.len()
    );
    hidden
}

fn hide_all(mcodes: &[u32]) -> usize {
    let find = FIND_MCODE.load(Ordering::Acquire);
    if find == 0 {
        log_warn!(
            "StepMania: find_music_by_mcode unresolved -- cannot hide injected songs; the taint is the only guard"
        );
        return 0;
    }
    let find: FindMcodeFn = unsafe { std::mem::transmute::<usize, FindMcodeFn>(find) };
    let mut hidden = 0usize;
    for &mcode in mcodes {
        if mcode > i32::MAX as u32 {
            continue;
        }
        let entry = unsafe { find(mcode as i32) };
        if entry.is_null() || !memory::is_readable(entry as *const u8, ENTRY_CORRUPT_FLAG + 1) {
            continue;
        }
        unsafe {
            (entry.add(ENTRY_CORRUPT_FLAG) as *mut u8).write_unaligned(1);
        }
        hidden += 1;
    }
    hidden
}
