//! Overlay Element Styling — per-player scale & opacity for the dynamic
//! gameplay feedback elements (combo counter, judgement text incl. freeze
//! O.K./N.G. and FAST/SLOW, and the pacemaker score tracker).
//!
//! Two always-visible rows on the game's native Options screen (Mods tab):
//!   - `OVERLAY SCALE`   — 25–150 %, default 100
//!   - `OVERLAY OPACITY` — 0–100 %,  default 100
//!
//! Both apply uniformly (one shared knob pair covering all five element
//! groups) at the next song's start; the receptor hit flashes (`dance_effect`)
//! are explicitly EXCLUDED.
//!
//! ## Mechanism (see `docs/gameplay_overlay_elements_research.md` and
//! `.agents/planning/20260712-overlay-element-styling/design/detailed-design.md`)
//!
//! Every scoped element is a BM2D `CMovieClip` pool wrapper around a
//! game-owned AFP layer. Three detours (added in later steps):
//!   - `capture.rs`  : `CMovieClip::Create` (name in R8) → 64-slot registry;
//!                     wrapper SetPosition (+0x38) → side-bind + scale/color
//!                     one-shots.
//!   - `color_hook.rs`: wrapper SetColor +0x90 (float) / +0xB0 (int) compose
//!                      detours that multiply the game's alpha by the opacity.
//!
//! ## Degradation (Q9)
//!
//! Load-bearing (mod self-disables, no rows): `cmovieclip_create` +
//! `cmovieclip_set_color_float` signatures, `afp_layer_set_matrix`
//! ([`bm2d_api::afp_layers_available`]) and `afp_layer_set_color`
//! ([`bm2d_api::layer_color_available`]). Non-fatal: the +0xB0 int detour and
//! the SetPosition side-binding detour (versus degrades to stock rendering;
//! single/double still styled via active-side attribution).
//!
//! NOTE (deviation from design §4.7): `required_signatures()` returns `&[]`
//! rather than listing the AOBs. The load-bearing set spans both signatures
//! AND runtime service state (`bm2d_api` availability), which the registry's
//! `required_signatures` gate cannot express — so all load-bearing checks live
//! in `init`/`enable` with an `is_active()` self-disable, matching the
//! `center_arrows_single` precedent. This keeps the mod visible/toggleable in
//! the mod menu but inert (no rows, no hooks) when the set is incomplete.
//!
//! ## Coexistence with `center-arrows-single`
//!
//! The two mods are orthogonal by construction and MUST stay that way:
//!   - Different hook targets (no shared detour): center-arrows detours the
//!     layout builder/setter (`hud_layout_builder` / `hud_layout_setter`); this
//!     mod detours `CMovieClip::Create` / `SetPosition` (+0x38) / `SetColor`
//!     (+0x90/+0xB0).
//!   - Different phase + axis: center-arrows rewrites `coord[0]` (**X only**) in
//!     the per-side layout registry during layout build, for the active 1P
//!     side. The game later reads that registry and calls the wrapper
//!     `SetPosition(x, y)` with the already-centered X. This mod's SetPosition
//!     detour captures that centered `x` as `orig_x` and writes the layer
//!     matrix `{s,0,0,s, orig_x, anchored_y}` — **preserving X** and only
//!     transforming Y (judge-anchored gap scaling) + scale.
//!   So with both enabled in 1P, elements are centered (X, by center-arrows)
//!   AND scaled + top-anchored (Y, by this mod), with no fight over any axis.
//!   The load-bearing invariant is that `place()` keeps `new_x = orig_x` (never
//!   derive X from a fixed constant), so whatever X the game/center-arrows set
//!   flows through untouched. FAST/SLOW's runtime reposition is likewise
//!   re-anchored from the game's own (centered) x.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::bm2d_api;
use crate::services::custom_options::{self, RegisterSpec, ScalarFormat};
use crate::services::scene_manager;
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

mod capture;
mod color_hook;

