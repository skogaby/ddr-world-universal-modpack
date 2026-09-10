//! 2-Player BPL Mode — the in-shop battle (BPL) gameplay HUD for ordinary
//! local 2-player versus play on ONE cabinet.
//!
//! The stock "in-shop battle" HUD (per-player score boards, score-ratio
//! gauges, live 1st/2nd rank badges, score-margin readout) is ONE
//! self-contained game actor, `sequence::dance::MatchingBattleFrameActor`,
//! that `MatchingDancePlaySequence` creates as a sibling of the two
//! `GamePlayActor`s. Its art (`dance_matching`) is already resident in normal
//! gameplay and its screen anchor (`matching_usr`) is registered by the same
//! `LayoutActor` builder both play sequences use — only its DATA source is
//! network-bound (it reads scores from `CNetworkManager`'s cabinet blocks,
//! which are null in local play).
//!
//! Mechanism (zero detours, zero byte patches): at the first GAMEPLAY frame
//! where the normal `DancePlaySequence` has its `LayoutActor` and both
//! `GamePlayActor`s, allocate the stock actor from the AGCS app heap, run the
//! STOCK constructor, install a mod-owned 9-slot clone of the class vtable
//! with two slots replaced, fill the participant table, and attach it with the
//! game's own `Actor::addChild`:
//!
//! * slot 4 `onInitialize` — wraps stock with a scope-guarded `GameWork+0 → 0`
//!   flip so it builds the 2-participant `main_single` layout (hides the 3P/4P
//!   boards, picks the `_single` gauge art). Pre-checks that `dance_matching`
//!   is resident and neutralises the actor otherwise (stock would NULL-deref).
//! * slot 6 `onUpdate` — replaced: target score per board from the two live
//!   `GamePlayActor`s (the game's own isEx-selected counter), then the stock
//!   smoothing arithmetic and a tail call into the stock rank/diff function.
//!
//! Teardown is the parent's: the DPS's destruction runs the stock deleting
//! dtor → `agcs_heap_free` on our allocation (allocator matched by
//! construction). One frame per DPS INSTANCE (in-place `song_reset` restarts
//! keep the DPS and the frame; a quick-restart `finish` builds a fresh DPS and
//! gets a fresh frame). Everything fails open: any gate/derivation miss ⇒ no
//! frame, one latched WARN per class, game untouched.
//!
//! Gates: `GameWork+0 == 1` (versus) ∧ both sides entered ∧ event mode ∉ {1,2}
//! (a real BPL session already has the frame) ∧ not course ∧ GAMEPLAY ∧
//! network idle (`CNetworkManager` local cabinet index == −1 — the stock ctor
//! indexes the cabinet-block array with it unchecked) ∧ package resident.
//!
//! Planning: `.agents/planning/2026-09-09-two-player-bpl-mode/`. RE:
//! `docs/in_shop_battle_local_versus_research.md`.

pub mod logic;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Arc, OnceLock};

use crate::core::memory;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{input_manager, scene_manager, song_reset, stage_records};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use logic::{Gate, GateInputs};

// ── Actor layout (byte-identical on all four supported builds; the ctor
//    signature is the attestation) ─────────────────────────────────────
/// Stock allocation size of `MatchingBattleFrameActor`.
const FRAME_SIZE: usize = 0x280;
/// Mod allocation: the actor + the `GamePlayActor*[2]` array the ctor's
/// `actors` argument points at (its lifetime must equal the actor's).
const ALLOC_SIZE: usize = FRAME_SIZE + 0x10;
const ACTORS_ARRAY_OFFSET: usize = FRAME_SIZE;
/// `agcs::Actor` message flags word (ctor = 3: bit0 update, bit1 draw, bit8
/// initialised — set by the dispatcher after slot 4 returns).
const ACTOR_MSG_FLAGS: usize = 0x50;
const ACTOR_PARENT: usize = 0x08;
/// `max_score` (0 until onInitialize computed it).
const FRAME_MAX_SCORE: usize = 0x90;
/// `BATTLE_INFO[4]`, 0x40 stride.
const FRAME_BATTLE_INFO: usize = 0xB0;
const BATTLE_INFO_STRIDE: usize = 0x40;
/// Participant count (2 for the mod).
const FRAME_PARTICIPANTS: usize = 0x1B0;
/// `GamePlayActor**` the ctor stored.
const FRAME_ACTORS_PTR: usize = 0x278;
/// `LayoutActor + 0x98` = the layout descriptor the ctor receives.
const LAYOUT_DESC_OFFSET: usize = 0x98;

