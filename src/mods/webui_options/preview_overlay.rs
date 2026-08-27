//! WebUI Options preview overlay — draws the focused customizer value's real
//! asset art as native sprites over the per-category chrome template.
//!
//! ## Model
//!
//! The engine renders a per-category chrome template into the option preview
//! box (via the `custom_options` slot-0 override returning the base
//! `seop_image_<id>`). This module overlays the *focused* value's real asset
//! art ON TOP as `agcs::Sprite`s, loaded on demand through
//! `services::asset_loader` (FileManager → ResourceManager). This eliminates
//! the ~500–600 per-asset preview textures the old compositing path injected
//! via LayeredFS (the song-select loading-screen cost) while keeping full live
//! previews.
//!
//! Every discovered category whose `overlay_layers()` is non-empty gets a live
//! overlay (CHARACTER ×2, APPEAL BOARD, LANE ×2, LANE COVER ×2). APPEAL BOARD
//! has TWO layers (the `_result` art in the green marker + the base art in the
//! red marker), so each side owns a small sprite POOL ([`MAX_OVERLAY_LAYERS`]).
//! Categories with no overlay layers (BACKGROUND ×2) and other mods' rows
//! (e.g. movie_size_customization's VIDEO SIZE) show chrome/whatever the
//! slot-0 override names — focusing them just hides the overlay sprites.
//!
//! ## Prefetch window (the resident set)
//!
//! To avoid a chrome-only flash on every scroll step (an arc takes ~43 pump
//! polls to load+register), each side keeps a **resident set** of loaded
//! assets: the **focused overlay category's** window `[cur-N, cur+N]` around
//! its current selection (`N` = [`OverlayState::window_n`], clamped to the
//! discovered asset range). On each focus/value change the set is re-diffed —
//! newcomers are loaded, leavers released. Scrolling within the focused row
//! (the hot path) moves the window by ±1, so the newly-focused value is
//! already resident and its art shows instantly; the load happens at the
//! window *edge*, N steps ahead. Switching categories swaps the whole window
//! (the focused item binds after one load latency, edges fill behind it).
//!
//! Deliberately NOT the union of every category's window, and menu-open
//! prefetches nothing: the engine's FileManager queue is serialized and
//! shared with song-select's own streaming (jackets/music previews), and the
//! original prefetch-all-on-open design burst ~50 loads into it — most of the
//! burst starved behind the queue and timed out (cabinet, 2026-07-08: 32 of
//! 48 loads never resolved). One category's window — at most `2N+1` entries ×
//! ≤2 layers — is a burst the queue absorbs comfortably.
//!
//! State per resident entry is one [`LayerLoad`] per overlay layer:
//! `Loading` (pump is polling), `Resolved` (bindable texture handle in hand),
//! or `Failed` (load error / timeout — handle released, stays chrome-only
//! until the entry leaves the window / the modal reopens). Binds only ever
//! read from the resident map and releases only happen on window-leave /
//! modal-close / shutdown, so stale-bind races (A→B→A refocus during a load)
//! are structurally impossible — no generation tokens needed.
//!
//! `AssetHandle` release discipline: each layer's handle lives in an
//! `Option<AssetHandle>` and `asset_loader::release` consumes it, so every
//! load is released exactly once (window-leave, teardown, timeout, or
//! disable-mid-load) and double-release is a compile error. Releasing a
//! still-loading handle is safe (cabinet-proven, 2026-07-07 churn test).
//!
//! ## The pump
//!
//! One self-rescheduling render-thread closure ([`pump_tick`]) polls every
//! `Loading` entry per tick via `asset_loader::resolve_hash` (cheap: one
//! engine hash-tree lookup each). When the focused entry resolves it binds +
//! shows immediately. The pump runs only while at least one entry is loading
//! (`pump_running` prevents double-scheduling; all callbacks and the pump run
//! serially on the render thread). NOTE: pump ticks are render-HOOK
//! invocations, not rendered frames — the hook fires several times per frame
//! (multiple passes), ~300–600 ticks/s measured on cabinet, so poll budgets
//! are calibrated in ticks, not frames.
//!
//! ## Lifetime (visibility is the modal, not the scene)
//!
//! Overlay sprites are shown only while BOTH hold:
//!  1. the options **modal** is open for that side (tracked via
//!     `custom_options::on_menu_open`/`on_menu_close`), and
//!  2. the focused row is an overlay-bearing WebUI category whose art has
//!     resolved.
//!
//! The modal is a child of scene 25 (`SONG_SELECT`), so scene-change events
//! are too coarse (they'd leave overlays up for all of song-select). Menu-open
//! readies the side's hidden sprite pool (loading starts on the first
//! overlay-row focus); menu-close hides the sprites and releases the side's
//! entire resident set (memory returns to baseline between modals). The sprite
//! objects themselves live for the **process lifetime** (at most
//! [`MAX_OVERLAY_LAYERS`] per side): `ImageWidget::destroy` is hide-and-leak
//! and `create_image_widget` permanently consumes a node from the game's
//! finite render-list pool, so destroy-per-close + create-per-open would drain
//! that pool on a long-uptime cabinet. Hidden sprites cost nothing per frame.
//!
//! ## Focus signal
//!
//! `custom_options::on_preview_request(side, option_id)` fires on the render
//! thread whenever the options menu asks the focused **mod** row for its
//! preview name — i.e. on focus change and on a value change within the
//! focused row (see `docs/option_preview_image_box.md`), re-firing every focus
//! tick. The handler is O(1) when nothing changed (`focused` equality check);
//! on a real transition it re-syncs the window set and refreshes the display.
//!
//! **Known limitation (carried from Step 5):** the focus signal only exists on
//! *mod* rows (it fires from our cloned slot-0 vtable). Moving focus from an
//! overlay row directly to a *native* row or the tab strip does not fire an
//! event, so the overlay can linger over the native row's preview until the
//! next mod-row focus or menu close. Pinning overlay visibility to the native
//! `image_usr` clip's own visibility is the eventual fix; deferred.
//!
//! ## Placement (screenshot-anchored, Step 6)
//!
//! The whole `seop_image_<id>` chrome template renders **1:1 into screen
//! pixels** at a per-side constant origin ([`CHROME_ORIGIN`], measured once
//! from captures — the P2 modal is on the RIGHT of the screen, not a mirror of
//! P1). Each overlay layer's art goes where the *compositor* put it: at init
//! we read each layer's `_TEMPLATE.png` marker rect (in template px) via
//! `preview_gen::marker_rect_for`, and at show time offset it by the side's
//! origin. The sprite is sized to that marker rect (not the full box), so the
//! art fills exactly the region the composited preview used — same aspect
//! handling as the old path, no runtime geometry read needed.
//!
//! ## Threading (CLAUDE.md rule 6)
//!
//! Sprite creation/mutation and asset load/resolve/release must happen on the
//! game's render thread. The menu-open/close and preview-request callbacks all
//! fire there already (from the option-form detours / slot-0 getter), and the
//! pump runs there via `run_on_render_thread` — so everything is serial. The
//! `OverlayState` mutex is never held across a `run_on_render_thread`
//! **schedule**: callbacks and the pump compute under the lock, drop it, then
//! schedule. `asset_loader`'s own mutex is a leaf (it never takes ours), so
//! calling load/resolve/release while holding `STATE` is ordering-safe.
//! Poisoned locks are recovered with `into_inner` (the state is plain data
//! whose invariants self-heal through the resident-map identity checks) with a
//! one-shot WARN.

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::core::arc::ArcArchive;
use crate::services::asset_loader::{self, AssetHandle};
use crate::services::avs_layeredfs::cache_hasher::CacheHasher;
use crate::services::avs_layeredfs::mod_paths;
use crate::services::custom_options;
use crate::services::widget_renderer;
use crate::widgets::image_widget::{ImageWidget, ImageWidgetConfig};
use crate::{log_info, log_warn};

