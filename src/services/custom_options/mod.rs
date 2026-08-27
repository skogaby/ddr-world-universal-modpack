//! Custom Player Options Framework — registration + UI injection service.
//!
//! Mods register option rows declaratively via [`register_option`], passing a
//! [`RegisterSpec`][api::RegisterSpec]. Registrations succeed synchronously
//! and the framework primes the per-player value cache from `default_value`
//! before returning. Change callbacks fire on initial load, user-driven
//! value changes, and persistence round-trips.
//!
//! Concurrency model:
//!   - All state reads and writes go through a single [`std::sync::Mutex`]
//!     in [`registry`]. Holding the lock during framework-internal work is
//!     fine; holding it across an [`api::OnChangeFn`] invocation is not.
//!   - Write paths (`register_option`, `resolve_from_load`) mutate state
//!     under the lock, then release the lock, then invoke the callback.
//!     Callbacks are wrapped in [`std::panic::catch_unwind`]; a panicking
//!     callback is logged at ERROR and replaced with a no-op so subsequent
//!     change notifications are silently dropped for that option without
//!     compromising handle stability.
//!
//! This module owns the top-level service lifecycle and the public API
//! entry points; type definitions live in [`api`] and the mutable state
//! behind the mutex lives in [`registry`].

pub mod api;
pub mod asset_gen;
pub mod builder_hook;
pub mod dtor_hook;
pub mod filter_hook;
pub mod observers;
pub mod ordering;
pub mod registry;
pub mod rows;

#[cfg(test)]
mod availability_tests;
#[cfg(test)]
mod header_rows_tests;
#[cfg(test)]
mod persist_matrix_tests;
#[cfg(test)]
mod scalar_bounds_tests;
#[cfg(test)]
mod scalar_format_tests;

#[allow(unused_imports)]
pub use api::{
    EnumValue, MenuPlacement, OnChangeFn, OptionHandle, PageTag, PersistMode, RegisterError,
    RegisterSpec, ScalarFormat, ShowWhen, UiKind,
};
pub use asset_gen::flush_label_atlas;
#[allow(unused_imports)]
pub use observers::{subscribe_value_changed, unsubscribe_value_changed};
#[allow(unused_imports)]
pub use registry::{OverlayRowInfo, OverlayRowKind};

use once_cell::sync::Lazy;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::{log_error, log_info, log_warn};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Callback fired when the native options modal opens or closes, for one
/// player side (`0` = P1, `1` = P2). Plain `fn` pointers (no capture) keep
/// dispatch allocation- and `Send`-free — subscribers stash any state in
/// their own statics. Both edges fire on the game's render thread: open from
/// the row-builder detour (after all native rows + their `image_usr` preview
/// clips exist), close from the `OptionForm` destructor detour (before the
/// form is torn down). This is the modal (not scene) lifecycle — the options
/// screen is a child of scene 25 (`SONG_SELECT`), so scene-change events are
/// too coarse to track it.
pub type MenuLifecycleFn = fn(player_side: u8);

/// Callback fired when the options menu asks a mod row for its preview-image
/// name — i.e. when that row becomes (or stays) the focused row and the
/// preview box is refreshed. `player_side` is `0` (P1) / `1` (P2); `option_id`
/// is the focused row's registered id. Fires on the game's render thread from
/// the `IOptionElement` slot-0 getter (see `docs/option_preview_image_box.md`),
/// which the native preview observer invokes on the focused row on every focus
/// change (and on a value change within the focused row, since native per-value
/// previews re-evaluate it). This is the "focused customizer value changed"
/// signal the WebUI preview overlay drives its on-demand art from.
///
/// The `option_id` is borrowed for the duration of the call — subscribers must
/// not retain it. Register once at mod enable time; there is no unsubscribe.
pub type PreviewRequestFn = fn(player_side: u8, option_id: &str);

