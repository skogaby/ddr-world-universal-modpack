//! Player Perspective Mod — per-player OVERHEAD / HALLWAY / DISTANT lane
//! view (from the StepMania perspective family).
//!
//! The non-overhead presets render the note lane in true perspective with
//! perspective-correct texturing. Each is a constant preset against one
//! parameterized perspective map (design §Data Models):
//!
//! - HALLWAY (SM tilt −1): notes spawn near a vanishing point and grow to
//!   full size as they approach the receptor row.
//! - DISTANT (SM tilt +1): the field recedes the other way — notes start
//!   large and shrink toward receptors, which sit toward the horizon
//!   (mid-field anchor + base zoom + a receptor-row realignment shift keep
//!   the field inside the stock lane rectangle with the receptors at their
//!   stock height).
//!
//! The skewed SM presets (INCOMING/SPACE) were implemented and then REMOVED
//! after live evaluation (maintainer call, 2026-07-31): not pleasant to
//! play, and their screen-center convergence exits the stock filter band in
//! versus. `PerspConstants::cx` remains a free constant, so re-adding them
//! is a preset-table change only.
//!
//! Mechanism (Option C1 from `docs/custom_arrow_renderer_research.md` §8):
//! the extended `.gsp` shader containers carry a second (perspective)
//! vertex-shader program that reconstructs pixel coordinates from NDC,
//! applies the hyperbolic map about the preset's anchor, and outputs a
//! real `w` — the GPU rasterizer then does the perspective divide and
//! perspective-correct UV interpolation (which is what keeps single-quad
//! freeze bodies straight and correctly tiled). Per side and per frame, the
//! DLL uploads that side's perspective parameters as VS constants (tag-0x14
//! record emitted BEFORE the lane pass) and then flips the `program` field
//! of the pass's SetShader records from 0 → 1 (`pass_rewrite`).
//!
//! Purely visual by construction: judging, timing windows, and scoring are
//! untouched (the note collector and judge never see the transform).

pub mod pass_rewrite;

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use crate::mods::config::{self, PlayerPerspectiveConfig};
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::custom_options::{self, EnumValue, RegisterSpec};
use crate::services::{cull_window, render_notes_hook, scene_manager};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

const MOD_ID: &str = "player-perspective";
/// The per-player option row id (label texture `seop_item_perspective`).
const OPT_ID: &str = "perspective";
/// Enum values (also the wire/persisted values). Values 3/4 (the removed
/// INCOMING/SPACE) may still arrive from old profiles — the clamps map
/// them to DISTANT; DISTANT keeps its original wire value.
const PERSP_OVERHEAD: i32 = 0;
const PERSP_HALLWAY: i32 = 1;
const PERSP_DISTANT: i32 = 2;

fn mode_name(mode: u32) -> &'static str {
    match mode as i32 {
        PERSP_HALLWAY => "HALLWAY",
        PERSP_DISTANT => "DISTANT",
        _ => "overhead",
    }
}

// ── Per-side latched state ──────────────────────────────────────────
//
// Written once per song at GAMEPLAY entry (scene callback), consumed by the
// pass_rewrite hot path for the whole song. Values changed mid-song are
// deliberately not picked up (repo convention: options apply next song).

/// Latched per-side mode (`PERSP_*` value; 0 = OVERHEAD/stock).
static LATCHED_MODE: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
/// Latched cabinet tunables (f32 bits), shared by both sides. Which of them
/// a side consumes depends on its latched mode (`latched_params`).
static LATCHED_FOCAL: AtomicU32 = AtomicU32::new(0);
static LATCHED_DISTANT_FOCAL: AtomicU32 = AtomicU32::new(0);
static LATCHED_DISTANT_ZOOM: AtomicU32 = AtomicU32::new(0);
/// Fast whole-mod gate for the hot path.
static MOD_ENABLED: AtomicBool = AtomicBool::new(false);

/// Per-side option-value mirrors (the `OnChangeFn` is a plain `fn(side,
/// value)`, so each option mirrors into its own atomics — the
/// playfield_styling pattern). Consulted as the latch's fallback when the
/// authoritative registry read fails.
static PERSPECTIVE_VALUE: [AtomicI32; 2] = [
    AtomicI32::new(PERSP_OVERHEAD),
    AtomicI32::new(PERSP_OVERHEAD),
];

