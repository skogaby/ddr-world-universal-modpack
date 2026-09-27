//! S-Marvelous art targets — the package names, art paths and rename rules
//! the staging in [`super::assets`] works from (pure, host-tested).
//!
//! Two kinds of target carry S-Marvelous art:
//!
//! * **World** (skin 0): World's own `_v3` packages, art under
//!   `data_mods/s_marvelous/`.
//! * **DDR SELECTION's legacy skins** 1..=8: the packages the DDR SELECTION
//!   package helper registers — an era's (1..=5) `dance_judge000N` /
//!   `dance_fullcombo000N` / `dance_combo000N`, a theme's (6 DDR A, 7 A3
//!   White, 8 A3 Gold) A3 skin-0 `…0000_v0` / `_v2` / `_v1` (named by
//!   `ddr_selection::policy`, see `assets::legacy_target`). Art lives under
//!   `data_mods/ddr_selection/s_marvelous/<art set>/` (same layout as
//!   World's; [`art_set`]: White and Gold share set 7). Research:
//!   `.agents/planning/2026-09-22-ddr-selection/research/smarv-legacy.md`.
//!
//! Dependency-free on purpose: `scripts/validate_s_marvelous.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.

/// The legacy skins S-Marvelous can dress: DDR SELECTION's eras (1st-5th …
/// 2013-A) and themes (DDR A, A3 White, A3 Gold).
pub const LEGACY_SKINS: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// A DDR SELECTION theme skin (A3's own skin-0 UI generations).
pub fn is_theme(skin: u8) -> bool {
    (6..=8).contains(&skin)
}

/// The `%04d` inside a skin's texture names: the era's own number, 0 for a
/// theme (A3's skin-0 art: `dance_combo0000_*`). Mirrors
/// `ddr_selection::policy::tex_number`; `combo::add_legacy` checks the
/// combo sheet name against DDR SELECTION's before staging.
pub fn tex_number(skin: u8) -> u8 {
    if is_theme(skin) {
        0
    } else {
        skin
    }
}

/// The art folder a skin reads: its own, except A3 White (7) and A3 Gold
/// (8), whose `_v2` / `_v1` Marvelous art is pixel-identical and shares
/// set 7.
pub fn art_set(skin: u8) -> u8 {
    if skin == 8 {
        7
    } else {
        skin
    }
}

/// Root of the legacy skins' S-Marvelous art (one sub-folder per skin).
pub const LEGACY_ART_ROOT: &str = "./data_mods/ddr_selection/s_marvelous";

/// Marvelous-art shapes the S-MFC splash clone expects per template: World's
/// `dance_fullcombo_v3` has four (text, light, rocket, side light), every
/// legacy skin five (text, light, ring, rsring01, side light — the eras'
/// `…000N` and the themes' `…0000_vN` alike). Any other count is an unknown
/// template and refuses.
pub fn fc_expected_shapes(skin: u8) -> usize {
    if skin == 0 {
        4
    } else {
        5
    }
}

/// Whether the skin's combo has one sheet per worst grade — A3's rule
/// (skins 1–3 draw one sheet whatever the grade; the themes are A3's skin
/// 0, which has per-grade sheets). Only these skins get an S-Marvelous combo
/// sheet.
pub fn legacy_combo_has_grade_sheets(skin: u8) -> bool {
    matches!(skin, 4..=8)
}

/// The eleven combo textures of a sheet: digits then the "combo" word.
pub const COMBO_KEYS: [&str; 11] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "combo"];

/// An era's package base (`dance_judge` + 1 → `dance_judge0001`) — the
/// name DDR SELECTION's package helper registers for skins 1..=5 (a theme's
/// full `_vN` name comes from DDR SELECTION's policy).
pub fn legacy_base(kind: &str, skin: u8) -> String {
    format!("{}{:04}", kind, skin)
}

/// The arc files World's probe tries for a package base, in order.
pub fn arc_candidates(base: &str) -> [String; 3] {
    [
        format!("{}_v3.arc", base),
        format!("{}_v0.arc", base),
        format!("{}.arc", base),
    ]
}

/// The IFS inside an arc (`dance_judge0001_v0.arc` → `dance_judge0001_v0.ifs`)
/// — also the normalized IFS path the game's opens resolve to.
pub fn ifs_name(arc_file: &str) -> String {
    format!("{}.ifs", arc_file.strip_suffix(".arc").unwrap_or(arc_file))
}

/// The LayeredFS mod path of an IFS (`.ifs` → `_ifs`).
pub fn ifs_mod_path(ifs: &str) -> String {
    format!("{}_ifs", ifs.strip_suffix(".ifs").unwrap_or(ifs))
}

/// A legacy skin's art folder (its [`art_set`]).
pub fn legacy_art_dir(skin: u8) -> String {
    format!("{}/{}", LEGACY_ART_ROOT, art_set(skin))
}

/// A legacy skin's judgement word for one "Judgement Color" key
/// (`all_purple` / `purple_shadow`).
pub fn legacy_word_png(skin: u8, color_key: &str) -> String {
    format!(
        "{}/dance_judge/smarvelous_{}.png",
        legacy_art_dir(skin),
        color_key
    )
}