// BATTLE_INFO fields.
const BI_PLAYER_INDEX: usize = 0x08;
const BI_DDRCODE: usize = 0x0C;
const BI_NAME: usize = 0x10;
const BI_TEAM_ID: usize = 0x20;
const BI_SCORE_TARGET: usize = 0x24;
const BI_SCORE_DISPLAY: usize = 0x28;
const BI_SCORE_DIFF: usize = 0x2C;
const BI_RANK: usize = 0x30;
const BI_GAUGE: usize = 0x34;
const BI_POSITION_MAP: usize = 0x38;
const BI_IS_EX: usize = 0x3C;

// PlayerWork fields (shared facts: stage_records reads +0x4, persistence +0x18).
const PW_ENTERED: usize = 0x04;
const PW_NAME: usize = 0x0C;
const PW_NAME_LEN: usize = 12; // up to the ddrcode at +0x18
const PW_DDRCODE: usize = 0x18;

// Stage record header (premium_free/ghost_cache convention).
const REC_MCODE: usize = 0x00;
const REC_DIFF: usize = 0x04;

/// Upper bound on the DPS child walk (corruption guard).
const MAX_CHILD_WALK: usize = 256;

type FrameCtor = unsafe extern "C" fn(
    this: *mut u8,
    layout_desc: *const u8,
    actors: *const *mut u8,
    is_ex: u8,
    is_double: i32,
    mcode: i32,
    diff: i32,
) -> *mut u8;
type AddChild = unsafe extern "C" fn(parent: *mut u8, child: *mut u8);
type HeapMalloc = unsafe extern "C" fn(*const u8, usize, usize, usize) -> *mut u8;
type ActorSlot = unsafe extern "C" fn(this: *mut u8);

/// Every resolved address the mod uses, frozen at `init`.
struct Sites {
    ctor: FrameCtor,
    add_child: AddChild,
    rank_fn: ActorSlot,
    stock_on_initialize: ActorSlot,
    stock_vtable: *const *const u8,
    layout_vtable: *const u8,
    gpa_vtable: *const u8,
    heap_malloc: HeapMalloc,
    /// Global holding the AGCS app-heap handle (deref at use).
    heap_handle: *const *const u8,
    /// `CNetworkManager` local cabinet index (i32; −1 = no session).
    local_cab_idx: *const i32,
    /// Scene resource manager global (pointer-to-pointer).
    scene_res_mgr: *const *const u8,
    slot_off: usize,
    is_ex_off: usize,
    ex_off: usize,
    money_off: usize,
}
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
/// The installed vtable pointer (`image + 8`; `[-1]` = COL).
static CLONE_VTABLE: AtomicPtr<*const u8> = AtomicPtr::new(std::ptr::null_mut());
static ENABLED: AtomicBool = AtomicBool::new(false);
/// GAMEPLAY entered and no placement decision made yet for this play.
static ARMED: AtomicBool = AtomicBool::new(false);
/// The DPS that already carries a frame (per-instance latch).
static DONE_FOR_DPS: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
/// Dev-mode diagnostic: log the would-be placement, never allocate.
static DRY_RUN: AtomicBool = AtomicBool::new(false);

