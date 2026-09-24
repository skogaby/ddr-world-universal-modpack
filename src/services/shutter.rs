//! The game's `ShutterActor` (the stage-jacket panel before a song and the
//! CLEARED / FAILED banners after it): read its state, dismiss the stage
//! panel with the game's own bannerless message, and unblock the drain.
//!
//! Promoted from `quick_restart_or_fail` (its bannerless fast paths are the
//! original consumer; DDR SELECTION's legacy intro is the second). Behaviour
//! is unchanged; callers own their logging.
//!
//! RE: `docs/quick_restart_fail_speedup_research.md` §4a (state machine),
//! §4b (msg `0x100c`), §14.1 (the drain unblock).
//!
//! * Singleton = `*shutter_actor_global` (derived from the
//!   `shutter_close_request` AOB).
//! * Embedded `agcs::StackStep`: values at `+0x58 + idx*8`, depth index u16 at
//!   `+0x82`.
//! * Active / pending kind + the stage-panel kind id are per build
//!   (`shutter_actor_layout`: `+0x310/+0x314`, kind 3 on 20260324+;
//!   `+0x2E0/+0x2E4`, kind 1 on 20250805 / 20260224).
//! * States: 0 idle, 1–3 art load / swap, 4 covered (the READY window),
//!   5 the `stage_out` reveal, 6 parked after the reveal (mid-song), 7 the
//!   drain entered by `0x100c`, 8 wait `out_end`/`end` → release → 0.
//!
//! Game thread only (the actor is a game object mutated by its own update).

use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

use crate::core::signatures::{ShutterActorLayout, SignatureStore};
use crate::core::{memory, module_resolver};
use crate::services::bm2d_api;

const STEP_BASE: usize = 0x58;
const STEP_INDEX: usize = 0x82;
pub const STATE_IDLE: i32 = 0;
pub const STATE_COVERED: i32 = 4;
pub const STATE_REVEALING: i32 = 5;
pub const STATE_PARKED_REVEALED: i32 = 6;
pub const STATE_DRAIN_TAIL: i32 = 7;
const STATE_MAX: i32 = 8;
/// The bannerless stage-panel dismiss: the handler forces state 7 iff the
/// active kind is the stage kind (it does not check the state — the state-7
/// drain replays `stage_out` if it never ran), pending kind untouched.
const MSG_DISMISS_STAGE: i32 = 0x100C;
const TREE_FLAGS_OFFSET: usize = 0x20;
const TREE_FLAGS_DISPATCH_SUPPRESSED: u32 = 0x20;
/// `agcs::Actor::onMessage(this, msg, param)` vtable slot.
const VTBL_ON_MESSAGE_OFFSET: usize = 0x18;
/// Per-kind `shared_ptr<Layer>` table: layer object at `+0x88 + kind*0x10`.
const LAYER_TABLE_OFFSET: usize = 0x88;
/// AFP MovieClip id on the layer object (the id every shutter wait reads).
const LAYER_MC_ID_OFFSET: usize = 0x110;
/// `afp_mc_op` SetFrame.
const MC_OP_SET_FRAME: i32 = 0xF08;

type OnMessageFn = unsafe extern "C" fn(*mut u8, i32, *mut u8) -> i32;

static GLOBAL: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static LAYOUT: OnceLock<ShutterActorLayout> = OnceLock::new();

/// Resolve the singleton global and the per-build layout. `false` when either
/// is missing (every read then fails and callers fall back).
pub fn init(signatures: &SignatureStore) -> bool {
    if let Some(addr) = signatures.get_address("shutter_actor_global") {
        GLOBAL.store(addr as *mut u8, Ordering::Release);
    }
    if let Some(layout) = signatures.shutter_actor_layout() {
        let _ = LAYOUT.set(layout);
    }
    is_available()
}

pub fn global_available() -> bool {
    !GLOBAL.load(Ordering::Acquire).is_null()
}

pub fn layout_available() -> bool {
    LAYOUT.get().is_some()
}

pub fn is_available() -> bool {
    global_available() && layout_available()
}

/// The stage-panel kind id on this build.
pub fn stage_kind() -> Option<i32> {
    LAYOUT.get().map(|l| l.stage_kind)
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub actor: *mut u8,
    pub state: i32,
    pub active_kind: i32,
    pub pending_kind: i32,
}

