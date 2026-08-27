//! Center Arrows for Single Player — centers the single-player playfield.
//!
//! Port of the 32-bit "center arrows" hex hack (see `docs/hex_edit_porting.md`,
//! Hack 2) to a 64-bit Rust hook. When the per-player option is enabled and the
//! session is single-player, the lone active side's lane-relative HUD elements
//! (arrow receptors, freeze judge, judge/combo/fast_slow/filter/score_compare)
//! are repositioned to the screen-center X. `score`/`gauge` are left in place.
//!
//! Mechanism (two detours on the gameplay HUD layout builder):
//!   1. `hud_layout_builder` entry — captures the builder root and computes
//!      `{single_player, active_side}` from the per-side play-states.
//!   2. `hud_layout_setter` (`set(parent, name, coord)`) — for the active
//!      single-player side, rewrites `coord[0]` (X) of the target keys to
//!      `CENTER_X`. The engine's own renderers read these stored coords and push
//!      them into the AFP layers, so the rewrite moves the rendered elements
//!      (Strategy A; confirmed by static RE — see research/r1).
//!
//! Gating: `single_player && side == active_side && style == single &&
//! option_enabled[side]`. The single-player condition is the hard gate —
//! centering never applies in 2P. The style condition (builder `+0x84+side*4`,
//! `0=single/1=double/2=absent`) excludes DOUBLES play: the game already
//! centers the 8-panel `double_lane_usr` lane itself, so the shift must only
//! apply to the side-offset single-style layout.
//!
//! See `.agents/planning/20260612-center-arrows-single/`.

use retour::GenericDetour;
use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::core::scanner::decode_rip_relative;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec};
use crate::{log_info, log_warn};

// ── Constants ───────────────────────────────────────────────────────

/// Custom-option id (distinct from the mod-registry id `center-arrows-single`).
/// Drives the row-label texture name `seop_item_center_arrows_1p` (see R4).
const OPTION_ID: &str = "center_arrows_1p";

/// Horizontal shift (layout units) that moves a side's playfield to screen
/// center. Derived empirically from a live 2P demo: every lane-relative element
/// is offset by EXACTLY the same spacing between P1 and P2 (`P2.x - P1.x = 719`
/// for arrow/arrow_raw/freeze_judge/judge/combo/filter/fast_slow/score_compare),
/// i.e. the two playfields are a rigid translation. Centering = move a side to
/// the midpoint, so the per-side shift is `719/2 = 359.5`, rounded to 360 (the
/// 0.5px is imperceptible and the whole group shifts rigidly, preserving every
/// element's relative alignment — which a flat absolute X did NOT).
///
/// P1 (left side) shifts +RIGHT; P2 (right side) shifts -LEFT. The game is
/// fixed-resolution (1280x720), so a constant is acceptable (Q5).
const LANE_SHIFT: i32 = 360;

/// Player-object array (resolved via `player_array_anchor`): two pointers,
/// P1 = `[0]`, P2 = `[1]` (at `+8`). Each points at a player object whose byte
/// at `+0x4` is the authoritative "this side is playing" flag — the same signal
/// the game's own per-side lamp/credit code gates on (verified live: the builder
/// object's `+0x80/+0x82/+0x84` fields are LayoutActor construction params, NOT
/// player count, so they read identically in 1P and 2P).
const PLAYER_PRESENT_OFFSET: usize = 0x4;

/// Builder object (LayoutActor): per-side layout parent at
/// `root + 0xE0 + side*0x48`. This is the `parent` (RCX) the setter receives, so
/// `side = (parent - (root+0xE0)) / 0x48`. (research/r2; side mapping verified
/// live — `side=Some(0)` resolved correctly.)
const PER_SIDE_PARENT_BASE: usize = 0xE0;
const PER_SIDE_STRIDE: usize = 0x48;

