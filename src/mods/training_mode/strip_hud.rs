//! Chart-strip timeline HUD — live sourcing + texture pipeline (Training
//! Mode Step 6 task-02, design R7 as amended 2026-08-14).
//!
//! Drives the pure synthesis layer (`strip_synth`) from live game state,
//! once per song:
//!
//! 1. **Arm** at GAMEPLAY entry (scene callback — no actors exist yet).
//! 2. **Snapshot** on the first judge dispatch (game thread; notes,
//!    palette manager, arrow renderer and player options are all live by
//!    then): side → `song_reset::decoded_notes` + `chart_end_raw`; the
//!    player's arrow design via the `player_option_table` chain
//!    (`Option+0x60`); per-note palette rows by CALLING the game's own
//!    quantization row selector (`arrow_row_selector`, resolved by AOB)
//!    with the live `screen::ArrowRenderer` (actor+0x148). Bar COLORS
//!    ship from the fixed offline ramp (`flat_ramp_palette` — the
//!    approved host-render recipe; maintainer directive 2026-08-15
//!    round 3). The live `screen::ArrowPalette` walk (actor+0x130 →
//!    generator table at *(mgr+0x28), each generator's
//!    `evaluate(rowArg, column, phase)` peak-phase swept — the game's
//!    own update loop pointed at a private buffer) stays implemented
//!    behind `USE_LIVE_PALETTE` for revisit: even at its beat-cycle
//!    peak it read flat in situ next to the ramp.
//! 3. **Synthesize** on a background thread (generation-tokened):
//!    measure enumeration (4096-tick bars through
//!    `seek::raw_for_display` — the chart's own time mapping), then
//!    `strip_synth::render_strip_bars` + `encode_png` + a cache-file
//!    write. BAR MODE is the shipped style (maintainer-approved
//!    2026-08-14 after the density finding: noteskin glyphs are
//!    unreadable on real expert charts): taps/heads as 1-px
//!    quantization-colored bars, freeze bodies as solid rects, shocks
//!    full-width and mines per-panel in a fixed bright blue-white. Bar
//!    colors go through `strip_synth::row_bar_color` over the chosen
//!    palette; no noteskin sheet is read at all (one less failure mode).
//!    The noteskin rasterizer stays available (pure + tested) for
//!    future zoomed/alternate views.
//! 4. **Load + show** through the mine-texture FileManager pipeline
//!    (`services::asset_loader`): per-song stem `training_strip_<gen>`,
//!    lazy per-frame resolve poll, ONE reused ImageWidget (widget nodes
//!    are permanently consumed — created once, resized per song), shown
//!    while the texture is resolved AND the latched TIMELINE PLACEMENT
//!    row shows the HUD (round-4 UX amendment 2026-08-15: the row's
//!    OFF/LEFT/RIGHT value is the ONE visibility input — placement OFF
//!    skips the whole per-song pipeline at the arm gate; the old
//!    training-session-active predicate is gone).
//! 5. **Teardown** at GAMEPLAY exit / supersession: hide, release the
//!    FileManager handle (paired exactly-once), delete the cache file.
//!
//! Runtime safety: the actor's palette-manager and renderer objects are
//! validated by RTTI vtable identity (`arrow_palette_vtable` /
//! `arrow_renderer_vtable`, resolved at init) before ANY use — offset
//! drift on a future build degrades to the flat-color ladder, never a
//! wild virtual call. Everything engine-facing runs on the game thread
//! (the judge callback); the background thread touches only owned
//! buffers. Fail-open per design §6: any missing piece costs the strip
//! (or just its live colors), never the session. The color rungs
//! (selector / renderer RTTI / palette) log distinct one-shot WARNs —
//! the snapshot runs once per song — so a re-demo log identifies the
//! failing rung; `DDR_STRIP_FAULT` injection announces itself loudly.
//!
//! RE record: `docs/chart_strip_hud_research.md` + the task-02 working
//! record (actor-init decompile: manager at `param_1[0x26]` = +0x130,
//! ArrowRenderer at `param_1[0x29]` = +0x148 — the task text's +0x138 is
//! the SpotRenderer).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::mods::training_mode::bounds;
use crate::mods::training_mode::strip_synth::{
    self, StripLayout, StripNote, StripPalette, StripScene,
};
use crate::services::song_reset::seek::{self, NoteView};
use crate::services::{asset_loader, judge_hook, scene_manager, song_reset, widget_renderer};
use crate::types::scenes::scene;
use crate::widgets::image_widget::{ImageWidget, ImageWidgetConfig};
use crate::widgets::text_widget::{TextAlignment, TextWidget};
use crate::{log_debug, log_info, log_warn};

// ── Engine offsets (Ghidra-verified on 20260616 + 20260721) ──────────

/// GamePlayActor: play side (i32 0/1) — mine_render's validated read.
const ACTOR_PLAY_SIDE: usize = 0x84;
/// GamePlayActor: the `screen::ArrowPalette` manager (actor-init
/// `param_1[0x26]` store; RTTI-validated before use).
const ACTOR_PALETTE_MGR: usize = 0x130;
/// GamePlayActor: the `screen::ArrowRenderer` (actor-init `param_1[0x29]`
/// store — NOT +0x138, which is the SpotRenderer; RTTI-validated).
const ACTOR_ARROW_RENDERER: usize = 0x148;
/// ArrowPalette manager: per-frame beat phase (i32).
const MGR_PHASE: usize = 0x18;
/// ArrowPalette manager: POINTER to the row→generator table (8-byte
/// slots; the factory writes slots through `*(mgr+0x28)`).
const MGR_TABLE: usize = 0x28;
/// ArrowPalette manager: the generator table's end pointer (vector end —
/// the update loop derives `count` from it).
const MGR_TABLE_END: usize = 0x30;
/// The palette rows the strip needs: tap rows 1..4 (the selector's
/// output range) + row 8 (the idle-freeze encoding the fill uses).
const NEEDED_ROWS: [usize; 5] = [1, 2, 3, 4, 8];
/// The generator-table fold rules' constants (the update loop's shape).
const FREEZE_SLOT: usize = 7;
/// Bar-color source (maintainer directive, 2026-08-15 round 3): the
/// FIXED offline ramp ([`flat_ramp_palette`] — the exact recipe the
/// approved host-side renders used). The live generator walk
/// ([`walk_palette`], peak-phase swept) stays implemented behind this
/// switch for a future revisit — even at its beat-cycle peak the live
/// read looked flat in situ next to the ramp. The per-note ROW
/// selection stays live either way (the game's own quantization
/// classifier).
const USE_LIVE_PALETTE: bool = false;

/// Quantization → palette-row selector: `u32 selector(renderer, beat)`.
type SelectorFn = unsafe extern "C" fn(*mut u8, i32) -> u32;
/// Generator vtable slot 1: `u32 evaluate(this, rowArg, column, phase)`.
type EvaluateFn = unsafe extern "C" fn(*mut u8, i32, i32, i32) -> u32;