static MENU_OPEN_CBS: Lazy<Mutex<Vec<MenuLifecycleFn>>> = Lazy::new(|| Mutex::new(Vec::new()));
static MENU_CLOSE_CBS: Lazy<Mutex<Vec<MenuLifecycleFn>>> = Lazy::new(|| Mutex::new(Vec::new()));
static PREVIEW_REQUEST_CBS: Lazy<Mutex<Vec<PreviewRequestFn>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// One-time service init. Initializes the row-allocation submodule
/// (resolving game function pointers via the signature store), then logs
/// startup and runs the registration smoke test. Returns `true` on
/// success. Idempotent — re-calls are no-ops that still return `true`.
///
/// `signatures` must already have had its derived addresses resolved
/// (i.e. `resolve_derived` called) so that the row-allocation module can
/// pick up `option_element_arrowcolor_primary_vtable`, `string_assign`,
/// and `metadata_insert`. If any required address is missing, row
/// allocation degrades gracefully — the service still initializes and
/// registrations still succeed, but attempts to allocate rows return
/// `None` with a WARN line.
pub fn init(signatures: &SignatureStore) -> bool {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return true;
    }
    log_info!("custom_options: init");
    asset_gen::generate_static_tab_assets();
    // The native Back (tab 6) is repurposed as the Mods tab. libafp silently
    // drops appended placements past the native 6 in option_tab_list, so the
    // tab strip cannot be extended to 7 slots; the Mods tab reuses slot 6
    // with its textures overridden, and a separate "BACK [7]" indicator is
    // rendered elsewhere in the options scene.
    if !rows::init(signatures) {
        log_warn!(
            "custom_options: row allocation unavailable — custom option rows will not render"
        );
    }
    if !builder_hook::init(signatures) {
        log_warn!(
            "custom_options: builder-hook unavailable — registered options will not appear in the options menu"
        );
    }
    if !filter_hook::init(signatures) {
        log_warn!(
            "custom_options: filter-hook unavailable — Page6 content rows will not appear under the Mods tab"
        );
    } else {
        FILTER_READY.store(true, Ordering::Release);
    }
    if !dtor_hook::init(signatures) {
        log_warn!(
            "custom_options: OptionForm-dtor hook unavailable — stale option rows cleared only on next menu open"
        );
    }

    // Operator-defined row order + placement overrides
    // (custom_options.option_menu_settings). Absent or empty ⇒ registration
    // order, no overrides. Read once here; the order is applied by the
    // builder hook on each menu open, the placement overrides by the
    // placement consumers. `custom_options_persistence` already reads
    // crate::mods::config at its init, so this is an established dependency.
    // (The retired `row_order` key is no longer read — serde silently
    // ignores a leftover key.)
    let configured_settings = crate::mods::config::get()
        .and_then(|c| c.custom_options.as_ref())
        .and_then(|c| c.option_menu_settings.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|s| ordering::OptionMenuSetting {
            id: s.id,
            overlay: s.overlay,
            in_game: s.in_game,
        })
        .collect();
    ordering::set_configured_settings(configured_settings);

    true
}

/// Has `init()` been called successfully?
pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}

/// Latched at [`init`]: the Mods-tab filter hook installed (without it,
/// Page6 content rows never become visible — load-bearing for any row).
static FILTER_READY: AtomicBool = AtomicBool::new(false);

/// STRICT row-injection readiness: the service initialized AND the row
/// allocator can build scalar rows (donor ctor/vtable, coarse-step event,
/// text layer) AND the builder detour is actually installed AND the
/// Mods-tab filter hook is live. Defaults false until full initialization.
///
/// This is deliberately stricter than [`is_available`] (which only says
/// `init()` ran): a mod whose row would silently fail to allocate or never
/// become visible should treat the row as unavailable rather than register
/// inert UI. Registration itself is still safe while this is false —
/// registration/persistence are independent of injection.
pub fn row_injection_available() -> bool {
    is_available()
        && rows::is_scalar_ready()
        && builder_hook::is_ready()
        && FILTER_READY.load(Ordering::Acquire)
}

