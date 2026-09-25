//! 1st-5th option forcing (Step 12): A3 played every 1st-5th song with the
//! classic options — speed ×1.00, no boost, visible arrows, step zone on,
//! normal scroll, FLAT colour, CLASSIC arrows, no lane filter, no guideline.
//!
//! A3 did it in its `CourseOption` getters while the skin was 1; World's
//! getters are 4-byte stubs, so this writes the eleven World
//! `ddr::player::Option` fields ([`logic::FIELDS`], World enum values) on
//! every entered side for the song and writes the player's own values back
//! afterwards — zero detours, zero code patches:
//!
//! * every scene change inside {26, 27, 28} with skin 1 armed: a side with
//!   a player and no snapshot is snapshotted (values range-checked — an
//!   implausible read leaves the side alone) and forced; a snapshotted side
//!   is re-asserted (quick restart's fresh DPS re-reads the fields);
//! * the first scene outside the window restores every snapshot — scene 29
//!   precedes the per-stage save marshal, the logout save comes later; a
//!   restart through 29 → 28 re-forces from a fresh snapshot;
//! * disarm / disable restore at once;
//! * the save trampoline rewrites the eleven `/data/option` nodes from the
//!   snapshot if a save is ever built while forced ([`leaked`]; unreachable
//!   by design — the `<timing_music>` precedent).
//!
//! The multiplayer bot's side is a player (`PlayerWork+0x4`): forced too,
//! whichever scene callback runs first (research §3). World's in-song speed
//! change still works — its `ControlSpeedActor` copies the forced Option and
//! steps its own copy from ×1.00, exactly like A3's.
//!
//! Game thread (scene callbacks) except [`leaked`] (the save sender's
//! thread; atomics only). RE: `.agents/planning/2026-09-22-ddr-selection/
//! research/option-forcing.md`.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::stage_records;
use crate::{log_info, log_warn};

use super::options_force_logic::{self as logic, Action, Values, COUNT, FIELDS};

/// The verified field offsets ([`FIELDS`] order).
static OFFSETS: OnceLock<[usize; COUNT]> = OnceLock::new();

#[allow(clippy::declare_interior_mutable_const)]
const ZERO: AtomicI32 = AtomicI32::new(0);
static SNAPSHOT: [[AtomicI32; COUNT]; 2] = [[ZERO; COUNT], [ZERO; COUNT]];
static SNAPSHOTTED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// One implausible-read WARN per side per window.
static WARNED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static WARNED_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

/// Resolve (mod init). `false` ⇒ skin 1 plays with the player's options.
pub fn init(signatures: &SignatureStore) -> bool {
    let mut offs = [0usize; COUNT];
    for (slot, f) in offs.iter_mut().zip(FIELDS.iter()) {
        match signatures.ddr_sel_option_force_offset(f.name) {
            Some(o) if o == f.offset => *slot = o,
            other => {
                log_warn!(
                    "DDR SELECTION: option field {} is {:?} (want +0x{:X}) -- 1st-5th songs keep the player's options",
                    f.name,
                    other,
                    f.offset
                );
                return false;
            }
        }
    }
    if stage_records::player_option_offset().is_none() {
        log_warn!(
            "DDR SELECTION: PlayerWork Option offset underived -- 1st-5th songs keep the player's options"
        );
        return false;
    }
    let _ = OFFSETS.set(offs);
    true
}

pub fn capable() -> bool {
    OFFSETS.get().is_some()
}

/// The side's `ddr::player::Option`, probed.
fn option_ptr(side: usize) -> Option<*mut u8> {
    let work = stage_records::player_work(side)?;
    let off = stage_records::player_option_offset()?;
    let opt = unsafe { work.add(off) };
    memory::is_readable(opt, 0x80).then_some(opt)
}

fn read_values(opt: *mut u8, offs: &[usize; COUNT]) -> Values {
    let mut v = [0; COUNT];
    for (slot, o) in v.iter_mut().zip(offs.iter()) {
        *slot = unsafe { memory::read_i32(opt.add(*o)) };
    }
    v
}

fn write_values(opt: *mut u8, offs: &[usize; COUNT], values: &Values) {
    for (o, v) in offs.iter().zip(values.iter()) {
        unsafe { memory::write_i32(opt.add(*o), *v) };
    }
}

fn describe(values: &Values) -> String {
    FIELDS
        .iter()
        .zip(values.iter())
        .map(|(f, v)| format!("{}={}", f.name, v))
        .collect::<Vec<_>>()
        .join(" ")
}

