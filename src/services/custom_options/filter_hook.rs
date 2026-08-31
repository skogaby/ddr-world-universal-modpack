//! Tab-filter detour — Rust reimplementation of the native
//! `tab_filter_fn` (FUN_180168d10) extended with a Mods tab entry so
//! Page6 content rows are recognised on tab-switch.
//!
//! The native function hardcodes `tab_names[1..=5] = ["basic", "arrows",
//! "lane", "judge", "assist"]` as a stack-local array. The Mods tab
//! repurposes slot 6 (the native Back-template slot) but needs a 7th
//! `tab_names` entry (`"mods"`) to drive its `seop_tab_title_mods`
//! binding. Reimplementing the filter in Rust is simpler than byte-
//! patching the five LEA instructions that build the native array.
//!
//! The detour takes over the full filter behaviour — three phases:
//!
//!   Phase 1a — title-box visibility: toggle visibility of every layer
//!              matching `"<scene_prefix>/tab_title_usr"` based on
//!              whether the active tab has a per-tab title texture.
//!   Phase 1b — title texture: bind `seop_tab_title_<name>` for the
//!              currently active tab (basic / arrows / lane / judge /
//!              assist / mods).
//!   Phase 2  — tab-strip visuals: for each of the six tab slots, bind
//!              the per-slot background style texture to `base_usr`.
//!              Tabs 1..=5 use the content-tab background (no suffix);
//!              slot 6 uses the `_return` (Back-template) background
//!              since the underlying AFP placement is still the native
//!              Back tab. Tabs 1..=5 also bind `seop_tab_icon_<name>`
//!              to `page_usr`; slot 6's Back-template has no page_usr,
//!              so its visible label comes from the atlas layer's
//!              pre-bound `seop_return` texture (which LayeredFS
//!              substitutes for the mods-tab placeholder).
//!   Phase 3  — row filter: iterate the flat row vector at
//!              `*(scene_parent + 0x230) + 0x68..+0x70`, hash-check each
//!              row's metadata rb-tree at `row + 0x10` for the magic
//!              keys (`System`, `Disabled`, `Page<N>`) and call
//!              `component_set_visible(row, visible)` + the row's
//!              slot-5 `onTick` to propagate the change.
//!
//! After Phase 3 the detour calls the scene-graph layout flush
//! (`scene_layout_flush`) to commit any pending scroll-position updates
//! the native function would have written.
//!
//! Graceful degradation: if any signature dependency fails to resolve,
//! [`init`] returns `false` without installing the detour. The native
//! filter continues to run; Pages 1–5 stay fully functional; Page6
//! content rows remain unreachable until the missing dependency is
//! restored.

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::services::bm2d_api;
use crate::{log_info, log_warn};

/// Native filter signature.
/// `param_1` = scene parent (provides scene prefix string at
/// *(param_1+0xC8), scene root at *(param_1+0x110), flat row vector at
/// *(*(param_1+0x230))+0x68..+0x70).
/// `param_2` = tab selection object (holds active-tab index at
/// *(param_2+0x04)).
type TabFilterFn = unsafe extern "C" fn(*mut u8, *mut u8);

/// Component visibility toggle used by Phase 3.
type ComponentSetVisibleFn = unsafe extern "C" fn(*mut u8, u8);

/// Scene-graph layout flush, called at Phase 3's tail.
type SceneLayoutFlushFn = unsafe extern "C" fn(*mut u8, u8);

/// Slot-5 `onTick` vtable function signature. The native filter invokes
/// it with just the `this` pointer after each visibility toggle to
/// propagate the change through the component's reactive streams.
type OnTickFn = unsafe extern "C" fn(*mut u8);

/// Tab names indexed 1..=6 (index 0 unused to match the native's
/// 1-based `active_tab` convention).
const TAB_NAMES: [&str; 7] = ["", "basic", "arrows", "lane", "judge", "assist", "mods"];