use super::discovery::{CategoryDef, DiscoveredCategory};
use super::preview_gen::{self, MarkerRect};

/// Default half-width N of the focused category's prefetch window
/// (`[cur-N, cur+N]`). Resident set per side ≤ (2N+1) entries (7 at N=3),
/// ≤2 layers each — one category's window only. Used when
/// `custom_options.preview_window` is absent from `mod-config.json`; a
/// configured value overrides this (clamped 0..=10 in [`init`] — see
/// `webui_options::mod.rs`'s `preview_window_config`).
pub const DEFAULT_WINDOW_N: i32 = 3;

/// Most overlay layers any category has (APPEAL BOARD's base + `_result`).
/// Sizes each side's process-lifetime sprite pool.
const MAX_OVERLAY_LAYERS: usize = 2;

/// Where the pre-brightened lane arcs are cached (one `.arc` + `.hash` sidecar
/// per lane asset). See [`build_lane_cache`].
const LANE_CACHE_DIR: &str = "./data_mods/_cache/preview_overlay";

/// Full template dimensions (px). The game renders the whole
/// `seop_image_<id>` chrome template at this size, 1:1 into screen pixels
/// (see [`CHROME_ORIGIN`]).
const TEMPLATE_W: f32 = 368.0;
const TEMPLATE_H: f32 = 172.0;

/// Screen-space top-left (px) where the game renders each side's option
/// preview-box chrome. The 368×172 template maps **1:1** to screen pixels, so
/// any template-pixel rect `(mx, my, mw, mh)` renders on screen at
/// `(origin.x + mx, origin.y + my, mw, mh)`.
///
/// Derivation (both sides measured from 1280×720 = native-render captures,
/// each with the old composited CHARACTER art still in the box as ground
/// truth). The template green marker is at `(209, 11)`:
///   - P1 (`capture_20260707_233249.jpg`): art at screen `(394, 474)` →
///     origin `(394-209, 474-11) = (185, 463)`; box x186..553 (w≈368).
///   - P2 (`capture_20260708_001422.jpg`): art at screen `(951, 473)` →
///     origin `(951-209, 473-11) = (742, 463)`; box x742..1109 (w=368).
/// The box width on screen (368) confirms unit scale. The box sits at the same
/// vertical position on both sides — only X differs (the modal is on the
/// player's side of the screen). All options share the same box per side, so
/// one origin per side serves every category.
const CHROME_ORIGIN_Y: f32 = 463.0;
const CHROME_ORIGIN: [(f32, f32); 2] = [
    (185.0, CHROME_ORIGIN_Y), // P1 — measured
    (742.0, CHROME_ORIGIN_Y), // P2 — measured (same Y as P1)
];

/// A screen-space rectangle (px) for positioning + sizing an overlay sprite.
#[derive(Clone, Copy, Debug)]
struct ScreenRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl ScreenRect {
    /// Map a template-pixel marker rect into screen space for `side` (1:1
    /// scale, offset by the side's chrome origin).
    fn from_marker(side: usize, m: MarkerRect) -> Self {
        let (ox, oy) = CHROME_ORIGIN[side];
        Self {
            x: ox + m.x as f32,
            y: oy + m.y as f32,
            w: m.w as f32,
            h: m.h as f32,
        }
    }

    /// The full chrome box for `side` — the fallback placement when a
    /// single-layer option's template marker can't be resolved (art shows,
    /// stretched to the whole box, rather than hiding).
    fn full_box(side: usize) -> Self {
        let (ox, oy) = CHROME_ORIGIN[side];
        Self {
            x: ox,
            y: oy,
            w: TEMPLATE_W,
            h: TEMPLATE_H,
        }
    }
}

/// Pump polls (render-hook ticks, NOT rendered frames — the hook fires
/// several times per frame; ~300–600 ticks/s measured on cabinet 2026-07-08)
/// before a `Loading` entry gives up. One arc resolves in ~43 polls in
/// isolation; under song-select's own FileManager streaming a small window
/// burst (≤14 loads) clears in well under a second, so 3600 (~6–12 s wall)
/// only ever reclaims genuinely-broken loads (missing/corrupt arc). Timeout ⇒
/// the layer latches `Failed` (chrome only until the entry leaves the window
/// / the modal reopens); never blocks the thread.
const LOAD_TIMEOUT_POLLS: u32 = 3600;

