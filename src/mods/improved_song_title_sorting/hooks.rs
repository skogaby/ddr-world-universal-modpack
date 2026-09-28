//! The MUSIC TITLE builder detour (owned by improved_song_title_sorting;
//! nothing else hooks this function). Passes straight through to the stock
//! builder unless [`set_active`]`(true)`.
//!
//! Builder contract (every supported build, pinned by `derive_title_filter`):
//! `builder(capture, factory)`; `capture` +0x00 template, +0x08 label prefix
//! (`"title"`); `factory` is a by-value `std::function<FilterButton*(int)>`
//! the builder owns — impl pointer at +0x18, vtable slot 1 invoke
//! `(impl, selection_index)` (returns a button already pushed into the item
//! grid, `button+0x60` = the grid), slot 3 delete `(impl, impl != factory)`.
//! The replacement destroys it exactly once, outside the fallible section.
//!
//! Row break: the stock `FilterHeader` the VERSION builder makes between its
//! tabs and entries (0xF8 bytes from the game's `operator new`, `Component`
//! ctor, both vtables, an empty `std::string` at +0xC0, two null words at
//! +0xE8/+0xF0), pushed onto the grid's `children` vector (`grid+0x68`
//! begin/end/cap, grown by the game's own grow-by-one) with `parent` (+0x60)
//! = the grid. Its layout hook makes it grid-wide and 1 px tall; it draws
//! nothing and is not focusable. The grid's clear destroys it through its
//! deleting destructor (the game's `operator delete`), like the stock one.

use super::layout::Item;
use crate::core::signatures::TitleFilterSites;
use crate::{log_info, log_warn};
use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

type BuilderFn = unsafe extern "C" fn(*mut u8, *mut u8);
type SetTemplateFn = unsafe extern "C" fn(*mut u8, i32);
type AssignFn = unsafe extern "C" fn(*mut u8, *const u8, usize) -> *mut u8;
type OperatorNewFn = unsafe extern "C" fn(usize) -> *mut u8;
type ComponentCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;
type VectorGrowFn = unsafe extern "C" fn(*mut u8);
type FactoryInvokeFn = unsafe extern "C" fn(*mut u8, i32) -> *mut u8;
type FactoryDeleteFn = unsafe extern "C" fn(*mut u8, u8);

/// `std::function` impl pointer offset (MSVC tr1: 0x18 bytes of inline storage).
const FUNCTION_IMPL: usize = 0x18;
/// `FilterButton+0xC8` — label key `std::string` (texture = `sefi_` + key).
const BUTTON_LABEL: usize = 0xC8;
/// `Component+0x60` — parent (the item grid for factory-made buttons).
const COMPONENT_PARENT: usize = 0x60;
/// `Component+0x68` — children `std::vector<Component*>` (begin/end/cap).
const COMPONENT_CHILDREN: usize = 0x68;
/// `FilterHeader` object size and field offsets (stock construction).
const HEADER_SIZE: usize = 0xF8;
const HEADER_VTABLE2: usize = 0x28;
const HEADER_STRING: usize = 0xC0;
const HEADER_WORDS: [usize; 2] = [0xE8, 0xF0];

/// One builder step with its label as bytes (no per-open allocation).
enum Step {
    Button {
        selection: i32,
        template: i32,
        label: Vec<u8>,
    },
    RowBreak,
}

/// Everything the detour needs, fixed at enable.
pub struct Runtime {
    steps: Vec<Step>,
    set_template: SetTemplateFn,
    assign: AssignFn,
    operator_new: OperatorNewFn,
    component_ctor: ComponentCtorFn,
    vector_grow: VectorGrowFn,
    header_vtable: *const u8,
    header_vtable2: *const u8,
}

// Function and vtable addresses in the loaded module, valid for the process
// lifetime.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

