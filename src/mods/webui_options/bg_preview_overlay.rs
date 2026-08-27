//! Animated BACKGROUND previews — draws the focused background value's clip,
//! animating and looping, inside the options-modal chrome marker rect by
//! driving the game's own AFP/BM2D runtime.
//!
//! ## Model
//!
//! The two BACKGROUND rows (`customize_background`, `customize_background_gameplay`)
//! are animated AFP composites, not static PNGs — so the shipped
//! `preview_overlay` (which draws static `agcs::Sprite`s) leaves them
//! `overlay_layers: &[]` and shows only the generic chrome. This sibling module
//! fills that gap: on focusing a background value it loads that value's BM2D
//! package on demand (`bm2d_package`), instantiates its `bg_root` clip as an
//! AFP layer we own (`bm2d_api`), scales/positions/masks it into the chrome's
//! 16:9 marker rect, and lets the ENGINE animate + loop it. Both modules never
//! draw for the same side at once (a side's focus is on exactly one row), so
//! there is no z-order conflict between them.
//!
//! ## Package lifecycle — private alias packages, resident until modal close
//!
//! **We never bind a layer to a registry entry the game also uses.** The
//! game's BM2D data manager is name-keyed, not refcounted, and its `release`
//! **defers the actual `afpu_destroy_package_data` to `Manager::Update`** (a
//! later frame, in `gameMain`). Because WebUI Options applies selections
//! immediately, every scroll step makes the game's backdrop manager switch the
//! live song-select background — releasing the previously-applied package *by
//! name*. Two cabinet crashes (2026-07-09) both came from that: our preview
//! layer sat on the shared `background_%04d` entry, the game released it, and
//! Update destroyed the stream under our live layer
//! (`F:afpu-package: destroy stream[..] is used at layer[..]`).
//!
//! The fix is ownership, not timing: at init we copy each background arc to a
//! LayeredFS-served **alias** (`data_mods/bg_preview/arc/custom/background/
//! bgprev_background_%04d.arc`, hash-guarded so warm boots skip) and the
//! overlay loads `bgprev_background_%04d` — a registry entry + afpu package
//! **only this module ever references**. The game's own load/release of
//! `background_%04d` can never touch it. The manager's arc-existence probe is
//! `avs_fs_lstat` (Ghidra: `Ordinal_100` = `XCnbrep7000063`) and the open goes
//! through `avs_fs_open` — both already LayeredFS-hooked, so the alias rides
//! the game's entire proven load pipeline with zero new hook code.
//!
//! Alias packages are kept resident as a bounded **prefetch window**: the
//! focused background category's `[cur-N, cur+N]` around the current value
//! (`N` = `custom_options.preview_window`, the shipped overlay's knob). The
//! window is re-diffed on every focus/value change — newcomers are requested
//! BEFORE leavers are released, so a key common to both (the two background
//! rows share one asset pool) refcount-overlaps in `bm2d_package` instead of
//! paying a release+reload. Scrolling within the row is instant (the window
//! slides by ±1; the edge loads N steps ahead). Releasing a private alias on
//! window-leave is safe: only the focused value ever has a layer (and it is
//! by construction the window center, never a leaver), and the game's own
//! backdrop churn proves the manager tolerates quick release→re-request of
//! the same name (whole-list held-scroll torture, 2026-07-09). Leaving the
//! background rows keeps the window warm (instant return); menu-close
//! releases everything.
//!
//! ## Scope
//!
//! Both background rows, both player sides, independently and simultaneously
//! (per-side state; the two layers share group 4 / priority 300 with disjoint
//! screen masks — equal priorities coexist fine, e.g. the modal's own rows
//! are all priority 100). At most ONE AFP layer is alive per side (the
//! focused, shown value); scrolling past the window edge shows chrome until
//! the package resolves (~0.5 s), then the clip.
//!
//! ## Placement (screenshot-anchored, mirrors `preview_overlay`)
//!
//! The `seop_image_<id>` chrome renders 1:1 into screen pixels at a per-side
//! origin ([`CHROME_ORIGIN`], measured from captures — the P2 modal is on the
//! RIGHT, not a mirror). The maintainer authored a 16:9 green marker rect on
//! each background `_TEMPLATE.png`; at init we read it (template px) via
//! `preview_gen::marker_rect_for`, and at show time offset it by the side's
//! origin. The 1280×720 clip scales into that rect (`sx = marker_w/1280`,
//! `sy = marker_h/720`; equal for a true-16:9 marker) via `layer_set_scale` +
//! `layer_set_position` (top-left anchor, they compose), and a screen-space
//! `layer_set_mask` hard-crops any overflow.
//!
//! ## Z-order & playback (cabinet-validated, Step 1)
//!
//! The layer is put in **group 4, priority 300** (over the modal: its root is
//! grp-4 prio 99, rows prio 100 — display sorts ascending, higher on top),
//! with attribute `0x200` (the standard post-create display setup the game
//! applies to every layer) and `play(1.0)` (looping is the engine default).
//! The engine renders it from its per-group display jobs — we NEVER self-drive
//! `afp_do_render` (Step-1 probe v1 proved that crashes).
//!
//! ## Threading & lifetime
//!
//! All package/layer calls run on the render thread (the modal events fire
//! there; the pump self-reschedules via `run_on_render_thread`). The state
//! mutex is never held across a `run_on_render_thread` schedule (compute under
//! lock, drop, then schedule — CLAUDE.md rule 6); poisoned locks recover via
//! `into_inner`. Panic-free by construction: no unwrap in callbacks or the
//! pump, and the few index ops (`sides[side]`, `CHROME_ORIGIN[side]`) are
//! bounds-guaranteed by the `side > 1` gates / constant 0..2 loops. Layers
//! are destroyed BEFORE their package is released, and the consuming
//! `AfpLayer`/`LoadTicket` handles make double-free a compile error.

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::services::avs_layeredfs::cache_hasher::CacheHasher;
use crate::services::avs_layeredfs::mod_paths;
use crate::services::{bm2d_api, bm2d_package, custom_options, widget_renderer};
use crate::{log_info, log_warn};