// ── Strip geometry (task-03 finalizes placement) ─────────────────────

/// Per-column glyph edge in px.
const COLUMN_PX: u32 = 14;
/// Strip height in px (720-px screen, centered vertically).
const STRIP_HEIGHT_PX: u32 = 620;
/// Right-edge margin (default placement; the TIMELINE PLACEMENT row
/// lands in task-03).
const EDGE_MARGIN_PX: f32 = 8.0;
/// Guideline color: subtle white (one line per measure).
const GUIDELINE_RGBA: [u8; 4] = [255, 255, 255, 48];
/// Strip background: translucent dark backing so the chart reads over
/// any stage background.
const BACKGROUND_RGBA: [u8; 4] = [16, 16, 24, 200];
/// Defensive cap on enumerated measures (a corrupt display-time mapping
/// must not spin the enumeration).
const MAX_MEASURES: usize = 1024;

/// Cache directory + stem prefix (per-song stems — the risk-1 refresh
/// probe's chosen shape; paired release + file delete per song).
const CACHE_DIR: &str = "./data_mods/_cache/training_hud";
const STEM_PREFIX: &str = "training_strip_";

// ── Overlay (task-03: cursor / A-B / veil / readout) ─────────────────

/// The repo-shipped marker asset: a 4×4 PNG whose rows are
/// dark/white/white/dark — stretched wide it is an outlined line for
/// free; the veil/track sample only the white center rows via UV.
const MARKER_TEX_PATH: &str = "./data_mods/training_mode/tex/training_marker.png";
const MARKER_TEX_STEM: &str = "training_marker";
/// UV v-range selecting only the marker texture's white center rows
/// (veil/track — stretching the outline rows tall would band them).
const MARKER_UV_CENTER: (f32, f32) = (0.30, 0.70);

/// Cursor: 6 px tall, overhanging the strip on both sides (re-demo
/// sizing — the 4 px/3 px original read too small live).
const CURSOR_H: f32 = 6.0;
const CURSOR_OVERHANG: f32 = 5.0;
/// A/B marker lines: 3 px, smaller overhang than the cursor.
const MARKER_H: f32 = 3.0;
const MARKER_OVERHANG: f32 = 3.0;
/// ABGR tints (ImageWidget::set_color): cursor = yellow (the marker
/// texture's white center rows multiply to pure yellow; the dark
/// outline rows stay dark), A = bright green, B = bright red, veil =
/// mostly-opaque blue tint (the low-alpha white original was
/// imperceptible live), fallback track = the strip's dark backing.
const COLOR_CURSOR: u32 = 0xFF00_FFFF;
const COLOR_A: u32 = 0xFF50_FF40;
const COLOR_B: u32 = 0xFF40_50FF;
const COLOR_VEIL: u32 = 0xA0FF_7828;
const COLOR_TRACK: u32 = 0xC818_1010;
/// Readout: small white text centered under the strip.
const READOUT_SCALE: f32 = 0.4;
const READOUT_GAP_PX: f32 = 8.0;
/// Estimated per-glyph advance at scale 1.0 (calibrated from the
/// 2026-08-15 demo screenshot: ~17 px/char) — used only to clamp the
/// centered readout onto the screen (LEFT placement puts the strip's
/// center 36 px from the edge; an unclamped center clips the leading
/// "0:").
const READOUT_GLYPH_PX: f32 = 17.0;

// ── Resolved addresses (set once at init) ────────────────────────────

static SELECTOR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static PALETTE_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
static RENDERER_VTABLE: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// Whether the HUD's lifecycle callbacks are registered and live.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Render-pump supersession token (toast.rs's model): each activate
/// bumps it and queues a fresh pump; a stale pump sees the mismatch and
/// stops requeueing.
static PUMP_GENERATION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// TIMELINE PLACEMENT per-side values (0 OFF / 1 LEFT / 2 RIGHT — the
/// row's own enum encoding; round-4 UX amendment 2026-08-15: this row
/// ALONE dictates HUD visibility) and the per-song latch (placement
/// edits apply at the next song — the strip's own x is bind-time, and a
/// mid-song flip moving only the markers would tear the ensemble apart).
/// Unknown values fail safe to OFF.
const PLACEMENT_OFF: i32 = 0;
const PLACEMENT_LEFT: i32 = 1;
const PLACEMENT_RIGHT: i32 = 2;
static PLACEMENT: [AtomicI32; 2] = [AtomicI32::new(PLACEMENT_OFF), AtomicI32::new(PLACEMENT_OFF)];
static LATCHED_PLACEMENT: AtomicI32 = AtomicI32::new(PLACEMENT_OFF);

// ── Per-song state machine ───────────────────────────────────────────

enum Phase {
    /// Not in a song (or torn down).
    Idle,
    /// GAMEPLAY entered; the first judge dispatch runs the snapshot.
    Armed,
    /// Background synthesis in flight for `generation`.
    Synthesizing,
    /// The PNG is on disk; the next judge tick issues the load.
    PngReady {
        path: String,
        stem: String,
        width: u32,
        height: u32,
    },
    /// FileManager load issued; polling for the texture to register.
    Loading {
        name_hash: u32,
        width: u32,
        height: u32,
    },
    /// Texture bound to the widget; visibility follows the placement gate.
    Resolved,
}

struct SongState {
    phase: Phase,
    /// Monotonic per-song token; a stale background post is discarded.
    generation: u32,
    /// One-WARN-per-song latch for the fail-open ladder.
    warned: bool,
    /// The cache file currently owned by this song (deleted at teardown).
    file_path: Option<String>,
}

impl SongState {
    const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            generation: 0,
            warned: false,
            file_path: None,
        }
    }
}

static SONG: Lazy<Mutex<SongState>> = Lazy::new(|| Mutex::new(SongState::new()));

/// Background-thread → game-thread hand-off: (generation, path, stem,
/// display width, display height).
type PendingPng = (u32, String, String, u32, u32);
static PENDING: Lazy<Mutex<Option<PendingPng>>> = Lazy::new(|| Mutex::new(None));

/// The ONE strip widget (a widget node is permanently consumed — created
/// on first use, reused and resized per song). Render-thread only.
struct WidgetSlot(Option<ImageWidget>);
// SAFETY: the native pointer is game memory valid for the process
// lifetime; all mutation happens on the render thread (toast.rs's model).
unsafe impl Send for WidgetSlot {}
static WIDGET: Lazy<Mutex<WidgetSlot>> = Lazy::new(|| Mutex::new(WidgetSlot(None)));

/// The current song's overlay geometry, published by the snapshot (the
/// overlay runs even when the strip texture never resolves — the
/// fail-open track). Cleared at teardown.
#[derive(Clone, Copy)]
struct SongGeometry {
    columns: u32,
    chart_end_ms: i32,
    reverse: bool,
}
static GEOMETRY: Lazy<Mutex<Option<SongGeometry>>> = Lazy::new(|| Mutex::new(None));