impl Runtime {
    /// # Safety
    /// `sites` must come from `SignatureStore::title_filter_sites`.
    pub unsafe fn new(sites: &TitleFilterSites, items: Vec<Item>) -> Runtime {
        let steps = items
            .into_iter()
            .map(|item| match item {
                Item::Button {
                    selection,
                    template,
                    label,
                } => Step::Button {
                    selection,
                    template,
                    label: label.into_bytes(),
                },
                Item::RowBreak => Step::RowBreak,
            })
            .collect();
        Runtime {
            steps,
            set_template: std::mem::transmute::<*const u8, SetTemplateFn>(sites.set_template),
            assign: std::mem::transmute::<*const u8, AssignFn>(sites.string_assign),
            operator_new: std::mem::transmute::<*const u8, OperatorNewFn>(sites.operator_new),
            component_ctor: std::mem::transmute::<*const u8, ComponentCtorFn>(sites.component_ctor),
            vector_grow: std::mem::transmute::<*const u8, VectorGrowFn>(sites.vector_grow),
            header_vtable: sites.header_vtable,
            header_vtable2: sites.header_vtable2,
        }
    }
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ROW_BREAK_WARNED: AtomicBool = AtomicBool::new(false);
static mut BUILDER_HOOK: Option<GenericDetour<BuilderFn>> = None;

/// Install the builder detour (pass-through until [`set_active`]).
///
/// # Safety
/// `sites` must come from `SignatureStore::title_filter_sites`.
pub unsafe fn install(sites: &TitleFilterSites, runtime: Runtime) -> bool {
    if RUNTIME.set(runtime).is_err() {
        log_info!("ImprovedTitleSorting: runtime already set (re-enable)");
    }
    if (*std::ptr::addr_of!(BUILDER_HOOK)).is_some() {
        return true;
    }
    let builder: BuilderFn = std::mem::transmute(sites.builder);
    match crate::core::hooks::install_enabled(
        std::ptr::addr_of_mut!(BUILDER_HOOK),
        builder,
        builder_hook,
    ) {
        Ok(()) => {
            log_info!(
                "ImprovedTitleSorting: MUSIC TITLE builder {:p} detoured",
                sites.builder
            );
            true
        }
        Err(e) => {
            log_warn!("ImprovedTitleSorting: builder detour failed: {:?}", e);
            false
        }
    }
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
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(rt, factory)));
    destroy_factory(factory);
}

/// Letters, OTHER, the row break, then the kana lines.
unsafe fn build(rt: &Runtime, factory: *mut u8) {
    if factory.is_null() {
        return;
    }
    let imp = *(factory.add(FUNCTION_IMPL) as *const *mut u8);
    if imp.is_null() {
        log_warn!("ImprovedTitleSorting: builder factory has no target — no cells");
        return;
    }
    let vtbl = *(imp as *const *const usize);
    let invoke: FactoryInvokeFn = std::mem::transmute(*vtbl.add(1));
    let mut last_button: *mut u8 = std::ptr::null_mut();
    for step in &rt.steps {
        match step {
            Step::Button {
                selection,
                template,
                label,
            } => {
                let btn = invoke(imp, *selection);
                if btn.is_null() {
                    continue;
                }
                (rt.set_template)(btn, *template);
                (rt.assign)(btn.add(BUTTON_LABEL), label.as_ptr(), label.len());
                last_button = btn;
            }
            Step::RowBreak => {
                if !last_button.is_null() && !push_row_break(rt, last_button) {
                    if !ROW_BREAK_WARNED.swap(true, Ordering::Relaxed) {
                        log_warn!(
                            "ImprovedTitleSorting: row break not added — the kana lines follow OTHER on its row"
                        );
                    }
                }
            }
        }
    }
}

/// Append a `FilterHeader` to the grid `last_button` was just pushed into.
/// Gates: the grid's children vector must be sane and end with
/// `last_button` (the factory's push), so the header lands right after it.
unsafe fn push_row_break(rt: &Runtime, last_button: *mut u8) -> bool {
    let grid = *(last_button.add(COMPONENT_PARENT) as *const *mut u8);
    if !crate::core::memory::is_readable(grid, COMPONENT_CHILDREN + 0x18) {
        return false;
    }
    let vec = grid.add(COMPONENT_CHILDREN);
    let begin = *(vec as *const *mut *mut u8);
    let end = *(vec.add(8) as *const *mut *mut u8);
    let cap = *(vec.add(16) as *const *mut *mut u8);
    if begin.is_null()
        || end <= begin
        || cap < end
        || (end as usize - begin as usize) % 8 != 0
        || *end.sub(1) != last_button
    {
        return false;
    }

    let header = (rt.operator_new)(HEADER_SIZE);
    if header.is_null() {
        return false;
    }
    (rt.component_ctor)(header);
    *(header as *mut *const u8) = rt.header_vtable;
    *(header.add(HEADER_VTABLE2) as *mut *const u8) = rt.header_vtable2;
    // Empty SSO std::string: buffer[0] = 0, size 0, capacity 15.
    *header.add(HEADER_STRING) = 0;
    *(header.add(HEADER_STRING + 0x10) as *mut u64) = 0;
    *(header.add(HEADER_STRING + 0x18) as *mut u64) = 0x0F;
    for off in HEADER_WORDS {
        *(header.add(off) as *mut u64) = 0;
    }

    // children.push_back(header) — the stock inline form.
    if end == cap {
        (rt.vector_grow)(vec);
    }
    let end_slot = vec.add(8) as *mut *mut *mut u8;
    let end = *end_slot;
    if end.is_null() {
        return false;
    }
    *end = header;
    *end_slot = end.add(1);
    *(header.add(COMPONENT_PARENT) as *mut *mut u8) = grid;
    true
}
