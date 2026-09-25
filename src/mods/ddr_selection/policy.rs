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
//!   A3-oldest arcs whose export names do not match World's actors. A row that
//!   wants A3's own skin-0 art names the arc with its version suffix
//!   ([`Entry::fixed_arc`], e.g. `dance_song_info0000_v2` — the file A3
//!   opened on an HD cabinet), which the probe reaches only through its bare
//!   rung.

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
    /// A3's skins 3–5 song-info panel (`dance_song_info0000_v2` + the A3
    /// SongInfoChild text layout).
    SongInfoPanel = 8,
    /// A3's option-icon sprites (`dance_option_icon0000_v0` on World's
    /// OptionIconActor).
    OptionIcons = 9,
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
    /// A fixed package name instead of `<arc_base>000N` — only for A3's own
    /// skin-0 art, and always with its explicit `_vN` suffix (never a bare
    /// `…0000`, which World's probe would resolve to early-World art).
    pub fixed_arc: Option<&'static str>,
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

/// A3's own song-info panel (skin 0), the one skins 3–5 used.
pub const A3_SONG_INFO_PANEL: &str = "dance_song_info0000_v2";
/// A3's pacemaker (skin 0 — the only one A3 had), used on every skin.
pub const A3_PACEMAKER: &str = "dance_score_compare0000_v0";
/// A3's option-icon textures (skin 0 — the only set A3 had), skins 2–5.
pub const A3_OPTION_ICONS: &str = "dance_option_icon0000_v0";

/// The full policy (every phase). A base may appear in several rows with
/// disjoint skin sets (danger: skins 1–2 need nothing, 3–5 need markers).
pub const TABLE: &[Entry] = &[
    // Whole-package swaps (World's consumer matches the legacy exports).
    Entry {
        base: "dance_judge",
        arc_base: "dance_judge",
        skins: ALL,
        adapter: Adapter::None,
        fixed_arc: None,
    },
    Entry {
        base: "dance_fast_slow",
        arc_base: "dance_fast_slow",
        skins: ALL,
        adapter: Adapter::None,
        fixed_arc: None,
    },
    Entry {
        base: "dance_fullcombo",
        arc_base: "dance_fullcombo",
        skins: ALL,
        adapter: Adapter::None,
        fixed_arc: None,
    },
    Entry {
        base: "dance_game_over",
        arc_base: "dance_game_over",
        skins: ALL,
        adapter: Adapter::None,
        fixed_arc: None,
    },
    // World's DanceDangerActor still has A3's skin branches: skins 1–2 draw
    // centred (no marker needed), skins 3–5 at the `danger_gauge` marker,
    // which only the legacy layout roots carry.
    Entry {
        base: "dance_danger",
        arc_base: "dance_danger",
        skins: skins(&[1, 2]),
        adapter: Adapter::None,
        fixed_arc: None,
    },
    Entry {
        base: "dance_danger",
        arc_base: "dance_danger",
        skins: skins(&[3, 4, 5]),
        adapter: Adapter::Markers,
        fixed_arc: None,
    },
    // Consumers that need a DLL adapter first.
    Entry {
        base: "dance_gauge",
        arc_base: "dance_gauge",
        skins: ALL,
        adapter: Adapter::Gauge,
        fixed_arc: None,
    },
    Entry {
        base: "dance_combo",
        arc_base: "dance_combo",
        skins: ALL,
        adapter: Adapter::Combo,
        fixed_arc: None,
    },
    Entry {
        base: "dance_score",
        arc_base: "dance_score",
        skins: ALL,
        adapter: Adapter::Score,
        fixed_arc: None,
    },
    Entry {
        base: "dance_stage",
        arc_base: "dance_stage_frame",
        skins: ALL,
        adapter: Adapter::StageFrame,
        fixed_arc: None,
    },
    Entry {
        base: "dance_song_info",
        arc_base: "dance_song_info",
        skins: skins(&[2]),
        adapter: Adapter::SongInfo,
        fixed_arc: None,
    },
    // Skins 3–5 had no song-info package of their own: A3 fell back to its
    // own skin-0 panel, `dance_song_info0000` → `_v2` on an HD cabinet
    // (World ships the same file byte-identical).
    Entry {
        base: "dance_song_info",
        arc_base: "dance_song_info",
        skins: skins(&[3, 4, 5]),
        adapter: Adapter::SongInfoPanel,
        fixed_arc: Some(A3_SONG_INFO_PANEL),
    },
    // The pacemaker: A3 had no per-skin art — every skin's `%04d` probe fell
    // back to A3's own `dance_score_compare0000_v0` (World ships it
    // byte-identical). World's NoteResultActor asks the record's package for
    // the same export / labels / `%08d_usr` children / `dascco_*` textures and
    // runs A3's digit + tint logic unchanged, so no adapter is needed.
    Entry {
        base: "dance_score_compare",
        arc_base: "dance_score_compare",
        skins: ALL,
        adapter: Adapter::None,
        fixed_arc: Some(A3_PACEMAKER),
    },
    // The in-gameplay option icons: A3's texture-only `dance_option_icon`
    // package (skin 0 only — A3's probe fell back to it on every skin) drawn
    // as sprites by the re-hosted A3 icon row. Skin 1 has no icons (World's
    // own gate, as in A3).
    Entry {
        base: "dance_option",
        arc_base: "dance_option_icon",
        skins: skins(&[2, 3, 4, 5]),
        adapter: Adapter::OptionIcons,
        fixed_arc: Some(A3_OPTION_ICONS),
    },
    Entry {
        base: "dance_message",
        arc_base: "dance_message",
        skins: ALL,
        adapter: Adapter::ReadyGo,
        fixed_arc: None,
    },
    // dance_common (the layout root) is deliberately absent: World's layout
    // builder needs World's root markers; the legacy positions are applied by
    // a post-pass that reads the legacy root itself. dance_effect / bpm /
    // filter / cover have no legacy variants.
];

