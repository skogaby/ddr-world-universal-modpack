//! Options Scroll — per-tab scroll driver for overflowing options menus.
//!
//! When a tab's mod-row count exceeds the native viewport capacity (7 rows),
//! this service clips rows outside a sliding window by zeroing their
//! `+0xB8` active flag. The game's grid layout engine (`FUN_18004a720`)
//! skips rows with `+0xB8 == 0`, packing the remaining visible rows
//! contiguously into the viewport top.
//!
//! Two hook points feed the mask:
//!
//!   1. `filter_hook` (existing) calls [`apply_mask`] at the tail of its
//!      Phase 3 row-visibility pass (only on the Mods tab), so the current
//!      scroll window takes effect on every tab switch.
//!   2. `grid_positional_step_fn` detour wraps the native positional
//!      focus-advance function (`FUN_18004a030`). BEFORE calling the
//!      original we pre-advance our scroll window so the target row
//!      already has `+0xB8 == 1`. The original's `+0xB8 != 0` check then
//!      finds the newly-unmasked row and returns it as the next focus.
//!      After the original returns, we re-apply the full mask.
//!
//! The positional step function is called by the GridPanel's own up/down
//! navigation lambdas (stored at `container+0x178`/`+0x198`) when the
//! mode flag at `*(lambda+0x08)+0xC0` is 0 — which is the case for the
//! options row list. Its signature is `fn(container, direction) -> i32`.
//!
//! Scroll state is kept per player side. State is session-scoped.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::services::custom_options::{self, PageTag};
use crate::{log_info, log_warn};

/// Currently active tab index, updated by the filter_hook detour on every
/// tab switch. The scroll trampoline only activates when this is 6 (Mods).
/// Atomic because the filter runs on the render thread and we read it from
/// the same thread in the step trampoline.
static ACTIVE_TAB: AtomicI32 = AtomicI32::new(0);

/// Native viewport capacity in rows.
const VIEWPORT_ROWS: usize = 7;

/// Layout-container field offsets.
mod container_offset {
    pub const VEC_BEGIN: usize = 0x68;
    pub const VEC_END: usize = 0x70;
    pub const FOCUS_INDEX: usize = 0x168;
}

/// Row field offset for the "active" byte.
const ROW_ACTIVE_BYTE_OFFSET: usize = 0xB8;

/// Per-side scroll state.
#[derive(Copy, Clone, Default)]
struct ScrollState {
    window_top: usize,
}

static STATE: Lazy<Mutex<HashMap<u8, ScrollState>>> = Lazy::new(|| Mutex::new(HashMap::new()));

static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ── Detour type ─────────────────────────────────────────────────────────

/// Positional focus-advance: `fn(container: *mut u8, direction: i32) -> i32`.
type GridPositionalStepFn = unsafe extern "C" fn(*mut u8, i32) -> i32;

static mut POSITIONAL_STEP_HOOK: Option<GenericDetour<GridPositionalStepFn>> = None;

pub fn init(signatures: &SignatureStore) -> bool {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return true;
    }
    log_info!("options_scroll: init");

    let step_addr = match signatures.get_address("grid_positional_step_fn") {
        Some(a) => a,
        None => {
            log_warn!(
                "options_scroll: grid_positional_step_fn not resolved — cursor-driven scroll disabled"
            );
            return true;
        }
    };

    unsafe {
        let target: GridPositionalStepFn = std::mem::transmute(step_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(POSITIONAL_STEP_HOOK),
            target,
            positional_step_trampoline,
        ) {
            Ok(()) => {
                log_info!(
                    "options_scroll: positional-step detour installed @ {:p}",
                    step_addr
                );
            }
            Err(e) => {
                log_warn!(
                    "options_scroll: positional-step detour install failed: {:?}",
                    e
                );
                return true;
            }
        }
    }

    true
}

pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}

/// Called by the filter_hook detour to notify us of the current active tab.
/// Only tab 6 (Mods) has scroll behavior; all other tabs pass through.
pub fn set_active_tab(tab: i32) {
    ACTIVE_TAB.store(tab, Ordering::Relaxed);
}

// ── Detour trampoline ───────────────────────────────────────────────────

