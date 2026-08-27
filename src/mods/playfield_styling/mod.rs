//! Playfield Styling — per-player scale & opacity for the gameplay
//! playfield: scrolling arrows (normal / freeze / shock + electric overlay),
//! the receptor row, the sprite-based receptor hit flash, the measure
//! guideline, and mines (when `note_types_expansion` is active).
//!
//! Two always-visible rows on the game's native Options screen (Mods tab):
//!   - `ARROW SCALE`   — 25–150 %, default 100
//!   - `ARROW OPACITY` — 0–100 %,  default 100
//!
//! The playfield scales about the **lane center X + receptor row Y**: the
//! receptor row shrinks in place, staying horizontally centered on the stock
//! lane; arrows converge toward the center line as they scroll. Purely
//! visual — zero effect on timing, judging, or scoring. Values latch at
//! GAMEPLAY entry (one snapshot per side per song).
//!
//! ## Mechanism (design:
//! `.agents/planning/20260716-arrow-receptor-styling/design/detailed-design.md`)
//!
//! Three legs:
//!   1. `fill_hook.rs`  : one detour on the shared per-quad sprite fill
//!      (`render_sprite_final`) — every lane quad flows through it with
//!      lane-relative `(x, y, w, h)` args + a color ptr. The detour scales
//!      the geometry about the lane center and composes opacity into a
//!      copied color.
//!   2. `services::cull_window` (promoted from this mod's former
//!      `cull_patch.rs`): verified 4-byte disp32 redirects on the note
//!      collector's (and guideline draw's) 720.0f cull loads, pointing at
//!      a mod-owned float. This mod contributes its latched `min(scale)`
//!      per song so shrunken playfields never pop arrows in mid-screen
//!      (player_perspective contributes its hallway draw distance; the
//!      effective bound is `max(720, distance)/min(scale, 1)` — the two
//!      transforms stack, so the composition is multiplicative). The shared
//!      720.0 constant itself is NEVER patched (14 unrelated readers).
//!   3. `guideline_hook.rs`: capture detour on the guideline draw (Y-base
//!      pre-scale, exact for both scroll directions) + transform detour on
//!      its single-caller bulk emitter.
//!
//! ## Degradation (requirement A6 — all-or-nothing)
//!
//! The full gate set — fill detour + collector cull patch (byte-verified) +
//! guideline detours/patch — must ALL install, or the mod self-disables and
//! registers NO option rows (no inert UI). All load-bearing resolution
//! happens in `init` via the `derive_playfield_styling` signature chain;
//! `required_signatures()` returns `&[]` and `is_active()` self-reports
//! (the `overlay_element_styling` precedent), so the mod stays visible in
//! the mod menu but inert when the set is incomplete.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::core::scanner::decode_rip_relative;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, RegisterSpec, ScalarFormat};
use crate::services::{cull_window, scene_manager};
use crate::types::scenes::scene;
use crate::{log_debug, log_info, log_warn};

mod fill_hook;
mod guideline_hook;
mod lane_hook;

// ── Identity ────────────────────────────────────────────────────────
const MOD_ID: &str = "playfield-styling";
const OPT_SCALE: &str = "arrow_scale";
const OPT_OPACITY: &str = "arrow_opacity";

const SCALE_MIN: i32 = 25;
const SCALE_MAX: i32 = 125;
const OPACITY_MIN: i32 = 0;
const OPACITY_MAX: i32 = 100;
const STEP_FINE: i32 = 5;
const STEP_COARSE: i32 = 25;
const DEFAULT_PCT: i32 = 100;

/// The game's render-height / top-cull bound (shared 720.0f constant).
pub(crate) const RENDER_HEIGHT: f32 = 720.0;

// ── Cross-thread option mirrors (design §5) ─────────────────────────
// Written by the `on_*_change` callbacks (render thread + persistence
// prime), read only at latch time (GAMEPLAY entry). Seeded to 100
// (identity) so a read before the first `on_change` prime is inert.
static SCALE_PCT: [AtomicI32; 2] = [AtomicI32::new(DEFAULT_PCT), AtomicI32::new(DEFAULT_PCT)];
static OPACITY_PCT: [AtomicI32; 2] = [AtomicI32::new(DEFAULT_PCT), AtomicI32::new(DEFAULT_PCT)];

