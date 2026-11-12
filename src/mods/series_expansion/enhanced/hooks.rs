//! Detours for the enhanced VERSION layout: the VERSION builder and the
//! GROUP-tab press body. Both are owned by series_expansion (nothing else
//! hooks these functions) and pass straight through to the original unless
//! [`set_active`]`(true)` — i.e. only while enhanced mode is enabled.
//!
//! Builder contract (every supported build): `builder(capture, factory)`;
//! `capture` +0x00 selection state, +0x08 category, +0x10 FilterPanel,
//! +0x40 FilterPanel (the stock row-break code reads the item grid from
//! `[capture+0x40]+0x228`); `factory` is a by-value
//! `std::function<FilterButton*(int)>` the builder owns — impl pointer at
//! +0x18, vtable slot 1 invoke `(impl, idx)`, slot 3 delete
//! `(impl, impl != factory)`. The replacement destroys it exactly once,
//! outside the fallible section.
//!
//! The replacement replays the plan's slots in order: tabs through the stock
//! tab factory (`g` = the tab's press id), cells through the builder's
//! factory (selection index = cell index), and row breaks as spacers.
//!
//! Row-break spacer: a bare `sequence::Component` — `operator new(0xC0)` +
//! `Component::Component` (base vtables: every virtual a no-op, not
//! focusable, deleting destructor → CRT `free`), size `(grid width,
//! thickness)`, pushed onto the item grid's `children` (`grid+0x68`, grown by
//! the game's own grow-by-one) with `parent` (+0x60) = the grid. Unlike the
//! stock `FilterHeader` (whose activation hook forces its height to 1 px) a
//! Component never touches its own size, so any thickness — including 0, a
//! pure line break — holds. Full width ends the current line in the grid's
//! flow layout; the grid's clear destroys it with the other children.
//!
//! Press contract: `press(captures, on)`; `captures` +0x00 state, +0x08
//! category (i32), +0x18 `g` (i32; stock 0 CLASSIC, 1 WHITE, 2 GOLD; config
//! tabs 3+), +0x20 FilterPanel. The replacement never falls through to the
//! stock body while active (the stock body reads the stock group table), and
//! never hands it a config tab's `g` (≥ 3) while inactive.

use super::model::{EnhancedPlan, Slot, CUSTOM_TAB_ID_BASE};
use crate::core::memory;
use crate::core::signatures::SeriesEnhancedSites;
use crate::services::series_filter_scroll;
use crate::{log_info, log_warn};
use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

type BuilderFn = unsafe extern "C" fn(*mut u8, *mut u8);
type PressFn = unsafe extern "C" fn(*mut u8, u8);
type TabFactoryFn = unsafe extern "C" fn(*mut u8, i32) -> *mut u8;
type SetTemplateFn = unsafe extern "C" fn(*mut u8, i32);
type AssignFn = unsafe extern "C" fn(*mut u8, *const u8, usize) -> *mut u8;
type ClearCategoryFn = unsafe extern "C" fn(*mut u8, i32);
type SetOneFn = unsafe extern "C" fn(*mut u8, i32, i32, u8);
type NotifyFn = unsafe extern "C" fn(*mut u8);
type FactoryInvokeFn = unsafe extern "C" fn(*mut u8, i32) -> *mut u8;
type FactoryDeleteFn = unsafe extern "C" fn(*mut u8, u8);
type OperatorNewFn = unsafe extern "C" fn(usize) -> *mut u8;
type ComponentCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
type VectorGrowFn = unsafe extern "C" fn(*mut u8);

/// `std::function` impl pointer offset (MSVC tr1: 0x18 bytes of inline storage).
const FUNCTION_IMPL: usize = 0x18;
/// `FilterButton+0xC8` — label key `std::string` (texture = `sefi_` + key).
const BUTTON_LABEL: usize = 0xC8;
/// Builder capture `+0x40` — the FilterPanel; `+0x228` there is the item grid.
const CAPTURE_PANEL: usize = 0x40;
const PANEL_ITEM_GRID: usize = 0x228;
/// `sequence::Component` layout.
const COMPONENT_SIZE: usize = 0xC0;
const COMPONENT_PARENT: usize = 0x60;
const COMPONENT_CHILDREN: usize = 0x68;
const COMPONENT_WIDTH: usize = 0xA0;
const COMPONENT_HEIGHT: usize = 0xA8;
/// `sequence::GridPanel` object size (readability gate).
const GRID_PANEL_SIZE: usize = 0x240;