unsafe extern "C" fn positional_step_trampoline(container: *mut u8, direction: i32) -> i32 {
    let result = std::panic::catch_unwind(|| positional_step_body(container, direction));
    result.unwrap_or_else(|_| {
        // On panic, call original as fallback.
        unsafe {
            match (*std::ptr::addr_of!(POSITIONAL_STEP_HOOK)).as_ref() {
                Some(d) => d.call(container, direction),
                None => 0,
            }
        }
    })
}

/// Trampoline body:
/// 1. Check if this container has our mod rows (via `side_for_container`).
///    If not, pass through to the original unchanged.
/// 2. Read the current focus index and find it in our mod-row list.
/// 3. Pre-advance the scroll window so the target row has `+0xB8=1`.
/// 4. Call the original (which now finds the freshly-unmasked row).
/// 5. Re-apply the full mask based on where focus actually landed.
fn positional_step_body(container: *mut u8, direction: i32) -> i32 {
    let detour = unsafe {
        match (*std::ptr::addr_of!(POSITIONAL_STEP_HOOK)).as_ref() {
            Some(d) => d,
            None => return 0,
        }
    };

    // Only activate scroll logic on the Mods tab (Page6).
    if ACTIVE_TAB.load(Ordering::Relaxed) != 6 {
        return unsafe { detour.call(container, direction) };
    }

    let side = custom_options::side_for_container(container);
    if side.is_none() {
        return unsafe { detour.call(container, direction) };
    }
    let side = side.unwrap();

    let rows = custom_options::row_handles_for_tab(side, PageTag::Page6);
    let total = rows.len();
    if total <= VIEWPORT_ROWS {
        return unsafe { detour.call(container, direction) };
    }

    // Read current focus and find it in our mod-row list.
    let focus_idx = unsafe { read_focus_index(container) };
    let vec_len = unsafe { read_row_count(container) };

    let focused_ptr = if focus_idx >= 0 && (focus_idx as usize) < vec_len {
        unsafe { read_row_ptr_at(container, focus_idx as usize) }
    } else {
        std::ptr::null_mut()
    };

    let current_mod_pos = rows.iter().position(|r| r.row_ptr == focused_ptr);

    if let Some(pos) = current_mod_pos {
        // Focus is on a mod row. Compute the target directly from our
        // ordered list — don't rely on the native positional step which
        // uses stale position coordinates on freshly-unmasked rows.
        let target_pos = predict_target(&rows, pos, direction);
        if target_pos == pos {
            // At boundary (first row going up, or last row going down).
            // Let native handle it — it may return focus to the tab strip.
            let result = unsafe { detour.call(container, direction) };
            // Check if focus left our rows (went to tab strip).
            let result_ptr = if result >= 0 && (result as usize) < vec_len {
                unsafe { read_row_ptr_at(container, result as usize) }
            } else {
                std::ptr::null_mut()
            };
            let still_on_mod = rows.iter().any(|r| r.row_ptr == result_ptr);
            custom_options::filter_hook::set_tab_highlight(!still_on_mod);
            return result;
        }

        // Shift scroll window to include the target row.
        let mut state_map = STATE.lock().unwrap();
        let state = state_map.entry(side).or_default();
        adjust_window_for_target(state, &rows, target_pos);
        let window_top = state.window_top;
        drop(state_map);
        apply_mask_with_window(&rows, window_top, total);

        // Find the target row's vector index and return it directly.
        let target_row_ptr = rows[target_pos].row_ptr;
        let target_vec_idx = find_vector_index(container, target_row_ptr, vec_len);
        if let Some(idx) = target_vec_idx {
            custom_options::filter_hook::set_tab_highlight(false);
            return idx as i32;
        }

        // Fallback: couldn't find target in vector (shouldn't happen).
        let result = unsafe { detour.call(container, direction) };
        custom_options::filter_hook::set_tab_highlight(false);
        return result;
    }

    // Focus is NOT on a mod row — entering the mod-row list from the
    // tab strip (native wrap). Determine which end to land on based on
    // direction: DOWN enters at the first SELECTABLE mod row, UP at the
    // last (skipping header rows sitting at either end).
    let target_pos = if direction > 0 {
        rows.iter().position(|r| r.selectable).unwrap_or(0)
    } else {
        rows.iter().rposition(|r| r.selectable).unwrap_or(total - 1)
    };

    // Set window around the target.
    let mut state_map = STATE.lock().unwrap();
    let state = state_map.entry(side).or_default();
    adjust_window_for_target(state, &rows, target_pos);
    let window_top = state.window_top;
    drop(state_map);
    apply_mask_with_window(&rows, window_top, total);

    // Return the target's vector index directly.
    let target_row_ptr = rows[target_pos].row_ptr;
    let target_vec_idx = find_vector_index(container, target_row_ptr, vec_len);
    if let Some(idx) = target_vec_idx {
        custom_options::filter_hook::set_tab_highlight(false);
        return idx as i32;
    }

    // Fallback: let native handle it with all rows unmasked.
    for row in &rows {
        unsafe { set_row_active(row.row_ptr, 1) };
    }
    let result = unsafe { detour.call(container, direction) };
    custom_options::filter_hook::set_tab_highlight(false);
    result
}