/// A legacy skin's S-MFC splash art for a renamed region.
pub fn legacy_fc_png(skin: u8, new_region: &str) -> String {
    format!(
        "{}/dance_fullcombo/{}.png",
        legacy_art_dir(skin),
        new_region
    )
}

/// A legacy skin's S-Marvelous combo art for one "Judgement Color" key
/// (`all_purple` / `purple_shadow` — the word's choice also picks the
/// sheet) and one [`COMBO_KEYS`] entry.
pub fn legacy_combo_png(skin: u8, color_key: &str, key: &str) -> String {
    format!(
        "{}/dance_combo/smarvelous_{}_{}.png",
        legacy_art_dir(skin),
        color_key,
        key
    )
}

/// The texture a legacy S-Marvelous combo sheet entry is staged under —
/// A3's `dance_combo%04d_<grade>_<key>` family with grade `smarvelous`
/// (DDR SELECTION's `combo_math::sheet_prefix` names the same prefix; a
/// theme's `%04d` is 0).
pub fn legacy_combo_texture(skin: u8, key: &str) -> String {
    format!("dance_combo{:04}_smarvelous_{}", tex_number(skin), key)
}

/// The stock Marvelous texture a legacy S-Marvelous combo entry stands in for
/// (the FRESH set's format / compression reference).
pub fn legacy_combo_donor(skin: u8) -> String {
    format!("dance_combo{:04}_marvelous_0", tex_number(skin))
}

/// The S-MFC splash region rename: prefix `s` onto the last `_` token iff it
/// starts with `mar` (`dafu_eff_mar` → `dafu_eff_smar`,
/// `dafu_ring_marvelous` → `dafu_ring_smarvelous`). Shared by World's splash,
/// the results emblem and every legacy skin.
pub fn fc_region_rename(region: &str) -> Option<String> {
    let (head, tail) = region.rsplit_once('_')?;
    if !tail.starts_with("mar") {
        return None;
    }
    Some(format!("{}_s{}", head, tail))
}

/// The judgement word's new region: the donor region with its `marvelous`
/// suffix replaced (`daju_marvelous` → `daju_smarvelous`,
/// `dance_judge0001_marvelous` → `dance_judge0001_smarvelous`).
pub fn word_region(donor_region: &str) -> Option<String> {
    let stem = donor_region.strip_suffix("marvelous")?;
    Some(format!("{}smarvelous", stem))
}

/// Which staged target a streaming template belongs to: World's (0) when
/// the package is not legacy for this song, else the armed skin. `None`:
/// legacy with no valid skin (never patch).
pub fn target_skin(legacy: bool, armed_skin: u8) -> Option<u8> {
    if !legacy {
        Some(0)
    } else if LEGACY_SKINS.contains(&armed_skin) {
        Some(armed_skin)
    } else {
        None
    }
}

/// Bit for a target skin in the per-target "patched this session" masks
/// (0 = World, 1..=8 the legacy skins).
pub fn skin_bit(skin: u8) -> u16 {
    if skin <= 15 {
        1 << skin
    } else {
        0
    }
}