/// One overlay layer's load lifecycle within a resident entry.
struct LayerLoad {
    /// The release handle. `Some` while Loading/Resolved; taken (released)
    /// exactly once — on `Failed` transition, window-leave, teardown, or
    /// disable. `release` consumes the handle, so double-release can't compile.
    handle: Option<AssetHandle>,
    /// Bare texture stem (registration key + log identity).
    tex_name: String,
    phase: Phase,
}

enum Phase {
    /// Pump is polling `resolve_hash` for this layer. `polls` counts ticks
    /// toward [`LOAD_TIMEOUT_POLLS`].
    Loading { polls: u32 },
    /// Registered — `tex` is the bindable texture handle (`TextureData+0x04`).
    Resolved { tex: u32 },
    /// Load error or resolve timeout. Handle already released; chrome only.
    Failed,
}

/// One resident asset: the per-layer loads for a `(category, asset-index)`
/// window member.
struct ResidentEntry {
    layers: Vec<LayerLoad>,
}

/// Release every layer handle of a removed entry (window-leave / teardown /
/// menu-open retry). MUST run on the render thread. The caller has already
/// removed the entry from the resident map (and hidden/re-bound any sprite
/// that showed it), so nothing visible references the textures being freed.
fn release_entry(entry: ResidentEntry) {
    for mut layer in entry.layers {
        if let Some(handle) = layer.handle.take() {
            asset_loader::release(handle);
        }
    }
}

/// One player side's overlay sprite pool + resident set.
struct SideOverlays {
    /// Overlay sprites, index = overlay layer. Created hidden on first
    /// menu-open and kept for the **process lifetime** (see the module doc's
    /// render-list-pool rationale) — teardown hides them, never destroys.
    sprites: Vec<ImageWidget>,
    /// The prefetch-window resident set: `(category index, asset index)` →
    /// per-layer load state. Populated on menu-open, re-diffed on focus/value
    /// change, fully released on menu-close.
    resident: HashMap<(usize, usize), ResidentEntry>,
    /// What the display should currently show: the focused overlay category +
    /// its current asset index, or `None` (non-overlay row focused → sprites
    /// hidden, residents kept warm).
    focused: Option<(usize, usize)>,
    /// True while this side's options modal is open. Overlays only show while
    /// this holds (belt-and-suspenders with the sprites' own visibility).
    modal_open: bool,
    /// Stems already warned about this modal session (load failure / resolve
    /// timeout) — one WARN per stem per session instead of one per window
    /// re-entry. Cleared on open/close.
    warned_stems: HashSet<String>,
}

impl SideOverlays {
    fn new() -> Self {
        Self {
            sprites: Vec::new(),
            resident: HashMap::new(),
            focused: None,
            modal_open: false,
            warned_stems: HashSet::new(),
        }
    }
}

struct OverlayState {
    /// Categories discovered by the mod. Resident-map keys index into this.
    categories: Vec<DiscoveredCategory>,
    /// Per player side overlay pools (index 0 = P1, 1 = P2).
    side_overlays: [SideOverlays; 2],
    /// Per-option per-overlay-layer marker rect, in TEMPLATE pixels, read once
    /// at `init` from each option's `_TEMPLATE.png`. `None` for a layer whose
    /// marker couldn't be resolved (→ full-box fallback for layer 0; hidden
    /// for later layers, since two full-box sprites would overlap). On-screen
    /// placement is the rect offset by the side's `CHROME_ORIGIN` (1:1).
    marker_rects: HashMap<String, Vec<Option<MarkerRect>>>,
    /// Gamma-corrected (lane) stems → the pre-brightened cached arc to load
    /// instead of the stock (dark) one. Built once at `init` (the image work
    /// happens there, on the init thread — never on the render thread); a
    /// stem absent here loads its stock arc. See [`build_lane_cache`].
    lane_arc_paths: HashMap<String, String>,
    /// Prefetch window half-width N (config knob in Step 10).
    window_n: i32,
    /// Whether a [`pump_tick`] closure is scheduled/running. Prevents double-
    /// scheduling; only mutated on the render thread (callbacks + pump are
    /// serial there).
    pump_running: bool,
}

// Raw game pointers inside the ImageWidgets are valid for the process lifetime
// and only touched from the render thread (codebase norm).
unsafe impl Send for OverlayState {}

impl OverlayState {
    fn new() -> Self {
        Self {
            categories: Vec::new(),
            side_overlays: [SideOverlays::new(), SideOverlays::new()],
            marker_rects: HashMap::new(),
            lane_arc_paths: HashMap::new(),
            window_n: DEFAULT_WINDOW_N,
            pump_running: false,
        }
    }
}

static STATE: Lazy<Mutex<OverlayState>> = Lazy::new(|| Mutex::new(OverlayState::new()));

/// Whether the overlay system is active. The modal/preview callbacks are
/// registered once for the process lifetime (there's no unsubscribe), so they
/// gate on this flag — set true by `init`, false by `shutdown` — to no-op when
/// the mod is disabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the modal/preview callbacks have been registered (once ever).
static CALLBACKS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// One-shot latches for degradation logging (avoid per-focus-tick spam).
/// The two compute-failure classes get SEPARATE latches so the first
/// occurrence of one can't permanently silence the other's diagnostics:
/// `LOOKUP_FAIL_LOGGED` covers a focused value that can't be resolved
/// (registry lookup failed / index out of range); `CATEGORY_CONFIG_FAIL_LOGGED`
/// covers a category defined with overlay layers but no scan_dir/file_prefix
/// (a compile-time table bug).
static POISON_LOGGED: AtomicBool = AtomicBool::new(false);
static LOOKUP_FAIL_LOGGED: AtomicBool = AtomicBool::new(false);
static CATEGORY_CONFIG_FAIL_LOGGED: AtomicBool = AtomicBool::new(false);