/// Flip a registered option's injection availability. While unavailable the
/// builder hook omits the row from its per-open snapshot (an already-open
/// form is never mutated — rows exist only per open); registration, the
/// handle, per-side values, and persistence are untouched, so a boot-
/// disabled mod can be enabled later and simply flip its row back on for
/// the next form rebuild. Unknown ids WARN and no-op.
pub fn set_option_available(id: &str, available: bool) {
    let Ok(mut state) = registry::STATE.lock() else {
        return;
    };
    if !state.set_available(id, available) {
        log_warn!("custom_options: set_option_available('{id}') — id is not registered");
    }
}

/// Register an option. On success, returns an [`OptionHandle`] and primes
/// both players' caches to `spec.default_value`. The option's `on_change`
/// callback is invoked immediately for each side with the primed value, so
/// mods that cache per-player state in atomics see a consistent initial
/// state without having to also read [`get_value`] at enable time.
///
/// On failure, returns a [`RegisterError`] describing the reason. All
/// failures are recoverable: the mod continues, the option simply isn't
/// registered.
pub fn register_option(spec: RegisterSpec) -> Result<OptionHandle, RegisterError> {
    if !is_available() {
        return Err(RegisterError::NotInitialized);
    }

    let id = spec.id;
    let default_value = spec.default_value;
    let on_change = spec.on_change;
    let registered_in_game = spec.menus.in_game;

    let handle = {
        let mut state = registry::STATE.lock().unwrap();
        match state.try_register(spec) {
            Ok(h) => h,
            Err(e) => {
                match &e {
                    RegisterError::Duplicate { .. } | RegisterError::UnknownParent { .. } => {
                        log_error!("custom_options: {e}");
                    }
                    _ => {
                        log_warn!("custom_options: {e}");
                    }
                }
                return Err(e);
            }
        }
    };

    log_info!("custom_options: registered {id:?} (default={default_value})");

    // Generate (or regenerate) the row-label atlas entry for this option.
    // The PNG must exist at data_mods/.../tex/seop_item_<id>.png; the
    // cloner warns and skips if missing without blocking registration.
    // Skipped when the row's RESOLVED in-game placement is false (overlay-
    // only rows never render an in-game label; config override wins) —
    // fail-open: any doubt generates.
    let effective_in_game = ordering::placement_override_for(id)
        .0
        .unwrap_or(registered_in_game);
    if effective_in_game {
        asset_gen::register_label_for(id);
    } else {
        log_info!("custom_options: {id:?} is not placed in-game — label texture skipped");
    }

    // Likewise register the preview-image texture(s) shown in the options
    // preview box when this row is focused: the base `seop_image_<id>` plus
    // any per-value `seop_image_<id>_<key>` from enum values carrying a
    // `preview_key`. Each ships at data_mods/.../tex/<name>.png; a missing
    // PNG → blank preview box, not an error. All ride the same lang_eng atlas
    // flush at init as the labels.
    {
        let state = registry::STATE.lock().unwrap();
        let preview_names = state.preview_image_names_for(id);
        let ribbon_names = state.ribbon_texture_names_for(id);
        drop(state);
        asset_gen::register_preview_images(&preview_names);
        // Net-new value-ribbon chips (seop_op_<key>) for enum options; stock
        // ribbons (seop_op_on/off) are filtered out inside register_op_ribbons.
        asset_gen::register_op_ribbons(&ribbon_names);
    }

    // Fire the initial change callback for both sides outside the lock.
    dispatch_callback(id, on_change, 0, default_value);
    dispatch_callback(id, on_change, 1, default_value);

    Ok(handle)
}

/// Query the current resolved value for `(player_side, option_id)`. Returns
/// `None` if the id isn't registered, `player_side` is not 0 or 1, or the
/// registry lock is poisoned (read-only query — degrade, don't panic; this is
/// called from render-thread hot paths like the preview-request subscriber).
pub fn get_value(player_side: u8, option_id: &str) -> Option<i32> {
    if !is_available() {
        return None;
    }
    let state = registry::STATE.lock().ok()?;
    state.get_value(option_id, player_side)
}

