//! Live 3D previews for the BACKGROUND DANCER / BACKGROUND STAGE rows
//! (design §4.6): while a player has one of the two rows focused in the
//! options modal at song select with a non-RANDOM value, that dancer (a
//! random routine, fixed frontal camera) or that stage (its `_play_loop`
//! animations under its own `.camanm` camera cuts, cropped to the box) is
//! rendered LIVE inside the row's preview box — by the side's own clones of
//! the engine's MODEL passes attached into RENDER_2D with the box as their
//! viewport ([`scene3d::viewport_pass`]), over a scene built and torn down
//! by the same [`SceneWindow`] machinery the gameplay window uses.
//!
//! Shape: the `custom_options` callbacks (`on_preview_request` fires every
//! focus tick for the focused row; `on_menu_open` / `on_menu_close` per
//! side) only RECORD into the per-side [`state::SlotState`] under a short
//! lock; ALL engine work — pass creation / rect / camera / enable, arc
//! loads, the scene phases — happens in [`on_frame`] from the mod's
//! `input_manager::on_frame` callback (mid-frame: the compositor's contract).
//! A value edit re-targets after [`layout::SETTLE_MS`]; focus loss, modal
//! close and leaving song select tear the preview down (the graph is still
//! enabled through scenes 26/27, so a teardown begun at scene-25 exit
//! completes while the gameplay window is loading). The passes are created
//! lazily once per side, kept attached (DISABLED when nothing is live —
//! zero cost) and detached only at [`shutdown`]. Everything fails open:
//! without the compositor the driver manages nothing; a per-preview failure
//! leaves the box showing its chrome, one WARN per class per boot.

pub mod badge;
pub mod camera;
pub mod layout;
pub mod scene;
pub mod state;

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::mods::mod_menu;
use crate::mods::webui_options::discovery::MarkerColor;
use crate::mods::webui_options::preview_gen;
use crate::services::scene3d::scene_graph;
use crate::services::scene3d::viewport_pass::{self, ClearSpec, PassSet, RtRect};
use crate::services::scene3d::viewport_pass_layout::{FILTER_BIT, PRIO_BASE};
use crate::services::{custom_options, widget_renderer};
use crate::types::scenes::scene as scene_ids;
use crate::{log_info, log_warn};

use super::catalog::Kind;
use super::lifecycle;
use super::movie_mode::SceneMask;
use super::options;
use super::scene_window::{self, SceneWindow};
use super::selection::{seed_from, Rng};
use super::session::ParseOptions;
use badge::Badge;
use layout::{box_rect, CanvasRect, BACKDROP_ARGB, FALLBACK_MARKER};
use scene::PreviewWindow;
use state::{Action, SlotState};

type Identity = (Kind, String);

/// Mod enabled (the callbacks no-op while false; close is never gated).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The `custom_options` subscriptions have no unsubscribe: register once.
static CALLBACKS_REGISTERED: AtomicBool = AtomicBool::new(false);
/// The per-frame driver has work (O(1) when clear).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// One-shot WARN classes (bits of [`Warn`]).
static WARNED: AtomicU8 = AtomicU8::new(0);
static EPOCH: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy)]
enum Warn {
    Compositor = 1,
    GraphDisabled = 2,
    Tables = 4,
    UnknownKey = 8,
    Marker = 16,
}

fn warn_once(class: Warn) -> bool {
    let bit = class as u8;
    WARNED.fetch_or(bit, Ordering::AcqRel) & bit == 0
}

/// Process-relative milliseconds (the settle clock).
fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

struct PreviewSlot {
    side: usize,
    state: SlotState<Identity>,
    modal_open: bool,
    /// Which of our rows is focused (RANDOM included) — the badge's box.
    focused_kind: Option<Kind>,
    live: Option<PreviewWindow>,
    /// Windows in teardown (driven to their end, then finished).
    retiring: Vec<PreviewWindow>,
    passes: Option<PassSet>,
    /// The canvas aspect the live preview's camera frames at.
    aspect: f32,
    /// The RANDOM badge (FR-9).
    badge: Badge,
}

/// The shipped box's aspect (170×150) until a preview measures its own.
const DEFAULT_ASPECT: f32 = 170.0 / 150.0;

impl PreviewSlot {
    const fn new(side: usize) -> PreviewSlot {
        PreviewSlot {
            side,
            state: SlotState::new(),
            modal_open: false,
            focused_kind: None,
            live: None,
            retiring: Vec::new(),
            passes: None,
            aspect: DEFAULT_ASPECT,
            badge: Badge::new(),
        }
    }

    fn is_idle(&self) -> bool {
        self.state.is_idle()
            && self.live.is_none()
            && self.retiring.is_empty()
            && !self.badge.is_shown()
            && !(self.modal_open && self.focused_kind.is_some())
    }
}