/// The overlay's widget set (render-thread only; created lazily once —
/// widget nodes are permanently consumed). Creation order = z order:
/// track (bottom), veil, A, B, cursor; the readout TextWidget is
/// independent chrome.
struct OverlayWidgets {
    track: Option<ImageWidget>,
    veil: Option<ImageWidget>,
    line_a: Option<ImageWidget>,
    line_b: Option<ImageWidget>,
    cursor: Option<ImageWidget>,
    readout: Option<TextWidget>,
    /// The resolved marker texture handle (None until the async load
    /// lands; image widgets are only created after it).
    marker_texture: Option<i32>,
    /// Whether the marker-asset load request has been issued.
    marker_load_requested: bool,
    /// Last readout string (text updates only when the displayed second
    /// changes — re-layout is the only non-constant cost here).
    last_readout: String,
}
// SAFETY: native pointers are game memory valid for the process
// lifetime; all mutation happens on the render thread.
unsafe impl Send for OverlayWidgets {}
static OVERLAY: Lazy<Mutex<OverlayWidgets>> = Lazy::new(|| {
    Mutex::new(OverlayWidgets {
        track: None,
        veil: None,
        line_a: None,
        line_b: None,
        cursor: None,
        readout: None,
        marker_texture: None,
        marker_load_requested: false,
        last_readout: String::new(),
    })
});
/// The current song's loaded asset (paired release at teardown).
static ASSET: Lazy<Mutex<Option<asset_loader::AssetHandle>>> = Lazy::new(|| Mutex::new(None));
/// Last shown/hidden state (show/hide only on transitions).
static VISIBLE: AtomicBool = AtomicBool::new(false);

/// Callback registrations (owned by activate/deactivate).
static SCENE_CB: Lazy<Mutex<Option<usize>>> = Lazy::new(|| Mutex::new(None));
static JUDGE_CB: Lazy<Mutex<Option<judge_hook::CallbackHandle>>> = Lazy::new(|| Mutex::new(None));

// ── Placement (TIMELINE PLACEMENT row → visibility + the HUD's edge) ─

/// Store a side's TIMELINE PLACEMENT value (row change callback + the
/// enable-time seed). Applies at the next song (per-song latch).
/// Out-of-range values normalize to OFF (fail-safe hidden).
pub fn set_placement(side: u8, value: i32) {
    if let Some(slot) = PLACEMENT.get(side as usize) {
        let value = if (PLACEMENT_OFF..=PLACEMENT_RIGHT).contains(&value) {
            value
        } else {
            PLACEMENT_OFF
        };
        slot.store(value, Ordering::Release);
    }
}

/// Latch the ENTERED side's placement for the song (P1 fallback —
/// solo/doubles only per R8, so exactly one side is entered).
fn latch_placement() {
    let side = (0..2usize)
        .find(|&s| crate::services::stage_records::side_entered(s).unwrap_or(false))
        .unwrap_or(0);
    LATCHED_PLACEMENT.store(PLACEMENT[side].load(Ordering::Acquire), Ordering::Release);
}

/// Whether the latched placement shows the HUD this song (round-4
/// amendment: the ONE visibility input besides the GAMEPLAY scene gate).
fn latched_visible() -> bool {
    LATCHED_PLACEMENT.load(Ordering::Acquire) != PLACEMENT_OFF
}

/// The strip rect's top-left for the latched placement.
fn strip_origin(width: u32, height: u32) -> (f32, f32) {
    let x = if LATCHED_PLACEMENT.load(Ordering::Acquire) == PLACEMENT_LEFT {
        EDGE_MARGIN_PX
    } else {
        1280.0 - width as f32 - EDGE_MARGIN_PX
    };
    (x, (720.0 - height as f32) / 2.0)
}

// ── Init / lifecycle ─────────────────────────────────────────────────

/// Resolve the strip's optional addresses. Called once from the mod's
/// `init` — every miss degrades (fail-open ladder), never blocks the mod.
pub fn init(signatures: &SignatureStore) {
    let selector = signatures.get_address("arrow_row_selector");
    let palette_vt = signatures.get_address("arrow_palette_vtable");
    let renderer_vt = signatures.get_address("arrow_renderer_vtable");

    if let Some(a) = selector {
        SELECTOR.store(a as *mut u8, Ordering::Release);
    }
    if let Some(a) = palette_vt {
        PALETTE_VTABLE.store(a as *mut u8, Ordering::Release);
    }
    if let Some(a) = renderer_vt {
        RENDERER_VTABLE.store(a as *mut u8, Ordering::Release);
    }
    log_info!(
        "StripHud: init -- selector={} palette_vt={} renderer_vt={}",
        selector.is_some(),
        palette_vt.is_some(),
        renderer_vt.is_some()
    );
}

/// Register the scene + judge callbacks. Called from the mod's enable().
pub fn activate() {
    if ENABLED.swap(true, Ordering::AcqRel) {
        return;
    }
    if scene_manager::is_available() {
        if let Ok(mut slot) = SCENE_CB.lock() {
            if slot.is_none() {
                *slot = Some(scene_manager::on_scene_change(Box::new(on_scene_change)));
            }
        }
    }
    if let Ok(mut slot) = JUDGE_CB.lock() {
        if slot.is_none() {
            *slot = judge_hook::register_pre(judge_hook::Priority::Late, on_judge_tick);
            if slot.is_none() {
                log_warn!("StripHud: judge hook unavailable -- strip inactive");
            }
        }
    }
    // Start the render-thread pump (self-requeueing; a bumped generation
    // orphans any previous loop).
    let pump_generation = PUMP_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    widget_renderer::run_on_render_thread(move || render_pump(pump_generation));
    log_info!("StripHud: activated");
}

/// Tear down the current song and unregister callbacks. Called from the
/// mod's disable().
pub fn deactivate() {
    if !ENABLED.swap(false, Ordering::AcqRel) {
        return;
    }
    // Orphan the pump loop (it checks the generation before requeueing).
    PUMP_GENERATION.fetch_add(1, Ordering::AcqRel);
    teardown_song("mod disabled");
    if let Ok(mut slot) = SCENE_CB.lock() {
        if let Some(id) = slot.take() {
            scene_manager::remove_callback(id);
        }
    }
    if let Ok(mut slot) = JUDGE_CB.lock() {
        if let Some(handle) = slot.take() {
            judge_hook::unregister(handle);
        }
    }
    log_info!("StripHud: deactivated");
}

// ── Scene wiring ─────────────────────────────────────────────────────

