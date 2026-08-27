//! Series Filter Scroll — Drives scroll behavior for the VERSION filter panel.
//!
//! Hooks the filter category panel builder to capture VERSION FilterButton entries.
//! Hides entries outside the visible window via set_mask. Hooks the BM2D
//! set_position vtable method to inject a scroll Y offset — this works around
//! the grid layout engine overwriting base_y 774 times/frame.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::services::{bm2d_api, scene_manager, widget_renderer};
use crate::{log_info, log_warn};

pub struct ScrollConfig {
    pub row_height: f64,
    pub visible_rows: usize,
    pub total_entries: usize,
    pub columns: usize,
}

struct FilterEntry {
    this_ptr: *mut u8,
    layer_id: u32,
    row: usize,
}

unsafe impl Send for FilterEntry {}

struct ScrollState {
    config: Option<ScrollConfig>,
    entries: Vec<FilterEntry>,
    scroll_row: usize,
    active: bool,
    scene_callback_id: Option<usize>,
}

unsafe impl Send for ScrollState {}

static STATE: Lazy<Mutex<ScrollState>> = Lazy::new(|| {
    Mutex::new(ScrollState {
        config: None,
        entries: Vec::new(),
        scroll_row: 0,
        active: false,
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
        SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
        tracked.clear();
        state.entries.clear();
        state.scroll_row = 0;
        state.active = false;
    }

    let columns = state.config.as_ref().unwrap().columns;
    let row = state.entries.len() / columns;

    tracked.insert(layer_id);
    drop(tracked);
    state.entries.push(FilterEntry {
        this_ptr: this,
        layer_id,
        row,
    });

    let entry_count = state.entries.len();

    if entry_count == total_expected {
        drop(state);
        widget_renderer::run_on_render_thread(|| {
            activate_scroll();
        });
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
        "SeriesFilterScroll: configured — {} entries, {} visible rows",
        config.total_entries,
        config.visible_rows
    );
    state.config = Some(config);
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

fn activate_scroll() {
    let mut state = STATE.lock().unwrap();
    if state.config.is_none() || state.entries.is_empty() || state.active {
        return;
    }

    let visible_rows = state.config.as_ref().unwrap().visible_rows;
    let columns = state.config.as_ref().unwrap().columns;
    let total_rows = state.entries.len().div_ceil(columns);

    log_info!(
        "SeriesFilterScroll: ACTIVATED — {} entries, {} rows, {} visible",
        state.entries.len(),
        total_rows,
        visible_rows
    );

    state.scroll_row = 0;
    state.active = true;

    if total_rows <= visible_rows {
        return;
    }

    apply_visibility(&state.entries, 0, visible_rows);
    SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
    drop(state);
    schedule_update();
}

fn apply_visibility(entries: &[FilterEntry], scroll_row: usize, visible_rows: usize) {
    for entry in entries {
        if entry.row >= scroll_row && entry.row < scroll_row + visible_rows {
            bm2d_api::set_mask(entry.layer_id, -1000, -1000, 3000, 3000);
        } else {
            bm2d_api::set_mask(entry.layer_id, 0, 0, 0, 0);
        }
    }
}

fn deactivate_scroll() {
    let mut state = STATE.lock().unwrap();
    if state.active {
        SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
        TRACKED_LAYERS.lock().unwrap().clear();
        for entry in &state.entries {
            bm2d_api::set_mask(entry.layer_id, -1000, -1000, 3000, 3000);
        }
        state.active = false;
        state.entries.clear();
        state.scroll_row = 0;
        log_info!("SeriesFilterScroll: deactivated");
    }
}

fn schedule_update() {
    widget_renderer::run_on_render_thread(|| {
        if scroll_update_frame() {
            schedule_update();
        }
    });
}

fn scroll_update_frame() -> bool {
    let mut state = STATE.lock().unwrap();
    if !state.active || state.entries.is_empty() {
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
        SCROLL_Y_OFFSET.store(0u64, Ordering::Release);
        TRACKED_LAYERS.lock().unwrap().clear();
        state.active = false;
        state.entries.clear();
        state.scroll_row = 0;
        log_info!("SeriesFilterScroll: entries gone — deactivating");
        return false;
    }

    let (visible_rows, row_height) = match state.config.as_ref() {
        Some(c) => (c.visible_rows, c.row_height),
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
    let mut new_scroll = old_scroll;

    if cursor_row < new_scroll {
        new_scroll = cursor_row;
    } else if cursor_row >= new_scroll + visible_rows {
        new_scroll = cursor_row - visible_rows + 1;
    }

    if new_scroll != old_scroll {
        log_info!(
            "SeriesFilterScroll: cursor at row {}, scroll {} -> {}",
            cursor_row,
            old_scroll,
            new_scroll
        );
        state.scroll_row = new_scroll;
        let y_offset = new_scroll as f64 * row_height;
        SCROLL_Y_OFFSET.store(y_offset.to_bits(), Ordering::Release);
        apply_visibility(&state.entries, new_scroll, visible_rows);
    }

    true
}
