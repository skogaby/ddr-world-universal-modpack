//! Foot-Panel Swap — the single owner of the `judgeNotes` foot-panel swap.
//!
//! `GamePlayActor::judgeNotes` reads its input through an `IFootPanel*` slot on
//! the actor (`judge_hook::foot_panel_offset()`, 0x270 / 0x278 by build). The
//! game's own autoplay is nothing more than a different object in that slot —
//! `AutoFootPanel`, whose `update` fills the flag arrays with a perfect press
//! for every note. This service owns the pre-judge / post-judge pair that
//! swaps a side's slot for the duration of ONE `judgeNotes` call and restores
//! it afterwards, and arbitrates WHICH object goes in via a per-side
//! [`Controller`]:
//!
//! * `Perfect` — the stock `AutoFootPanel` object driven by the game's own
//!   `update` (the autoplay mod's request; behaviour identical to the
//!   pre-extraction autoplay mod).
//! * `Bot` — a DLL-owned [`BotFootPanel`] whose vtable is a clone of the stock
//!   one with slots 5/6 (`getPressAge` / `consumePress`) replaced, so the
//!   judge's `event = mc − getPressAge(panel)` lands exactly on the planned
//!   `event_mc[panel]` on every build; a registered [`BotFillFn`] produces the
//!   flag block per frame (the multiplayer-bot mod's request).
//! * `Off` — the game's own object stays in the slot.
//!
//! `Bot` outranks `Perfect`: per-side option values outlive the player, so the
//! bot's side may carry a cached `autoplay = ON` that must not win.
//!
//! ## Why one owner
//!
//! The judge dispatcher allows many subscribers but fires same-priority
//! callbacks in registration order, which must not be relied on. Two mods each
//! swapping the same slot would stash each other's object. The swap therefore
//! has exactly one pre (`Priority::Late`) and one post (`Priority::Early`)
//! registration, here — the slots the autoplay mod occupied, so
//! `per_song_judgement_offsets` (pre Early) and `power_user_statistics` (pre
//! Normal) keep their relative order.
//!
//! ## Init order
//!
//! `init` runs from `lib.rs` right after `judge_hook::init` (it registers on
//! that dispatcher) and before any mod's `init` (autoplay and multiplayer-bot
//! both refuse to init when this service is unavailable).

mod layout;

pub use layout::{BotFootPanel, BotPanelFlags, Controller, ACTOR_CUR_BEAT, ACTOR_SIDE};

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicUsize, Ordering};

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::services::judge_hook::{self, Priority};
use crate::{log_info, log_warn};

use layout::{
    bot_vtable_image, effective_controller, panel_index, ACTOR_RESULTS_BEGIN, PANEL_OBJECT_SIZE,
    VTABLE_SLOTS,
};

/// Fills the bot panel for one judge frame. Called on the game thread inside
/// the pre-judge callback for a side whose controller is `Bot`. Must be
/// panic-free (the dispatcher's `catch_unwind` is the last line of defence,
/// not the first).
pub type BotFillFn = fn(side: usize, actor: *mut u8, music_count: i32, out: &mut BotPanelFlags);

/// The game's `AutoFootPanel::update(this, &results, cur_beat, music_count)`.
type AutoUpdateFn = unsafe extern "C" fn(*mut u8, *const u8, i32, i32);

static AVAILABLE: AtomicBool = AtomicBool::new(false);
/// `judge_hook::foot_panel_offset()` captured at init (0 = unavailable).
static FOOT_PANEL_OFFSET: AtomicUsize = AtomicUsize::new(0);
/// The stock-shaped `AutoFootPanel` object (stock vtable at +0x00) used by
/// the `Perfect` controller. One shared object suffices: sides are judged
/// sequentially on one thread and the game's `update` rewrites every field.
static STOCK_PANEL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// `AutoFootPanel::update`, stored as a raw pointer (null = unresolved).
static STOCK_UPDATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

/// The two DLL-owned bot panel objects (one per side; null until the vtable
/// clone succeeded at init). They live in one RWX region together with the
/// cloned vtable image: `[COL][slot0..6][BotFootPanel 0][BotFootPanel 1]`.
static BOT_PANEL: [AtomicPtr<BotFootPanel>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];
/// The music count of the judge frame in progress, per side — the `mc` in
/// `event = mc − getPressAge(panel)`. Stored by `swap_in` immediately before
/// the game's `judgeNotes` reads the panel; sides are judged sequentially on
/// one thread.
static CURRENT_MC: [AtomicI32; 2] = [AtomicI32::new(0), AtomicI32::new(0)];