/// Builder object (LayoutActor): per-side play STYLE at `root + 0x84 + side*4`
/// (i32). `0` = single (side-offset `%dp_lane_usr` lane), `1` = double (the
/// centered `double_lane_usr` lane), `2` = side absent/skipped (per the
/// decompile's builder loop; never observed live — doubles reads `[1,1]` and
/// attract/singles read `[0,0]`, so don't rely on `2` marking an inactive
/// side). This is the exact field the builder's own lane-name selector
/// branches on (see `docs/hex_edit_porting.md`, Hack 2, and research/r2's
/// correction note). Used to suppress our shift for doubles: the game ALREADY
/// centers the 8-panel doubles lane, so shifting on top pushed it half off
/// the playfield (fixed 2026-07-19, cabinet-validated).
const PER_SIDE_STYLE_BASE: usize = 0x84;

/// `PER_SIDE_STYLE_BASE` value meaning "this side laid out with the side-offset
/// SINGLE style" — the only layout our centering shift is valid for.
const STYLE_SINGLE: i32 = 0;

/// Lane-relative element keys to recenter (Q1). `score`/`gauge`/`bpm`/`option`
/// and the lane-name keys are intentionally excluded.
///
/// `fullcombo` drives the end-of-song rocketship + "Fullcombo" accolade effect:
/// the FullcomboActor positions its AFP layer in its onCreate by reading the
/// `"fullcombo"` coord from this same map and calling setPositionXY, so shifting
/// that stored coord centers the effect for free (no separate hook needed).
/// Verified by RE; it was simply missing from the list initially, leaving the
/// effect at the side-offset position.
const TARGET_KEYS: &[&str] = &[
    "arrow_raw",
    "arrow",
    "freeze_judge",
    "judge",
    "combo",
    "fast_slow",
    "filter",
    "score_compare",
    "fullcombo",
];

// ── Pass state ──────────────────────────────────────────────────────
// Populated at builder entry, read by the setter hook within the same
// synchronous game-thread call stack. `static mut` + addr_of! matches the
// project's hook-state idiom; only ever touched on the game thread inside the
// nested builder→setter call, so no locking is required.
struct PassState {
    builder_root: usize,
    single_player: bool,
    active_side: u8, // 0 or 1 when single_player; 0xFF otherwise
    /// Per-side play STYLE (`root + 0x84 + side*4`): 0=single, 1=double,
    /// 2=absent. Read once per pass at builder entry.
    styles: [i32; 2],
}

static mut PASS_STATE: PassState = PassState {
    builder_root: 0,
    single_player: false,
    active_side: 0xFF,
    styles: [2, 2],
};