/// Programmatically set an option's value for a specific player side.
/// Fires the option's change callback only if the value actually changed.
/// No-ops if the id isn't registered, value is unchanged, or side >= 2.
///
/// Use this for "shared" options where toggling one side should mirror
/// to the other — call `set_value` for the other side from inside the
/// `on_change` callback. Because the check prevents re-firing when the
/// value is already set, the sync terminates naturally without recursion.
pub fn set_value(option_id: &str, player_side: u8, value: i32) {
    if !is_available() {
        return;
    }
    let callback_info = {
        let mut state = registry::STATE.lock().unwrap();
        let current = state.get_value(option_id, player_side);
        if current == Some(value) {
            return;
        }
        state.set_value(option_id, player_side, value)
    };
    if let Some((cb, side, v)) = callback_info {
        dispatch_callback(option_id, cb, side, v);
        observers::dispatch(option_id, side, v);
    }
}

/// Write a value into an option's per-player cache after a persistence load
/// (or any other non-user-driven update). Fires the option's change
/// callback with the new value. No-ops if the id isn't registered, or if the
/// option's [`PersistMode`] doesn't accept loads
/// ([`PersistMode::loaded_from_network`]) — this is the single load-side
/// gate: both the network `load_receiver` and the JSON-prime timer funnel
/// through here, so `SaveOnly`/`None`/`Session` options are inert on every
/// load path.
pub(crate) fn resolve_from_load(option_id: &str, player_side: u8, value: i32) {
    if !is_available() {
        return;
    }
    let (callback_info, changed) = {
        let mut state = registry::STATE.lock().unwrap();
        // Load-side persistence gate (the PersistMode matrix).
        let idx = match state.index_of(option_id) {
            Some(i) => i,
            None => return,
        };
        if !state.options[idx].persist.loaded_from_network() {
            return;
        }
        // Apply the option's load_transform (if any) before caching.
        let stored = match state.options[idx].load_transform {
            Some(f) => f(option_id, value),
            None => value,
        };
        let prior = state.get_value(option_id, player_side);
        (
            state.set_value(option_id, player_side, stored),
            prior != Some(stored),
        )
    };
    if let Some((cb, side, v)) = callback_info {
        // on_change fires on every accepted load (existing contract — mods
        // sync state from it); the OBSERVER is changed-only.
        dispatch_callback(option_id, cb, side, v);
        if changed {
            observers::dispatch(option_id, side, v);
        }
    }
}

/// Set an option's per-side value WITHOUT dispatching its `on_change`
/// callback. For non-user-driven state *seeding* — e.g. reading the game's
/// own loaded state into the menu registry — where firing `on_change` would
/// cause an unwanted write-back into game memory. Contrast [`set_value`] /
/// [`resolve_from_load`], which dispatch the callback.
///
/// No-ops if the service is uninitialized, the id isn't registered,
/// `player_side >= 2`, or the registry lock is poisoned (seeding is a
/// best-effort read-only sync — degrade, don't panic).
pub fn set_value_silent(option_id: &str, player_side: u8, value: i32) {
    if !is_available() {
        return;
    }
    let mut state = match registry::STATE.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    // Silent means the option's own on_change is NOT dispatched — but the
    // value-changed OBSERVER still fires (after the lock drops) when the
    // value actually changed: the overlay mirror must see every mutation,
    // seeding included, without spam from unchanged per-frame re-seeds.
    if state.get_value(option_id, player_side) == Some(value) {
        return;
    }
    let changed = state.set_value(option_id, player_side, value);
    drop(state);
    if let Some((_cb, side, v)) = changed {
        observers::dispatch(option_id, side, v);
    }
}

/// Whether option `id` participates in the offline JSON cache
/// ([`PersistMode::json_cached`] — `Full` only). Consulted by the
/// persistence service's JSON writer so `SaveOnly`/`None`/`Session` options
/// never enter `mod-config.json`. Unregistered ids return `false`.
pub(crate) fn json_persisted(option_id: &str) -> bool {
    if !is_available() {
        return false;
    }
    let state = match registry::STATE.lock() {
        Ok(s) => s,
        Err(_) => return false,
    };
    match state.index_of(option_id) {
        Some(i) => state.options[i].persist.json_cached(),
        None => false,
    }
}