// ── Per-song latch (requirement A4/R4) ──────────────────────────────
// One snapshot per side per song, taken at GAMEPLAY entry; the fill /
// guideline hooks and the cull float consume ONLY these for the whole
// song. Stored as f32 bit patterns in atomics so the hot-path reads are
// lock-free from any thread. Identity = 1.0/1.0.
static LATCHED_SCALE: [AtomicU32; 2] = [
    AtomicU32::new(f32::to_bits(1.0)),
    AtomicU32::new(f32::to_bits(1.0)),
];
static LATCHED_OPACITY: [AtomicU32; 2] = [
    AtomicU32::new(f32::to_bits(1.0)),
    AtomicU32::new(f32::to_bits(1.0)),
];

/// True while the mod is enabled AND its load-bearing hooks are live. The
/// detours and the scene latch gate on this.
static MOD_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the mod is enabled with live hooks (read by the detours).
pub(crate) fn is_enabled() -> bool {
    MOD_ENABLED.load(Ordering::Acquire)
}

/// The latched `(scale, opacity)` for a side. Lock-free; identity for an
/// out-of-range side.
pub(crate) fn latched(side: usize) -> (f32, f32) {
    match (LATCHED_SCALE.get(side), LATCHED_OPACITY.get(side)) {
        (Some(s), Some(o)) => (
            f32::from_bits(s.load(Ordering::Acquire)),
            f32::from_bits(o.load(Ordering::Acquire)),
        ),
        _ => (1.0, 1.0),
    }
}

/// The minimum latched playfield scale across sides (≤ 1.0) — this mod's
/// contribution to the shared cull window (`services::cull_window` divides
/// the effective bound by it: `720/s` alone, `distance/s` composed with the
/// hallway draw distance).
pub(crate) fn latched_min_scale() -> f32 {
    let (s0, _) = latched(0);
    let (s1, _) = latched(1);
    s0.min(s1).min(1.0)
}

/// The LIVE playfield cull bound — the value the patched collector /
/// guideline sites currently read: 720.0 stock (mod disabled, identity
/// scale, or patch never installed); `720/min(scale)` while a shrunken
/// song is latched. Exposed for sibling render passes that maintain their
/// own top-cull window (`note_types_expansion::mine_render` — its mine
/// quads flow through the detoured fill and inherit the transform
/// automatically, but its window check must widen in lockstep or shrunken
/// mines pop in mid-screen).
pub(crate) fn cull_bound() -> f32 {
    crate::services::cull_window::cull_bound()
}

/// Shared guideline-detour surface for `player_perspective` (the detours
/// live in this mod's `guideline_hook` but install/remove is refcounted per
/// consumer — either mod may be config-disabled independently). The
/// `cull_bound()` re-export precedent.
pub(crate) fn guideline_acquire_perspective(
    draw: *const u8,
    emitter: *const u8,
    player_array: *const u8,
) -> bool {
    guideline_hook::acquire(
        guideline_hook::Consumer::PlayerPerspective,
        draw,
        emitter,
        player_array,
    )
}

/// Drop `player_perspective`'s interest in the guideline detours.
pub(crate) fn guideline_release_perspective() {
    guideline_hook::release(guideline_hook::Consumer::PlayerPerspective);
}

/// Shared receptor-hit-flash surface for `player_perspective` (same
/// consumer-refcount contract as the guideline detours): the flash is an
/// AFP clip the perspective VS never touches, so `lane_hook` composes the
/// published perspective map onto it CPU-side.
pub(crate) fn flash_acquire_perspective(note_result_setup: Option<*const u8>) -> bool {
    lane_hook::flash_acquire(
        lane_hook::FlashConsumer::PlayerPerspective,
        note_result_setup,
    )
}

/// Drop `player_perspective`'s interest in the flash capture.
pub(crate) fn flash_release_perspective() {
    lane_hook::flash_release(lane_hook::FlashConsumer::PlayerPerspective);
}

/// Drain the lane-clip pending queue (perspective's lane pass drives this
/// when playfield_styling is config-disabled; one atomic load when idle).
pub(crate) fn lane_apply_pending() {
    if lane_hook::has_pending() {
        lane_hook::apply_pending();
    }
}

