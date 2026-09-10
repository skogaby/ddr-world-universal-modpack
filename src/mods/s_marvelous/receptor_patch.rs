//! The `dance_effect` AFP patch — the receptor hit flash's size TIERS.
//!
//! Stock `dance_effect_v3` (the per-panel receptor flash clip, RE in
//! `docs/s_marvelous_judgement_research.md` "Violet receptor hit flash")
//! plays the greyscale additive `ef_bomb` at TWO sizes: `in_marvelous`
//! 0.80→1.15 over 8 frames, `in_perfect` 0.40→0.80 over 5. With
//! S-Marvelous the top tier moves up one, so the sizes move down one
//! (maintainer directive 2026-09-10):
//!
//! | label | stock | patched |
//! |---|---|---|
//! | `in_smarvelous` (NEW — clone of stock `in_marvelous`) | — | 0.80→1.15 |
//! | `in_marvelous` | 0.80→1.15 | 0.40→0.80 (what Perfect used to be) |
//! | `in_perfect` | 0.40→0.80 | 0.20→0.40 |
//!
//! The transform is `core/ap2`'s host-tested
//! [`Ap2Doc::clone_segment_and_rescale`] — placements-only clone of the
//! stock segment into every section carrying the label (root + exported
//! sprite 32 — the dual-timeline rule), then affine remaps of the named
//! `ef_bomb` instance's scale ramps. It is atomic and self-validating (label
//! set, record counts, ramp ranges — a different skin's template refuses
//! wholesale), so unlike `dance_judge` there is no staged-bytes identity
//! gate and nothing to stage: no art, no geo, no texture. The S-Marvelous
//! event then re-seeks the hit lanes' clips to `in_smarvelous`
//! (`receptor::on_judge_event`) exactly like the stock handler seeks to
//! `in_marvelous`; the violet comes from the layer CXFORM, not the art.
//!
//! Fail-open: a refused transform streams the stock template (stock sizes,
//! S-Marv shows the stock-size Marvelous flash in violet) with one WARN.
//! Lifecycle mirrors `afp_patches`: registered once, gated by `PATCH_READY`
//! (mod enable/disable), `PATCH_APPLIED` latched when a load was patched.
//!
//! PANIC SAFETY: the afp_patcher hook does NOT catch_unwind around patch
//! fns — everything here is Option-chained, no unwrap/index.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crate::core::ap2::{Ap2Doc, ScaleRemap, SegmentScaleEdit};
use crate::services::afp_patcher;
use crate::{log_info, log_warn};

/// The template the receptor flash clips are created from (exported name
/// == `afp_patcher`'s match key).
pub const TEMPLATE_NAME: &str = "dance_effect";
/// The stock top-tier segment and the synthesized S-Marvelous copy.
pub const SRC_LABEL: &str = "in_marvelous";
pub const NEW_LABEL: &str = "in_smarvelous";
/// The only instance the retier touches: the additive bomb sprite placed by
/// the two visible segments (the alpha-0 placeholder placement is unnamed).
const INSTANCE: &str = "ef_bomb";

/// Stock ramps (bemaniutils dump of the live template, 2026-09-10).
const STOCK_MARVELOUS: (f32, f32) = (0.80, 1.15);
const STOCK_PERFECT: (f32, f32) = (0.40, 0.80);
/// Patched ramps: Marvelous takes Perfect's old sizes; Perfect halves.
const NEW_MARVELOUS: (f32, f32) = STOCK_PERFECT;
const NEW_PERFECT: (f32, f32) = (0.20, 0.40);
/// Records per segment per section: the create + its update frames
/// (Marvelous: f0 + f1..f7; Perfect: f24 + f25..f28).
const MARVELOUS_RECORDS: usize = 8;
const PERFECT_RECORDS: usize = 5;

/// The two scale edits, in recipe order (the clone of `SRC_LABEL` runs
/// FIRST inside the recipe, so the copy keeps the stock ramp).
pub fn edits() -> [SegmentScaleEdit<'static>; 2] {
    [
        SegmentScaleEdit {
            label: "in_marvelous",
            instance: INSTANCE,
            remap: ScaleRemap::from_f32(STOCK_MARVELOUS, NEW_MARVELOUS),
            expected_records: MARVELOUS_RECORDS,
        },
        SegmentScaleEdit {
            label: "in_perfect",
            instance: INSTANCE,
            remap: ScaleRemap::from_f32(STOCK_PERFECT, NEW_PERFECT),
            expected_records: PERFECT_RECORDS,
        },
    ]
}

/// Pure: the whole transform on descrambled template bytes. `None` = the
/// template is not the shape this retier knows (stock streams).
pub fn transform(afp: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut doc = Ap2Doc::parse(afp)?;
    let n = doc.clone_segment_and_rescale((SRC_LABEL, NEW_LABEL), &edits())?;
    Some((doc.serialize()?, n))
}

static PATCH_READY: AtomicBool = AtomicBool::new(false);
/// Latched when a `dance_effect` load was patched this session. NOT cleared
/// on disable — a template already patched in game memory stays patched;
/// the re-seek gates on `patch_applied() && mod active`.
static PATCH_APPLIED: AtomicBool = AtomicBool::new(false);
static REGISTER_ONCE: Once = Once::new();
static WARN_TRANSFORM: AtomicBool = AtomicBool::new(false);

pub fn patch_applied() -> bool {
    PATCH_APPLIED.load(Ordering::Acquire)
}

/// Register the patch (once) and arm it. Mod enable.
pub fn activate() {
    REGISTER_ONCE.call_once(|| {
        afp_patcher::register_patch(TEMPLATE_NAME, Box::new(patch_dance_effect));
    });
    PATCH_READY.store(true, Ordering::Release);
}

/// Make the patch fn inert (mod disable): subsequent loads stream stock.
pub fn deactivate() {
    PATCH_READY.store(false, Ordering::Release);
}

fn patch_dance_effect(afp: &[u8], _bsi: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !PATCH_READY.load(Ordering::Acquire) {
        return None;
    }
    match transform(afp) {
        Some((out, n)) => {
            PATCH_APPLIED.store(true, Ordering::Release);
            log_info!(
                "SMarvelous: dance_effect patched ({} -> {} bytes, {} segment cloned, {} bomb records rescaled: Marvelous {:.2}->{:.2}, Perfect {:.2}->{:.2})",
                afp.len(),
                out.len(),
                NEW_LABEL,
                n,
                NEW_MARVELOUS.0,
                NEW_MARVELOUS.1,
                NEW_PERFECT.0,
                NEW_PERFECT.1
            );
            Some((out, vec![0u8; 2]))
        }
        None => {
            if !WARN_TRANSFORM.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "SMarvelous: dance_effect template is not the known shape (labels/ramps/record counts) -- streaming stock; receptor flash sizes stay stock"
                );
            }
            None
        }
    }
}
