//! Builder-hook detour for custom option row injection.
//!
//! Installs a `retour::GenericDetour` on the native row builder
//! resolved at the `row_builder_fn` signature. After the native builder
//! runs — which constructs the OptionTab scaffolding plus every native
//! `OptionElement<KIND>` row and pushes them into the flat row vector at
//! `(parent + 0x230) + 0x68` — the detour iterates every registered
//! option in the framework registry, allocates a mod row for it via
//! [`super::rows::allocate_row_for_option`], and appends it to the same
//! flat vector using the native register helper resolved at
//! `option_tab_register`.
//!
//! The detour runs on the game's render thread (same thread that called
//! the native builder), and every prerequisite the donor ctor needs is
//! warm at this point — that's the whole point of doing row allocation
//! here rather than at DLL init time.
//!
//! Panic discipline: the detour body is wrapped in `catch_unwind`. The
//! native builder runs FIRST, so even if the mod-injection code panics,
//! the native rows are already in the vector and the options menu will
//! render normally, minus the mod rows.
//!
//! Graceful degradation: if `row_builder_fn` or `option_tab_register`
//! signatures don't resolve, or if the `GenericDetour::new` / `enable`
//! calls fail, [`init`] returns `false` and no detour is installed. The
//! framework continues to accept registrations and the registry stays
//! consistent, but no rows appear in-game.

use retour::GenericDetour;

use crate::core::signatures::SignatureStore;
use crate::{log_error, log_info, log_warn};

use super::api::{OptionHandle, UiKind};
use super::registry;
use super::rows;

/// Row kind captured from the registry snapshot, used to route the
/// option to the correct allocation path. Kept local to this module —
/// `rows` maintains its own independent `RowKind` for runtime dispatch.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum RowKindTag {
    Enum,
    Scalar,
    Header,
}

/// Native row-builder signature: `void FUN_180163970(void* parent)`.
/// `parent` is the OptionForm (or an adjacent scene-graph container)
/// whose `+0x228` field holds the active player side and whose
/// `*(this+0x230) + 0x68` holds the flat row vector.
type RowBuilderFn = unsafe extern "C" fn(*mut u8);

/// Native row-register helper signature:
/// `void* FUN_180168c70(void** parent_slot, void* row)`.
///
/// The helper reads `*parent_slot` to get the parent, dereferences
/// `[parent+0x230]` to reach the scene-graph anchor, wraps `row` in a
/// shared_ptr (built in-place on the callee's stack), and `push_back`s
/// into the anchor's `+0x68` vector via `std::vector::push_back`.
type OptionTabRegisterFn = unsafe extern "C" fn(*mut *mut u8, *mut u8) -> *mut u8;

/// Offset within the parent object where the active player side is
/// stored. Matches what the native builder reads at its prologue.
const PARENT_PLAYER_SIDE_OFFSET: usize = 0x228;

static mut BUILDER_HOOK: Option<GenericDetour<RowBuilderFn>> = None;
static mut FN_OPTION_TAB_REGISTER: Option<OptionTabRegisterFn> = None;
static mut READY: bool = false;

/// Resolve the builder + register-helper addresses and install the
/// detour. Returns `true` on success; any failure logs WARN and returns
/// `false`, leaving the framework functional but without row injection.
pub(crate) fn init(signatures: &SignatureStore) -> bool {
    let builder_addr = match signatures.get_address("row_builder_fn") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/builder_hook: row_builder_fn not resolved — mod rows will not inject"
            );
            return false;
        }
    };
    let register_addr = match signatures.get_address("option_tab_register") {
        Some(a) => a,
        None => {
            log_warn!(
                "custom_options/builder_hook: option_tab_register not resolved — mod rows will not inject"
            );
            return false;
        }
    };

    unsafe {
        let register_fn = std::mem::transmute::<*const u8, OptionTabRegisterFn>(register_addr);
        FN_OPTION_TAB_REGISTER = Some(register_fn);

        let target: RowBuilderFn = std::mem::transmute(builder_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(BUILDER_HOOK),
            target,
            builder_detour,
        ) {
            log_warn!(
                "custom_options/builder_hook: detour install failed: {:?} — mod rows will not inject",
                e
            );
            return false;
        }
        READY = true;
        log_info!(
            "custom_options/builder_hook: row-builder detour installed @ {:p}",
            builder_addr
        );
    }
    true
}

pub(crate) fn is_ready() -> bool {
    unsafe { READY }
}

/// Our detour body. Calls the original first so all native rows land,
/// then iterates registered options and appends mod rows for the active
/// player side.
unsafe extern "C" fn builder_detour(parent: *mut u8) {
    let _ = std::panic::catch_unwind(|| builder_detour_body(parent));
}