// One latched WARN per failure class.
static WARN_GATE_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static WARN_NETWORK_NOT_IDLE: AtomicBool = AtomicBool::new(false);
static WARN_PACKAGE_MISSING: AtomicBool = AtomicBool::new(false);
static WARN_ALLOC_FAILED: AtomicBool = AtomicBool::new(false);
static WARN_ATTACH_FAILED: AtomicBool = AtomicBool::new(false);
static WARN_INIT_PANIC: AtomicBool = AtomicBool::new(false);
static WARN_UPDATE_PANIC: AtomicBool = AtomicBool::new(false);
static WARN_INPUTS: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::AcqRel) {
        log_warn!("TwoPlayerBpl: {}", msg);
    }
}

pub struct TwoPlayerBplMod {
    resolved: bool,
    scene_cb: Option<usize>,
    frame_cb: Option<usize>,
}

impl TwoPlayerBplMod {
    pub fn new() -> Self {
        Self {
            resolved: false,
            scene_cb: None,
            frame_cb: None,
        }
    }
}

impl Default for TwoPlayerBplMod {
    fn default() -> Self {
        Self::new()
    }
}

impl Mod for TwoPlayerBplMod {
    fn id(&self) -> &str {
        "two-player-bpl-mode"
    }

    fn name(&self) -> &str {
        "2-Player BPL Mode"
    }

    fn description(&self) -> &str {
        "Shows the in-shop battle (BPL) score boards, ratio gauges and live rank badges during local 2-player versus play"
    }

    fn required_signatures(&self) -> &[&str] {
        &[
            "battle_frame_ctor",
            "actor_add_child",
            "dance_matching_slot_probe",
            "gpa_score_select",
            "battle_frame_actor_vtable",
            "layout_actor_vtable",
            "battle_frame_rank_fn",
            "matching_local_cabinet_idx",
            "scene_resource_manager",
            "dance_matching_slot_off",
            "gpa_is_ex_off",
            "gpa_ex_score_off",
            "gpa_money_score_off",
            "gameplay_actor_vtable",
            "agcs_heap_malloc",
            "app_heap_handle",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let sig = ctx.signatures;
        let Some(slot_off) = sig.dance_matching_slot_off() else {
            log_warn!("TwoPlayerBpl: dance_matching slot offset unpublished");
            return false;
        };
        let Some((is_ex_off, ex_off, money_off)) = sig.gpa_score_offsets() else {
            log_warn!("TwoPlayerBpl: GamePlayActor score offsets unpublished");
            return false;
        };
        if !stage_records::is_available() || !stage_records::session_state_available() {
            log_warn!("TwoPlayerBpl: stage_records unavailable -- gates cannot be read");
            return false;
        }
        if !song_reset::is_available() {
            log_warn!("TwoPlayerBpl: song_reset unavailable -- no DPS/actor walk");
            return false;
        }

        let stock_vtable = sig.require_address("battle_frame_actor_vtable") as *const *const u8;
        let sites = unsafe {
            let slot = |n: usize| *stock_vtable.add(n);
            let on_init = slot(logic::SLOT_ON_INITIALIZE);
            if on_init.is_null() {
                log_warn!("TwoPlayerBpl: stock onInitialize slot is null");
                return false;
            }
            Sites {
                ctor: std::mem::transmute::<*const u8, FrameCtor>(
                    sig.require_address("battle_frame_ctor"),
                ),
                add_child: std::mem::transmute::<*const u8, AddChild>(
                    sig.require_address("actor_add_child"),
                ),
                rank_fn: std::mem::transmute::<*const u8, ActorSlot>(
                    sig.require_address("battle_frame_rank_fn"),
                ),
                stock_on_initialize: std::mem::transmute::<*const u8, ActorSlot>(on_init),
                stock_vtable,
                layout_vtable: sig.require_address("layout_actor_vtable"),
                gpa_vtable: sig.require_address("gameplay_actor_vtable"),
                heap_malloc: std::mem::transmute::<*const u8, HeapMalloc>(
                    sig.require_address("agcs_heap_malloc"),
                ),
                heap_handle: sig.require_address("app_heap_handle") as *const *const u8,
                local_cab_idx: sig.require_address("matching_local_cabinet_idx") as *const i32,
                scene_res_mgr: sig.require_address("scene_resource_manager") as *const *const u8,
                slot_off,
                is_ex_off,
                ex_off,
                money_off,
            }
        };

        // Build the vtable clone once (inert until installed into an instance).
        let clone = unsafe { build_clone_vtable(stock_vtable) };
        if clone.is_null() {
            log_warn!("TwoPlayerBpl: vtable clone allocation failed");
            return false;
        }
        if SITES.set(sites).is_err() {
            log_warn!("TwoPlayerBpl: init called twice");
            return false;
        }
        CLONE_VTABLE.store(clone, Ordering::Release);

        let dev_mode = crate::mods::config::get()
            .and_then(|c| c.layeredfs.as_ref())
            .map(|l| l.developer_mode)
            .unwrap_or(false);
        if dev_mode && std::env::var_os("DDR_BPL_DRY_RUN").is_some() {
            DRY_RUN.store(true, Ordering::Release);
            log_info!(
                "TwoPlayerBpl: DRY RUN (DDR_BPL_DRY_RUN) -- placement logged, never performed"
            );
        }

        self.resolved = true;
        log_info!(
            "TwoPlayerBpl: initialized (slot_off=0x{:X}, gpa isEx/ex/money=0x{:X}/0x{:X}/0x{:X})",
            slot_off,
            is_ex_off,
            ex_off,
            money_off
        );
        true
    }

    fn enable(&mut self) {
        if !self.resolved {
            return;
        }
        ENABLED.store(true, Ordering::Release);
        if self.scene_cb.is_none() {
            self.scene_cb = Some(scene_manager::on_scene_change(Box::new(|prev, next| {
                if next == scene::GAMEPLAY && prev != scene::GAMEPLAY {
                    DONE_FOR_DPS.store(std::ptr::null_mut(), Ordering::Release);
                    ARMED.store(ENABLED.load(Ordering::Acquire), Ordering::Release);
                } else if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
                    ARMED.store(false, Ordering::Release);
                }
            })));
        }
        if self.frame_cb.is_none() {
            self.frame_cb = Some(input_manager::on_frame(Arc::new(on_frame)));
        }
        log_info!("TwoPlayerBpl: enabled (battle HUD in local 2P versus; applies next song)");
    }

