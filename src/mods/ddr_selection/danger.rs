//! Theme danger on doubles — World's skin-0 `danger_double` rule on a theme's
//! own `dance_danger0000_v0`.
//!
//! World's `DanceDangerActor::onInitialize` kept A3's branches: it copies the
//! record skin to `actor+0xB4` and picks the clip with
//!
//! ```text
//! CMP [RDI+0xB4],0 ; LEA RSI,["danger_single"] ; LEA R15,["danger_double"]
//! MOV R8,RSI ; JNZ +7 ; TEST R13B,R13B (doubles) ; CMOVNE R8,R15
//! ```
//!
//! so any record skin ≠ 0 draws `danger_single`, the single-lane clip, even
//! on doubles. For a record skin ≥ 6 (a theme) the position and layer
//! branches already take skin 0's paths; only that `JNZ` differs. A3's own
//! skin-0 UI (and World's) shows the full-width `danger_double` on doubles.
//! The eras (1..=5) keep A3's `danger_single`-only behaviour, which is
//! authentic for them. World's skin-0 second clip (`actor+0xA0`, created
//! paused and never read) is not reproduced.
//!
//! Mechanism (the stage-frame / gauge / song-info scoping): one checked
//! 2-byte patch, `75 07` → `90 90`, live exactly while the current
//! `LayoutActor`'s `dance_danger` record is a theme's. The package helper
//! calls [`sync`] for every `dance_danger` request (a theme's registration ⇒
//! on; an era's registration, a stock request or a probe miss ⇒ off), and it
//! is restored at disarm and disable. A failure is cosmetic (doubles keeps
//! the single-lane clip), so the theme package is never held stock for it.
//!
//! Game thread only (the helper runs in `LayoutActor::onInitialize`, before
//! the danger actor initialises). Site: `ddr_sel_danger_double_jnz`
//! (`derive_ddr_sel_danger`); RE in `docs/ddr_selection_a3_themes_research.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// World's `JNZ +7` (a record skin ≠ 0 skips the doubles choice).
const STOCK: [u8; 2] = [0x75, 0x07];
/// Fall through to `TEST R13B,R13B; CMOVNE R8,R15` (World's skin-0 choice).
const PATCHED: [u8; 2] = [0x90, 0x90];

static SITE: OnceLock<usize> = OnceLock::new();
/// The site currently holds [`PATCHED`].
static APPLIED: AtomicBool = AtomicBool::new(false);
/// A patch failed once — the site stays as it is for the session.
static BROKEN: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());

/// Resolve the site and check its stock shape (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(jnz) = signatures.ddr_sel_danger_double_jnz() else {
        log_warn!(
            "DDR SELECTION: danger doubles site unresolved -- theme doubles shows the single-lane danger clip"
        );
        return false;
    };
    let at = jnz as usize;
    if !memory::is_readable(jnz, STOCK.len()) {
        log_warn!(
            "DDR SELECTION: danger doubles site unreadable -- theme doubles shows the single-lane danger clip"
        );
        return false;
    }
    let mut now = [0u8; 2];
    unsafe { std::ptr::copy_nonoverlapping(jnz, now.as_mut_ptr(), now.len()) };
    if now != STOCK {
        log_warn!(
            "DDR SELECTION: danger doubles site is {:02X} {:02X}, not World's JNZ +7 -- theme doubles shows the single-lane danger clip",
            now[0],
            now[1]
        );
        return false;
    }
    let _ = SITE.set(at);
    true
}

/// Put the site in the wanted state: `true` = World's skin-0 doubles choice
/// for the registered theme package, `false` = stock. Idempotent; a no-op
/// when the site is unavailable (and `false` then needs nothing).
pub fn sync(want: bool) {
    let Some(&at) = SITE.get() else {
        return;
    };
    if BROKEN.load(Ordering::Acquire) {
        return;
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if APPLIED.load(Ordering::Acquire) == want {
        return;
    }
    let (from, to) = if want {
        (&STOCK, &PATCHED)
    } else {
        (&PATCHED, &STOCK)
    };
    match unsafe { memory::apply_checked_patch(at as *mut u8, from, to) } {
        Ok(()) => {
            APPLIED.store(want, Ordering::Release);
            if want {
                log_info!(
                    "DDR SELECTION: danger doubles on (the theme's danger_double on doubles)"
                );
            } else {
                log_info!("DDR SELECTION: danger doubles restored (World's record-skin rule)");
            }
        }
        Err(e) => {
            BROKEN.store(true, Ordering::Release);
            log_warn!(
                "DDR SELECTION: danger doubles patch ({}) failed: {:?} -- left as is for this session",
                if want { "apply" } else { "restore" },
                e
            );
        }
    }
}

/// World's rule back (a stock or era `dance_danger`, disarm, disable). No-op
/// when nothing is patched.
pub fn restore() {
    if APPLIED.load(Ordering::Acquire) {
        sync(false);
    }
}
