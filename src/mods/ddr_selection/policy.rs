//! DDR SELECTION package policy — which World gameplay package is replaced
//! by which legacy skin package (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! World's `LayoutActor::onInitialize` hands every gameplay package name to a
//! per-package helper. A3's helper appended the skin id (`dance_judge` →
//! `dance_judge0001`) and World's never does; `package_helper.rs` restores the
//! append for exactly the packages this table allows. Two rules shape it:
//!
//! * **A package may only turn legacy once its World consumer is adapted.**
//!   Every World HUD actor NULL-derefs when the package it is handed lacks the
//!   export it asks for, so each entry names the [`Adapter`] it needs; an
//!   entry whose adapter is not available on this boot stays stock.
//! * **The fallback is the unsuffixed World base, never `<base>0000`.** World's
//!   arc probe (`_v3`, `_v0`, `_lite`, bare) resolves `…0000` to early-World or
//!   A3-oldest arcs whose export names do not match World's actors.

/// The legacy skins: 1 = 1st-5th, 2 = MAX-EXTREME, 3 = SuperNOVA, 4 = X,
/// 5 = 2013-A. 0 = World's own UI.
pub const SKIN_MAX: u8 = 5;

/// Display name of a skin (logs, option labels). `None` for 0 / out of range.
pub fn skin_name(skin: u8) -> Option<&'static str> {
    match skin {
        1 => Some("1st-5th"),
        2 => Some("MAX-EXTREME"),
        3 => Some("SuperNOVA"),
        4 => Some("X"),
        5 => Some("2013-A"),
        _ => None,
    }
}

/// The DLL component a legacy package's World consumer needs before the
/// package may be swapped. Bit positions in [`AdapterSet`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// World's consumer already matches the legacy package (same export and
    /// label names) — a whole-package swap.
    None = 0,
    /// Legacy element positions post-pass (danger at the `danger_gauge` marker).
    Markers = 1,
    /// Legacy life gauge (export alias, P2 mirror, segmented fill).
    Gauge = 2,
    /// A3 ComboActor behaviour.
    Combo = 3,
    /// A3 ScoreActor behaviour.
    Score = 4,
    /// A3 StageFrameActor names.
    StageFrame = 5,
    /// Legacy song-info panel.
    SongInfo = 6,
    /// Re-implemented A3 ReadyGoActor (the only `dance_message` consumer).
    ReadyGo = 7,
}

/// Which adapters resolved on this boot. `Adapter::None` is always present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AdapterSet(u32);

impl AdapterSet {
    pub const fn none() -> Self {
        AdapterSet(0)
    }
    pub const fn with(self, a: Adapter) -> Self {
        AdapterSet(self.0 | (1 << a as u32))
    }
    pub const fn has(self, a: Adapter) -> bool {
        matches!(a, Adapter::None) || self.0 & (1 << a as u32) != 0
    }
}

/// One policy row: `base` (the name World's `LayoutActor` requests) turns into
/// `<arc_base>000N` for every skin N in `skins` whose adapter is available.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub base: &'static str,
    pub arc_base: &'static str,
    /// Bit N set ⇔ skin N (1..=5) uses this row.
    pub skins: u8,
    pub adapter: Adapter,
}

const fn skins(list: &[u8]) -> u8 {
    let mut m = 0u8;
    let mut i = 0;
    while i < list.len() {
        m |= 1 << list[i];
        i += 1;
    }
    m
}

const ALL: u8 = skins(&[1, 2, 3, 4, 5]);