fn on_perspective_change(side: u8, value: i32) {
    if let Some(a) = PERSPECTIVE_VALUE.get(side as usize) {
        a.store(
            value.clamp(PERSP_OVERHEAD, PERSP_DISTANT),
            Ordering::Release,
        );
    }
}

/// Authoritative per-side read (registry first, atomic mirror fallback).
fn perspective_value(side: u8) -> i32 {
    custom_options::get_value(side, OPT_ID)
        .unwrap_or_else(|| {
            PERSPECTIVE_VALUE
                .get(side as usize)
                .map(|a| a.load(Ordering::Acquire))
                .unwrap_or(PERSP_OVERHEAD)
        })
        .clamp(PERSP_OVERHEAD, PERSP_DISTANT)
}

/// Register the per-player PERSPECTIVE enum row. Called only after the
/// enable-time install gate has passed (a partial install never shows the
/// row). `PersistMode::Full` (builder default): network save + load + JSON
/// cache — the value rides the player profile.
fn register_rows() {
    let spec = RegisterSpec::enum_values(
        OPT_ID,
        vec![
            EnumValue::with_preview(PERSP_OVERHEAD, "seop_op_overhead", "overhead")
                .display_label("OVERHEAD"),
            EnumValue::with_preview(PERSP_HALLWAY, "seop_op_hallway", "hallway")
                .display_label("HALLWAY"),
            EnumValue::with_preview(PERSP_DISTANT, "seop_op_distant", "distant")
                .display_label("DISTANT"),
        ],
    )
    .display_name("Perspective")
    .description("StepMania-style lane camera: flat, receding, or distant view")
    .default_value(PERSP_OVERHEAD)
    .on_change(on_perspective_change);
    match custom_options::register_option(spec) {
        Ok(_) => {}
        Err(custom_options::RegisterError::Duplicate { .. }) => {
            // Re-enable after a disable: rows stay registered (no
            // unregister API); the enable-time reseed re-primes the
            // mirrors. Not an error.
        }
        Err(e) => log_warn!("{MOD_ID}: option registration failed: {e}"),
    }
}

/// Perspective parameters for one side, latched at GAMEPLAY entry and
/// consumed by `pass_rewrite` and the guideline transform. Carries the
/// preset mode plus the cabinet tunables that preset resolves to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PerspParams {
    /// Latched preset (`PERSP_HALLWAY..`; never `PERSP_OVERHEAD` — overhead
    /// sides simply have no params).
    pub mode: i32,
    /// Focal length k (px).
    pub k: f32,
    /// Base zoom about the anchor (1.0 = none; <1 for DISTANT's
    /// containment compensation).
    pub z0: f32,
}

/// The resolved per-pass constant block — c48/c49 for the perspective VS,
/// and the identical CPU map ([`PerspConstants::map_point`]) for guidelines
/// and the receptor hit flash. One source of truth: every consumer feeds
/// its own per-pass `pos_y`/`cx_lane`/`y_dir` reads through
/// `compute_constants` (design §Data Models).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PerspConstants {
    /// c48.x — the s=z0 fixed point / Y convergence anchor.
    pub anchor_y: f32,
    /// c48.y — X convergence target (the lane center; kept as a free
    /// constant so a skewed preset stays a table change).
    pub cx: f32,
    /// c48.z — focal length (px).
    pub k: f32,
    /// c48.w — effective receding direction (preset tilt ⊗ reverse flag).
    pub dir: f32,
    /// c49.x — signed-distance clamp guarding the `k + d → 0` blow-up
    /// (missed-note growth capped at `1/(1−0.5) = 2×`).
    pub d_min: f32,
    /// c49.y — base zoom. MUST never be emitted as 0.0: the perspective VS
    /// multiplies its scale by this register.
    pub z0: f32,
    /// c49.z — rigid vertical realignment shift, chosen so the receptor
    /// row maps back to its stock screen Y (maintainer feedback from the
    /// first DISTANT deploy: un-shifted, the row sat ~57 px too low).
    /// Exactly 0 for HALLWAY (its anchor IS the receptor row).
    pub ty: f32,
}