/// GAMEPLAY entry arms a fresh song (entry can re-fire without a results
/// screen — quick restart — so state resets on ENTRY, assist_tick's
/// discipline); exit tears down. Placement OFF skips the arm entirely
/// (round-4 amendment: no snapshot, no synthesis, no texture — the row
/// alone dictates the HUD).
fn on_scene_change(prev: i32, next: i32) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    if next == scene::GAMEPLAY {
        teardown_song("gameplay entry");
        latch_placement();
        if !latched_visible() {
            log_debug!("StripHud: placement OFF -- HUD idle this song");
            return;
        }
        if let Ok(mut song) = SONG.lock() {
            song.phase = Phase::Armed;
        }
        log_debug!("StripHud: armed for the new song");
    } else if prev == scene::GAMEPLAY {
        teardown_song("gameplay exit");
    }
}

/// Full per-song teardown. State (generation bump, phase reset, pending
/// discard) is synchronous — an immediately following arm must see clean
/// state — while the widget hide + asset release are scheduled onto the
/// render thread (the widget/texture threading rule; preview_overlay's
/// hide-before-release order is preserved inside the closure). The cache
/// file is deleted after the release is issued.
fn teardown_song(why: &str) {
    let file_path = {
        let mut song = match SONG.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        song.generation = song.generation.wrapping_add(1);
        song.phase = Phase::Idle;
        song.warned = false;
        song.file_path.take()
    };
    if let Ok(mut pending) = PENDING.lock() {
        // A synthesis that already posted: nobody owns its file now.
        if let Some((_, path, _, _, _)) = pending.take() {
            let _ = std::fs::remove_file(&path);
        }
    }
    if let Ok(mut geometry) = GEOMETRY.lock() {
        *geometry = None;
    }
    let why = why.to_string();
    widget_renderer::run_on_render_thread(move || {
        if VISIBLE.swap(false, Ordering::AcqRel) {
            if let Ok(widget) = WIDGET.lock() {
                if let Some(w) = widget.0.as_ref() {
                    w.hide();
                }
            }
        }
        hide_overlay();
        if let Ok(mut asset) = ASSET.lock() {
            if let Some(handle) = asset.take() {
                asset_loader::release(handle);
                log_debug!("StripHud: released strip texture ({})", why);
            }
        }
        if let Some(path) = file_path {
            let _ = std::fs::remove_file(&path);
        }
    });
}

// ── The per-frame judge tick (game thread) ───────────────────────────

/// The ONLY judge-tick job is the once-per-song snapshot (the actor and
/// its palette/renderer objects are judge-callback-scoped state). All
/// widget/texture work — load, resolve poll, bind, visibility — lives on
/// the render-thread pump ([`render_pump`], toast.rs's self-requeueing
/// model), per the widget/texture threading rule.
fn on_judge_tick(actor: *mut u8, _music_count: i32) {
    if actor.is_null() || !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let snapshot_generation = {
        let mut song = match SONG.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match song.phase {
            Phase::Armed => {
                song.phase = Phase::Synthesizing;
                Some(song.generation)
            }
            _ => None,
        }
    };
    if let Some(generation) = snapshot_generation {
        run_snapshot(actor, generation);
    }
}

// ── The render-thread pump ───────────────────────────────────────────

/// Self-requeueing render-thread callback (toast.rs's pattern): while
/// the HUD is enabled, each frame moves the texture pipeline forward —
/// background hand-off pickup, FileManager load, resolve poll, widget
/// bind — and applies the visibility gate. O(1) per frame. Started at
/// [`activate`]; a bumped [`PUMP_GENERATION`] orphans the loop.
fn render_pump(pump_generation: usize) {
    if pump_generation != PUMP_GENERATION.load(Ordering::Acquire)
        || !ENABLED.load(Ordering::Acquire)
    {
        return; // superseded — do not requeue
    }

    pump_once();
    overlay_update();

    widget_renderer::run_on_render_thread(move || render_pump(pump_generation));
}