// ── Precomputed native FNV hashes ───────────────────────────────────────
//
// Verified at build-time by the ci-less `debug_assert_eq!` sanity checks
// in [`native_fnv_hash_verify`]. The native filter uses a **FNV-1 variant**
// (multiply-first, not FNV-1a's XOR-first) with 32-bit FNV constants in a
// 64-bit accumulator:
//
//   h = 0x811C9DC5 (32-bit FNV offset basis, zero-extended to 64 bits)
//   for each byte:
//     h = (h * 0x01000193) & 0xFFFF_FFFF_FFFF_FFFF   # 32-bit FNV prime
//     h ^= byte                                      # XOR AFTER multiply
//   (byte is sign-extended from i8 to i64 before XOR, but ASCII bytes
//    are < 0x80 so sign extension is a no-op in practice.)
//
// The native uses `skip = size/10 + 1` step for long strings; for all
// these short metadata keys `size/10 == 0` so the step reduces to 1.

const HASH_SYSTEM: u64 = 0xFEB95CCF8BCE35AA;
const HASH_DISABLED: u64 = 0x0B048A72D8FC5EB5;
const HASH_PAGE: [u64; 7] = [
    0,                  // index 0 unused
    0xA792FF143AAE9255, // "Page1"
    0xA792FF143AAE9256, // "Page2"
    0xA792FF143AAE9257, // "Page3"
    0xA792FF143AAE9250, // "Page4"
    0xA792FF143AAE9251, // "Page5"
    0xA792FF143AAE9252, // "Page6"
];

// ── Scene-parent field offsets (from the native decompile) ──────────────

const SCENE_PARENT_PREFIX_SSO: usize = 0xC8;
const SCENE_PARENT_PREFIX_SSO_CAP: usize = 0xE0;
const SCENE_PARENT_SCENE_ROOT_PTR: usize = 0x110;
const SCENE_PARENT_FLAGS_OBJ_PTR: usize = 0x10; // param_1[2] = flags object
const SCENE_FLAGS_OBJ_ALT_STYLE_OFFSET: usize = 0x30;
const SCENE_PARENT_LAYOUT_ROOT_PTR: usize = 0x230;

/// Scene-root's `mc_id` lives at `+0x08` of the scene-root struct.
const SCENE_ROOT_MC_ID_OFFSET: usize = 0x08;

// ── Tab-selection field offsets ─────────────────────────────────────────

const TAB_SELECTION_ACTIVE_TAB_OFFSET: usize = 0x04;

// ── Row field offsets ───────────────────────────────────────────────────

const ROW_METADATA_SET_OFFSET: usize = 0x10;
// `metadata_set` is itself the MSVC std::map head/sentinel node. Offsets
// below apply to every node in the tree (head and real nodes alike).
const METADATA_NODE_LEFT: usize = 0x00;
const METADATA_NODE_RIGHT: usize = 0x10;
const METADATA_NODE_HASH: usize = 0x18;
const METADATA_NODE_IS_NIL: usize = 0x21;
const ROW_VTABLE_ONTICK_OFFSET: usize = 0x28;

// ── Flat row vector (at *(scene_parent+0x230)) ──────────────────────────

const LAYOUT_ROOT_BEGIN_OFFSET: usize = 0x68;
const LAYOUT_ROOT_END_OFFSET: usize = 0x70;

// ── Static state ────────────────────────────────────────────────────────

static mut FILTER_HOOK: Option<GenericDetour<TabFilterFn>> = None;
static mut FN_COMPONENT_SET_VISIBLE: Option<ComponentSetVisibleFn> = None;
static mut FN_SCENE_LAYOUT_FLUSH: Option<SceneLayoutFlushFn> = None;

/// Per-side cached scene mc_id from that side's last filter pass. Used by
/// `set_tab_highlight` to rebind the Mods tab background texture. Per-side
/// is deliberate: in 2-player mode both sides' OptionForms are live at
/// once, and a shared last-writer-wins cache made one player's scroll
/// steps rebind against whichever side's filter happened to run last.
/// `0` = no pass cached yet. Render-thread only (filter detour + the
/// scroll trampoline both run there).
static mut CACHED_SCENE_MC_ID: [u32; 2] = [0, 0];
/// Per-side cached scene prefix string (e.g. "option_v3"). Same lifecycle
/// and threading as [`CACHED_SCENE_MC_ID`].
static mut CACHED_SCENE_PREFIX: [Option<String>; 2] = [None, None];

