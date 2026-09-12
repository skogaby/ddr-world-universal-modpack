//! The song-select wheel's HIGHLIGHTED song, read from the game's own
//! select-music model — the identity the song-select refreshers (header
//! card, RecordPanel, DifficultyPanel) themselves draw from.
//!
//! Why not `PlayerWork+0x54`: that field is the COMMITTED / last-played
//! song (written at decide), not the wheel cursor. The S-MFC lamp badge
//! keyed on it "worked" on cabinet deploys #1–#4 only because the tested
//! chart was always the song just played (2026-09-12).
//!
//! Chain (music_wheel_song_length research §6b, cabinet-verified there):
//! `selectmusic_model` global → model pointer → `+highlight_slot`
//! (`0x1B0` on 20260324+, `0x190` before; derived) = an outer HOLDER
//! (`ChartMetadata`) whose `+0x00/+0x08` is the inner
//! `shared_ptr<music::Info>` `{obj, ctrl}`; each wheel card holds its OWN
//! song's holder at `card+0x148` (deploy #5: the cards drew the HIGHLIGHTED
//! song's badge — every visible jacket must be keyed on its own holder);
//! `music::Info` vtable slot 0 = mcode getter (the game's own clear-kind
//! lookup `FUN_1800ff4a0` locks that pair and calls vfunc 0 for the mcode),
//! slot 1 = code-string getter. Every pointer is null-checked, the inner
//! strong count is checked, and the vtable + getter are bounds-checked
//! against the game module before the indirect call (the 2026-08-16
//! wild-call lesson).

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::module_resolver::GameModule;
use crate::core::signatures::SignatureStore;
use crate::log_info;

type McodeGetterFn = unsafe extern "C" fn(*mut u8) -> i32;

static MODEL_GLOBAL: AtomicUsize = AtomicUsize::new(0);
static HIGHLIGHT_SLOT: AtomicUsize = AtomicUsize::new(0);
static MODULE_BASE: AtomicUsize = AtomicUsize::new(0);
static MODULE_SIZE: AtomicUsize = AtomicUsize::new(0);

/// Resolve the model global + highlight slot. Fail-open: `false` leaves
/// [`highlighted_mcode`] returning `None`.
pub fn init(signatures: &SignatureStore, module: &GameModule) -> bool {
    let (Some(model), Some(slot)) = (
        signatures.get_address("selectmusic_model"),
        signatures.selectmusic_highlight_slot(),
    ) else {
        return false;
    };
    MODEL_GLOBAL.store(model as usize, Ordering::Release);
    HIGHLIGHT_SLOT.store(slot, Ordering::Release);
    MODULE_BASE.store(module.base as usize, Ordering::Release);
    MODULE_SIZE.store(module.size, Ordering::Release);
    log_info!(
        "selectmusic_highlight: model global {:p}, highlight slot +0x{:X}",
        model,
        slot
    );
    true
}

pub fn is_available() -> bool {
    MODEL_GLOBAL.load(Ordering::Acquire) != 0
}

/// The highlighted song's HOLDER (`selectmusic::sequence::ChartMetadata` —
/// the object the model's `+highlight_slot` and each wheel card's `+0x148`
/// point at), or `None` when nothing is highlighted / unreadable.
pub fn highlighted_holder() -> Option<*const u8> {
    let global = MODEL_GLOBAL.load(Ordering::Acquire);
    let slot = HIGHLIGHT_SLOT.load(Ordering::Acquire);
    if global == 0 || slot == 0 {
        return None;
    }
    let global = global as *const u8;
    if !memory::is_readable(global, 8) {
        return None;
    }
    let model = unsafe { memory::read_ptr(global) };
    if model.is_null() || !memory::is_readable(unsafe { model.add(slot) }, 16) {
        return None;
    }
    let holder = unsafe { memory::read_ptr(model.add(slot)) };
    (!holder.is_null()).then_some(holder)
}