/// One pump step. Render thread only.
fn pump_once() {
    // Move a completed background synthesis into the state machine.
    if let Ok(mut pending) = PENDING.lock() {
        if let Some((generation, path, stem, width, height)) = pending.take() {
            if let Ok(mut song) = SONG.lock() {
                if song.generation == generation && matches!(song.phase, Phase::Synthesizing) {
                    song.file_path = Some(path.clone());
                    song.phase = Phase::PngReady {
                        path,
                        stem,
                        width,
                        height,
                    };
                } else {
                    // Superseded while synthesizing: nobody owns the file.
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    // Phase step (the lock is held only to decide; the engine calls run
    // after it drops — asset_loader takes its own short lock).
    let step = {
        let mut song = match SONG.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match &song.phase {
            Phase::PngReady {
                path,
                stem,
                width,
                height,
            } => {
                let action = StepAction::Load {
                    generation: song.generation,
                    path: path.clone(),
                    stem: stem.clone(),
                    width: *width,
                    height: *height,
                };
                // The transition to Loading happens in the action (it
                // needs the load's name hash); park the phase meanwhile.
                song.phase = Phase::Synthesizing;
                Some(action)
            }
            Phase::Loading {
                name_hash,
                width,
                height,
            } => Some(StepAction::Poll {
                name_hash: *name_hash,
                width: *width,
                height: *height,
            }),
            _ => None,
        }
    };

    match step {
        Some(StepAction::Load {
            generation,
            path,
            stem,
            width,
            height,
        }) => issue_load(generation, path, stem, width, height),
        Some(StepAction::Poll {
            name_hash,
            width,
            height,
        }) => poll_resolve(name_hash, width, height),
        None => {}
    }

    // Visibility: resolved AND the latched placement shows the HUD
    // during gameplay (round-4 amendment: the TIMELINE PLACEMENT row is
    // the ONE visibility input — no session predicate; OFF songs never
    // arm, so `resolved` is unreachable there anyway).
    let resolved = SONG
        .lock()
        .map(|song| matches!(song.phase, Phase::Resolved))
        .unwrap_or(false);
    let want_visible =
        resolved && latched_visible() && scene_manager::current_scene() == scene::GAMEPLAY;
    if want_visible != VISIBLE.load(Ordering::Acquire) {
        if let Ok(widget) = WIDGET.lock() {
            if let Some(w) = widget.0.as_ref() {
                if want_visible {
                    w.show();
                } else {
                    w.hide();
                }
                VISIBLE.store(want_visible, Ordering::Release);
            }
        }
    }
}

enum StepAction {
    Load {
        generation: u32,
        path: String,
        stem: String,
        width: u32,
        height: u32,
    },
    Poll {
        name_hash: u32,
        width: u32,
        height: u32,
    },
}

// ── The snapshot (game thread, once per song) ────────────────────────

/// Everything the background synthesis consumes, snapshotted into owned
/// buffers on the game thread.
struct SynthesisInputs {
    generation: u32,
    notes: Vec<NoteView>,
    chart_end_ms: i32,
    /// Note columns (8 = doubles), detected at snapshot.
    columns: u32,
    /// Per-note tap palette rows (the selector's output; row 1 fallback).
    tap_rows: Vec<u8>,
    palette: Box<StripPalette>,
    /// Reverse scroll (the lane's travel direction — the timeline runs
    /// bottom-to-top to match; maintainer directive 2026-08-14).
    reverse: bool,
}

/// Read every live input and spawn the synthesis thread. Any hard miss
/// (notes/chart end) drops the strip for this song with one WARN; soft
/// misses (selector/renderer/manager) degrade colors per the ladder.
fn run_snapshot(actor: *mut u8, generation: u32) {
    let fault = std::env::var("DDR_STRIP_FAULT").unwrap_or_default();
    // Make fault injection unmissable: a stale env var from a fault-leg
    // test once masqueraded as a live RTTI failure (2026-08-15 demo).
    if !fault.is_empty() {
        log_warn!(
            "StripHud: DDR_STRIP_FAULT='{}' is SET -- failures below are injected, not real",
            fault
        );
    }

    let side = unsafe { memory::read_i32(actor.add(ACTOR_PLAY_SIDE)) };
    if !(0..=1).contains(&side) {
        warn_once(&format!("actor play side out of range ({side})"));
        return;
    }

    let (Some(notes), Some(chart_end_ms)) = (
        song_reset::decoded_notes(side),
        song_reset::chart_end_raw(side),
    ) else {
        warn_once("chart notes/end unavailable -- no strip this song");
        return;
    };
    if notes.is_empty() || chart_end_ms <= 0 {
        warn_once("degenerate chart -- no strip this song");
        return;
    }

    // Per-note tap rows: CALL the game's selector with the live renderer
    // (both RTTI/AOB gated). Fallback: row 1 flat. Each rung logs its
    // own distinct failure — the snapshot runs once per song, so these
    // are naturally one-shot, and a shared latch would hide the palette
    // rung's outcome behind this one (2026-08-15 diagnostic finding).
    let selector = SELECTOR.load(Ordering::Acquire);
    let renderer = validated_object(actor, ACTOR_ARROW_RENDERER, &RENDERER_VTABLE);
    // Reverse scroll off the same validated renderer (the exact guarded
    // read the fill performs — player_perspective's read_y_dir shape).
    // Renderer unavailable ⇒ forward.
    let reverse = renderer
        .map(|r| unsafe { read_reverse_flag(r) })
        .unwrap_or(false);
    let (tap_rows, taps_live): (Vec<u8>, bool) = if fault == "selector" {
        log_warn!("StripHud: fault-injected selector miss -- flat tap coloring");
        (vec![1u8; notes.len()], false)
    } else if selector.is_null() {
        log_warn!("StripHud: arrow_row_selector unresolved -- flat tap coloring");
        (vec![1u8; notes.len()], false)
    } else if let Some(renderer) = renderer {
        let selector: SelectorFn = unsafe { std::mem::transmute(selector) };
        (
            notes
                .iter()
                .map(|n| unsafe { (selector(renderer, n.display_time) & 31) as u8 })
                .collect(),
            true,
        )
    } else {
        let object = unsafe { *(actor.add(ACTOR_ARROW_RENDERER) as *const *const u8) };
        log_warn!(
            "StripHud: arrow renderer failed RTTI validation (actor+0x148={:p} expected_vt={:p}) -- flat tap coloring",
            object,
            RENDERER_VTABLE.load(Ordering::Acquire)
        );
        (vec![1u8; notes.len()], false)
    };

    // The bar palette: the fixed offline ramp (the shipped source —
    // maintainer directive round 3), or the live-walk path behind
    // USE_LIVE_PALETTE (kept for revisit; distinct one-shot logs per
    // failure rung).
    let (palette, palette_source) = if !USE_LIVE_PALETTE {
        (flat_ramp_palette(), "ramp")
    } else {
        let palette_result: Result<Box<StripPalette>, &str> = if fault == "palette" {
            Err("fault-injected palette miss")
        } else {
            match validated_object(actor, ACTOR_PALETTE_MGR, &PALETTE_VTABLE) {
                None => Err("palette manager failed RTTI validation at actor+0x130"),
                Some(mgr) => {
                    unsafe { walk_palette(mgr) }.ok_or("palette generator table failed validation")
                }
            }
        };
        match palette_result {
            Ok(palette) => (palette, "live"),
            Err(why) => {
                log_warn!("StripHud: {} -- flat quantization colors", why);
                (flat_ramp_palette(), "flat")
            }
        }
    };

    // Doubles chart ⇒ 8 columns (any second-side panel participates).
    let doubles = notes
        .iter()
        .any(|n| n.panel_flags[4..].iter().any(|&f| f != 0));
    let columns = if doubles { 8 } else { 4 };

    // Publish the overlay geometry — the cursor/markers/readout run on
    // it even if the strip texture never resolves (fail-open track).
    if let Ok(mut geometry) = GEOMETRY.lock() {
        *geometry = Some(SongGeometry {
            columns,
            chart_end_ms,
            reverse,
        });
    }

    log_info!(
        "StripHud: snapshot gen={} side={} notes={} chart_end={}ms reverse={} columns={} taps={} palette={}",
        generation,
        side,
        notes.len(),
        chart_end_ms,
        reverse,
        columns,
        if taps_live { "live" } else { "flat" },
        palette_source
    );

    let inputs = SynthesisInputs {
        generation,
        notes,
        chart_end_ms,
        columns,
        tap_rows,
        palette,
        reverse,
    };
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            synthesize(inputs);
        }));
        if result.is_err() {
            log_warn!("StripHud: synthesis thread panicked -- no strip this song");
        }
    });
}

/// Read the ArrowSprite-base reverse flag off the validated renderer:
/// `*(u8*)(this + 0x80 + *(i32*)(*(u64*)(this+0x80) + 4))` — the exact
/// read the fill (and player_perspective's `read_y_dir`) performs. Null
/// vb ⇒ forward scroll.
unsafe fn read_reverse_flag(renderer: *mut u8) -> bool {
    const OFF_VBPTR: usize = 0x80;
    let vb = *(renderer.add(OFF_VBPTR) as *const *const u8);
    if vb.is_null() {
        return false;
    }
    let disp = *(vb.add(4) as *const i32);
    *renderer.add(OFF_VBPTR + disp as usize) != 0
}

/// Read `*(actor + offset)` and require the object's vptr to equal the
/// RTTI-resolved vtable. `None` on any null/mismatch — offset drift on a
/// future build fails here, closed, instead of a wild call.
fn validated_object(actor: *mut u8, offset: usize, vtable: &AtomicPtr<u8>) -> Option<*mut u8> {
    let expected = vtable.load(Ordering::Acquire);
    if expected.is_null() {
        return None;
    }
    unsafe {
        let object = *(actor.add(offset) as *const *mut u8);
        if object.is_null() {
            return None;
        }
        let vptr = *(object as *const *mut u8);
        if vptr == expected {
            Some(object)
        } else {
            None
        }
    }
}