// ── Per-side controller state ──────────────────────────────────────────
static PERFECT: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static BOT_ARMED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// The armed [`BotFillFn`] per side (null = none).
static BOT_FILL: [AtomicPtr<()>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];
/// One-shot latch for the "bot armed but no panel object yet" WARN, cleared
/// by `disarm_bot` so every arm reports once.
static BOT_UNWIRED_WARNED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

// ── Per-frame scratch state ────────────────────────────────────────────
/// One stash per side so P1 and P2 restore independently when both are in
/// gameplay (versus).
static ORIGINAL_FOOT_PANEL: [AtomicPtr<u8>; 2] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

/// Play side of an actor, clamped to 0..=1 (doubles reports 0; anything
/// outside 0..=1 is treated as side 0 rather than indexing out of bounds).
fn actor_side(actor: *mut u8) -> usize {
    let side = unsafe { memory::read_i32(actor.add(ACTOR_SIDE)) };
    usize::from(side == 1)
}

/// Which side a bot object belongs to (unknown pointer ⇒ side 0 — never
/// reached for a well-formed swap; the mask keeps it in range regardless).
fn side_of(this: *const BotFootPanel) -> usize {
    usize::from(this == BOT_PANEL[1].load(Ordering::Acquire) as *const BotFootPanel)
}

/// Cloned vtable slot 5 — `IFootPanel::getPressAge(this, panel) -> i32`.
/// The judge computes `event = mc − age`; returning `mc − event_mc[panel]`
/// lands the graded event exactly on the planner's `E`, on every build,
/// independent of the stock press-time clock and stride.
unsafe extern "C" fn bot_get_press_age(this: *mut BotFootPanel, panel: i32) -> i32 {
    if this.is_null() {
        return 0;
    }
    let mc = CURRENT_MC[side_of(this)].load(Ordering::Acquire);
    mc.wrapping_sub((*this).event_mc[panel_index(panel)])
}

/// Cloned vtable slot 6 — `IFootPanel::consumePress(this, panel)`: the judge
/// consumes the accepted note's presses; zero the event so a stale stamp can
/// never be re-read.
unsafe extern "C" fn bot_consume_press(this: *mut BotFootPanel, panel: i32) {
    if this.is_null() {
        return;
    }
    (*this).event_mc[panel_index(panel)] = 0;
}

/// Build the cloned vtable + the two bot objects from the RTTI-resolved stock
/// vtable. Fail-open: `None` (one WARN) leaves the `Bot` controller unarmable
/// while `Perfect` keeps working.
fn build_bot_objects(stock_vtable: *const u8) -> Option<[*mut BotFootPanel; 2]> {
    // COL at [-1] plus the 7 slots must be readable.
    let base = unsafe { stock_vtable.sub(8) };
    if !memory::is_readable(base, 8 * (VTABLE_SLOTS + 1)) {
        log_warn!("FootPanelSwap: AutoFootPanel vtable not readable -- bot controller unavailable");
        return None;
    }
    let mut donor = [0usize; VTABLE_SLOTS];
    let col = unsafe { memory::read_ptr(base) } as usize;
    for (i, slot) in donor.iter_mut().enumerate() {
        *slot = unsafe { memory::read_ptr(stock_vtable.add(i * 8)) } as usize;
        if *slot == 0 {
            log_warn!(
                "FootPanelSwap: AutoFootPanel vtable slot {} is NULL -- bot controller unavailable",
                i
            );
            return None;
        }
    }
    let image = bot_vtable_image(
        &donor,
        col,
        bot_get_press_age as *const () as usize,
        bot_consume_press as *const () as usize,
    );

    let image_bytes = 8 * (VTABLE_SLOTS + 1);
    let total = image_bytes + 2 * PANEL_OBJECT_SIZE;
    let region = unsafe { memory::alloc_zeroed(total) };
    if region.is_null() {
        log_warn!(
            "FootPanelSwap: bot panel region allocation failed -- bot controller unavailable"
        );
        return None;
    }
    unsafe {
        for (i, v) in image.iter().enumerate() {
            memory::write_ptr(region.add(i * 8), *v as *const u8);
        }
        let vtable_ptr = region.add(8) as *const *const u8;
        let mut objs = [std::ptr::null_mut(); 2];
        for (side, obj) in objs.iter_mut().enumerate() {
            let p = region.add(image_bytes + side * PANEL_OBJECT_SIZE) as *mut BotFootPanel;
            p.write(BotFootPanel::zeroed());
            (*p).vtable = vtable_ptr;
            *obj = p;
        }
        Some(objs)
    }
}