/// Acquire the overlay state, recovering from a poisoned mutex (one-shot
/// WARN). `OverlayState` is plain data with no cross-field invariants a
/// mid-panic writer can break irrecoverably — binds/releases key off the
/// resident map, which self-heals on the next window sync — so recovery beats
/// the alternative (silently abandoning all cleanup and stranding a visible
/// overlay + resident handles for the process lifetime). Never panics.
fn lock_state() -> MutexGuard<'static, OverlayState> {
    STATE.lock().unwrap_or_else(|poisoned| {
        if !POISON_LOGGED.swap(true, Ordering::AcqRel) {
            log_warn!("preview_overlay: state mutex was poisoned — recovered (one-shot notice)");
        }
        poisoned.into_inner()
    })
}

/// Initialize the overlay system with the mod's discovered categories and the
/// prefetch-window half-width. Subscribes to the options-modal open/close
/// lifecycle and the focused-row preview-request signal. Safe to call from
/// `WebUiOptionsMod::enable()` after discovery; guarded by the caller on
/// `asset_loader::is_available()` (graceful degradation, R-8).
///
/// Idempotent: replaces the categories/window each call and registers the
/// callbacks exactly once (subsequent enables just re-arm `ENABLED`). Any
/// residents left over from a previous enable/disable cycle are released on
/// the render thread before the new session's state takes effect.
pub fn init(categories: Vec<DiscoveredCategory>, window_n: i32) {
    // Build/refresh the pre-brightened lane arc cache BEFORE taking the state
    // lock: this is the slow part (decode + gamma + re-encode per new/changed
    // lane asset) and runs on the caller's (init) thread, keeping the render
    // thread clean. Warm boots are cheap — per-asset hash sidecars skip
    // unchanged work.
    let lane_arc_paths = build_lane_cache(&categories, super::lane_gamma_override());

    {
        let mut state = lock_state();

        // Read each overlay layer's marker rect from the option's template
        // once, up front (reading the PNG per focus tick would be wasteful).
        // Placement = the template-pixel rect offset by the side's
        // CHROME_ORIGIN, so the on-demand art lands exactly where the
        // compositor drew it. A missing template/marker caches `None` →
        // full-box fallback (layer 0) / hidden (later layers) at show time.
        let mut marker_rects = HashMap::new();
        for cat in &categories {
            let layers = cat.def.overlay_layers();
            if layers.is_empty() {
                continue;
            }
            let rects: Vec<Option<MarkerRect>> = layers
                .iter()
                .enumerate()
                .map(|(i, layer)| {
                    let rect = preview_gen::marker_rect_for(cat.def.option_id, layer.color);
                    match rect {
                        Some(r) => log_info!(
                            "preview_overlay: {} layer {} marker rect (template px) {}x{} at ({},{})",
                            cat.def.option_id,
                            i,
                            r.w,
                            r.h,
                            r.x,
                            r.y
                        ),
                        None => log_warn!(
                            "preview_overlay: {} layer {} has no resolvable template marker — {}",
                            cat.def.option_id,
                            i,
                            if i == 0 { "will use full-box placement" } else { "layer will not render" }
                        ),
                    }
                    rect
                })
                .collect();
            marker_rects.insert(cat.def.option_id.to_string(), rects);
        }

        state.categories = categories;
        state.marker_rects = marker_rects;
        state.lane_arc_paths = lane_arc_paths;
        state.window_n = window_n.clamp(0, 10);
        log_info!(
            "preview_overlay: init ({} categories, {} with overlay markers, window_n={})",
            state.categories.len(),
            state.marker_rects.len(),
            state.window_n
        );
    }

    // Release anything a previous session left resident (covers the
    // re-enable-before-shutdown-teardown-ran race, where shutdown's queued
    // closure sees ENABLED re-armed and skips). Only sides WITHOUT an open
    // modal are swept: menu-open fires inline on the render thread (not via
    // this queue), so a new session's open may land before this queued
    // closure runs — an open modal owns its state (its residents are live and
    // the next window sync heals any drift), and menu-close releases
    // unconditionally in any case.
    widget_renderer::run_on_render_thread(|| {
        let mut state = lock_state();
        for side in 0..2usize {
            if !state.side_overlays[side].modal_open {
                teardown_side(&mut state, side);
            }
        }
    });

    // Register the modal-lifecycle + preview-request callbacks once. They fire
    // on the render thread (from the option-form detours / slot-0 getter) and
    // gate on ENABLED.
    if !CALLBACKS_REGISTERED.swap(true, Ordering::AcqRel) {
        custom_options::on_menu_open(on_menu_open);
        custom_options::on_menu_close(on_menu_close);
        custom_options::on_preview_request(on_preview_request);
    }
    ENABLED.store(true, Ordering::Release);
}

/// Tear down: disable the callbacks and hide/release any live overlays. Called
/// from `WebUiOptionsMod::disable()`. The callbacks remain registered but
/// no-op while `ENABLED` is false.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Release);
    // Hide/release overlays on the render thread (sprite mutation + asset
    // release must be there). The closure re-checks ENABLED: if the mod was
    // re-enabled before this queued closure ran, init()'s own queued teardown
    // owns the cleanup instead.
    widget_renderer::run_on_render_thread(|| {
        if ENABLED.load(Ordering::Acquire) {
            return;
        }
        let mut state = lock_state();
        for side in 0..2usize {
            teardown_side(&mut state, side);
        }
    });
}