/// The game's palette-update loop pointed at a private buffer: for each
/// needed row, resolve the generator per the update's fold rules and
/// call its `evaluate(rowArg, column, phase)` per column. Game thread
/// only (generators may read game state). Returns `None` when the table
/// shape fails validation.
///
/// The generators ANIMATE on `phase` (0x400 = one beat): the classic
/// note borders blink transparent↔bright on beat quarters
/// (`(phase>>8) & 3`) and the body cells run a phase-driven pulse — a
/// single-instant bake lands wherever the pulse happened to be, usually
/// dimmer than peak (2026-08-15 re-demo finding: live colors read
/// "flat"). So each cell is evaluated across a full sweep of phases and
/// the peak-luminance sample wins — the game's own brightest instant,
/// phase-independent, no color math replicated. Cost: `PHASE_SWEEP` ×
/// 1280 leaf calls once per song (the game itself runs 8192 per frame).
unsafe fn walk_palette(mgr: *mut u8) -> Option<Box<StripPalette>> {
    /// Samples across two 0x400 blink cycles (covers slower pulses).
    const PHASE_SWEEP: i32 = 16;
    const PHASE_STEP: i32 = 0x800 / PHASE_SWEEP;

    let table = *(mgr.add(MGR_TABLE) as *const *const *mut u8);
    let table_end = *(mgr.add(MGR_TABLE_END) as *const *const *mut u8);
    if table.is_null() || table_end.is_null() {
        return None;
    }
    let count = (table_end as usize).checked_sub(table as usize)? / 8;
    // The factory writes slots 0..=7 (+ the Other family); anything
    // smaller means the layout drifted.
    if !(8..=64).contains(&count) {
        return None;
    }
    let phase = memory::read_i32(mgr.add(MGR_PHASE));

    let mut palette: Box<StripPalette> = Box::new([[[0, 0, 0, 0]; 256]; 32]);
    for &row in &NEEDED_ROWS {
        // The update loop's fold: rows 8..15 use the Freeze slot with
        // rowArg = row − 7; rows past the table end fold to the last
        // slot (not reachable for NEEDED_ROWS with count ≥ 8).
        let (slot, row_arg) = if (8..16).contains(&row) {
            (FREEZE_SLOT, (row - FREEZE_SLOT) as i32)
        } else if row >= count {
            (count - 1, (row - (count - 1)) as i32)
        } else {
            (row, row as i32)
        };
        let generator = *table.add(slot);
        if generator.is_null() {
            continue;
        }
        let vtable = *(generator as *const *const usize);
        if vtable.is_null() {
            return None;
        }
        let evaluate: EvaluateFn = std::mem::transmute(*vtable.add(1));
        for column in 0..256usize {
            let mut best = [0u8, 0, 0, 0];
            let mut best_luma = 0u32;
            for k in 0..PHASE_SWEEP {
                let argb = evaluate(
                    generator,
                    row_arg,
                    column as i32,
                    phase.wrapping_add(k * PHASE_STEP),
                );
                let cell = [
                    (argb >> 16) as u8,
                    (argb >> 8) as u8,
                    argb as u8,
                    (argb >> 24) as u8,
                ];
                // row_bar_color's alpha-weighted Rec.601-ish pick, so
                // the peak choice here and the bar-color choice there
                // agree on what "brightest" means.
                let luma = (2 * u32::from(cell[0]) + 5 * u32::from(cell[1]) + u32::from(cell[2]))
                    * u32::from(cell[3]);
                if luma > best_luma || (best_luma == 0 && cell[3] > best[3]) {
                    best_luma = luma;
                    best = cell;
                }
            }
            palette[row][column] = best;
        }
    }
    Some(palette)
}

/// The fixed offline ramp — the SHIPPED bar-color source (maintainer
/// directive 2026-08-15 round 3: the approved host-side renders used
/// exactly this recipe and read better in situ than the live palette
/// even at its beat-cycle peak). Per-row tint ramps over the index
/// channel; `row_bar_color` picks the full-intensity endpoint. Also the
/// fallback when `USE_LIVE_PALETTE` is re-enabled and the live
/// machinery is unavailable.
fn flat_ramp_palette() -> Box<StripPalette> {
    let mut palette: Box<StripPalette> = Box::new([[[0, 0, 0, 0]; 256]; 32]);
    let tints: [(usize, [u8; 3]); 5] = [
        (1, [255, 90, 130]),  // 4th
        (2, [255, 215, 80]),  // 16th
        (3, [110, 150, 255]), // 8th
        (4, [140, 255, 120]), // other
        (8, [130, 230, 140]), // freeze
    ];
    for (row, tint) in tints {
        for (idx, entry) in palette[row].iter_mut().enumerate() {
            let scale = |c: u8| ((u32::from(c) * idx as u32) / 255) as u8;
            *entry = [scale(tint[0]), scale(tint[1]), scale(tint[2]), 255];
        }
    }
    palette
}

fn warn_once(message: &str) {
    if let Ok(mut song) = SONG.lock() {
        if song.warned {
            return;
        }
        song.warned = true;
    }
    log_warn!("StripHud: {}", message);
}

// ── Background synthesis ─────────────────────────────────────────────

