//! Row allocation for custom options.
//!
//! Given a registered option and a player side, this module produces a
//! 0x330-byte `OptionElement`-shaped row that the game's scene graph can
//! walk without modification — layout-identical to a native
//! `OptionElement<ArrowColor>` except that its primary vtable pointer is
//! overwritten to point at a mod-owned 8-slot table whose slot 4
//! (`advanceValue`) cycles through the option's `allowed_values` instead of
//! the game's native value list.
//!
//! Pipeline (`allocate_row_for_option`):
//!   1. `game_malloc(0x330)` + zero-fill.
//!   2. Call the ArrowColor donor ctor on the buffer — initializes all
//!      fields correctly (including the 3 MI-base vtables we do *not*
//!      override).
//!   3. Synthesize an 8-slot primary vtable: slots 0/1/2/3/5/6/7 copied
//!      verbatim from the donor's primary vtable; slot 4 =
//!      [`advance_value_enum_trampoline`].
//!   4. Write the mod vtable pointer to `row[+0x00]`.
//!   5. For each `PageTag`: build a stack-local SSO `std::string` holding
//!      the `"PageN"` literal, then call
//!      `metadata_insert(row, &str)` to hash-insert into the row's inlined
//!      rb-tree at `+0x08..+0x28`. Run the std::string destructor to
//!      release any heap buffer (impossible for a 5-char literal, but
//!      keeps the sequence symmetric with the native row builder).
//!   6. Record the row in [`ROWS`] so (a) the pointer is kept alive for
//!      process lifetime, (b) slot-4 dispatch can resolve the row pointer
//!      back to `(OptionHandle, side)`, and (c) callers can iterate rows
//!      by `(side, page)` for scroll-driver and builder-hook use.
//!
//! Allocator discipline:
//! - The 0x330 row is allocated via `game_malloc` because the game's own
//!   destructor will call CRT `free()` on it if the row is ever
//!   unregistered (Konami's `FUN_18017cc20` scalar-deleting dtor).
//! - The synthesized vtable is allocated via
//!   [`crate::core::memory::alloc_zeroed`] (VirtualAlloc — RWX). Safe
//!   because the game never frees the vtable; we leak it for process
//!   lifetime.
//!
//! Slot-4 dispatch uses a `this` → `(OptionHandle, side)` lookup map
//! ([`ROWS`]) rather than per-option `extern "C"` trampolines, avoiding a
//! fixed upper bound on concurrent options.

use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::{log_debug, log_error, log_info, log_warn};

use super::api::{format_scalar_value, OptionHandle, PageTag, ShowWhen, UiKind};
use super::registry;

/// Size of an `OptionElement<KIND>` row in bytes. Every enum-kind
/// specialization shares this layout, which is why the donor-vtable
/// technique works without per-kind field adjustments.
const ROW_SIZE: usize = 0x330;

/// Number of qword slots in the primary vtable we synthesize per row.
const PRIMARY_VTABLE_SLOTS: usize = 8;

/// Primary-vtable slot index for `advanceValue` — the only slot whose
/// body needs to touch mod-owned state. Points at the
/// [`advance_value_enum_trampoline`] override.
const SLOT_ADVANCE_VALUE: usize = 4;

/// Primary-vtable slot index for `onCreate`. The donor's implementation
/// installs reactive-stream subscribers keyed to the donor kind's
/// templated [`OptionItem<KIND>`] layout; inheriting them on a mod row
/// wires those subscribers to the wrong state and triggers a crash when
/// the streams fire. Overridden with a no-op so mod rows opt out of the
/// reactive pipeline entirely.
const SLOT_ON_CREATE: usize = 6;

/// Primary-vtable slot index for `render`. Enum rows override this with
/// a custom renderer that binds textures via `mc_load_bitmap`. Scalar
/// rows inherit the donor's slot 7 (which drives TextLayer ticks) and
/// wrap it to also push formatted value text via `textlayer_set_text`
/// — see [`render_scalar_trampoline`].
const SLOT_RENDER: usize = 7;

/// Offset of the primary vtable pointer within a row. Always `+0x00`.
const ROW_PRIMARY_VTABLE_OFFSET: usize = 0x00;

/// Offset of the `IOptionElement` MI-base vtable pointer within a row. This
/// is the third multiple-inheritance base; the options menu's preview-image
/// box resolves the focused row via `__RTDynamicCast(Component ->
/// IOptionElement)` and calls slot 0 of this vtable to get the
/// `seop_image_*` preview texture name. See
/// `docs/option_preview_image_box.md`.
const ROW_IOPTIONELEMENT_VTABLE_OFFSET: usize = 0xC0;

/// Number of qword slots in the `IOptionElement` MI vtable. Determined from
/// the donor vtable layout (slots 0..7 are `.text`; slot 8 is the next MI
/// base's COL pointer). We clone all of them and override only slot 0.
const IOPTIONELEMENT_VTABLE_SLOTS: usize = 8;

/// `IOptionElement`-vtable slot index for the preview-image-name getter
/// (`get preview image name`). The native slot returns `""` when the row's
/// value-model self-pointer (`row+0x110`) is null — which it always is on
/// our injected rows — so we override it to write `seop_image_<id>`
/// directly from the registry.
const SLOT_PREVIEW_IMAGE_NAME: usize = 0;

/// Offset of the selectability MI-interface vtable pointer within a row.
/// This is the second multiple-inheritance base; slot 0 is
/// `bool isSelectable(this)` (tested by every cursor path together with the
/// `+0xB8` active byte) and slot 1 is `void onFocusChanged(this, bool)`.
/// Header rows swap this pointer for a mod-owned `{return 0, no-op}` table —
/// the engine's own gray-row ("MIN~CORE~MAX") mechanism. See
/// `docs/option_header_rows_research.md` §1–2.
const ROW_SELECTABLE_IFACE_OFFSET: usize = 0x28;

/// Number of function slots in the `+0x28` selectability interface vtable
/// (the qword after slot 1 is the next MI base's RTTI COL pointer on both
/// native classes — the table ends at 2 slots).
const SELECTABLE_VTABLE_SLOTS: usize = 2;

// ── Function-pointer types for the game functions we call ────────────────

/// CRT operator new. `__fastcall(RCX=size) -> *mut u8`.
type GameMallocFn = unsafe extern "C" fn(usize) -> *mut u8;

/// `OptionElement<ArrowColor>::ctor`. `__fastcall(RCX=this) -> *mut u8`.
/// The ctor initializes all 4 vtables and every field in the row; we then
/// selectively overwrite just `row[+0x00]` with our mod vtable.
type DonorCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;

/// MSVC `std::basic_string::assign(const char*, size_t)`. `__fastcall(RCX=this,
/// RDX=src, R8=len) -> *mut u8`. Writes into a pre-initialized 32-byte SSO
/// string object.
type StringAssignFn = unsafe extern "C" fn(*mut u8, *const u8, usize) -> *mut u8;

/// OptionElement metadata-set insert. `__fastcall(RCX=row, RDX=key) -> *mut u8`.
/// Hashes the std::string's contents (FNV-1a) and inserts into the rb-tree
/// embedded at `row[+0x08..+0x28]`.
type MetadataInsertFn = unsafe extern "C" fn(*mut u8, *mut u8) -> *mut u8;

/// Engine event-callback registration. `__fastcall(RCX=event_obj, EDX=type,
/// R8=lambda)`. Registers a type-gated lambda against the input event
/// currently being dispatched. Only the registration whose `type` matches
/// the event-in-flight has its invoke slot fired; other registrations are
/// destructed in place. Used by mod slot-4 to receive left (type=1) and
/// right (type=2) presses without intercepting up/down/stop events.
///
/// This variant registers lambdas gated on `event_obj+0x10 == 1`, i.e.
/// the Start-modifier is NOT held (fine-step on scalar rows; the only
/// register path used by enum rows).
type EventRegisterFn = unsafe extern "C" fn(*mut u8, i32, *mut u8);

/// Twin of [`EventRegisterFn`] that gates on `event_obj+0x10 == 2`
/// (Start IS held). `__fastcall(RCX=event_obj, EDX=type, R8=aux_predicate,
/// R9=lambda)`. The extra `aux_predicate` arg is passed through to a
/// secondary gate the native path always populates with `0`; we match
/// that behavior. Used only by scalar rows for coarse-step lambdas.
type EventRegisterNoConsumeFn = unsafe extern "C" fn(*mut u8, i32, u32, *mut u8);

/// TextLayer constructor. `__fastcall(RCX=this) -> *mut u8`.
/// Initializes a 0x150-byte TextLayer object in-place; callers allocate
/// the buffer via `game_malloc(0x150)` before calling.
type TextLayerCtorFn = unsafe extern "C" fn(*mut u8) -> *mut u8;

/// TextLayer bind. `__fastcall(RCX=this, RDX=parent_mc_ptr,
/// R8=path_sso_string_ptr) -> *mut u8`. Attaches the TextLayer to a named
/// child path under the parent MC.
type TextLayerBindFn = unsafe extern "C" fn(*mut u8, *mut u8, *mut u8) -> *mut u8;

/// TextLayer set-text. `__fastcall(RCX=this, RDX=text_sso_string_ptr,
/// R8D=mode) -> *mut u8`. Sets the text/bitmap key on the layer; mode 3
/// is normal display.
type TextLayerSetTextFn = unsafe extern "C" fn(*mut u8, *mut u8, i32) -> *mut u8;

// ── Resolved function pointers (populated by init) ───────────────────────

static mut FN_GAME_MALLOC: Option<GameMallocFn> = None;
static mut FN_DONOR_CTOR: Option<DonorCtorFn> = None;
/// `OptionElement<int>::ctor`. Donor for scalar rows. Resolved from the
/// `option_element_int_ctor` signature.
static mut FN_DONOR_CTOR_INT: Option<DonorCtorFn> = None;
static mut FN_STRING_ASSIGN: Option<StringAssignFn> = None;
static mut FN_METADATA_INSERT: Option<MetadataInsertFn> = None;
static mut FN_EVENT_REGISTER: Option<EventRegisterFn> = None;
/// Coarse-step twin of [`FN_EVENT_REGISTER`] — registers lambdas that
/// fire only when Start is held. Scalar-row-only.
static mut FN_EVENT_REGISTER_NO_CONSUME: Option<EventRegisterNoConsumeFn> = None;
/// Required for scalar value display; unused on the enum path.
static mut FN_TEXTLAYER_CTOR: Option<TextLayerCtorFn> = None;
static mut FN_TEXTLAYER_BIND: Option<TextLayerBindFn> = None;
static mut FN_TEXTLAYER_SET_TEXT: Option<TextLayerSetTextFn> = None;

/// ArrowColor donor's primary vtable (used for enum rows). We read slots
/// 0/1/2/3/5/6/7 from it verbatim when building each enum mod vtable.
static mut DONOR_PRIMARY_VTABLE: *const *const u8 = std::ptr::null();

/// OptionElement<int> donor's primary vtable (used for scalar rows).
/// Slots 0/1/2/3/5 are copied verbatim; slot 4 is overridden with
/// [`advance_value_scalar_trampoline`] and slot 6 with a no-op. Slot 7 is
/// overridden with [`render_scalar_trampoline`] which calls the donor's
/// native slot 7 then pushes the current value text.
static mut DONOR_PRIMARY_VTABLE_INT: *const *const u8 = std::ptr::null();

/// Process-lifetime "left fine-press" lambda vtable. Captured argument
/// is the row pointer; invoke trampoline is [`invoke_left_trampoline`]
/// which advances the row's value left by one fine step (enum: cycles
/// one entry back; scalar: subtracts `step_fine` and clamps). Shared
/// between enum and scalar rows.
static mut MOD_LAMBDA_VTABLE_LEFT: *mut *const u8 = std::ptr::null_mut();

/// Process-lifetime "right fine-press" lambda vtable. Twin of
/// [`MOD_LAMBDA_VTABLE_LEFT`] for forward cycling / positive fine-step.
static mut MOD_LAMBDA_VTABLE_RIGHT: *mut *const u8 = std::ptr::null_mut();

/// Process-lifetime "left coarse-press" lambda vtable (scalar-only).
/// Captured argument is the row pointer; invoke trampoline subtracts
/// `step_coarse` and clamps.
static mut MOD_LAMBDA_VTABLE_LEFT_COARSE: *mut *const u8 = std::ptr::null_mut();

/// Process-lifetime "right coarse-press" lambda vtable (scalar-only).
/// Twin of [`MOD_LAMBDA_VTABLE_LEFT_COARSE`].
static mut MOD_LAMBDA_VTABLE_RIGHT_COARSE: *mut *const u8 = std::ptr::null_mut();

/// Kind tag distinguishing enum rows (ArrowColor donor) from scalar rows
/// (OptionElement<int> donor). Determines which invoke-trampoline
/// behavior fires and which render slot was synthesized. Header rows use
/// the ArrowColor donor with a label-only render and a swapped `+0x28`
/// selectability interface (never focusable, so no advance/press paths).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RowKind {
    Enum,
    Scalar,
    Header,
}