/// Song-boundary lane-state reset + gameplay-window flag, callable by
/// `player_perspective`'s scene callback (idempotent with this mod's own
/// scene callback — both fire at the same transitions; required when this
/// mod is config-disabled and its callback isn't registered).
pub(crate) fn lane_scene_transition(entering_gameplay: bool) {
    lane_hook::reset();
    fill_hook::set_in_gameplay(entering_gameplay);
}

/// The per-side "is playing" presence flags (the game's own player-array
/// read — the side-attribution primitive shared with the guideline/lane
/// captures). Re-exported for `player_perspective`'s judge-effect pass.
pub(crate) fn read_presence() -> (bool, bool) {
    fill_hook::read_presence()
}

// ── Option change callbacks ─────────────────────────────────────────
// Two callbacks because `OnChangeFn` carries only `(side, value)`, not the
// option id — each option mirrors into its own atomic array. Fired by the
// framework at registration (initial prime), on user adjustment, and on a
// persistence load. Takes effect at the NEXT song's latch.

fn on_scale_change(side: u8, value: i32) {
    if let Some(a) = SCALE_PCT.get(side as usize) {
        a.store(value, Ordering::Release);
        log_debug!("{MOD_ID}: arrow_scale[{side}] = {value}");
    }
}

fn on_opacity_change(side: u8, value: i32) {
    if let Some(a) = OPACITY_PCT.get(side as usize) {
        a.store(value, Ordering::Release);
        log_debug!("{MOD_ID}: arrow_opacity[{side}] = {value}");
    }
}

/// Authoritative per-side scale % at latch time: prefer the
/// `custom_options` registry, fall back to the atomic mirror.
fn scale_pct(side: u8) -> i32 {
    custom_options::get_value(side, OPT_SCALE).unwrap_or_else(|| {
        SCALE_PCT
            .get(side as usize)
            .map(|a| a.load(Ordering::Acquire))
            .unwrap_or(DEFAULT_PCT)
    })
}

/// Authoritative per-side opacity % at latch time (see [`scale_pct`]).
fn opacity_pct(side: u8) -> i32 {
    custom_options::get_value(side, OPT_OPACITY).unwrap_or_else(|| {
        OPACITY_PCT
            .get(side as usize)
            .map(|a| a.load(Ordering::Acquire))
            .unwrap_or(DEFAULT_PCT)
    })
}

/// Register the two scalar rows once. Idempotent: a `Duplicate` on
/// re-enable is expected (the framework has no unregister) and treated as
/// success.
///
/// Called ONLY after the complete all-or-nothing install gate (fill detour
/// + both cull patches + both guideline detours) has succeeded — a partial
/// install must never show rows (requirement A6: no inert UI).
fn register_rows() {
    let specs = [
        RegisterSpec::scalar(
            OPT_SCALE,
            SCALE_MIN,
            SCALE_MAX,
            STEP_FINE,
            ScalarFormat::Unit { unit: "%" },
        )
        .display_name("Arrow Scale")
        .description("Size of the arrows, receptors, and lane effects")
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
        .display_name("Arrow Opacity")
        .description("Opacity of the arrows, receptors, and lane effects")
        .step_coarse(STEP_COARSE)
        .default_value(DEFAULT_PCT)
        .on_change(on_opacity_change),
    ];
    for spec in specs {
        match custom_options::register_option(spec) {
            Ok(_) => {}
            Err(custom_options::RegisterError::Duplicate { .. }) => {
                // Re-enable after a disable: rows stay registered (no
                // unregister API); the enable-time reseed below re-seeds the
                // atomics. Not an error.
            }
            Err(e) => log_warn!("{MOD_ID}: option registration failed: {e}"),
        }
    }
}

// ── Per-song latch (scene callback) ─────────────────────────────────