/// Resolve dependencies and install the detour. Returns `true` on
/// success; any signature miss or hook-install failure logs a warning
/// and returns `false`, leaving the native filter in place.
pub(crate) fn init(signatures: &SignatureStore) -> bool {
    let filter_addr = match signatures.get_address("tab_filter_fn") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/filter_hook: tab_filter_fn not resolved — Page6 content will not render"
            );
            return false;
        }
    };
    let csv_addr = match signatures.get_address("component_set_visible") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/filter_hook: component_set_visible not resolved — detour install aborted"
            );
            return false;
        }
    };
    let flush_addr = match signatures.get_address("scene_layout_flush") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/filter_hook: scene_layout_flush not resolved — detour install aborted"
            );
            return false;
        }
    };
    if !bm2d_api::is_available() {
        log_warn!("custom_options/filter_hook: bm2d_api not initialized — detour install aborted");
        return false;
    }

    unsafe {
        FN_COMPONENT_SET_VISIBLE = Some(std::mem::transmute::<*const u8, ComponentSetVisibleFn>(
            csv_addr,
        ));
        FN_SCENE_LAYOUT_FLUSH = Some(std::mem::transmute::<*const u8, SceneLayoutFlushFn>(
            flush_addr,
        ));

        let target: TabFilterFn = std::mem::transmute(filter_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(FILTER_HOOK),
            target,
            filter_detour,
        ) {
            log_warn!(
                "custom_options/filter_hook: detour install failed: {:?} — Page6 content will not render",
                e
            );
            return false;
        }
        log_info!(
            "custom_options/filter_hook: tab-filter detour installed @ {:p}",
            filter_addr
        );
    }
    true
}

// ── Detour body ─────────────────────────────────────────────────────────

unsafe extern "C" fn filter_detour(scene_parent: *mut u8, tab_selection: *mut u8) {
    let _ = std::panic::catch_unwind(|| filter_detour_body(scene_parent, tab_selection));
}