/// Predict where focus will land after a step in `direction`, skipping
/// unselectable rows (headers — the cursor must never land on them; the
/// native scan honors their `+0x28` predicate, but this driver replaces
/// that scan on the Mods tab, so the skip lives here). Clamps at
/// boundaries — matches native wrap_mode=0 behavior: returning
/// `current_pos` signals the boundary (first selectable row going up hands
/// focus back to the tab strip; last selectable row going down stays put).
/// A run of unselectable rows extending to either end is treated as the
/// boundary itself.
fn predict_target(rows: &[custom_options::RowHandle], current_pos: usize, direction: i32) -> usize {
    if direction == 0 {
        return current_pos;
    }
    let total = rows.len() as i32;
    let mut candidate = current_pos as i32 + direction.signum();
    while (0..total).contains(&candidate) {
        let pos = candidate as usize;
        if rows[pos].selectable {
            return pos;
        }
        candidate += direction.signum();
    }
    current_pos
}

/// Shift `state.window_top` so `target_pos` lies within the visible window.
/// Handles both incremental scrolling (one row at a time) and large jumps
/// from wrap-around (last→first, first→last).
///
/// Upward shifts anchor on the contiguous run of unselectable header rows
/// sitting directly above the target (if any), so a group's heading scrolls
/// back into view together with its first row. Without this the cursor —
/// which can never land on a header — could never re-establish a window
/// containing a header directly above the topmost selectable row: the very
/// first list row being a header meant it showed on the form's initial
/// window (top 0) but was unreachable forever after scrolling away.
fn adjust_window_for_target(
    state: &mut ScrollState,
    rows: &[custom_options::RowHandle],
    target_pos: usize,
) {
    let total = rows.len();
    if total <= VIEWPORT_ROWS {
        state.window_top = 0;
        return;
    }
    let max_top = total - VIEWPORT_ROWS;
    // Headers immediately above the target scroll in with it.
    let mut anchor = target_pos.min(total - 1);
    while anchor > 0 && !rows[anchor - 1].selectable {
        anchor -= 1;
    }
    if anchor < state.window_top {
        // Target (or a heading directly above it) is above the current
        // window — snap the window top to the anchor.
        state.window_top = anchor.min(max_top);
    }
    if target_pos >= state.window_top + VIEWPORT_ROWS {
        // Target is below the current window (or a header run longer than
        // the viewport pushed it out) — shift the window so the target is
        // the last visible row.
        let want = target_pos + 1 - VIEWPORT_ROWS;
        state.window_top = want.min(max_top);
    }
    // else: target is already within window, no shift needed.
}

// ── Public entry point (called from filter_hook tail) ───────────────────

/// Re-apply the scroll mask for `container`. Called from the filter_hook
/// detour's Phase 3 tail (Mods tab only) to clip rows outside the scroll
/// window after the native visibility pass has set +0xB8=1 on all Page6
/// rows.
pub fn apply_mask(container: *mut u8) {
    if !is_available() || container.is_null() {
        return;
    }
    let Some(side) = custom_options::side_for_container(container) else {
        return;
    };
    let rows = custom_options::row_handles_for_tab(side, PageTag::Page6);
    let total = rows.len();
    if total == 0 {
        return;
    }

    let window_top = {
        let mut state_map = STATE.lock().unwrap();
        let state = state_map.entry(side).or_default();
        if total <= VIEWPORT_ROWS {
            state.window_top = 0;
        } else {
            let max_top = total - VIEWPORT_ROWS;
            if state.window_top > max_top {
                state.window_top = max_top;
            }
        }
        state.window_top
    };

    apply_mask_with_window(&rows, window_top, total);

    // Force-hide rows excluded by ShowWhen predicates. The native filter
    // sets +0xB8=1 on all Page6 rows before we run; excluded rows aren't
    // in our `rows` list so apply_mask_with_window doesn't touch them.
    custom_options::rows::hide_show_when_excluded(side);
}