/// Options modal opened for `side`. Fires on the render thread from the
/// row-builder detour, after all native rows and their `image_usr` preview
/// clips exist. Ready this side's hidden sprite pool and reset the session —
/// loading is deferred to the first overlay-row focus (`on_preview_request`
/// syncs the focused category's window); menu-open itself enqueues nothing
/// into the engine's file queue.
fn on_menu_open(side: u8) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let side = side as usize;
    if !widget_renderer::is_available() {
        log_warn!(
            "preview_overlay: renderer not ready on menu-open (side {}) — no overlays",
            side
        );
        return;
    }
    {
        let mut state = lock_state();
        {
            let overlays = &mut state.side_overlays[side];
            overlays.modal_open = true;
            overlays.focused = None;
            // Hide any sprite left visible from a previous session BEFORE the
            // resident sweep below. In the degraded no-dtor-hook mode
            // (menu-close never fired) a sprite can still be showing a
            // texture owned by an entry the sweep is about to release —
            // `release_entry`'s precondition is that nothing visible
            // references the textures being freed. Normal mode: already
            // hidden by menu-close; this is a cheap no-op.
            for sprite in &overlays.sprites {
                sprite.hide();
            }
            // Fresh modal session: clear the warn dedup so previously-failed
            // arcs get fresh diagnostics when retried.
            overlays.warned_stems.clear();
            ensure_sprites(overlays, side);
        }
        // With `focused == None` the desired set is empty, so this releases
        // every resident left over from a previous session (Failed entries
        // included — a stale-session Failed arc is retried when its window
        // re-forms on focus). Normal mode: the map is already empty; no-op.
        // No loads start here, so no pump arming.
        sync_windows(&mut state, side);
    }
}

/// Options modal closed for `side`. Fires on the render thread from the
/// OptionForm destructor detour, before the form is freed. Hide + release this
/// side's entire resident set so nothing draws (and nothing stays resident)
/// once the modal is gone. Deliberately NOT gated on `ENABLED` — a
/// disable-while-open still gets its close-time cleanup.
fn on_menu_close(side: u8) {
    if side > 1 {
        return;
    }
    let mut state = lock_state();
    let had_assets = !state.side_overlays[side as usize].resident.is_empty();
    teardown_side(&mut state, side as usize);
    if had_assets {
        log_info!(
            "preview_overlay: overlays torn down for side {} (menu close)",
            side
        );
    }
}

/// The focused mod row asked for its preview name (focus change / value change
/// on the focused row; re-fires every focus tick). Fires on the render thread
/// with `custom_options`' ROWS/STATE locks released. Update the side's focused
/// target, re-sync the prefetch window, and refresh the display.
fn on_preview_request(side: u8, option_id: &str) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let side = side as usize;

    // Everything under the lock computes at most a pump schedule, which is
    // executed AFTER the lock is dropped (never hold STATE across a
    // run_on_render_thread schedule — CLAUDE.md rule 6).
    let start_pump = {
        let mut state = lock_state();
        if !state.side_overlays[side].modal_open {
            // Preview-request without a menu-open we saw — no sprites to drive.
            return;
        }

        // Map the focused row to an overlay category + its current asset
        // index. Non-overlay rows (backgrounds, other mods' rows such as
        // VIDEO SIZE) hide the overlay but keep the resident set warm — refocusing
        // an overlay row is then instant. (Native rows never fire this event
        // — see the module doc's known limitation.)
        let target = match compute_target(&state, side, option_id) {
            Target::Overlay(ci, idx) => (ci, idx),
            Target::NotOverlay => {
                if state.side_overlays[side].focused.take().is_some() {
                    refresh_display(&state, side);
                }
                return;
            }
            Target::LookupFailed => {
                // Overlay category but no resolvable current value (value
                // lookup failed / out-of-range index). Hide — chrome only.
                // One-shot log so a silently-blank cabinet test is
                // diagnosable without 60 Hz spam.
                if !LOOKUP_FAIL_LOGGED.swap(true, Ordering::AcqRel) {
                    log_warn!(
                        "preview_overlay: could not resolve focused value for {} (side {}) — chrome only (one-shot notice)",
                        option_id,
                        side
                    );
                }
                if state.side_overlays[side].focused.take().is_some() {
                    refresh_display(&state, side);
                }
                return;
            }
        };

        // Fast path: the getter re-fires every focus tick — nothing to do
        // when the focused (category, value) hasn't changed.
        if state.side_overlays[side].focused == Some(target) {
            return;
        }

        state.side_overlays[side].focused = Some(target);
        ensure_sprites(&mut state.side_overlays[side], side);
        let started = sync_windows(&mut state, side);
        refresh_display(&state, side);

        // One line per focus/value transition (the per-tick refires early-out
        // above): what's focused and whether its art was already resident —
        // the "prefetch hit rate" signal for cabinet validation.
        if let Some(entry) = state.side_overlays[side].resident.get(&target) {
            let resolved = entry
                .layers
                .iter()
                .filter(|l| matches!(l.phase, Phase::Resolved { .. }))
                .count();
            log_info!(
                "preview_overlay: side {} focus {} #{} — {}/{} layer(s) resident",
                side,
                option_id,
                target.1,
                resolved,
                entry.layers.len()
            );
        }
        arm_pump(&mut state, started)
    };

    if start_pump {
        widget_renderer::run_on_render_thread(pump_tick);
    }
}

/// What a preview-request maps to.
enum Target {
    /// An overlay category: `(category index, current asset index)`.
    Overlay(usize, usize),
    /// A registered row without overlay layers (or not a WebUI row at all).
    NotOverlay,
    /// An overlay category whose current value couldn't be resolved.
    LookupFailed,
}

/// Resolve `option_id` to its overlay target for `side`. Never "defaults to
/// asset 0" on a failed lookup — showing wrong art is worse than degraded art.
fn compute_target(state: &OverlayState, side: usize, option_id: &str) -> Target {
    let Some((ci, cat)) = state
        .categories
        .iter()
        .enumerate()
        .find(|(_, c)| c.def.option_id == option_id)
    else {
        return Target::NotOverlay;
    };
    if cat.def.overlay_layers().is_empty() {
        return Target::NotOverlay;
    }
    let Some(cur) = custom_options::get_value(side as u8, option_id) else {
        return Target::LookupFailed;
    };
    if cur < 0 || (cur as usize) >= cat.asset_ids.len() {
        return Target::LookupFailed;
    }
    Target::Overlay(ci, cur as usize)
}