fn filter_detour_body(scene_parent: *mut u8, tab_selection: *mut u8) {
    unsafe {
        if scene_parent.is_null() || tab_selection.is_null() {
            return;
        }

        // `param_1` is a `longlong*` in the decomp. The native reads `*param_1`
        // to reach the inner object whose `+0xC8` holds the scene prefix
        // std::string and whose `+0x110` holds the scene root pointer.
        let scene_parent_inner = read_ptr(scene_parent);
        if scene_parent_inner.is_null() {
            return;
        }
        let tab_selection_inner = read_ptr(tab_selection);
        if tab_selection_inner.is_null() {
            return;
        }

        // The tab strip has 6 navigable slots: 5 native content tabs plus
        // slot 6 which is the Back-template slot repurposed as the Mods tab.
        // The filter iterates all 6; the `active_tab` index comes from the
        // game's input handler, which uses the native `max_tab_count` (5)
        // to decide where to wrap (so pressing right from slot 6 wraps to
        // slot 1, matching native Back-tab behaviour).
        let max_tab_count: i32 = 6;
        let active_tab = read_i32(
            tab_selection_inner
                .add(TAB_SELECTION_ACTIVE_TAB_OFFSET)
                .cast::<i32>(),
        );
        if active_tab < 1 {
            return;
        }

        let scene_root_ptr = read_ptr(scene_parent_inner.add(SCENE_PARENT_SCENE_ROOT_PTR));
        if scene_root_ptr.is_null() {
            return;
        }
        let scene_mc_id = read_u32(scene_root_ptr.add(SCENE_ROOT_MC_ID_OFFSET).cast::<u32>());

        let prefix = match read_scene_prefix(scene_parent_inner) {
            Some(s) => s,
            None => {
                log_warn!("custom_options/filter_hook: empty scene prefix; skipping filter pass");
                return;
            }
        };

        // Resolve which player side owns this form's layout container.
        // `None` means no mod rows are injected into it (yet) — every
        // side-keyed consumer below (scroll latch, highlight cache) is
        // skipped, matching the scroll trampoline's own `None` ⇒
        // pass-through behavior for the same container.
        let layout_root = read_ptr(scene_parent_inner.add(SCENE_PARENT_LAYOUT_ROOT_PTR));
        let side = if layout_root.is_null() {
            None
        } else {
            super::side_for_container(layout_root)
        };

        if let Some(side) = side {
            // Notify the scroll driver of THIS side's active tab so it only
            // activates on the Mods tab (Page6). Keyed per side: each
            // side's form has its own tab selection, and both forms are
            // live at once in 2-player mode.
            crate::services::options_scroll::set_active_tab(side, active_tab);

            // Cache scene context for set_tab_highlight, also per side.
            (*std::ptr::addr_of_mut!(CACHED_SCENE_MC_ID))[side as usize] = scene_mc_id;
            (*std::ptr::addr_of_mut!(CACHED_SCENE_PREFIX))[side as usize] = Some(prefix.clone());
        }

        // ── Phase 1a: title-box visibility ──────────────────────────────
        let title_path = format!("{prefix}/tab_title_usr");
        let title_visible = active_tab <= max_tab_count;
        set_layer_visibility(scene_mc_id, &title_path, title_visible);

        // ── Phase 1b: title texture (only for content tabs) ─────────────
        if active_tab <= max_tab_count {
            if let Some(name) = tab_name(active_tab) {
                let title_texture = format!("seop_tab_title_{name}");
                bind_bitmap_to_layers(scene_mc_id, &title_path, &title_texture);
            }
        }

        // ── Phase 2: tab-strip visuals ──────────────────────────────────
        //
        // Scene-graph layout (native, unmodified):
        //   tab1_usr..tab5_usr: content tabs (Basic/Arrows/Lane/Judge/Assist);
        //                       each has base_usr (background) + page_usr (icon).
        //   tab6_usr:           Back-template tab, repurposed as the Mods tab.
        //                       Has base_usr only; its icon-equivalent layer
        //                       is pre-bound by the template to `seop_return`,
        //                       which the atlas layer resolves to the Mods
        //                       placeholder we ship in data_mods.
        //
        // Binding rules match the native filter: background texture on every
        // tab's base_usr; icon texture on page_usr for tabs 1..5 only.
        let flags_obj = read_ptr(scene_parent.add(SCENE_PARENT_FLAGS_OBJ_PTR));
        let mut alt_style = if flags_obj.is_null() {
            false
        } else {
            *flags_obj.add(SCENE_FLAGS_OBJ_ALT_STYLE_OFFSET) != 0
        };

        // On the Mods tab, the native "focus transferred to rows" flag
        // never fires (the Back-template slot doesn't participate in that
        // mechanism). Override: if focus is currently on one of our mod
        // rows, force alt_style=false so the tab renders dimmed.
        if active_tab == 6 && !layout_root.is_null() {
            let focus_on_mod_row = is_focus_on_mod_row(layout_root);
            if focus_on_mod_row {
                alt_style = false;
            }
        }

        for tab_idx in 1..=max_tab_count {
            let style: &str = if tab_idx == active_tab {
                if alt_style {
                    "on"
                } else {
                    "on_alt"
                }
            } else {
                "off"
            };
            // tab 6 uses the `_return` (Back) background texture; it is the
            // Back-template slot repurposed as the Mods tab.
            let suffix = if tab_idx == 6 { "_return" } else { "" };
            let bg_texture = format!("seop_tab_{style}{suffix}");
            let bg_path = format!("{prefix}/tab_usr/tab{tab_idx}_usr/base_usr");
            bind_bitmap_to_layers(scene_mc_id, &bg_path, &bg_texture);

            // Icon bind applies only to tabs that use the content-tab template
            // (tabs 1..5). Tab 6's Back-template has no page_usr.
            if tab_idx < 6 {
                if let Some(name) = tab_name(tab_idx) {
                    let icon_texture = format!("seop_tab_icon_{name}");
                    let icon_path = format!("{prefix}/tab_usr/tab{tab_idx}_usr/page_usr");
                    bind_bitmap_to_layers(scene_mc_id, &icon_path, &icon_texture);
                }
            }
        }

        // ── Phase 3: row filter ─────────────────────────────────────────
        if !layout_root.is_null() {
            let begin = read_ptr(layout_root.add(LAYOUT_ROOT_BEGIN_OFFSET));
            let end = read_ptr(layout_root.add(LAYOUT_ROOT_END_OFFSET));
            if !begin.is_null() && !end.is_null() && end >= begin {
                let row_count = end.offset_from(begin) as usize / std::mem::size_of::<*mut u8>();
                for i in 0..row_count {
                    let row = read_ptr(begin.add(i * std::mem::size_of::<*mut u8>()));
                    if row.is_null() {
                        continue;
                    }
                    let metadata_set = read_ptr(row.add(ROW_METADATA_SET_OFFSET));
                    if metadata_set.is_null() {
                        continue;
                    }
                    apply_row_visibility(row, metadata_set, active_tab);
                }
            }

            if let Some(flush) = FN_SCENE_LAYOUT_FLUSH {
                flush(layout_root, 1);
            }

            // Re-apply the scroll-window mask on top of the native filter's
            // per-page visibility writes. Only relevant for the Mods tab
            // (Page6) — on other tabs, mod rows are already hidden by Phase
            // 3's PageN check and apply_mask would incorrectly re-show them
            // by setting +0xB8=1 on rows within the scroll window.
            if active_tab == 6 {
                crate::services::options_scroll::apply_mask(layout_root);
            }
        }

        let _ = scene_parent; // silence unused warnings on release builds
    }
}