/// One row's worth of persistent storage. The vtable buffer lives for the
/// process lifetime; the row pointer is owned by the game after we hand it
/// off (eventual free goes through the donor dtor → CRT free).
struct RowSlot {
    handle: OptionHandle,
    side: u8,
    row_ptr: *mut u8,
    kind: RowKind,
    /// Process-lifetime vtable backing. Never freed. 8 qword slots —
    /// slot 4 is the appropriate advance-value trampoline for this row's
    /// kind. Kept alongside the row so it's impossible to drop
    /// independently.
    _vtable: *mut *const u8,
    /// Scalar-only: cache of the last value text pushed into the value
    /// TextLayer. Used by the slot-7 override to skip the
    /// `textlayer_set_text` dispatch when the value hasn't changed.
    /// Wrapped in a `Mutex<Option<String>>` because the slot-7 override
    /// runs on the render thread and mutates this cache; reading it from
    /// other threads is a future concern but the mutex keeps us safe.
    last_value_text: Mutex<Option<String>>,
    /// Eased position state for the value marker bar
    /// (`choice_usr/scroll_usr`), used by both enum and scalar rows. The
    /// native render keeps this at row+0x140 and lerps it toward the target
    /// each frame; we keep our own copy so we don't depend on a field the
    /// donor ctor leaves in a non-native state. `f64::NAN` means
    /// "uninitialized" — the first marker frame snaps directly to the target
    /// instead of easing from 0. Touched only on the render thread. See
    /// `docs/option_row_marker_render.md`.
    marker_anim: Mutex<f64>,
}

// SAFETY: RowSlot holds raw pointers into game memory. The vtable buffer is
// valid for process lifetime (never freed). The `row_ptr` is owned by the
// game and freed when the options overlay closes — so a slot is only valid
// while it's tracked in ROWS: the OptionForm-dtor hook removes a side's slots
// the instant the game frees those rows (see `clear_side` / `dtor_hook`), so
// no consumer ever dereferences a freed row. All mutation is serialized by the
// Mutex around ROWS.
unsafe impl Send for RowSlot {}
unsafe impl Sync for RowSlot {}

/// Registry of every row we've allocated. Append-only; entries are never
/// reordered so raw `row_ptr` values can be looked up by linear scan from
/// the slot-4 trampoline.
static ROWS: Lazy<Mutex<Vec<RowSlot>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Snapshot all row pointers allocated for `side`. Used by the scroll
/// driver to enumerate candidate rows for a given OptionForm / player
/// side without exposing [`RowSlot`]'s private internals.
///
/// Note: the returned pointers may belong to multiple page tags once
/// multi-page support is added; at present every row is tagged `Page6`,
/// so this is equivalent to "all mod rows for `side`".
pub(crate) fn row_ptrs_for_side(side: u8) -> Vec<*mut u8> {
    row_entries_for_side(side)
        .into_iter()
        .map(|(ptr, _)| ptr)
        .collect()
}

/// Snapshot `(row_ptr, selectable)` for every row allocated for `side`, in
/// injection (display) order. `selectable` is `false` for header rows —
/// their `+0x28` interface returns 0 and the cursor must never land on
/// them. The scroll driver consumes this to skip headers when it computes
/// step targets itself (its window-driven step path replaces the native
/// scan, which is where the `+0x28` predicate would otherwise be honored).
pub(crate) fn row_entries_for_side(side: u8) -> Vec<(*mut u8, bool)> {
    let rows = ROWS.lock().unwrap();
    let state = registry::STATE.lock().unwrap();
    rows.iter()
        .filter(|slot| slot.side == side)
        .filter(|slot| state.show_when_satisfied(slot.handle, side))
        .map(|slot| (slot.row_ptr, slot.kind != RowKind::Header))
        .collect()
}

// `is_show_when_satisfied` moved to `registry::FrameworkState::show_when_satisfied`
// (overlay-rewrite Step 5) — the overlay snapshot's `visible` flag and the
// in-game scroll-mask filtering share one evaluator so both menus agree.

/// Write `+0xB8 = 0` on every row for `side` whose ShowWhen predicate is
/// NOT satisfied. Called after the native filter sets all Page6 rows to
/// visible, so our hidden rows get explicitly suppressed.
///
/// Only dereferences rows still tracked in [`ROWS`]. The OptionForm-dtor hook
/// removes a side's rows the instant the game frees them, so any slot present
/// here points at a live row; the early-return guard is a backstop should an
/// unforeseen free path ever leave `ROWS` empty for this side.
pub(crate) fn hide_show_when_excluded(side: u8) {
    let rows = ROWS.lock().unwrap();
    if rows.iter().all(|slot| slot.side != side) {
        return;
    }
    let state = registry::STATE.lock().unwrap();
    for slot in rows.iter() {
        if slot.side == side && !state.show_when_satisfied(slot.handle, side) {
            unsafe {
                std::ptr::write(slot.row_ptr.add(0xB8), 0u8);
            }
        }
    }
}

/// Remove all row entries for the given `side`. Called from two places:
///   - the OptionForm-dtor hook, as the overlay closes — *before* the game
///     frees that side's rows, so no stale pointer is ever left observable;
///   - the builder hook at the start of each menu rebuild, as a backstop in
///     case the dtor hook isn't installed (signature unresolved).
///
/// Either way, after this returns the side's (about-to-be / already) freed
/// rows are no longer tracked. The vtable buffers (VirtualAlloc) are
/// intentionally leaked since the cost is negligible (~72 bytes each) and
/// freeing them would require tracking whether the game's async dtor has
/// actually run.
pub(crate) fn clear_side(side: u8) {
    let mut rows = ROWS.lock().unwrap();
    let before = rows.len();
    rows.retain(|slot| slot.side != side);
    let removed = before - rows.len();
    if removed > 0 {
        log_debug!(
            "custom_options/rows: cleared {} stale row(s) for side {}",
            removed,
            side
        );
    }
    drop(rows);
    // The rows are gone, so the side's scroll window is meaningless —
    // reset it to the top to match the fresh form's initial focus (a
    // stale mid-list window would clip the top of the list, including a
    // leading header row, on the next open).
    crate::services::options_scroll::reset_window(side);
}

/// Find the player side of the row whose `+0x60` parent-container link
/// points at `container`. Returns `None` if no such row is registered.
/// Used by the scroll driver to map a container pointer (handed in by
/// focus-update / filter hooks) back to a side.
///
/// SAFETY: dereferences `row_ptr+0x60` for each tracked slot. Sound because
/// the OptionForm-dtor hook removes a side's rows the moment the game frees
/// them, so every slot still in [`ROWS`] points at a live row. (An empty
/// `ROWS` simply returns `None` without dereferencing anything.)
pub(crate) fn side_for_container(container: *mut u8) -> Option<u8> {
    let rows = ROWS.lock().unwrap();
    for slot in rows.iter() {
        let parent = unsafe { *(slot.row_ptr.add(0x60) as *const *mut u8) };
        if parent == container {
            return Some(slot.side);
        }
    }
    None
}

/// Has [`init`] populated the function pointers successfully?
static mut READY: bool = false;

/// Resolve the addresses this module needs from the signature store.
/// Returns `true` if every required address was resolved; `false`
/// indicates graceful degradation — callers will get `None` from
/// [`allocate_row_for_option`].
pub(crate) fn init(signatures: &SignatureStore) -> bool {
    unsafe {
        let game_malloc = match signatures.get_address("game_malloc") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: game_malloc not resolved — row allocation disabled"
                );
                return false;
            }
        };
        let donor_ctor = match signatures.get_address("option_element_arrowcolor_ctor") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: option_element_arrowcolor_ctor not resolved — row allocation disabled"
                );
                return false;
            }
        };
        let donor_vtable = match signatures.get_address("option_element_arrowcolor_primary_vtable")
        {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: option_element_arrowcolor_primary_vtable not resolved — row allocation disabled"
                );
                return false;
            }
        };
        // Scalar-path donor. If either the ctor or its primary vtable fails
        // to resolve, scalar rows gracefully degrade: enum rows continue to
        // work, scalar `register_option` calls still succeed (no Err), but
        // `allocate_scalar_row_for_option` returns None and the row is
        // simply absent from the menu.
        let donor_ctor_int = signatures.get_address("option_element_int_ctor");
        let donor_vtable_int = signatures.get_address("option_element_int_primary_vtable");
        if donor_ctor_int.is_none() || donor_vtable_int.is_none() {
            log_warn!(
                "custom_options/rows: option_element_int signatures not resolved — scalar rows disabled (enum rows still work)"
            );
        }
        let string_assign = match signatures.get_address("string_assign") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: string_assign not resolved — row allocation disabled"
                );
                return false;
            }
        };
        let metadata_insert = match signatures.get_address("metadata_insert") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: metadata_insert not resolved — row allocation disabled"
                );
                return false;
            }
        };

        let event_register = match signatures.get_address("event_register") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: event_register not resolved — row slot-4 input registration disabled"
                );
                return false;
            }
        };
        // Optional — if unresolved, scalar rows lose their coarse-step
        // (Start-held) behavior but fine-step still works via the shared
        // event_register path. Enum rows don't care.
        let event_register_no_consume = signatures.get_address("event_register_no_consume");
        let lambda_destruct_slot3 = match signatures.get_address("lambda_destruct_slot3") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: lambda_destruct_slot3 not resolved — mod lambda vtable cannot be built"
                );
                return false;
            }
        };
        let lambda_release_slot4 = match signatures.get_address("lambda_release_slot4") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: lambda_release_slot4 not resolved — mod lambda vtable cannot be built"
                );
                return false;
            }
        };
        let lambda_get_captured_slot5 = match signatures.get_address("lambda_get_captured_slot5") {
            Some(a) => a,
            None => {
                log_warn!(
                    "custom_options/rows: lambda_get_captured_slot5 not resolved — mod lambda vtable cannot be built"
                );
                return false;
            }
        };

        FN_GAME_MALLOC = Some(std::mem::transmute::<*const u8, GameMallocFn>(game_malloc));
        FN_DONOR_CTOR = Some(std::mem::transmute::<*const u8, DonorCtorFn>(donor_ctor));
        if let Some(addr) = donor_ctor_int {
            FN_DONOR_CTOR_INT = Some(std::mem::transmute::<*const u8, DonorCtorFn>(addr));
        }
        if let Some(addr) = donor_vtable_int {
            DONOR_PRIMARY_VTABLE_INT = addr as *const *const u8;
        }
        FN_STRING_ASSIGN = Some(std::mem::transmute::<*const u8, StringAssignFn>(
            string_assign,
        ));
        FN_METADATA_INSERT = Some(std::mem::transmute::<*const u8, MetadataInsertFn>(
            metadata_insert,
        ));
        FN_EVENT_REGISTER = Some(std::mem::transmute::<*const u8, EventRegisterFn>(
            event_register,
        ));
        if let Some(addr) = event_register_no_consume {
            FN_EVENT_REGISTER_NO_CONSUME = Some(std::mem::transmute::<
                *const u8,
                EventRegisterNoConsumeFn,
            >(addr));
        }
        DONOR_PRIMARY_VTABLE = donor_vtable as *const *const u8;

        // TextLayer functions — optional; gracefully degrade to mc_load_bitmap
        // if any are absent. Resolve all three or none (partial wiring would
        // leave bind/set-text dangling).
        match (
            signatures.get_address("textlayer_ctor"),
            signatures.get_address("textlayer_bind"),
            signatures.get_address("textlayer_set_text"),
        ) {
            (Some(ctor), Some(bind), Some(set_text)) => {
                FN_TEXTLAYER_CTOR = Some(std::mem::transmute::<*const u8, TextLayerCtorFn>(ctor));
                FN_TEXTLAYER_BIND = Some(std::mem::transmute::<*const u8, TextLayerBindFn>(bind));
                FN_TEXTLAYER_SET_TEXT = Some(std::mem::transmute::<*const u8, TextLayerSetTextFn>(
                    set_text,
                ));
                log_info!("custom_options/rows: TextLayer functions resolved — native value display enabled");
            }
            _ => {
                log_warn!(
                    "custom_options/rows: one or more textlayer signatures not resolved — value display falls back to mc_load_bitmap"
                );
            }
        }

        MOD_LAMBDA_VTABLE_LEFT = build_mod_lambda_vtable(
            invoke_left_trampoline as *const u8,
            lambda_destruct_slot3,
            lambda_release_slot4,
            lambda_get_captured_slot5,
        );
        MOD_LAMBDA_VTABLE_RIGHT = build_mod_lambda_vtable(
            invoke_right_trampoline as *const u8,
            lambda_destruct_slot3,
            lambda_release_slot4,
            lambda_get_captured_slot5,
        );
        MOD_LAMBDA_VTABLE_LEFT_COARSE = build_mod_lambda_vtable(
            invoke_left_coarse_trampoline as *const u8,
            lambda_destruct_slot3,
            lambda_release_slot4,
            lambda_get_captured_slot5,
        );
        MOD_LAMBDA_VTABLE_RIGHT_COARSE = build_mod_lambda_vtable(
            invoke_right_coarse_trampoline as *const u8,
            lambda_destruct_slot3,
            lambda_release_slot4,
            lambda_get_captured_slot5,
        );
        if MOD_LAMBDA_VTABLE_LEFT.is_null()
            || MOD_LAMBDA_VTABLE_RIGHT.is_null()
            || MOD_LAMBDA_VTABLE_LEFT_COARSE.is_null()
            || MOD_LAMBDA_VTABLE_RIGHT_COARSE.is_null()
        {
            log_warn!(
                "custom_options/rows: mod lambda vtable allocation failed — row slot-4 input registration disabled"
            );
            return false;
        }

        READY = true;
    }
    true
}