/// Card-in reset: restore every [`PersistMode::Session`] option to its
/// `default_value` for the carded-in `player_side`, then fire the affected
/// options' change callbacks (after the lock is released — the
/// [`resolve_from_load`] dispatch pattern). Called by the persistence
/// service's SONG_SELECT card-in drain, the same lifecycle point where
/// `Full` values land, so a new player session never inherits the previous
/// session's practice-tool state. No-op resets (value already at default)
/// dispatch nothing.
pub(crate) fn reset_session_values(player_side: u8) {
    if !is_available() {
        return;
    }
    let callbacks = {
        let mut state = match registry::STATE.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        state.reset_session_values(player_side)
    };
    if callbacks.is_empty() {
        return;
    }
    log_info!(
        "custom_options: card-in reset — {} session option(s) restored to defaults for side {}",
        callbacks.len(),
        player_side
    );
    for (id, cb, side, value) in callbacks {
        dispatch_callback(&id, cb, side, value);
        observers::dispatch(&id, side, value);
    }
}

/// Re-bound a registered scalar option's `min`/`max` at runtime (the
/// Training Mode rows' per-song ranges — the stepper and the row's
/// position marker consume the registry live, so the new range is
/// effective the same frame; an open menu repaints via the per-frame
/// render tick). Stored values outside the new range are clamped into it
/// and their change callbacks fired (after the lock is released — the
/// standard deferred-dispatch contract). Returns whether the bounds were
/// applied (`false` for unknown ids, non-scalar rows, inverted bounds,
/// or an uninitialized service).
pub fn set_scalar_bounds(option_id: &str, min: i32, max: i32) -> bool {
    if !is_available() {
        return false;
    }
    let callbacks = {
        let mut state = match registry::STATE.lock() {
            Ok(s) => s,
            Err(_) => return false,
        };
        match state.set_scalar_bounds(option_id, min, max) {
            Some(callbacks) => callbacks,
            None => return false,
        }
    };
    for (id, cb, side, value) in callbacks {
        dispatch_callback(&id, cb, side, value);
        observers::dispatch(&id, side, value);
    }
    true
}