use super::discovery::{DiscoveredCategory, MarkerColor};
use super::preview_gen::{self, MarkerRect};

/// Prefix that turns a background stem into this module's private alias name
/// (`background_0047` → `bgprev_background_0047`). The game never composes
/// `bgprev_*` names, so alias registry entries are wholly ours.
const ALIAS_PREFIX: &str = "bgprev_";
/// The LayeredFS mod folder the alias arc copies live in. Its inner layout
/// mirrors the game path (`arc/custom/background/<alias>.arc`) so the
/// existing direct-file-replacement machinery serves them.
const ALIAS_MOD_DIR: &str = "./data_mods/bg_preview";
/// Hash sidecars for the alias copies (in `_cache`, which mod scanning skips).
const ALIAS_HASH_DIR: &str = "./data_mods/_cache/bg_preview";

/// Native background clip dimensions — the `bg_root` clip is authored at
/// 1280×720, so scaling into a marker rect is `marker_dim / native_dim`.
const NATIVE_W: f32 = 1280.0;
const NATIVE_H: f32 = 720.0;

/// Screen-space top-left (px) where each side's option preview-box chrome
/// renders. Mirrors `preview_overlay::CHROME_ORIGIN` (kept local — the shipped
/// module's copy is private and must not be touched). The 368×172 template
/// maps 1:1 to screen pixels, so a template-pixel marker `(mx,my,mw,mh)`
/// renders at `(origin.x + mx, origin.y + my, mw, mh)`.
const CHROME_ORIGIN: [(f32, f32); 2] = [
    (185.0, 463.0), // P1 — measured
    (742.0, 463.0), // P2 — measured (same Y as P1)
];

/// The BM2D group the options modal lives in (root prio 99, rows prio 100).
const GROUP_OVER_MODAL: u16 = 4;
/// Above the modal's row clips — CE-confirmed to cover modal + darkener.
const PRIO_OVER_MODAL: u16 = 300;
/// Standard post-create display-setup attribute the game sets on every layer.
const ATTR_DISPLAY_SETUP: u32 = 0x200;
/// `bg_root` clip export name (uniform across all 51 background arcs).
const TEMPLATE: &str = "bg_root";

/// Pump polls (render-hook ticks, NOT frames — ~300–600/s on cabinet) before
/// a `Loading` package gives up. A background resolves in ~141 polls in
/// isolation; 3600 (~6–12 s wall) only ever reclaims a genuinely-broken load.
const LOAD_TIMEOUT_POLLS: u32 = 3600;

/// A resident background package's load lifecycle.
enum Phase {
    /// Package requested; polling `is_ready` (`polls` toward the timeout).
    Loading { polls: u32 },
    /// Package created + ready — a layer can be instantiated from it.
    Ready,
    /// Load timed out / package rejected — chrome only for this value.
    Failed,
}

/// A background package kept resident for the modal session.
struct Resident {
    ticket: bm2d_package::LoadTicket,
    phase: Phase,
}

/// One player side's background-preview state.
struct SideBg {
    modal_open: bool,
    /// The focused background target `(category index, asset index)`, or None
    /// when a non-background row (or nothing) is focused.
    focused: Option<(usize, usize)>,
    /// The prefetch window's resident alias packages: the focused category's
    /// `[cur-N, cur+N]`, re-diffed on each focus/value change (see the module
    /// doc). Kept warm while focus is on a non-background row; fully released
    /// on menu-close by [`teardown_side`].
    resident: HashMap<(usize, usize), Resident>,
    /// The single visible AFP layer (the focused, ready value), if any.
    layer: Option<bm2d_api::AfpLayer>,
    /// Which `(category, asset)` the live layer shows.
    layer_key: Option<(usize, usize)>,
    /// One-shot failure logging, keyed by asset id.
    warned: HashSet<u32>,
}