/// Is the scalar-row code path ready to allocate? Requires the
/// `OptionElement<int>` donor signatures + the coarse-step register. Enum
/// rows remain available even if this returns `false`.
pub(crate) fn is_scalar_ready() -> bool {
    unsafe {
        is_ready()
            && std::ptr::read(std::ptr::addr_of!(FN_DONOR_CTOR_INT)).is_some()
            && !std::ptr::read(std::ptr::addr_of!(DONOR_PRIMARY_VTABLE_INT)).is_null()
            && std::ptr::read(std::ptr::addr_of!(FN_EVENT_REGISTER_NO_CONSUME)).is_some()
            && std::ptr::read(std::ptr::addr_of!(FN_TEXTLAYER_SET_TEXT)).is_some()
    }
}

pub(crate) fn is_ready() -> bool {
    unsafe { READY }
}

/// Read-only diagnostic: log the slots the synthesized mod vtable would
/// contain for a row, without allocating a row or invoking the donor ctor.
///
/// [`allocate_row_for_option`] is only safe to call once the game has
/// populated the options subsystem's runtime prerequisites (reactive-stream
/// registries, resource manager, app-heap handle value). The donor ctor's
/// parent-class calls dereference those globals, so invoking it from the
/// DLL init thread — which runs well before the game's own subsystems come
/// up — produces an access violation inside the parent ctor. This preview
/// path reads only from `.rdata` (the donor vtable), which is valid from
/// the moment gamemdx is loaded, and validates every signature-derivation
/// link in the chain without touching writable game state.
pub(crate) fn log_vtable_preview() {
    if !is_ready() {
        log_warn!("custom_options/rows: vtable preview unavailable — rows module not ready");
        return;
    }
    unsafe {
        // Read the static into a local to avoid taking a shared reference
        // to a mutable static (disallowed by `static_mut_refs`).
        let donor_vtable = std::ptr::read(std::ptr::addr_of!(DONOR_PRIMARY_VTABLE));
        if donor_vtable.is_null() {
            log_warn!(
                "custom_options/rows: vtable preview — donor vtable pointer is null (derivation failed)"
            );
            return;
        }
        let mut slots: [*const u8; PRIMARY_VTABLE_SLOTS] = [std::ptr::null(); PRIMARY_VTABLE_SLOTS];
        for (i, slot) in slots.iter_mut().enumerate() {
            *slot = *donor_vtable.add(i);
        }
        let override_addr = advance_value_enum_trampoline as *const u8;
        let noop_addr = noop_virtual_trampoline as *const u8;
        let donor_col = *donor_vtable.offset(-1);
        log_info!(
            "custom_options/rows: vtable preview — donor vtable @ {:p}; COL = {:p}; slot-4 override = {:p}; slot-6/7 no-op = {:p}",
            donor_vtable,
            donor_col,
            override_addr,
            noop_addr
        );
        log_info!(
            "custom_options/rows: donor slots = [0:{:p} 1:{:p} 2:{:p} 3:{:p} 4:{:p} 5:{:p} 6:{:p} 7:{:p}]",
            slots[0], slots[1], slots[2], slots[3], slots[4], slots[5], slots[6], slots[7]
        );
        log_info!(
            "custom_options/rows: synthesized mod vtable would be [COL:{:p} 0:{:p} 1:{:p} 2:{:p} 3:{:p} 4:{:p}(OURS) 5:{:p} 6:{:p}(NOOP) 7:{:p}(NOOP)]",
            donor_col, slots[0], slots[1], slots[2], slots[3], override_addr, slots[5], noop_addr, noop_addr
        );
    }
}

/// Allocate and tag a row for the option identified by `handle`, for
/// `side` (0 = P1, 1 = P2). Returns `None` if the module isn't
/// initialized, if the allocation fails, or if the option isn't
/// registered (e.g. the handle is from a prior process lifetime).
///
/// The returned pointer is owned jointly: this module keeps a reference
/// alive in [`ROWS`] for slot-4 lookup, and the game will eventually free
/// it via the donor's scalar-deleting destructor once the OptionTab
/// container is torn down. Callers MUST NOT free the pointer themselves.
pub(crate) fn allocate_row_for_option(handle: OptionHandle, side: u8) -> Option<*mut u8> {
    if !is_ready() {
        log_warn!("custom_options/rows: allocate_row_for_option called before init ready");
        return None;
    }
    if side >= 2 {
        log_warn!("custom_options/rows: invalid side {side} (must be 0 or 1)");
        return None;
    }

    // Snapshot the id + ui-kind summary we need from the registry.
    // We drop the lock before the game-allocator call so that allocation or
    // ctor hooks can't accidentally recurse into custom_options state under
    // the same lock.
    let (id, is_enum) = {
        let state = registry::STATE.lock().unwrap();
        let idx = handle.0 as usize;
        if idx >= state.options.len() {
            log_error!(
                "custom_options/rows: handle {:?} out of range (registered={})",
                handle,
                state.options.len()
            );
            return None;
        }
        let opt = &state.options[idx];
        (opt.id.clone(), matches!(opt.ui_kind, UiKind::Enum { .. }))
    };

    if !is_enum {
        // Scalar rows go through allocate_scalar_row_for_option; silently
        // skip here so the caller can route by kind without double-logging.
        return None;
    }

    unsafe {
        let game_malloc = FN_GAME_MALLOC?;
        let donor_ctor = FN_DONOR_CTOR?;

        // 1. Allocate and zero the row.
        let row = game_malloc(ROW_SIZE);
        if row.is_null() {
            log_error!("custom_options/rows: game_malloc({ROW_SIZE}) failed for option {id:?}");
            return None;
        }
        std::ptr::write_bytes(row, 0, ROW_SIZE);

        // 2. Run the donor ctor. Initializes all 4 vtables and every
        // field; we overwrite only the primary vtable below.
        donor_ctor(row);

        // 3. Synthesize the mod vtable: 7 slots verbatim + slot 4 override.
        let vtable = build_mod_vtable();
        if vtable.is_null() {
            log_error!("custom_options/rows: failed to allocate mod vtable for option {id:?}");
            return None;
        }

        // 4. Overwrite primary vtable pointer at row+0x00.
        memory::write_ptr(row.add(ROW_PRIMARY_VTABLE_OFFSET), vtable as *const u8);

        // 4b. Clone + override the IOptionElement vtable (row+0xC0) so the
        // options preview box renders seop_image_<id> for this row. Best-
        // effort: a failure leaves a blank preview but the row still works.
        if install_ioptionelement_vtable(row).is_null() {
            log_warn!("custom_options/rows: IOptionElement vtable clone failed for {id:?} — preview image disabled");
        }

        // 5. Tag with Page6 (all custom options live on the Mods tab).
        if !write_page_tag(row, PageTag::Page6) {
            log_warn!("custom_options/rows: failed to tag row for {id:?} with Page6");
        }

        // 6. Record for slot-4 dispatch and lifetime tracking.
        {
            let mut rows = ROWS.lock().unwrap();
            rows.push(RowSlot {
                handle,
                side,
                row_ptr: row,
                kind: RowKind::Enum,
                _vtable: vtable,
                last_value_text: Mutex::new(None),
                marker_anim: Mutex::new(f64::NAN),
            });
        }

        log_info!(
            "custom_options/rows: allocated row for {id:?} side={side} @ {:p} vtable={:p} slot4={:p}",
            row,
            vtable,
            *(vtable.add(SLOT_ADVANCE_VALUE)),
        );
        log_debug!(
            "custom_options/rows: mod vtable for {id:?} slots = [{:p}, {:p}, {:p}, {:p}, {:p}, {:p}, {:p}, {:p}]",
            *vtable.add(0), *vtable.add(1), *vtable.add(2), *vtable.add(3),
            *vtable.add(4), *vtable.add(5), *vtable.add(6), *vtable.add(7),
        );

        Some(row)
    }
}

/// Allocate a scalar row for the option identified by `handle`, for `side`.
/// Scalar counterpart of [`allocate_row_for_option`] — uses the
/// `OptionElement<int>` donor ctor (so the inherited fourth-MI slot-0
/// visibility handler creates the AFP layer + value/label TextLayer
/// shared_ptrs on show), and overrides primary slot 4 with
/// [`advance_value_scalar_trampoline`] and primary slot 7 with
/// [`render_scalar_trampoline`]. Primary slot 6 (`onCreate`) is a no-op
/// so the mod row opts out of per-KIND reactive-stream wiring.
///
/// Returns `None` if the module isn't scalar-ready (missing signatures),
/// if the option isn't a `UiKind::Scalar`, or if allocation fails.
pub(crate) fn allocate_scalar_row_for_option(handle: OptionHandle, side: u8) -> Option<*mut u8> {
    if !is_scalar_ready() {
        log_warn!(
            "custom_options/rows: allocate_scalar_row_for_option called before scalar path ready"
        );
        return None;
    }
    if side >= 2 {
        log_warn!("custom_options/rows: invalid side {side} (must be 0 or 1)");
        return None;
    }

    let (id, is_scalar) = {
        let state = registry::STATE.lock().unwrap();
        let idx = handle.0 as usize;
        if idx >= state.options.len() {
            log_error!(
                "custom_options/rows: handle {:?} out of range (registered={})",
                handle,
                state.options.len()
            );
            return None;
        }
        let opt = &state.options[idx];
        (opt.id.clone(), matches!(opt.ui_kind, UiKind::Scalar { .. }))
    };

    if !is_scalar {
        return None;
    }

    unsafe {
        let game_malloc = FN_GAME_MALLOC?;
        let donor_ctor_int = FN_DONOR_CTOR_INT?;

        let row = game_malloc(ROW_SIZE);
        if row.is_null() {
            log_error!(
                "custom_options/rows: game_malloc({ROW_SIZE}) failed for scalar option {id:?}"
            );
            return None;
        }
        std::ptr::write_bytes(row, 0, ROW_SIZE);

        // Run the OptionElement<int> donor ctor. Initializes all 4 MI
        // vtables (so the fourth-MI slot-0 visibility handler is live)
        // and every instance field. We override only the primary vtable
        // below; the fourth MI vtable stays intact so TextLayer creation
        // fires when the row becomes visible on the Mods tab.
        donor_ctor_int(row);

        let donor_vtable_int = std::ptr::read(std::ptr::addr_of!(DONOR_PRIMARY_VTABLE_INT));
        if donor_vtable_int.is_null() {
            log_error!(
                "custom_options/rows: DONOR_PRIMARY_VTABLE_INT went null after is_scalar_ready check — logic bug"
            );
            return None;
        }

        let vtable = build_mod_vtable_scalar(donor_vtable_int);
        if vtable.is_null() {
            log_error!(
                "custom_options/rows: failed to allocate scalar mod vtable for option {id:?}"
            );
            return None;
        }

        memory::write_ptr(row.add(ROW_PRIMARY_VTABLE_OFFSET), vtable as *const u8);

        // Clone + override the IOptionElement vtable (row+0xC0) for the
        // preview-image box, same as the enum path. Best-effort.
        if install_ioptionelement_vtable(row).is_null() {
            log_warn!("custom_options/rows: IOptionElement vtable clone failed for scalar {id:?} — preview image disabled");
        }

        if !write_page_tag(row, PageTag::Page6) {
            log_warn!("custom_options/rows: failed to tag scalar row for {id:?} with Page6");
        }

        {
            let mut rows = ROWS.lock().unwrap();
            rows.push(RowSlot {
                handle,
                side,
                row_ptr: row,
                kind: RowKind::Scalar,
                _vtable: vtable,
                last_value_text: Mutex::new(None),
                marker_anim: Mutex::new(f64::NAN),
            });
        }

        log_info!(
            "custom_options/rows: allocated scalar row for {id:?} side={side} @ {:p} vtable={:p} slot4={:p} slot7={:p}",
            row,
            vtable,
            *(vtable.add(SLOT_ADVANCE_VALUE)),
            *(vtable.add(SLOT_RENDER)),
        );
        log_debug!(
            "custom_options/rows: scalar mod vtable for {id:?} slots = [{:p}, {:p}, {:p}, {:p}, {:p}, {:p}, {:p}, {:p}]",
            *vtable.add(0), *vtable.add(1), *vtable.add(2), *vtable.add(3),
            *vtable.add(4), *vtable.add(5), *vtable.add(6), *vtable.add(7),
        );

        Some(row)
    }
}

