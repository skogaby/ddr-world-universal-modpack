//! DDR SELECTION themes — the stage panel's per-player score sets (pure,
//! host-tested). A port of A3 `FUN_180032240(root, set, name, area, record)`
//! over World's records; the engine side (`score_set.rs`) reads World's
//! memory into [`SideInputs`], `panel.rs` applies the [`Write`]s on the
//! adopted theme root. RE: `docs/ddr_selection_theme_score_sets.md`.
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate.

/// Glyph and digit textures (`playername_*`, `scene_choice_num_*`), requested
/// by FULL name: the bare `common_texture` resolves `_v3` (World's), which
/// has none of them.
pub const TEXTURE_PACKAGE: &str = "common_texture_v0";
/// Name glyph slots (`highscore_name1..8_usr`).
pub const NAME_SLOTS: usize = 8;
/// Score digit places (`highscore_0000001_usr` … `highscore_1000000_usr`).
pub const SCORE_DIGITS: usize = 7;
/// A3's rank textures by World's rank value 0..15 (the same order as World's
/// `musi_dif_rank_*` table).
pub const RANK_NAMES: [&str; 16] = [
    "aaa", "aa_p", "aa", "aa_m", "a_p", "a", "a_m", "b_p", "b", "b_m", "c_p", "c", "c_m", "d_p",
    "d", "e",
];
/// A3's `dancer_region_*` table (`0x180262960`), by area code 0..=118.
pub const AREA_NAMES: [&str; 119] = [
    "unknown",
    "hokaidou",
    "aomori",
    "iwate",
    "miyagi",
    "akita",
    "yamagata",
    "fukusima",
    "ibaragi",
    "totigi",
    "gunma",
    "saitama",
    "tiba",
    "tkyo",
    "kanagawa",
    "nigata",
    "toyama",
    "isikawa",
    "fukui",
    "yamanasi",
    "nagano",
    "gifu",
    "shizuoka",
    "aiti",
    "mie",
    "siga",
    "kyoto",
    "oosaka",
    "hyougo",
    "nara",
    "wakayama",
    "totori",
    "simane",
    "okayama",
    "hirosima",
    "yamaguti",
    "tokusima",
    "kagawa",
    "ehime",
    "kouti",
    "fukuoka",
    "saga",
    "nagasaki",
    "kumamoto",
    "ooita",
    "miyazaki",
    "kagosima",
    "okinawa",
    "hongkong",
    "korea",
    "taiwan",
    "america",
    "europe",
    "overseas",
    "usa_ak",
    "usa_al",
    "usa_ar",
    "usa_az",
    "usa_ca",
    "usa_co",
    "usa_ct",
    "usa_de",
    "usa_fl",
    "usa_ga",
    "usa_hi",
    "usa_ia",
    "usa_id",
    "usa_il",
    "usa_in",
    "usa_ks",
    "usa_ky",
    "usa_la",
    "usa_ma",
    "usa_md",
    "usa_me",
    "usa_mi",
    "usa_mn",
    "usa_mo",
    "usa_ms",
    "usa_mt",
    "usa_ne",
    "usa_nc",
    "usa_nd",
    "usa_nh",
    "usa_nj",
    "usa_nm",
    "usa_nv",
    "usa_ny",
    "usa_oh",
    "usa_ok",
    "usa_or",
    "usa_pa",
    "usa_ri",
    "usa_sc",
    "usa_sd",
    "usa_tn",
    "usa_tx",
    "usa_ut",
    "usa_va",
    "usa_vt",
    "usa_wa",
    "usa_wi",
    "usa_wv",
    "usa_wy",
    "usa_wdc",
    "japan",
    "canada",
    "singapore",
    "thai",
    "australia",
    "newzealand",
    "uk",
    "italy",
    "spain",
    "germany",
    "france",
    "portugal",
    "indonesia",
    "philippines",
];