/// Pure synthesis + the cache-file write. No engine calls, no disk
/// reads besides the output (bar mode needs no noteskin sheet).
fn synthesize(inputs: SynthesisInputs) {
    let start = std::time::Instant::now();
    let fault = std::env::var("DDR_STRIP_FAULT").unwrap_or_default();

    // Measure guidelines: 4096-tick bars mapped through the chart's own
    // display→raw interpolation (`seek::raw_for_display` is pure).
    let mut guideline_ms: Vec<i32> = Vec::new();
    let max_display = inputs
        .notes
        .iter()
        .map(|n| n.display_time)
        .max()
        .unwrap_or(0);
    for bar in 0..MAX_MEASURES {
        let tick = (bar as i64 * 4096).min(i64::from(i32::MAX)) as i32;
        if tick > max_display {
            break;
        }
        if let Some(raw) = seek::raw_for_display(&inputs.notes, tick) {
            if raw >= 0 && raw <= inputs.chart_end_ms {
                guideline_ms.push(raw);
            }
        }
    }

    let Some(layout) = StripLayout::new(
        inputs.columns,
        COLUMN_PX,
        STRIP_HEIGHT_PX,
        inputs.chart_end_ms,
    )
    .map(|layout| layout.with_reverse(inputs.reverse)) else {
        log_warn!("StripHud: degenerate layout -- no strip this song");
        return;
    };

    // NoteView → StripNote (rows injected per note; freeze row 8 is the
    // fill's idle-freeze encoding).
    let strip_notes: Vec<StripNote> = inputs
        .notes
        .iter()
        .zip(inputs.tap_rows.iter())
        .map(|(n, &tap_row)| StripNote {
            kind: n.kind,
            raw_time: n.raw_time,
            panel_flags: n.panel_flags,
            durations: n.durations,
            tap_row,
            freeze_row: 8,
        })
        .collect();

    let scene = StripScene {
        notes: &strip_notes,
        guideline_ms: &guideline_ms,
        guideline_rgba: GUIDELINE_RGBA,
        shock_lightning: None,
        mine_lightning: None,
        background: BACKGROUND_RGBA,
    };
    let strip = strip_synth::render_strip_bars(&layout, &scene, &inputs.palette);
    let png = match strip_synth::encode_png(&strip) {
        Ok(bytes) => bytes,
        Err(e) => {
            log_warn!("StripHud: {} -- no strip this song", e.describe());
            return;
        }
    };

    if fault == "synthesis" {
        log_warn!("StripHud: DDR_STRIP_FAULT=synthesis -- dropping the strip");
        return;
    }

    let stem = format!("{}{}", STEM_PREFIX, inputs.generation);
    let path = format!("{}/{}.png", CACHE_DIR, stem);
    if std::fs::create_dir_all(CACHE_DIR).is_err() || std::fs::write(&path, &png).is_err() {
        log_warn!(
            "StripHud: cache write failed ({}) -- no strip this song",
            path
        );
        return;
    }

    log_info!(
        "StripHud: synthesized gen={} {}x{} notes={} bars={} png={}B in {}ms",
        inputs.generation,
        strip.width(),
        strip.height(),
        strip_notes.len(),
        guideline_ms.len(),
        png.len(),
        start.elapsed().as_millis()
    );

    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some((inputs.generation, path, stem, strip.width(), strip.height()));
    }
}

// ── The overlay: cursor / A-B markers / veil / readout (task-03) ─────

/// Hide every overlay widget (teardown / gate-off). Render thread.
fn hide_overlay() {
    let Ok(overlay) = OVERLAY.lock() else { return };
    for widget in [
        overlay.track.as_ref(),
        overlay.veil.as_ref(),
        overlay.line_a.as_ref(),
        overlay.line_b.as_ref(),
        overlay.cursor.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        widget.hide();
    }
    if let Some(readout) = overlay.readout.as_ref() {
        readout.hide();
    }
}

/// One overlay frame (render thread, called by the pump): position the
/// cursor from the live music count, the A/B lines and the section veil
/// from the bounds accessors, and refresh the readout when its second
/// changes. Constant work — ≤5 position writes + one occasional text
/// update. Fail-open ladder: no strip texture ⇒ the widgets ride a
/// plain translucent track; marker asset missing ⇒ readout only.
fn overlay_update() {
    let geometry = GEOMETRY.lock().ok().and_then(|g| *g);
    // Round-4 amendment: visibility = the latched TIMELINE PLACEMENT row
    // (the geometry only exists on armed songs, i.e. placement != OFF).
    let visible = geometry.is_some()
        && latched_visible()
        && scene_manager::current_scene() == scene::GAMEPLAY;
    let Some(geometry) = geometry.filter(|_| visible) else {
        hide_overlay();
        return;
    };
    let Some(layout) = StripLayout::new(
        geometry.columns,
        COLUMN_PX,
        STRIP_HEIGHT_PX,
        geometry.chart_end_ms,
    )
    .map(|layout| layout.with_reverse(geometry.reverse)) else {
        hide_overlay();
        return;
    };
    let width = layout.width_px();
    let (origin_x, origin_y) = strip_origin(width, layout.height_px);

    let Ok(mut overlay) = OVERLAY.lock() else {
        return;
    };

    // Marker asset: request once, then poll; widgets are created only
    // after the texture resolves (the readout is texture-free and is
    // created regardless — the "readout only" rung).
    if !overlay.marker_load_requested {
        overlay.marker_load_requested = true;
        if asset_loader::load(MARKER_TEX_PATH, MARKER_TEX_STEM).is_none() {
            log_warn!("StripHud: marker asset load failed -- readout-only overlay");
        }
        // Never released: process-lifetime chrome (the mine-texture model).
    }
    if overlay.marker_texture.is_none() {
        overlay.marker_texture = asset_loader::resolve(MARKER_TEX_STEM).map(|t| t.handle as i32);
    }
    ensure_overlay_widgets(&mut overlay);

    // The strip texture's own visibility (the pump's placement gate)
    // doubles as the fallback-track decision: the track shows only
    // while the real strip is absent.
    let strip_resolved = SONG
        .lock()
        .map(|song| matches!(song.phase, Phase::Resolved))
        .unwrap_or(false);
    if let Some(track) = overlay.track.as_ref() {
        if strip_resolved {
            track.hide();
        } else {
            track.set_position(origin_x, origin_y);
            track.set_size(width as f32, layout.height_px as f32);
            track.show();
        }
    }

    // Section veil (re-demo amendment 2026-08-15: ALWAYS shade the
    // active region — no markers means the whole song is active, so
    // the whole strip shades).
    let veil_span = strip_synth::section_veil(
        bounds::active_section_start(),
        bounds::section_end(),
        geometry.chart_end_ms,
    );
    if let Some(veil) = overlay.veil.as_ref() {
        if let Some((start_ms, end_ms)) = veil_span {
            let y0 = layout.y_for_ms(start_ms);
            let y1 = layout.y_for_ms(end_ms);
            let (top, span) = (y0.min(y1), (y1 - y0).abs().max(1));
            veil.set_position(origin_x, origin_y + top as f32);
            veil.set_size(width as f32, span as f32);
            veil.show();
        } else {
            veil.hide();
        }
    }

    // A/B marker lines (poll — gestures move them mid-song). BOTH lines
    // always render (re-demo amendment 2026-08-15): A falls back to the
    // song start and B to the timeline end when unset — the strip's
    // edges ARE the song bounds. Line tops clamp inside the strip so an
    // edge line stays fully visible instead of half-hanging past it.
    let place_line = |line: Option<&ImageWidget>, at_ms: Option<i32>, overhang: f32, h: f32| {
        let Some(line) = line else { return };
        if let Some(ms) = at_ms {
            let y = layout.y_for_ms(ms) as f32;
            let top = (y - h / 2.0).clamp(0.0, (layout.height_px as f32 - h).max(0.0));
            line.set_position(origin_x - overhang, origin_y + top);
            line.set_size(width as f32 + 2.0 * overhang, h);
            line.show();
        } else {
            line.hide();
        }
    };
    place_line(
        overlay.line_a.as_ref(),
        Some(bounds::active_section_start().unwrap_or(0)),
        MARKER_OVERHANG,
        MARKER_H,
    );
    place_line(
        overlay.line_b.as_ref(),
        Some(bounds::section_end().unwrap_or(geometry.chart_end_ms)),
        MARKER_OVERHANG,
        MARKER_H,
    );

    // Cursor from the live music count (negative pre-song counts clamp
    // to the start; the layout clamps past the end).
    let now_ms = song_reset::current_raw_music_count().unwrap_or(0).max(0);
    place_line(
        overlay.cursor.as_ref(),
        Some(now_ms),
        CURSOR_OVERHANG,
        CURSOR_H,
    );

    // Readout: "m:ss / m:ss", re-laid-out only when the text changes.
    if overlay.readout.is_some() {
        let text = format!(
            "{} / {}",
            strip_synth::format_mss(now_ms),
            strip_synth::format_mss(geometry.chart_end_ms)
        );
        let changed = text != overlay.last_readout;
        if changed {
            overlay.last_readout = text;
        }
        if let Some(readout) = overlay.readout.as_ref() {
            if changed {
                readout.set_text(&overlay.last_readout);
            }
            // Centered under the strip (per-line Center alignment about
            // x — the toast's model), clamped so the centered line stays
            // on-screen: LEFT placement puts the strip's center 36 px
            // from the screen edge and an unclamped center clips the
            // leading "0:" (2026-08-15 demo finding).
            let half_w = overlay.last_readout.len() as f32 * READOUT_GLYPH_PX * READOUT_SCALE / 2.0;
            let center_x =
                (origin_x + width as f32 / 2.0).clamp(half_w + 2.0, 1280.0 - half_w - 2.0);
            readout.set_position(
                center_x,
                origin_y + layout.height_px as f32 + READOUT_GAP_PX,
            );
            readout.show();
        }
    }
}