// ── Identity ────────────────────────────────────────────────────────
const MOD_ID: &str = "overlay-element-styling";
const OPT_SCALE: &str = "overlay_scale";
const OPT_OPACITY: &str = "overlay_opacity";

const SCALE_MIN: i32 = 25;
const SCALE_MAX: i32 = 150;
const OPACITY_MIN: i32 = 0;
const OPACITY_MAX: i32 = 100;
const STEP_FINE: i32 = 5;
const STEP_COARSE: i32 = 25;
const DEFAULT_PCT: i32 = 100;

// ── Cross-thread option mirrors (design §5.1) ───────────────────────
// Written by the `on_*_change` callbacks (render thread + persistence prime),
// read by the capture bind path and the color compose detours. Seeded to 100
// (identity) so a read before the first `on_change` prime is inert.
pub(crate) static SCALE_PCT: [AtomicI32; 2] =
    [AtomicI32::new(DEFAULT_PCT), AtomicI32::new(DEFAULT_PCT)];
pub(crate) static OPACITY_PCT: [AtomicI32; 2] =
    [AtomicI32::new(DEFAULT_PCT), AtomicI32::new(DEFAULT_PCT)];

/// True while the mod is enabled AND its load-bearing hooks are live. The
/// Create detour gates capture on this so a disabled mod stops tracking new
/// songs' clips.
pub(crate) static MOD_ENABLED: AtomicBool = AtomicBool::new(false);

/// Calibration hide (timing-offsets auto-calibration): while set, every
/// tracked overlay element renders at opacity 0 on BOTH sides — the two
/// opacity accessors below return 0, which the bind-time one-shot
/// (Judge/FreezeJudge/FastSlow) and the SetColor compose detours
/// (Combo/Pacemaker) both consume. Orthogonal to the per-side option values;
/// song-scoped, never persisted; cleared by the calibration session and by
/// this mod's `disable()`.
static CALIBRATION_HIDE: AtomicBool = AtomicBool::new(false);

/// Song-scoped opacity-0 override used by `timing_offsets::calibration`.
/// Returns whether the hide mechanism is live (mod enabled with hooks
/// installed) so the caller can fail open with a WARN when it isn't. Must be
/// set at GAMEPLAY entry — before the song's clips bind — so the bind-time
/// one-shots see it.
pub fn set_calibration_hide(on: bool) -> bool {
    CALIBRATION_HIDE.store(on, Ordering::Release);
    is_enabled()
}

// ── Shared capture (cross-mod consumers) ────────────────────────────

/// Install the CMovieClip capture detours for a SHARING consumer
/// (s_marvelous' flash re-drive needs the side-bound `dance_judge` clip even
/// when this mod is config-disabled). Idempotent — whichever of this fn /
/// this mod's own `enable` runs first installs; the detours are never torn
/// down while a shared consumer is registered. Returns whether the Create
/// capture (the load-bearing half) is live; SetPosition (versus side
/// binding) and the player-array derivation are best-effort.
///
/// Precedent: `power_user_statistics::data_feed::install` (multiple mods'
/// inits request one shared detour).
pub fn ensure_capture_installed(signatures: &crate::core::signatures::SignatureStore) -> bool {
    capture::set_shared_capture(true);
    derive_player_array(signatures);

    let Some(create_addr) = signatures.get_address("cmovieclip_create") else {
        log_warn!("OverlayElementStyling: shared capture — cmovieclip_create unresolved");
        return false;
    };
    if !capture::install_create(create_addr) {
        return false;
    }
    if let Some(addr) = signatures.get_address("cmovieclip_set_position") {
        let _ = capture::install_set_position(addr);
    }
    true
}

/// The side-bound `dance_judge` clip's pool wrapper for `side` (0/1), or
/// `None` when not captured/bound this song. GAME-THREAD-ONLY.
pub fn judge_clip(side: usize) -> Option<*mut u8> {
    if side > 1 {
        return None;
    }
    capture::judge_wrapper_for_side(side as u8)
}