/// One entry of World's best-record / rival-set score tables (0x30 bytes;
/// the three fields the panel reads).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub score: u32,
    /// 0..=15 ([`RANK_NAMES`]); `0x11` = none.
    pub rank: u32,
    /// 0 none, 2 assisted, 3 clear, 4..=6 LIFE4, 7 good FC, 8 great FC, 9 PFC,
    /// 10 MFC.
    pub clear_kind: u32,
}

impl Record {
    /// The chart was played (World's own test: its song select shows no score
    /// and its panel no target score on clear kind 0).
    pub fn played(&self) -> bool {
        self.clear_kind != 0
    }
}

/// What a record read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordRead {
    /// The lookup is unavailable or its memory unreadable: the score, rank
    /// and mark are hidden (fail-open).
    Unavailable,
    /// No record for this chart: A3's zero record (score `0`, no rank, no
    /// mark).
    Absent,
    Found(Record),
}

impl RecordRead {
    /// The record A3 would display (`None` = its zero record).
    fn shown(&self) -> Option<Record> {
        match *self {
            RecordRead::Found(r) if r.played() => Some(r),
            _ => None,
        }
    }
}

/// One set's inputs (a `None` field is hidden).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetInputs {
    /// Chart difficulty 0..=4 (the high-score set only; the target set has no
    /// difficulty child).
    pub difficulty: Option<i32>,
    /// The dancer name's bytes (up to [`NAME_SLOTS`] are shown).
    pub name: Option<Vec<u8>>,
    /// The area texture (`dancer_region_*`), `None` when the area package is
    /// not available.
    pub area: Option<String>,
    pub record: RecordRead,
}

/// Everything one side's `pN_score_set_mc` needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideInputs {
    /// 0 = P1, 1 = P2.
    pub side: usize,
    /// [`set_visible`].
    pub visible: bool,
    /// `common_texture_v0` is registered: the name glyphs and score digits
    /// can be shown.
    pub glyphs_ready: bool,
    pub high: SetInputs,
    /// `None` = the target set is hidden ([`target_choice`] or an unresolved
    /// set).
    pub target: Option<SetInputs>,
}

/// One MovieClip update under the adopted root: load `texture` (if any) into
/// every instance of `path`, then set its visibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub path: String,
    pub texture: Option<String>,
    pub visible: bool,
}

impl Write {
    fn show(path: String, texture: impl Into<String>) -> Self {
        Write {
            path,
            texture: Some(texture.into()),
            visible: true,
        }
    }

    fn hide(path: String) -> Self {
        Write {
            path,
            texture: None,
            visible: false,
        }
    }
}

/// A3: `pN_score_set_mc` shows for an entered side outside courses, or on a
/// course's first stage (courses are refused by DDR SELECTION's mode policy).
pub fn set_visible(entered: bool, course: bool, stage: i32) -> bool {
    entered && (!course || stage == 0)
}

/// A3 `FUN_1800ffe00`: the `playername_*` glyph of one name byte. Letters
/// (upper case lower-cased), digits and seven symbols; everything else is
/// `blank`.
pub fn glyph_texture(c: u8) -> String {
    let suffix = match c {
        b'a'..=b'z' | b'0'..=b'9' => (c as char).to_string(),
        b'A'..=b'Z' => (c.to_ascii_lowercase() as char).to_string(),
        b'!' => "exclamation".into(),
        b'$' => "doll".into(),
        b'&' => "and".into(),
        b'-' => "hifun".into(),
        b'.' => "dot".into(),
        b'?' => "question".into(),
        _ => "blank".into(),
    };
    format!("playername_{suffix}")
}

/// The eight glyph textures of a name (A3 `FUN_180100170`): one per byte up
/// to the first NUL, `blank` for the unused slots.
pub fn name_glyphs(name: &[u8]) -> Vec<String> {
    let len = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    (0..NAME_SLOTS)
        .map(|i| glyph_texture(if i < len { name[i] } else { b' ' }))
        .collect()
}