/// Build the overlay menu's plain-data mirror of the registered options for
/// `side` (overlay-menu rewrite design §4.3.4): availability ⊕ resolved
/// overlay placement (operator `option_menu_settings` override wins over the
/// registered `MenuPlacement`) ⊕ configured display order (the SAME
/// permutation source as the in-game builder hook) ⊕ live scalar bounds ⊕
/// per-side `ShowWhen` (reported via `visible`, not filtered), with scalar
/// text formatted identically to the in-game rows. One STATE lock; the
/// returned rows are owned data — consume lock-free.
pub fn overlay_snapshot(side: u8) -> Vec<OverlayRowInfo> {
    if !is_available() {
        return Vec::new();
    }
    let state = match registry::STATE.lock() {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    registry::overlay_snapshot_rows(
        &state,
        side,
        &|id| ordering::placement_override_for(id).1,
        &ordering::display_order_for,
    )
}

/// Opaque pointer-bearing handle for a single mod-owned row. Handed to the
/// scroll driver so it can manipulate per-row state (`+0xB8` active byte,
/// `+0x30` focus byte) without importing the private [`rows::RowSlot`]
/// layout.
///
/// The pointer is valid only as long as its row is tracked in [`rows::ROWS`]:
/// the game frees rows when the options overlay closes, and the OptionForm-dtor
/// hook removes the corresponding slots at that moment. Handles are therefore
/// obtained fresh (via [`row_handles_for_tab`]) for each scroll/visibility pass
/// while the menu is live, never cached across a menu close. `RowHandle` is
/// `Copy` so the scroll driver can snapshot a tab's handle list without
/// fighting borrow state.
#[derive(Copy, Clone, Debug)]
pub struct RowHandle {
    pub row_ptr: *mut u8,
    /// Whether the cursor may land on this row. `false` for header rows
    /// (their `+0x28` selectability interface returns 0). The scroll
    /// driver's window-step path computes focus targets itself — bypassing
    /// the native scan that honors the `+0x28` predicate — so it must skip
    /// unselectable rows using this flag.
    pub selectable: bool,
}

// SAFETY: `row_ptr` targets game memory that's valid while the row is tracked
// in ROWS (see above); the value is read-only from the handle itself.
// Cross-thread passing is sound for the same reasons `unsafe impl Send` is
// valid on the rows registry: the pointer is stable for the row's lifetime and
// the only mutation paths go through the single-threaded game render thread.
unsafe impl Send for RowHandle {}
unsafe impl Sync for RowHandle {}

/// Count the mod rows registered for the given `(player_side, page)` tab.
/// Used by the scroll driver to decide whether the tab's row count
/// exceeds viewport capacity.
///
/// Rows are currently all tagged [`PageTag::Page6`]; queries with a
/// different tag return `0` until multi-page tagging is added.
pub(crate) fn row_count_for_tab(player_side: u8, page: PageTag) -> usize {
    if !is_available() || page != PageTag::Page6 {
        return 0;
    }
    rows::row_ptrs_for_side(player_side).len()
}

/// Enumerate mod rows registered for the given `(player_side, page)` tab.
/// Returned in registration order (same order as the underlying
/// [`rows::ROWS`] vector), which mirrors the order the builder hook
/// injects them into the scene-graph row vector on the same side.
///
/// Rows are currently all tagged [`PageTag::Page6`]; queries with a
/// different tag return an empty vec.
pub(crate) fn row_handles_for_tab(player_side: u8, page: PageTag) -> Vec<RowHandle> {
    if !is_available() || page != PageTag::Page6 {
        return Vec::new();
    }
    rows::row_entries_for_side(player_side)
        .into_iter()
        .map(|(row_ptr, selectable)| RowHandle {
            row_ptr,
            selectable,
        })
        .collect()
}

/// Resolve a layout-container pointer (handed in by focus-update or tab-
/// filter hooks as RCX) to the player side whose OptionForm owns it.
/// Returns `None` when no mod row has yet been injected into this
/// container — i.e. the tab has zero mod rows, so no scroll work is
/// needed anyway.
pub(crate) fn side_for_container(container: *mut u8) -> Option<u8> {
    if !is_available() {
        return None;
    }
    rows::side_for_container(container)
}

/// Snapshot of the current per-player values for every registered option
/// that participates in network save ([`PersistMode::saved_to_network`] —
/// `Full` + `SaveOnly`; `None` and `Session` are excluded). Returned as
/// `Vec<(id, [p1_value, p2_value])>`. Used by the persistence service at
/// save time to emit `<mod_{id}>` kbin children.
pub(crate) fn snapshot_for_save() -> Vec<(String, [i32; 2])> {
    if !is_available() {
        return Vec::new();
    }
    let state = registry::STATE.lock().unwrap();
    state
        .options
        .iter()
        .filter(|o| o.persist.saved_to_network())
        .map(|o| {
            let vals = match o.save_transform {
                Some(f) => [f(&o.id, o.values[0]), f(&o.id, o.values[1])],
                None => o.values,
            };
            (o.id.clone(), vals)
        })
        .collect()
}

/// Subscribe to options-modal open events. `cb` fires once per player side
/// each time the modal opens, on the render thread, from inside the row-builder
/// detour (after all native rows and their `image_usr` preview clips exist —
/// the right moment to read box geometry or build overlays). Register once at
/// mod enable time; there is no unsubscribe (callbacks live for the process).
pub fn on_menu_open(cb: MenuLifecycleFn) {
    if let Ok(mut cbs) = MENU_OPEN_CBS.lock() {
        cbs.push(cb);
    }
}

/// Subscribe to options-modal close events. `cb` fires once per player side
/// when the modal closes, on the render thread, from inside the `OptionForm`
/// destructor detour (before the form is freed — the right moment to hide/
/// release overlays). Register once at mod enable time; no unsubscribe.
pub fn on_menu_close(cb: MenuLifecycleFn) {
    if let Ok(mut cbs) = MENU_CLOSE_CBS.lock() {
        cbs.push(cb);
    }
}

/// One-shot latch for lifecycle-subscriber panic logging: a panicking
/// subscriber is caught (it must never unwind into game code) but would
/// otherwise be invisible — and the preview-request event fires at focus-tick
/// rate, so per-occurrence logging would spam. First caught panic per event
/// kind logs at ERROR; the rest stay silent.
static LIFECYCLE_PANIC_LOGGED: AtomicBool = AtomicBool::new(false);

/// Run one lifecycle subscriber panic-isolated; log the FIRST caught panic
/// (across all lifecycle events) at ERROR so a wedged subscriber is
/// diagnosable from the log.
fn dispatch_lifecycle(event: &str, side: u8, cb: MenuLifecycleFn) {
    if panic::catch_unwind(AssertUnwindSafe(|| cb(side))).is_err()
        && !LIFECYCLE_PANIC_LOGGED.swap(true, Ordering::AcqRel)
    {
        log_error!(
            "custom_options: a {event} subscriber panicked (side={side}); \
             further subscriber panics will be suppressed silently"
        );
    }
}

/// Fire all registered menu-open callbacks for `side`. Called from the
/// builder-hook detour. Each callback is panic-isolated so one bad subscriber
/// can't unwind into game code or block the others. The list is snapshotted
/// before dispatch so a callback that (re-)subscribes can't deadlock on the lock.
pub(crate) fn fire_menu_open(side: u8) {
    let cbs = match MENU_OPEN_CBS.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    for cb in cbs {
        dispatch_lifecycle("menu-open", side, cb);
    }
}

/// Fire all registered menu-close callbacks for `side`. Called from the
/// OptionForm-dtor detour. Panic-isolated + snapshotted, as `fire_menu_open`.
pub(crate) fn fire_menu_close(side: u8) {
    let cbs = match MENU_CLOSE_CBS.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    for cb in cbs {
        dispatch_lifecycle("menu-close", side, cb);
    }
}

/// Subscribe to preview-request events. `cb` fires whenever the options menu
/// asks a mod row for its preview-image name — the focused-row signal the
/// WebUI preview overlay uses to load the focused customizer value's art on
/// demand. Fires on the render thread from the `IOptionElement` slot-0 getter.
/// Register once at mod enable time; there is no unsubscribe.
pub fn on_preview_request(cb: PreviewRequestFn) {
    if let Ok(mut cbs) = PREVIEW_REQUEST_CBS.lock() {
        cbs.push(cb);
    }
}

/// Fire all registered preview-request callbacks for the focused row. Called
/// from the `IOptionElement` slot-0 preview-name getter in [`rows`], which the
/// native preview observer invokes only on the focused mod row. Panic-isolated
/// + snapshotted, as the menu-lifecycle fires. Runs inside the getter (render
/// thread), so subscribers must not block or re-enter the rows lock. A
/// panicking subscriber logs once (this event fires at focus-tick rate, so
/// per-occurrence logging would spam).
pub(crate) fn fire_preview_request(side: u8, option_id: &str) {
    let cbs = match PREVIEW_REQUEST_CBS.lock() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    for cb in cbs {
        if panic::catch_unwind(AssertUnwindSafe(|| cb(side, option_id))).is_err()
            && !LIFECYCLE_PANIC_LOGGED.swap(true, Ordering::AcqRel)
        {
            log_error!(
                "custom_options: a preview-request subscriber panicked \
                 (side={side}, option={option_id}); further subscriber panics \
                 will be suppressed silently"
            );
        }
    }
}

/// Invoke a change callback with panic isolation. On panic, logs at ERROR
/// and replaces the option's callback with a no-op so subsequent changes
/// for this option don't repeatedly panic in the same way.
fn dispatch_callback(option_id: &str, cb: OnChangeFn, side: u8, value: i32) {
    let result = panic::catch_unwind(AssertUnwindSafe(|| cb(side, value)));
    if result.is_err() {
        log_error!(
            "custom_options: change callback for {option_id:?} panicked \
             (side={side}, value={value}); suppressing future callbacks for this option"
        );
        let mut state = registry::STATE.lock().unwrap();
        if let Some(idx) = state.index_of(option_id) {
            state.options[idx].on_change = noop_on_change;
        }
    }
}

fn noop_on_change(_side: u8, _value: i32) {}