    fn disable(&mut self) {
        ENABLED.store(false, Ordering::Release);
        ARMED.store(false, Ordering::Release);
        if let Some(id) = self.frame_cb.take() {
            input_manager::remove_frame_callback(id);
        }
        if let Some(id) = self.scene_cb.take() {
            scene_manager::remove_callback(id);
        }
        log_info!("TwoPlayerBpl: disabled (an already-placed frame lives until its song ends)");
    }

    fn is_active(&self) -> bool {
        self.resolved
    }
}

// ── Frame driver ─────────────────────────────────────────────────────

enum Outcome {
    /// DPS / LayoutActor / GamePlayActors not present yet — retry next frame.
    NotReady,
    /// Terminal for this play (already logged where needed).
    Refused,
    /// Frame placed (or dry-run logged).
    Placed,
}

/// Per-frame (game thread). O(1) when disarmed.
fn on_frame() {
    if !ARMED.load(Ordering::Acquire) {
        return;
    }
    if !ENABLED.load(Ordering::Acquire) {
        ARMED.store(false, Ordering::Release);
        return;
    }
    match create_frame() {
        Outcome::NotReady => {}
        Outcome::Refused | Outcome::Placed => ARMED.store(false, Ordering::Release),
    }
}

/// One classified pass over the DPS's children.
struct Children {
    layout: *mut u8,
    gpas: [*mut u8; 2],
    frame_exists: bool,
}