fn builder_detour_body(parent: *mut u8) {
    unsafe {
        // 1. Call the original builder so every native row registers.
        // Borrow the detour through a shared reference — taking ownership
        // of the static would run its destructor and disable the hook.
        let detour = match (*std::ptr::addr_of!(BUILDER_HOOK)).as_ref() {
            Some(d) => d,
            None => {
                log_error!(
                    "custom_options/builder_hook: detour storage empty in callback — logic bug"
                );
                return;
            }
        };
        detour.call(parent);

        // 2. Read the active player side from the parent.
        let side_ptr = parent.add(PARENT_PLAYER_SIDE_OFFSET) as *const i32;
        let side_raw = std::ptr::read_unaligned(side_ptr);
        if !(0..=1).contains(&side_raw) {
            log_warn!(
                "custom_options/builder_hook: parent+0x228 = {} (expected 0 or 1) — skipping mod rows",
                side_raw
            );
            return;
        }
        let side = side_raw as u8;

        // Notify menu-open subscribers (e.g. the WebUI preview overlay). The
        // native rows — including each option's `image_usr` preview clip — now
        // exist, so this is the correct point to read box geometry or build
        // overlays. Fired regardless of whether any mod rows get injected below.
        super::fire_menu_open(side);

        // 3. Snapshot the list of registered option handles to inject.
        // Capture the UI kind alongside the id so we can dispatch to the
        // right allocation path (enum vs scalar donor) without taking
        // the registry lock again per-row. Dropping the lock before the
        // game-allocator call keeps the ctor's potential re-entry into
        // custom_options state safe.
        let handles: Vec<(OptionHandle, String, RowKindTag)> = {
            let state = registry::STATE.lock().unwrap();
            state
                .options
                .iter()
                .enumerate()
                // Availability filter (`set_option_available`): unavailable
                // options are silently not injected — registration, handles,
                // values, and persistence stay intact, and because rows only
                // exist per open, an already-open form is never mutated.
                // Placement filter rides the same predicate: a row whose
                // RESOLVED in-game placement is false (operator
                // `option_menu_settings` override wins over the registered
                // `MenuPlacement`) is not injected either — headers included.
                .filter(|(_, opt)| {
                    let in_game = super::ordering::placement_override_for(&opt.id)
                        .0
                        .unwrap_or(opt.menus.in_game);
                    opt.available && in_game
                })
                .map(|(i, opt)| {
                    let kind = match &opt.ui_kind {
                        UiKind::Enum { .. } => RowKindTag::Enum,
                        UiKind::Scalar { .. } => RowKindTag::Scalar,
                        UiKind::Header => RowKindTag::Header,
                    };
                    (OptionHandle(i as u32), opt.id.clone(), kind)
                })
                .collect()
        };

        // Reorder the snapshot per the operator's configured
        // `option_menu_settings`
        // (identity when unconfigured/empty). This is the sole lever for row
        // display order: the loop below injects rows in this order, and both
        // the scene-graph row vector and `rows::ROWS` follow it. The registry
        // and every `OptionHandle` index stay untouched (each tuple carries
        // its option's true registry index, availability-filtered rows
        // included), so handles remain valid. Header rows are injected ONLY
        // when listed in `option_menu_settings` (R10) — `display_order_for` excludes
        // unlisted headers from the returned order, so they simply never
        // reach the injection loop.
        let handles: Vec<(OptionHandle, String, RowKindTag)> = {
            let ids: Vec<&str> = handles.iter().map(|(_, id, _)| id.as_str()).collect();
            let is_header: Vec<bool> = handles
                .iter()
                .map(|(_, _, kind)| *kind == RowKindTag::Header)
                .collect();
            let perm = super::ordering::display_order_for(&ids, &is_header);
            perm.into_iter().map(|i| handles[i].clone()).collect()
        };

        if handles.is_empty() {
            // Still let the log tell operators the detour fired — useful
            // for confirming hook installation without needing any mod
            // to have registered an option.
            log_info!(
                "custom_options/builder_hook: native builder done (side={side}); no mod options registered"
            );
            return;
        }

        // 4. For each registered option, allocate a row and register it.
        let register_fn = match FN_OPTION_TAB_REGISTER {
            Some(f) => f,
            None => {
                log_error!(
                    "custom_options/builder_hook: register fn not populated in callback — logic bug"
                );
                return;
            }
        };

        // The game frees old rows (via donor dtor → CRT free) when the menu
        // closes. Clear stale entries for this side so the ROWS vec doesn't
        // accumulate dangling pointers from prior menu opens.
        rows::clear_side(side);

        let mut injected = 0usize;
        for (handle, id, kind) in &handles {
            let row = match kind {
                RowKindTag::Enum => rows::allocate_row_for_option(*handle, side),
                RowKindTag::Scalar => rows::allocate_scalar_row_for_option(*handle, side),
                RowKindTag::Header => rows::allocate_header_row_for_option(*handle, side),
            };
            let row = match row {
                Some(r) => r,
                None => continue,
            };
            let mut parent_slot: *mut u8 = parent;
            let parent_slot_ptr: *mut *mut u8 = &mut parent_slot;
            register_fn(parent_slot_ptr, row);
            // One-shot label bind at injection time for enum and header rows.
            // Scalar rows bind their label per-frame inside
            // `render_scalar_trampoline` because `row+0x118` is only
            // populated once the row becomes visible on a tab switch
            // (populated by the inherited fourth-MI visibility
            // handler, which hasn't fired by the time we get here).
            // (Enum and header renders also re-bind per frame; this early
            // bind just avoids a first-frame flash when possible.)
            if matches!(kind, RowKindTag::Enum | RowKindTag::Header) {
                rows::bind_textures(*handle, row);
            }
            injected += 1;
            log_info!(
                "custom_options/builder_hook: injected {kind:?} row for {id:?} side={side} @ {:p}",
                row
            );
        }

        log_info!(
            "custom_options/builder_hook: native builder done (side={side}); injected {injected} mod row(s)"
        );
    }
}