/// World's dancer-name rule for an entered side (`FUN_180035f00`): the
/// profile name (`PlayerWork+0xC`, at most 8 bytes), or `PLAYER1` / `PLAYER2`
/// by the side index (`PlayerWork+0`) when it is empty, else `PLAYER`.
pub fn player_name(raw: &[u8], side_index: u32) -> Vec<u8> {
    let len = raw
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(raw.len())
        .min(NAME_SLOTS);
    if len > 0 {
        return raw[..len].to_vec();
    }
    match side_index {
        0 => b"PLAYER1".to_vec(),
        1 => b"PLAYER2".to_vec(),
        _ => b"PLAYER".to_vec(),
    }
}

/// The child of digit place `i` (0 = ones): `highscore_%07d_usr` of 10^i.
pub fn digit_child(i: usize) -> String {
    format!("highscore_{:07}_usr", 10u32.pow(i as u32))
}

/// A3 `FUN_1800ff9d0` (no padding): place `i` shows `(score / 10^i) % 10`
/// and is visible iff it is the ones place or `score >= 10^i`.
pub fn digit(score: u32, i: usize) -> (u32, bool) {
    let p = 10u64.pow(i as u32);
    let s = score as u64;
    (((s / p) % 10) as u32, i == 0 || s >= p)
}

/// `scene_choice_{beginner, basic, difficult, expert, challenge}` (World has
/// no edit charts, so A3's `_edit_*` variants are not ported).
pub fn difficulty_texture(difficulty: i32) -> Option<&'static str> {
    Some(match difficulty {
        0 => "scene_choice_beginner",
        1 => "scene_choice_basic",
        2 => "scene_choice_difficult",
        3 => "scene_choice_expert",
        4 => "scene_choice_challenge",
        _ => return None,
    })
}

/// `scene_choice_rank_<r>` for rank 0..=15, `None` otherwise (`0x11` = none).
pub fn rank_texture(rank: u32) -> Option<String> {
    RANK_NAMES
        .get(rank as usize)
        .map(|r| format!("scene_choice_rank_{r}"))
}

/// A3's full-combo marks for clear kinds 7..=10; nothing otherwise.
pub fn fc_mark_texture(clear_kind: u32) -> Option<&'static str> {
    Some(match clear_kind {
        7 => "scene_choice_fullcombomark_good",
        8 => "scene_choice_fullcombomark_great",
        9 => "scene_choice_fullcombomark_perfect",
        10 => "scene_choice_fullcombomark_marvelous",
        _ => return None,
    })
}

/// A3 `FUN_180100280` (World's twin `FUN_1801ae0d0`): the area texture for a
/// region (`arkMDXGetLicenceKeyVersion`, 0 when unreadable) and an area code.
pub fn area_texture(region: i32, area: i32) -> String {
    let name = if matches!(region, 1 | 4 | 6) && (1..=47).contains(&area) {
        "japan"
    } else if !matches!(region, 1 | 6) && (54..=104).contains(&area) {
        "america"
    } else {
        usize::try_from(area)
            .ok()
            .and_then(|a| AREA_NAMES.get(a))
            .copied()
            .unwrap_or("unknown")
    };
    format!("dancer_region_{name}")
}

/// World's package-language suffix for `arkMDXGetGameOptionsLanguage`
/// (`FUN_180001060` maps 0 / 1 / 2 / 3 / 4 to the suffix-table indices
/// 0 / 1 / 10 / 9 / 8; an empty slot reads `_lang_jpn`).
pub fn lang_suffix(ark_language: i32) -> &'static str {
    match ark_language {
        1 => "_lang_eng",
        2 => "_lang_kor",
        3 => "_lang_han",
        4 => "_lang_kan",
        _ => "_lang_jpn",
    }
}

/// The theme generation's area package (`common_area_lang_eng_v2`, …).
pub fn area_package(lang_suffix: &str, theme_suffix: &str) -> String {
    format!("common_area{lang_suffix}{theme_suffix}")
}

/// What the TARGET option shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetChoice {
    Hidden,
    /// The player's own best.
    Own,
    /// A rival slot 0..=2 (its code at `TARGET + 4 + slot·4`).
    Rival(usize),
    /// A ranking set of kind 0..=2 (world / area / machine).
    Ranking(i32),
}