/// Walk the DPS child list once, classifying by vtable identity. Returns
/// `None` until a LayoutActor and two side-distinct GamePlayActors exist.
unsafe fn classify_children(sites: &Sites, dps: *mut u8) -> Option<Children> {
    let clone = CLONE_VTABLE.load(Ordering::Acquire) as *const u8;
    let mut layout: *mut u8 = std::ptr::null_mut();
    let mut gpas: [*mut u8; 2] = [std::ptr::null_mut(); 2];
    let mut frame_exists = false;

    let mut child = memory::read_ptr(dps.add(song_reset::FIRST_CHILD_OFFSET)) as *mut u8;
    let mut walked = 0usize;
    while !child.is_null() && walked < MAX_CHILD_WALK {
        if !memory::is_readable(child, 0x90) {
            return None;
        }
        let vt = memory::read_ptr(child);
        if vt == sites.layout_vtable {
            layout = child;
        } else if vt == sites.gpa_vtable {
            let side = memory::read_i32(child.add(song_reset::GPA_SIDE_OFFSET));
            if (0..2).contains(&side) {
                gpas[side as usize] = child;
            }
        } else if vt == sites.stock_vtable as *const u8 || vt == clone {
            frame_exists = true;
        }
        child = memory::read_ptr(child.add(song_reset::NEXT_SIBLING_OFFSET)) as *mut u8;
        walked += 1;
    }
    if layout.is_null() || gpas[0].is_null() || gpas[1].is_null() {
        return None;
    }
    Some(Children {
        layout,
        gpas,
        frame_exists,
    })
}

/// The resident `dance_matching` package pointer, or null. Mirrors the stock
/// three-load chain exactly: `global → manager object → slot array (mgr+0) →
/// [slot_off]` (`MOV RAX,[rip]; MOV RCX,[RAX]; MOV R13,[RCX+0x7F0]`).
unsafe fn dance_matching_package(sites: &Sites) -> *const u8 {
    if !memory::is_readable(sites.scene_res_mgr as *const u8, 8) {
        return std::ptr::null();
    }
    let mgr = memory::read_ptr(sites.scene_res_mgr as *const u8);
    if mgr.is_null() || !memory::is_readable(mgr, 8) {
        return std::ptr::null();
    }
    let slots = memory::read_ptr(mgr);
    if slots.is_null() || !memory::is_readable(slots.add(sites.slot_off), 8) {
        return std::ptr::null();
    }
    memory::read_ptr(slots.add(sites.slot_off))
}

/// The stock player-name / ddrcode pair for a side (design R-BOARDS).
unsafe fn player_identity(side: usize) -> Option<([u8; logic::NAME_FIELD], i32)> {
    let pw = stage_records::player_work(side)?;
    if !memory::is_readable(pw, PW_DDRCODE + 4) {
        return None;
    }
    let entered = memory::read_u8(pw.add(PW_ENTERED)) != 0;
    let raw = std::slice::from_raw_parts(pw.add(PW_NAME) as *const u8, PW_NAME_LEN);
    let name = logic::player_name(entered, raw, side);
    let ddrcode = memory::read_i32(pw.add(PW_DDRCODE));
    Some((name, if ddrcode == 0 { -1 } else { ddrcode }))
}

