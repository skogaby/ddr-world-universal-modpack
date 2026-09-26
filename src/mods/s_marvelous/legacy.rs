//! DDR SELECTION bridge: S-Marvelous on the legacy skins (design §4.9 P6;
//! research `.agents/planning/2026-09-22-ddr-selection/research/smarv-legacy.md`).
//!
//! DDR SELECTION swaps World's judge / full-combo / combo packages for A3's
//! per-era `…000N` packages. Each legacy skin whose art exists under
//! `data_mods/ddr_selection/s_marvelous/N/` gets the same three S-Marvelous
//! surfaces World has, staged here once per boot:
//!
//! - the judgement word — [`super::afp_patches::add_legacy`] (the World word
//!   recipe on `dance_judge000N`; A3's MARVELOUS keeps its additive pulse,
//!   the violet copy is static);
//! - the S-MFC splash — [`super::splash::add_legacy`] (World's multi-shape
//!   recipe on the four `dance_fullcombo000N` templates, five Marvelous
//!   regions each);
//! - skins 4–5 only: the all-S-Marvelous combo sheet —
//!   [`super::combo::add_legacy`], consumed by DDR SELECTION's A3 texture
//!   write through [`super::legacy_combo_smarv`].
//!
//! A skin without art (or whose staging fails) keeps A3's presentation —
//! the patch fns find no entry for it and stream the legacy template as is.
//! FAST/SLOW, the receptor burst and the results / lamps are
//! package-independent and need nothing here.
//!
//! Staging runs when BOTH mods are enabled: S-Marvelous enables first at
//! boot, so DDR SELECTION's `enable` calls [`super::on_ddr_selection_enabled`];
//! S-Marvelous' own `enable` covers the reverse (live) order. Synchronous on
//! the enabling thread, like World's staging (cache-guarded after the first
//! boot).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::log_info;

use super::{afp_patches, assets, combo, splash, targets};

/// Skins whose staging was attempted this boot (one attempt per skin; the
/// staged entries survive a disable / re-enable).
static ATTEMPTED: AtomicU8 = AtomicU8::new(0);

/// Stage every legacy skin with art, once per boot. No-op unless both
/// S-Marvelous and DDR SELECTION are enabled.
pub fn stage_if_ready(color: assets::JudgementColor) {
    if !super::is_enabled() || !crate::mods::ddr_selection::is_enabled() {
        return;
    }
    let started = std::time::Instant::now();
    let mut word = Vec::new();
    let mut splash_skins = Vec::new();
    let mut combo_skins = Vec::new();
    let mut attempted_any = false;
    for skin in targets::LEGACY_SKINS {
        let bit = targets::skin_bit(skin);
        if ATTEMPTED.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
            continue;
        }
        attempted_any = true;
        if !std::path::Path::new(&targets::legacy_art_dir(skin)).is_dir() {
            log_info!(
                "SMarvelous: no S-Marvelous art for DDR SELECTION skin {} ({}) -- A3's presentation",
                skin,
                targets::legacy_art_dir(skin)
            );
            continue;
        }
        if afp_patches::add_legacy(skin, color) {
            word.push(skin);
        }
        if splash::add_legacy(skin) {
            splash_skins.push(skin);
        }
        if targets::legacy_combo_has_grade_sheets(skin) && combo::add_legacy(skin) {
            combo_skins.push(skin);
        }
    }
    if attempted_any {
        log_info!(
            "SMarvelous: DDR SELECTION legacy art staged in {} ms (word {:?}, S-MFC splash {:?}, combo sheet {:?})",
            started.elapsed().as_millis(),
            word,
            splash_skins,
            combo_skins
        );
    }
}