impl SideBg {
    fn new() -> Self {
        Self {
            modal_open: false,
            focused: None,
            resident: HashMap::new(),
            layer: None,
            layer_key: None,
            warned: HashSet::new(),
        }
    }
}

struct BgState {
    /// Discovered categories (resident keys index into this).
    categories: Vec<DiscoveredCategory>,
    /// Per-background-category green marker rect (template px), read once at
    /// init. Absent ⇒ that row can't be placed (chrome only).
    markers: HashMap<String, MarkerRect>,
    sides: [SideBg; 2],
    /// Whether a `pump_tick` closure is scheduled/running (render-thread only).
    pump_running: bool,
    /// `custom_options.animate_backgrounds` (default true). False → layers are
    /// created then left paused (`play(0.0)`) — static first frame, R7.
    animate: bool,
    /// Prefetch-window half-width N (`custom_options.preview_window`, clamped
    /// 0..=10 at init). Resident packages per side ≤ 2N+1.
    window_n: i32,
}

// NOTE: no `unsafe impl Send` — every field is already Send (handles wrap a
// u32 / CString; no raw pointers are stored). Keeping auto-derivation means
// the compiler re-checks this if a future field changes that.

impl BgState {
    fn new() -> Self {
        Self {
            categories: Vec::new(),
            markers: HashMap::new(),
            sides: [SideBg::new(), SideBg::new()],
            pump_running: false,
            animate: true,
            window_n: 3,
        }
    }
}

static STATE: Lazy<Mutex<BgState>> = Lazy::new(|| Mutex::new(BgState::new()));

/// Active flag — callbacks are registered once for the process lifetime and
/// gate on this so a disabled mod no-ops.
static ENABLED: AtomicBool = AtomicBool::new(false);
static CALLBACKS_REGISTERED: AtomicBool = AtomicBool::new(false);
static POISON_LOGGED: AtomicBool = AtomicBool::new(false);

fn lock_state() -> MutexGuard<'static, BgState> {
    STATE.lock().unwrap_or_else(|poisoned| {
        if !POISON_LOGGED.swap(true, Ordering::AcqRel) {
            log_warn!("bg_preview_overlay: state mutex poisoned — recovered (one-shot notice)");
        }
        poisoned.into_inner()
    })
}

/// Initialize the animated-background overlay with the mod's discovered
/// categories, the prefetch-window half-width (`custom_options.preview_window`,
/// clamped 0..=10 here) and the `animate_backgrounds` config value (false →
/// previews show a static first frame instead of animating; never blank
/// chrome). Reads each background row's green marker rect once, registers the
/// modal-lifecycle + preview-request callbacks (once ever), and arms the
/// overlay. Call from `WebUiOptionsMod::enable()`, guarded by the caller on
/// `bm2d_api::afp_layers_available() && bm2d_package::is_available()`.
pub fn init(categories: Vec<DiscoveredCategory>, window_n: i32, animate: bool) {
    // The alias arcs are served by LayeredFS's direct-file-replacement path;
    // without it the game's loader can't see them at all.
    if !crate::services::avs_layeredfs::is_available() {
        log_warn!(
            "bg_preview_overlay: LayeredFS unavailable — alias arcs can't be served; background previews disabled (chrome only)"
        );
        return;
    }

    // Build/refresh the private alias arc copies BEFORE taking the state lock
    // (file IO, on the init thread — mirrors the shipped lane cache). The
    // overlay only ever loads `bgprev_*` aliases; see the module doc's
    // ownership rationale (the two 2026-07-09 crashes).
    let (copied, reused, missing) = build_alias_cache(&categories);
    if copied > 0 {
        // New alias files exist on disk — refresh LayeredFS's cached mod-file
        // list so lstat/open see them (the folder_expansion rescan pattern).
        mod_paths::init_mod_paths();
    }
    if copied + reused + missing > 0 {
        log_info!(
            "bg_preview_overlay: alias arc cache ready ({} copied, {} cached, {} missing)",
            copied,
            reused,
            missing
        );
    }

    // End-to-end serving check: the aliases only work if LayeredFS will
    // actually resolve them (a configured `layeredfs.allowlist` that predates
    // this feature silently excludes the generated `bg_preview` folder — the
    // game's lstat probe would then fail every request with one warn per
    // value and no hint why). Probe one alias through the same lookup the
    // hooks use; if it can't resolve, stay chrome-only with ONE actionable
    // line.
    if copied + reused > 0 && !alias_serving_works(&categories) {
        log_warn!(
            "bg_preview_overlay: alias arcs exist but LayeredFS can't serve them (mod folder 'bg_preview' excluded by allowlist/blocklist?) — background previews disabled (chrome only)"
        );
        return;
    }

    {
        let mut state = lock_state();

        let mut markers = HashMap::new();
        for cat in &categories {
            if !cat.def.bg_overlay {
                continue;
            }
            match preview_gen::marker_rect_for(cat.def.option_id, MarkerColor::Green) {
                Some(r) => {
                    log_info!(
                        "bg_preview_overlay: {} marker rect (template px) {}x{} at ({},{})",
                        cat.def.option_id,
                        r.w,
                        r.h,
                        r.x,
                        r.y
                    );
                    markers.insert(cat.def.option_id.to_string(), r);
                }
                None => log_warn!(
                    "bg_preview_overlay: {} has no resolvable green marker — chrome only",
                    cat.def.option_id
                ),
            }
        }

        state.categories = categories;
        state.markers = markers;
        state.animate = animate;
        state.window_n = window_n.clamp(0, 10);
        log_info!(
            "bg_preview_overlay: init ({} categories, {} background marker(s), animate={}, window_n={})",
            state.categories.len(),
            state.markers.len(),
            state.animate,
            state.window_n
        );
    }

    // Tear down BOTH sides' previous-session state on the render thread —
    // unconditionally, including sides with an open modal. `state.categories`
    // was just replaced, so any surviving `(ci, idx)` keys would silently
    // remap to different assets (wrong art), and a pump chain that died
    // during a disabled window would leave Loading residents nobody re-arms.
    // An open modal recovers automatically: `modal_open` survives teardown
    // and the preview request re-fires every focus tick, so the next tick
    // rebuilds the window + layer from the NEW categories.
    widget_renderer::run_on_render_thread(|| {
        let mut state = lock_state();
        for side in 0..2usize {
            teardown_side(&mut state, side);
        }
    });

    if !CALLBACKS_REGISTERED.swap(true, Ordering::AcqRel) {
        custom_options::on_menu_open(on_menu_open);
        custom_options::on_menu_close(on_menu_close);
        custom_options::on_preview_request(on_preview_request);
    }
    ENABLED.store(true, Ordering::Release);
}