fn create_frame() -> Outcome {
    let Some(sites) = SITES.get() else {
        return Outcome::Refused;
    };

    // 1. Session gate.
    let course_word = stage_records::game_work()
        .map(|gw| unsafe { memory::read_u64(gw.add(stage_records::course_field_offset())) });
    let inputs = GateInputs {
        versus: stage_records::game_work().map(|gw| unsafe { memory::read_i32(gw) }),
        entered: [
            stage_records::side_entered(0),
            stage_records::side_entered(1),
        ],
        event_mode: stage_records::event_mode(),
        course_word,
        // ARMED is only ever set inside GAMEPLAY (scene callback).
        scene_is_gameplay: true,
    };
    match logic::eligibility(&inputs) {
        Gate::Eligible => {}
        Gate::Ineligible(reason) => {
            log_info!("TwoPlayerBpl: no battle frame this song ({})", reason);
            return Outcome::Refused;
        }
        Gate::Unavailable(what) => {
            warn_once(
                &WARN_GATE_UNAVAILABLE,
                &format!("gate input unavailable ({what}) -- battle frame disabled"),
            );
            return Outcome::Refused;
        }
    }

    // 2. Actor tree.
    let Some(dps) = song_reset::live_dps() else {
        return Outcome::NotReady;
    };
    if dps == DONE_FOR_DPS.load(Ordering::Acquire) {
        return Outcome::Refused;
    }
    let Some(children) = (unsafe { classify_children(sites, dps) }) else {
        return Outcome::NotReady;
    };
    if children.frame_exists {
        return Outcome::Refused;
    }

    // 3. Network idle (the stock ctor indexes the cabinet-block array with it).
    let cab_idx = unsafe {
        if memory::is_readable(sites.local_cab_idx as *const u8, 4) {
            Some(*sites.local_cab_idx)
        } else {
            None
        }
    };
    if cab_idx != Some(-1) {
        warn_once(
            &WARN_NETWORK_NOT_IDLE,
            &format!(
                "matching network not idle (local cabinet idx {:?}) -- battle frame skipped",
                cab_idx
            ),
        );
        return Outcome::Refused;
    }

    // 4. Package residency (early exit; the slot-4 wrapper re-checks).
    let pkg = unsafe { dance_matching_package(sites) };
    if pkg.is_null() {
        warn_once(
            &WARN_PACKAGE_MISSING,
            "dance_matching package not resident -- battle frame skipped",
        );
        return Outcome::Refused;
    }

    // 5. Inputs.
    let Some(stage) = stage_records::stage_counter() else {
        warn_once(
            &WARN_INPUTS,
            "stage counter unavailable -- battle frame skipped",
        );
        return Outcome::Refused;
    };
    let Some(rec) = stage_records::stage_record(0, stage.max(0) as usize) else {
        warn_once(
            &WARN_INPUTS,
            "stage record unavailable -- battle frame skipped",
        );
        return Outcome::Refused;
    };
    let (mcode, diff, is_ex) = unsafe {
        if !memory::is_readable(rec, 8)
            || !memory::is_readable(children.gpas[0], sites.is_ex_off + 1)
        {
            warn_once(
                &WARN_INPUTS,
                "record / actor unreadable -- battle frame skipped",
            );
            return Outcome::Refused;
        }
        (
            memory::read_i32(rec.add(REC_MCODE)),
            memory::read_i32(rec.add(REC_DIFF)),
            memory::read_u8(children.gpas[0].add(sites.is_ex_off)),
        )
    };
    let (Some(id0), Some(id1)) = (unsafe { player_identity(0) }, unsafe { player_identity(1) })
    else {
        warn_once(
            &WARN_INPUTS,
            "player identity unreadable -- battle frame skipped",
        );
        return Outcome::Refused;
    };
    let name_str = |n: &[u8; logic::NAME_FIELD]| {
        let end = n.iter().position(|&b| b == 0).unwrap_or(n.len());
        String::from_utf8_lossy(&n[..end]).into_owned()
    };

    if DRY_RUN.load(Ordering::Acquire) {
        log_info!(
            "TwoPlayerBpl (dry run): would place frame dps={:p} layout={:p} gpa=[{:p},{:p}] is_ex={} mcode={} diff={} names=[{},{}] pkg={:p}",
            dps,
            children.layout,
            children.gpas[0],
            children.gpas[1],
            is_ex,
            mcode,
            diff,
            name_str(&id0.0),
            name_str(&id1.0),
            pkg
        );
        DONE_FOR_DPS.store(dps, Ordering::Release);
        return Outcome::Placed;
    }

    // 6–9. Allocate, construct, own, attach.
    let clone = CLONE_VTABLE.load(Ordering::Acquire);
    if clone.is_null() {
        return Outcome::Refused;
    }
    unsafe {
        if !memory::is_readable(sites.heap_handle as *const u8, 8) {
            warn_once(
                &WARN_ALLOC_FAILED,
                "app heap handle unreadable -- battle frame skipped",
            );
            return Outcome::Refused;
        }
        let handle = *sites.heap_handle;
        let frame = (sites.heap_malloc)(handle, ALLOC_SIZE, 0, 0);
        if frame.is_null() {
            warn_once(
                &WARN_ALLOC_FAILED,
                "agcs_heap_malloc failed -- battle frame skipped",
            );
            return Outcome::Refused;
        }
        std::ptr::write_bytes(frame, 0, ALLOC_SIZE);

        // The actors array lives inside our allocation, ordered by side.
        let actors = frame.add(ACTORS_ARRAY_OFFSET) as *mut *mut u8;
        *actors = children.gpas[0];
        *actors.add(1) = children.gpas[1];

        (sites.ctor)(
            frame,
            children.layout.add(LAYOUT_DESC_OFFSET),
            actors as *const *mut u8,
            is_ex,
            0, // single (not doubles) layout
            mcode,
            diff,
        );

        // Own it: clone vtable, 2 participants, our BATTLE_INFO.
        memory::write_ptr(frame, clone as *const u8);
        memory::write_i32(frame.add(FRAME_PARTICIPANTS), 2);
        for (i, (name, ddrcode)) in [id0, id1].iter().enumerate() {
            let bi = frame.add(FRAME_BATTLE_INFO + i * BATTLE_INFO_STRIDE);
            memory::write_i32(bi.add(BI_PLAYER_INDEX), i as i32);
            memory::write_i32(bi.add(BI_DDRCODE), *ddrcode);
            std::ptr::copy_nonoverlapping(name.as_ptr(), bi.add(BI_NAME), logic::NAME_FIELD);
            memory::write_i32(bi.add(BI_TEAM_ID), 0);
            memory::write_i32(bi.add(BI_SCORE_TARGET), 0);
            memory::write_i32(bi.add(BI_SCORE_DISPLAY), 0);
            memory::write_i32(bi.add(BI_SCORE_DIFF), 0);
            memory::write_i32(bi.add(BI_RANK), -1);
            memory::write_f32(bi.add(BI_GAUGE), 0.0);
            memory::write_i32(bi.add(BI_POSITION_MAP), i as i32);
            memory::write_u8(bi.add(BI_IS_EX), is_ex);
        }
        for i in 2..4 {
            let bi = frame.add(FRAME_BATTLE_INFO + i * BATTLE_INFO_STRIDE);
            memory::write_i32(bi.add(BI_POSITION_MAP), -1);
            memory::write_i32(bi.add(BI_RANK), -1);
        }

        (sites.add_child)(dps, frame);
        if memory::read_ptr(frame.add(ACTOR_PARENT)) != dps as *const u8 {
            // addChild refused silently. The object is fully constructed;
            // leaking 0x290 bytes once beats freeing a live actor.
            warn_once(
                &WARN_ATTACH_FAILED,
                "Actor::addChild refused the battle frame -- skipped (allocation leaked)",
            );
            return Outcome::Refused;
        }
    }

    DONE_FOR_DPS.store(dps, Ordering::Release);
    log_info!(
        "TwoPlayerBpl: battle frame placed (dps={:p}, is_ex={}, mcode={}, diff={}, names=[{},{}])",
        dps,
        is_ex,
        mcode,
        diff,
        name_str(&id0.0),
        name_str(&id1.0)
    );
    Outcome::Placed
}