/// Allocate a header row for the option identified by `handle`, for `side`.
/// Header counterpart of [`allocate_row_for_option`] — same ArrowColor donor
/// clone (so the inherited fourth-MI visibility handler creates the AFP row
/// clip on show), then the header mutations from
/// `docs/option_header_rows_research.md` §4:
///
///   1. `row+0x28` → the shared `{return 0, no-op}` selectability interface
///      ([`header_selectable_vtable`]) — every cursor path skips the row.
///   2. Primary slot 7 → [`render_header_trampoline`] (label-only render);
///      slots 4/6 no-ops.
///
/// The row keeps the standard full-height slot (a half-height variant was
/// tried and dropped 2026-08-15: the `+0xA8` y-extent halving shrinks only
/// the layout slot, not the clip art, and art-side scaling compromised the
/// label — maintainer settled on full height with the header look carried
/// entirely by the label texture).
///
/// The `+0xC0` IOptionElement preview vtable is NOT cloned: the preview
/// getter runs only on the focused row and headers can never be focused
/// (the donor's native slot returns `""` safely if anything unforeseen asks).
///
/// Fail-open (design §6): any failure returns `None` with a WARN — the
/// header is simply absent, normal rows unaffected.
pub(crate) fn allocate_header_row_for_option(handle: OptionHandle, side: u8) -> Option<*mut u8> {
    if !is_ready() {
        log_warn!("custom_options/rows: allocate_header_row_for_option called before init ready");
        return None;
    }
    if side >= 2 {
        log_warn!("custom_options/rows: invalid side {side} (must be 0 or 1)");
        return None;
    }

    let (id, is_header) = {
        let state = registry::STATE.lock().unwrap();
        let idx = handle.0 as usize;
        if idx >= state.options.len() {
            log_error!(
                "custom_options/rows: handle {:?} out of range (registered={})",
                handle,
                state.options.len()
            );
            return None;
        }
        let opt = &state.options[idx];
        (opt.id.clone(), matches!(opt.ui_kind, UiKind::Header))
    };

    if !is_header {
        return None;
    }

    unsafe {
        let game_malloc = FN_GAME_MALLOC?;
        let donor_ctor = FN_DONOR_CTOR?;

        let row = game_malloc(ROW_SIZE);
        if row.is_null() {
            log_error!(
                "custom_options/rows: game_malloc({ROW_SIZE}) failed for header {id:?} — header absent"
            );
            return None;
        }
        std::ptr::write_bytes(row, 0, ROW_SIZE);

        // Donor ctor initializes all 4 MI vtables and every field; we then
        // overwrite the primary and the +0x28 interface below.
        donor_ctor(row);

        // Non-selectable: swap the +0x28 interface for the shared mod table
        // (built on first use from the donor-written table's COL).
        let donor_iface = *(row.add(ROW_SELECTABLE_IFACE_OFFSET) as *const *const *const u8);
        let iface_vtable = header_selectable_vtable(donor_iface);
        if iface_vtable.is_null() {
            log_warn!(
                "custom_options/rows: header selectability vtable unavailable for {id:?} — header absent"
            );
            return None;
        }
        memory::write_ptr(
            row.add(ROW_SELECTABLE_IFACE_OFFSET),
            iface_vtable as *const u8,
        );

        let vtable = build_mod_vtable_header();
        if vtable.is_null() {
            log_warn!(
                "custom_options/rows: failed to allocate header vtable for {id:?} — header absent"
            );
            return None;
        }
        memory::write_ptr(row.add(ROW_PRIMARY_VTABLE_OFFSET), vtable as *const u8);

        if !write_page_tag(row, PageTag::Page6) {
            log_warn!("custom_options/rows: failed to tag header row for {id:?} with Page6");
        }

        {
            let mut rows = ROWS.lock().unwrap();
            rows.push(RowSlot {
                handle,
                side,
                row_ptr: row,
                kind: RowKind::Header,
                _vtable: vtable,
                last_value_text: Mutex::new(None),
                marker_anim: Mutex::new(f64::NAN),
            });
        }

        log_info!(
            "custom_options/rows: allocated header row for {id:?} side={side} @ {:p} vtable={:p} iface={:p}",
            row,
            vtable,
            iface_vtable,
        );

        Some(row)
    }
}

// ── Texture binding (called from builder_hook after injection) ──────────

/// Bind the label and value textures on a row's sub-MC children. Called
/// once from the builder hook immediately after the row is registered in
/// the scene graph. At this point the AFP context is live and the sub-MC's
/// mc_id is valid.
/// Bind the label texture on a row's sub-MC. Called once from the builder
/// hook immediately after the row is registered in the scene graph.
///
/// The `option_usr` child layer starts hidden (no `onCreate` wiring), so
/// it needs a visibility toggle before the bitmap bind takes effect.
///
/// Value textures (ON/OFF) are NOT bound here — the value display
/// requires either `onCreate` reactive-stream wiring or dynamic sub-clip
/// instantiation under `choice_usr`, neither of which is available
/// without the donor's full initialization pipeline. Value display is
/// tracked as a follow-up.
pub(crate) fn bind_textures(handle: OptionHandle, row: *mut u8) {
    use crate::services::bm2d_api;

    unsafe {
        let sub_mc_ptr = *(row.add(0x118) as *const *const u8);
        if sub_mc_ptr.is_null() {
            return;
        }
        let mc_id = *(sub_mc_ptr.add(0x08) as *const u32);

        let label_texture = {
            let state = registry::STATE.lock().unwrap();
            let idx = handle.0 as usize;
            if idx >= state.options.len() {
                return;
            }
            state.options[idx].label_texture_name()
        };

        if !label_texture.is_empty() {
            if let Some(layer) = bm2d_api::layer_find_child(mc_id, "option_usr") {
                bm2d_api::mc_set_param(layer, 0x1007, 1);
                bm2d_api::mc_set_param(layer, 0x101e, 1);
                bm2d_api::mc_load_bitmap(layer, &label_texture);
            }
        }
    }
}

// ── Mod vtable synthesis ─────────────────────────────────────────────────

