//! Series Filter Scroll — scrolls the song-select filter menu's VERSION panel so
//! the entries series_expansion adds beyond the stock grid stay reachable.
//!
//! ## Owned detours
//!
//! - `filter_panel_builder` (signature, required) — captures each VERSION
//!   (category 2) `FilterButton` as it is built. When the configured entry count
//!   is reached, activation is scheduled on the render thread; a per-frame job
//!   then follows the cursor, shows only the visible rows via `bm2d_api::set_mask`,
//!   and publishes a scroll Y offset.
//! - BM2D `set_position` (BM2D vtable +0x30 via `bm2d_api`, required) — subtracts
//!   the scroll offset from the Y of tracked VERSION layers. The offset is
//!   injected here because the grid layout engine rewrites every entry's base Y
//!   every frame.
//! - `FilterButton::~FilterButton` (signature `filterbutton_dtor`, optional) —
//!   closing the filter overlay frees the buttons without a scene change, so the
//!   first destructor deactivates the scroll and drops the cached pointers. When
//!   it is missing, only the scene-change callback (leaving SONG_SELECT) and the
//!   per-frame liveness check clear them.
//!
//! ## Rows and the viewport
//!
//! Every tracked entry has a row (0 = the first scrollable line) and a top in
//! px relative to row 0's top; rows need not be evenly spaced (the enhanced
//! layout's row breaks add height). Scrolled to row `s`, the offset is
//! `top(s) − top(0)` and an entry is shown while `row >= s` and its bottom
//! (`top − top(s) + row_height`) stays within `viewport` px. Legacy tracking
//! uses evenly spaced rows (`top = row × row_height`, viewport 9 rows).
//!
//! ## Contract with series_expansion
//!
//! [`init`] (run from `lib.rs` only when BM2D is available) installs the detours
//! and returns false if a required one fails. The service does nothing until the
//! consumer calls [`configure`] with the panel layout (row height, viewport, and
//! for legacy tracking the columns and total stock + custom entries);
//! series_expansion does so from its `enable` when [`is_available`]. Without a
//! configuration the builder detour passes through.
//!
//! See `docs/filter_scroll_research.md`.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::core::deferred_work::PendingPump;
use crate::core::signatures::SignatureStore;
use crate::services::{bm2d_api, scene_manager, widget_renderer};
use crate::{log_info, log_warn};

/// Row / viewport math (pure; host-tested by `validate_series_expansion.sh`).
mod math;

pub struct ScrollConfig {
    /// Height of one entry row (px).
    pub row_height: f64,
    /// Px below row 0's top in which rows are shown (see the module docs).
    pub viewport: f64,
    /// Template2 only: entries per build pass (activation count).
    pub total_entries: usize,
    /// Template2 only: entries per row (row = creation index / columns).
    pub columns: usize,
    pub tracking: Tracking,
}

/// How VERSION entry buttons are recognised in the CreateVisual detour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tracking {
    /// Legacy: every template-2 button, rows by creation order.
    Template2,
    /// Enhanced layout: exactly the buttons the builder passed to
    /// [`register_entry`] (any template; the GROUP tabs are never registered).
    Registered,
}

struct FilterEntry {
    this_ptr: *mut u8,
    layer_id: u32,
    row: usize,
    /// Px relative to row 0's top.
    top: f64,
}

unsafe impl Send for FilterEntry {}

struct ScrollState {
    config: Option<ScrollConfig>,
    entries: Vec<FilterEntry>,
    /// Top of every row (index = row), fixed at activation.
    row_tops: Vec<f64>,
    scroll_row: usize,
    active: bool,
    pump: PendingPump,
    scene_callback_id: Option<usize>,
}

unsafe impl Send for ScrollState {}

static STATE: Lazy<Mutex<ScrollState>> = Lazy::new(|| {
    Mutex::new(ScrollState {
        config: None,
        entries: Vec::new(),
        row_tops: Vec::new(),
        scroll_row: 0,
        active: false,
        pump: PendingPump::new(),
        scene_callback_id: None,
    })
});

/// Layer IDs of VERSION FilterButtons — checked by set_position hook.
static TRACKED_LAYERS: Lazy<Mutex<HashSet<u32>>> = Lazy::new(|| Mutex::new(HashSet::new()));

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Scroll Y offset in pixels as f64 bits — read by set_position hook.
static SCROLL_Y_OFFSET: AtomicU64 = AtomicU64::new(0);

