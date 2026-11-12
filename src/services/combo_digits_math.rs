//! Five-digit combo display — the pure layer (`services::combo_digits`).
//!
//! World's combo counter shows at most 9,999: the digit refresh clamps the
//! value and the digit count, and the `number` sprite of the three
//! `dance_combo_root%d` clips only has four digit instances (`1_usr` …
//! `1000_usr`) and four layouts (`loop_1` … `loop_1000`). The counter itself
//! is not capped. This file holds what the fifth digit needs that is not
//! engine-facing: the clip-patch recipe's constants, the digit math and the
//! texture names — std-only, so `scripts/validate_combo_digits.sh` mounts it
//! and runs the recipe on the real templates.
//!
//! Layout. The stock rows step `(-66, +12)` px per place and sit centred on
//! `(190, 83)`: three digits are `(256,71) (190,83) (124,95)`, four are
//! `(289,67) … (91,103)`. Five digits take the three-digit row and extend
//! it one place each way — `(322,59) (256,71) (190,83) (124,95) (58,107)` —
//! which is the four-digit row moved by [`ROW_SHIFT`] plus the new place at
//! [`NEW_TRANSLATE`]. The new instance sits at depth 1, under the thousands
//! (depth 2): a higher place is always drawn behind the place to its right.
//!
//! Five digits is every combo a chart can reach — an SSQ chart holds at most
//! 65,535 steps, and a freeze end does not add to the combo.

/// The largest count the stock refresh shows.
pub const STOCK_MAX_SHOWN: i32 = 9_999;
/// The largest count five digits show.
pub const MAX_SHOWN: i32 = 99_999;

/// The three clips of World's combo (`ComboActor` roots 1..3), each with its
/// own copy of the `number` sprite.
pub const TEMPLATES: [&str; 3] = [
    "dance_combo_root1",
    "dance_combo_root2",
    "dance_combo_root3",
];

/// The four-digit layout the recipe clones, and the label of the clone.
pub const SRC_LABEL: &str = "loop_1000";
pub const NEW_LABEL: &str = "loop_10000";
/// The instance the new digit is a copy of, and the new digit's name — the
/// stock `%d_usr` naming, so the place value alone addresses it.
pub const DONOR_INSTANCE: &str = "1000_usr";
pub const NEW_INSTANCE: &str = "10000_usr";
/// Depth of the new digit (the stock rows use 2..=17).
pub const NEW_DEPTH: u16 = 1;
/// Shift of the cloned four-digit row, raw fixed-point (/20): +33 px, −8 px.
pub const ROW_SHIFT: (i32, i32) = (33 * 20, -8 * 20);
/// Position of the new digit, raw fixed-point (/20): (58, 107) px.
pub const NEW_TRANSLATE: (i32, i32) = (58 * 20, 107 * 20);

/// The `number` sprite's instance path under a root layer — where the
/// layout label lives.
pub const NUMBER_PATH: &str = "combo_usr/number_usr";

/// Digit places, ones first.
pub const PLACES: [u32; 5] = [1, 10, 100, 1000, 10_000];

/// Whether `combo` needs the fifth digit.
pub fn needs_five(combo: i32) -> bool {
    combo > STOCK_MAX_SHOWN
}

/// The count the digits show.
pub fn shown(combo: i32) -> u32 {
    combo.clamp(0, MAX_SHOWN) as u32
}

/// The digit of `shown` at `place` (a power of ten).
pub fn digit(shown: u32, place: u32) -> u32 {
    if place == 0 {
        return 0;
    }
    (shown / place) % 10
}

/// Path of a place's digit instances under a root layer (the stock
/// `"combo_usr/number_usr/%d_usr"`).
pub fn instance_path(place: u32) -> String {
    format!("{NUMBER_PATH}/{place}_usr")
}

