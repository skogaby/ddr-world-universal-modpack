//! 1st-5th option forcing — the pure half (host-tested).
//!
//! Dependency-free (mounted by `scripts/validate_ddr_selection.sh`).
//!
//! On the 1st-5th skin A3 played every song with the classic options:
//! speed ×1.00, no boost, visible arrows, step zone on, normal scroll, FLAT
//! colour, CLASSIC arrows, no lane filter, no guideline. A3 did it in its
//! `CourseOption` getters; World has no such getters, so the engine half
//! writes the eleven World `ddr::player::Option` fields for the song and
//! puts the player's values back afterwards. **World's enum orders are not
//! A3's** — [`FIELDS`] holds the World values (name tables verified on all
//! five builds). RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! option-forcing.md`.

/// One forced `ddr::player::Option` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// Short name (log lines, derivation value names).
    pub name: &'static str,
    /// The `/data/option` save node (NUL-terminated; every one is an s32).
    pub node: &'static [u8],
    /// The field's getter vslot on the RTTI `ddr::player::Option` vtable.
    pub getter_slot: usize,
    /// The field offset (checked against the getter stub at derivation).
    pub offset: usize,
    /// The value forced on 1st-5th songs.
    pub forced: i32,
    /// Plausible stored values (inclusive) — outside ⇒ the chain is wrong.
    pub range: (i32, i32),
}

/// Speed type `speed_rate` (a fixed multiplier; `real_speed` = 0 would be
/// re-derived from the chart by the game and by `song_rate::real_speed`).
pub const SPEED_TYPE_RATE: i32 = 1;

/// The eleven fields, in the order the snapshots store them.
pub const FIELDS: [Field; 11] = [
    Field {
        name: "speed_type",
        node: b"speed_type\0",
        getter_slot: 0x208,
        offset: 0x08,
        forced: SPEED_TYPE_RATE,
        range: (0, 1),
    },
    Field {
        name: "hispeed",
        node: b"hispeed\0",
        getter_slot: 0x220,
        offset: 0x0C,
        forced: 100,
        range: (25, 800),
    },
    Field {
        name: "scroll_moving",
        node: b"scroll_moving\0",
        getter_slot: 0x2A8,
        offset: 0x54,
        forced: 0, // normal (boost / brake / wave off)
        range: (0, 3),
    },
    Field {
        name: "visibility",
        node: b"visibility\0",
        getter_slot: 0x250,
        offset: 0x28,
        forced: 0, // normal (constant / stealth off)
        range: (0, 2),
    },
    Field {
        name: "lane_cover",
        node: b"lane\0",
        getter_slot: 0x268,
        offset: 0x34,
        forced: 0, // off (hidden / sudden / hidden_sudden)
        range: (0, 3),
    },
    Field {
        name: "stepzone",
        node: b"stepzone\0",
        getter_slot: 0x280,
        offset: 0x40,
        forced: 0, // on
        range: (0, 1),
    },
    Field {
        name: "scroll_direction",
        node: b"scroll_direction\0",
        getter_slot: 0x238,
        offset: 0x1C,
        forced: 0, // normal
        range: (0, 1),
    },
    Field {
        name: "arrow_color",
        node: b"arrow_color\0",
        getter_slot: 0x2B8,
        offset: 0x5C,
        forced: 3, // {note, rainbow, vivid, flat} → FLAT
        range: (0, 3),
    },
    Field {
        name: "arrow_design",
        node: b"arrow_design\0",
        getter_slot: 0x2C0,
        offset: 0x60,
        forced: 2, // {normal, x, classic, cyber, medium, small, dot} → classic
        range: (0, 6),
    },
    Field {
        name: "lane_filter",
        node: b"lane_filter\0",
        getter_slot: 0x260,
        offset: 0x30,
        forced: 100, // transparency 100 = no filter (menu: 100 − darkness)
        range: (0, 100),
    },
    Field {
        name: "guideline",
        node: b"guideline\0",
        getter_slot: 0x278,
        offset: 0x3C,
        forced: 2, // {center, border, off} → off
        range: (0, 2),
    },
];

pub const COUNT: usize = FIELDS.len();

/// The scenes the forced values must cover (0-indexed): the song-to-stage
/// interstitial, the stage loader and gameplay. The first scene after
/// gameplay precedes the per-stage save marshal by a whole loader scene.
pub const WINDOW: [i32; 3] = [26, 27, 28];

/// A3's forcing belongs to this skin (1st-5th) only.
pub const FORCED_SKIN: u8 = 1;

/// Whether the forced values must be in place in `scene` for `skin`.
pub fn in_window(scene: i32, skin: u8) -> bool {
    skin == FORCED_SKIN && WINDOW.contains(&scene)
}

/// Values of the eleven fields, [`FIELDS`] order.
pub type Values = [i32; COUNT];

/// The forced values.
pub fn forced_values() -> Values {
    let mut v = [0; COUNT];
    for (slot, f) in v.iter_mut().zip(FIELDS.iter()) {
        *slot = f.forced;
    }
    v
}