/// Derive + register the player-object array for capture side binding.
/// Idempotent (re-stores the same pointer). Shared by `init` and
/// `ensure_capture_installed`.
fn derive_player_array(signatures: &crate::core::signatures::SignatureStore) {
    if let Some(anchor) = signatures.get_address("player_array_anchor") {
        unsafe {
            if *anchor == 0x48 && *anchor.add(1) == 0x8B && *anchor.add(2) == 0x05 {
                let arr = crate::core::scanner::decode_rip_relative(anchor.add(3));
                capture::set_player_array(arr);
            } else {
                log_warn!(
                    "OverlayElementStyling: player_array_anchor opcode mismatch — side binding unavailable"
                );
            }
        }
    } else {
        log_warn!(
            "OverlayElementStyling: player_array_anchor unresolved — side binding unavailable"
        );
    }
}

// ── Value accessors for the detour modules ──────────────────────────

/// Authoritative per-side scale % at bind time (design §5.1): prefer the
/// `custom_options` registry, fall back to the atomic mirror if the registry
/// is unavailable / lock poisoned. `side` must be 0 or 1.
pub(crate) fn scale_pct(side: u8) -> i32 {
    custom_options::get_value(side, OPT_SCALE).unwrap_or_else(|| scale_pct_fast(side as usize))
}

/// Authoritative per-side opacity % at bind time (see [`scale_pct`]).
/// Calibration hide wins over everything (single relaxed-class load first).
pub(crate) fn opacity_pct(side: u8) -> i32 {
    if CALIBRATION_HIDE.load(Ordering::Acquire) {
        return 0;
    }
    custom_options::get_value(side, OPT_OPACITY).unwrap_or_else(|| opacity_pct_fast(side as usize))
}

/// Lock-free per-side scale % read for hot paths (color detours). Reads the
/// atomic mirror only. Out-of-range `side` → identity (100).
pub(crate) fn scale_pct_fast(side: usize) -> i32 {
    SCALE_PCT
        .get(side)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(DEFAULT_PCT)
}

/// Lock-free per-side opacity % read for hot paths (color detours). Reads the
/// atomic mirror only. Out-of-range `side` → identity (100). Calibration
/// hide wins over everything (one extra atomic load on the compose path).
pub(crate) fn opacity_pct_fast(side: usize) -> i32 {
    if CALIBRATION_HIDE.load(Ordering::Acquire) {
        return 0;
    }
    OPACITY_PCT
        .get(side)
        .map(|a| a.load(Ordering::Acquire))
        .unwrap_or(DEFAULT_PCT)
}

/// Whether the mod is enabled with live hooks (read by the detours).
pub(crate) fn is_enabled() -> bool {
    MOD_ENABLED.load(Ordering::Acquire)
}

// ── Option change callbacks ─────────────────────────────────────────
// Two callbacks because `OnChangeFn` carries only `(side, value)`, not the
// option id — each option mirrors into its own atomic array. Fired by the
// framework at registration (initial prime), on user adjustment, and on a
// persistence load.

fn on_scale_change(side: u8, value: i32) {
    if let Some(a) = SCALE_PCT.get(side as usize) {
        a.store(value, Ordering::Release);
    }
}

fn on_opacity_change(side: u8, value: i32) {
    if let Some(a) = OPACITY_PCT.get(side as usize) {
        a.store(value, Ordering::Release);
    }
}