/// What the per-package helper does with one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// World's own behaviour (the original helper with skin 0).
    Stock,
    /// Register `<arc_base>000N` (or `fixed_arc`) under `base` with record
    /// skin N.
    Legacy {
        arc_base: &'static str,
        skin: u8,
        fixed_arc: Option<&'static str>,
    },
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
                fixed_arc: e.fixed_arc,
            };
        }
    }
    Decision::Stock
}

/// The adapter of the row [`decide`] would pick for `base` / `skin` (any
/// adapter availability), or `None` when no row covers it.
pub fn adapter_for(base: &str, skin: u8) -> Option<Adapter> {
    if skin == 0 || skin > SKIN_MAX {
        return None;
    }
    TABLE
        .iter()
        .find(|e| e.base == base && e.skins & (1 << skin) != 0)
        .map(|e| e.adapter)
}

/// The package name the helper registers for a legacy decision
/// (`dance_judge` + 1 → `dance_judge0001`), NUL-terminated for the game.
pub fn legacy_name(arc_base: &str, skin: u8) -> String {
    format!("{}{:04}\0", arc_base, skin)
}

/// The package name for a legacy decision: the row's fixed arc, else
/// [`legacy_name`]. NUL-terminated.
pub fn package_name(arc_base: &str, skin: u8, fixed_arc: Option<&str>) -> String {
    match fixed_arc {
        Some(n) => format!("{}\0", n),
        None => legacy_name(arc_base, skin),
    }
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
        "dance_score_compare",
        "dance_option",
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
            Adapter::SongInfoPanel,
            Adapter::OptionIcons,
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
                        skin,
                        fixed_arc: None,
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
            ] {
                assert_eq!(decide(base, skin, P0), Decision::Stock, "{base} {skin}");
            }
            assert_eq!(
                decide("dance_score_compare", skin, P0),
                Decision::Legacy {
                    arc_base: "dance_score_compare",
                    skin,
                    fixed_arc: Some("dance_score_compare0000_v0"),
                },
                "the A3 pacemaker needs no adapter (skin {skin})"
            );
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
    fn stage_maps_to_stage_frame_and_song_info_band_is_skin_2() {
        let all = every_adapter();
        assert_eq!(
            decide("dance_stage", 1, all),
            Decision::Legacy {
                arc_base: "dance_stage_frame",
                skin: 1,
                fixed_arc: None,
            }
        );
        assert_eq!(
            decide("dance_song_info", 2, all),
            Decision::Legacy {
                arc_base: "dance_song_info",
                skin: 2,
                fixed_arc: None,
            }
        );
        // Skin 1 has no song info at all (World's own gate).
        assert_eq!(decide("dance_song_info", 1, all), Decision::Stock);
    }

    #[test]
    fn song_info_panel_is_a3s_own_v2_arc_for_skins_3_to_5() {
        let all = every_adapter();
        for skin in [3, 4, 5] {
            let d = decide("dance_song_info", skin, all);
            assert_eq!(
                d,
                Decision::Legacy {
                    arc_base: "dance_song_info",
                    skin,
                    fixed_arc: Some("dance_song_info0000_v2"),
                }
            );
            // The record keeps skin N (World's SongInfoActor takes the
            // record's package only for a non-zero skin).
            assert_eq!(
                package_name("dance_song_info", skin, Some(A3_SONG_INFO_PANEL)),
                "dance_song_info0000_v2\0"
            );
        }
        // The band adapter alone does not unlock the panel, nor vice versa.
        let band = AdapterSet::none().with(Adapter::SongInfo);
        let panel = AdapterSet::none().with(Adapter::SongInfoPanel);
        assert_eq!(decide("dance_song_info", 3, band), Decision::Stock);
        assert_eq!(decide("dance_song_info", 2, panel), Decision::Stock);
        assert!(matches!(
            decide("dance_song_info", 4, panel),
            Decision::Legacy { .. }
        ));
    }

    #[test]
    fn option_icons_are_a3s_texture_set_on_skins_2_to_5() {
        let all = every_adapter();
        assert_eq!(decide("dance_option", 1, all), Decision::Stock);
        for skin in 2..=5 {
            assert_eq!(
                decide("dance_option", skin, all),
                Decision::Legacy {
                    arc_base: "dance_option_icon",
                    skin,
                    fixed_arc: Some("dance_option_icon0000_v0"),
                }
            );
        }
        assert_eq!(
            decide("dance_option", 3, AdapterSet::none()),
            Decision::Stock
        );
        assert_eq!(adapter_for("dance_option", 4), Some(Adapter::OptionIcons));
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
    fn no_row_can_produce_a_bare_0000_name() {
        // World's probe tries `_v3`, `_v0`, `_lite`, bare: a bare
        // `<base>0000` lands on early-World `*0000_v3` art whose exports do
        // not match World's actors. The suffixed path never builds one; the
        // only deliberate `0000` names are fixed arcs of A3's own skin-0 art,
        // and those always carry their `_vN` suffix (reached through the
        // probe's bare rung — `dance_common0000_v2` is the proven precedent).
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
                    let name = package_name(e.arc_base, skin, e.fixed_arc);
                    let name = name.trim_end_matches('\0');
                    assert!(!name.ends_with("0000"), "{name}");
                    if let Some(f) = e.fixed_arc {
                        let (stem, ver) = f.rsplit_once("_v").expect(f);
                        assert!(stem.starts_with(e.arc_base), "{f}");
                        assert!(
                            !ver.is_empty() && ver.bytes().all(|b| b.is_ascii_digit()),
                            "{f}"
                        );
                    }
                }
            }
        }
        let fixed: Vec<&str> = TABLE.iter().filter_map(|e| e.fixed_arc).collect();
        assert_eq!(
            fixed,
            [
                "dance_song_info0000_v2",
                "dance_score_compare0000_v0",
                "dance_option_icon0000_v0"
            ],
            "only A3's own song-info panel, pacemaker and option icons name a fixed arc"
        );
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
    fn adapter_for_reports_the_row_adapter() {
        assert_eq!(adapter_for("dance_danger", 2), Some(Adapter::None));
        assert_eq!(adapter_for("dance_danger", 3), Some(Adapter::Markers));
        assert_eq!(adapter_for("dance_stage", 4), Some(Adapter::StageFrame));
        assert_eq!(adapter_for("dance_song_info", 2), Some(Adapter::SongInfo));
        assert_eq!(
            adapter_for("dance_song_info", 5),
            Some(Adapter::SongInfoPanel)
        );
        assert_eq!(adapter_for("dance_common", 2), None);
        assert_eq!(adapter_for("dance_judge", 0), None);
    }

    #[test]
    fn step7_adapters_unlock_stage_frame_and_danger() {
        let a = AdapterSet::none()
            .with(Adapter::Markers)
            .with(Adapter::StageFrame);
        assert_eq!(
            decide("dance_stage", 3, a),
            Decision::Legacy {
                arc_base: "dance_stage_frame",
                skin: 3,
                fixed_arc: None,
            }
        );
        assert!(matches!(
            decide("dance_danger", 5, a),
            Decision::Legacy { .. }
        ));
        assert_eq!(
            decide("dance_danger", 5, AdapterSet::none()),
            Decision::Stock
        );
        assert_eq!(decide("dance_common", 3, a), Decision::Stock);
        assert_eq!(decide("dance_gauge", 3, a), Decision::Stock);
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