// Raw pointers live inside `PassSet` / `SceneWindow`'s sessions (mod-owned
// blocks, game-thread only by contract) — the same reasoning as
// `lifecycle::STATE`.
static SLOTS: Mutex<[PreviewSlot; 2]> = Mutex::new([PreviewSlot::new(0), PreviewSlot::new(1)]);

/// Mod enable: subscribe once and arm.
pub fn init() {
    if !CALLBACKS_REGISTERED.swap(true, Ordering::AcqRel) {
        custom_options::on_menu_open(on_menu_open);
        custom_options::on_menu_close(on_menu_close);
        custom_options::on_preview_request(on_preview_request);
    }
    ENABLED.store(true, Ordering::Release);
    // Only the derivation is knowable at enable: the engine constructs the
    // MODEL pass objects in its render-graph boot AFTER the mods are enabled,
    // so the live pass check runs at first use (`viewport_pass::availability`
    // treats null pass globals as "not yet", never as a refusal). The first
    // cabinet build probed the live passes here and latched "unavailable"
    // for the whole boot (2026-09-22).
    if !viewport_pass::derivation_present() && warn_once(Warn::Compositor) {
        log_warn!(
            "BackgroundDancers: 3D previews unavailable this boot (viewport sub-group not derived) -- the option rows keep working without a preview"
        );
    }
}

/// Mod disable: neutralise both sides' scenes (the frame callback that
/// would drive a teardown is gone — nodes are disabled and leaked with their
/// arcs, the gameplay rule) and detach the pass sets. The detached blocks
/// wait for `viewport_pass::reap()`, which no longer runs once the mod is
/// off — they stay allocated until a re-enable's frames reap them (a few
/// hundred bytes).
pub fn shutdown() {
    ENABLED.store(false, Ordering::Release);
    widget_renderer::run_on_render_thread(|| {
        if ENABLED.load(Ordering::Acquire) {
            return;
        }
        let Ok(mut slots) = SLOTS.lock() else { return };
        let mut leaked = false;
        for slot in slots.iter_mut() {
            for w in slot.live.take().into_iter().chain(slot.retiring.drain(..)) {
                leaked |= w.scene.neutralise();
            }
            if let Some(p) = slot.passes.take() {
                p.detach();
            }
            slot.badge.destroy();
            slot.state = SlotState::new();
            slot.modal_open = false;
            slot.focused_kind = None;
        }
        ACTIVE.store(false, Ordering::Release);
        if leaked {
            log_warn!(
                "BackgroundDancers: preview scene live at mod disable -- nodes disabled and LEAKED with their items and arcs"
            );
        }
    });
}

/// Scene callback (game thread): leaving song select ends every preview.
pub fn on_scene_change(prev: i32, next: i32) {
    if prev == scene_ids::SONG_SELECT && next != scene_ids::SONG_SELECT {
        if let Ok(mut slots) = SLOTS.lock() {
            for slot in slots.iter_mut() {
                slot.state.on_clear();
                slot.modal_open = false;
                slot.focused_kind = None;
            }
        }
        ACTIVE.store(true, Ordering::Release);
    }
}

fn on_menu_open(side: u8) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    if let Ok(mut slots) = SLOTS.lock() {
        slots[side as usize].modal_open = true;
    }
}

/// Modal closed. Not gated on `ENABLED` (a disable-while-open must still
/// clear the side's state).
fn on_menu_close(side: u8) {
    if side > 1 {
        return;
    }
    if let Ok(mut slots) = SLOTS.lock() {
        let s = &mut slots[side as usize];
        s.modal_open = false;
        s.focused_kind = None;
        s.state.on_clear();
    }
    ACTIVE.store(true, Ordering::Release);
}

/// The focused row asked for its preview (every focus tick).
fn on_preview_request(side: u8, option_id: &str) {
    if !ENABLED.load(Ordering::Acquire) || side > 1 {
        return;
    }
    let kind = options::kind_for_option(option_id);
    let wanted: Option<Identity> =
        kind.and_then(|k| options::choice_key(k, side).map(|key| (k, key)));
    let now = now_ms();
    if let Ok(mut slots) = SLOTS.lock() {
        let s = &mut slots[side as usize];
        if !s.modal_open {
            return;
        }
        s.focused_kind = kind;
        s.state.on_request(kind.is_some(), wanted, now);
    }
    ACTIVE.store(true, Ordering::Release);
}

