//! DDR SELECTION legacy score / difficulty display (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! A port of A3's `sequence::dance::ScoreActor` (`gamemdx_20240402`): the
//! export names of its init `FUN_180055390`, the digit refresh
//! `FUN_180055be0` (the same smoothing and place walk as World's
//! `FUN_180077eb0`, A3's texture names) and the difficulty message
//! `FUN_180056080` (labels on `difficulty_level_usr` /
//! `difficulty_level_base_usr`, the level texture).
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/legacy-score.md`.

/// A3's score export (World: `dance_score`).
pub const SCORE_EXPORT: &str = "frame_score";
/// World's name clip has no legacy export (the legacy UIs showed no player
/// name): World's init is pointed at this small export instead — it exists
/// in every `dance_score000N` and has no `name_usr`, so World's name text is
/// never built; the clip is hidden after the init.
pub const NAME_STAND_IN: &str = "difficulty_level_base";
/// A3's difficulty-frame layer priority on skin 2 (World and the other skins: 7).
pub const SKIN2_DIFFICULTY_PRIORITY: u8 = 3;
pub const PRIORITY: u8 = 7;

/// The seven digit places, ones first (World's and A3's own child names —
/// `0010001_usr` is the authored name of the ten-thousands place).
pub const PLACES: [&str; 7] = [
    "0000001_usr",
    "0000010_usr",
    "0000100_usr",
    "0001000_usr",
    "0010001_usr",
    "0100000_usr",
    "1000000_usr",
];

/// The `%04d` of A3's texture names (0 for a theme — A3's skin-0 art).
fn tex(skin: u8) -> u8 {
    super::policy::tex_number(skin)
}

/// The difficulty names by World's difficulty index (A3's table).
pub const DIFFICULTIES: [&str; 6] = [
    "beginner",
    "basic",
    "difficult",
    "expert",
    "challenge",
    "basic",
];

/// A3's difficulty frame export: `frame_difficulty_<side+1>p[_reverse]`.
pub fn difficulty_export(side: i32, reverse: bool) -> String {
    format!(
        "frame_difficulty_{}p{}",
        side + 1,
        if reverse { "_reverse" } else { "" }
    )
}

/// The difficulty-frame priority World's init should pass.
pub fn difficulty_priority(skin: u8) -> u8 {
    if skin == 2 {
        SKIN2_DIFFICULTY_PRIORITY
    } else {
        PRIORITY
    }
}

/// The difficulty name for World's index (clamped like a table lookup would
/// need to be — A3 indexed unchecked).
pub fn difficulty_name(index: i32) -> &'static str {
    DIFFICULTIES[index.clamp(0, DIFFICULTIES.len() as i32 - 1) as usize]
}

/// A3's difficulty message: the label for `difficulty_level_usr` (skin 2:
/// `<diff><side+1>`; others `<diff><1 | 2 when level ≥ 10>`), the level
/// texture for `difficulty_level_usr/level_tex` (not skin 2), and the label
/// for `difficulty_level_base_usr` (skin 2 only: `<diff>_in`).
#[derive(Clone, Debug, PartialEq)]
pub struct DifficultyWrites {
    pub level_label: String,
    pub level_texture: Option<String>,
    pub base_label: Option<String>,
}

pub fn difficulty_writes(skin: u8, side: i32, difficulty: i32, level: i32) -> DifficultyWrites {
    let d = difficulty_name(difficulty);
    if skin == 2 {
        DifficultyWrites {
            level_label: format!("{}{}", d, side + 1),
            level_texture: None,
            base_label: Some(format!("{}_in", d)),
        }
    } else {
        DifficultyWrites {
            level_label: format!("{}{}", d, if level > 9 { 2 } else { 1 }),
            level_texture: Some(format!("dance_score{:04}_lv{:02}", tex(skin), level)),
            base_label: None,
        }
    }
}

/// World's / A3's displayed-score smoothing: halfway towards the target each
/// refresh, never past it (C integer division).
pub fn smooth(displayed: i32, target: i32) -> i32 {
    let v = (target.wrapping_add(1).wrapping_add(displayed)) / 2;
    if target < v {
        target
    } else {
        v
    }
}

/// One place (or comma) write: texture and visibility.
#[derive(Clone, Debug, PartialEq)]
pub struct Write {
    pub path: String,
    pub texture: String,
    pub visible: bool,
}

/// A3's digit refresh from `old` (the displayed value before this refresh;
/// negative = repaint everything — `song_reset`'s sentinel and the ctor's
/// `-1`) to `new`: every place whose digit changed (or all, forced) gets its
/// digit texture, or the leading-zero texture once the value is exhausted;
/// leading zeros are hidden in EX mode; commas follow places 3 (`comma2`) and
/// 6 (`comma1`).
pub fn digit_writes(skin: u8, old: i32, new: i32, ex: bool) -> Vec<Write> {
    let force = old < 0;
    let mut o = old;
    let mut v = new;
    let mut lead = false;
    let mut out = Vec::new();
    for (i, path) in PLACES.iter().enumerate() {
        let changed = o % 10 != v % 10 || force;
        o /= 10;
        if changed {
            let texture = if lead {
                format!("dance_score{:04}_score_num_0_gray", tex(skin))
            } else {
                format!("dance_score{:04}_score_num_{}", tex(skin), v % 10)
            };
            let visible = !ex || !lead;
            out.push(Write {
                path: (*path).to_string(),
                texture,
                visible,
            });
            if i == 3 || i == 6 {
                out.push(Write {
                    path: format!("comma{}_usr", if i == 3 { 2 } else { 1 }),
                    texture: format!(
                        "dance_score{:04}_score_comma{}",
                        tex(skin),
                        if lead { "_gray" } else { "" }
                    ),
                    visible,
                });
            }
        }
        if v / 10 == 0 {
            lead = true;
        }
        v /= 10;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names() {
        assert_eq!(difficulty_export(0, false), "frame_difficulty_1p");
        assert_eq!(difficulty_export(1, true), "frame_difficulty_2p_reverse");
        assert_eq!(difficulty_priority(2), 3);
        assert_eq!(difficulty_priority(4), 7);
    }

    #[test]
    fn difficulty_labels() {
        let w = difficulty_writes(1, 0, 3, 12);
        assert_eq!(w.level_label, "expert2");
        assert_eq!(w.level_texture.as_deref(), Some("dance_score0001_lv12"));
        assert_eq!(w.base_label, None);
        let w = difficulty_writes(4, 1, 0, 3);
        assert_eq!(w.level_label, "beginner1");
        assert_eq!(w.level_texture.as_deref(), Some("dance_score0004_lv03"));
        let w = difficulty_writes(2, 1, 4, 15);
        assert_eq!(w.level_label, "challenge2");
        assert_eq!(w.level_texture, None);
        assert_eq!(w.base_label.as_deref(), Some("challenge_in"));
        assert_eq!(difficulty_name(9), "basic");
        assert_eq!(difficulty_name(-1), "beginner");
    }

    #[test]
    fn smoothing() {
        assert_eq!(smooth(0, 100), 50);
        assert_eq!(smooth(50, 100), 75);
        assert_eq!(smooth(99, 100), 100);
        assert_eq!(smooth(100, 100), 100);
        assert_eq!(smooth(-1, 1_000_000), 500_000);
        assert_eq!(smooth(-1, 0), 0);
        // never past the target (a lowered target snaps down)
        assert_eq!(smooth(900, 100), 100);
    }

    #[test]
    fn forced_repaint_of_zero() {
        let w = digit_writes(3, -1, 0, false);
        assert_eq!(w.len(), 9);
        assert_eq!(w[0].texture, "dance_score0003_score_num_0");
        assert!(w[1..].iter().all(|x| x.texture.ends_with("_gray")));
        assert_eq!(w[4].path, "comma2_usr");
        assert_eq!(w[4].texture, "dance_score0003_score_comma_gray");
        assert_eq!(w[8].path, "comma1_usr");
        assert!(w.iter().all(|x| x.visible));
        // EX: leading places hidden, the ones place shown
        let w = digit_writes(3, -1, 0, true);
        assert!(w[0].visible);
        assert!(w[1..].iter().all(|x| !x.visible));
    }

    #[test]
    fn only_changed_places() {
        // 1234 -> 1235: only the ones place
        let w = digit_writes(1, 1234, 1235, false);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].path, "0000001_usr");
        assert_eq!(w[0].texture, "dance_score0001_score_num_5");
        // 999 -> 1000: four places + comma2 (place 3 turns 1)
        let w = digit_writes(5, 999, 1000, false);
        let paths: Vec<&str> = w.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "0000001_usr",
                "0000010_usr",
                "0000100_usr",
                "0001000_usr",
                "comma2_usr"
            ]
        );
        assert_eq!(w[3].texture, "dance_score0005_score_num_1");
        assert_eq!(w[4].texture, "dance_score0005_score_comma");
    }

    #[test]
    fn full_value() {
        let w = digit_writes(4, -1, 1_000_000, false);
        let tex: Vec<&str> = w.iter().map(|x| x.texture.as_str()).collect();
        assert_eq!(tex[0], "dance_score0004_score_num_0");
        assert_eq!(tex.last().copied(), Some("dance_score0004_score_comma"));
        assert_eq!(w[w.len() - 2].texture, "dance_score0004_score_num_1");
    }

    #[test]
    fn themes_use_a3s_skin_0_textures() {
        for skin in 6..=8 {
            assert_eq!(difficulty_priority(skin), PRIORITY);
            let w = difficulty_writes(skin, 1, 3, 14);
            assert_eq!(w.level_label, "expert2");
            assert_eq!(w.level_texture.as_deref(), Some("dance_score0000_lv14"));
            assert_eq!(w.base_label, None);
            let w = digit_writes(skin, -1, 1_234, false);
            assert_eq!(w[0].texture, "dance_score0000_score_num_4");
            assert_eq!(w[4].path, "comma2_usr");
            assert_eq!(w[4].texture, "dance_score0000_score_comma");
            assert!(w
                .iter()
                .any(|x| x.texture == "dance_score0000_score_num_0_gray"));
            assert!(w
                .iter()
                .any(|x| x.texture == "dance_score0000_score_comma_gray"));
        }
    }
}
