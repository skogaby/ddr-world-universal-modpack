//! The `dance_judge` AFP patch: registration with the shared
//! `afp_patcher` seam, the patch fn itself, and the confidence flags
//! task-03's flash re-drive gates on.
//!
//! The doc-transform core is `core/ap2`'s host-tested
//! [`Ap2Doc::clone_word_segment_with_new_shape_ex`] recipe (run through
//! [`super::assets::run_word_clone`] with the additive-glow mutes — the
//! S-Marv copy's always, the stock Marvelous word's on World only) — this
//! module is the thin impure wiring around it: staged-state storage, the
//! per-target byte gate, latched WARNs, and the ready/applied atomics.
//!
//! Targets: World's `dance_judge_v3` (skin 0, staged by [`activate`]) and,
//! while DDR SELECTION is enabled, each legacy skin with art
//! ([`add_legacy`], called by [`super::legacy`]). Every legacy
//! `dance_judge000N` exports the same `dance_judge` name, so the patch fn
//! picks the target by the SONG (`ddr_selection::legacy_package` + the armed
//! skin — legacy templates of different skins can be byte-identical, and a
//! patch whose new shape has no geo in the streaming IFS would draw
//! nothing), then byte-gates on that target's staged stock bytes.
//!
//! Lifecycle: [`activate`] (mod enable) stages World's assets via
//! [`super::assets::stage`] and registers the patch fn ONCE (afp_patcher
//! has no unregister — the fn body checks [`ENABLED`], so [`deactivate`]
//! making it return `None` restores stock streaming for subsequent template
//! loads). Fail-open everywhere: any refusal streams stock bytes with one
//! latched WARN naming the reason (AC-3); a legacy song whose skin has no
//! staged art streams A3's template silently (the staging logged why).
//!
//! PANIC SAFETY: the afp_patcher hook does NOT catch_unwind around patch
//! fns — everything in [`patch_dance_judge`] is Option-chained, no
//! unwrap/index.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, Once};

use once_cell::sync::Lazy;

use crate::core::ap2::Ap2Doc;
use crate::services::afp_patcher;
use crate::{log_info, log_warn};

use super::assets::{self, StagedPatch, NEW_LABEL, TEMPLATE_NAME};
use super::targets;

/// Mod enabled with the patch fn registered. Cleared by [`deactivate`]; the
/// registered fn returns `None` while false.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Bit per target skin ([`targets::skin_bit`]): latched when the patch fn
/// actually produced output for that target this session. NOT cleared on
/// disable: a template already patched in game memory STAYS patched —
/// task-03 must gate re-drives on `patch_applied*() && mod active`, not on
/// this flag alone.
static APPLIED: AtomicU8 = AtomicU8::new(0);

/// One staged patch per target (World's first when it staged).
static STAGED: Lazy<Mutex<Vec<StagedPatch>>> = Lazy::new(|| Mutex::new(Vec::new()));

static REGISTER_ONCE: Once = Once::new();

// One latched WARN per failure class (AC-3: "exactly one WARN names the
// reason").
static WARN_VARIANT: AtomicBool = AtomicBool::new(false);
static WARN_TRANSFORM: AtomicBool = AtomicBool::new(false);
/// Per legacy skin: its template differed from the staged one.
static WARN_LEGACY_VARIANT: AtomicU8 = AtomicU8::new(0);

fn warn_once(latch: &AtomicBool, msg: &str) {
    if !latch.swap(true, Ordering::Relaxed) {
        log_warn!("{}", msg);
    }
}

/// True when the patch is registered with at least one target staged.
pub fn patch_ready() -> bool {
    ENABLED.load(Ordering::Acquire) && STAGED.lock().map(|g| !g.is_empty()).unwrap_or(false)
}

/// True when the patch fn produced a patched World template this session
/// (task-03 gating; see the [`APPLIED`] docs for disable semantics).
pub fn patch_applied() -> bool {
    patch_applied_for(0)
}

/// [`patch_applied`] for any target: 0 = World, 1..=5 = a legacy skin.
pub fn patch_applied_for(skin: u8) -> bool {
    APPLIED.load(Ordering::Acquire) & targets::skin_bit(skin) != 0
}

/// Stage World's assets (with the `color` word art) + register the patch.
/// Called from the mod's `enable()`. Staging runs once per boot (re-enable
/// reuses the staged state — the template inputs cannot change within a
/// session); a re-enable restages every target's word art in `color`.
pub fn activate(color: assets::JudgementColor) {
    let world_staged = STAGED
        .lock()
        .map(|g| g.iter().any(|p| p.skin == 0))
        .unwrap_or(false);
    if world_staged {
        set_judgement_color(color);
    } else if let Some(p) = assets::stage(color) {
        // (Staged outside the lock — the loading thread's patch fn must
        // never wait on enable-time IO.)
        if let Ok(mut g) = STAGED.lock() {
            g.insert(0, p);
        }
    }
    // assets::stage already WARNed with the specific reason on failure;
    // World's template then streams stock (no entry for skin 0).

    REGISTER_ONCE.call_once(|| {
        afp_patcher::register_patch(TEMPLATE_NAME, Box::new(patch_dance_judge));
    });
    ENABLED.store(true, Ordering::Release);
}