/// One builder step with its label as bytes (no per-open allocation).
enum Step {
    Tab {
        press_id: i32,
        template: i32,
        label: Vec<u8>,
    },
    Cell {
        selection: i32,
        template: i32,
        label: Vec<u8>,
        row: usize,
        top: f64,
    },
    Break {
        thickness: f64,
    },
}

/// Everything the detours need, fixed at enable.
pub struct Runtime {
    steps: Vec<Step>,
    /// Cell indices per press id.
    members: Vec<Vec<i32>>,
    tab_factory: TabFactoryFn,
    set_template: SetTemplateFn,
    assign: AssignFn,
    clear_category: ClearCategoryFn,
    set_one: SetOneFn,
    notify: NotifyFn,
    operator_new: OperatorNewFn,
    component_ctor: ComponentCtorFn,
    vector_grow: VectorGrowFn,
}

impl Runtime {
    /// # Safety
    /// `sites` must come from `SignatureStore::series_enhanced_sites` (verified
    /// function entries in the loaded module).
    pub unsafe fn new(sites: &SeriesEnhancedSites, plan: &EnhancedPlan) -> Runtime {
        let placements = plan.scroll_layout().cells;
        let steps = plan
            .slots
            .iter()
            .filter_map(|&slot| match slot {
                Slot::Tab(i) => plan.tabs.get(i).map(|t| Step::Tab {
                    press_id: t.press_id,
                    template: plan.group_columns as i32,
                    label: t.label_key.clone().into_bytes(),
                }),
                Slot::Cell(i) => placements.get(i).map(|p| Step::Cell {
                    selection: i as i32,
                    template: plan.columns as i32,
                    label: plan.label_key(i).into_bytes(),
                    row: p.row,
                    top: p.top,
                }),
                Slot::Break(t) => Some(Step::Break {
                    thickness: t as f64,
                }),
            })
            .collect();
        Runtime {
            steps,
            members: plan.members_by_press_id(),
            tab_factory: std::mem::transmute::<*const u8, TabFactoryFn>(sites.tab_factory),
            set_template: std::mem::transmute::<*const u8, SetTemplateFn>(sites.set_template),
            assign: std::mem::transmute::<*const u8, AssignFn>(sites.string_assign),
            clear_category: std::mem::transmute::<*const u8, ClearCategoryFn>(sites.clear_category),
            set_one: std::mem::transmute::<*const u8, SetOneFn>(sites.set_one),
            notify: std::mem::transmute::<*const u8, NotifyFn>(sites.notify),
            operator_new: std::mem::transmute::<*const u8, OperatorNewFn>(sites.operator_new),
            component_ctor: std::mem::transmute::<*const u8, ComponentCtorFn>(sites.component_ctor),
            vector_grow: std::mem::transmute::<*const u8, VectorGrowFn>(sites.vector_grow),
        }
    }
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static SPACER_WARNED: AtomicBool = AtomicBool::new(false);
static mut BUILDER_HOOK: Option<GenericDetour<BuilderFn>> = None;
static mut PRESS_HOOK: Option<GenericDetour<PressFn>> = None;

/// Install both detours (pass-through until [`set_active`]). All-or-nothing:
/// a failure drops whichever was installed and returns false.
///
/// # Safety
/// `sites` must come from `SignatureStore::series_enhanced_sites`.
pub unsafe fn install(sites: &SeriesEnhancedSites, runtime: Runtime) -> bool {
    if RUNTIME.set(runtime).is_err() {
        log_info!("SeriesExpansion[enhanced]: runtime already set (re-enable)");
    }
    if (*std::ptr::addr_of!(BUILDER_HOOK)).is_some() && (*std::ptr::addr_of!(PRESS_HOOK)).is_some()
    {
        return true;
    }
    let builder: BuilderFn = std::mem::transmute(sites.builder);
    let press: PressFn = std::mem::transmute(sites.group_press);
    if let Err(e) = crate::core::hooks::install_enabled(
        std::ptr::addr_of_mut!(BUILDER_HOOK),
        builder,
        builder_hook,
    ) {
        log_warn!("SeriesExpansion[enhanced]: builder detour failed: {:?}", e);
        return false;
    }
    if let Err(e) =
        crate::core::hooks::install_enabled(std::ptr::addr_of_mut!(PRESS_HOOK), press, press_hook)
    {
        log_warn!(
            "SeriesExpansion[enhanced]: group-press detour failed: {:?}",
            e
        );
        if let Some(hook) = (*std::ptr::addr_of!(BUILDER_HOOK)).as_ref() {
            let _ = hook.disable();
        }
        BUILDER_HOOK = None;
        return false;
    }
    log_info!(
        "SeriesExpansion[enhanced]: builder {:p} and group press {:p} detoured",
        sites.builder,
        sites.group_press
    );
    true
}

pub fn set_active(active: bool) {
    ACTIVE.store(active, Ordering::Release);
}

fn runtime() -> Option<&'static Runtime> {
    if ACTIVE.load(Ordering::Acquire) {
        RUNTIME.get()
    } else {
        None
    }
}