// ── Vtable clone + replaced slots ────────────────────────────────────

/// Clone the stock 9-slot vtable into mod memory, COL at `[-1]`, slots 4/6
/// replaced. Returns the pointer to install (`image + 8`), or null.
unsafe fn build_clone_vtable(stock: *const *const u8) -> *mut *const u8 {
    let slot_size = std::mem::size_of::<usize>();
    let mut donor = [0usize; logic::VTABLE_SLOTS];
    for (i, d) in donor.iter_mut().enumerate() {
        *d = *stock.add(i) as usize;
    }
    let col = *stock.offset(-1) as usize;
    let image = logic::clone_vtable_image(
        &donor,
        col,
        on_initialize_wrapper as *const () as usize,
        on_update_replacement as *const () as usize,
    );
    let raw = memory::alloc_zeroed((logic::VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }
    let backing = raw as *mut usize;
    for (i, v) in image.iter().enumerate() {
        *backing.add(i) = *v;
    }
    (backing as *mut *const u8).add(1)
}

/// Restores `GameWork+0` on every exit path of the slot-4 wrapper.
struct RestoreWord {
    ptr: *mut u8,
    value: i32,
}
impl Drop for RestoreWord {
    fn drop(&mut self) {
        unsafe { memory::write_i32(self.ptr, self.value) };
    }
}

/// Slot 4 — `onInitialize(this)`. Presents `GameWork+0 == 0` to the stock
/// implementation so it builds the 2-participant layout; refuses to run stock
/// at all when `dance_matching` is not resident (stock NULL-derefs).
unsafe extern "C" fn on_initialize_wrapper(this: *mut u8) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(sites) = SITES.get() else {
            return;
        };
        let neutralise = |why: &str| {
            // Clear update+draw bits: slots 6/7 must never run without a layer.
            let flags = memory::read_u32(this.add(ACTOR_MSG_FLAGS));
            memory::write_u32(this.add(ACTOR_MSG_FLAGS), flags & !0x3);
            warn_once(&WARN_PACKAGE_MISSING, why);
        };
        if dance_matching_package(sites).is_null() {
            neutralise("dance_matching not resident at onInitialize -- frame neutralised");
            return;
        }
        let Some(gw) = stage_records::game_work() else {
            neutralise("GameWork unavailable at onInitialize -- frame neutralised");
            return;
        };
        let saved = memory::read_i32(gw);
        let _restore = RestoreWord {
            ptr: gw,
            value: saved,
        };
        memory::write_i32(gw, 0);
        (sites.stock_on_initialize)(this);
    }));
    if result.is_err() {
        warn_once(
            &WARN_INIT_PANIC,
            "panic inside onInitialize wrapper (contained)",
        );
    }
}