/// Lazily create the overlay widgets (render thread; once — nodes are
/// permanently consumed). Image widgets need the marker texture; the
/// readout does not. Creation order = z order (later draws on top), so
/// the STRIP widget is force-created FIRST: it and the overlay widgets
/// race their texture resolves, and a session engaged before the strip
/// texture landed once put the whole overlay UNDER the strip for the
/// process lifetime (2026-08-15 round-3 finding). Then track under
/// veil under lines under cursor.
fn ensure_overlay_widgets(overlay: &mut OverlayWidgets) {
    if overlay.marker_texture.is_some() && overlay.track.is_none() {
        ensure_strip_widget();
    }
    if let Some(texture) = overlay.marker_texture {
        let make = |color: u32, uv_center: bool| -> Option<ImageWidget> {
            let widget = widget_renderer::create_image_widget(&ImageWidgetConfig {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                texture_name: None,
                ..Default::default()
            })?;
            widget.set_texture_id(texture);
            widget.set_color(color);
            if uv_center {
                let (v0, v1) = MARKER_UV_CENTER;
                widget.set_uv(0.0, v0, 1.0, v1);
            }
            Some(widget)
        };
        if overlay.track.is_none() {
            overlay.track = make(COLOR_TRACK, true);
        }
        if overlay.veil.is_none() {
            overlay.veil = make(COLOR_VEIL, true);
        }
        if overlay.line_a.is_none() {
            overlay.line_a = make(COLOR_A, true);
        }
        if overlay.line_b.is_none() {
            overlay.line_b = make(COLOR_B, true);
        }
        if overlay.cursor.is_none() {
            overlay.cursor = make(COLOR_CURSOR, false); // full UV: baked outline
        }
    }
    if overlay.readout.is_none() {
        if let Some(readout) = widget_renderer::create_text_widget() {
            readout.set_scale(READOUT_SCALE, READOUT_SCALE);
            readout.set_color(1.0, 1.0, 1.0, 1.0);
            readout.set_outline(0.0, 0.0, 0.0, 1.0, 1);
            readout.set_alignment(TextAlignment::Center);
            overlay.readout = Some(readout);
        }
    }
}

// ── Texture load + widget (game thread) ──────────────────────────────

/// Issue the FileManager load for the finished PNG and move to Loading.
/// Generation-checked: a teardown racing this step (song exited between
/// the pump's decision and here) makes the phase write a no-op and the
/// just-issued load is released immediately — the pairing stays exact.
fn issue_load(generation: u32, path: String, stem: String, width: u32, height: u32) {
    if std::env::var("DDR_STRIP_FAULT").as_deref() == Ok("load") {
        warn_once("DDR_STRIP_FAULT=load -- skipping texture load");
        return;
    }
    let Some(handle) = asset_loader::load(&path, &stem) else {
        warn_once("texture load request failed -- no strip this song");
        return;
    };
    let name_hash = handle.name_hash;

    let still_current = SONG
        .lock()
        .map(|song| song.generation == generation)
        .unwrap_or(false);
    if !still_current {
        asset_loader::release(handle);
        return;
    }

    if let Ok(mut asset) = ASSET.lock() {
        if let Some(previous) = asset.replace(handle) {
            asset_loader::release(previous);
        }
    }
    if let Ok(mut song) = SONG.lock() {
        if song.generation == generation {
            song.phase = Phase::Loading {
                name_hash,
                width,
                height,
            };
        }
    }
    log_debug!(
        "StripHud: load issued for {} (hash {:#010x})",
        stem,
        name_hash
    );
}

/// Ensure the ONE strip widget exists (hidden, unbound — the resolve
/// path binds texture + rect per song). Render thread only. Called from
/// the resolve path AND immediately before the overlay's image widgets
/// are first created, so the strip always sits BELOW the overlay in the
/// widget z-order (z = creation order; see ensure_overlay_widgets).
fn ensure_strip_widget() {
    let Ok(mut widget) = WIDGET.lock() else {
        return;
    };
    if widget.0.is_none() {
        widget.0 = widget_renderer::create_image_widget(&ImageWidgetConfig {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            texture_name: None,
            ..Default::default()
        });
    }
}

/// Poll the ResourceManager for the registered texture; on readiness,
/// bind it to the (lazily created) widget and move to Resolved.
fn poll_resolve(name_hash: u32, width: u32, height: u32) {
    let Some(texture) = asset_loader::resolve_hash(name_hash) else {
        return; // still loading — poll again next tick
    };

    ensure_strip_widget();
    let widget_guard = match WIDGET.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if widget_guard.0.is_none() {
        warn_once("widget creation failed -- no strip this song");
        return;
    }
    let (origin_x, origin_y) = strip_origin(width, height);
    if let Some(widget) = widget_guard.0.as_ref() {
        // Reposition/resize for this song's dims (4 vs 8 columns) and
        // the latched placement, then bind. Visibility is the per-tick
        // placement poll's job.
        widget.set_position(origin_x, origin_y);
        widget.set_size(width as f32, height as f32);
        widget.set_texture_id(texture.handle as i32);
    }
    drop(widget_guard);

    if let Ok(mut song) = SONG.lock() {
        song.phase = Phase::Resolved;
    }
    log_info!(
        "StripHud: strip texture resolved and bound ({}x{})",
        width,
        height
    );
}