/// Destroy the builder's by-value factory exactly as the stock builder does.
unsafe fn destroy_factory(factory: *mut u8) {
    if factory.is_null() {
        return;
    }
    let slot = factory.add(FUNCTION_IMPL) as *mut *mut u8;
    let imp = *slot;
    if imp.is_null() {
        return;
    }
    let vtbl = *(imp as *const *const usize);
    let delete: FactoryDeleteFn = std::mem::transmute(*vtbl.add(3));
    delete(imp, (imp != factory) as u8);
    *slot = std::ptr::null_mut();
}

unsafe extern "C" fn builder_hook(capture: *mut u8, factory: *mut u8) {
    let Some(rt) = runtime() else {
        match (*std::ptr::addr_of!(BUILDER_HOOK)).as_ref() {
            Some(hook) => hook.call(capture, factory),
            None => destroy_factory(factory),
        }
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(rt, capture, factory)));
    destroy_factory(factory);
}

fn warn_spacer_once(why: &str) {
    if !SPACER_WARNED.swap(true, Ordering::Relaxed) {
        log_warn!(
            "SeriesExpansion[enhanced]: row break not added ({}) — rows around it may merge",
            why
        );
    }
}

/// The item grid the stock builder pushes its row break onto
/// (`[capture+0x40]+0x228`), or null.
unsafe fn item_grid(capture: *mut u8) -> *mut u8 {
    let slot = capture.add(CAPTURE_PANEL);
    if !memory::is_readable(slot, 8) {
        return std::ptr::null_mut();
    }
    let panel = *(slot as *const *mut u8);
    if panel.is_null() || !memory::is_readable(panel.add(PANEL_ITEM_GRID), 8) {
        return std::ptr::null_mut();
    }
    let grid = *(panel.add(PANEL_ITEM_GRID) as *const *mut u8);
    if grid.is_null() || !memory::is_readable(grid, GRID_PANEL_SIZE) {
        return std::ptr::null_mut();
    }
    grid
}

/// Tabs, cells (registered for scrolling) and row breaks, in plan order.
unsafe fn build(rt: &Runtime, capture: *mut u8, factory: *mut u8) {
    if capture.is_null() {
        return;
    }
    series_filter_scroll::begin_build();
    let invoke = if factory.is_null() {
        None
    } else {
        let imp = *(factory.add(FUNCTION_IMPL) as *const *mut u8);
        if imp.is_null() {
            log_warn!("SeriesExpansion[enhanced]: builder factory has no target — no cells");
            None
        } else {
            let vtbl = *(imp as *const *const usize);
            let f: FactoryInvokeFn = std::mem::transmute(*vtbl.add(1));
            Some((imp, f))
        }
    };
    let grid = item_grid(capture);
    // Spacers go only where the buttons went (a factory button's `parent`).
    let mut grid_confirmed: Option<bool> = None;
    for step in &rt.steps {
        match step {
            Step::Tab {
                press_id,
                template,
                label,
            } => {
                let btn = (rt.tab_factory)(capture, *press_id);
                if btn.is_null() {
                    continue;
                }
                (rt.set_template)(btn, *template);
                (rt.assign)(btn.add(BUTTON_LABEL), label.as_ptr(), label.len());
                confirm_grid(&mut grid_confirmed, btn, grid);
            }
            Step::Cell {
                selection,
                template,
                label,
                row,
                top,
            } => {
                let Some((imp, invoke)) = invoke else {
                    continue;
                };
                let btn = invoke(imp, *selection);
                if btn.is_null() {
                    continue;
                }
                (rt.set_template)(btn, *template);
                (rt.assign)(btn.add(BUTTON_LABEL), label.as_ptr(), label.len());
                confirm_grid(&mut grid_confirmed, btn, grid);
                series_filter_scroll::register_entry(btn, *row, *top);
            }
            Step::Break { thickness } => {
                if grid_confirmed == Some(false) {
                    continue;
                }
                if grid.is_null() {
                    warn_spacer_once("item grid unreadable");
                } else if !push_spacer(rt, grid, *thickness) {
                    warn_spacer_once("grid children vector unexpected");
                }
            }
        }
    }
}