/// Allocate a primary vtable in RWX memory, copy slots 0/1/2/3/5/6/7 from
/// the donor vtable, and install our trampoline at slot 4.
///
/// MSVC C++ vtables carry an RTTI `CompleteObjectLocator` pointer at
/// `vtable[-1]`. `__RTDynamicCast` (used by the game during row
/// registration to check whether the row implements secondary interfaces
/// like `IResourceSharing`) reads through that negative slot; omitting
/// it causes the cast to walk uninitialized memory and raise a C++
/// exception. The backing buffer therefore reserves one extra qword at
/// the front, copies the donor's `[-1]` slot into it, and the returned
/// pointer is offset so that callers' `vtable[0..7]` indexing stays
/// correct without any index math.
///
/// The vtable is allocated via VirtualAlloc (the game never frees it;
/// it's tied to the process lifetime).
unsafe fn build_mod_vtable() -> *mut *const u8 {
    let donor_vtable = std::ptr::read(std::ptr::addr_of!(DONOR_PRIMARY_VTABLE));
    if donor_vtable.is_null() {
        return std::ptr::null_mut();
    }

    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed((PRIMARY_VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    // Physical backing: [col_ptr, slot_0, slot_1, ..., slot_7]
    // Returned pointer: points at slot_0 so callers' vtable[i] matches
    // MSVC's virtual-slot indexing; vtable[-1] reads the COL pointer.
    let backing = raw as *mut *const u8;
    let vtable = backing.add(1);

    // Copy the donor's COL pointer into vtable[-1].
    let donor_col = *donor_vtable.offset(-1);
    *backing = donor_col;

    for i in 0..PRIMARY_VTABLE_SLOTS {
        let donor_slot = *donor_vtable.add(i);
        *vtable.add(i) = donor_slot;
    }

    *vtable.add(SLOT_ADVANCE_VALUE) = advance_value_enum_trampoline as *const u8;
    *vtable.add(SLOT_ON_CREATE) = noop_virtual_trampoline as *const u8;
    *vtable.add(SLOT_RENDER) = render_enum_trampoline as *const u8;

    vtable
}

/// Synthesize the scalar primary vtable from the `OptionElement<int>`
/// donor. Same structural layout as [`build_mod_vtable`] — COL pointer at
/// the head, 8 function slots — but:
///
/// - Slot 4 points at [`advance_value_scalar_trampoline`] so left/right
///   presses dispatch step-based advance through the mod registry.
/// - Slot 6 is a no-op — skips the donor's reactive-stream wiring
///   (Learning 7) the same way enum rows do.
/// - Slot 7 points at [`render_scalar_trampoline`], which delegates to
///   the donor's native slot 7 (position pinning + TextLayer tick) and
///   then pushes the formatted value text via `textlayer_set_text`.
/// - Slots 0/1/2/3/5 are inherited verbatim from the donor (dtor,
///   initIndex, onReset, onTick are fine as-is).
/// - Fourth MI vtable at `row+0xC8` is NOT touched — stays pointing at
///   the donor's `OptionElement<int>` fourth MI vtable so its slot-0
///   visibility handler (`FUN_180178c50`) fires on row show and creates
///   the AFP layer + TextLayer shared_ptrs.
unsafe fn build_mod_vtable_scalar(donor_vtable: *const *const u8) -> *mut *const u8 {
    if donor_vtable.is_null() {
        return std::ptr::null_mut();
    }

    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed((PRIMARY_VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    let backing = raw as *mut *const u8;
    let vtable = backing.add(1);

    let donor_col = *donor_vtable.offset(-1);
    *backing = donor_col;

    for i in 0..PRIMARY_VTABLE_SLOTS {
        let donor_slot = *donor_vtable.add(i);
        *vtable.add(i) = donor_slot;
    }

    *vtable.add(SLOT_ADVANCE_VALUE) = advance_value_scalar_trampoline as *const u8;
    *vtable.add(SLOT_ON_CREATE) = noop_virtual_trampoline as *const u8;
    *vtable.add(SLOT_RENDER) = render_scalar_trampoline as *const u8;

    vtable
}

// ── Header rows (non-selectable group headings) ──────────────────────────

/// ONE process-lifetime `+0x28` selectability-interface vtable shared by all
/// header rows: slot 0 returns 0 (non-selectable — the predicate every cursor
/// path tests), slot 1 is a no-op (`onFocusChanged` is unreachable once
/// unfocusable, but stubbed so no donor state-dependency can ever fire —
/// research §7). Built lazily by [`header_selectable_vtable`] from the first
/// header row's donor-written interface table; leaked (VirtualAlloc, process
/// lifetime) like every other synthesized vtable here.
static mut HEADER_SELECTABLE_VTABLE: *mut *const u8 = std::ptr::null_mut();

/// Slot 0 of the header `+0x28` interface: `bool isSelectable(this)`,
/// hardcoded false. `this` is the MI subobject (`row + 0x28`), unused.
/// A mod stub, NOT the native `return 0` body — pointing our table at native
/// addresses would add signature dependencies for nothing (research §7).
/// Provably panic-free.
unsafe extern "C" fn header_not_selectable_trampoline(_this: *mut u8) -> i32 {
    0
}

/// Slot 1 of the header `+0x28` interface: `void onFocusChanged(this, bool)`.
/// Unreachable (headers can never gain or lose focus) but stubbed with mod
/// code so the table has no dangling slot. Provably panic-free.
unsafe extern "C" fn header_focus_noop_trampoline(_this: *mut u8, _focused: u8) {}

/// Get (building on first use) the shared header selectability vtable.
///
/// `donor_iface_vtable` is the donor-ctor-written table read from
/// `row+0x28` — used only to source the RTTI CompleteObjectLocator copied
/// into our table's `[-1]` slot, same COL discipline as
/// [`build_mod_vtable`] (RTTI walks read through the negative slot; both
/// function slots are mod stubs). Returns null when the donor pointer is
/// null or the one-time allocation fails — callers degrade (header absent).
unsafe fn header_selectable_vtable(donor_iface_vtable: *const *const u8) -> *mut *const u8 {
    let existing = std::ptr::read(std::ptr::addr_of!(HEADER_SELECTABLE_VTABLE));
    if !existing.is_null() {
        return existing;
    }
    if donor_iface_vtable.is_null() {
        return std::ptr::null_mut();
    }

    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed((SELECTABLE_VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    // Physical backing: [col_ptr, slot_0, slot_1]; the returned pointer
    // points at slot_0 so vtable[-1] reads the COL.
    let backing = raw as *mut *const u8;
    let vtable = backing.add(1);
    *backing = *donor_iface_vtable.offset(-1);
    *vtable = header_not_selectable_trampoline as *const u8;
    *vtable.add(1) = header_focus_noop_trampoline as *const u8;

    std::ptr::write(std::ptr::addr_of_mut!(HEADER_SELECTABLE_VTABLE), vtable);
    vtable
}

/// Synthesize the header primary vtable from the ArrowColor donor. Same
/// structural layout as [`build_mod_vtable`] (COL at the head, 8 slots) but
/// slot 4 (`advanceValue`) is a no-op alongside slot 6 — headers hold no
/// value and can never be focused, so nothing should ever advance — and
/// slot 7 renders the label-only header body.
unsafe fn build_mod_vtable_header() -> *mut *const u8 {
    let donor_vtable = std::ptr::read(std::ptr::addr_of!(DONOR_PRIMARY_VTABLE));
    if donor_vtable.is_null() {
        return std::ptr::null_mut();
    }

    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed((PRIMARY_VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    let backing = raw as *mut *const u8;
    let vtable = backing.add(1);
    *backing = *donor_vtable.offset(-1);
    for i in 0..PRIMARY_VTABLE_SLOTS {
        *vtable.add(i) = *donor_vtable.add(i);
    }

    *vtable.add(SLOT_ADVANCE_VALUE) = noop_virtual_trampoline as *const u8;
    *vtable.add(SLOT_ON_CREATE) = noop_virtual_trampoline as *const u8;
    *vtable.add(SLOT_RENDER) = render_header_trampoline as *const u8;

    vtable
}

/// Clone the row's `IOptionElement` MI vtable (at `row+0xC0`) into
/// process-lifetime RWX memory, override slot 0 with
/// [`preview_image_name_trampoline`], and write the cloned pointer back to
/// `row+0xC0`. Shared by enum and scalar rows — the donor ctor already
/// populated `row+0xC0` with the donor kind's `IOptionElement` vtable, so we
/// read the current pointer (rather than a separate signature) and clone it.
///
/// Without this, the options menu's preview-image box calls the donor's slot
/// 0, which returns `""` because our rows leave the value-model self-pointer
/// (`row+0x110`) null — so the preview renders blank. The override writes
/// `seop_image_<id>` from the registry instead.
///
/// COL discipline mirrors [`build_mod_vtable`]: reserve one extra qword at
/// the head for the donor's `vtable[-1]` RTTI Complete Object Locator, so
/// the `__RTDynamicCast(Component -> IOptionElement)` the menu performs on
/// the focused row keeps working. Returns the cloned vtable pointer (kept
/// alive for slot-table accounting) or null on allocation failure — callers
/// log and continue (the row still works, just with a blank preview).
unsafe fn install_ioptionelement_vtable(row: *mut u8) -> *mut *const u8 {
    let slot_ptr = row.add(ROW_IOPTIONELEMENT_VTABLE_OFFSET) as *mut *const *const u8;
    let donor_vtable = *slot_ptr;
    if donor_vtable.is_null() {
        return std::ptr::null_mut();
    }

    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed((IOPTIONELEMENT_VTABLE_SLOTS + 1) * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }

    // Physical backing: [col_ptr, slot_0, ..., slot_7]; returned/installed
    // pointer points at slot_0 so vtable[-1] reads the COL.
    let backing = raw as *mut *const u8;
    let vtable = backing.add(1);
    *backing = *donor_vtable.offset(-1);
    for i in 0..IOPTIONELEMENT_VTABLE_SLOTS {
        *vtable.add(i) = *donor_vtable.add(i);
    }
    *vtable.add(SLOT_PREVIEW_IMAGE_NAME) = preview_image_name_trampoline as *const u8;

    // Swap the row's IOptionElement vtable pointer to our clone.
    *slot_ptr = vtable as *const *const u8;
    vtable
}

/// Catch-all no-op for virtual slots we don't want to inherit from the
/// donor. `extern "C"` + panic-safe (no Rust code that can unwind past
/// the boundary). Used for slot 6 (`onCreate`) on every mod row's
/// primary vtable — prevents reactive-stream wiring that would crash
/// without per-KIND subscriber state.
unsafe extern "C" fn noop_virtual_trampoline(_this: *mut u8, _a: *mut u8, _b: *mut u8) {}

// ── IOptionElement slot-0 override (preview-image name getter) ───────────

/// `IOptionElement::getPreviewImageName`, installed at slot 0 of the cloned
/// `row+0xC0` vtable. Called by the options menu's preview-image observer on
/// the focused row to fetch the `seop_image_*` texture shown in the gray
/// preview box below the rows.
///
/// Calling convention (matches the native getter `FUN_18017a170`):
///   RCX (`this`) — the `IOptionElement` MI subobject, i.e. `row + 0xC0`.
///   RDX (`out`)  — a caller-owned, UNINITIALIZED MSVC `std::string` the
///                  getter must populate and return. The native getter
///                  initializes it to empty-SSO then `string::assign`s; we
///                  do the same. Ownership/free stays with the caller, so we
///                  do NOT destruct it here.
/// Returns `out` (RAX), per the native ABI.
///
/// FFI-boundary safety: `extern "C"` callback from game code; the body is
/// panic-caught. On any failure we still return an initialized empty string
/// so the caller's downstream dirty-check / bind path sees a valid object.
unsafe extern "C" fn preview_image_name_trampoline(this: *mut u8, out: *mut u8) -> *mut u8 {
    // Initialize `out` to a valid empty string FIRST, unconditionally — three
    // provably panic-free raw writes. A panic below (e.g. a poisoned lock in
    // resolve_focused_preview) is caught, but without this pre-init the game
    // would receive an UNINITIALIZED MSVC std::string and its dirty-check /
    // destructor would read garbage SSO fields — UB inside game code.
    unsafe { init_empty_sso(out) };

    // Compute the name and (for a mod row) the focused-row identity under the
    // ROWS/STATE locks, write the out-string, then drop the locks — all inside
    // catch_unwind. The focus event is fired AFTER, with the locks released,
    // so a subscriber (e.g. the WebUI preview overlay) can re-enter
    // custom_options (get_value, etc.) without deadlocking on ROWS/STATE.
    let focused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resolve_focused_preview(this).map(|fp| {
            // `out` is already the empty string; only a real name needs the
            // assign (empty = hide-the-box per the binder contract).
            if let Some(name) = fp.name.as_deref() {
                write_out_string(out, name);
            }
            (fp.side, fp.option_id)
        })
    }))
    .ok()
    .flatten();

    // Focus signal for on-demand preview art (WebUI overlay). Fires on the
    // render thread with ROWS/STATE unlocked; panic-isolated inside the fire.
    if let Some((side, option_id)) = focused {
        super::fire_preview_request(side, &option_id);
    }
    out
}

/// The focused mod row's identity plus its resolved preview-image name.
struct FocusedPreview {
    side: u8,
    option_id: String,
    /// The `seop_image_*` name to bind, or `None` if this value has no shipped
    /// preview PNG (getter then writes `""` so the native binder hides the box
    /// rather than leaving the previously-focused row's image showing).
    name: Option<String>,
}

/// Resolve the focused mod row (whose `IOptionElement` subobject is `this` =
/// `row + 0xC0`) into its side, option id, and preview-image name. Returns
/// `None` if the row isn't one of ours or its option is gone. Acquires and
/// releases the `ROWS`/`STATE` locks internally — the caller must fire any
/// event with them dropped.
fn resolve_focused_preview(this: *mut u8) -> Option<FocusedPreview> {
    let row = (this as usize).checked_sub(ROW_IOPTIONELEMENT_VTABLE_OFFSET)? as *mut u8;
    let rows = ROWS.lock().unwrap();
    let slot = rows.iter().find(|r| r.row_ptr == row)?;
    let side = slot.side;
    let state = registry::STATE.lock().unwrap();
    let opt = state.options.get(slot.handle.0 as usize)?;
    let option_id = opt.id.clone();
    // Key the preview off the row's current per-player value so enum rows
    // with per-value `preview_key`s switch art as the player cycles; scalar
    // and keyless enum values fall back to the base `seop_image_<id>`.
    let name = opt.preview_image_name_for_value(opt.values[side as usize]);
    // Only hand the binder a name we actually injected. If this value has no
    // shipped preview PNG, `name` is None → the getter writes "" → the native
    // binder hides the preview box (instead of failing to bind and leaving
    // the previously-focused row's image showing).
    let name = if super::asset_gen::preview_is_available(&name) {
        Some(name)
    } else {
        None
    };
    Some(FocusedPreview {
        side,
        option_id,
        name,
    })
}

/// Initialize `out` (a caller-owned, uninitialized MSVC `std::string`) to an
/// empty SSO string: bytes[0]=0, size=0, capacity=0xF (SSO sentinel). Three
/// raw writes — provably panic-free, so it's safe to run BEFORE any fallible
/// work in an `extern "C"` callback (the panic-path pre-init in the slot-0
/// trampoline depends on this).
unsafe fn init_empty_sso(out: *mut u8) {
    *out = 0;
    memory::write_u64(out.add(SSO_SIZE_OFFSET), 0);
    memory::write_u64(out.add(SSO_CAPACITY_OFFSET), SSO_CAPACITY_SENTINEL);
}

/// Initialize `out` to an empty SSO string and assign `text` into it via the
/// game's `string::assign` (so any heap promotion uses the game allocator).
/// The caller owns `out` and will destruct it — we must not free it here.
/// Falls back to leaving an empty-but-initialized string if `string::assign`
/// is unavailable. Safe to call on an already-initialized empty SSO string
/// (re-init is idempotent).
unsafe fn write_out_string(out: *mut u8, text: &str) {
    init_empty_sso(out);
    if text.is_empty() {
        return;
    }
    if let Some(string_assign) = FN_STRING_ASSIGN {
        string_assign(out, text.as_ptr(), text.len());
    }
}

// ── Slot-7 override (render) ────────────────────────────────────────────

/// Custom render for mod option rows. Two responsibilities:
///
/// 1. **Position pinning**: re-apply `row+0x88/+0x90` (plus ancestor
///    offsets from the parent chain at `+0x60`) to the sub-MC's AFP
///    layer every frame via `afp_layer_set_position`. Without this the
///    AFP scene graph can drift the sub-MC's position on tab
///    transitions (the native render does the same at `FUN_18016f8f0`).
///
/// 2. **Texture binding**: bind label and value textures to the sub-MC
///    children each frame.
unsafe extern "C" fn render_enum_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| render_enum(this));
}

fn render_enum(this: *mut u8) {
    use crate::services::bm2d_api;

    unsafe {
        let sub_mc_ptr = *(this.add(0x118) as *const *const u8);
        if sub_mc_ptr.is_null() {
            return;
        }
        let mc_id = *(sub_mc_ptr.add(0x08) as *const u32);

        // Position pinning — mirrors the native render's per-frame
        // sub_mc->vtable[6](sub_mc, &xy) call. Accumulate row position
        // (row+0x88/+0x90) plus ancestor offsets from the parent chain.
        let mut x = *(this.add(0x88) as *const f64);
        let mut y = *(this.add(0x90) as *const f64);
        let parent = *(this.add(0x60) as *const *const u8);
        if !parent.is_null() {
            let (ax, ay) = accumulate_ancestor_xy(parent);
            x += ax;
            y += ay;
        }
        bm2d_api::set_position(mc_id, x as f32, y as f32);

        // Texture binding. Bind the label and value textures each frame
        // since mc_ids can change on tab switch. Also snapshot the
        // (current index, value count) the marker needs — same lock.
        let (label_texture, value_texture, marker) = {
            let rows = ROWS.lock().unwrap();
            match rows.iter().find(|r| r.row_ptr == this) {
                Some(r) => {
                    let state = registry::STATE.lock().unwrap();
                    let idx = r.handle.0 as usize;
                    if idx >= state.options.len() {
                        return;
                    }
                    let opt = &state.options[idx];
                    let label = opt.label_texture_name();
                    let current_value = opt.values[r.side as usize];
                    let (value_tex, marker) = match &opt.ui_kind {
                        UiKind::Enum { allowed_values } => {
                            let cur_idx = allowed_values
                                .iter()
                                .position(|v| v.value == current_value)
                                .unwrap_or(0);
                            let tex = allowed_values
                                .get(cur_idx)
                                .map(|v| v.label_texture_name.clone());
                            // Step the eased marker position under the same
                            // lock so the per-row anim state stays serialized.
                            let pos = step_marker_anim(
                                &mut r.marker_anim.lock().unwrap(),
                                cur_idx,
                                allowed_values.len(),
                            );
                            (tex, pos)
                        }
                        _ => (None, None),
                    };
                    (label, value_tex, marker)
                }
                None => return,
            }
        };

        if !label_texture.is_empty() {
            if let Some(layer) = bm2d_api::layer_find_child(mc_id, "option_usr") {
                bm2d_api::mc_set_param(layer, 0x1007, 1);
                bm2d_api::mc_set_param(layer, 0x101e, 1);
                bm2d_api::mc_load_bitmap(layer, &label_texture);
            }
        }

        if let Some(val_tex) = &value_texture {
            if !val_tex.is_empty() {
                if let Some(layer) = bm2d_api::layer_find_child(mc_id, "choice_usr/choice_usr") {
                    bm2d_api::mc_set_param(layer, 0x1007, 1);
                    bm2d_api::mc_set_param(layer, 0x101e, 1);
                    bm2d_api::mc_load_bitmap(layer, val_tex);
                }
            }
        }

        // Drive the value-position marker (the green bar at the bottom of the
        // selector) and the left/right cycling arrows. Mirrors the native
        // slot-7 marker block — see `docs/option_row_marker_render.md`. For an
        // enum row, (index, count) come straight from the position in
        // `allowed_values`. Pinning the marker every frame is also what stops
        // it free-running its intro sweep animation. The scalar path drives
        // the same marker from its {min, max, step_fine} range.
        if let Some(m) = marker {
            drive_value_marker(mc_id, &m);
        }
    }
}

/// Header row's slot-7 render: position pinning + the full-width label bind,
/// and nothing else — no value chip, no marker, no tri-arrows, no preview
/// (design §4.8 / R10). The donor clip's value-box art (`choice_usr` and
/// everything under it, including the marker bar and both arrows) is hidden
/// each frame so the row reads as a bare group heading. The header's entire
/// look is the label texture itself — an opaque full-width bar
/// (`seop_item_<id>`, dark blue with white centered text) that paints over
/// the donor clip's default gray frame art.
unsafe extern "C" fn render_header_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| render_header(this));
}

fn render_header(this: *mut u8) {
    use crate::services::bm2d_api;

    unsafe {
        let sub_mc_ptr = *(this.add(0x118) as *const *const u8);
        if sub_mc_ptr.is_null() {
            return;
        }
        let mc_id = *(sub_mc_ptr.add(0x08) as *const u32);

        // Position pinning — same per-frame re-apply as the enum render
        // (the AFP scene graph can drift the sub-MC on tab transitions).
        // No nudge: the label bitmap sits at the row's text-zone origin
        // like every other label, with margin on all four sides (the
        // maintainer-picked centered-strip look; full-box variants were
        // tried 2026-08-15 and bled into the next row — the text-zone
        // origin sits below the row's box grid line, so a taller bitmap
        // cannot fill the box without overshooting it).
        let mut x = *(this.add(0x88) as *const f64);
        let mut y = *(this.add(0x90) as *const f64);
        let parent = *(this.add(0x60) as *const *const u8);
        if !parent.is_null() {
            let (ax, ay) = accumulate_ancestor_xy(parent);
            x += ax;
            y += ay;
        }
        bm2d_api::set_position(mc_id, x as f32, y as f32);

        // Label bind, each frame (mc_ids can change on tab switch).
        let label_texture = {
            let rows = ROWS.lock().unwrap();
            match rows.iter().find(|r| r.row_ptr == this) {
                Some(r) => {
                    let state = registry::STATE.lock().unwrap();
                    match state.options.get(r.handle.0 as usize) {
                        Some(opt) => opt.label_texture_name(),
                        None => return,
                    }
                }
                None => return,
            }
        };
        if !label_texture.is_empty() {
            if let Some(layer) = bm2d_api::layer_find_child(mc_id, "option_usr") {
                bm2d_api::mc_set_param(layer, 0x1007, 1);
                bm2d_api::mc_set_param(layer, 0x101e, 1);
                bm2d_api::mc_load_bitmap(layer, &label_texture);
            }
        }

        // Hide the donor clip's entire value-selector subtree (value box art,
        // chip, marker bar, tri-arrows). Hiding the parent clip hides its
        // children; re-applied per frame for the same tab-switch reason as
        // the label bind.
        if let Some(layer) = bm2d_api::layer_find_child(mc_id, "choice_usr") {
            bm2d_api::mc_set_param(layer, 0x1007, 0);
            bm2d_api::mc_set_param(layer, 0x101e, 1);
        }

        // Hide the gray-out cover. The donor clip's authored default state
        // shows `invalid_usr` (the disabled-row darkener); ordinary rows
        // have it cleared by the donor's `onFocusChanged(false)` at menu
        // build, but a header's `+0x28` slot-1 is deliberately a no-op, so
        // without this the whole row renders darkened (cabinet-observed
        // 2026-08-15).
        if let Some(layer) = bm2d_api::layer_find_child(mc_id, "invalid_usr") {
            bm2d_api::mc_set_param(layer, 0x1007, 0);
            bm2d_api::mc_set_param(layer, 0x101e, 1);
        }
    }
}

/// Marker geometry, lifted verbatim from the native slot-7 render
/// (`FUN_1801754c0` on 20260526). See `docs/option_row_marker_render.md`.
const MARKER_TRACK: f64 = 1.0; // full normalized bar track (DAT_18038f1c0)
const MARKER_MIN_WIDTH: f64 = 0.2; // floor on width fraction (DAT_18038f358)
const MARKER_PX_SCALE: f64 = 100.0; // normalized -> 0x0f04 units (DAT_18038eb20)
const MARKER_EASE: f64 = 0.5; // per-frame lerp toward target (DAT_18038eb88)

/// One frame of marker state for an enum row: the eased bar position and the
/// fixed bar width (both as normalized fractions), plus the current index and
/// count for the cycling-arrow visibility.
struct MarkerState {
    pos: f64,
    width: f64,
    index: usize,
    count: usize,
}

/// Advance the eased marker position toward the target for `(index, count)`
/// and return the `MarkerState` to render, or `None` when there's nothing to
/// draw (fewer than 2 values — matches the native render, which only draws the
/// bar for multi-value options). `anim` is the per-row eased state
/// (`f64::NAN` = uninitialized; first frame snaps to target).
fn step_marker_anim(anim: &mut f64, index: usize, count: usize) -> Option<MarkerState> {
    if count < 2 {
        return None;
    }
    let width = MARKER_MIN_WIDTH.max(MARKER_TRACK / count as f64);
    let target = (MARKER_TRACK - width) * (index as f64) / ((count - 1) as f64);
    let pos = if anim.is_nan() {
        target
    } else {
        (target - *anim) * MARKER_EASE + *anim
    };
    *anim = pos;
    Some(MarkerState {
        pos,
        width,
        index,
        count,
    })
}

/// Derive `(index, count)` for a scalar row's marker from its `{min, max,
/// step_fine}` range and current value. The native scalar marker reflects the
/// value's position within the discrete enumerated range; we reproduce that by
/// treating `step_fine` as the enumeration granularity. Returns `None` for a
/// degenerate range (so the caller draws nothing, matching the native
/// count-`< 2` behavior). The current value is clamped into range and snapped
/// to the nearest step so a mid-step value still maps to a valid index.
fn scalar_index_count(current: i32, min: i32, max: i32, step_fine: i32) -> Option<(usize, usize)> {
    if step_fine <= 0 || max <= min {
        return None;
    }
    let span = (max - min) as i64;
    let step = step_fine as i64;
    // Number of reachable values, inclusive of both endpoints.
    let count = (span / step) as usize + 1;
    if count < 2 {
        return None;
    }
    let clamped = current.clamp(min, max) as i64;
    // Round to nearest step rather than truncating, so a value sitting between
    // two steps lands on the closer index.
    let index = (((clamped - min as i64) + step / 2) / step) as usize;
    Some((index.min(count - 1), count))
}

/// Push the marker position/width to the `scroll_usr`/`move_usr` sub-clips and
/// set the `tri_l_usr`/`tri_r_usr` arrow visibility, exactly as the native
/// slot-7 render does. `mc_id` is the row's sub-MC layer id (the same id used
/// for `option_usr`/`choice_usr` binding).
///
/// Child resolution uses `layer_find_child` (`afp_layer_mc_refer`, the type-1
/// layer-id namespace) — NOT `find_child` (`afp_mc_refer`, type-4 MC-id), which
/// returns -1 for these paths because `mc_id` is a layer id. This matches the
/// working `option_usr`/`choice_usr` binds in `render_enum`/`render_scalar`;
/// verified live via Cheat Engine (afp_mc_refer for `choice_usr/scroll_usr`
/// returned -1, afp_layer_mc_refer resolves).
fn drive_value_marker(mc_id: u32, m: &MarkerState) {
    use crate::services::bm2d_api;

    // Position bar: choice_usr/scroll_usr, op 0x0f04 = round(pos * 100).
    if let Some(scroll) = bm2d_api::layer_find_child(mc_id, "choice_usr/scroll_usr") {
        bm2d_api::mc_op(
            scroll,
            0x0F04,
            (m.pos * MARKER_PX_SCALE + 0.5).trunc() as i32,
        );
    }
    // Width bar: choice_usr/scroll_usr/move_usr, op 0x0f04 = round(width * 100).
    if let Some(mv) = bm2d_api::layer_find_child(mc_id, "choice_usr/scroll_usr/move_usr") {
        bm2d_api::mc_op(mv, 0x0F04, (m.width * MARKER_PX_SCALE).trunc() as i32);
    }

    // Cycling arrows: left visible unless at the first value, right visible
    // unless at the last. Walk the sibling chain (native uses afp_mc_traversal
    // depth 6) so every layer under the named clip gets toggled.
    set_arrow_visibility(mc_id, "choice_usr/tri_l_usr", m.index != 0);
    set_arrow_visibility(mc_id, "choice_usr/tri_r_usr", m.index != m.count - 1);
}

/// Set visibility on every sibling layer reachable from the named child, the
/// way the native render does (`0x1007` visibility + `0x101e` apply, iterating
/// via `afp_mc_traversal` at depth 6). Resolves via `layer_find_child` for the
/// same type-1-vs-type-4 reason documented on [`drive_value_marker`].
fn set_arrow_visibility(mc_id: u32, name: &str, visible: bool) {
    use crate::services::bm2d_api;

    let mut layer = match bm2d_api::layer_find_child(mc_id, name) {
        Some(l) => l,
        None => return,
    };
    let vis = if visible { 1 } else { 0 };
    // Depth-cap the walk to guard against an unexpected cyclic sibling chain.
    for _ in 0..32 {
        bm2d_api::mc_set_param(layer, 0x1007, vis);
        bm2d_api::mc_set_param(layer, 0x101e, 1);
        match bm2d_api::mc_traversal(layer, 6) {
            Some(next) => layer = next,
            None => break,
        }
    }
}

/// Walk the parent chain starting at `node` (typically `row+0x60`),
/// summing `+0x88` (x) and `+0x90` (y) at each ancestor. Mirrors the
/// native `FUN_180046220`. Depth-capped at 32 to guard against cycles.
unsafe fn accumulate_ancestor_xy(start: *const u8) -> (f64, f64) {
    let mut x: f64 = 0.0;
    let mut y: f64 = 0.0;
    let mut node = start;
    for _ in 0..32 {
        if node.is_null() {
            break;
        }
        x += *(node.add(0x88) as *const f64);
        y += *(node.add(0x90) as *const f64);
        node = *(node.add(0x60) as *const *const u8);
    }
    (x, y)
}

/// Scalar row's slot-7 render. Mirrors the subset of the native slot-7
/// behavior we actually need for mod rows:
///
/// 1. Pin the row's position on the AFP sub-clip at `row+0x118` via
///    `afp_layer_set_position` (walking the ancestor chain at `+0x60` to
///    accumulate world-space coordinates). Our rows are appended after
///    the native builder runs, so `+0x88/+0x90` remain zero without
///    this step.
/// 2. Bind the left-column label texture and drive the value-position
///    marker (`choice_usr/scroll_usr` + arrows) — the same marker block
///    enum rows use, with (index, count) derived from the scalar's
///    `{min, max, step_fine}` range. See `docs/option_row_marker_render.md`.
/// 3. Tick the label TextLayer at `row+0x120` and the value TextLayer
///    at `row+0x130` (virtual slot 0 on each). The first tick lazy-allocates
///    the BmpString at `TextLayer+0x68`; subsequent ticks refresh the
///    transform and color.
/// 4. Push the formatted current value into the value TextLayer via
///    `textlayer_set_text` when it differs from the last-pushed cache.
///
/// We deliberately do NOT call the donor's native slot 7 (the shared
/// `option_row_render`, `FUN_1801754c0` on 20260526). It reads `row+0x110`
/// behind a does-not-return abort gate; `+0x110` is a self-pointer to an
/// embedded `_Impl_no_alloc0<lambda, BM2D::CLayer*>` closure the native
/// *builder* populates (the ctor zeroes it). Our injected rows skip that
/// builder path, so `+0x110` stays null and delegating would abort. The
/// marker block we DO want sits past `+0x110` and only needs `+0x118` +
/// our own index/count, so we replicate just that — see the doc above.
unsafe extern "C" fn render_scalar_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| render_scalar(this));
}