/// Stage a DDR SELECTION legacy skin's word (its `dance_judge000N`
/// package) with the `color` art. Idempotent per skin (a second call only
/// restages the art). `false` when the mod is not active or staging failed
/// (WARNed) — that skin's songs keep A3's MARVELOUS only.
pub fn add_legacy(skin: u8, color: assets::JudgementColor) -> bool {
    if !ENABLED.load(Ordering::Acquire) {
        return false;
    }
    let existing = STAGED
        .lock()
        .map(|g| g.iter().any(|p| p.skin == skin))
        .unwrap_or(false);
    if existing {
        return true;
    }
    let Some(target) = assets::legacy_target(TEMPLATE_NAME, skin) else {
        log_warn!(
            "SMarvelous: no dance_judge{:04} package on this install -- skin {} keeps A3's word",
            skin,
            skin
        );
        return false;
    };
    let Some(p) = assets::stage_word(&target, color) else {
        return false;
    };
    match STAGED.lock() {
        Ok(mut g) => {
            g.push(p);
            true
        }
        Err(_) => false,
    }
}

/// Live "Judgement Color" apply: swap every staged target's word art to
/// `color`. No-op (false) when nothing was staged this session — there is
/// no staged image to swap and nothing renders the word anyway.
pub fn set_judgement_color(color: assets::JudgementColor) -> bool {
    // Copy the names out first: the file IO below must not hold the lock the
    // loading thread's patch fn takes.
    let targets: Vec<(u8, String, String)> = match STAGED.lock() {
        Ok(g) => g
            .iter()
            .map(|p| (p.skin, p.ifs_mod_path.clone(), p.new_region.clone()))
            .collect(),
        Err(_) => return false,
    };
    let mut any = false;
    for (skin, ifs_mod_path, new_region) in &targets {
        any |= assets::restage_word_art(color, *skin, ifs_mod_path, new_region);
    }
    any
}

/// Make the registered patch fn inert (mod disable). Templates already
/// loaded stay patched in game memory; subsequent loads stream stock.
pub fn deactivate() {
    ENABLED.store(false, Ordering::Release);
}

/// The afp_patcher callback for `dance_judge`. Runs on the game's loading
/// thread with the template ALREADY DESCRAMBLED; returns the patched
/// buffer + the empty 2-byte BSI (shipped convention), or `None` to stream
/// stock bytes.
fn patch_dance_judge(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !ENABLED.load(Ordering::Acquire) {
        return None; // disabled — stock, no warn (normal state)
    }
    // Which package is streaming: World's, or this song's DDR SELECTION
    // legacy skin (same export name).
    let skin = targets::target_skin(
        crate::mods::ddr_selection::legacy_package(TEMPLATE_NAME),
        crate::mods::ddr_selection::armed_skin(),
    )?;
    let guard = STAGED.lock().ok()?;
    // No entry: World's staging WARNed at enable, or this legacy skin has
    // no art (logged at staging) — stream stock.
    let staged = guard.iter().find(|p| p.skin == skin)?;

    // Skin gate: the seam only carries bytes (no IFS identity), so the
    // arriving template must be byte-identical to the stock template staged
    // for this target — anything else is an unknown variant whose
    // geo/texture names we did not inject for.
    if afp != staged.stock_bytes.as_slice() {
        if skin == 0 {
            warn_once(
                &WARN_VARIANT,
                "SMarvelous: dance_judge variant differs from the staged default-skin template (unknown skin?) — streaming stock",
            );
        } else if WARN_LEGACY_VARIANT.fetch_or(targets::skin_bit(skin), Ordering::Relaxed)
            & targets::skin_bit(skin)
            == 0
        {
            log_warn!(
                "SMarvelous: dance_judge{:04} (skin {}) differs from the template staged at enable — A3's word shows",
                skin,
                skin
            );
        }
        return None;
    }

    // The real transform — the same host-tested recipe the enable-time dry
    // run executed on these exact bytes (the recipe has no live inputs, so
    // the allocated ids are those of the dry run by construction).
    let run = || -> Option<(Vec<u8>, usize, usize)> {
        let mut doc = Ap2Doc::parse(afp)?;
        let ids = assets::run_word_clone(&mut doc, staged.word_shape_id, staged.mute_stock)?;
        // The staged geo/texture names were derived from the dry-run ids;
        // a mismatch would bind the new shape to a geo we never wrote.
        if ids.new_shape_id != staged.new_shape_id || ids.new_sprite_id != staged.new_sprite_id {
            return None;
        }
        Some((
            doc.serialize()?,
            ids.muted_records,
            ids.muted_source_records,
        ))
    };
    match run() {
        Some((out, muted, muted_stock)) => {
            APPLIED.fetch_or(targets::skin_bit(skin), Ordering::AcqRel);
            if skin == 0 {
                log_info!(
                    "SMarvelous: dance_judge patched ({} -> {} bytes, {} segment, shape {}, additive glow records muted: {} (S-Marv) / {} (stock Marvelous)",
                    afp.len(),
                    out.len(),
                    NEW_LABEL,
                    staged.new_shape_id,
                    muted,
                    muted_stock
                );
            } else {
                log_info!(
                    "SMarvelous: dance_judge{:04} (skin {}) patched ({} -> {} bytes, {} segment, shape {}, additive glow records muted: {} (S-Marv) / {} (A3's MARVELOUS keeps its pulse))",
                    skin,
                    skin,
                    afp.len(),
                    out.len(),
                    NEW_LABEL,
                    staged.new_shape_id,
                    muted,
                    muted_stock
                );
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