/// World's panel rule (`FUN_180035f00`) on the TARGET option
/// (`PlayerWork+0x1328`; `+0x1308` on the old builds): −1 or an event mode
/// (1 / 2) hides the set; 0 = own best, 1..=3 rivals, 4..=6 ranking kinds
/// 0..=2. Any other value hides it (A3 left such a set unfilled).
pub fn target_choice(option: i32, event_mode: i32) -> TargetChoice {
    if event_mode == 1 || event_mode == 2 {
        return TargetChoice::Hidden;
    }
    match option {
        0 => TargetChoice::Own,
        1..=3 => TargetChoice::Rival((option - 1) as usize),
        4..=6 => TargetChoice::Ranking(option - 4),
        _ => TargetChoice::Hidden,
    }
}

/// The writes for one side's `pN_score_set_mc`.
pub fn fill_side(inp: &SideInputs) -> Vec<Write> {
    let n = inp.side + 1;
    let root = format!("p{n}_score_set_mc");
    let mut out = vec![Write {
        path: root.clone(),
        texture: None,
        visible: inp.visible,
    }];
    if !inp.visible {
        return out;
    }
    fill_set(
        &mut out,
        &format!("{root}/p{n}_highscore_usr"),
        &inp.high,
        inp.glyphs_ready,
        true,
    );
    let target = format!("{root}/p{n}_target_usr");
    match &inp.target {
        Some(t) => {
            out.push(Write {
                path: target.clone(),
                texture: None,
                visible: true,
            });
            fill_set(&mut out, &target, t, inp.glyphs_ready, false);
        }
        None => out.push(Write::hide(target)),
    }
    out
}

