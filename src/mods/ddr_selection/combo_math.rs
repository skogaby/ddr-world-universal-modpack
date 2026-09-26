//! DDR SELECTION legacy combo math (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! A port of A3's `sequence::dance::ComboActor` (`gamemdx_20240402`): the
//! count-dependent growth factor `FUN_180046970`, the digit count
//! `FUN_180046a20`, the layout `FUN_180047460` (the clip is re-centred on the
//! `combo` marker for the digits shown), the texture writes `FUN_1800470e0`
//! (single sheet on skins 1–3, worst-grade sheets on 4–5; leading places
//! hidden; cap 9999) and the counters of the combo message. Every float is
//! computed in `f32` with A3's operation order.
//!
//! Only the HD path is ported: A3's SD-cabinet shift (machine types 0/1, 2P,
//! 3+ digits: `x -= 30`) is not — World always draws the HD clip.
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/legacy-combo.md`.

/// A3's export in `dance_combo000N` (the one clip World's init creates in
/// place of `dance_combo_root1..3`).
pub const EXPORT: &str = "dance_combo";
/// World's init assigns its root-name format with a fixed length of 0x12
/// bytes (`"dance_combo_root%d"`); the replacement format is the export
/// padded with NULs to that length (the `sprintf_s` then yields the export).
pub const ROOT_FMT_LEN: usize = 0x12;

/// Shown only from this combo (A3 and World: `combo < 4` ⇒ hidden).
pub const MIN_VISIBLE_COMBO: i32 = 4;
/// Highest count the digits show (4 places).
pub const MAX_SHOWN: i32 = 9999;
/// World's worst-grade sentinel (no graded step in the combo).
pub const WORST_NONE: i32 = 0xFF;
/// World / A3's O.K. grade (folded into Marvelous for the worst grade).
pub const GRADE_OK: i32 = 6;

/// The digit places, ones first, and their child paths in the clip.
pub const PLACES: [&str; 4] = [
    "number_usr/0001_usr",
    "number_usr/0010_usr",
    "number_usr/0100_usr",
    "number_usr/1000_usr",
];
/// A3's worst-grade sheet names (`marvelous` .. `good`).
pub const GRADE_NAMES: [&str; 4] = ["marvelous", "perfect", "great", "good"];

// A3's float constants (`gamemdx_20240402`; bits checked by the tests).
/// `DAT_180260fa0`.
pub const ONE: f32 = 1.0;
/// `DAT_180264a58`.
pub const TEN: f32 = 10.0;
/// `DAT_1802888b8`.
pub const SKIN1_STEP: f32 = 0.1;
/// `DAT_1802888b4`.
pub const SKIN1_BIG: f32 = 2.1;
/// `_DAT_180288d9c` = 1/4096.
pub const TENS_STEP: f32 = 1.0 / 4096.0;
/// `DAT_1802647b8`.
pub const HUNDREDS: f32 = 1.5;
/// `DAT_180288d98`.
pub const THOUSANDS: f32 = 1.25;
/// `DAT_180265198`.
pub const HALF: f32 = 0.5;

/// A3's growth factor for `combo` on `skin` (`FUN_180046970`): the digits
/// grow with the count. Skin 1: 1.0, 1.1..1.99 for 10–99, 2.1 for 100–9999;
/// other skins: 1.0, 1 + tens·160/4096 for 10–99, 1.5 for 100–999, 1.25 for
/// 1000–9999; 1.0 below 10 and from 10000.
pub fn growth(combo: i32, skin: u8) -> f32 {
    if combo <= 9 {
        return ONE;
    }
    if skin == 1 {
        if combo < 100 {
            return (combo as f32 / TEN) * SKIN1_STEP + ONE;
        }
        if combo < 10_000 {
            return SKIN1_BIG;
        }
        return ONE;
    }
    if combo < 100 {
        return ((combo / 10) * 0xA0) as f32 * TENS_STEP + ONE;
    }
    if combo < 1000 {
        return HUNDREDS;
    }
    if combo < 10_000 {
        return THOUSANDS;
    }
    ONE
}

/// A3's digit count (`FUN_180046a20`): 1 for anything below 10.
pub fn digit_count(mut n: i32) -> i32 {
    let mut d = 1;
    while n > 9 {
        d += 1;
        n /= 10;
    }
    d
}

/// The count the digits show (`min(combo, 9999)`).
pub fn shown(combo: i32) -> i32 {
    combo.min(MAX_SHOWN)
}

/// Whether the clip shows for `combo`.
pub fn visible(combo: i32) -> bool {
    combo >= MIN_VISIBLE_COMBO
}

/// The digit cell width A3 lays out with: the `number_usr/0001_usr` width,
/// halved on skin 1 (C integer division).
pub fn cell_width(raw_w: i32, skin: u8) -> i32 {
    if skin == 1 {
        raw_w / 2
    } else {
        raw_w
    }
}

/// A3's clip x for `digits` shown at `growth` (`FUN_180047460`): the clip is
/// authored for 4 cells; it moves right by half of the unused width,
/// `x = (int)(marker_x − (4·cell − digits·cell·growth)·0.5)`.
pub fn layout_x(marker_x: i32, cell_w: i32, digits: i32, growth: f32) -> i32 {
    let full = cell_w.wrapping_mul(4) as f32;
    let used = cell_w.wrapping_mul(digits) as f32 * growth;
    (marker_x as f32 - (full - used) * HALF) as i32
}

/// The combo centre A3 sends the NoteResultActor (FAST/SLOW x): the clip x
/// plus half its layer width (C integer division).
pub fn center_x(x: i32, clip_w: i32) -> i32 {
    clip_w / 2 + x
}

/// The combo message's counters (World and A3 alike): a broken combo resets
/// the worst grade; otherwise the worst grade of the chain so far (O.K. = 6
/// counts as Marvelous).
pub fn update_worst(worst: i32, combo: i32, grade: i32) -> i32 {
    if combo < 1 {
        return WORST_NONE;
    }
    let g = if grade == GRADE_OK { 0 } else { grade };
    if worst == WORST_NONE || worst < g {
        g
    } else {
        worst
    }
}

/// Skins 1–3 use one sheet (`DAT_180265038 = {1, 2, 3}`), the others (4–5
/// and A3's own skin 0 — the themes) one per grade.
pub fn single_sheet(skin: u8) -> bool {
    (1..=3).contains(&skin)
}

/// The S-Marvelous sheet name on a per-grade skin — the S-Marvelous
/// Judgement mod's legacy combo art (`dance_combo%04d_smarvelous_*`, staged
/// by `s_marvelous::assets::stage_legacy_combo` under the same names).
pub const SMARV_SHEET: &str = "smarvelous";

/// The texture-name prefix (`dance_combo%04d` or `dance_combo%04d_<grade>`,
/// worst grade clamped to `good` — A3 indexed its 4-name table unchecked;
/// `%04d` = [`super::policy::tex_number`], 0 for a theme).
/// `all_smarvelous` (S-Marvelous: every step of the combo within its window,
/// that skin's sheet staged) turns a Marvelous combo on a per-grade skin into
/// the `smarvelous` sheet; single-sheet skins ignore it, as A3 ignored the
/// grade there.
pub fn sheet_prefix(skin: u8, worst: i32, all_smarvelous: bool) -> String {
    let n = super::policy::tex_number(skin);
    if single_sheet(skin) {
        format!("dance_combo{:04}", n)
    } else if all_smarvelous && worst == 0 {
        format!("dance_combo{:04}_{}", n, SMARV_SHEET)
    } else {
        let i = worst.clamp(0, 3) as usize;
        format!("dance_combo{:04}_{}", n, GRADE_NAMES[i])
    }
}

/// One digit place: texture and visibility.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub path: &'static str,
    pub texture: String,
    pub visible: bool,
}

/// A3's digit writes for `shown` (0..=9999): every place gets its digit's
/// texture; the ones place is always visible, a higher place only when the
/// count reaches it.
pub fn places(prefix: &str, shown: i32) -> [Place; 4] {
    let mut n = shown.max(0);
    let mut vis = true;
    PLACES.map(|path| {
        let p = Place {
            path,
            texture: format!("{}_{}", prefix, n % 10),
            visible: vis,
        };
        n /= 10;
        if n == 0 {
            vis = false;
        }
        p
    })
}

/// The `combo_usr` word texture.
pub fn word_texture(prefix: &str) -> String {
    format!("{}_combo", prefix)
}

/// The replacement root-name format World's init assigns (the export padded
/// with NULs to [`ROOT_FMT_LEN`]).
pub fn root_fmt_bytes() -> [u8; ROOT_FMT_LEN] {
    let mut b = [0u8; ROOT_FMT_LEN];
    b[..EXPORT.len()].copy_from_slice(EXPORT.as_bytes());
    b
}

/// IFS container magic (`0x6CAD8F89`, big-endian on disk).
pub const IFS_MAGIC: [u8; 4] = [0x6C, 0xAD, 0x8F, 0x89];

/// Whether an arc member (the decompressed `.ifs`) is a real IFS — World's
/// blanked `dance_combo0005_v0.arc` decompresses to zeros.
pub fn ifs_member_ok(member: &[u8]) -> bool {
    member.len() >= 16 && member[..4] == IFS_MAGIC
}

/// The arc names World's package probe tries for `base` (`_v3`, `_v0`,
/// bare — the pcType-gated `_lite` is never used for legacy packages).
pub fn arc_candidates(base: &str) -> [String; 3] {
    [
        format!("{}_v3.arc", base),
        format!("{}_v0.arc", base),
        format!("{}.arc", base),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a3_constant_bits() {
        assert_eq!(ONE.to_bits(), 0x3F80_0000);
        assert_eq!(TEN.to_bits(), 0x4120_0000);
        assert_eq!(SKIN1_STEP.to_bits(), 0x3DCC_CCCD);
        assert_eq!(SKIN1_BIG.to_bits(), 0x4006_6666);
        assert_eq!(TENS_STEP.to_bits(), 0x3980_0000);
        assert_eq!(HUNDREDS.to_bits(), 0x3FC0_0000);
        assert_eq!(THOUSANDS.to_bits(), 0x3FA0_0000);
        assert_eq!(HALF.to_bits(), 0x3F00_0000);
    }

    #[test]
    fn growth_skin1() {
        assert_eq!(growth(0, 1), 1.0);
        assert_eq!(growth(9, 1), 1.0);
        assert_eq!(growth(10, 1), (10.0f32 / 10.0) * 0.1 + 1.0);
        assert!((growth(10, 1) - 1.1).abs() < 1e-6);
        assert!((growth(99, 1) - 1.99).abs() < 1e-6);
        assert_eq!(growth(100, 1), SKIN1_BIG);
        assert_eq!(growth(9999, 1), SKIN1_BIG);
        assert_eq!(growth(10_000, 1), 1.0);
    }

    #[test]
    fn growth_other_skins() {
        for skin in 2..=5 {
            assert_eq!(growth(9, skin), 1.0);
            assert_eq!(growth(10, skin), 1.0 + 160.0 / 4096.0);
            assert_eq!(growth(19, skin), 1.0 + 160.0 / 4096.0);
            assert_eq!(growth(99, skin), 1.0 + 1440.0 / 4096.0);
            assert_eq!(growth(100, skin), 1.5);
            assert_eq!(growth(999, skin), 1.5);
            assert_eq!(growth(1000, skin), 1.25);
            assert_eq!(growth(9999, skin), 1.25);
            assert_eq!(growth(10_000, skin), 1.0);
        }
    }

    #[test]
    fn digits_and_visibility() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(-5), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(999), 3);
        assert_eq!(digit_count(1000), 4);
        assert_eq!(digit_count(12345), 5);
        assert_eq!(shown(12345), 9999);
        assert!(!visible(3));
        assert!(visible(4));
    }

    #[test]
    fn layout_recentres_on_the_marker() {
        // 4 digits at growth 1: the authored position.
        assert_eq!(layout_x(640, 52, 4, 1.0), 640);
        // 1 digit: half of the three unused cells to the right.
        assert_eq!(layout_x(640, 52, 1, 1.0), 640 - (208 - 52) / 2);
        // truncation toward zero of the float result
        assert_eq!(
            layout_x(100, 53, 2, 1.0390625),
            (100.0f32 - (212.0 - 106.0 * 1.0390625) * 0.5) as i32
        );
        assert_eq!(center_x(300, 101), 350);
        assert_eq!(center_x(-10, 7), -7);
        assert_eq!(cell_width(53, 1), 26);
        assert_eq!(cell_width(53, 2), 53);
    }

    #[test]
    fn worst_grade_bookkeeping() {
        assert_eq!(update_worst(2, 0, 1), WORST_NONE);
        assert_eq!(update_worst(WORST_NONE, 1, 2), 2);
        assert_eq!(update_worst(1, 5, 0), 1);
        assert_eq!(update_worst(0, 5, 2), 2);
        assert_eq!(update_worst(1, 5, GRADE_OK), 1);
        assert_eq!(update_worst(WORST_NONE, 1, GRADE_OK), 0);
    }

    #[test]
    fn texture_names() {
        assert_eq!(sheet_prefix(1, 2, false), "dance_combo0001");
        assert_eq!(sheet_prefix(3, 0, false), "dance_combo0003");
        assert_eq!(sheet_prefix(4, 0, false), "dance_combo0004_marvelous");
        assert_eq!(sheet_prefix(5, 2, false), "dance_combo0005_great");
        assert_eq!(sheet_prefix(5, WORST_NONE, false), "dance_combo0005_good");
        assert_eq!(word_texture("dance_combo0002"), "dance_combo0002_combo");
        let p = places("dance_combo0001", 305);
        assert_eq!(p[0].texture, "dance_combo0001_5");
        assert_eq!(p[1].texture, "dance_combo0001_0");
        assert_eq!(p[2].texture, "dance_combo0001_3");
        assert_eq!(p[3].texture, "dance_combo0001_0");
        assert_eq!(
            p.iter().map(|x| x.visible).collect::<Vec<_>>(),
            [true, true, true, false]
        );
        let p = places("x", 7);
        assert_eq!(
            p.iter().map(|x| x.visible).collect::<Vec<_>>(),
            [true, false, false, false]
        );
        let p = places("x", 1000);
        assert!(p.iter().all(|x| x.visible));
    }

    #[test]
    fn smarvelous_sheet() {
        // Per-grade skins: an all-S-Marvelous Marvelous combo takes the
        // S-Marvelous sheet (the names S-Marvelous stages).
        assert_eq!(sheet_prefix(4, 0, true), "dance_combo0004_smarvelous");
        assert_eq!(sheet_prefix(5, 0, true), "dance_combo0005_smarvelous");
        assert_eq!(
            places(&sheet_prefix(4, 0, true), 12)[0].texture,
            "dance_combo0004_smarvelous_2"
        );
        assert_eq!(
            word_texture(&sheet_prefix(5, 0, true)),
            "dance_combo0005_smarvelous_combo"
        );
        // A worse grade in the combo wins over the flag.
        assert_eq!(sheet_prefix(4, 1, true), "dance_combo0004_perfect");
        assert_eq!(sheet_prefix(5, WORST_NONE, true), "dance_combo0005_good");
        // Single-sheet skins ignore it (A3 ignored the grade there).
        assert_eq!(sheet_prefix(1, 0, true), "dance_combo0001");
        assert_eq!(sheet_prefix(3, 0, true), "dance_combo0003");
    }

    #[test]
    fn root_format_and_arc_checks() {
        let b = root_fmt_bytes();
        assert_eq!(&b[..11], b"dance_combo");
        assert!(b[11..].iter().all(|c| *c == 0));
        assert_eq!(b.len(), "dance_combo_root%d".len());
        assert!(!ifs_member_ok(&[0u8; 64]));
        let mut ok = vec![0u8; 64];
        ok[..4].copy_from_slice(&IFS_MAGIC);
        assert!(ifs_member_ok(&ok));
        assert_eq!(
            arc_candidates("dance_combo0005")[1],
            "dance_combo0005_v0.arc"
        );
    }

    #[test]
    fn themes_use_a3s_skin_0_sheets() {
        // Per-grade sheets named with A3's skin-0 number.
        for skin in 6..=8 {
            assert!(!single_sheet(skin));
            for (worst, grade) in GRADE_NAMES.iter().enumerate() {
                assert_eq!(
                    sheet_prefix(skin, worst as i32, false),
                    format!("dance_combo0000_{grade}")
                );
            }
            assert_eq!(
                sheet_prefix(skin, WORST_NONE, false),
                "dance_combo0000_good"
            );
            assert_eq!(sheet_prefix(skin, 0, true), "dance_combo0000_smarvelous");
            assert_eq!(
                word_texture(&sheet_prefix(skin, 2, false)),
                "dance_combo0000_great_combo"
            );
            // Standard growth and full cells (A3's non-skin-1 paths).
            assert_eq!(growth(100, skin), growth(100, 4));
            assert_eq!(growth(15, skin), growth(15, 4));
            assert_eq!(cell_width(40, skin), cell_width(40, 4));
        }
    }
}