/// Per-frame driver (game thread, from the mod's frame callback).
pub fn on_frame() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut slots) = SLOTS.lock() else { return };
    let now = now_ms();
    let menu_open = mod_menu::is_open();
    for slot in slots.iter_mut() {
        drive_slot(slot, now, menu_open);
    }
    if slots.iter().all(PreviewSlot::is_idle) {
        ACTIVE.store(false, Ordering::Release);
    }
}

fn drive_slot(slot: &mut PreviewSlot, now: u64, menu_open: bool) {
    let side = slot.side;
    // 1. Windows in teardown.
    let mut still = Vec::with_capacity(slot.retiring.len());
    for mut w in slot.retiring.drain(..) {
        if w.scene.drive_teardown("preview teardown") {
            w.scene.finish("preview ");
        } else {
            still.push(w);
        }
    }
    slot.retiring = still;

    // 2. What the state machine wants.
    match slot.state.poll(now) {
        Action::None => {}
        Action::Teardown => {
            if let Some(mut w) = slot.live.take() {
                log_info!(
                    "BackgroundDancers: preview P{} -- {} {:?} ends",
                    side + 1,
                    kind_tag(w.identity.0),
                    w.identity.1
                );
                if w.scene.begin_teardown("preview exit") {
                    slot.retiring.push(w);
                } else {
                    w.scene.finish_silent();
                }
            }
            slot.state.mark_torn_down();
            if let Some(p) = slot.passes.as_mut() {
                if p.is_enabled() {
                    p.set_enabled(false);
                }
            }
        }
        Action::Start(id) => {
            // One scene per side at a time: a retiring window still owns
            // the side's board slots and nodes.
            if slot.retiring.is_empty() {
                match start_preview(slot, &id) {
                    StartOutcome::Started(w) => {
                        slot.live = Some(w);
                        slot.state.mark_started(id);
                    }
                    // "Attempted": the state machine treats it as live so
                    // the same value is not retried every frame; a value
                    // change tears the (absent) window down and starts anew.
                    StartOutcome::Failed => slot.state.mark_started(id),
                    StartOutcome::Retry => {}
                }
            }
        }
    }

    // 3. Drive the live window.
    drive_live_window(slot, menu_open);

    // 4. The RANDOM badge: our row focused, nothing wanted (RANDOM), modal
    // open, overlay menu closed. Mutually exclusive with a live preview on
    // this side by construction (a wanted value hides it).
    let badge_on = slot.modal_open
        && slot.state.is_focused()
        && slot.state.wanted().is_none()
        && slot.live.is_none()
        && !menu_open;
    let rect = slot
        .focused_kind
        .map(|k| box_rect(side, marker_for(k)))
        .unwrap_or_else(|| box_rect(side, FALLBACK_MARKER));
    slot.badge.set_visible(side, badge_on, rect);
}

fn drive_live_window(slot: &mut PreviewSlot, menu_open: bool) {
    let side = slot.side;
    let Some(w) = slot.live.as_mut() else {
        if let Some(p) = slot.passes.as_mut() {
            if p.is_enabled() {
                p.set_enabled(false);
            }
        }
        return;
    };
    let has_built = w.scene.drive_assets(|pick, parsed, requested_at| {
        scene::make_session(side, pick, parsed, requested_at)
    });
    if has_built {
        if w.built_at.is_none() {
            w.built_at = Some(Instant::now());
            log_info!(
                "BackgroundDancers: preview P{} -- visible {} ms after request",
                side + 1,
                w.scene.since_request_ms()
            );
        }
        w.scene.park_engine_destroyed();
        let t = w.t();
        w.scene.publish(t, true, SceneMask::ALL);
        if let Some(passes) = slot.passes.as_mut() {
            let f = camera::frustum_for(w, t, slot.aspect);
            camera::apply(&f, passes);
        }
        w.scene.retry_textures();
        w.scene.attached_diagnostics();
    }
    let show = has_built && !menu_open;
    if let Some(p) = slot.passes.as_mut() {
        if p.is_enabled() != show {
            p.set_enabled(show);
        }
    }
}

enum StartOutcome {
    Started(PreviewWindow),
    /// A hard failure for this identity (logged once per class).
    Failed,
    /// Transient (render target not readable yet): try again next frame.
    Retry,
}

fn kind_tag(kind: Kind) -> &'static str {
    match kind {
        Kind::Dancer => "dancer",
        Kind::Stage => "stage",
    }
}