/// Disable: hide/destroy any live layer + release every resident package on
/// the render thread. Callbacks stay registered but no-op while `ENABLED` is
/// false.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Release);
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

/// Destroy a layer, surfacing an engine-side failure. A failed destroy means
/// the layer may still be bound to its package when that package is later
/// released — the exact crash class this module exists to avoid — so it must
/// never pass silently.
fn destroy_layer_checked(layer: bm2d_api::AfpLayer) {
    let id = layer.id();
    if !bm2d_api::destroy_layer(layer) {
        log_warn!(
            "bg_preview_overlay: destroy_layer(0x{:08X}) reported failure — possible engine-side layer leak",
            id
        );
    }
}

/// Full teardown: destroy the side's layer FIRST, then release every resident
/// package (nothing visible references them once the layer is gone). Leaves
/// the side idle. Render-thread only.
fn teardown_side(state: &mut BgState, side: usize) {
    let s = &mut state.sides[side];
    if let Some(layer) = s.layer.take() {
        destroy_layer_checked(layer);
    }
    s.layer_key = None;
    for (_key, res) in s.resident.drain() {
        bm2d_package::release(res.ticket);
    }
    s.focused = None;
}

/// Destroy the visible layer but KEEP resident packages (focus left a
/// background row; returning is then instant). Render-thread only.
fn hide_layer(state: &mut BgState, side: usize) {
    let s = &mut state.sides[side];
    if let Some(layer) = s.layer.take() {
        destroy_layer_checked(layer);
    }
    s.layer_key = None;
}

fn on_menu_open(side: u8) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let side = side as usize;
    let mut state = lock_state();
    // Sweep any stale layer/packages from a previous session, then mark open.
    teardown_side(&mut state, side);
    state.sides[side].modal_open = true;
    state.sides[side].warned.clear();
}

/// Modal closed. Deliberately NOT gated on `ENABLED` (unlike open/preview):
/// a disable-while-open must still get its close-time cleanup, or the side's
/// layer + resident packages would strand until process exit.
fn on_menu_close(side: u8) {
    if side > 1 {
        return;
    }
    let side = side as usize;
    let mut state = lock_state();
    let had = !state.sides[side].resident.is_empty() || state.sides[side].layer.is_some();
    teardown_side(&mut state, side);
    state.sides[side].modal_open = false;
    if had {
        log_info!(
            "bg_preview_overlay: torn down for side {} (menu close)",
            side
        );
    }
}

fn on_preview_request(side: u8, option_id: &str) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let side = side as usize;

    let start_pump = {
        let mut state = lock_state();
        if !state.sides[side].modal_open {
            return;
        }

        let wants_pump = match compute_target(&state, side, option_id) {
            Target::Background(ci, idx) => ensure_focus(&mut state, side, ci, idx),
            Target::NotBackground | Target::LookupFailed => {
                // Focus left the background rows: drop the visible layer but
                // keep packages resident (instant return). No release here.
                if state.sides[side].focused.take().is_some() {
                    hide_layer(&mut state, side);
                }
                false
            }
        };
        // One pump chain at a time: pump_tick self-reschedules while work
        // remains, so scheduling here while a chain is live would stack
        // chains (double-polling → double-speed timeouts). `pump_running` is
        // only mutated under the lock on the render thread (callbacks + pump
        // are serial there).
        if wants_pump && !state.pump_running {
            state.pump_running = true;
            true
        } else {
            false
        }
    };

    if start_pump {
        widget_renderer::run_on_render_thread(pump_tick);
    }
}

