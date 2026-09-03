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
//!      `{single_player, active_side}` from the per-side play-states. The
//!      direct prologue AOB bakes in per-build stack-frame constants (it misses
//!      on 20250805 and 20260224), so the entry falls back to a derivation from
//!      the build-stable `hud_layout_builder_style_cluster` anchor (unique on
//!      all six inspected builds, entry = match-0x1DC) via a backward scan for
//!      the frame-size-agnostic prologue head.
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
//! Dark song-info card (third detour, best-effort): the centered lane's lower
//! portion is occluded by the opaque 1P song-info/jacket card at the bottom of
//! the screen. DOUBLES play natively swaps that card for a dark transparent
//! variant (`dance_song_info_double`) precisely so it doesn't cover the
//! centered lane. The song-info card builder picks the variant from its own
//! style field (`card+0xC4`, 0=single/1=double): `CMP [RBP+0xC4],EDI; SETZ
//! R13B`, where R13B selects the card name AND gates the dark-tint color write
//! at the tail. A community hex patch (20250805, file offset 476947:
//! `41 0F 94 C5` -> `41 B5 00 90`, i.e. SETZ R13B -> MOV R13B,0) forces the
//! doubles card unconditionally. We reproduce it runtime-gated: detour the
//! card builder (entry derived from the `song_info_card_style` AOB via a
//! backward prologue scan) and, when the same centering gate holds, flip
//! `card+0xC4` to 1 across the original call and restore it after — identical
//! in-function behavior to the byte patch, zero code patching, and only when
//! the lane is actually centered. Validated in Ghidra on all four supported
//! builds (20250805/20260324/20260616/20260721; cluster unique on each).
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

/// Song-info card object: card style field (i32) at `card + 0xC4`.
/// `0` = single (opaque side card), nonzero = double (dark transparent card).
/// The card builder's variant branch (`song_info_card_style` AOB) reads it
/// exactly once; flipping it to 1 across the original call forces the dark
/// doubles card without touching code bytes.
const CARD_STYLE_OFFSET: usize = 0xC4;

/// Song-info card builder prologue, used to derive the function entry by
/// scanning backwards from the `song_info_card_style` cluster match:
/// `MOV RAX,RSP; PUSH RDI; PUSH R12; PUSH R13; SUB RSP,0x70`. Byte-identical
/// on all four supported builds (entry = match - 0x9D on each, but the scan
/// tolerates drift).
const CARD_BUILDER_PROLOGUE: &[u8] = &[
    0x48, 0x8B, 0xC4, 0x57, 0x41, 0x54, 0x41, 0x55, 0x48, 0x83, 0xEC, 0x70,
];

/// Maximum backward-scan distance from the style-cluster match to the builder
/// entry (0x9D on all four builds; generous headroom for code drift).
const CARD_BUILDER_SCAN_BACK: usize = 0x200;

/// HUD layout builder prologue HEAD, used to derive the builder entry from the
/// `hud_layout_builder_style_cluster` match when the full `hud_layout_builder`
/// AOB misses. The full AOB bakes in the stack-frame constants (`LEA RBP,[RAX-
/// 0x1D8]; SUB RSP,0x2B0; MOV [RBP+0x20],-2`), which drift per build — 20250805
/// is `-0x1D8/0x2A0/+0x18`, 20260224 `-0x1C8/0x2A0/+0x18` — so only the
/// frame-size-agnostic head is matched here:
/// `MOV RAX,RSP; PUSH RBP; PUSH R12; PUSH R13; PUSH R14; PUSH R15; LEA RBP,[RAX+disp32]`.
/// The `48 8D A8` LEA opcode (RBP ← RAX-relative) is included to reject the
/// far more common frame-less `MOV RAX,RSP; PUSH...` prologues; its disp32 is
/// not.
const HUD_BUILDER_PROLOGUE_HEAD: &[u8] = &[
    0x48, 0x8B, 0xC4, 0x55, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x8D, 0xA8,
];

/// Style-cluster → builder-entry distance is exactly 0x1DC on all six builds
/// inspected (20250805/20260224/20260324/20260616/20260721/20260825); allow
/// generous drift but stay well inside the ~0x1D00-byte function.
const HUD_BUILDER_SCAN_BACK: usize = 0x400;

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

// ── Song-info card detour (dark card for centered 1P play) ──────────

/// Song-info card builder — `void(card /*RCX*/)`. Entry derived from the
/// `song_info_card_style` cluster match via backward prologue scan.
type SongInfoBuilderFn = unsafe extern "C" fn(*mut u8);
static mut SONG_INFO_HOOK: Option<GenericDetour<SongInfoBuilderFn>> = None;

unsafe extern "C" fn song_info_builder_hook(card: *mut u8) {
    let force = std::panic::catch_unwind(|| should_force_dark_card(card)).unwrap_or(false);
    let Some(ref hook) = *std::ptr::addr_of!(SONG_INFO_HOOK) else {
        return;
    };
    if force {
        // Transiently present the card builder with the DOUBLE style so it
        // picks the dark transparent `dance_song_info_double` card and applies
        // the doubles tint — same in-function effect as the community byte
        // patch (SETZ R13B -> MOV R13B,0), but gated and restored. The builder
        // reads the field exactly once (the CMP feeding SETZ) and runs
        // synchronously on the game thread, so the flip is invisible outside
        // this call.
        let style = unsafe { card.add(CARD_STYLE_OFFSET) as *mut i32 };
        unsafe { style.write_unaligned(1) };
        hook.call(card);
        unsafe { style.write_unaligned(STYLE_SINGLE) };
    } else {
        hook.call(card);
    }
}