impl Snapshot {
    /// The stage panel is the active kind and nothing else is queued.
    pub fn stage_panel_alone(&self) -> bool {
        Some(self.active_kind) == stage_kind() && self.pending_kind < 0
    }
    pub fn fully_idle(&self) -> bool {
        self.state == STATE_IDLE && self.active_kind < 0 && self.pending_kind < 0
    }
}

/// Read state / kinds, range-validating every value (a layout drift must read
/// as "unknown", never as a plausible state). `Ok(None)` = no shutter actor.
pub fn snapshot() -> Result<Option<Snapshot>, &'static str> {
    let global = GLOBAL.load(Ordering::Acquire);
    if global.is_null() {
        return Err("shutter global unresolved");
    }
    if LAYOUT.get().is_none() {
        return Err("shutter layout underived");
    }
    unsafe {
        let actor = *(global as *const *mut u8);
        if actor.is_null() {
            return Ok(None);
        }
        snapshot_of(actor).map(Some)
    }
}

/// [`snapshot`] of a known ShutterActor (`this` inside its own update).
pub fn snapshot_of(actor: *mut u8) -> Result<Snapshot, &'static str> {
    let Some(layout) = LAYOUT.get() else {
        return Err("shutter layout underived");
    };
    if actor.is_null() {
        return Err("null shutter");
    }
    unsafe {
        let idx = *(actor.add(STEP_INDEX) as *const u16) as usize;
        if idx >= 5 {
            return Err("step index out of range");
        }
        let state = *(actor.add(STEP_BASE + idx * 8) as *const i32);
        let active_kind = *(actor.add(layout.active_kind) as *const i32);
        let pending_kind = *(actor.add(layout.pending_kind) as *const i32);
        if !(0..=STATE_MAX).contains(&state)
            || !(-1..=STATE_MAX).contains(&active_kind)
            || !(-1..=STATE_MAX).contains(&pending_kind)
        {
            return Err("state/kind fields out of range");
        }
        Ok(Snapshot {
            actor,
            state,
            active_kind,
            pending_kind,
        })
    }
}

/// One kind's CMovieClip in the per-kind layer table: the object (a live
/// game object — call its vtable, never free it), its AFP layer id and its
/// root MovieClip id. `None` for an empty slot or a zero id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindClip {
    pub object: *mut u8,
    pub layer: u32,
    pub mc: u32,
}

/// The CMovieClip of `kind` (0..=8), or `None`.
pub fn kind_clip(actor: *mut u8, kind: i32) -> Option<KindClip> {
    if actor.is_null() || !(0..=STATE_MAX).contains(&kind) {
        return None;
    }
    unsafe {
        let object = *(actor.add(LAYER_TABLE_OFFSET + kind as usize * 0x10) as *const *mut u8);
        if object.is_null() || !memory::is_readable(object, LAYER_MC_ID_OFFSET + 4) {
            return None;
        }
        let layer = *(object.add(LAYER_ID_OFFSET) as *const u32);
        let mc = *(object.add(LAYER_MC_ID_OFFSET) as *const u32);
        (layer != 0 && mc != 0).then_some(KindClip { object, layer, mc })
    }
}

/// Layer id on the CMovieClip wrapper (`+0x08`, what `afp_layer_*` take).
const LAYER_ID_OFFSET: usize = 0x08;

/// The active kind's AFP layer id (for `afp_layer_mc_refer` child lookups).
pub fn active_clip_layer(s: &Snapshot) -> Option<u32> {
    if s.active_kind < 0 {
        return None;
    }
    unsafe {
        let layer = *(s
            .actor
            .add(LAYER_TABLE_OFFSET + s.active_kind as usize * 0x10)
            as *const *const u8);
        if layer.is_null() {
            return None;
        }
        let id = *(layer.add(LAYER_ID_OFFSET) as *const u32);
        (id != 0).then_some(id)
    }
}

