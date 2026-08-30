//! The `dance_judge` AFP patch: registration with the shared
//! `afp_patcher` seam, the patch fn itself, and the confidence flags
//! task-03's flash re-drive gates on.
//!
//! The doc-transform core is `core/ap2`'s host-tested
//! [`Ap2Doc::clone_word_segment_with_new_shape`] recipe — this module is
//! the thin impure wiring around it: staged-state storage, the v1 skin
//! gate, latched WARNs, and the ready/applied atomics.
//!
//! Lifecycle: [`activate`] (mod enable) stages the assets via
//! [`super::assets::stage`] and registers the patch fn ONCE (afp_patcher
//! has no unregister — the fn body checks [`PATCH_READY`], so
//! [`deactivate`] making it return `None` restores stock streaming for
//! subsequent template loads). Fail-open everywhere: any refusal streams
//! stock bytes with one latched WARN naming the reason (AC-3).
//!
//! PANIC SAFETY: the afp_patcher hook does NOT catch_unwind around patch
//! fns — everything in [`patch_dance_judge`] is Option-chained, no
//! unwrap/index.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

use once_cell::sync::Lazy;

use crate::core::ap2::Ap2Doc;
use crate::services::afp_patcher;
use crate::{log_info, log_warn};

use super::assets::{self, StagedPatch, NEW_LABEL, SRC_LABEL, TEMPLATE_NAME};

/// Assets staged + patch registered + mod enabled. Cleared by
/// [`deactivate`]; the registered fn returns `None` while false.
static PATCH_READY: AtomicBool = AtomicBool::new(false);

/// Latched true when the patch fn actually produced output this session.
/// NOT cleared on disable: a template already patched in game memory STAYS
/// patched — task-03 must gate re-drives on `patch_applied() && mod
/// active`, not on this flag alone.
static PATCH_APPLIED: AtomicBool = AtomicBool::new(false);

static STAGED: Lazy<Mutex<Option<StagedPatch>>> = Lazy::new(|| Mutex::new(None));

static REGISTER_ONCE: Once = Once::new();

// One latched WARN per failure class (AC-3: "exactly one WARN names the
// reason").
static WARN_UNSTAGED: AtomicBool = AtomicBool::new(false);
static WARN_VARIANT: AtomicBool = AtomicBool::new(false);
static WARN_TRANSFORM: AtomicBool = AtomicBool::new(false);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

/// True when the patch is registered with assets staged (task-03 gating).
pub fn patch_ready() -> bool {
    PATCH_READY.load(Ordering::Acquire)
}

/// True when the patch fn produced a patched template this session
/// (task-03 gating; see the [`PATCH_APPLIED`] docs for disable semantics).
pub fn patch_applied() -> bool {
    PATCH_APPLIED.load(Ordering::Acquire)
}

/// Stage assets + register the patch. Called from the mod's `enable()`.
/// Staging runs once per boot (re-enable reuses the staged state — the
/// inputs cannot change within a session).
pub fn activate() {
    {
        let mut staged = match STAGED.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if staged.is_none() {
            *staged = assets::stage();
        }
        if staged.is_none() {
            // assets::stage already WARNed with the specific reason.
            PATCH_READY.store(false, Ordering::Release);
            return;
        }
    }

    REGISTER_ONCE.call_once(|| {
        afp_patcher::register_patch(TEMPLATE_NAME, Box::new(patch_dance_judge));
    });
    PATCH_READY.store(true, Ordering::Release);
}

/// Make the registered patch fn inert (mod disable). Templates already
/// loaded stay patched in game memory; subsequent loads stream stock.
pub fn deactivate() {
    PATCH_READY.store(false, Ordering::Release);
}

/// The afp_patcher callback for `dance_judge`. Runs on the game's loading
/// thread with the template ALREADY DESCRAMBLED; returns the patched
/// buffer + the empty 2-byte BSI (shipped convention), or `None` to stream
/// stock bytes.
fn patch_dance_judge(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !PATCH_READY.load(Ordering::Acquire) {
        return None; // disabled/unstaged — stock, no warn (normal state)
    }
    let guard = STAGED.lock().ok()?;
    let Some(staged) = guard.as_ref() else {
        warn_once(
            &WARN_UNSTAGED,
            "SMarvelous: dance_judge patch ready without staged assets — streaming stock",
        );
        return None;
    };

    // v1 skin gate: the seam only carries bytes (no IFS identity), so the
    // arriving template must be byte-identical to the default-skin stock
    // template staged at enable — anything else is an unknown variant whose
    // geo/texture names we did not inject for.
    if afp != staged.stock_bytes.as_slice() {
        warn_once(
            &WARN_VARIANT,
            "SMarvelous: dance_judge variant differs from the staged default-skin template (unknown skin?) — streaming stock",
        );
        return None;
    }

    // The real transform — the same host-tested recipe the enable-time dry
    // run executed on these exact bytes.
    let run = || -> Option<Vec<u8>> {
        let mut doc = Ap2Doc::parse(afp)?;
        let ids =
            doc.clone_word_segment_with_new_shape(SRC_LABEL, NEW_LABEL, staged.word_shape_id)?;
        // The staged geo/texture names were derived from the dry-run ids;
        // a mismatch would bind the new shape to a geo we never wrote.
        if ids.new_shape_id != staged.new_shape_id || ids.new_sprite_id != staged.new_sprite_id {
            return None;
        }
        doc.serialize()
    };
    match run() {
        Some(out) => {
            PATCH_APPLIED.store(true, Ordering::Release);
            log_info!(
                "SMarvelous: dance_judge patched ({} -> {} bytes, {} segment, shape {})",
                afp.len(),
                out.len(),
                NEW_LABEL,
                staged.new_shape_id
            );
            // Dev diagnostic (deploy #7 "resolved but blank" fork): dump
            // the EXACT bytes handed to the game so they can be pulled off
            // the cabinet and re-rendered offline (Leg-D style) — any
            // divergence between this dump and the harness's own patched
            // output localizes the bug to the live seam.
            if std::env::var_os("DDR_SMARV_DUMP").is_some() {
                let dump = "./data_mods/_cache/smarv_debug_dance_judge.bin";
                match std::fs::write(dump, &out) {
                    Ok(()) => log_info!("SMarvelous: DEV dumped patched template to {}", dump),
                    Err(e) => log_warn!("SMarvelous: DEV dump failed: {}", e),
                }
            }
            Some((out, vec![0u8; 2]))
        }
        None => {
            warn_once(
                &WARN_TRANSFORM,
                "SMarvelous: dance_judge transform failed at stream time — streaming stock",
            );
            None
        }
    }
}