/// Slot 6 — `onUpdate(this)`. Stock minus the network read: target from the
/// isEx-selected `GamePlayActor` counter, stock smoothing, stock rank fn.
unsafe extern "C" fn on_update_replacement(this: *mut u8) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(sites) = SITES.get() else {
            return;
        };
        let actors = memory::read_ptr(this.add(FRAME_ACTORS_PTR)) as *const *mut u8;
        let n = memory::read_i32(this.add(FRAME_PARTICIPANTS)).clamp(0, 4) as usize;
        for i in 0..n {
            let bi = this.add(FRAME_BATTLE_INFO + i * BATTLE_INFO_STRIDE);
            let mut target = 0;
            if !actors.is_null() && i < 2 {
                let gpa = *actors.add(i);
                if !gpa.is_null()
                    && memory::is_readable(gpa, sites.ex_off.max(sites.money_off) + 4)
                    && memory::read_ptr(gpa) == sites.gpa_vtable
                {
                    let off = if memory::read_u8(gpa.add(sites.is_ex_off)) != 0 {
                        sites.ex_off
                    } else {
                        sites.money_off
                    };
                    target = memory::read_i32(gpa.add(off));
                }
            }
            memory::write_i32(bi.add(BI_SCORE_TARGET), target);
        }
        let max = memory::read_i32(this.add(FRAME_MAX_SCORE));
        if max != 0 {
            for i in 0..n {
                let bi = this.add(FRAME_BATTLE_INFO + i * BATTLE_INFO_STRIDE);
                let target = memory::read_i32(bi.add(BI_SCORE_TARGET));
                let display = memory::read_i32(bi.add(BI_SCORE_DISPLAY));
                let next = logic::smooth(display, target);
                memory::write_i32(bi.add(BI_SCORE_DISPLAY), next);
                memory::write_f32(bi.add(BI_GAUGE), logic::gauge_fraction(next, max));
            }
        }
        (sites.rank_fn)(this);
    }));
    if result.is_err() {
        warn_once(
            &WARN_UPDATE_PANIC,
            "panic inside onUpdate replacement (contained)",
        );
    }
}