/// Register the two scalar rows once. Idempotent: a `Duplicate` on re-enable
/// is expected (the framework has no unregister) and treated as success.
fn register_rows() {
    let specs = [
        RegisterSpec::scalar(
            OPT_SCALE,
            SCALE_MIN,
            SCALE_MAX,
            STEP_FINE,
            ScalarFormat::Unit { unit: "%" },
        )
        .display_name("Overlay Scale")
        .description("Size of the combo, judgement, and pacemaker overlay elements")
        .step_coarse(STEP_COARSE)
        .default_value(DEFAULT_PCT)
        .on_change(on_scale_change),
        RegisterSpec::scalar(
            OPT_OPACITY,
            OPACITY_MIN,
            OPACITY_MAX,
            STEP_FINE,
            ScalarFormat::Unit { unit: "%" },
        )
        .display_name("Overlay Opacity")
        .description("Opacity of the combo, judgement, and pacemaker overlay elements")
        .step_coarse(STEP_COARSE)
        .default_value(DEFAULT_PCT)
        .on_change(on_opacity_change),
    ];
    for spec in specs {
        match custom_options::register_option(spec) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {
                // Re-enable after a disable: rows stay registered (no
                // unregister API); the fresh on_change prime already
                // re-seeded the atomics. Not an error.
            }
            Err(e) => log_warn!("OverlayElementStyling: option registration failed: {e}"),
        }
    }
}

// ── Mod implementation ──────────────────────────────────────────────

pub struct OverlayElementStylingMod {
    /// Resolved hook-target addresses (stashed in `init`, consumed by the
    /// detour installers in `enable`). `None` = signature unresolved.
    create_addr: Option<*const u8>,
    set_color_float_addr: Option<*const u8>,
    set_color_int_addr: Option<*const u8>,
    set_position_addr: Option<*const u8>,
    /// The load-bearing set resolved (signatures + libafp raw ops).
    available: bool,
    /// Recorded after `enable` — reported by `is_active()`.
    active: bool,
    /// Scene-change callback id (registry clearing), removed on disable.
    scene_cb_id: Option<usize>,
}

// Raw pointers into the game module; valid for the process lifetime, only
// dereferenced on the game thread inside detour installers.
unsafe impl Send for OverlayElementStylingMod {}

impl OverlayElementStylingMod {
    pub fn new() -> Self {
        Self {
            create_addr: None,
            set_color_float_addr: None,
            set_color_int_addr: None,
            set_position_addr: None,
            available: false,
            active: false,
            scene_cb_id: None,
        }
    }
}