/// The desired resident set for `side`: the **focused** overlay category's
/// `[cur-N, cur+N]` window (clamped to its discovered asset range), or empty
/// when nothing overlay-bearing is focused. `cur` comes from the focused
/// target itself (set from a fresh registry read in `compute_target`), so the
/// window is always centered on what the display shows.
///
/// Deliberately NOT the union of every category's window: the engine's
/// FileManager queue is serialized and shared with song-select's own
/// streaming, and the original prefetch-all design burst ~50 loads into it —
/// 32 of 48 starved past the timeout on cabinet (2026-07-08). One category's
/// window (≤ 2N+1 entries, ≤2 layers each) is a burst the queue absorbs
/// comfortably, and it keeps the hot path — scrolling within the focused row —
/// fully prefetched.
fn desired_set(state: &OverlayState, side: usize) -> HashSet<(usize, usize)> {
    let mut set = HashSet::new();
    let Some((ci, cur)) = state.side_overlays[side].focused else {
        return set;
    };
    let Some(cat) = state.categories.get(ci) else {
        return set;
    };
    let len = cat.asset_ids.len();
    if cat.def.overlay_layers().is_empty() || len == 0 || cur >= len {
        return set;
    }
    let n = state.window_n;
    let cur = cur as i32;
    let lo = (cur - n).max(0) as usize;
    let hi = ((cur + n) as usize).min(len - 1);
    for idx in lo..=hi {
        set.insert((ci, idx));
    }
    set
}

/// Diff `side`'s resident set against the desired (focused-category) window:
/// release leavers, load newcomers. Returns whether any new load was started
/// (the caller arms the pump). Caller holds the lock and is on the render
/// thread.
///
/// Release-safety: the displayed entry is always the focused one, which is by
/// construction at the center of its window and therefore never a leaver; a
/// previously-displayed entry that leaves the window has already been
/// re-bound/hidden by `refresh_display` on the focus change that moved the
/// window (all within this same render-thread call — no frame is drawn
/// between the release and the display refresh).
fn sync_windows(state: &mut OverlayState, side: usize) -> bool {
    let desired = desired_set(state, side);

    // Leavers.
    let leavers: Vec<(usize, usize)> = state.side_overlays[side]
        .resident
        .keys()
        .filter(|k| !desired.contains(k))
        .copied()
        .collect();
    let released = leavers.len();
    for key in leavers {
        if let Some(entry) = state.side_overlays[side].resident.remove(&key) {
            release_entry(entry);
        }
    }

    // Newcomers. `def` is `&'static` and `asset_id` is Copy, so building the
    // entry doesn't hold a borrow of `state.categories` across the insert.
    let mut loaded = 0usize;
    for key in desired {
        if state.side_overlays[side].resident.contains_key(&key) {
            continue;
        }
        let (ci, idx) = key;
        let (def, asset_id) = match state.categories.get(ci) {
            Some(cat) => match cat.asset_ids.get(idx) {
                Some(&id) => (cat.def, id),
                None => continue,
            },
            None => continue,
        };
        let overlays = &mut state.side_overlays[side];
        let entry = build_entry(
            def,
            asset_id,
            side,
            &mut overlays.warned_stems,
            &state.lane_arc_paths,
        );
        overlays.resident.insert(key, entry);
        loaded += 1;
    }

    if loaded > 0 || released > 0 {
        log_info!(
            "preview_overlay: side {} window sync: +{} -{} (resident={})",
            side,
            loaded,
            released,
            state.side_overlays[side].resident.len()
        );
    }
    loaded > 0
}

/// Request-load every overlay layer of one `(category, asset)` and return the
/// entry tracking them. A layer whose load request fails is latched `Failed`
/// immediately (one WARN per stem per session via `warned`). MUST run on the
/// render thread.
fn build_entry(
    def: &'static CategoryDef,
    asset_id: u32,
    side: usize,
    warned: &mut HashSet<String>,
    lane_arc_paths: &HashMap<String, String>,
) -> ResidentEntry {
    let layers_def = def.overlay_layers();
    let (Some(scan_dir), Some(file_prefix)) = (def.scan_dir, def.file_prefix) else {
        // Overlay layers but no path formula — config bug; degrade to chrome.
        if !CATEGORY_CONFIG_FAIL_LOGGED.swap(true, Ordering::AcqRel) {
            log_warn!(
                "preview_overlay: {} has overlay layers but no scan_dir/file_prefix — chrome only (one-shot notice)",
                def.option_id
            );
        }
        return ResidentEntry {
            layers: layers_def
                .iter()
                .map(|_| LayerLoad {
                    handle: None,
                    tex_name: String::new(),
                    phase: Phase::Failed,
                })
                .collect(),
        };
    };

    let layers = layers_def
        .iter()
        .map(|layer| {
            // Same arc/name formula the old compositor used:
            //   arc  = <scan_dir>/<file_prefix><id:04><arc_suffix>.arc
            //   stem = <file_prefix><id:04><arc_suffix>   (engine registers by stem)
            let stem = format!("{}{:04}{}", file_prefix, asset_id, layer.arc_suffix);
            let arc_path = arc_path_for(lane_arc_paths, scan_dir, &stem);
            match asset_loader::load(&arc_path, &stem) {
                Some(handle) => LayerLoad {
                    handle: Some(handle),
                    tex_name: stem,
                    phase: Phase::Loading { polls: 0 },
                },
                None => {
                    // load() already logged the failure detail; warn once per
                    // stem per session with the identity, then stay chrome-only.
                    if warned.insert(stem.clone()) {
                        log_warn!(
                            "preview_overlay: load request failed for '{}' (side {}) — chrome only for this value",
                            stem,
                            side
                        );
                    }
                    LayerLoad {
                        handle: None,
                        tex_name: stem,
                        phase: Phase::Failed,
                    }
                }
            }
        })
        .collect();
    ResidentEntry { layers }
}

/// The on-disk arc to load for one overlay layer of `stem`: the pre-brightened
/// cached arc for gamma-corrected (lane) stems, the stock arc otherwise. A
/// lane stem whose cache build failed is absent from the map → stock (dark)
/// fallback, already warned at init.
fn arc_path_for(lane_arc_paths: &HashMap<String, String>, scan_dir: &str, stem: &str) -> String {
    lane_arc_paths
        .get(stem)
        .cloned()
        .unwrap_or_else(|| format!("{}/{}.arc", scan_dir, stem))
}

