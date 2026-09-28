//! Detours for the enhanced VERSION layout: the VERSION builder and the
//! GROUP-tab press body. Both are owned by series_expansion (nothing else
//! hooks these functions) and pass straight through to the original unless
//! [`set_active`]`(true)` — i.e. only while enhanced mode is enabled.
//!
//! Builder contract (every supported build): `builder(capture, factory)`;
//! `capture` +0x00 selection state, +0x08 category, +0x10 FilterPanel;
//! `factory` is a by-value `std::function<FilterButton*(int)>` the builder
//! owns — impl pointer at +0x18, vtable slot 1 invoke `(impl, idx)`, slot 3
//! delete `(impl, impl != factory)`. The replacement destroys it exactly
//! once, outside the fallible section.
//!
//! Press contract: `press(captures, on)`; `captures` +0x00 state, +0x08
//! category (i32), +0x18 `g` (i32; 0 CLASSIC, 1 WHITE, 2 GOLD), +0x20
//! FilterPanel. The replacement never falls through to the stock body while
//! active (the stock body reads the stock group table).

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

/// `std::function` impl pointer offset (MSVC tr1: 0x18 bytes of inline storage).
const FUNCTION_IMPL: usize = 0x18;
/// `FilterButton+0xC8` — label key `std::string` (texture = `sefi_` + key).
const BUTTON_LABEL: usize = 0xC8;
/// Stock group tabs use the 72-px template (three fill the 216-px line).
const TAB_TEMPLATE: i32 = 3;
/// GROUP tabs in stock creation order (GOLD, WHITE, CLASSIC): stock group
/// index `g` and label key.
const TABS: [(i32, &[u8]); 3] = [
    (2, b"version_gold"),
    (1, b"version_white"),
    (0, b"version_classic"),
];

/// Everything the detours need, fixed at enable.
pub struct Runtime {
    pub columns: i32,
    /// `version_<texture>_<N>col` per cell, in selection-index order.
    pub label_keys: Vec<Vec<u8>>,
    /// Cell indices per stock group index `g`.
    pub members: [Vec<i32>; 3],
    tab_factory: TabFactoryFn,
    set_template: SetTemplateFn,
    assign: AssignFn,
    clear_category: ClearCategoryFn,
    set_one: SetOneFn,
    notify: NotifyFn,
}

impl Runtime {
    /// # Safety
    /// `sites` must come from `SignatureStore::series_enhanced_sites` (verified
    /// function entries in the loaded module).
    pub unsafe fn new(
        sites: &SeriesEnhancedSites,
        columns: u8,
        label_keys: Vec<Vec<u8>>,
        members: [Vec<i32>; 3],
    ) -> Runtime {
        Runtime {
            columns: columns as i32,
            label_keys,
            members,
            tab_factory: std::mem::transmute::<*const u8, TabFactoryFn>(sites.tab_factory),
            set_template: std::mem::transmute::<*const u8, SetTemplateFn>(sites.set_template),
            assign: std::mem::transmute::<*const u8, AssignFn>(sites.string_assign),
            clear_category: std::mem::transmute::<*const u8, ClearCategoryFn>(sites.clear_category),
            set_one: std::mem::transmute::<*const u8, SetOneFn>(sites.set_one),
            notify: std::mem::transmute::<*const u8, NotifyFn>(sites.notify),
        }
    }
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
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

/// GROUP tabs, then one cell per config entry (registered for scrolling).
unsafe fn build(rt: &Runtime, capture: *mut u8, factory: *mut u8) {
    if capture.is_null() {
        return;
    }
    for (g, key) in TABS {
        let btn = (rt.tab_factory)(capture, g);
        if btn.is_null() {
            continue;
        }
        (rt.set_template)(btn, TAB_TEMPLATE);
        (rt.assign)(btn.add(BUTTON_LABEL), key.as_ptr(), key.len());
    }
    series_filter_scroll::begin_build();
    if factory.is_null() {
        return;
    }
    let imp = *(factory.add(FUNCTION_IMPL) as *const *mut u8);
    if imp.is_null() {
        log_warn!("SeriesExpansion[enhanced]: builder factory has no target — no cells");
        return;
    }
    let vtbl = *(imp as *const *const usize);
    let invoke: FactoryInvokeFn = std::mem::transmute(*vtbl.add(1));
    for (i, key) in rt.label_keys.iter().enumerate() {
        let btn = invoke(imp, i as i32);
        if btn.is_null() {
            continue;
        }
        (rt.set_template)(btn, rt.columns);
        (rt.assign)(btn.add(BUTTON_LABEL), key.as_ptr(), key.len());
        series_filter_scroll::register_entry(btn, i / rt.columns.max(1) as usize);
    }
}

unsafe extern "C" fn press_hook(captures: *mut u8, on: u8) {
    let Some(rt) = runtime() else {
        if let Some(hook) = (*std::ptr::addr_of!(PRESS_HOOK)).as_ref() {
            hook.call(captures, on);
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