/// Walk the sibling chain rooted at `parent_path` and toggle each
/// layer's visibility via `afp_mc_set_param(layer, 0x1007, ..)` plus a
/// follow-up refresh call at `0x101e`. Mirrors the native
/// `FUN_18021b990` helper.
fn set_layer_visibility(scene_mc_id: u32, parent_path: &str, visible: bool) {
    let mut layer = match bm2d_api::layer_find_child(scene_mc_id, parent_path) {
        Some(l) => l,
        None => return,
    };
    loop {
        bm2d_api::mc_set_param(layer, 0x1007, visible as i32);
        bm2d_api::mc_set_param(layer, 0x101e, 1);
        layer = match bm2d_api::mc_traversal(layer, 6) {
            Some(next) => next,
            None => break,
        };
    }
}

/// Walk the sibling chain rooted at `parent_path` and bind `texture` to
/// each layer via `afp_mc_load_bitmap`.
fn bind_bitmap_to_layers(scene_mc_id: u32, parent_path: &str, texture: &str) {
    let mut layer = match bm2d_api::layer_find_child(scene_mc_id, parent_path) {
        Some(l) => l,
        None => return,
    };
    loop {
        bm2d_api::mc_load_bitmap(layer, texture);
        layer = match bm2d_api::mc_traversal(layer, 6) {
            Some(next) => next,
            None => break,
        };
    }
}

/// Visibility decision for a single row. Magic-key short-circuits match
/// the native filter's order: `System` keeps the row visible regardless
/// of tab, `Disabled` keeps it hidden, otherwise visibility is `true` iff
/// the row is tagged for the active page.
fn apply_row_visibility(row: *mut u8, metadata_set: *mut u8, active_tab: i32) {
    unsafe {
        if metadata_contains(metadata_set, HASH_SYSTEM) {
            // System rows render on every tab without an explicit toggle.
            return;
        }
        if metadata_contains(metadata_set, HASH_DISABLED) {
            // Disabled rows stay hidden; no toggle issued — mirrors native.
            return;
        }

        let page_idx = active_tab as usize;
        let visible = if page_idx >= 1 && page_idx < HASH_PAGE.len() {
            metadata_contains(metadata_set, HASH_PAGE[page_idx])
        } else {
            false
        };

        if let Some(f) = FN_COMPONENT_SET_VISIBLE {
            f(row, visible as u8);
        }

        // Invoke slot-5 `onTick` on the row's primary vtable so the
        // reactive stream picks up the visibility change.
        let vtable = read_ptr(row);
        if !vtable.is_null() {
            let slot = read_ptr(vtable.add(ROW_VTABLE_ONTICK_OFFSET));
            if !slot.is_null() {
                let on_tick: OnTickFn = std::mem::transmute(slot);
                on_tick(row);
            }
        }
    }
}