/// The active kind's AFP MovieClip id (the clip every shutter wait reads).
pub fn active_clip_mc(s: &Snapshot) -> Option<u32> {
    if s.active_kind < 0 {
        return None;
    }
    unsafe {
        let layer = *(s
            .actor
            .add(LAYER_TABLE_OFFSET + s.active_kind as usize * 0x10)
            as *const *const u8);
        if layer.is_null() {
            return None;
        }
        let mc = *(layer.add(LAYER_MC_ID_OFFSET) as *const u32);
        (mc != 0).then_some(mc)
    }
}

#[derive(Debug)]
pub enum DismissError {
    /// The dispatch-suppressed tree flag is set (actor dying).
    DispatchSuppressed(u32),
    /// The vtable's onMessage lies outside gamemdx.
    BadOnMessage(*const u8),
    /// The handler did not write state 7.
    NotTaken { before: i32, after: i32 },
}

/// Send `0x100c` to a shutter whose stage panel is active alone in state
/// 4 / 5 / 6 and verify the synchronous state-7 write. The caller checks
/// those preconditions on `s` (they differ per consumer's refusal logging).
pub fn send_dismiss(s: &Snapshot) -> Result<(), DismissError> {
    unsafe {
        let flags = memory::read_u32(s.actor.add(TREE_FLAGS_OFFSET));
        if flags & TREE_FLAGS_DISPATCH_SUPPRESSED != 0 {
            return Err(DismissError::DispatchSuppressed(flags));
        }
        let vtable = *(s.actor as *const *const u8);
        let on_message_addr = *(vtable.add(VTBL_ON_MESSAGE_OFFSET) as *const *const u8);
        let in_module = module_resolver::get_game_module().is_some_and(|m| {
            let base = m.base as usize;
            let addr = on_message_addr as usize;
            addr > base && addr < base + m.size
        });
        if !in_module {
            return Err(DismissError::BadOnMessage(on_message_addr));
        }
        let on_message: OnMessageFn = std::mem::transmute(on_message_addr);
        on_message(s.actor, MSG_DISMISS_STAGE, std::ptr::null_mut());
        let idx = *(s.actor.add(STEP_INDEX) as *const u16) as usize;
        let after = *(s.actor.add(STEP_BASE + idx.min(4) * 8) as *const i32);
        if after != STATE_DRAIN_TAIL {
            return Err(DismissError::NotTaken {
                before: s.state,
                after,
            });
        }
    }
    Ok(())
}

/// Outcome of [`unblock_drain`] (for the caller's log line).
#[derive(Debug)]
pub enum Unblock {
    /// Nothing to do (no stage layer, no clip, or not the stage kind).
    NotNeeded,
    /// Both wait labels absent: the drain's target is 0 and completes alone.
    TargetZero {
        out_end: Option<u32>,
        end: Option<u32>,
    },
    FastForwarded {
        target: u32,
        out_end: Option<u32>,
        end: Option<u32>,
        ok: bool,
    },
}

/// Fast-forward the stage clip to the drain's own wait target
/// (`max(frame("out_end"), frame("end"))`) — the 2026-08-31 fix for art whose
/// `"out"` label play silently fails (the clip never advances and state 8
/// parks forever). On healthy art state 7's `"out"` play re-seeks anyway.
pub fn unblock_drain(actor: *mut u8) -> Unblock {
    let Some(layout) = LAYOUT.get() else {
        return Unblock::NotNeeded;
    };
    unsafe {
        let kind = *(actor.add(layout.active_kind) as *const i32);
        if kind != layout.stage_kind || kind < 0 {
            return Unblock::NotNeeded;
        }
        let layer = *(actor.add(LAYER_TABLE_OFFSET + kind as usize * 0x10) as *const *const u8);
        if layer.is_null() {
            return Unblock::NotNeeded;
        }
        let mc_id = *(layer.add(LAYER_MC_ID_OFFSET) as *const u32);
        if mc_id == 0 {
            return Unblock::NotNeeded;
        }
        let out_end = bm2d_api::mc_frame_by_label(mc_id, c"out_end");
        let end = bm2d_api::mc_frame_by_label(mc_id, c"end");
        let target = out_end.unwrap_or(0).max(end.unwrap_or(0));
        if target == 0 {
            return Unblock::TargetZero { out_end, end };
        }
        let ok = bm2d_api::mc_op(mc_id, MC_OP_SET_FRAME, target as i32);
        Unblock::FastForwarded {
            target,
            out_end,
            end,
            ok,
        }
    }
}