/// A3 `FUN_180032240` for one set (+ the high-score set's difficulty).
fn fill_set(out: &mut Vec<Write>, prefix: &str, set: &SetInputs, glyphs: bool, difficulty: bool) {
    if difficulty {
        let path = format!("{prefix}/highscore_difficulty_usr");
        out.push(match set.difficulty.and_then(difficulty_texture) {
            Some(t) => Write::show(path, t),
            None => Write::hide(path),
        });
    }

    let names = format!("{prefix}/choice_dancer_name_usr");
    match set.name.as_deref() {
        Some(name) if glyphs => {
            out.push(Write {
                path: names.clone(),
                texture: None,
                visible: true,
            });
            for (i, g) in name_glyphs(name).into_iter().enumerate() {
                out.push(Write::show(
                    format!("{names}/highscore_name{}_usr", i + 1),
                    g,
                ));
            }
        }
        _ => out.push(Write::hide(names)),
    }

    let shown = set.record.shown();
    let digits = format!("{prefix}/choice_score_usr");
    if glyphs && set.record != RecordRead::Unavailable {
        out.push(Write {
            path: digits.clone(),
            texture: None,
            visible: true,
        });
        let score = shown.map_or(0, |r| r.score);
        for i in 0..SCORE_DIGITS {
            let (d, visible) = digit(score, i);
            out.push(Write {
                path: format!("{digits}/{}", digit_child(i)),
                texture: Some(format!("scene_choice_num_{d}")),
                visible,
            });
        }
    } else {
        out.push(Write::hide(digits));
    }

    let rank = format!("{prefix}/highscore_rank_usr");
    out.push(match shown.and_then(|r| rank_texture(r.rank)) {
        Some(t) => Write::show(rank, t),
        None => Write::hide(rank),
    });

    let mark = format!("{prefix}/fullcombo_mark_rotate_usr/fullcombo_mark_usr");
    out.push(match shown.and_then(|r| fc_mark_texture(r.clear_kind)) {
        Some(t) => Write::show(mark, t),
        None => Write::hide(mark),
    });

    let area = format!("{prefix}/highscore_area_usr");
    out.push(match &set.area {
        Some(t) => Write::show(area, t.clone()),
        None => Write::hide(area),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(score: u32, rank: u32, clear_kind: u32) -> RecordRead {
        RecordRead::Found(Record {
            score,
            rank,
            clear_kind,
        })
    }

    fn set(name: &[u8], record: RecordRead) -> SetInputs {
        SetInputs {
            difficulty: Some(3),
            name: Some(name.to_vec()),
            area: Some("dancer_region_tkyo".into()),
            record,
        }
    }

    fn side(side: usize, high: SetInputs, target: Option<SetInputs>) -> SideInputs {
        SideInputs {
            side,
            visible: true,
            glyphs_ready: true,
            high,
            target,
        }
    }

    fn find<'a>(w: &'a [Write], path: &str) -> &'a Write {
        w.iter()
            .find(|x| x.path == path)
            .unwrap_or_else(|| panic!("no write for {path}: {w:#?}"))
    }

    fn absent(w: &[Write], path: &str) -> bool {
        w.iter().all(|x| x.path != path)
    }

    const HS: &str = "p1_score_set_mc/p1_highscore_usr";
    const TG: &str = "p1_score_set_mc/p1_target_usr";

    #[test]
    fn visibility_rule() {
        assert!(set_visible(true, false, 0));
        assert!(set_visible(true, false, 3));
        assert!(!set_visible(false, false, 0));
        assert!(set_visible(true, true, 0));
        assert!(!set_visible(true, true, 1));
    }

    #[test]
    fn hidden_side_writes_only_its_root() {
        let mut s = side(1, set(b"ABC", rec(1, 0, 3)), None);
        s.visible = false;
        let w = fill_side(&s);
        assert_eq!(w, vec![Write::hide("p2_score_set_mc".into())]);
    }

    #[test]
    fn difficulty_textures() {
        let want = ["beginner", "basic", "difficult", "expert", "challenge"];
        for (d, n) in want.iter().enumerate() {
            assert_eq!(
                difficulty_texture(d as i32).unwrap(),
                format!("scene_choice_{n}")
            );
        }
        assert!(difficulty_texture(-1).is_none());
        assert!(difficulty_texture(5).is_none());
    }

    #[test]
    fn glyph_map() {
        assert_eq!(glyph_texture(b'A'), "playername_a");
        assert_eq!(glyph_texture(b'Z'), "playername_z");
        assert_eq!(glyph_texture(b'q'), "playername_q");
        for c in b'0'..=b'9' {
            assert_eq!(glyph_texture(c), format!("playername_{}", c as char));
        }
        for (c, n) in [
            (b' ', "blank"),
            (b'!', "exclamation"),
            (b'$', "doll"),
            (b'&', "and"),
            (b'-', "hifun"),
            (b'.', "dot"),
            (b'?', "question"),
        ] {
            assert_eq!(glyph_texture(c), format!("playername_{n}"));
        }
        // World's extra name symbols and anything else: blank.
        for c in [b',', b'%', b'+', b'/', b'~', b'_', b'*', 0x80, 0xFF, 0x01] {
            assert_eq!(glyph_texture(c), "playername_blank", "{c:#x}");
        }
    }

    #[test]
    fn name_glyphs_fill_eight_slots() {
        let g = name_glyphs(b"Ab1\0zz");
        assert_eq!(g.len(), NAME_SLOTS);
        assert_eq!(&g[..3], ["playername_a", "playername_b", "playername_1"]);
        assert!(g[3..].iter().all(|x| x == "playername_blank"));
        let long = name_glyphs(b"ABCDEFGHIJ");
        assert_eq!(long[7], "playername_h");
        assert!(name_glyphs(b"").iter().all(|x| x == "playername_blank"));
    }

    #[test]
    fn world_name_rule() {
        assert_eq!(player_name(b"DANCER\0\0\0", 0), b"DANCER");
        assert_eq!(player_name(b"\0\0\0", 0), b"PLAYER1");
        assert_eq!(player_name(b"", 1), b"PLAYER2");
        assert_eq!(player_name(b"\0", 2), b"PLAYER");
        assert_eq!(player_name(b"ABCDEFGHI", 0), b"ABCDEFGH");
    }

    #[test]
    fn digit_places_and_leading_zeros() {
        assert_eq!(digit_child(0), "highscore_0000001_usr");
        assert_eq!(digit_child(3), "highscore_0001000_usr");
        assert_eq!(digit_child(6), "highscore_1000000_usr");
        // 0: only the ones place, showing 0.
        assert_eq!(digit(0, 0), (0, true));
        for i in 1..SCORE_DIGITS {
            assert_eq!(digit(0, i), (0, false));
        }
        // 1,000,000: every place, a 1 in the millions.
        for i in 0..6 {
            assert_eq!(digit(1_000_000, i), (0, true));
        }
        assert_eq!(digit(1_000_000, 6), (1, true));
        // 987,654: six places.
        let d: Vec<_> = (0..SCORE_DIGITS).map(|i| digit(987_654, i)).collect();
        assert_eq!(
            d,
            vec![
                (4, true),
                (5, true),
                (6, true),
                (7, true),
                (8, true),
                (9, true),
                (0, false)
            ]
        );
        assert_eq!(digit(10, 1), (1, true));
        assert_eq!(digit(10, 2), (0, false));
        assert_eq!(digit(9, 1), (0, false));
    }

    #[test]
    fn ranks() {
        for (r, n) in RANK_NAMES.iter().enumerate() {
            assert_eq!(
                rank_texture(r as u32).unwrap(),
                format!("scene_choice_rank_{n}")
            );
        }
        assert_eq!(rank_texture(0).unwrap(), "scene_choice_rank_aaa");
        assert_eq!(rank_texture(15).unwrap(), "scene_choice_rank_e");
        assert!(rank_texture(16).is_none());
        assert!(rank_texture(0x11).is_none());
    }

    #[test]
    fn fc_marks() {
        assert_eq!(fc_mark_texture(7), Some("scene_choice_fullcombomark_good"));
        assert_eq!(fc_mark_texture(8), Some("scene_choice_fullcombomark_great"));
        assert_eq!(
            fc_mark_texture(9),
            Some("scene_choice_fullcombomark_perfect")
        );
        assert_eq!(
            fc_mark_texture(10),
            Some("scene_choice_fullcombomark_marvelous")
        );
        for k in (0..=6).chain([11, 12, 0xFF]) {
            assert!(fc_mark_texture(k).is_none(), "{k}");
        }
    }

    #[test]
    fn area_rule() {
        assert_eq!(AREA_NAMES.len(), 119);
        assert_eq!(area_texture(0, 13), "dancer_region_tkyo");
        // Japanese licences group the prefectures.
        for region in [1, 4, 6] {
            assert_eq!(area_texture(region, 1), "dancer_region_japan");
            assert_eq!(area_texture(region, 47), "dancer_region_japan");
            assert_eq!(area_texture(region, 48), "dancer_region_hongkong");
        }
        // Everyone but licences 1 / 6 groups the US states.
        for region in [0, 2, 3, 4, 5] {
            assert_eq!(area_texture(region, 54), "dancer_region_america");
            assert_eq!(area_texture(region, 104), "dancer_region_america");
        }
        assert_eq!(area_texture(1, 60), "dancer_region_usa_ct");
        assert_eq!(area_texture(6, 104), "dancer_region_usa_wdc");
        assert_eq!(area_texture(0, 105), "dancer_region_japan");
        assert_eq!(area_texture(0, 118), "dancer_region_philippines");
        assert_eq!(area_texture(0, 0), "dancer_region_unknown");
        assert_eq!(area_texture(0, 119), "dancer_region_unknown");
        assert_eq!(area_texture(1, -1), "dancer_region_unknown");
    }

    #[test]
    fn area_package_names() {
        assert_eq!(lang_suffix(0), "_lang_jpn");
        assert_eq!(lang_suffix(1), "_lang_eng");
        assert_eq!(lang_suffix(2), "_lang_kor");
        assert_eq!(lang_suffix(3), "_lang_han");
        assert_eq!(lang_suffix(4), "_lang_kan");
        assert_eq!(lang_suffix(5), "_lang_jpn");
        assert_eq!(lang_suffix(-1), "_lang_jpn");
        assert_eq!(
            area_package(lang_suffix(1), "_v2"),
            "common_area_lang_eng_v2"
        );
        assert_eq!(
            area_package(lang_suffix(0), "_v0"),
            "common_area_lang_jpn_v0"
        );
    }

    #[test]
    fn target_rules() {
        assert_eq!(target_choice(-1, 0), TargetChoice::Hidden);
        assert_eq!(target_choice(0, 0), TargetChoice::Own);
        assert_eq!(target_choice(1, 0), TargetChoice::Rival(0));
        assert_eq!(target_choice(3, 0), TargetChoice::Rival(2));
        assert_eq!(target_choice(4, 0), TargetChoice::Ranking(0));
        assert_eq!(target_choice(6, 0), TargetChoice::Ranking(2));
        assert_eq!(target_choice(7, 0), TargetChoice::Hidden);
        assert_eq!(target_choice(-2, 0), TargetChoice::Hidden);
        for opt in [0, 1, 4] {
            assert_eq!(target_choice(opt, 1), TargetChoice::Hidden);
            assert_eq!(target_choice(opt, 2), TargetChoice::Hidden);
            assert_ne!(target_choice(opt, 3), TargetChoice::Hidden);
        }
    }

    #[test]
    fn full_high_score_set() {
        let w = fill_side(&side(0, set(b"AB", rec(987_654, 0, 10)), None));
        assert_eq!(find(&w, "p1_score_set_mc").visible, true);
        assert_eq!(
            find(&w, &format!("{HS}/highscore_difficulty_usr"))
                .texture
                .as_deref(),
            Some("scene_choice_expert")
        );
        let n1 = find(
            &w,
            &format!("{HS}/choice_dancer_name_usr/highscore_name1_usr"),
        );
        assert_eq!(n1.texture.as_deref(), Some("playername_a"));
        let n8 = find(
            &w,
            &format!("{HS}/choice_dancer_name_usr/highscore_name8_usr"),
        );
        assert_eq!(n8.texture.as_deref(), Some("playername_blank"));
        assert!(n8.visible);
        let d0 = find(&w, &format!("{HS}/choice_score_usr/highscore_0000001_usr"));
        assert_eq!(
            (d0.texture.as_deref(), d0.visible),
            (Some("scene_choice_num_4"), true)
        );
        let d6 = find(&w, &format!("{HS}/choice_score_usr/highscore_1000000_usr"));
        assert!(!d6.visible);
        let rank = find(&w, &format!("{HS}/highscore_rank_usr"));
        assert_eq!(
            (rank.texture.as_deref(), rank.visible),
            (Some("scene_choice_rank_aaa"), true)
        );
        let mark = find(
            &w,
            &format!("{HS}/fullcombo_mark_rotate_usr/fullcombo_mark_usr"),
        );
        assert_eq!(
            mark.texture.as_deref(),
            Some("scene_choice_fullcombomark_marvelous")
        );
        let area = find(&w, &format!("{HS}/highscore_area_usr"));
        assert_eq!(area.texture.as_deref(), Some("dancer_region_tkyo"));
        assert!(!find(&w, TG).visible);
    }

    #[test]
    fn no_record_is_a3s_zero_record() {
        for record in [RecordRead::Absent, rec(0, 0, 0)] {
            let w = fill_side(&side(0, set(b"AB", record), None));
            let d0 = find(&w, &format!("{HS}/choice_score_usr/highscore_0000001_usr"));
            assert_eq!(
                (d0.texture.as_deref(), d0.visible),
                (Some("scene_choice_num_0"), true)
            );
            let d1 = find(&w, &format!("{HS}/choice_score_usr/highscore_0000010_usr"));
            assert!(!d1.visible);
            assert!(!find(&w, &format!("{HS}/highscore_rank_usr")).visible);
            assert!(
                !find(
                    &w,
                    &format!("{HS}/fullcombo_mark_rotate_usr/fullcombo_mark_usr")
                )
                .visible
            );
            assert!(find(&w, &format!("{HS}/choice_dancer_name_usr")).visible);
        }
    }

    #[test]
    fn clear_without_full_combo_hides_only_the_mark() {
        let w = fill_side(&side(0, set(b"AB", rec(900_000, 2, 3)), None));
        assert!(find(&w, &format!("{HS}/highscore_rank_usr")).visible);
        assert!(
            !find(
                &w,
                &format!("{HS}/fullcombo_mark_rotate_usr/fullcombo_mark_usr")
            )
            .visible
        );
    }

    #[test]
    fn missing_fields_hide_only_themselves() {
        // Record lookup unavailable: score, rank, mark hidden; name, area,
        // difficulty stay.
        let w = fill_side(&side(0, set(b"AB", RecordRead::Unavailable), None));
        assert!(!find(&w, &format!("{HS}/choice_score_usr")).visible);
        assert!(absent(
            &w,
            &format!("{HS}/choice_score_usr/highscore_0000001_usr")
        ));
        assert!(!find(&w, &format!("{HS}/highscore_rank_usr")).visible);
        assert!(find(&w, &format!("{HS}/choice_dancer_name_usr")).visible);
        assert!(find(&w, &format!("{HS}/highscore_area_usr")).visible);
        assert!(find(&w, &format!("{HS}/highscore_difficulty_usr")).visible);

        // Name unreadable / area package missing / difficulty out of range.
        let mut s = set(b"AB", rec(5, 1, 7));
        s.name = None;
        s.area = None;
        s.difficulty = Some(9);
        let w = fill_side(&side(0, s, None));
        assert!(!find(&w, &format!("{HS}/choice_dancer_name_usr")).visible);
        assert!(!find(&w, &format!("{HS}/highscore_area_usr")).visible);
        assert!(!find(&w, &format!("{HS}/highscore_difficulty_usr")).visible);
        assert!(find(&w, &format!("{HS}/highscore_rank_usr")).visible);
        assert!(find(&w, &format!("{HS}/choice_score_usr")).visible);

        // Glyph package not ready: name and digits hidden, the root's own
        // textures still written.
        let mut sd = side(0, set(b"AB", rec(5, 1, 7)), None);
        sd.glyphs_ready = false;
        let w = fill_side(&sd);
        assert!(!find(&w, &format!("{HS}/choice_dancer_name_usr")).visible);
        assert!(!find(&w, &format!("{HS}/choice_score_usr")).visible);
        assert!(find(&w, &format!("{HS}/highscore_rank_usr")).visible);
        assert!(
            find(
                &w,
                &format!("{HS}/fullcombo_mark_rotate_usr/fullcombo_mark_usr")
            )
            .visible
        );
    }

    #[test]
    fn target_set() {
        let mut t = set(b"RIVAL", rec(1_000_000, 0, 10));
        t.difficulty = Some(3); // ignored: the target set has no difficulty child
        let w = fill_side(&side(1, set(b"ME", RecordRead::Absent), Some(t)));
        let tg = "p2_score_set_mc/p2_target_usr";
        assert!(find(&w, tg).visible);
        assert!(absent(&w, &format!("{tg}/highscore_difficulty_usr")));
        assert_eq!(
            find(
                &w,
                &format!("{tg}/choice_dancer_name_usr/highscore_name5_usr")
            )
            .texture
            .as_deref(),
            Some("playername_l")
        );
        assert!(find(&w, &format!("{tg}/choice_score_usr/highscore_1000000_usr")).visible);
        assert!(
            find(
                &w,
                &format!("{tg}/fullcombo_mark_rotate_usr/fullcombo_mark_usr")
            )
            .visible
        );
        // The high-score set of P2 is filled at its own paths.
        assert!(
            find(
                &w,
                "p2_score_set_mc/p2_highscore_usr/highscore_difficulty_usr"
            )
            .visible
        );
    }
}