enum Target {
    Background(usize, usize),
    NotBackground,
    LookupFailed,
}

/// Resolve `option_id` to its background target for `side`. Never defaults to
/// asset 0 on a failed lookup (wrong art is worse than none). A background row
/// whose template marker didn't resolve at init is treated as NotBackground:
/// the degradation decision (chrome-only) was already made — loading packages
/// for it would just burn engine file-queue time and fail at create.
fn compute_target(state: &BgState, side: usize, option_id: &str) -> Target {
    let Some((ci, cat)) = state
        .categories
        .iter()
        .enumerate()
        .find(|(_, c)| c.def.option_id == option_id)
    else {
        return Target::NotBackground;
    };
    if !cat.def.bg_overlay || !state.markers.contains_key(option_id) {
        return Target::NotBackground;
    }
    let Some(cur) = custom_options::get_value(side as u8, option_id) else {
        return Target::LookupFailed;
    };
    if cur < 0 || (cur as usize) >= cat.asset_ids.len() {
        return Target::LookupFailed;
    }
    Target::Background(ci, cur as usize)
}

/// Point the side at `(ci, idx)`: drop a layer showing a different value and
/// re-diff the prefetch window around the new focus (the pump then (re)creates
/// the focused layer once its package is ready — instantly if already
/// resident). Returns whether the pump should run. Caller holds the lock and
/// is on the render thread.
fn ensure_focus(state: &mut BgState, side: usize, ci: usize, idx: usize) -> bool {
    let key = (ci, idx);
    if state.sides[side].focused == Some(key) {
        return false; // re-fires every focus tick — nothing changed
    }
    state.sides[side].focused = Some(key);

    // If the live layer shows a different value, drop it now (the pump
    // recreates for the new focus once its package is ready).
    if state.sides[side].layer_key != Some(key) {
        if let Some(layer) = state.sides[side].layer.take() {
            destroy_layer_checked(layer);
        }
        state.sides[side].layer_key = None;
    }

    sync_window(state, side);
    // Always pump on a real focus change: it creates the layer immediately if
    // the focused package is already resident+ready, else polls until it
    // resolves. (sync_window's return alone would miss the already-ready case.)
    true
}