fn render_scalar(this: *mut u8) {
    use crate::services::bm2d_api;

    // Confirm the row is one we own; silently skip if not (shouldn't
    // happen — the vtable pointer came from our synthesis path).
    {
        let rows = ROWS.lock().unwrap();
        if !rows.iter().any(|r| r.row_ptr == this) {
            return;
        }
    }

    unsafe {
        // Position pinning on the AFP sub-clip. Also used as the
        // label-bind target — `option_usr` is a child of this sub-clip.
        let sub_mc_ptr = *(this.add(0x118) as *const *const u8);
        if !sub_mc_ptr.is_null() {
            let mc_id = *(sub_mc_ptr.add(0x08) as *const u32);
            let mut x = *(this.add(0x88) as *const f64);
            let mut y = *(this.add(0x90) as *const f64);
            let parent = *(this.add(0x60) as *const *const u8);
            if !parent.is_null() {
                let (ax, ay) = accumulate_ancestor_xy(parent);
                x += ax;
                y += ay;
            }
            bm2d_api::set_position(mc_id, x as f32, y as f32);

            // Bind the left-column label texture each frame — the
            // `option_usr` child's mc_id can change on tab switches,
            // and the inherited visibility handler doesn't write the
            // label name for us (it binds the TextLayer path, not the
            // bitmap path). Same per-frame pattern as render_enum.
            let label_texture = {
                let rows = ROWS.lock().unwrap();
                match rows.iter().find(|r| r.row_ptr == this) {
                    Some(r) => {
                        let state = registry::STATE.lock().unwrap();
                        let idx = r.handle.0 as usize;
                        if idx < state.options.len() {
                            state.options[idx].label_texture_name()
                        } else {
                            String::new()
                        }
                    }
                    None => String::new(),
                }
            };
            if !label_texture.is_empty() {
                if let Some(layer) = bm2d_api::layer_find_child(mc_id, "option_usr") {
                    bm2d_api::mc_set_param(layer, 0x1007, 1);
                    bm2d_api::mc_set_param(layer, 0x101e, 1);
                    bm2d_api::mc_load_bitmap(layer, &label_texture);
                }
            }

            // Drive the value-position marker. Native scalar rows show the
            // same `choice_usr/scroll_usr` bar as enum rows (the value's
            // position within its discrete range); see
            // `docs/option_row_marker_render.md`. We derive (index, count)
            // from the scalar's {min, max, step_fine} so the bar slides as
            // the value changes, and pinning it each frame stops the intro
            // sweep — same as the enum path.
            let marker = {
                let rows = ROWS.lock().unwrap();
                rows.iter().find(|r| r.row_ptr == this).and_then(|r| {
                    let index_count = {
                        let state = registry::STATE.lock().unwrap();
                        let idx = r.handle.0 as usize;
                        match state
                            .options
                            .get(idx)
                            .map(|o| (&o.ui_kind, o.values[r.side as usize]))
                        {
                            Some((
                                UiKind::Scalar {
                                    min,
                                    max,
                                    step_fine,
                                    ..
                                },
                                current,
                            )) => scalar_index_count(current, *min, *max, *step_fine),
                            _ => None,
                        }
                    };
                    let (index, count) = index_count?;
                    step_marker_anim(&mut r.marker_anim.lock().unwrap(), index, count)
                })
            };
            if let Some(m) = marker {
                drive_value_marker(mc_id, &m);
            }
        }

        // Tick the label and value TextLayers. Each shared_ptr at
        // `row+0x120` / `row+0x130` points at a TextLayer whose vtable
        // slot 0 is `FUN_1801d2d60` — the tick that lazy-inits the
        // BmpString on first call and refreshes the render-time
        // transform on subsequent calls.
        tick_textlayer(*(this.add(0x120) as *const *mut u8));
        tick_textlayer(*(this.add(0x130) as *const *mut u8));
    }

    // Push the current value text if it's changed. Must run AFTER the
    // value TextLayer's first tick has allocated its BmpString;
    // `textlayer_set_text` silently no-ops when `TextLayer+0x68` is
    // null, so the first frame after the visibility handler fires may
    // drop the initial text push — we'll repush on the next tick when
    // the cached value still differs from the layer's (empty) content.
    push_scalar_value_text(this);
}