impl Mod for OverlayElementStylingMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Overlay Element Styling"
    }
    fn description(&self) -> &str {
        "Per-player scale and opacity for combo, judgement, and pacemaker displays"
    }
    fn required_signatures(&self) -> &[&str] {
        // Empty on purpose — see the module doc comment. The load-bearing set
        // (AOBs + bm2d service availability) is checked in `init`/`enable`.
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        self.create_addr = ctx.signatures.get_address("cmovieclip_create");
        self.set_color_float_addr = ctx.signatures.get_address("cmovieclip_set_color_float");
        self.set_color_int_addr = ctx.signatures.get_address("cmovieclip_set_color_int");
        self.set_position_addr = ctx.signatures.get_address("cmovieclip_set_position");

        // Derive the player-object array for active-side detection (binding).
        // Non-fatal: without it, side attribution fails (both versus and the
        // single-side Create fallback need presence) — the mod still enables
        // and captures but applies nothing. Model: center_arrows_single.
        // (Shared helper — also invoked by `ensure_capture_installed`.)
        derive_player_array(ctx.signatures);

        let matrix_ok = bm2d_api::afp_layers_available();
        let color_ok = bm2d_api::layer_color_available();
        self.available = self.create_addr.is_some()
            && self.set_color_float_addr.is_some()
            && matrix_ok
            && color_ok;

        if !self.available {
            log_warn!(
                "OverlayElementStyling: load-bearing set incomplete \
                 (create={}, set_color_float={}, afp_layer_set_matrix={}, afp_layer_set_color={}) \
                 — mod will be inert",
                self.create_addr.is_some(),
                self.set_color_float_addr.is_some(),
                matrix_ok,
                color_ok
            );
        }
        // Always register (so the mod is visible/toggleable); enable()
        // self-disables if unavailable.
        true
    }

    fn enable(&mut self) {
        self.active = false;

        if !self.available {
            log_warn!("OverlayElementStyling: enabled but load-bearing set unavailable — inert");
            return;
        }
        if !custom_options::is_available() {
            log_warn!(
                "OverlayElementStyling: custom_options unavailable — options will not render"
            );
            return;
        }

        // Install the load-bearing Create detour first — if it fails, refuse
        // to enable (no rows, no scene callback) so the registry stays
        // consistent. `create_addr` is Some here (checked in `available`).
        let create_addr = self.create_addr.expect("available implies create_addr");
        if !capture::install_create(create_addr) {
            log_warn!("OverlayElementStyling: Create hook unavailable — refusing enable");
            return;
        }

        // Load-bearing +0x90 float compose detour. If it fails, roll back the
        // Create detour and refuse to enable (registry stays consistent).
        let color_float_addr = self
            .set_color_float_addr
            .expect("available implies set_color_float_addr");
        if !color_hook::install_float(color_float_addr) {
            log_warn!("OverlayElementStyling: color compose hook unavailable — refusing enable");
            capture::remove();
            return;
        }

        // SetPosition side-binding detour — NON-fatal (design §4.4). Without
        // it, versus renders stock and single/double bind via the Create-time
        // fallback.
        match self.set_position_addr {
            Some(addr) => {
                let _ = capture::install_set_position(addr);
            }
            None => log_warn!(
                "OverlayElementStyling: cmovieclip_set_position unresolved — versus binding degrades to stock"
            ),
        }

        // The +0xB0 int-variant compose detour (non-fatal). Its coverage of
        // the scoped elements is unproven (design Appendix C.2); the float form
        // is the load-bearing path, so a miss here is logged and tolerated.
        match self.set_color_int_addr {
            Some(addr) => {
                let _ = color_hook::install_int(addr);
            }
            None => log_warn!(
                "OverlayElementStyling: cmovieclip_set_color_int unresolved — +0xB0 compose skipped (non-fatal)"
            ),
        }

        register_rows();

        // Seed the atomic mirrors from the authoritative registry values. On
        // first enable `register_option` already primed them via `on_change`;
        // on a RE-enable it returns `Duplicate` and does NOT re-fire, so this
        // guarantees the hot-path color detour (which reads the atomics) sees
        // the current per-side values rather than a stale default.
        for side in 0u8..2 {
            on_scale_change(
                side,
                custom_options::get_value(side, OPT_SCALE).unwrap_or(DEFAULT_PCT),
            );
            on_opacity_change(
                side,
                custom_options::get_value(side, OPT_OPACITY).unwrap_or(DEFAULT_PCT),
            );
        }

        // Clear the clip registry whenever gameplay begins (fresh song) and
        // when it ends (log the capture summary). Belt-and-braces alongside
        // Create-time slot-reuse eviction.
        if scene_manager::is_available() {
            let id = scene_manager::on_scene_change(Box::new(|prev, next| {
                if next == scene::GAMEPLAY {
                    capture::clear(false);
                } else if prev == scene::GAMEPLAY {
                    capture::clear(true);
                }
            }));
            self.scene_cb_id = Some(id);
        }

        MOD_ENABLED.store(true, Ordering::Release);
        self.active = true;
        log_info!("OverlayElementStyling: enabled (Create hook + options)");
    }

    fn disable(&mut self) {
        MOD_ENABLED.store(false, Ordering::Release);
        // Belt and braces: never leave a calibration hide behind (the
        // calibration session also clears it on its own teardown).
        CALIBRATION_HIDE.store(false, Ordering::Release);
        self.active = false;
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        color_hook::remove();
        capture::remove();
        // Rows stay registered (no unregister API). The atomic mirrors are
        // left at their last-known values — a disabled mod's detours are gated
        // off by `MOD_ENABLED`, and a re-enable re-seeds the mirrors from the
        // registry, so there is nothing to reset here.
        log_info!("OverlayElementStyling: disabled");
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