/// The focused category's desired resident set: `[cur-N, cur+N]` clamped to
/// its asset range, or empty when no background row is focused.
fn desired_window(state: &BgState, side: usize) -> HashSet<(usize, usize)> {
    let mut set = HashSet::new();
    let Some((ci, cur)) = state.sides[side].focused else {
        return set;
    };
    let Some(cat) = state.categories.get(ci) else {
        return set;
    };
    let len = cat.asset_ids.len();
    if len == 0 || cur >= len {
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

/// Diff the side's resident set against the focused window: request newcomers
/// FIRST, then release leavers — a key common to both (the two background
/// rows share one asset pool, so switching rows re-keys the same alias names)
/// refcount-overlaps in `bm2d_package` instead of paying a release+reload.
/// Leavers are never the focused key (it's the window center), so the live
/// layer's package is never released here. Returns whether any new load
/// started. Caller holds the lock and is on the render thread.
fn sync_window(state: &mut BgState, side: usize) -> bool {
    let desired = desired_window(state, side);

    // Newcomers.
    let mut started = 0usize;
    for &key in &desired {
        if state.sides[side].resident.contains_key(&key) {
            continue;
        }
        let Some((dir, name, asset_id)) = load_params(state, key.0, key.1) else {
            continue;
        };
        match bm2d_package::request_load(&dir, &name) {
            Some(ticket) => {
                state.sides[side].resident.insert(
                    key,
                    Resident {
                        ticket,
                        phase: Phase::Loading { polls: 0 },
                    },
                );
                started += 1;
            }
            None => {
                if state.sides[side].warned.insert(asset_id) {
                    log_warn!(
                        "bg_preview_overlay: request_load({:?}) failed (side {}) — chrome only",
                        name,
                        side
                    );
                }
            }
        }
    }

    // Leavers (Failed entries included — they retry when the window re-forms
    // over them later).
    let leavers: Vec<(usize, usize)> = state.sides[side]
        .resident
        .keys()
        .filter(|k| !desired.contains(k))
        .copied()
        .collect();
    let released = leavers.len();
    for key in leavers {
        if let Some(res) = state.sides[side].resident.remove(&key) {
            bm2d_package::release(res.ticket);
        }
    }

    if started > 0 || released > 0 {
        log_info!(
            "bg_preview_overlay: side {} window sync: +{} -{} (resident={})",
            side,
            started,
            released,
            state.sides[side].resident.len()
        );
    }
    started > 0
}

/// The `(manager_dir, alias_name, asset_id)` for a background target, or None
/// if the category isn't a placeable background (no scan_dir/prefix). The
/// name is the PRIVATE alias (`bgprev_background_%04d`) — never the game's
/// own `background_%04d` (see the module doc's ownership rationale).
fn load_params(state: &BgState, ci: usize, idx: usize) -> Option<(String, String, u32)> {
    let cat = state.categories.get(ci)?;
    let scan_dir = cat.def.scan_dir?;
    let file_prefix = cat.def.file_prefix?;
    let &asset_id = cat.asset_ids.get(idx)?;
    // The BM2D data manager takes a dir relative to `data/arc/`; discovery's
    // scan_dir is the full `data/arc/custom/background` path.
    let manager_dir = scan_dir.strip_prefix("data/arc/").unwrap_or(scan_dir);
    let name = format!("{}{}{:04}", ALIAS_PREFIX, file_prefix, asset_id);
    Some((manager_dir.to_string(), name, asset_id))
}

/// Probe whether LayeredFS's mod-file lookup resolves the first discovered
/// background's alias arc — the exact same `find_first_modfile` the
/// lstat/open hooks consult. False means the generated folder isn't being
/// served (allowlist/blocklist), so every preview load would fail.
fn alias_serving_works(categories: &[DiscoveredCategory]) -> bool {
    let Some((cat, &asset_id)) = categories
        .iter()
        .filter(|c| c.def.bg_overlay)
        .find_map(|c| c.asset_ids.first().map(|id| (c, id)))
    else {
        return true; // nothing to serve, nothing to check
    };
    let (Some(scan_dir), Some(file_prefix)) = (cat.def.scan_dir, cat.def.file_prefix) else {
        return true;
    };
    let manager_dir = scan_dir.strip_prefix("data/arc/").unwrap_or(scan_dir);
    // Normalized game path as the hooks see it (post `data/` strip).
    let probe = format!(
        "arc/{}/{}{}{:04}.arc",
        manager_dir, ALIAS_PREFIX, file_prefix, asset_id
    );
    mod_paths::find_first_modfile(&probe).is_some()
}

/// Ensure every discovered background has an up-to-date private alias copy of
/// its arc under [`ALIAS_MOD_DIR`] (hash-sidecar-guarded — warm boots skip
/// unchanged sources). Returns `(copied, reused, missing)`. Runs on the init
/// thread (file IO — never on the render thread).
fn build_alias_cache(categories: &[DiscoveredCategory]) -> (usize, usize, usize) {
    let (mut copied, mut reused, mut missing) = (0usize, 0usize, 0usize);
    // Both background rows share one asset pool — process each (out_dir, stem)
    // once. (Keyed on the pair so a future bg category with a different
    // scan_dir but colliding stems isn't silently skipped.)
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for cat in categories {
        if !cat.def.bg_overlay {
            continue;
        }
        let (Some(scan_dir), Some(file_prefix)) = (cat.def.scan_dir, cat.def.file_prefix) else {
            continue;
        };
        let manager_dir = scan_dir.strip_prefix("data/arc/").unwrap_or(scan_dir);
        let out_dir = format!("{}/arc/{}", ALIAS_MOD_DIR, manager_dir);
        for &asset_id in &cat.asset_ids {
            let stem = format!("{}{:04}", file_prefix, asset_id);
            if !seen.insert((out_dir.clone(), stem.clone())) {
                continue;
            }
            match ensure_alias_arc(scan_dir, &out_dir, &stem) {
                Some(true) => copied += 1,
                Some(false) => reused += 1,
                None => missing += 1, // ensure_alias_arc logged the specific cause
            }
        }
    }
    (copied, reused, missing)
}

/// Ensure the alias copy of `stem`'s arc exists and is current. The source is
/// resolved by probing the game loader's variant names (`_v3`, `_v0`, plain,
/// `_lite` — NOTE: not the loader's exact precedence, whose `_lite` pick is
/// machine-type-gated; irrelevant here because every variant carries the same
/// clip, so any hit previews correctly).
///
/// The copy is NOT byte-identical: the inner IFS is renamed to carry the
/// alias stem (`data/custom/background/background_0001.ifs` →
/// `.../bgprev_background_0001.ifs`). The BM2D data manager locates an arc's
/// IFS by the FNV-1a hash of `<arc_name>.ifs`; with the original name inside,
/// that lookup misses and `Manager::Update` null-derefs (cabinet crash,
/// 2026-07-09 third run). Payload bytes are copied raw (still compressed) via
/// `arc::rewrite_paths` — only the cue/string tables are rebuilt.
///
/// Returns `Some(was_copied)`, or `None` on failure (each cause logged
/// specifically: no source variant / unreadable source / rewrite reject /
/// write failure).
fn ensure_alias_arc(scan_dir: &str, out_dir: &str, stem: &str) -> Option<bool> {
    let Some(source) = ["_v3", "_v0", "", "_lite"].iter().find_map(|suffix| {
        preview_gen::find_asset_arc_opt(scan_dir, &format!("{}{}.arc", stem, suffix))
    }) else {
        log_warn!(
            "bg_preview_overlay: no source arc for '{}' (any variant) — that value stays chrome-only",
            stem
        );
        return None;
    };
    let source_str = source.to_string_lossy().into_owned();
    let alias_stem = format!("{}{}", ALIAS_PREFIX, stem);

    let out_path = format!("{}/{}.arc", out_dir, alias_stem);
    let hash_path = format!("{}/{}.arc.hash", ALIAS_HASH_DIR, alias_stem);
    let mut hasher = CacheHasher::new(&hash_path);
    hasher.add(&source_str); // path + mtime
                             // Cache-format marker: v2 renames the inner IFS to the alias stem. Bumping
                             // this invalidates alias copies produced by older builds (the v1 plain
                             // copies crash the manager — see above).
    hasher.add_str("fmt:v2-inner-ifs-rename");
    hasher.finish();
    if hasher.matches() && Path::new(&out_path).is_file() {
        return Some(false);
    }

    let Ok(bytes) = std::fs::read(&source) else {
        log_warn!(
            "bg_preview_overlay: source arc {} exists but can't be read — '{}' stays chrome-only",
            source_str,
            stem
        );
        return None;
    };
    // Rename `<dir>/<stem or stem_variant>.ifs` → `<dir>/<alias_stem>.ifs`.
    // Tight predicate: after the stem, only ".ifs" or "_<variant>.ifs" counts
    // (so stem `background_0001` can't false-match `background_00010.ifs`),
    // and only the FIRST match renames (two matching IFS entries in one arc
    // would otherwise produce duplicate cue names).
    let renamed_once = std::cell::Cell::new(false);
    let renamed = crate::core::arc::rewrite_paths(&bytes, |path| {
        if renamed_once.get() {
            return None;
        }
        let (dir, base) = match path.rfind('/') {
            Some(p) => (&path[..=p], &path[p + 1..]),
            None => ("", path),
        };
        let rest = base.strip_prefix(stem)?;
        let is_variant_ifs = rest == ".ifs" || (rest.starts_with('_') && rest.ends_with(".ifs"));
        if is_variant_ifs {
            renamed_once.set(true);
            Some(format!("{}{}.ifs", dir, alias_stem))
        } else {
            None
        }
    });
    let Some(renamed) = renamed else {
        log_warn!(
            "bg_preview_overlay: {} is corrupt/unrewritable — '{}' stays chrome-only",
            source_str,
            stem
        );
        return None;
    };
    if !renamed_once.get() {
        log_warn!(
            "bg_preview_overlay: {} has no inner IFS matching '{}' — '{}' stays chrome-only",
            source_str,
            stem,
            stem
        );
        return None;
    }

    mod_paths::mkdir_p(out_dir);
    mod_paths::mkdir_p(ALIAS_HASH_DIR);
    if std::fs::write(&out_path, &renamed).is_err() {
        log_warn!(
            "bg_preview_overlay: couldn't write alias arc {} (from {})",
            out_path,
            source_str
        );
        return None;
    }
    hasher.commit();
    Some(true)
}

fn pump_tick() {
    let reschedule = {
        let mut state = lock_state();
        if !ENABLED.load(Ordering::Acquire) {
            state.pump_running = false;
            false
        } else {
            let mut any_loading = false;
            for side in 0..2usize {
                if poll_side(&mut state, side) {
                    any_loading = true;
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

/// Advance one side: poll every `Loading` resident, then (re)create the
/// focused layer once its package is ready. Returns whether anything is still
/// loading. Caller holds the lock and is on the render thread.
fn poll_side(state: &mut BgState, side: usize) -> bool {
    let BgState {
        categories,
        markers,
        sides,
        animate,
        ..
    } = &mut *state;
    let animate = *animate;
    let s = &mut sides[side];

    // 1. Advance loading residents; opportunistically recover Failed ones
    //    whose package the engine did eventually create (a timed-out load
    //    completing late). Collect timed-out asset ids to warn about AFTER
    //    the loop (can't borrow `s.warned` while iterating `s.resident`).
    let mut any_loading = false;
    let mut timed_out: Vec<u32> = Vec::new();
    for (key, res) in s.resident.iter_mut() {
        let polls = match res.phase {
            Phase::Loading { polls } => polls,
            Phase::Failed => {
                if bm2d_package::is_ready(&res.ticket) {
                    res.phase = Phase::Ready;
                }
                continue;
            }
            Phase::Ready => continue,
        };
        if bm2d_package::is_ready(&res.ticket) {
            res.phase = Phase::Ready;
        } else if polls + 1 >= LOAD_TIMEOUT_POLLS {
            res.phase = Phase::Failed;
            let asset_id = categories
                .get(key.0)
                .and_then(|c| c.asset_ids.get(key.1).copied())
                .unwrap_or(0);
            timed_out.push(asset_id);
        } else {
            res.phase = Phase::Loading { polls: polls + 1 };
            any_loading = true;
        }
    }
    for asset_id in timed_out {
        if s.warned.insert(asset_id) {
            log_warn!(
                "bg_preview_overlay: package never ready after {} polls (asset {}, side {}) — chrome only",
                LOAD_TIMEOUT_POLLS,
                asset_id,
                side
            );
        }
    }

    // 2. Ensure the focused value's layer exists once its package is ready.
    if let Some(fk) = s.focused {
        if s.layer_key != Some(fk)
            && matches!(s.resident.get(&fk).map(|r| &r.phase), Some(Phase::Ready))
        {
            // Drop any layer showing a stale value first.
            if let Some(layer) = s.layer.take() {
                destroy_layer_checked(layer);
            }
            s.layer_key = None;
            match create_layer(categories, markers, &s.resident, side, fk, animate) {
                Some(layer) => {
                    s.layer = Some(layer);
                    s.layer_key = Some(fk);
                }
                None => {
                    if let Some(res) = s.resident.get_mut(&fk) {
                        res.phase = Phase::Failed;
                    }
                    let asset_id = categories
                        .get(fk.0)
                        .and_then(|c| c.asset_ids.get(fk.1).copied())
                        .unwrap_or(0);
                    if s.warned.insert(asset_id) {
                        log_warn!(
                            "bg_preview_overlay: create/show failed for asset {} (side {}) — chrome only",
                            asset_id,
                            side
                        );
                    }
                }
            }
        }
    }

    any_loading
}

/// Look up the resident package for `key`, create its `bg_root` layer, place +
/// mask + play it (`animate=false` → `play(0.0)`: created paused, a static
/// first frame — R7's never-blank-chrome guarantee). Returns the layer on
/// success.
fn create_layer(
    categories: &[DiscoveredCategory],
    markers: &HashMap<String, MarkerRect>,
    resident: &HashMap<(usize, usize), Resident>,
    side: usize,
    key: (usize, usize),
    animate: bool,
) -> Option<bm2d_api::AfpLayer> {
    let cat = categories.get(key.0)?;
    let marker = *markers.get(cat.def.option_id)?;
    let res = resident.get(&key)?;
    let pkg = bm2d_package::lookup(&res.ticket)?;

    let layer = bm2d_api::create_layer_from_package(pkg.afpu_package_id(), TEMPLATE)?;

    // Screen placement: template-pixel marker offset by the side's origin.
    let (ox, oy) = CHROME_ORIGIN[side];
    let sx = ox + marker.x as f32;
    let sy = oy + marker.y as f32;
    let scale_x = marker.w as f32 / NATIVE_W;
    let scale_y = marker.h as f32 / NATIVE_H;
    let rate = if animate { 1.0 } else { 0.0 };

    let at = bm2d_api::layer_set_attribute(&layer, ATTR_DISPLAY_SETUP, ATTR_DISPLAY_SETUP);
    let pr = bm2d_api::layer_set_priority(&layer, PRIO_OVER_MODAL);
    let gr = bm2d_api::layer_set_group(&layer, GROUP_OVER_MODAL);
    let sc = bm2d_api::layer_set_scale(&layer, scale_x, scale_y);
    let po = bm2d_api::layer_set_position(&layer, sx, sy);
    let mk = bm2d_api::layer_set_mask(
        &layer,
        sx as i32,
        sy as i32,
        marker.w as i32,
        marker.h as i32,
    );

    // Geometry containment is non-negotiable: a layer whose scale/position/
    // mask didn't take would render as an unclipped 1280x720 clip at prio 300
    // — covering the whole options modal. That's an undefined visual state,
    // not a degradation; destroy and fall back to chrome instead. (These
    // setters can't realistically fail for a just-validated id, but the
    // failure path must still be well-defined.)
    if !(sc && po && mk) {
        log_warn!(
            "bg_preview_overlay: layer 0x{:08X} geometry setup failed (scale {} pos {} mask {}) — destroying, chrome only",
            layer.id(),
            sc,
            po,
            mk
        );
        destroy_layer_checked(layer);
        return None;
    }

    let pl = bm2d_api::layer_play(&layer, rate);
    log_info!(
        "bg_preview_overlay: side {} layer 0x{:08X} shown at ({},{}) {}x{} scale({:.3},{:.3}) [attr {} prio {} grp {} scale {} pos {} mask {} play({}) {}]",
        side,
        layer.id(),
        sx,
        sy,
        marker.w,
        marker.h,
        scale_x,
        scale_y,
        at,
        pr,
        gr,
        sc,
        po,
        mk,
        rate,
        pl
    );
    Some(layer)
}