/// Dispatch the virtual tick on a TextLayer (vtable slot 0). Null-safe;
/// silently skips when the TextLayer hasn't been allocated yet (e.g.
/// the row is out of view).
unsafe fn tick_textlayer(tl: *mut u8) {
    if tl.is_null() {
        return;
    }
    let vtable = *(tl as *const *const *const u8);
    if vtable.is_null() {
        return;
    }
    let slot0 = *vtable;
    if slot0.is_null() {
        return;
    }
    let tick: unsafe extern "C" fn(*mut u8) = std::mem::transmute(slot0);
    tick(tl);
}

/// Format the current value for the scalar option at `row` and push it
/// into the value TextLayer at `row+0x130` via `textlayer_set_text`.
/// Uses the per-row `last_value_text` cache to skip the dispatch when
/// the value hasn't changed since the previous frame.
fn push_scalar_value_text(row: *mut u8) {
    let set_text = match unsafe { FN_TEXTLAYER_SET_TEXT } {
        Some(f) => f,
        None => return,
    };

    // Resolve the TextLayer inside the value shared_ptr at row+0x130.
    let value_textlayer = unsafe { *(row.add(0x130) as *const *mut u8) };
    if value_textlayer.is_null() {
        // Visibility handler hasn't fired yet (row not on active tab).
        return;
    }

    // Snapshot the (value, format, side) we need — don't hold the lock
    // across the native call.
    let formatted = {
        let rows = ROWS.lock().unwrap();
        let row_slot = match rows.iter().find(|r| r.row_ptr == row) {
            Some(r) => r,
            None => return,
        };
        let side = row_slot.side;
        let state = registry::STATE.lock().unwrap();
        let idx = row_slot.handle.0 as usize;
        if idx >= state.options.len() {
            return;
        }
        let opt = &state.options[idx];
        let (format, current) = match &opt.ui_kind {
            UiKind::Scalar { format, .. } => (*format, opt.values[side as usize]),
            // Enum rows have no value TextLayer; headers additionally can
            // never reach this path (their render never calls it).
            UiKind::Enum { .. } | UiKind::Header => return,
        };
        format_scalar_value(current, format)
    };

    // Build an SSO std::string holding the formatted text and pass it
    // to textlayer_set_text. The native path stores the string at
    // TextLayer+0xA8 and copies content into it, so it's safe to let
    // the stack-local SSO string drop after the call.
    unsafe {
        let mut sso: [u8; SSO_BUF_SIZE] = [0; SSO_BUF_SIZE];
        if !prime_sso_string(&mut sso, &formatted) {
            return;
        }
        set_text(value_textlayer, sso.as_mut_ptr(), 3);
        destruct_sso_string(&mut sso);
    }
}

// `format_scalar_value` moved to `api.rs` (beside `ScalarFormat`) so the
// overlay snapshot renders values through the same function; imported above.

// ── Slot-4 override ──────────────────────────────────────────────────────

/// Our `advanceValue` implementation, installed at primary-vtable slot 4.
///
/// Calling convention (matches the native slot-4):
///   RCX (`this`)  — the row pointer we allocated.
///   RDX           — input `event_obj` (the dispatcher's stack-local
///                   passed through to every slot-4 handler).
///
/// The input dispatcher [`FUN_180048a90`] calls this slot once per input
/// event regardless of direction (and also on focus changes). The native
/// slot-4 distinguishes left/right from navigation by registering
/// per-direction lambdas against the event via [`FUN_18017dc40`] and
/// letting the engine fire only the lambda whose registered type matches
/// the event currently in flight. A mod override that cycles
/// unconditionally on every call leaks into up/down navigation AND fires
/// on focus-gain — neither of which the player means to toggle.
///
/// This override mirrors the native pattern with MSVC `_Impl_no_alloc0`
/// lambda frames: two 32-byte stack frames (one per direction) wired to
/// process-lifetime mod-authored vtables whose invoke slots carry the
/// direction as code. The engine consumes both event-type-1 (left) and
/// event-type-2 (right) registrations, invokes at most one (the matching
/// type), and destructs both at the end of the dispatch window. Up/down
/// (types 3/4) and stop (type 0) have no registration so they flow
/// through to tab/row navigation unchanged.
///
/// FFI-boundary safety: this is an `extern "C"` callback called from game
/// code. Any panic would unwind into the game and trigger UB, so the body
/// is wrapped in `catch_unwind`.
unsafe extern "C" fn advance_value_enum_trampoline(this: *mut u8, event_obj: *mut u8) {
    let _ = std::panic::catch_unwind(|| {
        if event_obj.is_null() {
            return;
        }
        let (left_vt, right_vt) = (
            std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_LEFT)),
            std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_RIGHT)),
        );
        let register = match FN_EVENT_REGISTER {
            Some(f) => f,
            None => return,
        };
        if left_vt.is_null() || right_vt.is_null() {
            return;
        }
        register_press_callback(register, event_obj, 1, left_vt, this);
        register_press_callback(register, event_obj, 2, right_vt, this);
    });
}

/// Scalar counterpart of [`advance_value_enum_trampoline`]. Registers
/// four lambdas per invocation — fine-step left/right via
/// [`FN_EVENT_REGISTER`] (fires when Start is NOT held) and coarse-step
/// left/right via [`FN_EVENT_REGISTER_NO_CONSUME`] (fires only when
/// Start IS held). The engine gates which pair fires based on
/// `event_obj+0x10` at dispatch time; each vtable's invoke trampoline
/// applies the correct step.
///
/// If [`FN_EVENT_REGISTER_NO_CONSUME`] isn't resolved, coarse-step
/// lambdas are skipped and the row behaves like a fine-step-only
/// scalar — still correct, just without the Start-held shortcut.
unsafe extern "C" fn advance_value_scalar_trampoline(this: *mut u8, event_obj: *mut u8) {
    let _ = std::panic::catch_unwind(|| {
        if event_obj.is_null() {
            return;
        }
        let left_vt = std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_LEFT));
        let right_vt = std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_RIGHT));
        let left_coarse_vt = std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_LEFT_COARSE));
        let right_coarse_vt = std::ptr::read(std::ptr::addr_of!(MOD_LAMBDA_VTABLE_RIGHT_COARSE));

        let register = match FN_EVENT_REGISTER {
            Some(f) => f,
            None => return,
        };
        if !left_vt.is_null() && !right_vt.is_null() {
            register_press_callback(register, event_obj, 1, left_vt, this);
            register_press_callback(register, event_obj, 2, right_vt, this);
        }

        if let Some(register_coarse) = FN_EVENT_REGISTER_NO_CONSUME {
            if !left_coarse_vt.is_null() && !right_coarse_vt.is_null() {
                register_press_callback_no_consume(
                    register_coarse,
                    event_obj,
                    1,
                    left_coarse_vt,
                    this,
                );
                register_press_callback_no_consume(
                    register_coarse,
                    event_obj,
                    2,
                    right_coarse_vt,
                    this,
                );
            }
        }
    });
}

/// Stack-frame one MSVC `_Impl_no_alloc0` lambda and hand it to the
/// engine's [`FN_EVENT_REGISTER`] against `event_type`. The frame layout
/// matches what [`FUN_18017dc40`] emits for native left/right/up/down
/// lambdas:
///
/// ```text
/// [+0x00] vtable      — mod-authored 7-slot vtable (left or right)
/// [+0x08] captured    — row pointer (read back by invoke via +0x08)
/// [+0x10] (unused)    — zero; MSVC reserves this slot for slot-5 return
/// [+0x18] self-ptr    — &frame, read by the engine to detect
///                       stack-vs-heap storage when destructing
/// ```
///
/// The engine's registration path walks a rb-tree for a matching type
/// entry; on hit it shallow-copies the lambda via slot 0 (which CRT-
/// allocates 16 bytes and copies vtable + captured) and later destructs
/// via slot 3. On miss it destructs the on-stack copy immediately. Either
/// way the frame's lifetime covers the whole call, which is why stack
/// storage is safe.
#[inline]
unsafe fn register_press_callback(
    register: EventRegisterFn,
    event_obj: *mut u8,
    event_type: i32,
    vtable: *mut *const u8,
    row: *mut u8,
) {
    // Write the frame through the raw pointer so the compiler sees the
    // qword stores as live (they are — `register` dereferences
    // `frame_ptr` — but going through an alias hides the flow from the
    // unused-assignment lint).
    let mut frame: [*mut u8; 4] = [std::ptr::null_mut(); 4];
    let frame_ptr = frame.as_mut_ptr() as *mut u8;
    let qw = frame_ptr as *mut *mut u8;
    std::ptr::write(qw.add(0), vtable as *mut u8);
    std::ptr::write(qw.add(1), row);
    std::ptr::write(qw.add(2), std::ptr::null_mut());
    std::ptr::write(qw.add(3), frame_ptr);
    register(event_obj, event_type, frame_ptr);
}

/// Coarse-step variant of [`register_press_callback`]. Uses
/// [`FN_EVENT_REGISTER_NO_CONSUME`]'s 4-argument ABI — the auxiliary
/// predicate arg is always `0` to match every native scalar call site.
#[inline]
unsafe fn register_press_callback_no_consume(
    register: EventRegisterNoConsumeFn,
    event_obj: *mut u8,
    event_type: i32,
    vtable: *mut *const u8,
    row: *mut u8,
) {
    let mut frame: [*mut u8; 4] = [std::ptr::null_mut(); 4];
    let frame_ptr = frame.as_mut_ptr() as *mut u8;
    let qw = frame_ptr as *mut *mut u8;
    std::ptr::write(qw.add(0), vtable as *mut u8);
    std::ptr::write(qw.add(1), row);
    std::ptr::write(qw.add(2), std::ptr::null_mut());
    std::ptr::write(qw.add(3), frame_ptr);
    register(event_obj, event_type, 0, frame_ptr);
}

// ── Mod lambda vtable + per-direction invoke trampolines ────────────────

/// Number of qword slots in the mod lambda vtables we build at init. MSVC
/// emits 7 slots for every `_Impl_no_alloc0` specialization; exceeding
/// that would put garbage at `vtable[>=7]` reachable from future engine
/// calls, so we match the native count exactly.
const MOD_LAMBDA_VTABLE_SLOTS: usize = 7;

/// Allocate a 7-slot lambda vtable in process-lifetime RWX memory. Slot 0
/// (copy constructor) is a mod-authored trampoline that preserves the
/// source lambda's vtable during heap-copy — NOT the game's native slot-0
/// body, which hardcodes `lambda232`'s vtable as the copy's initial
/// vtable. Using the native slot 0 here would make every heap-promoted
/// registration invoke the native left-direction native value-list
/// walker, silently no-opping because our rows don't carry the
/// `OptionTab` value-list layout.
///
/// Slot 1 (invoke) receives the direction-specific trampoline passed in.
/// Slots 3/4/5 (destruct / release / get-captured) can reuse the game's
/// versions safely — they're vtable-layout-agnostic. Slots 2 and 6 are
/// never invoked during the event-registration flow; we leave them null.
///
/// The vtable is leaked intentionally — it must outlive every stack frame
/// that references it, which in practice is the process lifetime because
/// slot-4 registers lambdas on every input event.
unsafe fn build_mod_lambda_vtable(
    invoke: *const u8,
    destruct: *const u8,
    release: *const u8,
    get_captured: *const u8,
) -> *mut *const u8 {
    let slot_size = std::mem::size_of::<*const u8>();
    let raw = memory::alloc_zeroed(MOD_LAMBDA_VTABLE_SLOTS * slot_size);
    if raw.is_null() {
        return std::ptr::null_mut();
    }
    let vtable = raw as *mut *const u8;
    *vtable.add(0) = copy_lambda_trampoline as *const u8;
    *vtable.add(1) = invoke;
    *vtable.add(2) = std::ptr::null();
    *vtable.add(3) = destruct;
    *vtable.add(4) = release;
    *vtable.add(5) = get_captured;
    *vtable.add(6) = std::ptr::null();
    vtable
}