/// Pre-judge callback (`Priority::Late`): swap the slot per the side's
/// controller. Panic-free: every index is derived from a clamped side.
fn swap_in(actor: *mut u8, music_count: i32) {
    let fp_offset = FOOT_PANEL_OFFSET.load(Ordering::Acquire);
    if fp_offset == 0 || actor.is_null() {
        return;
    }
    let side = actor_side(actor);

    match controller(side) {
        Controller::Off => {}
        Controller::Perfect => {
            let panel = STOCK_PANEL.load(Ordering::Acquire);
            let update = STOCK_UPDATE.load(Ordering::Acquire);
            if panel.is_null() || update.is_null() {
                return;
            }
            unsafe {
                let fp_slot = actor.add(fp_offset);
                let original_fp = *(fp_slot as *const *mut u8);
                ORIGINAL_FOOT_PANEL[side].store(original_fp, Ordering::Release);

                memory::write_ptr(fp_slot, panel as *const u8);

                let update_fn: AutoUpdateFn = std::mem::transmute(update);
                let cur_beat = memory::read_i32(actor.add(ACTOR_CUR_BEAT) as *const u8);
                update_fn(
                    panel,
                    actor.add(ACTOR_RESULTS_BEGIN) as *const u8,
                    cur_beat,
                    music_count,
                );
            }
        }
        Controller::Bot => {
            let obj = BOT_PANEL[side].load(Ordering::Acquire);
            let fill_raw = BOT_FILL[side].load(Ordering::Acquire);
            if obj.is_null() || fill_raw.is_null() {
                if !BOT_UNWIRED_WARNED[side].swap(true, Ordering::AcqRel) {
                    log_warn!(
                        "FootPanelSwap: bot controller armed on side {} but the bot panel object / filler is missing -- behaving as Off",
                        side
                    );
                }
                return;
            }
            // SAFETY: `fill_raw` was stored from a `BotFillFn` by `arm_bot`.
            let fill: BotFillFn = unsafe { std::mem::transmute(fill_raw) };
            CURRENT_MC[side].store(music_count, Ordering::Release);
            let mut flags = BotPanelFlags::default();
            fill(side, actor, music_count, &mut flags);
            unsafe {
                (*obj).apply(&flags);
                let fp_slot = actor.add(fp_offset);
                let original_fp = *(fp_slot as *const *mut u8);
                ORIGINAL_FOOT_PANEL[side].store(original_fp, Ordering::Release);
                memory::write_ptr(fp_slot, obj as *const u8);
            }
        }
    }
}

/// Post-judge callback (`Priority::Early`): restore whatever `swap_in`
/// stashed for this side. A null stash means nothing was swapped.
fn swap_out(actor: *mut u8, _music_count: i32) {
    let fp_offset = FOOT_PANEL_OFFSET.load(Ordering::Acquire);
    if fp_offset == 0 || actor.is_null() {
        return;
    }
    let side = actor_side(actor);

    let original_fp = ORIGINAL_FOOT_PANEL[side].swap(std::ptr::null_mut(), Ordering::AcqRel);
    if !original_fp.is_null() {
        unsafe {
            memory::write_ptr(actor.add(fp_offset), original_fp);
        }
    }
}