// ── Panel builder hook (captures VERSION entries) ───────────────────

type PanelBuilderFn = unsafe extern "C" fn(*mut u8);
static mut PANEL_BUILDER_HOOK: Option<GenericDetour<PanelBuilderFn>> = None;

unsafe extern "C" fn panel_builder_hook(this: *mut u8) {
    if let Some(ref hook) = *std::ptr::addr_of!(PANEL_BUILDER_HOOK) {
        hook.call(this);
    }

    let registered = matches!(
        STATE
            .lock()
            .ok()
            .and_then(|s| s.config.as_ref().map(|c| c.tracking)),
        Some(Tracking::Registered)
    );
    if registered {
        let _ = std::panic::catch_unwind(|| on_registered_visual(this));
        return;
    }

    let category = (this.add(0xF0) as *const u32).read_unaligned();
    if category != 2 {
        return;
    }

    let bm2d_ptr = *(this.add(0x178) as *const *const u8);
    if bm2d_ptr.is_null() {
        return;
    }
    let layer_id = (bm2d_ptr.add(0x08) as *const u32).read_unaligned();
    if layer_id == 0 {
        return;
    }

    let mut state = STATE.lock().unwrap();
    let total_expected = match state.config.as_ref() {
        Some(c) => c.total_entries,
        None => return,
    };

    // Detect the start of a fresh build pass and reset stale state first. The
    // filter panel rebuilds its buttons every time the menu opens, but the game
    // frees the previous pass's buttons without notice — so any entries still
    // held here from a prior open point at freed memory. Without this reset the
    // hook would APPEND to them: the dangling pointers would be dereferenced by
    // the open path's focus cursor (crash), and `entry_count` would overshoot
    // `total_expected` so scroll never re-activates. A pass is "fresh" if the
    // previous one already reached its full count, or this exact layer is
    // already tracked (a rebuild re-entered before completing). Cleared inline
    // because we already hold the STATE lock (deactivate_scroll would deadlock).
    let mut tracked = TRACKED_LAYERS.lock().unwrap();
    let fresh_pass = state.entries.len() >= total_expected || tracked.contains(&layer_id);
    if fresh_pass {
        state.pump.cancel();
        SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
        tracked.clear();
        state.entries.clear();
        state.row_tops.clear();
        state.scroll_row = 0;
        state.active = false;
    }

    let (columns, row_height) = {
        let c = state.config.as_ref().unwrap();
        (c.columns.max(1), c.row_height)
    };
    let row = state.entries.len() / columns;

    tracked.insert(layer_id);
    drop(tracked);
    state.entries.push(FilterEntry {
        this_ptr: this,
        layer_id,
        row,
        top: row as f64 * row_height,
    });

    let entry_count = state.entries.len();

    if entry_count == total_expected {
        let generation = state.pump.request();
        drop(state);
        if let Some(generation) = generation {
            widget_renderer::run_on_render_thread(move || activate_scroll(generation));
        }
    }
}

/// Registered tracking: CreateVisual ran for `this`. Record (or refresh — a
/// visibility flip recreates the movie with a new layer id) its layer, and
/// activate once every registered button has one.
unsafe fn on_registered_visual(this: *mut u8) {
    let bm2d_ptr = *(this.add(0x178) as *const *const u8);
    if bm2d_ptr.is_null() {
        return;
    }
    let layer_id = (bm2d_ptr.add(0x08) as *const u32).read_unaligned();
    if layer_id == 0 {
        return;
    }
    let Ok(mut state) = STATE.lock() else { return };
    let Some(pos) = state.entries.iter().position(|e| e.this_ptr == this) else {
        return;
    };
    let old = state.entries[pos].layer_id;
    if old == layer_id {
        return;
    }
    state.entries[pos].layer_id = layer_id;
    {
        let Ok(mut tracked) = TRACKED_LAYERS.lock() else {
            return;
        };
        if old != 0 {
            tracked.remove(&old);
        }
        tracked.insert(layer_id);
    }
    if state.active {
        // Re-apply this entry's mask for the current window.
        let window = Window::of(&state);
        let entry = &state.entries[pos];
        apply_visibility(std::slice::from_ref(entry), window);
        return;
    }
    if state.entries.iter().all(|e| e.layer_id != 0) {
        let generation = state.pump.request();
        drop(state);
        if let Some(generation) = generation {
            widget_renderer::run_on_render_thread(move || activate_scroll(generation));
        }
    }
}