impl PerspConstants {
    /// The scale factor of the map at screen-space `y` — identical math to
    /// the perspective VS (`s = z0·k/(k + clamp(d))`).
    pub fn scale_at(&self, y: f32) -> f32 {
        let d = ((y - self.anchor_y) * self.dir).max(self.d_min);
        self.z0 * self.k / (self.k + d)
    }

    /// Apply the full map to a screen-space point — the exact transform the
    /// perspective VS applies to a vertex at `(x, y)`. Returns
    /// `(x', y', s)`; consumers scaling art (guidelines, hit flash) reuse
    /// `s` as the size factor.
    pub fn map_point(&self, x: f32, y: f32) -> (f32, f32, f32) {
        let s = self.scale_at(y);
        (
            self.cx + (x - self.cx) * s,
            self.anchor_y + (y - self.anchor_y) * s + self.ty,
            s,
        )
    }
}

/// `d_min = PASSED_GROWTH_CLAMP · k` (see `PerspConstants::d_min`).
const PASSED_GROWTH_CLAMP: f32 = -0.5;

/// Lane half-widths (screen px, 1280-wide space) — 4 panels vs 8.
const HALF_WIDTH_SINGLE: f32 = 192.0;
const HALF_WIDTH_DOUBLE: f32 = 384.0;

/// Lane center X from the renderer/guideline object's lane left edge.
pub(crate) fn lane_center(is_double: bool, lane_left_x: f32) -> f32 {
    lane_left_x
        + if is_double {
            HALF_WIDTH_DOUBLE
        } else {
            HALF_WIDTH_SINGLE
        }
}

/// Screen-center Y / half-height (720-tall space), for the entrance-edge
/// derivation `entrance = 360 + 360·y_dir`.
const SCREEN_CENTER_Y: f32 = 360.0;

/// Resolve one side's latched preset into the per-pass constant block
/// (the design's §Data Models table).
///
/// Inputs are the pass's own reads (unchanged from the shipped mod):
/// `pos_y` = receptor row screen Y, `cx_lane` = lane center X, `y_dir` =
/// reverse flag (+1 receptors-top / −1 receptors-bottom).
///
/// - HALLWAY: anchor at the receptor row, field recedes toward the
///   entrance edge, no base zoom, no shift.
/// - DISTANT: anchor mid-field between the receptor row and the entrance
///   edge (`entrance = 360 + 360·y_dir` — the screen edge notes enter
///   from), field recedes toward/past the receptors (`dir = −y_dir`), base
///   zoom `z0` about the anchor for containment, and a rigid shift `ty`
///   putting the (shrunken) receptor row back at its stock height.
pub(crate) fn compute_constants(
    p: &PerspParams,
    pos_y: f32,
    cx_lane: f32,
    y_dir: f32,
) -> PerspConstants {
    let (anchor_y, dir) = if p.mode == PERSP_DISTANT {
        let entrance = SCREEN_CENTER_Y + SCREEN_CENTER_Y * y_dir;
        ((pos_y + entrance) * 0.5, -y_dir)
    } else {
        (pos_y, y_dir)
    };
    let mut c = PerspConstants {
        anchor_y,
        cx: cx_lane,
        k: p.k,
        dir,
        d_min: PASSED_GROWTH_CLAMP * p.k,
        z0: p.z0,
        ty: 0.0,
    };
    // Realignment: shift the whole mapped field so the receptor row lands
    // back at pos_y. Zero when the anchor is the receptor row (HALLWAY):
    // there the map's fixed point already is pos_y.
    let (_, mapped_receptor_y, _) = c.map_point(0.0, pos_y);
    c.ty = pos_y - mapped_receptor_y;
    c
}

/// The latched perspective params for a side, or `None` if that side is
/// OVERHEAD (or the mod is disabled). Hot-path safe (atomics only).
pub(crate) fn latched_params(side: u8) -> Option<PerspParams> {
    if !MOD_ENABLED.load(Ordering::Acquire) {
        return None;
    }
    let idx = (side as usize).min(1);
    let mode = LATCHED_MODE[idx].load(Ordering::Acquire) as i32;
    if mode == PERSP_OVERHEAD {
        return None;
    }
    let (k, z0) = if mode == PERSP_DISTANT {
        (
            f32::from_bits(LATCHED_DISTANT_FOCAL.load(Ordering::Acquire)),
            f32::from_bits(LATCHED_DISTANT_ZOOM.load(Ordering::Acquire)),
        )
    } else {
        (f32::from_bits(LATCHED_FOCAL.load(Ordering::Acquire)), 1.0)
    };
    Some(PerspParams { mode, k, z0 })
}