/// Per-player option mirror, written by the change callback. Read on the game
/// thread by the setter hook.
static OPTION_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Resolved address of the player-object array (set in `init` from
/// `player_array_anchor`). Stored as usize for atomic access; 0 = unresolved.
static PLAYER_ARRAY: AtomicU64 = AtomicU64::new(0);

static HOOKS_OK: AtomicBool = AtomicBool::new(false);

/// Last logged classification (packed). Sentinel u64::MAX = nothing logged yet,
/// so the first pass always logs; thereafter only transitions log.
static DIAG_LAST: AtomicU64 = AtomicU64::new(u64::MAX);

// ── Builder entry detour ────────────────────────────────────────────

/// Gameplay HUD/lane layout builder entry — `void(builder_root /*RCX*/)`.
/// (Resolved by the `hud_layout_builder` signature.)
type HudBuilderFn = unsafe extern "C" fn(*mut u8);
static mut HUD_BUILDER_HOOK: Option<GenericDetour<HudBuilderFn>> = None;

unsafe extern "C" fn hud_builder_hook(builder_root: *mut u8) {
    let _ = std::panic::catch_unwind(|| {
        compute_pass_state(builder_root);
    });
    if let Some(ref hook) = *std::ptr::addr_of!(HUD_BUILDER_HOOK) {
        hook.call(builder_root);
    }
}

/// Read per-side presence from the player-object array and classify the pass.
/// single_player := exactly one side present (`*(player[side] + 4) != 0`).
/// Also snapshots each side's play STYLE from the builder object (single vs
/// double vs absent) — the doubles gate for the setter hook.
fn compute_pass_state(builder_root: *mut u8) {
    if builder_root.is_null() {
        return;
    }

    let (p0_present, p1_present) = read_presence();
    let (single_player, active_side) = match (p0_present, p1_present) {
        (true, false) => (true, 0u8),
        (false, true) => (true, 1u8),
        _ => (false, 0xFFu8), // both present (2P) or neither
    };

    let styles = unsafe {
        [
            (builder_root.add(PER_SIDE_STYLE_BASE) as *const i32).read_unaligned(),
            (builder_root.add(PER_SIDE_STYLE_BASE + 4) as *const i32).read_unaligned(),
        ]
    };

    unsafe {
        let st = &mut *std::ptr::addr_of_mut!(PASS_STATE);
        st.builder_root = builder_root as usize;
        st.single_player = single_player;
        st.active_side = active_side;
        st.styles = styles;
    }

    // Log only when the classification changes (quiet in steady state; still
    // records 1P<->2P / side / style transitions for field debugging).
    let packed = ((p0_present as u64) << 2) | ((p1_present as u64) << 1) | (single_player as u64);
    let packed = (packed << 8) | active_side as u64;
    let packed = (packed << 8) | (((styles[0] & 0xF) as u64) << 4) | ((styles[1] & 0xF) as u64);
    if DIAG_LAST.swap(packed, Ordering::AcqRel) != packed {
        log_info!(
            "CenterArrowsSingle: layout pass — p0_present={} p1_present={} single_player={} active_side={} styles=[{},{}]",
            p0_present,
            p1_present,
            single_player,
            if active_side == 0xFF { -1 } else { active_side as i32 },
            styles[0],
            styles[1]
        );
    }
}

/// Read the two per-side "is playing" flags. The engine's own per-side lamp
/// accessors do:
///   MOV RAX,[slot]      ; RAX = *slot   (P1 slot = array+0, P2 slot = array+8)
///   MOV RCX,[RAX]       ; RCX = **slot  (the player object)
///   CMP [RCX+4],0       ; presence bool
/// i.e. presence := `*(*(*slot) + 4) != 0` — a TRIPLE dereference from the slot.
/// Returns `(false, false)` if unresolved or any pointer in the chain is null.
fn read_presence() -> (bool, bool) {
    let array = PLAYER_ARRAY.load(Ordering::Acquire) as *const *const *const u8;
    if array.is_null() {
        return (false, false);
    }
    unsafe {
        let present = |slot_index: usize| -> bool {
            let p1 = array.add(slot_index).read_unaligned(); // *slot
            if p1.is_null() {
                return false;
            }
            let player = p1.read_unaligned(); // **slot = player object
            !player.is_null() && player.add(PLAYER_PRESENT_OFFSET).read_unaligned() != 0
        };
        (present(0), present(1))
    }
}

// ── Setter detour ───────────────────────────────────────────────────

/// Named-layout setter — `void(parent /*RCX*/, name /*RDX, C-string*/, coord
/// /*R8, 6xi32*/)`. (Resolved by the `hud_layout_setter` signature.)
type HudSetterFn = unsafe extern "C" fn(*mut u8, *const i8, *mut i32);
static mut HUD_SETTER_HOOK: Option<GenericDetour<HudSetterFn>> = None;

unsafe extern "C" fn hud_setter_hook(parent: *mut u8, name: *const i8, coord: *mut i32) {
    let _ = std::panic::catch_unwind(|| {
        maybe_center(parent, name, coord);
    });
    if let Some(ref hook) = *std::ptr::addr_of!(HUD_SETTER_HOOK) {
        hook.call(parent, name, coord);
    }
}

/// If this call is for the active single-player side's lane-relative element and
/// the option is on, shift `coord[0]` (X) toward screen center before the
/// original stores it. P1 (left, side 0) shifts +RIGHT; P2 (right, side 1)
/// shifts -LEFT — landing either side's elements on the same centered midpoint.
fn maybe_center(parent: *mut u8, name: *const i8, coord: *mut i32) {
    if parent.is_null() || name.is_null() || coord.is_null() {
        return;
    }

    let st = unsafe { &*std::ptr::addr_of!(PASS_STATE) };

    // Compute the side index from the per-side parent pointer (range + exact
    // stride alignment). `side_opt` is None if the pointer doesn't map cleanly.
    let side_opt = if st.builder_root != 0 {
        let base = st.builder_root + PER_SIDE_PARENT_BASE;
        let pu = parent as usize;
        if pu >= base && (pu - base) % PER_SIDE_STRIDE == 0 {
            Some((pu - base) / PER_SIDE_STRIDE)
        } else {
            None
        }
    } else {
        None
    };

    let cname = unsafe { CStr::from_ptr(name) };
    let name_str = cname.to_str().unwrap_or("<bad>");

    // ── Gate ────────────────────────────────────────────────────────
    if !st.single_player || st.active_side > 1 {
        return;
    }
    let Some(side) = side_opt else { return };
    if side > 1 || side as u8 != st.active_side {
        return;
    }
    // Doubles gate: only shift a side laid out with the side-offset SINGLE
    // style. In doubles (style 1) the game itself already centers the 8-panel
    // `double_lane_usr` lane — shifting on top pushed it half off-screen
    // (pre-existing bug, capture_20260717_013031.jpg). Unknown styles are
    // treated conservatively (no shift).
    if st.styles[side] != STYLE_SINGLE {
        return;
    }
    if !OPTION_ENABLED[side].load(Ordering::Acquire) {
        return;
    }
    if !TARGET_KEYS.contains(&name_str) {
        return;
    }

    // Shift X toward center by the per-side delta. P1 (side 0, left) moves
    // +RIGHT; P2 (side 1, right) moves -LEFT. The two stock playfields are a
    // rigid translation of each other (P2.x - P1.x = constant across all
    // lane-relative elements), so a uniform shift preserves their relative
    // alignment and lands the active side on the centered midpoint.
    let delta = if side == 0 { LANE_SHIFT } else { -LANE_SHIFT };
    unsafe {
        let x = coord.read_unaligned();
        coord.write_unaligned(x + delta);
    }
}

// ── Hook lifecycle ──────────────────────────────────────────────────

fn install_hooks(builder_addr: *const u8, setter_addr: *const u8) -> bool {
    // Builder entry hook.
    unsafe {
        let target: HudBuilderFn = std::mem::transmute(builder_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(HUD_BUILDER_HOOK),
            target,
            hud_builder_hook,
        ) {
            log_warn!("CenterArrowsSingle: builder hook install failed: {:?}", e);
            return false;
        }
    }

    // Setter hook.
    unsafe {
        let target: HudSetterFn = std::mem::transmute(setter_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(HUD_SETTER_HOOK),
            target,
            hud_setter_hook,
        ) {
            log_warn!("CenterArrowsSingle: setter hook install failed: {:?}", e);
            // Roll back the builder hook so we don't half-install.
            if let Some(d) = (*std::ptr::addr_of_mut!(HUD_BUILDER_HOOK)).take() {
                let _ = d.disable();
            }
            return false;
        }
    }

    log_info!(
        "CenterArrowsSingle: hooks installed (builder @ {:p}, setter @ {:p})",
        builder_addr,
        setter_addr
    );
    true
}

fn remove_hooks() {
    unsafe {
        if let Some(d) = (*std::ptr::addr_of_mut!(HUD_SETTER_HOOK)).take() {
            let _ = d.disable();
        }
        if let Some(d) = (*std::ptr::addr_of_mut!(HUD_BUILDER_HOOK)).take() {
            let _ = d.disable();
        }
    }
}

/// Per-player option change callback. Per-player (no cross-sync, by design):
/// in 2P the single-player gate suppresses centering regardless; in 1P the lone
/// active side's value governs.
fn on_change(side: u8, value: i32) {
    if side < 2 {
        OPTION_ENABLED[side as usize].store(value != 0, Ordering::Release);
    }
}

pub struct CenterArrowsSingleMod {
    builder_addr: Option<*const u8>,
    setter_addr: Option<*const u8>,
}

unsafe impl Send for CenterArrowsSingleMod {}

impl CenterArrowsSingleMod {
    pub fn new() -> Self {
        Self {
            builder_addr: None,
            setter_addr: None,
        }
    }
}

impl Mod for CenterArrowsSingleMod {
    fn id(&self) -> &str {
        "center-arrows-single"
    }

    fn name(&self) -> &str {
        "Center Arrows (1P)"
    }

    fn description(&self) -> &str {
        "Centers the playfield during single-player (per-player option)"
    }

    fn required_signatures(&self) -> &[&str] {
        // Graceful degradation (Q6): not hard-required. The mod installs its
        // hooks in `enable` and goes inert (registers no option row) if either
        // is missing, rather than failing registration.
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.builder_addr = ctx.signatures.get_address("hud_layout_builder");
        self.setter_addr = ctx.signatures.get_address("hud_layout_setter");

        // Resolve the player-object array via the accessor anchor: the first
        // instruction is `MOV RAX,[RIP+disp32]` (48 8B 05), so the global is
        // decoded from disp32 at anchor+3. Validate the opcode before decoding.
        if let Some(anchor) = ctx.signatures.get_address("player_array_anchor") {
            unsafe {
                if *anchor == 0x48 && *anchor.add(1) == 0x8B && *anchor.add(2) == 0x05 {
                    let arr = decode_rip_relative(anchor.add(3));
                    PLAYER_ARRAY.store(arr as u64, Ordering::Release);
                    log_info!("CenterArrowsSingle: player_array (derived) @ {:p}", arr);
                } else {
                    log_warn!(
                        "CenterArrowsSingle: player_array_anchor opcode mismatch ({:02X} {:02X} {:02X}) — detection unavailable",
                        *anchor, *anchor.add(1), *anchor.add(2)
                    );
                }
            }
        } else {
            log_warn!("CenterArrowsSingle: player_array_anchor unresolved — detection unavailable");
        }

        if self.builder_addr.is_none() || self.setter_addr.is_none() {
            log_warn!(
                "CenterArrowsSingle: layout signatures unresolved (builder={}, setter={}) — mod will be inert",
                self.builder_addr.is_some(),
                self.setter_addr.is_some()
            );
        }
        true
    }

    fn enable(&mut self) {
        // Detection requires the player array; without it the mod can't tell
        // single- from two-player, so don't install/offer it (no inert row).
        let detection_ok = PLAYER_ARRAY.load(Ordering::Acquire) != 0;
        let ok = match (self.builder_addr, self.setter_addr, detection_ok) {
            (Some(b), Some(s), true) => install_hooks(b, s),
            _ => false,
        };
        HOOKS_OK.store(ok, Ordering::Release);

        if !ok {
            // No inert option row (Q6/UX): if hooks/detection aren't in, don't register.
            log_warn!(
                "CenterArrowsSingle: enabled but unavailable (detection={}) — option not offered",
                detection_ok
            );
            return;
        }

        // Register the per-player option only after hooks are confirmed.
        if custom_options::is_available() {
            let spec = RegisterSpec::bool_toggle(OPTION_ID)
                .display_name("Center Arrows (1P Only)")
                .description(
                    "Solo play renders the lane at the cabinet's center instead of the 1P side",
                )
                .default_value(0)
                .on_change(on_change);
            match custom_options::register_option(spec) {
                Ok(_handle) => log_info!("CenterArrowsSingle: enabled — option row registered"),
                Err(e) => log_warn!("CenterArrowsSingle: option registration failed: {e}"),
            }
        } else {
            log_warn!(
                "CenterArrowsSingle: custom_options unavailable — hooks active but no option row"
            );
        }
    }

    fn disable(&mut self) {
        remove_hooks();
        HOOKS_OK.store(false, Ordering::Release);
        DIAG_LAST.store(u64::MAX, Ordering::Release);
        OPTION_ENABLED[0].store(false, Ordering::Release);
        OPTION_ENABLED[1].store(false, Ordering::Release);
        log_info!("CenterArrowsSingle: disabled");
    }
}