// ── Set-position hook (injects scroll Y offset) ─────────────────────

type SetPositionFn = unsafe extern "C" fn(*mut u8, *mut [i32; 2]);
static mut SET_POSITION_HOOK: Option<GenericDetour<SetPositionFn>> = None;

// ── FilterButton destructor hook (invalidates tracked pointers) ─────

/// `void FilterButton::~FilterButton(FilterButton* this)`.
type FilterButtonDtorFn = unsafe extern "C" fn(*mut u8);
static mut FILTERBUTTON_DTOR_HOOK: Option<GenericDetour<FilterButtonDtorFn>> = None;

/// The filter menu is an overlay inside SONG_SELECT, so closing it frees the
/// VERSION FilterButton objects without any scene change — leaving the raw
/// `this_ptr`s we cached in `STATE.entries` dangling, which the per-frame scroll
/// loop would then dereference (`+0x30`). This detour fires as each FilterButton
/// is destroyed; on the first one it fully deactivates the scroll (clearing the
/// tracked pointers and stopping the loop), closing the deref-after-free window.
/// The panel's buttons are all freed together on close, so clearing on any one
/// is sufficient and safe (a re-open re-captures them via `panel_builder_hook`).
unsafe extern "C" fn filterbutton_dtor_hook(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| {
        deactivate_scroll();
    });
    if let Some(ref hook) = *std::ptr::addr_of!(FILTERBUTTON_DTOR_HOOK) {
        hook.call(this);
    }
}

/// Called for every BM2D object every frame. For tracked VERSION layers,
/// subtracts the scroll offset from the Y coordinate before passing through.
unsafe extern "C" fn set_position_hook(this: *mut u8, pos: *mut [i32; 2]) {
    let hook = match &*std::ptr::addr_of!(SET_POSITION_HOOK) {
        Some(h) => h,
        None => return,
    };

    let y_offset = f64::from_bits(SCROLL_Y_OFFSET.load(Ordering::Acquire));
    if y_offset != 0.0 && !pos.is_null() {
        let layer_id = (this.add(0x08) as *const u32).read_unaligned();
        if TRACKED_LAYERS.lock().unwrap().contains(&layer_id) {
            (*pos)[1] -= y_offset as i32;
        }
    }

    hook.call(this, pos);
}

// ── Public API ──────────────────────────────────────────────────────

pub fn configure(config: ScrollConfig) {
    let mut state = STATE.lock().unwrap();
    log_info!(
        "SeriesFilterScroll: configured — {:?} tracking, {} px viewport, {} px rows",
        config.tracking,
        config.viewport,
        config.row_height
    );
    state.config = Some(config);
}

/// Registered tracking: a new VERSION build pass starts (drops every
/// previously registered button and deactivates scrolling).
pub fn begin_build() {
    deactivate_scroll();
}

/// Registered tracking: `button` is a VERSION cell on scroll row `row`
/// (0 = the first cell line below the GROUP tabs) whose top lies `top` px
/// below row 0's. Call from the builder, in order.
pub fn register_entry(button: *mut u8, row: usize, top: f64) {
    let Ok(mut state) = STATE.lock() else { return };
    if !matches!(
        state.config.as_ref().map(|c| c.tracking),
        Some(Tracking::Registered)
    ) {
        return;
    }
    state.entries.push(FilterEntry {
        this_ptr: button,
        layer_id: 0,
        row,
        top,
    });
}