/// GAMEPLAY entry: snapshot both sides' option values into the latch, clear
/// the renderer registry (fresh song objects), and log the resulting
/// per-side styles + the would-be cull bound. GAMEPLAY exit: reset the
/// latch to identity, clear the registry, and log the capture stats.
fn on_scene_change(prev: i32, next: i32) {
    if next == scene::GAMEPLAY {
        fill_hook::clear_registry(false);
        lane_hook::reset();
        if !is_enabled() {
            return;
        }
        for side in 0u8..2 {
            let s = scale_pct(side).clamp(SCALE_MIN, SCALE_MAX) as f32 / 100.0;
            let o = opacity_pct(side).clamp(OPACITY_MIN, OPACITY_MAX) as f32 / 100.0;
            LATCHED_SCALE[side as usize].store(s.to_bits(), Ordering::Release);
            LATCHED_OPACITY[side as usize].store(o.to_bits(), Ordering::Release);
        }
        fill_hook::set_in_gameplay(true);
        cull_window::set_scale_contribution(latched_min_scale());
        let (s0, o0) = latched(0);
        let (s1, o1) = latched(1);
        log_info!(
            "{MOD_ID}: latch p1 s={s0:.2} op={o0:.2} | p2 s={s1:.2} op={o1:.2} | cull={:.0}",
            cull_window::cull_bound()
        );
    } else if prev == scene::GAMEPLAY {
        fill_hook::set_in_gameplay(false);
        fill_hook::clear_registry(true);
        lane_hook::reset();
        clear_latch();
        cull_window::clear_scale_contribution();
        log_info!("{MOD_ID}: gameplay exit — latch cleared");
    }
}

/// Reset the latch to identity (song end / mod disable).
fn clear_latch() {
    for side in 0..2 {
        LATCHED_SCALE[side].store(f32::to_bits(1.0), Ordering::Release);
        LATCHED_OPACITY[side].store(f32::to_bits(1.0), Ordering::Release);
    }
}

/// Every address the mod needs, resolved in `init`. All are load-bearing
/// (requirement A6): if ANY is missing the mod self-disables.
///
/// Raw pointers into the game module; valid for the process lifetime.
#[derive(Clone, Copy)]
pub(crate) struct ResolvedTargets {
    /// `render_sprite_final` — the shared per-quad fill (detour target).
    pub fill: *const u8,
    /// The per-pass note collector (diagnostic/log only; the patch site
    /// below lives inside it).
    pub collector: *const u8,
    /// The collector's `MOVSS XMM15,[RIP+disp32]` 720.0f load (patch site).
    pub collector_cull_site: *const u8,
    /// The measure-guideline draw function (capture-detour target).
    pub guideline_draw: *const u8,
    /// The guideline draw's `MOVSS XMM9,[RIP+disp32]` 720.0f load (patch site).
    pub guideline_cull_site: *const u8,
    /// The guideline's private bulk sprite emitter (transform-detour target).
    pub guideline_emitter: *const u8,
    /// Offset-0 vftables for renderer-instance classification.
    pub arrow_renderer_vtable: *const u8,
    pub spot_renderer_vtable: *const u8,
    pub judge_effect_renderer_vtable: *const u8,
    /// Player-object array global (presence read for side attribution).
    pub player_array: *const u8,
    /// CMovieClip pool-create wrapper (lane cover capture). Optional —
    /// lane styling is best-effort, not part of the load-bearing gate.
    pub pool_create: Option<*const u8>,
    /// NoteResultActor setup (receptor hit-flash capture). Optional.
    pub note_result_setup: Option<*const u8>,
}

// ── Mod implementation ──────────────────────────────────────────────

pub struct PlayfieldStylingMod {
    /// The full load-bearing target set, or `None` if any item failed to
    /// resolve (mod stays registered but inert).
    targets: Option<ResolvedTargets>,
    /// Game module bounds (int3-cave search range for the cull float slot).
    module_base: *const u8,
    module_size: usize,
    /// Recorded after `enable` — reported by `is_active()`.
    active: bool,
    /// Scene-change callback id (per-song latch), removed on disable.
    scene_cb_id: Option<usize>,
}

// Raw pointers into the game module; valid for the process lifetime, only
// dereferenced on the game thread inside detour installers / callbacks.
unsafe impl Send for PlayfieldStylingMod {}

impl PlayfieldStylingMod {
    pub fn new() -> Self {
        Self {
            targets: None,
            module_base: std::ptr::null(),
            module_size: 0,
            active: false,
            scene_cb_id: None,
        }
    }
}

