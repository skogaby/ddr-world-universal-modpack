//! Song-select wheel-card events — the single owner of the
//! `selectmusic_card_refresh` detour (design §4.11; promoted from
//! `s_marvelous/lamp_badge.rs`).
//!
//! The wheel is a 3-per-row grid of card objects; each card draws ITS OWN
//! song, held at `card+0x148` (a `ChartMetadata` holder — the same shape
//! `selectmusic_highlight` reads the highlight through; identical on every
//! supported build, pinned by the refresh's first instruction `MOV
//! RCX,[this+0x148]`). The game refreshes a card when it binds it to a song
//! — a vertical scroll binds one new row = three refreshes — so a refresh is
//! the "song became visible" signal.
//!
//! After the original ran, [`CardBound`] is dispatched to every
//! [`on_card_bound`] subscriber: the S-MFC lamp badge (re-binds the lamp
//! bitmap) and the StepMania prefetch scheduler (preview + jacket jobs).
//! Dispatch is scene-25-only, panic-contained per subscriber, and every
//! game-memory read is probed — subscribers receive a holder that resolved
//! to a non-negative mcode. Subscribers run on the frame thread: keep them
//! cheap.
//!
//! Fail-open: an unresolved signature or a failed detour leaves
//! `is_available()` false and no subscriber ever fires.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::{scene_manager, selectmusic_highlight};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

/// `fn(this, flag: u8)` — the header-card refresh.
type CardRefreshFn = unsafe extern "C" fn(*mut u8, u8);

/// A wheel card was (re)bound to a song.
#[derive(Debug, Clone, Copy)]
pub struct CardBound {
    /// The card object (`this` of the refresh).
    pub card: *mut u8,
    /// The card's song holder (`card+0x148`), readable, non-null.
    pub holder: *const u8,
    /// The song's mcode (≥ 0).
    pub mcode: i32,
}

pub type CardBoundFn = fn(&CardBound);

/// The card's OWN song holder pointer (`MOV RCX,[this+0x148]` — the first
/// thing the refresh does; identical on 20250805/20260224/20260825).
pub const CARD_SONG_HOLDER: usize = 0x148;

static mut HOOK: Option<GenericDetour<CardRefreshFn>> = None;
static AVAILABLE: AtomicBool = AtomicBool::new(false);
static SUBSCRIBERS: Mutex<Vec<CardBoundFn>> = Mutex::new(Vec::new());
static WARN_HOLDER: AtomicBool = AtomicBool::new(false);

/// Install the detour (lib.rs service init, after `selectmusic_highlight`).
/// `false` ⇒ no events this boot.
pub fn init(signatures: &SignatureStore) -> bool {
    if !selectmusic_highlight::is_available() {
        log_warn!("WheelCards: selectmusic_highlight unavailable -- card events off");
        return false;
    }
    let Some(target) = signatures.get_address("selectmusic_card_refresh") else {
        log_warn!("WheelCards: selectmusic_card_refresh unresolved -- card events off");
        return false;
    };
    let target: CardRefreshFn = unsafe { std::mem::transmute(target) };
    match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, card_refresh_hook) }
    {
        Ok(()) => {
            AVAILABLE.store(true, Ordering::Release);
            log_info!(
                "WheelCards: song-select card-refresh detour installed @ {:p}",
                target as *const ()
            );
            true
        }
        Err(e) => {
            log_warn!(
                "WheelCards: card-refresh detour failed: {:?} -- card events off",
                e
            );
            false
        }
    }
}

#[must_use]
pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Subscribe to card-bound events (idempotent per fn). Fires post-original,
/// in scene 25 only, on the frame thread.
pub fn on_card_bound(f: CardBoundFn) {
    let mut subs = SUBSCRIBERS.lock().unwrap_or_else(|p| p.into_inner());
    if !subs.contains(&f) {
        subs.push(f);
    }
}

unsafe extern "C" fn card_refresh_hook(this: *mut u8, flag: u8) {
    if let Some(hook) = (*addr_of!(HOOK)).as_ref() {
        hook.call(this, flag);
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(this)));
}

fn dispatch(this: *mut u8) {
    let subs: Vec<CardBoundFn> = match SUBSCRIBERS.lock() {
        Ok(s) if !s.is_empty() => s.clone(),
        _ => return,
    };
    if !scene_manager::is_available() || scene_manager::current_scene() != scene::SONG_SELECT {
        return;
    }
    if this.is_null() || !memory::is_readable(unsafe { this.add(CARD_SONG_HOLDER) }, 8) {
        if !WARN_HOLDER.swap(true, Ordering::Relaxed) {
            log_warn!(
                "WheelCards: card song holder unreadable (latched; events off for such cards)"
            );
        }
        return;
    }
    let holder = unsafe { memory::read_ptr(this.add(CARD_SONG_HOLDER)) };
    if holder.is_null() {
        return; // empty wheel slot
    }
    let Some(mcode) = selectmusic_highlight::mcode_of_holder(holder) else {
        return;
    };
    let ev = CardBound {
        card: this,
        holder,
        mcode,
    };
    for f in subs {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ev)));
    }
}