pub fn init(signatures: &SignatureStore) -> bool {
    // Hook panel builder
    let builder_addr = match signatures.get_address("filter_panel_builder") {
        Some(a) => a,
        None => {
            log_warn!("SeriesFilterScroll: filter_panel_builder not resolved");
            return false;
        }
    };
    unsafe {
        let target: PanelBuilderFn = std::mem::transmute(builder_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(PANEL_BUILDER_HOOK),
            target,
            panel_builder_hook,
        ) {
            Ok(()) => {
                log_info!(
                    "SeriesFilterScroll: panel builder hooked @ {:p}",
                    builder_addr
                );
            }
            Err(e) => {
                log_warn!("SeriesFilterScroll: panel builder hook failed: {:?}", e);
                return false;
            }
        }
    }

    // Hook BM2D set_position (vtable offset 0x30)
    let set_pos_addr = match bm2d_api::get_vtable_method(0x30) {
        Some(a) => a,
        None => {
            log_warn!("SeriesFilterScroll: BM2D vtable[0x30] not resolved");
            return false;
        }
    };
    unsafe {
        let target: SetPositionFn = std::mem::transmute(set_pos_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(SET_POSITION_HOOK),
            target,
            set_position_hook,
        ) {
            Ok(()) => {
                log_info!(
                    "SeriesFilterScroll: set_position hooked @ {:p}",
                    set_pos_addr
                );
            }
            Err(e) => {
                log_warn!("SeriesFilterScroll: set_position hook failed: {:?}", e);
                return false;
            }
        }
    }

    // Hook FilterButton::~FilterButton so we drop our cached panel pointers the
    // instant the filter menu closes (an overlay teardown, with no scene change).
    // Best-effort: if it doesn't resolve, the scroll feature still works and the
    // scene-change deactivation below remains a partial backstop.
    match signatures.get_address("filterbutton_dtor") {
        Some(dtor_addr) => unsafe {
            let target: FilterButtonDtorFn = std::mem::transmute(dtor_addr);
            match crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!(FILTERBUTTON_DTOR_HOOK),
                target,
                filterbutton_dtor_hook,
            ) {
                Ok(()) => {
                    log_info!(
                        "SeriesFilterScroll: FilterButton dtor hooked @ {:p}",
                        dtor_addr
                    );
                }
                Err(e) => {
                    log_warn!(
                        "SeriesFilterScroll: FilterButton dtor hook failed: {:?} — stale pointers cleared only on scene change",
                        e
                    );
                }
            }
        },
        None => {
            log_warn!(
                "SeriesFilterScroll: filterbutton_dtor not resolved — stale pointers cleared only on scene change"
            );
        }
    }

    if scene_manager::is_available() {
        let cb_id = scene_manager::on_scene_change(Box::new(|_prev, next| {
            if next != crate::types::scenes::scene::SONG_SELECT {
                deactivate_scroll();
            }
        }));
        STATE.lock().unwrap().scene_callback_id = Some(cb_id);
    }

    INITIALIZED.store(true, Ordering::Release);
    log_info!("SeriesFilterScroll: initialized");
    true
}

pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

// ── Internal ────────────────────────────────────────────────────────

/// The current scroll window (valid while active).
#[derive(Clone, Copy)]
struct Window<'a> {
    row_tops: &'a [f64],
    scroll_row: usize,
    viewport: f64,
    row_height: f64,
}