/// The full policy (every phase). A base may appear in several rows with
/// disjoint skin sets (danger: skins 1–2 need nothing, 3–5 need markers).
pub const TABLE: &[Entry] = &[
    // Whole-package swaps (World's consumer matches the legacy exports).
    Entry {
        base: "dance_judge",
        arc_base: "dance_judge",
        skins: ALL,
        adapter: Adapter::None,
    },
    Entry {
        base: "dance_fast_slow",
        arc_base: "dance_fast_slow",
        skins: ALL,
        adapter: Adapter::None,
    },
    Entry {
        base: "dance_fullcombo",
        arc_base: "dance_fullcombo",
        skins: ALL,
        adapter: Adapter::None,
    },
    Entry {
        base: "dance_game_over",
        arc_base: "dance_game_over",
        skins: ALL,
        adapter: Adapter::None,
    },
    // World's DanceDangerActor still has A3's skin branches: skins 1–2 draw
    // centred (no marker needed), skins 3–5 at the `danger_gauge` marker,
    // which only the legacy layout roots carry.
    Entry {
        base: "dance_danger",
        arc_base: "dance_danger",
        skins: skins(&[1, 2]),
        adapter: Adapter::None,
    },
    Entry {
        base: "dance_danger",
        arc_base: "dance_danger",
        skins: skins(&[3, 4, 5]),
        adapter: Adapter::Markers,
    },
    // Consumers that need a DLL adapter first.
    Entry {
        base: "dance_gauge",
        arc_base: "dance_gauge",
        skins: ALL,
        adapter: Adapter::Gauge,
    },
    Entry {
        base: "dance_combo",
        arc_base: "dance_combo",
        skins: ALL,
        adapter: Adapter::Combo,
    },
    Entry {
        base: "dance_score",
        arc_base: "dance_score",
        skins: ALL,
        adapter: Adapter::Score,
    },
    Entry {
        base: "dance_stage",
        arc_base: "dance_stage_frame",
        skins: ALL,
        adapter: Adapter::StageFrame,
    },
    Entry {
        base: "dance_song_info",
        arc_base: "dance_song_info",
        skins: skins(&[2]),
        adapter: Adapter::SongInfo,
    },
    Entry {
        base: "dance_message",
        arc_base: "dance_message",
        skins: ALL,
        adapter: Adapter::ReadyGo,
    },
    // dance_common (the layout root) is deliberately absent: World's layout
    // builder needs World's root markers; the legacy positions are applied by
    // a post-pass that reads the legacy root itself. dance_effect / bpm /
    // filter / cover / option / score_compare have no legacy variants.
];

/// What the per-package helper does with one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// World's own behaviour (the original helper with skin 0).
    Stock,
    /// Register `<arc_base>000N` under `base` with record skin N.
    Legacy { arc_base: &'static str, skin: u8 },
}

/// Decide one package request. `skin` 0 or out of range ⇒ stock.
pub fn decide(base: &str, skin: u8, adapters: AdapterSet) -> Decision {
    if skin == 0 || skin > SKIN_MAX {
        return Decision::Stock;
    }
    for e in TABLE {
        if e.base == base && e.skins & (1 << skin) != 0 && adapters.has(e.adapter) {
            return Decision::Legacy {
                arc_base: e.arc_base,
                skin,
            };
        }
    }
    Decision::Stock
}

/// The package name the helper registers for a legacy decision
/// (`dance_judge` + 1 → `dance_judge0001`), NUL-terminated for the game.
pub fn legacy_name(arc_base: &str, skin: u8) -> String {
    format!("{}{:04}\0", arc_base, skin)
}