fn snapshot_of(side: usize) -> Values {
    let mut v = [0; COUNT];
    for (slot, a) in v.iter_mut().zip(SNAPSHOT[side].iter()) {
        *slot = a.load(Ordering::Acquire);
    }
    v
}

/// Scene change (game thread; after the mod's arm / disarm of this edge).
pub fn sync(next: i32, skin: u8) {
    let window = logic::in_window(next, skin);
    let Some(offs) = OFFSETS.get() else {
        if window && !WARNED_UNAVAILABLE.swap(true, Ordering::AcqRel) {
            log_warn!(
                "DDR SELECTION: 1st-5th song but the option fields are unresolved -- the player's own options apply"
            );
        }
        return;
    };
    if !window {
        WARNED[0].store(false, Ordering::Release);
        WARNED[1].store(false, Ordering::Release);
    }
    for side in 0..2 {
        let entered = stage_records::side_entered(side).unwrap_or(false);
        let snapshotted = SNAPSHOTTED[side].load(Ordering::Acquire);
        match logic::action(window, entered, snapshotted) {
            Action::Nothing => {}
            Action::SnapshotAndForce => force(side, offs, next),
            Action::Reassert => reassert(side, offs, next),
            Action::Restore => restore(side, offs, "left the play window"),
        }
    }
}

fn force(side: usize, offs: &[usize; COUNT], scene: i32) {
    let Some(opt) = option_ptr(side) else {
        if !WARNED[side].swap(true, Ordering::AcqRel) {
            log_warn!(
                "DDR SELECTION: P{} Option unreadable -- the player's own options apply this song",
                side + 1
            );
        }
        return;
    };
    let values = read_values(opt, offs);
    if let Some((name, v)) = logic::implausible(&values) {
        if !WARNED[side].swap(true, Ordering::AcqRel) {
            log_warn!(
                "DDR SELECTION: P{} Option read looks wrong ({}={}) -- not forcing the 1st-5th options",
                side + 1,
                name,
                v
            );
        }
        return;
    }
    for (a, v) in SNAPSHOT[side].iter().zip(values.iter()) {
        a.store(*v, Ordering::Release);
    }
    SNAPSHOTTED[side].store(true, Ordering::Release);
    write_values(opt, offs, &logic::forced_values());
    log_info!(
        "DDR SELECTION: P{}{} 1st-5th options forced at scene {} ({} field(s) changed; player's: {})",
        side + 1,
        if crate::mods::multiplayer_bot::is_bot_side(side) {
            " (bot)"
        } else {
            ""
        },
        scene,
        logic::changed_count(&values),
        describe(&values)
    );
}

fn reassert(side: usize, offs: &[usize; COUNT], scene: i32) {
    let Some(opt) = option_ptr(side) else {
        return;
    };
    let now = read_values(opt, offs);
    let drift = logic::changed_count(&now);
    if drift != 0 {
        write_values(opt, offs, &logic::forced_values());
        log_info!(
            "DDR SELECTION: P{} 1st-5th options re-asserted at scene {} ({} field(s) had changed: {})",
            side + 1,
            scene,
            drift,
            describe(&now)
        );
    }
}

fn restore(side: usize, offs: &[usize; COUNT], reason: &str) {
    let Some(opt) = option_ptr(side) else {
        // Keep the snapshot: the save trampoline's rewrite still protects
        // the profile.
        log_warn!(
            "DDR SELECTION: P{} Option unreadable at restore ({}) -- relying on the save-tree rewrite",
            side + 1,
            reason
        );
        return;
    };
    let values = snapshot_of(side);
    write_values(opt, offs, &values);
    SNAPSHOTTED[side].store(false, Ordering::Release);
    log_info!(
        "DDR SELECTION: P{} player's options restored ({})",
        side + 1,
        reason
    );
}

/// Restore every snapshot now (disarm / disable; game thread).
pub fn restore_all(reason: &str) {
    let Some(offs) = OFFSETS.get() else {
        return;
    };
    for side in 0..2 {
        if SNAPSHOTTED[side].load(Ordering::Acquire) {
            restore(side, offs, reason);
        }
    }
}

/// The side's own values when a save is built while it is still forced
/// (should be unreachable). Save sender thread; atomics only.
pub fn leaked(side: usize) -> Option<Values> {
    let side = side.min(1);
    SNAPSHOTTED[side]
        .load(Ordering::Acquire)
        .then(|| snapshot_of(side))
}