/// Reset a side's scroll window to the top. Called when that side's row
/// registry is torn down (form build and form close both route through
/// `rows::clear_side`): the native form always reopens focused on the
/// topmost row, so a stale mid-list window from the previous open would
/// otherwise clip the top of the list — including a leading header row —
/// until the first cursor step self-corrected.
pub fn reset_window(side: u8) {
    let Ok(mut state_map) = STATE.lock() else {
        return;
    };
    state_map.insert(side, ScrollState::default());
}

/// Re-apply the scroll mask for a given player side using the current
/// window position. Called when ShowWhen visibility changes to immediately
/// update the viewport without waiting for cursor movement.
pub fn reapply_mask_for_side(side: u8) {
    if !is_available() {
        return;
    }
    let rows = custom_options::row_handles_for_tab(side, PageTag::Page6);
    let total = rows.len();
    if total == 0 {
        return;
    }

    let window_top = {
        let mut state_map = STATE.lock().unwrap();
        let state = state_map.entry(side).or_default();
        if total <= VIEWPORT_ROWS {
            state.window_top = 0;
        } else {
            let max_top = total - VIEWPORT_ROWS;
            if state.window_top > max_top {
                state.window_top = max_top;
            }
        }
        state.window_top
    };

    apply_mask_with_window(&rows, window_top, total);
    custom_options::rows::hide_show_when_excluded(side);
}

/// Write +0xB8 active bytes: 1 for rows in [window_top, window_top+VIEWPORT),
/// 0 for rows outside the window.
fn apply_mask_with_window(rows: &[custom_options::RowHandle], window_top: usize, total: usize) {
    if total <= VIEWPORT_ROWS {
        for row in rows {
            unsafe { set_row_active(row.row_ptr, 1) };
        }
        return;
    }
    let window_end = window_top + VIEWPORT_ROWS;
    for (i, row) in rows.iter().enumerate() {
        let visible = i >= window_top && i < window_end;
        unsafe { set_row_active(row.row_ptr, if visible { 1 } else { 0 }) };
    }
}

/// Find the vector index of `target_ptr` in the container's row vector.
/// Linear scan — the vector is small (48 entries max for options).
fn find_vector_index(container: *mut u8, target_ptr: *mut u8, vec_len: usize) -> Option<usize> {
    for i in 0..vec_len {
        let ptr = unsafe { read_row_ptr_at(container, i) };
        if ptr == target_ptr {
            return Some(i);
        }
    }
    None
}

// ── Low-level helpers ───────────────────────────────────────────────────

unsafe fn read_focus_index(container: *mut u8) -> i32 {
    std::ptr::read_unaligned(container.add(container_offset::FOCUS_INDEX) as *const i32)
}

unsafe fn read_row_count(container: *mut u8) -> usize {
    let begin =
        std::ptr::read_unaligned(container.add(container_offset::VEC_BEGIN) as *const *const u8);
    let end =
        std::ptr::read_unaligned(container.add(container_offset::VEC_END) as *const *const u8);
    if begin.is_null() || end.is_null() || end < begin {
        return 0;
    }
    end.offset_from(begin) as usize / 8
}

unsafe fn read_row_ptr_at(container: *mut u8, index: usize) -> *mut u8 {
    let begin =
        std::ptr::read_unaligned(container.add(container_offset::VEC_BEGIN) as *const *mut *mut u8);
    if begin.is_null() {
        return std::ptr::null_mut();
    }
    *begin.add(index)
}

unsafe fn set_row_active(row: *mut u8, value: u8) {
    std::ptr::write(row.add(ROW_ACTIVE_BYTE_OFFSET), value);
}