/// Log one INFO line for a resolved item (module-relative offset + the
/// instruction bytes at the address, for deploy-log comparison against the
/// RE notes).
fn log_resolved(base: *const u8, name: &str, addr: *const u8, byte_count: usize) {
    let off = addr as usize - base as usize;
    if byte_count > 0 {
        let bytes: Vec<String> = (0..byte_count)
            .map(|i| format!("{:02X}", unsafe { *addr.add(i) }))
            .collect();
        log_info!("{MOD_ID}: {} @ +0x{:X} [{}]", name, off, bytes.join(" "));
    } else {
        log_info!("{MOD_ID}: {} @ +0x{:X}", name, off);
    }
}

impl Mod for PlayfieldStylingMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Playfield Styling"
    }
    fn description(&self) -> &str {
        "Per-player scale and opacity for arrows, receptors, guideline, and mines"
    }
    fn required_signatures(&self) -> &[&str] {
        // Empty on purpose — see the module doc comment. The load-bearing
        // set spans AOBs + derived addresses + RTTI walks, checked here in
        // `init` with `is_active()` self-report.
        &[]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let base = ctx.game_module.base;
        self.module_base = base;
        self.module_size = ctx.game_module.size;
        let sig = ctx.signatures;

        // Resolve every load-bearing item, logging each. Collect misses so a
        // failed boot log names everything that's wrong, not just the first.
        let mut missing: Vec<&str> = Vec::new();
        let mut get = |name: &'static str| -> *const u8 {
            match sig.get_address(name) {
                Some(a) => a,
                None => {
                    missing.push(name);
                    std::ptr::null()
                }
            }
        };

        let fill = get("render_sprite_final");
        let collector = get("note_collector");
        let collector_cull_site = get("collector_cull_site");
        let guideline_draw = get("guideline_draw");
        let guideline_cull_site = get("guideline_cull_site");
        let guideline_emitter = get("guideline_bulk_emitter");
        let arrow_vt = get("arrow_renderer_vtable");
        let spot_vt = get("spot_renderer_vtable");
        let judge_vt = get("judge_effect_renderer_vtable");
        let anchor = get("player_array_anchor");

        // Lane-clip capture helpers — OPTIONAL (best-effort lane styling; not
        // part of the load-bearing gate). Resolved separately so a miss here
        // never disables the core arrow/receptor/guideline styling.
        let pool_create = sig.get_address("cmovieclip_pool_create");
        let note_result_setup = sig.get_address("note_result_setup");

        // Decode the player-object array from the anchor's first insn
        // (`MOV RAX,[RIP+disp32]` — the center_arrows_single derivation).
        let player_array = if anchor.is_null() {
            std::ptr::null()
        } else {
            unsafe {
                if *anchor == 0x48 && *anchor.add(1) == 0x8B && *anchor.add(2) == 0x05 {
                    decode_rip_relative(anchor.add(3))
                } else {
                    missing.push("player_array_anchor (opcode mismatch)");
                    std::ptr::null()
                }
            }
        };

        if !missing.is_empty() {
            log_warn!(
                "{MOD_ID}: load-bearing set incomplete (missing: {}) — mod will be inert",
                missing.join(", ")
            );
            // Register anyway (visible/toggleable in the mod menu); enable()
            // self-disables.
            return true;
        }

        // Boot-log inventory: one line per item. The two cull sites include
        // their 9 instruction bytes (opcode prefix + disp32) so a deploy log
        // is directly comparable to the RE notes; the derivation chain has
        // already verified their RIP targets read 720.0f.
        log_resolved(base, "render_sprite_final", fill, 0);
        log_resolved(base, "note_collector", collector, 0);
        log_resolved(base, "collector_cull_site", collector_cull_site, 9);
        log_resolved(base, "guideline_draw", guideline_draw, 0);
        log_resolved(base, "guideline_cull_site", guideline_cull_site, 9);
        log_resolved(base, "guideline_bulk_emitter", guideline_emitter, 0);
        log_resolved(base, "arrow_renderer_vtable", arrow_vt, 0);
        log_resolved(base, "spot_renderer_vtable", spot_vt, 0);
        log_resolved(base, "judge_effect_renderer_vtable", judge_vt, 0);
        log_info!("{MOD_ID}: player_array (derived) @ {:p}", player_array);
        log_info!(
            "{MOD_ID}: lane helpers — pool_create={} note_result={} (best-effort)",
            pool_create.is_some(),
            note_result_setup.is_some()
        );

        self.targets = Some(ResolvedTargets {
            fill,
            collector,
            collector_cull_site,
            guideline_draw,
            guideline_cull_site,
            guideline_emitter,
            arrow_renderer_vtable: arrow_vt,
            spot_renderer_vtable: spot_vt,
            judge_effect_renderer_vtable: judge_vt,
            player_array,
            pool_create,
            note_result_setup,
        });
        true
    }

    fn enable(&mut self) {
        self.active = false;

        let targets = match self.targets {
            Some(t) => t,
            None => {
                log_warn!("{MOD_ID}: enabled but load-bearing set unavailable — inert");
                return;
            }
        };
        if !custom_options::is_available() {
            log_warn!("{MOD_ID}: custom_options unavailable — refusing enable");
            return;
        }
        if !scene_manager::is_available() {
            log_warn!("{MOD_ID}: scene_manager unavailable — refusing enable (no per-song latch)");
            return;
        }

        // ── All-or-nothing install gate (A6) ────────────────────────
        // Fill detour + BOTH cull-site patches (collector + guideline) +
        // BOTH guideline detours. Any failure → refuse enable, roll back,
        // no rows.
        if !fill_hook::install(&targets) {
            log_warn!("{MOD_ID}: fill hook unavailable — refusing enable");
            return;
        }
        if !cull_window::ensure_installed() {
            log_warn!("{MOD_ID}: cull-window patch unavailable — refusing enable");
            fill_hook::remove();
            return;
        }
        if !guideline_hook::acquire(
            guideline_hook::Consumer::PlayfieldStyling,
            targets.guideline_draw,
            targets.guideline_emitter,
            targets.player_array,
        ) {
            log_warn!("{MOD_ID}: guideline hooks unavailable — refusing enable");
            cull_window::clear_scale_contribution();
            fill_hook::remove();
            return;
        }

        // Lane background + lane cover styling — BEST-EFFORT (not part of the
        // gate above): a miss here logs a warning but leaves the core
        // arrow/receptor/guideline styling fully functional.
        if !lane_hook::install(&targets) {
            log_warn!("{MOD_ID}: lane styling unavailable (non-fatal) — arrows/receptors/guideline still styled");
        }

        // NOTE: no texture-filter smoothing. The arrow/receptor/freeze art
        // is PALETTE-INDEXED (`gs_screencommand_arrow` shader: the atlas
        // red channel is an index into a POINT-filtered 256-wide palette on
        // stage 1) — POINT sampling on the atlas is load-bearing; LINEAR
        // blends palette INDICES and produces severe color banding
        // (cabinet-verified, 2026-07-19). The pixelated look of scaled
        // arrows is inherent to palette-indexed art off its native grid.

        // Option rows — registered only now that the COMPLETE gate above
        // has passed (A6: a partial install never shows rows).
        register_rows();

        // Seed the atomic mirrors from the authoritative registry values. On
        // first enable `register_option` already primed them via `on_change`;
        // on a RE-enable it returns `Duplicate` and does NOT re-fire, so this
        // guarantees the latch reads current per-side values rather than a
        // stale default.
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

        // Per-song latch: snapshot at GAMEPLAY entry, clear at exit.
        let id = scene_manager::on_scene_change(Box::new(on_scene_change));
        self.scene_cb_id = Some(id);

        MOD_ENABLED.store(true, Ordering::Release);
        self.active = true;
        log_info!("{MOD_ID}: enabled (fill hook + cull patches + guideline hooks + lane styling + options)");
    }

    fn disable(&mut self) {
        MOD_ENABLED.store(false, Ordering::Release);
        self.active = false;
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        guideline_hook::release(guideline_hook::Consumer::PlayfieldStyling);
        fill_hook::remove();
        lane_hook::remove();
        // Identity latch + stock cull bound so a mid-song disable is
        // visually stock by the next frame (the detours also gate on
        // MOD_ENABLED; the disp32 patches stay installed but point at a
        // slot holding 720.0 — semantically stock, no code unpatching).
        clear_latch();
        cull_window::clear_scale_contribution();
        log_info!("{MOD_ID}: disabled");
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