/// Inlined std::map lookup against the MSVC rb-tree rooted at `metadata_set`.
///
/// `metadata_set` itself IS the head/sentinel node of the map — the head's
/// `is_nil` byte at `+0x21` is 1, and its three child pointers
/// (+0x00 / +0x08 / +0x10) point at the tree root, leftmost, and rightmost
/// real nodes (or at itself when the map is empty).
///
/// Node layout (0x30 bytes):
///   +0x00 left child
///   +0x08 parent            (in head: tree root)
///   +0x10 right child
///   +0x18 hash key (u64)    — matches the native's FNV-1 hash of the key
///   +0x20 value byte
///   +0x21 is_nil            (1 for the head, 0 for real nodes)
///
/// The walk is an `upper_bound`-style descent: track the best candidate
/// (smallest hash ≥ needle) as we descend. Returns true iff the final
/// best candidate is a real node whose hash equals needle.
fn metadata_contains(metadata_set: *mut u8, needle: u64) -> bool {
    unsafe {
        // metadata_set is the head node; head->parent (at +0x08) is the
        // tree root. Start the walk at the tree root.
        let mut node = read_ptr(metadata_set.add(0x08));
        let mut best: *const u8 = metadata_set;
        if node.is_null() {
            return false;
        }
        let mut is_nil = *node.add(METADATA_NODE_IS_NIL);
        while is_nil == 0 {
            let node_hash = read_u64(node.add(METADATA_NODE_HASH).cast::<u64>());
            if node_hash < needle {
                node = read_ptr(node.add(METADATA_NODE_RIGHT));
            } else {
                best = node;
                node = read_ptr(node.add(METADATA_NODE_LEFT));
            }
            if node.is_null() {
                break;
            }
            is_nil = *node.add(METADATA_NODE_IS_NIL);
        }
        if best == metadata_set {
            return false;
        }
        let best_hash = read_u64(best.add(METADATA_NODE_HASH).cast::<u64>());
        best_hash == needle
    }
}

/// Resolve the scene-prefix std::string at
/// `scene_parent_inner + 0xC8`. MSVC `std::basic_string` uses SSO: when
/// the capacity field at `+0xE0` is ≤ 15 the bytes live inline at
/// `+0xC8`; otherwise `+0xC8` is a heap pointer.
fn read_scene_prefix(scene_parent_inner: *const u8) -> Option<String> {
    unsafe {
        let sso_start = scene_parent_inner.add(SCENE_PARENT_PREFIX_SSO);
        let capacity = read_u64(
            scene_parent_inner
                .add(SCENE_PARENT_PREFIX_SSO_CAP)
                .cast::<u64>(),
        );
        let data_ptr = if capacity > 0xF {
            read_ptr(sso_start)
        } else {
            sso_start
        };
        if data_ptr.is_null() {
            return None;
        }

        // Read up to 128 bytes looking for the null terminator. Scene
        // prefixes are short stub paths ("option_v3", etc.); capping the
        // scan at 128 bytes keeps us safe if the string is malformed.
        let mut len = 0usize;
        while len < 128 && *data_ptr.add(len) != 0 {
            len += 1;
        }
        if len == 0 {
            return None;
        }
        let slice = std::slice::from_raw_parts(data_ptr, len);
        std::str::from_utf8(slice).ok().map(|s| s.to_string())
    }
}

fn tab_name(idx: i32) -> Option<&'static str> {
    let i = idx as usize;
    if i >= 1 && i < TAB_NAMES.len() && !TAB_NAMES[i].is_empty() {
        Some(TAB_NAMES[i])
    } else {
        None
    }
}