/// Force the dark doubles card iff the same gate that centers the lane holds:
/// card is genuinely SINGLE-style, session is single-player, and the active
/// side's centering option is on. Doubles play (style already nonzero) needs
/// no help — the game picks the dark card natively.
fn should_force_dark_card(card: *mut u8) -> bool {
    if card.is_null() {
        return false;
    }
    let style = unsafe { (card.add(CARD_STYLE_OFFSET) as *const i32).read_unaligned() };
    if style != STYLE_SINGLE {
        return false;
    }
    let (p0_present, p1_present) = read_presence();
    let side = match (p0_present, p1_present) {
        (true, false) => 0usize,
        (false, true) => 1usize,
        _ => return false, // 2P (or unknown): never force
    };
    OPTION_ENABLED[side].load(Ordering::Acquire)
}

/// Derive the song-info card builder entry: backward-scan from the
/// `song_info_card_style` cluster match for the builder prologue. Returns
/// None (feature unavailable, WARN'd by the caller) if not found.
fn derive_card_builder_entry(cluster: *const u8) -> Option<*const u8> {
    derive_entry_behind(cluster, CARD_BUILDER_PROLOGUE, CARD_BUILDER_SCAN_BACK)
}

/// Backward-scan from an in-body anchor for the nearest preceding `prologue`
/// byte sequence; returns its address (the function entry) or None.
fn derive_entry_behind(anchor: *const u8, prologue: &[u8], max_back: usize) -> Option<*const u8> {
    unsafe {
        for back in prologue.len()..=max_back {
            let candidate = anchor.sub(back);
            let window = std::slice::from_raw_parts(candidate, prologue.len());
            if window == prologue {
                return Some(candidate);
            }
        }
    }
    None
}

/// Resolve the HUD layout builder entry: prefer the direct prologue AOB, else
/// derive it from the build-stable lane-name style cluster (exactly one match
/// required — ambiguity means the anchor drifted, so fail rather than guess).
fn resolve_hud_builder(ctx: &ModContext) -> Option<*const u8> {
    if let Some(addr) = ctx.signatures.get_address("hud_layout_builder") {
        return Some(addr);
    }
    let clusters = ctx
        .signatures
        .get_all_matches("hud_layout_builder_style_cluster");
    match clusters.as_slice() {
        [cluster] => {
            let entry =
                derive_entry_behind(*cluster, HUD_BUILDER_PROLOGUE_HEAD, HUD_BUILDER_SCAN_BACK);
            match entry {
                Some(e) => log_info!(
                    "CenterArrowsSingle: hud_layout_builder AOB missed; derived entry @ {:p} from style cluster @ {:p} (delta 0x{:X})",
                    e,
                    *cluster,
                    (*cluster as usize).wrapping_sub(e as usize)
                ),
                None => log_warn!(
                    "CenterArrowsSingle: builder prologue head not found behind style cluster @ {:p}",
                    *cluster
                ),
            }
            entry
        }
        other => {
            log_warn!(
                "CenterArrowsSingle: hud_layout_builder AOB missed and style cluster resolved {} matches (want 1)",
                other.len()
            );
            None
        }
    }
}

// ── Hook lifecycle ──────────────────────────────────────────────────

fn install_hooks(
    builder_addr: *const u8,
    setter_addr: *const u8,
    card_builder_addr: Option<*const u8>,
) -> bool {
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

    // Song-info dark-card hook (best-effort: centering works without it, the
    // card just stays opaque; one WARN at derivation/install failure).
    if let Some(card_addr) = card_builder_addr {
        unsafe {
            let target: SongInfoBuilderFn = std::mem::transmute(card_addr);
            match crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!(SONG_INFO_HOOK),
                target,
                song_info_builder_hook,
            ) {
                Ok(()) => log_info!(
                    "CenterArrowsSingle: song-info dark-card hook installed @ {:p}",
                    card_addr
                ),
                Err(e) => log_warn!(
                    "CenterArrowsSingle: song-info card hook install failed ({:?}) — card stays opaque",
                    e
                ),
            }
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
        if let Some(d) = (*std::ptr::addr_of_mut!(SONG_INFO_HOOK)).take() {
            let _ = d.disable();
        }
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
    /// Song-info card builder entry (derived from `song_info_card_style`);
    /// None = dark-card feature unavailable (centering still works).
    card_builder_addr: Option<*const u8>,
}

unsafe impl Send for CenterArrowsSingleMod {}

impl CenterArrowsSingleMod {
    pub fn new() -> Self {
        Self {
            builder_addr: None,
            setter_addr: None,
            card_builder_addr: None,
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
        self.builder_addr = resolve_hud_builder(ctx);
        self.setter_addr = ctx.signatures.get_address("hud_layout_setter");

        // Song-info dark-card derivation (best-effort). Require exactly one
        // cluster match (the pattern is unique on all four supported builds;
        // multiple matches would mean the anchor drifted — fail the feature,
        // not the mod), then backward-scan for the builder prologue.
        let cluster_matches = ctx.signatures.get_all_matches("song_info_card_style");
        self.card_builder_addr = match cluster_matches.as_slice() {
            [cluster] => match derive_card_builder_entry(*cluster) {
                Some(entry) => {
                    log_info!(
                        "CenterArrowsSingle: song-info card builder (derived) @ {:p}",
                        entry
                    );
                    Some(entry)
                }
                None => {
                    log_warn!(
                        "CenterArrowsSingle: card builder prologue not found behind style cluster @ {:p} — dark card unavailable",
                        *cluster
                    );
                    None
                }
            },
            other => {
                log_warn!(
                    "CenterArrowsSingle: song_info_card_style resolved {} matches (want 1) — dark card unavailable",
                    other.len()
                );
                None
            }
        };

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
            (Some(b), Some(s), true) => install_hooks(b, s, self.card_builder_addr),
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