/// The highlighted song's mcode, or `None` when nothing is highlighted or
/// any link of the chain is unreadable/implausible. Game thread only.
pub fn highlighted_mcode() -> Option<i32> {
    mcode_of_holder(highlighted_holder()?)
}

/// The mcode of the song a HOLDER refers to: lock-free read of the holder's
/// inner `shared_ptr<music::Info>` `{obj, ctrl}` at `+0x00/+0x08` (strong
/// count must be non-zero), then `Info` vtable slot 0 — the game's own
/// clear-kind lookup does exactly this (`FUN_18010f790` + vfunc 0). Every
/// pointer is probed and the vtable/getter bounds-checked against the game
/// module before the indirect call.
pub fn mcode_of_holder(holder: *const u8) -> Option<i32> {
    let base = MODULE_BASE.load(Ordering::Acquire);
    let size = MODULE_SIZE.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }
    let in_module = |p: usize| p >= base && p < base + size;
    if holder.is_null() || !memory::is_readable(holder, 16) {
        return None;
    }
    let inner = unsafe { memory::read_ptr(holder) } as *mut u8;
    let ctrl = unsafe { memory::read_ptr(holder.add(8)) };
    if inner.is_null() || ctrl.is_null() || !memory::is_readable(ctrl, 12) {
        return None;
    }
    // Inner strong count (MSVC control block +0x08) — expired ⇒ don't touch.
    if unsafe { memory::read_u32(ctrl.add(8)) } == 0 {
        return None;
    }
    if !memory::is_readable(inner, 8) {
        return None;
    }
    let vtable = unsafe { memory::read_ptr(inner) } as *const usize;
    if vtable.is_null()
        || !in_module(vtable as usize)
        || !memory::is_readable(vtable as *const u8, 16)
    {
        return None;
    }
    let getter = unsafe { *vtable };
    if !in_module(getter) {
        return None;
    }
    let getter: McodeGetterFn = unsafe { std::mem::transmute(getter) };
    let mcode = unsafe { getter(inner) };
    (mcode >= 0).then_some(mcode)
}

/// The side's selected difficulty KIND cursor (`model + 4 + side*4`, 0..=4)
/// — what the game's clear-kind lookup resolves each song's row from.
pub fn side_kind(side: usize) -> Option<i32> {
    let global = MODEL_GLOBAL.load(Ordering::Acquire);
    if global == 0 || side > 1 {
        return None;
    }
    let global = global as *const u8;
    if !memory::is_readable(global, 8) {
        return None;
    }
    let model = unsafe { memory::read_ptr(global) };
    if model.is_null() || !memory::is_readable(unsafe { model.add(4) }, 8) {
        return None;
    }
    let kind = unsafe { memory::read_i32(model.add(4 + side * 4)) };
    (0..=4).contains(&kind).then_some(kind)
}

/// The difficulty a song HOLDER resolves the side's KIND cursor to — the
/// game's rule (`FUN_1800ff4a0`): when the holder has NO child redirect
/// (`*(holder+0x10) == null`) it is `*(holder + 0x74 + kind*4)`, otherwise
/// the fixed `*(holder + 0x70)`. This is what the lamp the game draws for
/// that song is keyed on (NOT the side's raw cursor, which is unclamped on
/// songs lacking that chart).
pub fn holder_difficulty(holder: *const u8, kind: i32) -> Option<i32> {
    if holder.is_null() || !(0..=4).contains(&kind) || !memory::is_readable(holder, 0x88) {
        return None;
    }
    let child = unsafe { memory::read_ptr(holder.add(0x10)) };
    let diff = if child.is_null() {
        unsafe { memory::read_i32(holder.add(0x74 + kind as usize * 4)) }
    } else {
        unsafe { memory::read_i32(holder.add(0x70)) }
    };
    (0..=4).contains(&diff).then_some(diff)
}
