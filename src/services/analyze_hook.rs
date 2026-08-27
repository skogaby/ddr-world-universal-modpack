//! Analyze Hook Service — shared dispatcher for `IStepReader::Analyze`
//! (`step_reader_analyze`, gamemdx `FUN_1801c8680`).
//!
//! Installs exactly one `retour::GenericDetour` on Analyze. The detour runs
//! the ORIGINAL first, then dispatches every registered post-subscriber in
//! registration order, each wrapped in `catch_unwind`. It returns the
//! original's `u8` result unchanged.
//!
//! Rationale (identical to `judge_hook`): stacking two independent
//! `GenericDetour` handles on one target does not compose — the second
//! detour's "call original" captures the first's jmp, so one subscriber
//! silently bypasses the other. NoteTypesExpansion (mine injection) and
//! the ultrafast-boot capture BOTH need this boundary, so they register
//! here instead of each installing a detour.
//!
//! Subscribers are plain `fn` pointers (invoked from an `extern "C"`
//! context, so no captured state); per-subscriber state lives in the
//! subscriber's own statics. Post-subscribers only READ the analysis
//! outputs and/or mutate the notes vector; they are order-independent of
//! each other (capture reads `result`/`radar`/`ret`; NTX mutates `notes`).

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::sync::{Mutex, OnceLock};

use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};

/// The Analyze member function (MSVC x64 ABI). Signature confirmed via
/// Ghidra RTTI `.?AVIStepReader@step@@` (see
/// `src/mods/note_types_expansion/hooks.rs` for the full arg map):
/// `RCX this, RDX notes, R8 measures, R9 result, [+0x28] radar,
/// [+0x30] mode, [+0x38] difficulty, [+0x40] option -> AL bool`.
pub type AnalyzeFn = unsafe extern "C" fn(
    *mut u8,   // this   — step::SsqReader*
    *mut u8,   // notes  — per-note-record vector
    *mut u8,   // measures
    *mut u8,   // result — 14-int output block (may be null)
    *mut u8,   // radar  — 5-int output block
    i32,       // mode   — 0 single, 1 double
    i32,       // difficulty — 0..4
    *const u8, // option
) -> u8;

/// Read-only view of one Analyze invocation, handed to post-subscribers.
pub struct AnalyzeArgs {
    pub this: *mut u8,
    pub notes: *mut u8,
    pub measures: *mut u8,
    pub result: *mut u8,
    pub radar: *mut u8,
    pub mode: i32,
    pub difficulty: i32,
    pub option: *const u8,
}

/// Post-original subscriber: `(args, original_return)`.
pub type AnalyzePostFn = fn(&AnalyzeArgs, u8);

static DETOUR: OnceLock<GenericDetour<AnalyzeFn>> = OnceLock::new();
static POST_SUBSCRIBERS: Lazy<Mutex<Vec<AnalyzePostFn>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Install the shared Analyze detour. Idempotent: a second call with the
/// detour already armed returns `true`. Returns `false` only if the
/// signature is unresolved or the detour fails to arm — subscribers then
/// simply never fire (callers fail open).
pub fn init(signatures: &SignatureStore) -> bool {
    if DETOUR.get().is_some() {
        return true; // already installed
    }
    let addr = match signatures.get_address("step_reader_analyze") {
        Some(a) => a,
        None => {
            log_warn!("AnalyzeHook: step_reader_analyze unresolved -- Analyze subscribers inert");
            return false;
        }
    };

    unsafe {
        let target: AnalyzeFn = std::mem::transmute(addr);
        let detour = match GenericDetour::new(target, dispatcher) {
            Ok(d) => d,
            Err(e) => {
                log_warn!("AnalyzeHook: failed to create detour: {}", e);
                return false;
            }
        };
        // Publish BEFORE enabling: once the prologue is patched any thread
        // can enter the dispatcher, which needs `.get()` to reach the
        // original (store-before-enable rule).
        if DETOUR.set(detour).is_err() {
            log_warn!("AnalyzeHook: detour slot already populated");
            return false;
        }
        if let Some(Err(e)) = DETOUR.get().map(|d| d.enable()) {
            log_warn!("AnalyzeHook: failed to enable detour: {}", e);
            return false;
        }
    }
    log_info!(
        "AnalyzeHook: installed shared Analyze dispatcher @ {:p}",
        addr
    );
    true
}

/// True once the shared detour is armed.
pub fn is_available() -> bool {
    DETOUR.get().is_some()
}

/// Register a post-original subscriber. Fires on every Analyze call in
/// registration order after the original returns.
pub fn register_post(cb: AnalyzePostFn) {
    if let Ok(mut subs) = POST_SUBSCRIBERS.lock() {
        subs.push(cb);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn dispatcher(
    this: *mut u8,
    notes: *mut u8,
    measures: *mut u8,
    result: *mut u8,
    radar: *mut u8,
    mode: i32,
    difficulty: i32,
    option: *const u8,
) -> u8 {
    // Call the original first (mirrors both subscribers' prior behavior).
    let orig_ret = match DETOUR.get() {
        Some(d) => d.call(
            this, notes, measures, result, radar, mode, difficulty, option,
        ),
        None => 0,
    };

    let args = AnalyzeArgs {
        this,
        notes,
        measures,
        result,
        radar,
        mode,
        difficulty,
        option,
    };

    // Snapshot the subscriber list so the lock isn't held across callbacks.
    let subs: Vec<AnalyzePostFn> = POST_SUBSCRIBERS
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    for cb in subs {
        // A subscriber panic must never cross the extern "C" boundary.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb(&args, orig_ret)));
    }

    orig_ret
}