/// The first field whose stored value is outside its World enum range —
/// a stored Option that fails this was not read from a real Option.
pub fn implausible(values: &Values) -> Option<(&'static str, i32)> {
    FIELDS
        .iter()
        .zip(values.iter())
        .find(|(f, v)| !(f.range.0..=f.range.1).contains(*v))
        .map(|(f, v)| (f.name, *v))
}

/// How many of `values` differ from the forced values.
pub fn changed_count(values: &Values) -> usize {
    FIELDS
        .iter()
        .zip(values.iter())
        .filter(|(f, v)| f.forced != **v)
        .count()
}

/// What to do with one side at a scene change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Read and keep the player's values, then write the forced ones.
    SnapshotAndForce,
    /// Write the forced values again (already snapshotted).
    Reassert,
    /// Write the snapshot back and drop it.
    Restore,
}

/// The per-side decision. `window` = [`in_window`] for the new scene;
/// `entered` = the side has a player (the bot counts); `snapshotted` = we
/// hold the side's own values.
pub fn action(window: bool, entered: bool, snapshotted: bool) -> Action {
    match (window, snapshotted) {
        (true, true) => Action::Reassert,
        (true, false) if entered => Action::SnapshotAndForce,
        (true, false) => Action::Nothing,
        (false, true) => Action::Restore,
        (false, false) => Action::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forced_values_are_world_enums_not_a3() {
        let by = |n: &str| FIELDS.iter().find(|f| f.name == n).unwrap().forced;
        // A3: colour FLAT = 2 in {vivid, note, flat, rainbow}; World puts it
        // at 3 in {note, rainbow, vivid, flat}.
        assert_eq!(by("arrow_color"), 3);
        // A3 guideline off = 0; World {center, border, off} → 2.
        assert_eq!(by("guideline"), 2);
        // A3 filter off = 0; World stores the transparency (100 = none).
        assert_eq!(by("lane_filter"), 100);
        assert_eq!(by("arrow_design"), 2);
        assert_eq!(by("speed_type"), SPEED_TYPE_RATE);
        assert_eq!(by("hispeed"), 100);
        for n in [
            "scroll_moving",
            "visibility",
            "lane_cover",
            "stepzone",
            "scroll_direction",
        ] {
            assert_eq!(by(n), 0, "{}", n);
        }
    }

    #[test]
    fn table_is_consistent() {
        for (i, f) in FIELDS.iter().enumerate() {
            assert!((f.range.0..=f.range.1).contains(&f.forced), "{}", f.name);
            assert_eq!(f.node.last(), Some(&0), "{}", f.name);
            assert!(f.offset % 4 == 0 && f.offset < 0x80, "{}", f.name);
            assert!(f.getter_slot % 8 == 0, "{}", f.name);
            for g in &FIELDS[i + 1..] {
                assert_ne!(f.offset, g.offset);
                assert_ne!(f.getter_slot, g.getter_slot);
                assert_ne!(f.node, g.node);
                assert_ne!(f.name, g.name);
            }
        }
        assert_eq!(implausible(&forced_values()), None);
        assert_eq!(changed_count(&forced_values()), 0);
    }

    #[test]
    fn plausibility() {
        let mut v = forced_values();
        v[1] = 20; // hispeed below 25
        assert_eq!(implausible(&v), Some(("hispeed", 20)));
        let mut v = forced_values();
        v[9] = 101;
        assert_eq!(implausible(&v), Some(("lane_filter", 101)));
        // A typical player: real speed, stealth, rainbow, filter 70.
        let player = [0, 350, 0, 2, 0, 0, 0, 1, 0, 70, 0];
        assert_eq!(implausible(&player), None);
        assert_eq!(changed_count(&player), 7);
    }

    #[test]
    fn window_is_skin_one_play_scenes() {
        for s in 26..=28 {
            assert!(in_window(s, 1));
            assert!(!in_window(s, 2));
            assert!(!in_window(s, 0));
        }
        for s in [24, 25, 29, 30, 31, 34] {
            assert!(!in_window(s, 1), "{}", s);
        }
    }

    #[test]
    fn lifecycle() {
        use Action::*;
        // Arm edge: entered side snapshotted; empty side left alone.
        assert_eq!(action(true, true, false), SnapshotAndForce);
        assert_eq!(action(true, false, false), Nothing);
        // 26 → 27 → 28 and quick restart: re-assert.
        assert_eq!(action(true, true, true), Reassert);
        // A bot side that left mid-window keeps its forced values until the
        // window ends (the snapshot is restored then).
        assert_eq!(action(true, false, true), Reassert);
        // Leaving the window: restore.
        assert_eq!(action(false, true, true), Restore);
        assert_eq!(action(false, false, true), Restore);
        assert_eq!(action(false, true, false), Nothing);
    }

    #[test]
    fn restart_through_the_results_loader_reforces() {
        // 28 → 29 (restore) → 28 (restart redirect): a fresh snapshot of the
        // restored values, then forced again.
        let mut snap = true;
        for (scene, want) in [
            (28, Action::Reassert),
            (29, Action::Restore),
            (28, Action::SnapshotAndForce),
            (30, Action::Restore),
        ] {
            let a = action(in_window(scene, 1), true, snap);
            assert_eq!(a, want, "scene {}", scene);
            snap = matches!(a, Action::SnapshotAndForce | Action::Reassert);
        }
    }
}