// ── Published per-side constants (cross-mod consumers) ──────────────
//
// The receptor hit flash is an AFP clip (playfield_styling::lane_hook), not
// a lane-pass quad — the VS never touches it, so its correction needs the
// side's RESOLVED constants CPU-side. pass_rewrite publishes them from the
// notes/spot pass (the only place the true per-side geometry — receptor
// row, lane center, reverse flag — is read off the live renderer); they are
// constant within a song. Publication-flag-last/`Acquire`-first so a reader
// never sees a half-written block; cleared at song boundaries.

static PUBLISHED_FLAG: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static PUBLISHED: [[AtomicU32; 7]; 2] = [
    [const { AtomicU32::new(0) }; 7],
    [const { AtomicU32::new(0) }; 7],
];

pub(crate) fn publish_constants(side: u8, c: &PerspConstants) {
    let idx = (side as usize).min(1);
    let fields = [c.anchor_y, c.cx, c.k, c.dir, c.d_min, c.z0, c.ty];
    for (slot, v) in PUBLISHED[idx].iter().zip(fields) {
        slot.store(v.to_bits(), Ordering::Relaxed);
    }
    PUBLISHED_FLAG[idx].store(true, Ordering::Release);
}

/// The side's resolved constants as last published by the lane pass, or
/// `None` if no perspective pass has run yet this song.
pub(crate) fn published_constants(side: u8) -> Option<PerspConstants> {
    let idx = (side as usize).min(1);
    if !PUBLISHED_FLAG[idx].load(Ordering::Acquire) {
        return None;
    }
    let f: Vec<f32> = PUBLISHED[idx]
        .iter()
        .map(|s| f32::from_bits(s.load(Ordering::Relaxed)))
        .collect();
    Some(PerspConstants {
        anchor_y: f[0],
        cx: f[1],
        k: f[2],
        dir: f[3],
        d_min: f[4],
        z0: f[5],
        ty: f[6],
    })
}

pub(crate) fn clear_published() {
    PUBLISHED_FLAG[0].store(false, Ordering::Release);
    PUBLISHED_FLAG[1].store(false, Ordering::Release);
}

/// True when either side latched a non-OVERHEAD preset this song.
pub(crate) fn any_side_latched() -> bool {
    MOD_ENABLED.load(Ordering::Acquire)
        && (LATCHED_MODE[0].load(Ordering::Acquire) != 0
            || LATCHED_MODE[1].load(Ordering::Acquire) != 0)
}

fn clear_latch() {
    LATCHED_MODE[0].store(0, Ordering::Release);
    LATCHED_MODE[1].store(0, Ordering::Release);
}

// ── Scene latch ─────────────────────────────────────────────────────

fn on_scene_change(prev: i32, next: i32) {
    if next == scene::GAMEPLAY {
        if !MOD_ENABLED.load(Ordering::Acquire) {
            return;
        }
        let cfg = active_config();
        let k = cfg.hallway_focal.clamp(100.0, 100_000.0);
        let dk = cfg.distant_focal.clamp(100.0, 100_000.0);
        let dz = cfg.distant_zoom.clamp(0.1, 1.0);
        let modes = [perspective_value(0) as u32, perspective_value(1) as u32];
        LATCHED_MODE[0].store(modes[0], Ordering::Release);
        LATCHED_MODE[1].store(modes[1], Ordering::Release);
        LATCHED_FOCAL.store(k.to_bits(), Ordering::Release);
        LATCHED_DISTANT_FOCAL.store(dk.to_bits(), Ordering::Release);
        LATCHED_DISTANT_ZOOM.store(dz.to_bits(), Ordering::Release);
        pass_rewrite::reset_song_state();
        // Receptor-flash tracking shares playfield_styling's lane machinery
        // (idempotent with that mod's own scene callback; required when it
        // is config-disabled).
        crate::mods::playfield_styling::lane_scene_transition(true);
        if modes.iter().any(|&m| m != 0) {
            // Widen the collector's cull window to the draw distance so
            // notes exist out to the horizon (HALLWAY compresses the
            // approach region) / out past the entrance edge (DISTANT's
            // receptor-row realignment shifts the whole field up, pulling
            // content beyond the stock 720 px bound on screen). Failure
            // here is visual-only (pop-in at the stock window) — the
            // mechanism itself keeps working.
            cull_window::set_distance_contribution(cfg.hallway_draw_distance);
            log_info!(
                "{MOD_ID}: latch p1={} p2={} (k={k:.0}, dk={dk:.0}, dz={dz:.2}, draw_distance={:.0}, cull={:.0})",
                mode_name(modes[0]),
                mode_name(modes[1]),
                cfg.hallway_draw_distance,
                cull_window::cull_bound()
            );
        }
    } else if prev == scene::GAMEPLAY {
        clear_latch();
        cull_window::clear_distance_contribution();
        pass_rewrite::reset_song_state();
        crate::mods::playfield_styling::lane_scene_transition(false);
    }
}