/// Whether the word clone mutes the STOCK Marvelous word's additive pulse
/// too: World's pulse is muted (maintainer, 2026-09-13); a legacy skin's
/// MARVELOUS keeps A3's (maintainer, 2026-09-25). The S-Marvelous copy is
/// always muted.
pub fn mute_stock_glow(skin: u8) -> bool {
    skin == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_names() {
        assert_eq!(legacy_base("dance_judge", 1), "dance_judge0001");
        assert_eq!(legacy_base("dance_fullcombo", 5), "dance_fullcombo0005");
        assert_eq!(
            arc_candidates("dance_judge0003"),
            [
                "dance_judge0003_v3.arc".to_string(),
                "dance_judge0003_v0.arc".to_string(),
                "dance_judge0003.arc".to_string()
            ]
        );
        assert_eq!(ifs_name("dance_judge0001_v0.arc"), "dance_judge0001_v0.ifs");
        assert_eq!(
            ifs_mod_path("dance_judge0001_v0.ifs"),
            "dance_judge0001_v0_ifs"
        );
        assert_eq!(ifs_mod_path("dance_judge_v3.ifs"), "dance_judge_v3_ifs");
    }

    #[test]
    fn art_paths() {
        assert_eq!(
            legacy_word_png(2, "purple_shadow"),
            "./data_mods/ddr_selection/s_marvelous/2/dance_judge/smarvelous_purple_shadow.png"
        );
        assert_eq!(
            legacy_fc_png(1, "dafu_eff_smar"),
            "./data_mods/ddr_selection/s_marvelous/1/dance_fullcombo/dafu_eff_smar.png"
        );
        // The combo sheet follows the Judgement Color, like the word.
        assert_eq!(
            legacy_combo_png(4, "all_purple", "combo"),
            "./data_mods/ddr_selection/s_marvelous/4/dance_combo/smarvelous_all_purple_combo.png"
        );
        assert_eq!(
            legacy_combo_png(5, "purple_shadow", "3"),
            "./data_mods/ddr_selection/s_marvelous/5/dance_combo/smarvelous_purple_shadow_3.png"
        );
    }

    #[test]
    fn combo_sheet() {
        assert!(!legacy_combo_has_grade_sheets(1));
        assert!(!legacy_combo_has_grade_sheets(3));
        assert!(legacy_combo_has_grade_sheets(4));
        assert!(legacy_combo_has_grade_sheets(5));
        assert_eq!(legacy_combo_texture(4, "3"), "dance_combo0004_smarvelous_3");
        assert_eq!(
            legacy_combo_texture(5, "combo"),
            "dance_combo0005_smarvelous_combo"
        );
        assert_eq!(legacy_combo_donor(5), "dance_combo0005_marvelous_0");
        assert_eq!(COMBO_KEYS.len(), 11);
    }

    #[test]
    fn renames() {
        assert_eq!(
            fc_region_rename("dafu_eff_mar").as_deref(),
            Some("dafu_eff_smar")
        );
        assert_eq!(
            fc_region_rename("dafu_rsring01_marvelous").as_deref(),
            Some("dafu_rsring01_smarvelous")
        );
        assert_eq!(
            fc_region_rename("scre_fc_marvelous").as_deref(),
            Some("scre_fc_smarvelous")
        );
        assert_eq!(fc_region_rename("dafu_ring_Good"), None);
        assert_eq!(fc_region_rename("dafu_eff_perfect"), None);
        assert_eq!(fc_region_rename("marvelous"), None);
        assert_eq!(
            word_region("daju_marvelous").as_deref(),
            Some("daju_smarvelous")
        );
        assert_eq!(
            word_region("dance_judge0004_marvelous").as_deref(),
            Some("dance_judge0004_smarvelous")
        );
        assert_eq!(word_region("dance_judge0004_perfect"), None);
    }

    #[test]
    fn themes() {
        // DDR SELECTION's themes: 6 DDR A, 7 A3 (White), 8 A3 (Gold).
        for skin in 6..=8u8 {
            assert!(LEGACY_SKINS.contains(&skin));
            assert_eq!(target_skin(true, skin), Some(skin));
            assert_eq!(tex_number(skin), 0);
            assert!(legacy_combo_has_grade_sheets(skin));
            // Counted on every theme `dance_fullcombo0000_vN` template.
            assert_eq!(fc_expected_shapes(skin), 5);
            assert_eq!(
                legacy_combo_texture(skin, "7"),
                "dance_combo0000_smarvelous_7"
            );
            assert_eq!(legacy_combo_donor(skin), "dance_combo0000_marvelous_0");
            assert!(!mute_stock_glow(skin));
        }
        // White and Gold share one art set (A3's `_v1` ≡ `_v2` art).
        assert_eq!(art_set(6), 6);
        assert_eq!(art_set(7), 7);
        assert_eq!(art_set(8), 7);
        assert_eq!(
            legacy_word_png(8, "all_purple"),
            "./data_mods/ddr_selection/s_marvelous/7/dance_judge/smarvelous_all_purple.png"
        );
        assert_eq!(
            legacy_fc_png(6, "dafu_eff_smar"),
            "./data_mods/ddr_selection/s_marvelous/6/dance_fullcombo/dafu_eff_smar.png"
        );
        assert_eq!(
            legacy_combo_png(8, "purple_shadow", "combo"),
            "./data_mods/ddr_selection/s_marvelous/7/dance_combo/smarvelous_purple_shadow_combo.png"
        );
        assert_eq!(legacy_art_dir(7), legacy_art_dir(8));
        assert_ne!(legacy_art_dir(6), legacy_art_dir(7));
        assert_eq!(target_skin(true, 9), None);
    }

    #[test]
    fn eras_keep_their_own_numbers() {
        for skin in 1..=5u8 {
            assert_eq!(art_set(skin), skin);
            assert_eq!(tex_number(skin), skin);
        }
        assert_eq!(legacy_combo_texture(5, "0"), "dance_combo0005_smarvelous_0");
    }

    #[test]
    fn skin_bits_cover_every_target() {
        let mut seen = 0u16;
        for skin in 0..=8u8 {
            let bit = skin_bit(skin);
            assert_eq!(bit.count_ones(), 1, "skin {skin}");
            assert_eq!(seen & bit, 0, "skin {skin} bit reused");
            seen |= bit;
        }
        assert_eq!(skin_bit(8), 0x100);
        assert_eq!(skin_bit(16), 0);
    }

    #[test]
    fn target_selection() {
        assert_eq!(target_skin(false, 0), Some(0));
        assert_eq!(target_skin(false, 3), Some(0));
        assert_eq!(target_skin(true, 3), Some(3));
        assert_eq!(target_skin(true, 0), None);
        assert_eq!(target_skin(true, 6), Some(6));
        assert_eq!(skin_bit(0), 1);
        assert_eq!(skin_bit(5), 0x20);
        assert_eq!(fc_expected_shapes(0), 4);
        assert_eq!(fc_expected_shapes(2), 5);
        assert!(mute_stock_glow(0));
        assert!(!mute_stock_glow(1));
    }
}