/// Initialise the service: resolve the stock `AutoFootPanel` pieces, allocate
/// the shared stock-shaped object, and register the single pre/post pair on
/// the judge dispatcher. Fail-open: any missing prerequisite logs one WARN and
/// leaves `is_available() == false` (autoplay and multiplayer-bot then refuse
/// to init).
pub fn init(signatures: &SignatureStore) -> bool {
    if AVAILABLE.load(Ordering::Acquire) {
        return true;
    }
    if !judge_hook::is_available() {
        log_warn!("FootPanelSwap: judge_hook dispatcher unavailable -- service disabled");
        return false;
    }
    let Some(fp_offset) = judge_hook::foot_panel_offset() else {
        log_warn!(
            "FootPanelSwap: judge_hook did not detect the foot panel offset -- service disabled"
        );
        return false;
    };
    let Some(vtable) = signatures.get_address("auto_foot_panel_vtable") else {
        log_warn!("FootPanelSwap: auto_foot_panel_vtable not resolved -- service disabled");
        return false;
    };
    let Some(update_addr) = signatures.get_address("auto_foot_panel_update") else {
        log_warn!("FootPanelSwap: auto_foot_panel_update not resolved -- service disabled");
        return false;
    };

    let panel = unsafe { memory::alloc_zeroed(PANEL_OBJECT_SIZE) };
    if panel.is_null() {
        log_warn!("FootPanelSwap: failed to allocate the AutoFootPanel object -- service disabled");
        return false;
    }
    unsafe {
        memory::write_ptr(panel, vtable);
    }
    // The bot objects are optional: a failure here leaves `Bot` unarmable
    // (multiplayer-bot degrades to "no bot") while `Perfect` still works.
    if let Some(objs) = build_bot_objects(vtable) {
        BOT_PANEL[0].store(objs[0], Ordering::Release);
        BOT_PANEL[1].store(objs[1], Ordering::Release);
    }

    let Some(pre) = judge_hook::register_pre(Priority::Late, swap_in) else {
        log_warn!("FootPanelSwap: pre-judge registration failed -- service disabled");
        return false;
    };
    let Some(_post) = judge_hook::register_post(Priority::Early, swap_out) else {
        judge_hook::unregister(pre);
        log_warn!("FootPanelSwap: post-judge registration failed -- service disabled");
        return false;
    };

    FOOT_PANEL_OFFSET.store(fp_offset, Ordering::Release);
    STOCK_PANEL.store(panel, Ordering::Release);
    STOCK_UPDATE.store(update_addr as *mut (), Ordering::Release);
    AVAILABLE.store(true, Ordering::Release);
    log_info!(
        "FootPanelSwap: registered judge swap (foot panel offset 0x{:X}, panel object {} bytes, bot objects {})",
        fp_offset,
        PANEL_OBJECT_SIZE,
        if bot_objects_ready() { "ready" } else { "UNAVAILABLE" }
    );
    true
}

/// Whether the cloned-vtable bot objects exist (the `Bot` controller can be
/// armed).
pub fn bot_objects_ready() -> bool {
    !BOT_PANEL[0].load(Ordering::Acquire).is_null()
        && !BOT_PANEL[1].load(Ordering::Acquire).is_null()
}

/// Whether the swap pair is registered and the stock object exists.
pub fn is_available() -> bool {
    AVAILABLE.load(Ordering::Acquire)
}

/// Autoplay's request for `side`. Ignored in effect (not in state) while a bot
/// is armed on that side — the request is remembered and takes over again when
/// the bot disarms.
pub fn set_perfect(side: usize, on: bool) {
    if side < 2 {
        PERFECT[side].store(on, Ordering::Release);
    }
}

/// Arm the bot controller on `side` with `fill` as its per-frame producer.
/// Returns `false` when the service is unavailable or `side` is out of range.
pub fn arm_bot(side: usize, fill: BotFillFn) -> bool {
    if side >= 2 || !is_available() || !bot_objects_ready() {
        return false;
    }
    BOT_FILL[side].store(fill as *mut (), Ordering::Release);
    BOT_UNWIRED_WARNED[side].store(false, Ordering::Release);
    BOT_ARMED[side].store(true, Ordering::Release);
    if PERFECT[side].load(Ordering::Acquire) {
        log_info!(
            "FootPanelSwap: bot controller takes precedence over autoplay on side {}",
            side
        );
    }
    true
}

/// Disarm the bot controller on `side`; a pending `Perfect` request (if any)
/// resumes.
pub fn disarm_bot(side: usize) {
    if side < 2 {
        BOT_ARMED[side].store(false, Ordering::Release);
        BOT_FILL[side].store(std::ptr::null_mut(), Ordering::Release);
        BOT_UNWIRED_WARNED[side].store(false, Ordering::Release);
    }
}

/// The controller in effect for `side` (Bot > Perfect > Off). Out-of-range
/// sides report `Off`.
pub fn controller(side: usize) -> Controller {
    if side >= 2 {
        return Controller::Off;
    }
    effective_controller(
        BOT_ARMED[side].load(Ordering::Acquire),
        PERFECT[side].load(Ordering::Acquire),
    )
}