fn active_config() -> PlayerPerspectiveConfig {
    config::get()
        .and_then(|c| c.player_perspective.clone())
        .unwrap_or_default()
}

// ── Mod implementation ──────────────────────────────────────────────

pub struct PlayerPerspectiveMod {
    /// Dispatcher callback handles (unregistered on disable).
    pre_handle: Option<render_notes_hook::CallbackHandle>,
    post_handle: Option<render_notes_hook::CallbackHandle>,
    /// Scene-change callback id (per-song latch), removed on disable.
    scene_cb_id: Option<usize>,
    /// Guideline-transform targets (derived signatures + decoded player
    /// array), resolved at init for the shared guideline-hook acquire.
    guideline_draw: *const u8,
    guideline_emitter: *const u8,
    player_array: *const u8,
    /// NoteResultActor setup (receptor hit-flash capture), resolved at init
    /// for the shared flash acquire (best-effort).
    note_result_setup: Option<*const u8>,
    active: bool,
}

unsafe impl Send for PlayerPerspectiveMod {}

impl PlayerPerspectiveMod {
    pub fn new() -> Self {
        Self {
            pre_handle: None,
            post_handle: None,
            scene_cb_id: None,
            guideline_draw: std::ptr::null(),
            guideline_emitter: std::ptr::null(),
            player_array: std::ptr::null(),
            note_result_setup: None,
            active: false,
        }
    }
}

impl Mod for PlayerPerspectiveMod {
    fn id(&self) -> &str {
        MOD_ID
    }
    fn name(&self) -> &str {
        "Player Perspective"
    }
    fn description(&self) -> &str {
        "StepMania-family lane perspectives (per-player): Hallway, Distant"
    }
    fn required_signatures(&self) -> &[&str] {
        // The render_notes detour itself is owned by services::
        // render_notes_hook; `default_shader` feeds the pass rewrite's
        // shader-object filter (shock/mine passes bind it).
        &["default_shader"]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        pass_rewrite::resolve(ctx.signatures);
        // Guideline-transform targets (best-effort — hallway still renders
        // without guideline convergence). Same derivations playfield_styling
        // uses; the player array decodes from its anchor's first insn
        // (`MOV RAX,[RIP+disp32]`).
        self.guideline_draw = ctx
            .signatures
            .get_address("guideline_draw")
            .unwrap_or(std::ptr::null());
        self.guideline_emitter = ctx
            .signatures
            .get_address("guideline_bulk_emitter")
            .unwrap_or(std::ptr::null());
        self.player_array = ctx
            .signatures
            .get_address("player_array_anchor")
            .map(|anchor| unsafe {
                if *anchor == 0x48 && *anchor.add(1) == 0x8B && *anchor.add(2) == 0x05 {
                    crate::core::scanner::decode_rip_relative(anchor.add(3))
                } else {
                    std::ptr::null()
                }
            })
            .unwrap_or(std::ptr::null());
        self.note_result_setup = ctx.signatures.get_address("note_result_setup");
        true
    }