/// The row's template marker `(x, y, w, h)` relative to the preview panel —
/// read once per kind (a PNG decode) and cached; the shipped box when the
/// template is unreadable (one INFO).
fn marker_for(kind: Kind) -> (f32, f32, f32, f32) {
    static CACHE: Mutex<[Option<(f32, f32, f32, f32)>; 2]> = Mutex::new([None, None]);
    let idx = match kind {
        Kind::Dancer => 0,
        Kind::Stage => 1,
    };
    if let Ok(c) = CACHE.lock() {
        if let Some(m) = c[idx] {
            return m;
        }
    }
    let id = match kind {
        Kind::Dancer => options::OPT_DANCER,
        Kind::Stage => options::OPT_STAGE,
    };
    let marker = match preview_gen::marker_rect_for(id, MarkerColor::Green) {
        Some(m) => (m.x as f32, m.y as f32, m.w as f32, m.h as f32),
        None => {
            if warn_once(Warn::Marker) {
                log_info!(
                    "BackgroundDancers: preview -- {} template marker unreadable, using the shipped box {:?}",
                    id,
                    FALLBACK_MARKER
                );
            }
            FALLBACK_MARKER
        }
    };
    if let Ok(mut c) = CACHE.lock() {
        c[idx] = Some(marker);
    }
    marker
}

/// The row's preview box in render-target pixels + its canvas aspect, or
/// `None` while the display's target is not readable.
fn box_for(side: usize, kind: Kind) -> Option<(RtRect, f32)> {
    // Dims first: a not-yet-readable target retries next frame.
    let dims = viewport_pass::render_target_dims()?;
    let canvas: CanvasRect = box_rect(side, marker_for(kind));
    Some((
        RtRect::from_canvas(canvas.x, canvas.y, canvas.w, canvas.h, dims),
        canvas.aspect(),
    ))
}

/// Game thread (`on_frame`): pick, passes, arcs, window.
fn start_preview(slot: &mut PreviewSlot, id: &Identity) -> StartOutcome {
    let side = slot.side;
    let (kind, key) = (id.0, id.1.as_str());
    match viewport_pass::availability() {
        viewport_pass::Availability::Available => {}
        viewport_pass::Availability::NotYet => return StartOutcome::Retry,
        viewport_pass::Availability::Unavailable => {
            if warn_once(Warn::Compositor) {
                log_warn!(
                    "BackgroundDancers: 3D previews unavailable this boot (compositor) -- the option rows keep working without a preview"
                );
            }
            return StartOutcome::Failed;
        }
    }
    // The graph must be enabled for our nodes to be collected at all.
    if !scene_graph::graph_stats().map_or(false, |g| g.enabled) {
        if warn_once(Warn::GraphDisabled) {
            log_warn!(
                "BackgroundDancers: preview P{} -- the scene graph is disabled at song select; previews cannot render",
                side + 1
            );
        }
        return StartOutcome::Failed;
    }
    let Some(tables) = lifecycle::tables_snapshot() else {
        if warn_once(Warn::Tables) {
            log_warn!("BackgroundDancers: preview -- candidate tables unavailable");
        }
        return StartOutcome::Failed;
    };
    // The box + passes first (a transient miss retries without side effects).
    let Some((rect, aspect)) = box_for(side, kind) else {
        return StartOutcome::Retry;
    };
    if slot.passes.is_none() {
        let created = viewport_pass::create(
            FILTER_BIT[side],
            rect,
            ClearSpec {
                depth: true,
                color: Some(BACKDROP_ARGB),
            },
            PRIO_BASE[side],
        );
        match created {
            Some(mut p) => {
                p.set_enabled(false);
                slot.passes = Some(p);
            }
            None => {
                // `create` logged the reason.
                return StartOutcome::Failed;
            }
        }
    } else if let Some(p) = slot.passes.as_mut() {
        p.set_rect(rect);
    }
    slot.aspect = aspect;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    let seed = seed_from(
        nanos ^ (side as u64).rotate_left(23),
        scene_ids::SONG_SELECT as u32,
    );
    let mut rng = Rng::new(seed);
    let Some(mut pick) = scene::build_pick(kind, key, &tables, &mut rng, side) else {
        if warn_once(Warn::UnknownKey) {
            log_warn!(
                "BackgroundDancers: preview P{} -- {} {:?} is not in the candidate tables (catalog drift) -- no preview",
                side + 1,
                kind_tag(kind),
                key
            );
        }
        return StartOutcome::Failed;
    };
    pick.seed = seed;
    log_info!(
        "BackgroundDancers: preview P{} -- {} (box rt=({},{},{},{}) aspect {:.3})",
        side + 1,
        pick.summary(),
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        aspect
    );
    let loaded = scene_window::load_arcs(&pick, &ParseOptions::PREVIEW);
    let window = SceneWindow::start(
        format!("BackgroundDancers: preview P{}", side + 1),
        "this preview",
        pick,
        loaded,
        ParseOptions::PREVIEW,
    );
    StartOutcome::Started(PreviewWindow {
        identity: id.clone(),
        side,
        scene: window,
        built_at: None,
        seed,
    })
}