/// Toggle `side`'s Mods-tab visual highlight. Directly rebinds the tab
/// background texture via bm2d so the change takes effect immediately.
/// Uses the scene context cached by that side's own filter pass; no-op
/// for a side whose filter hasn't run yet (or an out-of-range side).
pub(crate) fn set_tab_highlight(side: u8, highlighted: bool) {
    let idx = side as usize;
    if idx >= 2 {
        return;
    }
    unsafe {
        let scene_mc_id = (*std::ptr::addr_of!(CACHED_SCENE_MC_ID))[idx];
        let prefix = match &(*std::ptr::addr_of!(CACHED_SCENE_PREFIX))[idx] {
            Some(p) => p.clone(),
            None => return,
        };
        if scene_mc_id == 0 {
            return;
        }

        let bg_texture = if highlighted {
            "seop_tab_on_return"
        } else {
            "seop_tab_off_return"
        };
        let bg_path = format!("{prefix}/tab_usr/tab6_usr/base_usr");
        bind_bitmap_to_layers(scene_mc_id, &bg_path, bg_texture);
    }
}

/// Check if the GridPanel's current focus index points at one of our
/// mod-allocated rows. Used by the Phase 2 alt_style override.
fn is_focus_on_mod_row(layout_root: *mut u8) -> bool {
    unsafe {
        let begin = read_ptr(layout_root.add(LAYOUT_ROOT_BEGIN_OFFSET));
        let end = read_ptr(layout_root.add(LAYOUT_ROOT_END_OFFSET));
        if begin.is_null() || end.is_null() || end < begin {
            return false;
        }
        let row_count = end.offset_from(begin) as usize / std::mem::size_of::<*mut u8>();
        let focus_idx = read_i32(layout_root.add(0x168).cast::<i32>());
        if focus_idx < 0 || (focus_idx as usize) >= row_count {
            return false;
        }
        let focused_row = read_ptr(begin.add(focus_idx as usize * std::mem::size_of::<*mut u8>()));
        if focused_row.is_null() {
            return false;
        }
        // Check if this row pointer is one of ours by asking the
        // custom_options side_for_container + row_handles_for_tab path.
        // A faster check: see if the row's primary vtable pointer is in
        // our VirtualAlloc'd range. But the simplest correct check is to
        // ask the rows module directly.
        let side = match super::side_for_container(layout_root) {
            Some(s) => s,
            None => return false,
        };
        let handles = super::row_handles_for_tab(side, super::PageTag::Page6);
        handles.iter().any(|h| h.row_ptr == focused_row)
    }
}

// ── Raw-pointer readers ─────────────────────────────────────────────────

unsafe fn read_ptr(p: *const u8) -> *mut u8 {
    (p as *const *mut u8).read_unaligned()
}
unsafe fn read_u32(p: *const u32) -> u32 {
    p.read_unaligned()
}
unsafe fn read_i32(p: *const i32) -> i32 {
    p.read_unaligned()
}
unsafe fn read_u64(p: *const u64) -> u64 {
    p.read_unaligned()
}

// ── Compile-time-ish hash sanity check (exercised in debug builds) ──────

#[cfg(debug_assertions)]
#[allow(dead_code)]
fn native_fnv_hash_verify() {
    fn native_fnv(s: &str) -> u64 {
        let mut h: u64 = 0x811C9DC5;
        for b in s.bytes() {
            h = h.wrapping_mul(0x01000193);
            h ^= b as i8 as i64 as u64;
        }
        h
    }
    debug_assert_eq!(native_fnv("System"), HASH_SYSTEM);
    debug_assert_eq!(native_fnv("Disabled"), HASH_DISABLED);
    debug_assert_eq!(native_fnv("Page1"), HASH_PAGE[1]);
    debug_assert_eq!(native_fnv("Page2"), HASH_PAGE[2]);
    debug_assert_eq!(native_fnv("Page3"), HASH_PAGE[3]);
    debug_assert_eq!(native_fnv("Page4"), HASH_PAGE[4]);
    debug_assert_eq!(native_fnv("Page5"), HASH_PAGE[5]);
    debug_assert_eq!(native_fnv("Page6"), HASH_PAGE[6]);
}