/// Stable index of a base in the "legacy this arm" bitmask (`None` for bases
/// the table never swaps).
pub fn package_index(base: &str) -> Option<u32> {
    const BASES: &[&str] = &[
        "dance_judge",
        "dance_fast_slow",
        "dance_fullcombo",
        "dance_game_over",
        "dance_danger",
        "dance_gauge",
        "dance_combo",
        "dance_score",
        "dance_stage",
        "dance_song_info",
        "dance_message",
    ];
    BASES.iter().position(|b| *b == base).map(|i| i as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P0: AdapterSet = AdapterSet::none();

    fn every_adapter() -> AdapterSet {
        [
            Adapter::Markers,
            Adapter::Gauge,
            Adapter::Combo,
            Adapter::Score,
            Adapter::StageFrame,
            Adapter::SongInfo,
            Adapter::ReadyGo,
        ]
        .iter()
        .fold(AdapterSet::none(), |s, a| s.with(*a))
    }

    #[test]
    fn skin_zero_and_out_of_range_are_stock() {
        for base in ["dance_judge", "dance_gauge", "dance_common", "nope"] {
            assert_eq!(decide(base, 0, every_adapter()), Decision::Stock);
            assert_eq!(decide(base, 6, every_adapter()), Decision::Stock);
            assert_eq!(decide(base, 255, every_adapter()), Decision::Stock);
        }
    }

    #[test]
    fn p0_swaps_exactly_the_whole_package_set() {
        for skin in 1..=SKIN_MAX {
            for base in [
                "dance_judge",
                "dance_fast_slow",
                "dance_fullcombo",
                "dance_game_over",
            ] {
                assert_eq!(
                    decide(base, skin, P0),
                    Decision::Legacy {
                        arc_base: base,
                        skin
                    },
                    "{base} skin {skin}"
                );
            }
            let danger = decide("dance_danger", skin, P0);
            if skin <= 2 {
                assert!(matches!(danger, Decision::Legacy { .. }), "skin {skin}");
            } else {
                assert_eq!(danger, Decision::Stock, "skin {skin} needs markers");
            }
            for base in [
                "dance_gauge",
                "dance_combo",
                "dance_score",
                "dance_stage",
                "dance_song_info",
                "dance_message",
                "dance_common",
                "dance_effect",
                "dance_bpm",
                "dance_filter",
                "dance_cover",
                "dance_option",
                "dance_score_compare",
            ] {
                assert_eq!(decide(base, skin, P0), Decision::Stock, "{base} {skin}");
            }
        }
    }

    #[test]
    fn adapters_unlock_their_rows_only() {
        let s = AdapterSet::none().with(Adapter::Gauge);
        assert!(matches!(
            decide("dance_gauge", 3, s),
            Decision::Legacy { .. }
        ));
        assert_eq!(decide("dance_combo", 3, s), Decision::Stock);
        assert_eq!(decide("dance_danger", 4, s), Decision::Stock);
        let m = AdapterSet::none().with(Adapter::Markers);
        assert!(matches!(
            decide("dance_danger", 4, m),
            Decision::Legacy { .. }
        ));
    }

    #[test]
    fn stage_maps_to_stage_frame_and_song_info_is_skin_2_only() {
        let all = every_adapter();
        assert_eq!(
            decide("dance_stage", 1, all),
            Decision::Legacy {
                arc_base: "dance_stage_frame",
                skin: 1
            }
        );
        assert!(matches!(
            decide("dance_song_info", 2, all),
            Decision::Legacy { .. }
        ));
        for skin in [1, 3, 4, 5] {
            assert_eq!(decide("dance_song_info", skin, all), Decision::Stock);
        }
    }

    #[test]
    fn layout_root_is_never_swapped() {
        for skin in 0..=SKIN_MAX {
            assert_eq!(
                decide("dance_common", skin, every_adapter()),
                Decision::Stock
            );
        }
        assert!(TABLE.iter().all(|e| e.base != "dance_common"));
    }

    #[test]
    fn no_row_can_produce_a_0000_name() {
        for e in TABLE {
            assert_eq!(
                e.skins & 1,
                0,
                "{}: skin 0 must never be a legacy skin",
                e.base
            );
            assert!(e.skins & !((1u8 << (SKIN_MAX + 1)) - 1) == 0);
            for skin in 1..=SKIN_MAX {
                if e.skins & (1 << skin) != 0 {
                    assert!(!legacy_name(e.arc_base, skin).contains("0000"));
                }
            }
        }
    }

    #[test]
    fn rows_for_one_base_never_overlap() {
        for (i, a) in TABLE.iter().enumerate() {
            for b in &TABLE[i + 1..] {
                if a.base == b.base {
                    assert_eq!(a.skins & b.skins, 0, "{} rows overlap", a.base);
                }
            }
        }
    }

    #[test]
    fn every_base_has_a_mask_index() {
        let mut seen = Vec::new();
        for e in TABLE {
            let i = package_index(e.base).expect(e.base);
            assert!(i < 32);
            if !seen.contains(&e.base) {
                seen.push(e.base);
            }
        }
        assert_eq!(package_index("dance_common"), None);
    }

    #[test]
    fn legacy_name_formats_like_a3() {
        assert_eq!(legacy_name("dance_judge", 1), "dance_judge0001\0");
        assert_eq!(
            legacy_name("dance_stage_frame", 5),
            "dance_stage_frame0005\0"
        );
    }

    #[test]
    fn skin_names() {
        assert_eq!(skin_name(0), None);
        assert_eq!(skin_name(1), Some("1st-5th"));
        assert_eq!(skin_name(5), Some("2013-A"));
        assert_eq!(skin_name(6), None);
        for s in 1..=SKIN_MAX {
            assert!(skin_name(s).unwrap().len() <= 15);
        }
    }
}
