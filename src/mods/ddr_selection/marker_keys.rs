//! DDR SELECTION legacy marker table (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! World's gameplay `LayoutActor` fills its marker maps from World's layout
//! root (`dance_common_v3` / `dance_root`); HUD actors position themselves
//! from those keys. A3 read the same keys from the skin's own root
//! (`dance_common000N`, `dance_common0000_v2` for skin 1) with partly
//! different marker names. `markers.rs` runs after World's builder and
//! overwrites the A3-defined keys from the legacy root; this table says which
//! key comes from which A3 marker, in which map, and when a key may move.
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! hud-layout-stage-frame.md` §1–§3, §6–§7, §9.

/// Which marker map a key lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Map {
    /// `LayoutActor + 0x98` (one per actor).
    Shared,
    /// `LayoutActor + 0xE0 + side*0x48`.
    Side,
}

/// Where the A3 marker is read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A root marker; `{n}` = side + 1, `{r}` = `"_reverse"` when the side
    /// scrolls reverse (A3's `difficuty_normal_%dp%s_usr`).
    Root(&'static str),
    /// The side's lane MC itself (`1p_lane_usr` / `double_lane_usr`),
    /// before any lane content is loaded.
    Lane,
    /// A lane child after World's judge-group lane load
    /// (`lane_*_{normal,reverse}` by `judge_position XOR reverse`).
    JudgeGroup(&'static str),
    /// A lane child after the arrow-group lane load (by `reverse`).
    ArrowGroup(&'static str),
}

/// When a key may move to its A3 position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Every legacy song (the element is identical in every root, or World
    /// art sits where A3 drew the same element).
    Always,
    /// Only when this World package is the legacy one for the song — so World
    /// art never moves to an A3 position before its own adapter lands.
    Package(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpec {
    /// The World marker key the HUD actor reads.
    pub key: &'static str,
    pub map: Map,
    pub source: Source,
    pub gate: Gate,
}

const fn k(key: &'static str, map: Map, source: Source, gate: Gate) -> KeySpec {
    KeySpec {
        key,
        map,
        source,
        gate,
    }
}

/// Every key the post-pass may overwrite, in World's builder order (the
/// lane groups are read in this order after their lane load). World-only
/// keys (`dance_matching`, `bpm`, `name`, `option_icon`) are absent: they stay World's (`bpm` / `name` are
/// hidden instead — [`HIDDEN_KEYS`]).
pub const KEYS: &[KeySpec] = &[
    k(
        "stage",
        Map::Shared,
        Source::Root("stage_frame_usr"),
        Gate::Package("dance_stage"),
    ),
    k(
        "song_info",
        Map::Shared,
        Source::Root("song_info_usr"),
        Gate::Package("dance_song_info"),
    ),
    k(
        "score",
        Map::Side,
        Source::Root("score_{n}p_usr"),
        Gate::Package("dance_score"),
    ),
    k(
        "difficulty",
        Map::Side,
        Source::Root("difficuty_normal_{n}p{r}_usr"),
        Gate::Package("dance_score"),
    ),
    k(
        "gauge",
        Map::Side,
        Source::Root("gauge_{n}p_usr"),
        Gate::Package("dance_gauge"),
    ),
    k(
        "danger_gauge",
        Map::Side,
        Source::Root("danger_gauge_{n}p_usr"),
        Gate::Package("dance_danger"),
    ),
    k(
        "gameover",
        Map::Side,
        Source::Root("{n}p_gameover_usr"),
        Gate::Package("dance_game_over"),
    ),
    // A3's option-icon row (World's `option` key; World's own icons read
    // `option_icon`, which stays World's).
    k(
        "option",
        Map::Side,
        Source::Root("option_icon_{n}p{r}_usr"),
        Gate::Package("dance_option"),
    ),
    k(
        "fullcombo",
        Map::Side,
        Source::Lane,
        Gate::Package("dance_fullcombo"),
    ),
    k(
        "judge",
        Map::Side,
        Source::JudgeGroup("judge_usr"),
        Gate::Package("dance_judge"),
    ),
    k(
        "combo",
        Map::Side,
        Source::JudgeGroup("combo_set_usr/combo_usr"),
        Gate::Package("dance_combo"),
    ),
    k(
        "fast_slow",
        Map::Side,
        Source::JudgeGroup("combo_set_usr/fast_slow_usr"),
        Gate::Package("dance_fast_slow"),
    ),
    k(
        "filter",
        Map::Side,
        Source::JudgeGroup("filter_usr"),
        Gate::Always,
    ),
    k(
        "score_compare",
        Map::Side,
        Source::JudgeGroup("score_compare_usr"),
        Gate::Always,
    ),
    // `arrow_raw` is the raw coord; `arrow` is derived from it (see
    // [`arrow_coord`]) — one read, two writes.
    k(
        "arrow_raw",
        Map::Side,
        Source::ArrowGroup("arrow_usr"),
        Gate::Always,
    ),
    k(
        "freeze_judge",
        Map::Side,
        Source::ArrowGroup("freeze_judge_usr"),
        Gate::Always,
    ),
];

/// The key World derives from `arrow_raw` (written right after it).
pub const ARROW_KEY: &str = "arrow";

/// World-only per-side elements A3's legacy screens do not have: moved off
/// screen on legacy songs (the actors position them once, at init).
pub const HIDDEN_KEYS: &[&str] = &["bpm", "name"];

/// The coord a hidden element is parked at (x, y, w, h, sx, sy — sx / sy
/// are raw f32 bits of 1.0).
pub const HIDDEN_COORD: [i32; 6] = [-4096, -4096, 0, 0, 0x3F80_0000, 0x3F80_0000];

/// World keys the post-pass must never write from a legacy root (the design's
/// World-only set; the DPS `movie_*_usr` placements are read from World's
/// loader-owned root directly, not through the maps).
pub const WORLD_ONLY_KEYS: &[&str] = &["dance_matching", "bpm", "name", "option_icon"];

/// The legacy layout root for a skin (A3 had no `dance_common0001`: skin 1
/// used A3's own `dance_common0000_v2`, which World ships and which World's
/// probe reaches only through its bare rung — hence the explicit suffix).
pub fn root_name(skin: u8) -> Option<&'static str> {
    match skin {
        1 => Some("dance_common0000_v2"),
        2 => Some("dance_common0002"),
        3 => Some("dance_common0003"),
        4 => Some("dance_common0004"),
        5 => Some("dance_common0005"),
        _ => None,
    }
}

/// The side's lane MC (`style` from the builder: 0 single, else double).
pub fn lane_name(side: u8, double: bool) -> String {
    if double {
        "double_lane_usr".to_string()
    } else {
        format!("{}p_lane_usr", side + 1)
    }
}

/// The root export World loads into the lane before reading its children.
pub fn lane_variant(double: bool, reverse: bool) -> String {
    format!(
        "lane_{}_{}",
        if double { "double" } else { "single" },
        if reverse { "reverse" } else { "normal" }
    )
}

/// World's judge-group lane choice: `(judge_position == 1) XOR reverse`.
pub fn judge_group_reverse(judge_position: i32, reverse: bool) -> bool {
    (judge_position == 1) != reverse
}

/// The A3 marker path for `source` (None for [`Source::Lane`] — the lane
/// name itself, see [`lane_name`]).
pub fn marker_path(source: Source, side: u8, double: bool, reverse: bool) -> String {
    let fill = |s: &str| {
        s.replace("{n}", &(side + 1).to_string())
            .replace("{r}", if reverse { "_reverse" } else { "" })
    };
    match source {
        Source::Root(s) => fill(s),
        Source::Lane => lane_name(side, double),
        Source::JudgeGroup(c) | Source::ArrowGroup(c) => {
            format!("{}/{}", lane_name(side, double), c)
        }
    }
}

/// World's coord from the four MovieClip reads: position `+ 0.5` then
/// truncated (`cvttss2si`), size truncated, scale kept as raw f32 bits.
pub fn coord_from(pos: (f32, f32), w: i32, h: i32, scale: (f32, f32)) -> [i32; 6] {
    [
        (pos.0 + 0.5) as i32,
        (pos.1 + 0.5) as i32,
        w,
        h,
        scale.0.to_bits() as i32,
        scale.1.to_bits() as i32,
    ]
}

/// World's `arrow` from `arrow_raw`: `x − w/2`, `y − h/2` (reverse
/// `y + h/2`), C division toward zero; the rest unchanged.
pub fn arrow_coord(raw: [i32; 6], reverse: bool) -> [i32; 6] {
    let mut c = raw;
    c[0] = raw[0] - raw[2] / 2;
    c[1] = if reverse {
        raw[1] + raw[3] / 2
    } else {
        raw[1] - raw[3] / 2
    };
    c
}

/// Whether a key's gate lets it move, given which World bases are legacy.
pub fn gate_open(gate: Gate, legacy: impl Fn(&str) -> bool) -> bool {
    match gate {
        Gate::Always => true,
        Gate::Package(base) => legacy(base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str) -> KeySpec {
        *KEYS.iter().find(|s| s.key == key).expect(key)
    }

    #[test]
    fn combo_and_fast_slow_are_nested_under_combo_set() {
        assert_eq!(
            marker_path(spec("combo").source, 0, false, false),
            "1p_lane_usr/combo_set_usr/combo_usr"
        );
        assert_eq!(
            marker_path(spec("fast_slow").source, 1, false, true),
            "2p_lane_usr/combo_set_usr/fast_slow_usr"
        );
        assert_eq!(
            marker_path(spec("judge").source, 0, true, false),
            "double_lane_usr/judge_usr"
        );
    }

    #[test]
    fn reverse_difficulty_uses_the_a3_suffix() {
        let s = spec("difficulty").source;
        assert_eq!(marker_path(s, 0, false, false), "difficuty_normal_1p_usr");
        assert_eq!(
            marker_path(s, 1, false, true),
            "difficuty_normal_2p_reverse_usr"
        );
        // score / gauge have no reverse variant
        assert_eq!(
            marker_path(spec("score").source, 1, false, true),
            "score_2p_usr"
        );
        assert_eq!(
            marker_path(spec("gameover").source, 0, false, true),
            "1p_gameover_usr"
        );
    }

    #[test]
    fn world_only_keys_are_never_written_from_the_root() {
        for s in KEYS {
            assert!(!WORLD_ONLY_KEYS.contains(&s.key), "{}", s.key);
        }
        assert!(!KEYS.iter().any(|s| s.key == ARROW_KEY));
        for h in HIDDEN_KEYS {
            assert!(WORLD_ONLY_KEYS.contains(h));
        }
    }

    #[test]
    fn keys_are_unique_and_the_shared_ones_are_shared() {
        for (i, a) in KEYS.iter().enumerate() {
            for b in &KEYS[i + 1..] {
                assert_ne!(a.key, b.key);
            }
        }
        assert_eq!(spec("stage").map, Map::Shared);
        assert_eq!(spec("song_info").map, Map::Shared);
        assert!(KEYS
            .iter()
            .filter(|s| s.map == Map::Shared)
            .all(|s| matches!(s.source, Source::Root(_))));
    }

    #[test]
    fn keys_whose_art_is_world_until_later_steps_are_package_gated() {
        for (key, base) in [
            ("score", "dance_score"),
            ("difficulty", "dance_score"),
            ("gauge", "dance_gauge"),
            ("combo", "dance_combo"),
            ("song_info", "dance_song_info"),
            ("stage", "dance_stage"),
            ("danger_gauge", "dance_danger"),
        ] {
            assert_eq!(spec(key).gate, Gate::Package(base), "{}", key);
        }
        let only_judge = |b: &str| b == "dance_judge";
        assert!(gate_open(spec("judge").gate, only_judge));
        assert!(!gate_open(spec("combo").gate, only_judge));
        assert!(gate_open(spec("filter").gate, only_judge));
    }

    #[test]
    fn lane_groups_follow_world() {
        assert_eq!(lane_variant(false, false), "lane_single_normal");
        assert_eq!(lane_variant(true, true), "lane_double_reverse");
        assert!(!judge_group_reverse(0, false));
        assert!(judge_group_reverse(0, true));
        assert!(judge_group_reverse(1, false));
        assert!(!judge_group_reverse(1, true));
        assert!(judge_group_reverse(2, true));
        assert_eq!(lane_name(1, false), "2p_lane_usr");
        assert_eq!(marker_path(Source::Lane, 1, true, false), "double_lane_usr");
    }

    #[test]
    fn coord_rounds_like_world() {
        let c = coord_from((281.0, 241.5), 40, 30, (1.0, 0.5));
        assert_eq!(&c[..4], &[281, 242, 40, 30]);
        assert_eq!(f32::from_bits(c[4] as u32), 1.0);
        assert_eq!(f32::from_bits(c[5] as u32), 0.5);
        assert_eq!(
            coord_from((136.4, 116.6), 0, 0, (1.0, 1.0))[..2],
            [136, 117]
        );
    }

    #[test]
    fn arrow_matches_world_math() {
        let raw = [136, 117, 291, 65, 7, 8];
        assert_eq!(
            arrow_coord(raw, false),
            [136 - 145, 117 - 32, 291, 65, 7, 8]
        );
        assert_eq!(arrow_coord(raw, true), [136 - 145, 117 + 32, 291, 65, 7, 8]);
        // division toward zero on negatives
        assert_eq!(arrow_coord([0, 0, -3, -3, 0, 0], false)[..2], [1, 1]);
    }

    #[test]
    fn root_names_never_use_a_bare_0000() {
        assert_eq!(root_name(1), Some("dance_common0000_v2"));
        for s in 2..=5u8 {
            assert_eq!(root_name(s).unwrap(), format!("dance_common{:04}", s));
        }
        assert_eq!(root_name(0), None);
        assert_eq!(root_name(6), None);
        for s in 1..=5u8 {
            assert!(!root_name(s).unwrap().ends_with("0000"));
        }
    }

    #[test]
    fn hidden_coord_is_off_screen_with_unit_scale() {
        assert!(HIDDEN_COORD[0] < -1000 && HIDDEN_COORD[1] < -1000);
        assert_eq!(f32::from_bits(HIDDEN_COORD[4] as u32), 1.0);
        assert_eq!(f32::from_bits(HIDDEN_COORD[5] as u32), 1.0);
    }
}