    fn enable(&mut self) {
        if self.active {
            return;
        }
        // All-or-nothing install gate: without the dispatcher (or the
        // command-list global it wraps) the mechanism cannot work at all.
        // Per-frame nulls are handled in the callbacks themselves.
        if !render_notes_hook::is_available() {
            log_warn!("{MOD_ID}: render_notes dispatcher unavailable -- mod inert");
            return;
        }

        // Pre @ Normal: constants + window snapshot. Post @ Late: record
        // rewrite — AFTER mine_render's post @ Normal so the mine pass's
        // SetShader(default) records fall inside the rewritten window.
        self.pre_handle = render_notes_hook::register_pre(
            render_notes_hook::Priority::Normal,
            pass_rewrite::pre_render_notes,
        );
        self.post_handle = render_notes_hook::register_post(
            render_notes_hook::Priority::Late,
            pass_rewrite::post_render_notes,
        );
        if self.pre_handle.is_none() || self.post_handle.is_none() {
            log_warn!("{MOD_ID}: dispatcher registration failed -- mod inert");
            if let Some(h) = self.pre_handle.take() {
                render_notes_hook::unregister(h);
            }
            if let Some(h) = self.post_handle.take() {
                render_notes_hook::unregister(h);
            }
            return;
        }

        self.scene_cb_id = Some(scene_manager::on_scene_change(Box::new(on_scene_change)));

        // Cull-window extension (draw distance). Failure is a visual-only
        // degrade (notes pop in at the stock 720 window), NOT part of the
        // install gate — the perspective mechanism itself still works.
        if !cull_window::ensure_installed() {
            log_warn!(
                "{MOD_ID}: cull-window extension unavailable — hallway will pop notes at the stock window"
            );
        }

        // Guideline transform (shared detours with playfield_styling;
        // refcounted install). Best-effort: without it hallway renders but
        // measure lines stay straight.
        if !crate::mods::playfield_styling::guideline_acquire_perspective(
            self.guideline_draw,
            self.guideline_emitter,
            self.player_array,
        ) {
            log_warn!("{MOD_ID}: guideline hooks unavailable — measure lines stay flat");
        }

        // Receptor hit-flash tracking (shared note-result capture with
        // playfield_styling; refcounted install). Best-effort: without it
        // the flash stays at the stock receptor position/size.
        if !crate::mods::playfield_styling::flash_acquire_perspective(self.note_result_setup) {
            log_warn!("{MOD_ID}: flash capture unavailable — hit flash stays at stock position");
        }

        // The player-facing PERSPECTIVE row — registered only now that the
        // install gate above has passed (a partial install never shows the
        // row). Without custom_options the mod stays enabled but inert
        // (every side reads the OVERHEAD default).
        if custom_options::is_available() {
            register_rows();
            // Seed the atomic mirrors from the authoritative registry
            // values: on first enable `register_option` primed them via
            // `on_change`; on a RE-enable it returns `Duplicate` and does
            // NOT re-fire.
            for side in 0u8..2 {
                on_perspective_change(
                    side,
                    custom_options::get_value(side, OPT_ID).unwrap_or(PERSP_OVERHEAD),
                );
            }
        } else {
            log_warn!("{MOD_ID}: custom_options unavailable — PERSPECTIVE row not shown");
        }

        MOD_ENABLED.store(true, Ordering::Release);
        self.active = true;

        let cfg = active_config();
        log_info!(
            "{MOD_ID}: enabled (hallway_focal={:.0}, draw_distance={:.0}, distant_focal={:.0}, distant_zoom={:.2})",
            cfg.hallway_focal,
            cfg.hallway_draw_distance,
            cfg.distant_focal,
            cfg.distant_zoom
        );
    }

    fn disable(&mut self) {
        MOD_ENABLED.store(false, Ordering::Release);
        clear_latch();
        cull_window::clear_distance_contribution();
        crate::mods::playfield_styling::guideline_release_perspective();
        crate::mods::playfield_styling::flash_release_perspective();
        // Drop the shared gameplay flag this mod may have raised — but only
        // when playfield_styling isn't running (it owns the flag then, and
        // clearing it mid-song would kill that mod's styling).
        if !crate::mods::playfield_styling::is_enabled() {
            crate::mods::playfield_styling::lane_scene_transition(false);
        }
        pass_rewrite::reset_song_state();
        if let Some(h) = self.pre_handle.take() {
            render_notes_hook::unregister(h);
        }
        if let Some(h) = self.post_handle.take() {
            render_notes_hook::unregister(h);
        }
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        self.active = false;
        log_info!("{MOD_ID}: disabled");
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