/// The digit texture the stock refresh loads for `place` on root `layer`
/// (0 = the digit art, 1 / 2 = the tinted underlays) at worst grade `worst`
/// (0 Marvelous … 3 Good) — `"daco_combo%s_%d"` with the stock suffix rule:
/// the underlays always use `_dummy`; the digit art uses the grade suffix,
/// except its ones place, which is unsuffixed. `None` for a layer or grade
/// outside the stock tables.
pub fn texture_name(layer: usize, place: u32, worst: i32, digit: u32) -> Option<String> {
    const GRADES: [&str; 4] = ["_marvelous", "_perfect", "_great", "_good"];
    let suffix = match layer {
        0 if place == 1 => "",
        0 => GRADES.get(usize::try_from(worst).ok()?)?,
        1 | 2 => "_dummy",
        _ => return None,
    };
    Some(format!("daco_combo{suffix}_{}", digit % 10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_digits_start_past_the_stock_cap() {
        assert!(!needs_five(0));
        assert!(!needs_five(9_999));
        assert!(needs_five(10_000));
        assert!(needs_five(65_535));
    }

    #[test]
    fn shown_clamps_to_five_digits() {
        assert_eq!(shown(-1), 0);
        assert_eq!(shown(12_345), 12_345);
        assert_eq!(shown(99_999), 99_999);
        assert_eq!(shown(100_000), 99_999);
        assert_eq!(shown(i32::MAX), 99_999);
    }

    #[test]
    fn digits_by_place() {
        let got: Vec<u32> = PLACES.iter().map(|&p| digit(65_535, p)).collect();
        assert_eq!(got, [5, 3, 5, 5, 6]);
        let got: Vec<u32> = PLACES.iter().map(|&p| digit(10_000, p)).collect();
        assert_eq!(got, [0, 0, 0, 0, 1]);
        assert_eq!(digit(123, 0), 0);
    }

    #[test]
    fn instance_paths_follow_the_stock_format() {
        assert_eq!(instance_path(1), "combo_usr/number_usr/1_usr");
        assert_eq!(instance_path(10_000), "combo_usr/number_usr/10000_usr");
        assert_eq!(
            instance_path(10_000),
            format!("{NUMBER_PATH}/{NEW_INSTANCE}")
        );
        assert_eq!(
            instance_path(1000),
            format!("{NUMBER_PATH}/{DONOR_INSTANCE}")
        );
    }

    #[test]
    fn texture_names_follow_the_stock_suffix_rule() {
        // Digit art: ones unsuffixed, the rest by worst grade.
        assert_eq!(texture_name(0, 1, 0, 7).as_deref(), Some("daco_combo_7"));
        assert_eq!(texture_name(0, 1, 3, 7).as_deref(), Some("daco_combo_7"));
        assert_eq!(
            texture_name(0, 10_000, 0, 1).as_deref(),
            Some("daco_combo_marvelous_1")
        );
        assert_eq!(
            texture_name(0, 10, 1, 2).as_deref(),
            Some("daco_combo_perfect_2")
        );
        assert_eq!(
            texture_name(0, 100, 2, 3).as_deref(),
            Some("daco_combo_great_3")
        );
        assert_eq!(
            texture_name(0, 1000, 3, 4).as_deref(),
            Some("daco_combo_good_4")
        );
        // Underlays: always the dummy set.
        assert_eq!(
            texture_name(1, 1, 0, 5).as_deref(),
            Some("daco_combo_dummy_5")
        );
        assert_eq!(
            texture_name(2, 10_000, 3, 9).as_deref(),
            Some("daco_combo_dummy_9")
        );
        // Outside the stock tables.
        assert_eq!(texture_name(0, 10, 0xFF, 1), None);
        assert_eq!(texture_name(0, 10, -1, 1), None);
        assert_eq!(texture_name(3, 10, 0, 1), None);
    }

    #[test]
    fn the_five_digit_row_extends_the_three_digit_row() {
        // Stock rows, px (ones first).
        let three = [(256, 71), (190, 83), (124, 95)];
        let four = [(289, 67), (223, 79), (157, 91), (91, 103)];
        let moved: Vec<(i32, i32)> = four
            .iter()
            .map(|&(x, y)| (x + ROW_SHIFT.0 / 20, y + ROW_SHIFT.1 / 20))
            .collect();
        // Same pitch as stock, the middle three on the three-digit row.
        assert_eq!(moved[0], (three[0].0 + 66, three[0].1 - 12));
        assert_eq!(&moved[1..], &three[..]);
        let new = (NEW_TRANSLATE.0 / 20, NEW_TRANSLATE.1 / 20);
        assert_eq!(new, (moved[3].0 - 66, moved[3].1 + 12));
    }
}