/// Slot-0 copy constructor for mod lambda vtables. Mirrors the native
/// `_Impl_no_alloc0::copy` contract except it preserves whatever vtable
/// the source lambda has instead of hardcoding lambda232's.
///
/// Calling convention (matches the native slot 0):
///   RCX (`this`) — source lambda (has vtable at `this[0]`, captured at `this[8]`)
///   RDX (`dst`)  — caller-supplied 16-byte buffer, or NULL to heap-allocate
///   R8           — MSVC unwind tag (0xFFFFFFFFFFFFFFFE); unused by us
///
/// Returns the destination pointer (either the caller's buffer or a
/// freshly CRT-heap-allocated 16-byte block). Returns NULL on allocation
/// failure — the native path throws bad_alloc here, but a C++ throw from
/// Rust `extern "C"` would unwind across the FFI boundary. Silent null
/// return is safer; it propagates as a degraded registration that just
/// fails to fire, not a process crash.
///
/// FFI-boundary safety: panic-caught; `catch_unwind` around every
/// dereference keeps any accidental Rust-side panic contained.
unsafe extern "C" fn copy_lambda_trampoline(
    this: *mut u8,
    dst: *mut u8,
    _unwind_tag: u64,
) -> *mut u8 {
    let result = std::panic::catch_unwind(|| {
        if this.is_null() {
            return std::ptr::null_mut();
        }
        let dst = if dst.is_null() {
            let malloc = match FN_GAME_MALLOC {
                Some(f) => f,
                None => return std::ptr::null_mut(),
            };
            malloc(0x10)
        } else {
            dst
        };
        if dst.is_null() {
            return std::ptr::null_mut();
        }
        let src_qw = this as *const *mut u8;
        let dst_qw = dst as *mut *mut u8;
        std::ptr::write(dst_qw.add(0), std::ptr::read(src_qw.add(0)));
        std::ptr::write(dst_qw.add(1), std::ptr::read(src_qw.add(1)));
        dst
    });
    result.unwrap_or(std::ptr::null_mut())
}

/// Left fine-step invoke, installed at slot 1 of
/// [`MOD_LAMBDA_VTABLE_LEFT`]. Enum rows: cycle one slot earlier in
/// `allowed_values`. Scalar rows: subtract `step_fine` from the current
/// value and clamp to `[min, max]`.
unsafe extern "C" fn invoke_left_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| press_body(this, Press::LeftFine));
}

/// Right fine-step invoke, twin of [`invoke_left_trampoline`] for the
/// right-press vtable. Enum: cycle forward. Scalar: add `step_fine`.
unsafe extern "C" fn invoke_right_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| press_body(this, Press::RightFine));
}

/// Scalar-only left coarse-step invoke (`step_coarse`, fires when Start
/// is held). Installed at slot 1 of [`MOD_LAMBDA_VTABLE_LEFT_COARSE`].
/// An enum row that somehow ended up dispatched here would be a
/// framework bug; `press_body` logs and returns safely.
unsafe extern "C" fn invoke_left_coarse_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| press_body(this, Press::LeftCoarse));
}

/// Scalar-only right coarse-step invoke. Twin of
/// [`invoke_left_coarse_trampoline`].
unsafe extern "C" fn invoke_right_coarse_trampoline(this: *mut u8) {
    let _ = std::panic::catch_unwind(|| press_body(this, Press::RightCoarse));
}

#[derive(Copy, Clone)]
enum Press {
    LeftFine,
    RightFine,
    LeftCoarse,
    RightCoarse,
}

impl Press {
    /// Sign of the step in this direction (negative for left, positive
    /// for right). Magnitude comes from the option's configuration.
    fn sign(self) -> i32 {
        match self {
            Press::LeftFine | Press::LeftCoarse => -1,
            Press::RightFine | Press::RightCoarse => 1,
        }
    }

    fn is_coarse(self) -> bool {
        matches!(self, Press::LeftCoarse | Press::RightCoarse)
    }

    fn label(self) -> &'static str {
        match self {
            Press::LeftFine => "LEFT_FINE",
            Press::RightFine => "RIGHT_FINE",
            Press::LeftCoarse => "LEFT_COARSE",
            Press::RightCoarse => "RIGHT_COARSE",
        }
    }
}

/// Shared body for all four invoke trampolines. Reads the captured row
/// pointer from the lambda frame at `+0x08`, resolves the row's kind
/// from [`ROWS`], advances the value appropriately (enum: cycle; scalar:
/// step + clamp), and fires the change callback outside every lock.
fn press_body(lambda_this: *mut u8, press: Press) {
    if lambda_this.is_null() {
        return;
    }
    let row = unsafe { *(lambda_this.add(0x08) as *const *mut u8) };
    if row.is_null() {
        return;
    }

    let (handle, side, kind) = {
        let rows = ROWS.lock().unwrap();
        match rows.iter().find(|r| r.row_ptr == row) {
            Some(r) => (r.handle, r.side, r.kind),
            None => {
                log_warn!(
                    "custom_options/rows: lambda invoke with unknown row {:p}",
                    row
                );
                return;
            }
        }
    };

    // Guard against a misdispatch: a coarse-step lambda fired on a row
    // whose kind is Enum would indicate a framework wiring bug.
    if press.is_coarse() && kind != RowKind::Scalar {
        log_warn!(
            "custom_options/rows: coarse-step invoke on non-scalar row {:p} (kind={:?})",
            row,
            kind
        );
        return;
    }

    let dispatch = {
        let mut state = registry::STATE.lock().unwrap();
        let idx = handle.0 as usize;
        if idx >= state.options.len() {
            log_warn!(
                "custom_options/rows: lambda invoke for stale handle {:?} (registry shrunk?)",
                handle
            );
            return;
        }

        let (next_value, id_for_log) = match &state.options[idx].ui_kind {
            UiKind::Enum { allowed_values } => {
                if allowed_values.is_empty() {
                    return;
                }
                let current = state.options[idx].values[side as usize];
                let cur_idx = allowed_values
                    .iter()
                    .position(|v| v.value == current)
                    .unwrap_or(0);
                let n = allowed_values.len();
                let next_idx = match press.sign() {
                    1 => (cur_idx + 1) % n,
                    _ => (cur_idx + n - 1) % n,
                };
                (
                    allowed_values[next_idx].value,
                    state.options[idx].id.clone(),
                )
            }
            UiKind::Scalar {
                min,
                max,
                step_fine,
                step_coarse,
                ..
            } => {
                let step_mag = if press.is_coarse() {
                    *step_coarse
                } else {
                    *step_fine
                };
                if step_mag <= 0 {
                    return;
                }
                let current = state.options[idx].values[side as usize];
                let delta = press.sign().saturating_mul(step_mag);
                let next = current.saturating_add(delta).clamp(*min, *max);
                (next, state.options[idx].id.clone())
            }
            // Unreachable: headers can never be focused, register no press
            // lambdas, and their slot-4 is a no-op — but stay defensive.
            UiKind::Header => return,
        };

        let opt = &mut state.options[idx];
        if opt.values[side as usize] == next_value {
            // No actual change (e.g. hit clamp boundary) — don't fire
            // the callback. Matches the native scalar slot-4's
            // at-endpoint behavior (error SFX; no value change).
            log_debug!(
                "custom_options/rows: press {} on {id_for_log:?} side={side} at endpoint (no change)",
                press.label()
            );
            return;
        }
        opt.values[side as usize] = next_value;
        let cb = opt.on_change;
        log_debug!(
            "custom_options/rows: press {} on {id_for_log:?} side={side} -> {next_value}",
            press.label()
        );
        (cb, side, next_value, id_for_log)
    };

    let (cb, side, value, option_id) = dispatch;
    cb(side, value);
    // The in-game press is a value mutation like any other — notify the
    // value-changed observers (overlay mirror) after the callback, with no
    // framework lock held (the deferred-dispatch contract).
    super::observers::dispatch(&option_id, side, value);

    // Immediately update visibility of any child rows that depend on
    // this option's value via ShowWhen::Equals/NotEquals.
    update_children_visibility(&option_id, side, value);
}

/// After a parent option changes, immediately reapply the scroll mask
/// so child rows appear/disappear on the same frame.
fn update_children_visibility(parent_id: &str, side: u8, _new_parent_value: i32) {
    let has_children = {
        let state = registry::STATE.lock().unwrap();
        state.options.iter().any(|opt| {
            matches!(
                &opt.show_when,
                ShowWhen::Equals { parent_id: pid, .. }
                | ShowWhen::NotEquals { parent_id: pid, .. } if pid == parent_id
            )
        })
    };
    if has_children {
        crate::services::options_scroll::reapply_mask_for_side(side);
    }
}

// ── Page-tag primitive ──────────────────────────────────────────────────

/// Size of the stack-local SSO std::string scratch buffer (16 bytes for
/// inline chars + 8 bytes size + 8 bytes capacity = 32 bytes).
const SSO_BUF_SIZE: usize = 32;
const SSO_SIZE_OFFSET: usize = 0x10;
const SSO_CAPACITY_OFFSET: usize = 0x18;
/// Capacity sentinel that marks an SSO (inline) std::string — when
/// capacity ≤ 0xF, the string's bytes live in the first 16 bytes;
/// setting capacity to 0xF before `assign` tells the assigner "start in
/// SSO mode".
const SSO_CAPACITY_SENTINEL: u64 = 0x0F;

/// Prime a 32-byte `std::string` scratch buffer with `data`'s contents
/// using the game's own `string::assign` (so any heap promotion is done
/// by the game's allocator, not Rust's). Returns `true` on success.
///
/// The buffer's size/capacity qwords are initialized to match what
/// `string::assign` expects for a default-constructed string: size = 0,
/// capacity = 0xF (the SSO sentinel). The assigner then writes the
/// bytes inline if they fit, or heap-allocates and promotes if they
/// don't.
///
/// Callers must invoke [`destruct_sso_string`] on the same buffer after
/// the native function call returns so any heap promotion gets freed.
unsafe fn prime_sso_string(buf: &mut [u8; SSO_BUF_SIZE], data: &[u8]) -> bool {
    let string_assign = match FN_STRING_ASSIGN {
        Some(f) => f,
        None => return false,
    };
    let buf_ptr = buf.as_mut_ptr();
    memory::write_u64(buf_ptr.add(SSO_SIZE_OFFSET), 0);
    memory::write_u64(buf_ptr.add(SSO_CAPACITY_OFFSET), SSO_CAPACITY_SENTINEL);
    string_assign(buf_ptr, data.as_ptr(), data.len());
    true
}

/// Release any heap buffer owned by a primed SSO string and zero the
/// 32-byte scratch. Safe to call multiple times. If the string stayed
/// in SSO (capacity ≤ 0xF), no free is needed; we only zero.
///
/// When the string DID promote to heap (capacity > 0xF), the buffer
/// pointer at `+0x00` was allocated by the game's CRT allocator inside
/// `string_assign`. Freeing it here would need a matching CRT `free`
/// we don't currently expose as a signature; for now we log and skip
/// the free to avoid calling the wrong allocator — a one-time leak per
/// heap-promoted push, which is acceptable for a value-text string
/// whose upper-bound length is ~16 chars (fixed-point max).
unsafe fn destruct_sso_string(buf: &mut [u8; SSO_BUF_SIZE]) {
    let buf_ptr = buf.as_mut_ptr();
    let capacity = memory::read_u64(buf_ptr.add(SSO_CAPACITY_OFFSET));
    if capacity > SSO_CAPACITY_SENTINEL {
        log_warn!(
            "custom_options/rows: SSO string overflowed to heap (capacity={:#x}) — skipping free (one-time leak per call)",
            capacity
        );
    }
    for i in 0..SSO_BUF_SIZE {
        *buf_ptr.add(i) = 0;
    }
}

/// Run the native row builder's per-tag sequence: prime a 32-byte SSO
/// std::string with the `"PageN"` literal and feed it to
/// `metadata_insert`. Returns `true` on success; `false` if the
/// primitives were unavailable (which shouldn't happen because we
/// checked in [`init`] but is reported as a WARN rather than a panic).
unsafe fn write_page_tag(row: *mut u8, page: PageTag) -> bool {
    let metadata_insert = match FN_METADATA_INSERT {
        Some(f) => f,
        None => return false,
    };

    let literal = page.metadata_key(); // "Page1".."Page6"
    let mut buf: [u8; SSO_BUF_SIZE] = [0; SSO_BUF_SIZE];
    if !prime_sso_string(&mut buf, literal.as_bytes()) {
        return false;
    }
    metadata_insert(row, buf.as_mut_ptr());
    destruct_sso_string(&mut buf);
    true
}