impl<'a> Window<'a> {
    fn of(state: &'a ScrollState) -> Window<'a> {
        let (viewport, row_height) = state
            .config
            .as_ref()
            .map(|c| (c.viewport, c.row_height))
            .unwrap_or((0.0, 0.0));
        Window {
            row_tops: &state.row_tops,
            scroll_row: state.scroll_row,
            viewport,
            row_height,
        }
    }

    fn shows(&self, row: usize) -> bool {
        math::is_visible(
            row,
            self.scroll_row,
            self.row_tops,
            self.viewport,
            self.row_height,
        )
    }
}

/// Registered tracking, once per boot: compare the builder's registered tops
/// with the live layout (`Component+0x90`, grid-relative Y) — the model's
/// flow simulation must match the game's for scrolling to line up.
static LAYOUT_CHECKED: AtomicBool = AtomicBool::new(false);

fn check_layout_once(entries: &[FilterEntry]) {
    if LAYOUT_CHECKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let live_y = |e: &FilterEntry| unsafe {
        let p = e.this_ptr.add(0x90);
        crate::core::memory::is_readable(p, 8).then(|| *(p as *const f64))
    };
    let Some(origin) = entries.first().and_then(live_y) else {
        return;
    };
    let base = entries.first().map(|e| e.top).unwrap_or(0.0);
    let mismatch = entries.iter().find_map(|e| {
        let live = live_y(e)? - origin;
        ((live - (e.top - base)).abs() > 0.5).then_some((e.row, e.top, live))
    });
    match mismatch {
        None => log_info!(
            "SeriesFilterScroll: layout check — {} registered tops match the live layout",
            entries.len()
        ),
        Some((row, top, live)) => log_warn!(
            "SeriesFilterScroll: layout check — row {} registered at {} px, live at {} px (scrolling may misalign)",
            row,
            top,
            live
        ),
    }
}

fn activate_scroll(generation: u64) {
    let Ok(mut state) = STATE.lock() else { return };
    if !state.pump.begin(generation)
        || state.config.is_none()
        || state.entries.is_empty()
        || state.active
    {
        return;
    }

    let (viewport, row_height, tracking) = {
        let c = state.config.as_ref().unwrap();
        (c.viewport, c.row_height, c.tracking)
    };
    if tracking == Tracking::Registered {
        check_layout_once(&state.entries);
    }
    let tops = math::row_tops(state.entries.iter().map(|e| (e.row, e.top)), row_height);
    let fits = math::fits(&tops, viewport, row_height);

    log_info!(
        "SeriesFilterScroll: ACTIVATED — {} entries, {} rows, {} px viewport{}",
        state.entries.len(),
        tops.len(),
        viewport,
        if fits { " (all visible)" } else { "" }
    );

    state.row_tops = tops;
    state.scroll_row = 0;
    state.active = true;

    if fits {
        return;
    }

    apply_visibility(&state.entries, Window::of(&state));
    SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
    drop(state);
    schedule_update(generation);
}

fn apply_visibility(entries: &[FilterEntry], window: Window<'_>) {
    for entry in entries {
        if window.shows(entry.row) {
            bm2d_api::set_mask(entry.layer_id, -1000, -1000, 3000, 3000);
        } else {
            bm2d_api::set_mask(entry.layer_id, 0, 0, 0, 0);
        }
    }
}

fn deactivate_scroll() {
    let mut state = STATE.lock().unwrap();
    // Closing before the queued activation runs must also drop captured
    // pointers. A reopened menu owns a new generation, never the old pump.
    state.pump.cancel();
    SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
    TRACKED_LAYERS.lock().unwrap().clear();
    if state.active {
        for entry in &state.entries {
            bm2d_api::set_mask(entry.layer_id, -1000, -1000, 3000, 3000);
        }
        log_info!("SeriesFilterScroll: deactivated");
    }
    state.active = false;
    state.entries.clear();
    state.row_tops.clear();
    state.scroll_row = 0;
}

fn schedule_update(generation: u64) {
    let requested = {
        let Ok(mut state) = STATE.lock() else { return };
        state.active && state.pump.is_current(generation) && state.pump.request().is_some()
    };
    if requested {
        widget_renderer::run_on_render_thread(move || {
            if scroll_update_frame(generation) {
                schedule_update(generation);
            }
        });
    }
}

fn scroll_update_frame(generation: u64) -> bool {
    let Ok(mut state) = STATE.lock() else {
        return false;
    };
    if !state.pump.begin(generation) || !state.active || state.entries.is_empty() {
        return false;
    }

    // Check entries still exist
    let first_lid = state.entries[0].layer_id;
    let mut found = false;
    bm2d_api::for_each_active(|_idx, lid| {
        if lid == first_lid {
            found = true;
            return false;
        }
        true
    });
    if !found {
        state.pump.cancel();
        SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
        TRACKED_LAYERS.lock().unwrap().clear();
        state.active = false;
        state.entries.clear();
        state.row_tops.clear();
        state.scroll_row = 0;
        log_info!("SeriesFilterScroll: entries gone — deactivating");
        return false;
    }

    let (viewport, row_height) = match state.config.as_ref() {
        Some(c) => (c.viewport, c.row_height),
        None => return false,
    };

    // Find cursor row
    let mut cursor_row: Option<usize> = None;
    for entry in &state.entries {
        let sel = unsafe { *(entry.this_ptr.add(0x30) as *const u8) };
        if sel == 1 {
            cursor_row = Some(entry.row);
            break;
        }
    }

    let cursor_row = match cursor_row {
        Some(r) => r,
        None => return true,
    };

    let old_scroll = state.scroll_row;
    let new_scroll = math::follow(
        &state.row_tops,
        old_scroll,
        cursor_row,
        viewport,
        row_height,
    );

    if new_scroll != old_scroll {
        log_info!(
            "SeriesFilterScroll: cursor at row {}, scroll {} -> {}",
            cursor_row,
            old_scroll,
            new_scroll
        );
        state.scroll_row = new_scroll;
        let y_offset = math::offset(&state.row_tops, new_scroll);
        SCROLL_Y_OFFSET.store(y_offset.to_bits(), Ordering::Release);
        apply_visibility(&state.entries, Window::of(&state));
    }

    true
}