/// Cross-check the capture-derived grid against a factory button's parent.
unsafe fn confirm_grid(state: &mut Option<bool>, btn: *mut u8, grid: *mut u8) {
    if state.is_some() {
        return;
    }
    let parent = *(btn.add(COMPONENT_PARENT) as *const *mut u8);
    let ok = !grid.is_null() && parent == grid;
    if !ok {
        warn_spacer_once("item grid is not the buttons' parent");
    }
    *state = Some(ok);
}

/// Append a full-width, `thickness`-tall bare Component to `grid`.
unsafe fn push_spacer(rt: &Runtime, grid: *mut u8, thickness: f64) -> bool {
    let vec = grid.add(COMPONENT_CHILDREN);
    let begin = *(vec as *const *mut *mut u8);
    let end = *(vec.add(8) as *const *mut *mut u8);
    let cap = *(vec.add(16) as *const *mut *mut u8);
    let sane = if begin.is_null() {
        end.is_null() && cap.is_null()
    } else {
        begin <= end && end <= cap && (end as usize - begin as usize) % 8 == 0
    };
    let width = *(grid.add(COMPONENT_WIDTH) as *const f64);
    if !sane || !(width > 0.0 && width < 4096.0) {
        return false;
    }

    let spacer = (rt.operator_new)(COMPONENT_SIZE);
    if spacer.is_null() {
        return false;
    }
    (rt.component_ctor)(spacer);
    *(spacer.add(COMPONENT_WIDTH) as *mut f64) = width;
    *(spacer.add(COMPONENT_HEIGHT) as *mut f64) = thickness;

    // children.push_back(spacer) — the stock inline form.
    if end == cap {
        (rt.vector_grow)(vec);
    }
    let end_slot = vec.add(8) as *mut *mut *mut u8;
    let end = *end_slot;
    if end.is_null() {
        // Unreachable after a successful grow; leak rather than free a
        // constructed Component with the wrong deleter.
        return false;
    }
    *end = spacer;
    *end_slot = end.add(1);
    *(spacer.add(COMPONENT_PARENT) as *mut *mut u8) = grid;
    true
}

unsafe extern "C" fn press_hook(captures: *mut u8, on: u8) {
    let Some(rt) = runtime() else {
        // A config tab (g >= 3) built while active must never reach the
        // stock body, which indexes the stock three-entry group table by g.
        let stock_g =
            !captures.is_null() && *(captures.add(0x18) as *const i32) < CUSTOM_TAB_ID_BASE;
        if stock_g {
            if let Some(hook) = (*std::ptr::addr_of!(PRESS_HOOK)).as_ref() {
                hook.call(captures, on);
            }
        }
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if captures.is_null() {
            return;
        }
        let state = *(captures as *const *mut u8);
        let category = *(captures.add(0x08) as *const i32);
        let g = *(captures.add(0x18) as *const i32);
        let panel = *(captures.add(0x20) as *const *mut u8);
        if state.is_null() {
            return;
        }
        (rt.clear_category)(state, category);
        if let Some(members) = usize::try_from(g).ok().and_then(|g| rt.members.get(g)) {
            for &idx in members {
                (rt.set_one)(state, category, idx, 1);
            }
        }
        if !panel.is_null() {
            (rt.notify)(panel);
        }
    }));
}