/// Build/refresh the pre-brightened lane arc cache: for every overlay layer
/// that opted into gamma correction (`PreviewLayer::gamma` — the LANE
/// single/double categories), ensure a brightened copy of each asset's arc
/// exists under [`LANE_CACHE_DIR`] and map its stem to that path. The overlay
/// then loads the cached arc instead of the stock one, so lanes render with
/// the same correction the old compositor applied (matching Konami's web
/// preview brightening).
///
/// Invalidation is per asset via a `CacheHasher` sidecar over (source arc
/// path+mtime, effective gamma bits): a brand-new asset (no sidecar), a
/// changed source arc (mtime), or a changed `lane_gamma_correction` all
/// rebuild that asset's cache entry; anything else is reused as-is. A failed
/// build warns and leaves the stem unmapped (stock dark art — degraded, not
/// broken). Runs on the init thread (image work — never call on the render
/// thread).
fn build_lane_cache(
    categories: &[DiscoveredCategory],
    gamma_override: Option<f32>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let (mut rebuilt, mut reused, mut failed) = (0usize, 0usize, 0usize);

    for cat in categories {
        let layers = cat.def.overlay_layers();
        if layers.is_empty() {
            continue;
        }
        let (Some(scan_dir), Some(file_prefix)) = (cat.def.scan_dir, cat.def.file_prefix) else {
            continue;
        };
        for layer in layers {
            // Only layers that opted into correction get a cache; the config
            // override replaces the built-in default when present (same
            // semantics as the old compositor's gamma_override).
            let Some(default_gamma) = layer.gamma else {
                continue;
            };
            let gamma = gamma_override.unwrap_or(default_gamma);
            for &asset_id in &cat.asset_ids {
                let stem = format!("{}{:04}{}", file_prefix, asset_id, layer.arc_suffix);
                match ensure_brightened_arc(scan_dir, &stem, gamma) {
                    Some((path, was_rebuilt)) => {
                        if was_rebuilt {
                            rebuilt += 1;
                        } else {
                            reused += 1;
                        }
                        map.insert(stem, path);
                    }
                    None => {
                        failed += 1;
                        log_warn!(
                            "preview_overlay: couldn't build brightened lane arc for '{}' — using stock (dark) art",
                            stem
                        );
                    }
                }
            }
        }
    }

    if rebuilt + reused + failed > 0 {
        log_info!(
            "preview_overlay: lane brighten cache ready ({} rebuilt, {} cached, {} failed)",
            rebuilt,
            reused,
            failed
        );
    }
    map
}

/// Ensure the brightened copy of `<scan_dir>/<stem>.arc` exists in the cache,
/// rebuilding if missing or stale. Returns `(cached_path, was_rebuilt)`, or
/// `None` on any failure (caller warns + falls back to the stock arc). The
/// inner PNG keeps its path inside the arc, so the engine registers the same
/// bare stem as the stock arc would.
fn ensure_brightened_arc(scan_dir: &str, stem: &str, gamma: f32) -> Option<(String, bool)> {
    let arc_name = format!("{}.arc", stem);
    let source = preview_gen::find_asset_arc(scan_dir, &arc_name)?; // warns if missing
    let source_str = source.to_string_lossy().into_owned();

    let out_path = format!("{}/{}.arc", LANE_CACHE_DIR, stem);
    let hash_path = format!("{}.hash", out_path);
    let mut hasher = CacheHasher::new(&hash_path);
    hasher.add(&source_str); // path + mtime
    hasher.add_str(&format!("gamma:{:08x}", gamma.to_bits()));
    hasher.finish();
    if hasher.matches() && Path::new(&out_path).is_file() {
        return Some((out_path, false));
    }

    // Rebuild: source arc → inner PNG → gamma LUT → re-encode → repack.
    let bytes = std::fs::read(&source).ok()?;
    let mut archive = ArcArchive::from_bytes(&bytes)?;
    let png_key = archive
        .files
        .keys()
        .find(|k| k.to_ascii_lowercase().ends_with(".png"))
        .cloned()?;
    let png = archive.files.get(&png_key)?;
    let mut img = image::load_from_memory(png).ok()?.into_rgba8();
    preview_gen::apply_gamma(&mut img, gamma);
    let mut encoded = Vec::new();
    img.write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
        .ok()?;
    archive.add_or_replace(png_key, encoded);

    mod_paths::mkdir_p(LANE_CACHE_DIR);
    if !archive.save(&out_path) {
        return None; // save() logs the write error
    }
    hasher.commit();
    Some((out_path, true))
}

/// One pump frame: poll every `Loading` layer on both sides, transition
/// resolved/timed-out ones, refresh the display when the focused entry gained
/// art, and re-schedule while anything is still loading. Runs on the render
/// thread; serial with all callbacks (same thread).
fn pump_tick() {
    let reschedule = {
        let mut state = lock_state();
        if !ENABLED.load(Ordering::Acquire) {
            // Mod disabled mid-load — stop pumping. shutdown()'s queued
            // teardown releases the resident handles.
            state.pump_running = false;
            false
        } else {
            let mut any_loading = false;
            for side in 0..2usize {
                let mut focused_gained_art = false;
                {
                    let overlays = &mut state.side_overlays[side];
                    let focused = overlays.focused;
                    let SideOverlays {
                        resident,
                        warned_stems,
                        ..
                    } = overlays;
                    for (key, entry) in resident.iter_mut() {
                        for layer in entry.layers.iter_mut() {
                            let Phase::Loading { polls } = layer.phase else {
                                continue;
                            };
                            // resolve_hash takes the loader's own leaf lock;
                            // STATE → LOADER is the established order.
                            let resolved = layer
                                .handle
                                .as_ref()
                                .and_then(|h| asset_loader::resolve_hash(h.name_hash));
                            match resolved {
                                Some(tex) => {
                                    layer.phase = Phase::Resolved { tex: tex.handle };
                                    if focused == Some(*key) {
                                        focused_gained_art = true;
                                        log_info!(
                                            "preview_overlay: bound '{}' (handle={:#x}) for side {} — overlay shown",
                                            layer.tex_name,
                                            tex.handle,
                                            side
                                        );
                                    }
                                }
                                None if polls + 1 >= LOAD_TIMEOUT_POLLS => {
                                    if let Some(handle) = layer.handle.take() {
                                        asset_loader::release(handle);
                                    }
                                    layer.phase = Phase::Failed;
                                    if warned_stems.insert(layer.tex_name.clone()) {
                                        log_warn!(
                                            "preview_overlay: '{}' never resolved after {} polls (side {}) — chrome only for this value",
                                            layer.tex_name,
                                            LOAD_TIMEOUT_POLLS,
                                            side
                                        );
                                    }
                                }
                                None => {
                                    layer.phase = Phase::Loading { polls: polls + 1 };
                                    any_loading = true;
                                }
                            }
                        }
                    }
                }
                if focused_gained_art {
                    refresh_display(&state, side);
                }
            }
            state.pump_running = any_loading;
            any_loading
        }
    };
    if reschedule {
        widget_renderer::run_on_render_thread(pump_tick);
    }
}

/// Under the lock: if new loads were started and the pump isn't already
/// running, mark it running and return true — the caller schedules
/// [`pump_tick`] AFTER dropping the lock (CLAUDE.md rule 6).
fn arm_pump(state: &mut OverlayState, started_loads: bool) -> bool {
    if started_loads && !state.pump_running {
        state.pump_running = true;
        true
    } else {
        false
    }
}

/// Bring `side`'s sprites in line with its focused entry: for each overlay
/// layer of the focused category, bind + show the layer's sprite if its art is
/// resolved (positioned/sized to the layer's marker rect), hide it otherwise
/// (chrome shows through while loading / after failure). Hides everything when
/// nothing overlay-bearing is focused or the modal is closed. Idempotent —
/// safe to call on every transition. Caller holds the lock and is on the
/// render thread.
fn refresh_display(state: &OverlayState, side: usize) {
    let overlays = &state.side_overlays[side];
    let target = if overlays.modal_open {
        overlays.focused
    } else {
        None
    };

    let Some((ci, idx)) = target else {
        for sprite in &overlays.sprites {
            sprite.hide();
        }
        return;
    };

    let cat = state.categories.get(ci);
    let entry = overlays.resident.get(&(ci, idx));
    let layer_count = cat.map_or(0, |c| c.def.overlay_layers().len());
    let rects = cat.and_then(|c| state.marker_rects.get(c.def.option_id));

    for (i, sprite) in overlays.sprites.iter().enumerate() {
        let mut shown = false;
        if i < layer_count {
            if let Some(Phase::Resolved { tex }) =
                entry.and_then(|e| e.layers.get(i)).map(|l| &l.phase)
            {
                if let Some(rect) = layer_rect(rects, i, side) {
                    // Position + size to the layer's chrome marker rect
                    // (screen px). Sizing to the marker — not the whole box —
                    // is what keeps the aspect correct: the art fills exactly
                    // the region the compositor drew into (full-UV quad
                    // stretched to the sprite's w/h, matching the
                    // compositor's resize-to-marker).
                    sprite.set_position(rect.x, rect.y);
                    sprite.set_size(rect.w, rect.h);
                    sprite.set_texture_id(*tex as i32);
                    sprite.set_uv(0.0, 0.0, 1.0, 1.0);
                    sprite.show();
                    shown = true;
                }
            }
        }
        if !shown {
            sprite.hide();
        }
    }
}

/// Screen placement for overlay layer `layer_idx` of the focused option: its
/// cached template marker rect mapped through `side`'s chrome origin. A
/// missing marker falls back to the full chrome box for layer 0 (art shows,
/// stretched — better than hiding) but hides later layers (two full-box
/// sprites would overlap into nonsense).
fn layer_rect(
    rects: Option<&Vec<Option<MarkerRect>>>,
    layer_idx: usize,
    side: usize,
) -> Option<ScreenRect> {
    match rects.and_then(|v| v.get(layer_idx)).copied().flatten() {
        Some(m) => Some(ScreenRect::from_marker(side, m)),
        None if layer_idx == 0 => Some(ScreenRect::full_box(side)),
        None => None,
    }
}

/// Ensure `side` has its full sprite pool ([`MAX_OVERLAY_LAYERS`], hidden).
/// No-op once created — the sprites are process-lifetime objects (see the
/// module doc). Created hidden at the side's full chrome box; the real
/// position + size are set per-bind in `refresh_display` from the focused
/// layer's marker rect. Caller holds the lock and is on the render thread.
fn ensure_sprites(overlays: &mut SideOverlays, side: usize) {
    while overlays.sprites.len() < MAX_OVERLAY_LAYERS {
        let init = ScreenRect::full_box(side);
        match widget_renderer::create_image_widget(&ImageWidgetConfig {
            x: init.x,
            y: init.y,
            width: init.w,
            height: init.h,
            texture_name: None,
            ..Default::default()
        }) {
            Some(sprite) => {
                // Created hidden (create_image_widget starts hidden); shown +
                // positioned once art binds in `refresh_display`.
                overlays.sprites.push(sprite);
            }
            None => {
                log_warn!(
                    "preview_overlay: create_image_widget returned None for side {} (renderer not ready?)",
                    side
                );
                return;
            }
        }
    }
}

/// Full teardown for `side`: hide the sprites FIRST (nothing visible may
/// reference a texture being freed), then release the entire resident set and
/// clear the session state. The sprites are kept (hidden) for the process
/// lifetime. Caller holds the lock and is on the render thread.
fn teardown_side(state: &mut OverlayState, side: usize) {
    let overlays = &mut state.side_overlays[side];
    for sprite in &overlays.sprites {
        sprite.hide();
    }
    overlays.focused = None;
    overlays.modal_open = false;
    overlays.warned_stems.clear();
    let resident = std::mem::take(&mut overlays.resident);
    for (_, entry) in resident {
        release_entry(entry);
    }
}
