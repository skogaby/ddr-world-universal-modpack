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
//!   ([`Naming::Fixed`], e.g. `dance_song_info0000_v2` — the file A3 opened
//!   on an HD cabinet; every [`Naming::Theme`] name), which the probe reaches
//!   only through its bare rung.
//!
//! Skins: 1..=5 are the **eras** (A3's legacy `…000N` packages); 6..=8 are the
//! **themes** — A3's own skin-0 UI generations World still ships (DDR A
//! `_v0`, A3 on a white cabinet `_v2`, A3 on the gold cabinet `_v1`), whose
//! packages are registered with the theme skin as the record skin (World's
//! record-skin readers take their skin-0 branches for any value ≥ 6).

/// The highest skin: 1..=5 eras, 6..=8 themes. 0 = World's own UI.
pub const SKIN_MAX: u8 = 8;
/// The highest era skin (1 = 1st-5th … 5 = 2013-2014).
pub const ERA_MAX: u8 = 5;

/// Display name of a skin (logs) — the option row's label for the skin.
/// `None` for 0 / out of range.
pub fn skin_name(skin: u8) -> Option<&'static str> {
    match skin {
        1 => Some("1stMIX-5thMIX"),
        2 => Some("MAX-EXTREME"),
        3 => Some("SuperNOVA 1-2"),
        4 => Some("X-X3 vs 2ndMIX"),
        5 => Some("2013-2014"),
        6 => Some("DDR A"),
        7 => Some("DDR A3 (White)"),
        8 => Some("DDR A3 (Gold)"),
        _ => None,
    }
}

/// One of A3's own skin-0 UI generations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub skin: u8,
    /// The package generation: `_v0` (DDR A), `_v2` (A3 white cabinet),
    /// `_v1` (A3 gold cabinet — A3's probe started at `_v1` there).
    pub suffix: &'static str,
}

const THEMES: [Theme; 3] = [
    Theme {
        skin: 6,
        suffix: "_v0",
    },
    Theme {
        skin: 7,
        suffix: "_v2",
    },
    Theme {
        skin: 8,
        suffix: "_v1",
    },
];

/// The theme of `skin` (6..=8), else `None`.
pub fn theme(skin: u8) -> Option<Theme> {
    THEMES.iter().copied().find(|t| t.skin == skin)
}

pub fn is_era(skin: u8) -> bool {
    (1..=ERA_MAX).contains(&skin)
}

pub fn is_theme(skin: u8) -> bool {
    theme(skin).is_some()
}

/// The `%04d` inside A3's texture names for a skin: the era's own number,
/// 0 for a theme (A3's skin-0 art: `dance_combo0000_*`,
/// `stage_frame0000_stage_*`).
pub fn tex_number(skin: u8) -> u8 {
    if is_theme(skin) {
        0
    } else {
        skin
    }
}

/// The value written to `GameWork+0xA8` for an armed skin: the era itself
/// (World's three `== 1` gates hide song info / option icons on 1st-5th);
/// 0 for a theme (World's DPS `int[6]` table forbids ≥ 6; nothing else
/// reads the field once a skin is armed).
pub fn engine_skin(skin: u8) -> u8 {
    if is_era(skin) {
        skin
    } else {
        0
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
    /// A3's skin-0 song-info panel (`dance_song_info0000_vN` + the A3
    /// SongInfoChild text layout): skins 3–5 and the themes.
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

/// Which package a theme row names (the theme's suffix `vN` is appended
/// where the row says so).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeArc {
    /// `<arc_base>0000<suffix>` — the theme generation's own package.
    Own,
    /// `<arc_base>0000_v0` — A3 used the `_v0` copy on every cabinet (game
    /// over, danger, pacemaker, option icons have no `_v1` / `_v2`).
    V0,
    /// `<arc_base><suffix>` — `dance_message_vN` has no `0000`.
    Message,
}

/// How a legacy row names its package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Naming {
    /// A3's append: `<arc_base>%04d` (the eras).
    Suffixed,
    /// A fixed package — only for A3's own skin-0 art, and always with its
    /// explicit `_vN` suffix (never a bare `…0000`, which World's probe
    /// would resolve to early-World art).
    Fixed(&'static str),
    /// A theme's package ([`ThemeArc`]).
    Theme(ThemeArc),
}

/// One policy row: `base` (the name World's `LayoutActor` requests) turns
/// into the row's package for every skin N in `skins` whose adapter is
/// available.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub base: &'static str,
    pub arc_base: &'static str,
    /// Bit N set ⇔ skin N (1..=[`SKIN_MAX`]) uses this row.
    pub skins: u16,
    pub adapter: Adapter,
    pub naming: Naming,
}

const fn skins(list: &[u8]) -> u16 {
    let mut m = 0u16;
    let mut i = 0;
    while i < list.len() {
        m |= 1 << list[i];
        i += 1;
    }
    m
}

const ALL: u16 = skins(&[1, 2, 3, 4, 5]);
const THEME_SKINS: u16 = skins(&[6, 7, 8]);

/// A3's own song-info panel (skin 0), the one skins 3–5 used.
pub const A3_SONG_INFO_PANEL: &str = "dance_song_info0000_v2";
/// A3's pacemaker (skin 0 — the only one A3 had), used on every era.
pub const A3_PACEMAKER: &str = "dance_score_compare0000_v0";
/// A3's option-icon textures (skin 0 — the only set A3 had), skins 2–5.
pub const A3_OPTION_ICONS: &str = "dance_option_icon0000_v0";

const fn row(
    base: &'static str,
    arc_base: &'static str,
    skins: u16,
    adapter: Adapter,
    naming: Naming,
) -> Entry {
    Entry {
        base,
        arc_base,
        skins,
        adapter,
        naming,
    }
}

/// The full policy (every phase). A base may appear in several rows with
/// disjoint skin sets (danger: skins 1–2 need nothing, 3–5 need markers).
pub const TABLE: &[Entry] = &[
    // Whole-package swaps (World's consumer matches the legacy exports).
    row(
        "dance_judge",
        "dance_judge",
        ALL,
        Adapter::None,
        Naming::Suffixed,
    ),
    row(
        "dance_fast_slow",
        "dance_fast_slow",
        ALL,
        Adapter::None,
        Naming::Suffixed,
    ),
    row(
        "dance_fullcombo",
        "dance_fullcombo",
        ALL,
        Adapter::None,
        Naming::Suffixed,
    ),
    row(
        "dance_game_over",
        "dance_game_over",
        ALL,
        Adapter::None,
        Naming::Suffixed,
    ),
    // World's DanceDangerActor still has A3's skin branches: skins 1–2 draw
    // centred (no marker needed), skins 3–5 at the `danger_gauge` marker,
    // which only the legacy layout roots carry.
    row(
        "dance_danger",
        "dance_danger",
        skins(&[1, 2]),
        Adapter::None,
        Naming::Suffixed,
    ),
    row(
        "dance_danger",
        "dance_danger",
        skins(&[3, 4, 5]),
        Adapter::Markers,
        Naming::Suffixed,
    ),
    // Consumers that need a DLL adapter first.
    row(
        "dance_gauge",
        "dance_gauge",
        ALL,
        Adapter::Gauge,
        Naming::Suffixed,
    ),
    row(
        "dance_combo",
        "dance_combo",
        ALL,
        Adapter::Combo,
        Naming::Suffixed,
    ),
    row(
        "dance_score",
        "dance_score",
        ALL,
        Adapter::Score,
        Naming::Suffixed,
    ),
    row(
        "dance_stage",
        "dance_stage_frame",
        ALL,
        Adapter::StageFrame,
        Naming::Suffixed,
    ),
    row(
        "dance_song_info",
        "dance_song_info",
        skins(&[2]),
        Adapter::SongInfo,
        Naming::Suffixed,
    ),
    // Skins 3–5 had no song-info package of their own: A3 fell back to its
    // own skin-0 panel, `dance_song_info0000` → `_v2` on an HD cabinet
    // (World ships the same file byte-identical).
    row(
        "dance_song_info",
        "dance_song_info",
        skins(&[3, 4, 5]),
        Adapter::SongInfoPanel,
        Naming::Fixed(A3_SONG_INFO_PANEL),
    ),
    // The pacemaker: A3 had no per-skin art — every skin's `%04d` probe fell
    // back to A3's own `dance_score_compare0000_v0` (World ships it
    // byte-identical). World's NoteResultActor asks the record's package for
    // the same export / labels / `%08d_usr` children / `dascco_*` textures and
    // runs A3's digit + tint logic unchanged, so no adapter is needed.
    row(
        "dance_score_compare",
        "dance_score_compare",
        ALL,
        Adapter::None,
        Naming::Fixed(A3_PACEMAKER),
    ),
    // The in-gameplay option icons: A3's texture-only `dance_option_icon`
    // package (skin 0 only — A3's probe fell back to it on every skin) drawn
    // as sprites by the re-hosted A3 icon row. Skin 1 has no icons (World's
    // own gate, as in A3).
    row(
        "dance_option",
        "dance_option_icon",
        skins(&[2, 3, 4, 5]),
        Adapter::OptionIcons,
        Naming::Fixed(A3_OPTION_ICONS),
    ),
    row(
        "dance_message",
        "dance_message",
        ALL,
        Adapter::ReadyGo,
        Naming::Suffixed,
    ),
    // ── Themes: A3's own skin-0 packages of the theme's generation. A row
    // lands only together with its adapter's acceptance of skins 6..=8. ──
    row(
        "dance_judge",
        "dance_judge",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_fast_slow",
        "dance_fast_slow",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_fullcombo",
        "dance_fullcombo",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_game_over",
        "dance_game_over",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::V0),
    ),
    // The HUD actors' adapters accept skins 6..=8 (A3's skin-0 rules: the
    // segmented 26-cell gauge, per-grade combo sheets, the level-texture
    // difficulty scheme, A3's option-icon sprites).
    row(
        "dance_gauge",
        "dance_gauge",
        THEME_SKINS,
        Adapter::Gauge,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_combo",
        "dance_combo",
        THEME_SKINS,
        Adapter::Combo,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_score",
        "dance_score",
        THEME_SKINS,
        Adapter::Score,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_option",
        "dance_option_icon",
        THEME_SKINS,
        Adapter::OptionIcons,
        Naming::Theme(ThemeArc::V0),
    ),
    // A record skin ≥ 6 takes the danger actor's skin-0 placement (the
    // `filter` marker, layer 0 / priority 6) — no adapter needed.
    row(
        "dance_danger",
        "dance_danger",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::V0),
    ),
    row(
        "dance_stage",
        "dance_stage_frame",
        THEME_SKINS,
        Adapter::StageFrame,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_song_info",
        "dance_song_info",
        THEME_SKINS,
        Adapter::SongInfoPanel,
        Naming::Theme(ThemeArc::Own),
    ),
    row(
        "dance_score_compare",
        "dance_score_compare",
        THEME_SKINS,
        Adapter::None,
        Naming::Theme(ThemeArc::V0),
    ),
    // READY / HERE WE GO from the theme's own `dance_message_vN`.
    row(
        "dance_message",
        "dance_message",
        THEME_SKINS,
        Adapter::ReadyGo,
        Naming::Theme(ThemeArc::Message),
    ),
    // dance_common (the layout root) is deliberately absent: World's layout
    // builder needs World's root markers; the legacy positions are applied by
    // a post-pass that reads the legacy root itself. dance_effect / bpm /
    // filter / cover / measure have no legacy variant World's actors accept.
];

/// What the per-package helper does with one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// World's own behaviour (the original helper with skin 0).
    Stock,
    /// Register [`package_name`]`(arc_base, skin, naming)` under `base` with
    /// record skin N.
    Legacy {
        arc_base: &'static str,
        skin: u8,
        naming: Naming,
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
                naming: e.naming,
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

/// A3's appended name (`dance_judge` + 1 → `dance_judge0001`),
/// NUL-terminated for the game.
pub fn legacy_name(arc_base: &str, skin: u8) -> String {
    format!("{}{:04}\0", arc_base, skin)
}

/// The package name the helper registers for a legacy decision — the one
/// naming source for every surface (package helper, READY, S-Marvelous).
/// NUL-terminated. A theme naming with a non-theme skin (unreachable through
/// [`TABLE`]) falls back to A3's append.
pub fn package_name(arc_base: &str, skin: u8, naming: Naming) -> String {
    match (naming, theme(skin)) {
        (Naming::Fixed(n), _) => format!("{}\0", n),
        (Naming::Theme(ThemeArc::Own), Some(t)) => format!("{}0000{}\0", arc_base, t.suffix),
        (Naming::Theme(ThemeArc::V0), Some(_)) => format!("{}0000_v0\0", arc_base),
        (Naming::Theme(ThemeArc::Message), Some(t)) => format!("{}{}\0", arc_base, t.suffix),
        (Naming::Suffixed, _) | (Naming::Theme(_), None) => legacy_name(arc_base, skin),
    }
}

/// Whether a `base` request decided as `decision` wants World's danger actor
/// to pick `danger_double` on doubles (`danger.rs`): exactly a theme's
/// `dance_danger` — World's skin-0 rule on the theme's own package. The eras
/// keep A3's `danger_single`-only behaviour; stock never needs the patch.
pub fn wants_danger_doubles(base: &str, decision: Decision) -> bool {
    base == "dance_danger" && matches!(decision, Decision::Legacy { skin, .. } if is_theme(skin))
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
            assert_eq!(decide(base, SKIN_MAX + 1, every_adapter()), Decision::Stock);
            assert_eq!(decide(base, 255, every_adapter()), Decision::Stock);
        }
    }

    #[test]
    fn p0_swaps_exactly_the_whole_package_set() {
        for skin in 1..=ERA_MAX {
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
                        naming: Naming::Suffixed,
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
                    naming: Naming::Fixed("dance_score_compare0000_v0"),
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
                naming: Naming::Suffixed,
            }
        );
        assert_eq!(
            decide("dance_song_info", 2, all),
            Decision::Legacy {
                arc_base: "dance_song_info",
                skin: 2,
                naming: Naming::Suffixed,
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
                    naming: Naming::Fixed("dance_song_info0000_v2"),
                }
            );
            // The record keeps skin N (World's SongInfoActor takes the
            // record's package only for a non-zero skin).
            assert_eq!(
                package_name("dance_song_info", skin, Naming::Fixed(A3_SONG_INFO_PANEL)),
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
                    naming: Naming::Fixed("dance_option_icon0000_v0"),
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
        // only deliberate `0000` names are A3's own skin-0 art (fixed arcs and
        // every theme name), and those always carry their `_vN` suffix
        // (reached through the probe's bare rung — `dance_common0000_v2` is
        // the proven precedent).
        let valid = (1u16 << (SKIN_MAX + 1)) - 2;
        for e in TABLE {
            assert_eq!(
                e.skins & 1,
                0,
                "{}: skin 0 must never be a legacy skin",
                e.base
            );
            assert_eq!(e.skins & !valid, 0, "{}: mask past SKIN_MAX", e.base);
            for skin in 1..=SKIN_MAX {
                if e.skins & (1 << skin) == 0 {
                    continue;
                }
                assert!(!legacy_name(e.arc_base, skin).contains("0000"));
                let name = package_name(e.arc_base, skin, e.naming);
                let name = name.trim_end_matches('\0');
                assert!(!name.ends_with("0000"), "{name}");
                if !matches!(e.naming, Naming::Suffixed) {
                    let (stem, ver) = name.rsplit_once("_v").expect(name);
                    assert!(stem.starts_with(e.arc_base), "{name}");
                    assert!(
                        !ver.is_empty() && ver.bytes().all(|b| b.is_ascii_digit()),
                        "{name}"
                    );
                }
            }
        }
        let fixed: Vec<&str> = TABLE
            .iter()
            .filter_map(|e| match e.naming {
                Naming::Fixed(f) => Some(f),
                _ => None,
            })
            .collect();
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
                naming: Naming::Suffixed,
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
        assert_eq!(skin_name(1), Some("1stMIX-5thMIX"));
        assert_eq!(skin_name(5), Some("2013-2014"));
        assert_eq!(skin_name(6), Some("DDR A"));
        assert_eq!(skin_name(8), Some("DDR A3 (Gold)"));
        assert_eq!(skin_name(9), None);
        for s in 1..=SKIN_MAX {
            assert!(skin_name(s).unwrap().len() <= 15);
            // The logs name a skin exactly as the option row does.
            assert_eq!(skin_name(s), super::super::trigger::row_label(s as i32 + 1));
        }
    }

    // ── Themes (skins 6..=8) ────────────────────────────────────────────

    const THEMES: [(u8, &str); 3] = [(6, "_v0"), (7, "_v2"), (8, "_v1")];

    fn legacy_name_of(base: &str, skin: u8, adapters: AdapterSet) -> Option<String> {
        match decide(base, skin, adapters) {
            Decision::Legacy {
                arc_base,
                skin: s,
                naming,
            } => {
                assert_eq!(s, skin, "{base}: the record skin is the theme skin");
                Some(
                    package_name(arc_base, s, naming)
                        .trim_end_matches('\0')
                        .to_string(),
                )
            }
            Decision::Stock => None,
        }
    }

    #[test]
    fn theme_table() {
        assert_eq!(SKIN_MAX, 8);
        assert_eq!(ERA_MAX, 5);
        for (skin, suffix) in THEMES {
            let t = theme(skin).unwrap();
            assert_eq!((t.skin, t.suffix), (skin, suffix));
            assert!(is_theme(skin) && !is_era(skin));
        }
        for skin in 1..=5 {
            assert!(is_era(skin) && !is_theme(skin) && theme(skin).is_none());
        }
        for skin in [0, 9, 255] {
            assert!(!is_era(skin) && !is_theme(skin) && theme(skin).is_none());
        }
    }

    #[test]
    fn texture_number_and_engine_skin() {
        for skin in 1..=5 {
            assert_eq!(tex_number(skin), skin);
            assert_eq!(engine_skin(skin), skin);
        }
        for skin in 6..=8 {
            // A3's skin-0 art: `dance_combo0000_*`, `stage_frame0000_stage_*`.
            assert_eq!(tex_number(skin), 0);
            // World's DPS `int[6]` table and its `== 1` gates see a stock value.
            assert_eq!(engine_skin(skin), 0);
        }
        assert_eq!(engine_skin(0), 0);
    }

    #[test]
    fn theme_package_names() {
        let all = every_adapter();
        for (skin, v) in THEMES {
            for base in ["dance_judge", "dance_fast_slow", "dance_fullcombo"] {
                assert_eq!(
                    legacy_name_of(base, skin, all).as_deref(),
                    Some(format!("{base}0000{v}").as_str())
                );
            }
            assert_eq!(
                legacy_name_of("dance_stage", skin, all),
                Some(format!("dance_stage_frame0000{v}"))
            );
            assert_eq!(
                legacy_name_of("dance_song_info", skin, all),
                Some(format!("dance_song_info0000{v}"))
            );
            // A3 used the `_v0` copies of these on every cabinet.
            for (base, name) in [
                ("dance_game_over", "dance_game_over0000_v0"),
                ("dance_danger", "dance_danger0000_v0"),
                ("dance_score_compare", "dance_score_compare0000_v0"),
            ] {
                assert_eq!(legacy_name_of(base, skin, all).as_deref(), Some(name));
                // Whole-package swaps: no adapter needed.
                assert_eq!(legacy_name_of(base, skin, P0).as_deref(), Some(name));
            }
        }
    }

    #[test]
    fn theme_rows_need_their_adapters() {
        for (skin, _) in THEMES {
            assert_eq!(decide("dance_stage", skin, P0), Decision::Stock);
            assert_eq!(decide("dance_song_info", skin, P0), Decision::Stock);
            // The band adapter never unlocks the theme's panel.
            let band = AdapterSet::none().with(Adapter::SongInfo);
            assert_eq!(decide("dance_song_info", skin, band), Decision::Stock);
            assert_eq!(adapter_for("dance_stage", skin), Some(Adapter::StageFrame));
            assert_eq!(
                adapter_for("dance_song_info", skin),
                Some(Adapter::SongInfoPanel)
            );
            assert_eq!(adapter_for("dance_danger", skin), Some(Adapter::None));
        }
    }

    #[test]
    fn unadapted_bases_stay_stock_on_themes() {
        // No legacy variant World's actor accepts.
        let all = every_adapter();
        for (skin, _) in THEMES {
            for base in [
                "dance_common",
                "dance_effect",
                "dance_filter",
                "dance_cover",
                "dance_bpm",
                "dance_measure",
            ] {
                assert_eq!(decide(base, skin, all), Decision::Stock, "{base} {skin}");
            }
        }
        for base in ["dance_judge", "dance_stage", "dance_gauge"] {
            assert_eq!(decide(base, 9, all), Decision::Stock);
        }
    }

    #[test]
    fn theme_hud_rows_need_their_adapters() {
        let all = every_adapter();
        for (skin, v) in THEMES {
            for (base, name, adapter) in [
                ("dance_gauge", format!("dance_gauge0000{v}"), Adapter::Gauge),
                ("dance_combo", format!("dance_combo0000{v}"), Adapter::Combo),
                ("dance_score", format!("dance_score0000{v}"), Adapter::Score),
                (
                    "dance_option",
                    "dance_option_icon0000_v0".to_string(),
                    Adapter::OptionIcons,
                ),
                // READY / HERE WE GO: `dance_message_vN` has no `0000`.
                (
                    "dance_message",
                    format!("dance_message{v}"),
                    Adapter::ReadyGo,
                ),
            ] {
                assert_eq!(legacy_name_of(base, skin, all), Some(name), "{base} {skin}");
                assert_eq!(adapter_for(base, skin), Some(adapter));
                assert_eq!(
                    decide(base, skin, P0),
                    Decision::Stock,
                    "{base} needs {adapter:?}"
                );
                assert!(matches!(
                    decide(base, skin, AdapterSet::none().with(adapter)),
                    Decision::Legacy { .. }
                ));
            }
        }
    }

    #[test]
    fn eras_never_take_theme_rows_and_vice_versa() {
        let all = every_adapter();
        for e in TABLE {
            let theme_row = matches!(e.naming, Naming::Theme(_));
            for skin in 1..=SKIN_MAX {
                if e.skins & (1 << skin) != 0 {
                    assert_eq!(theme_row, is_theme(skin), "{} skin {skin}", e.base);
                }
            }
        }
        // An era's names never carry a version suffix unless they are A3's
        // three shared skin-0 arcs.
        for skin in 1..=ERA_MAX {
            let name = legacy_name_of("dance_judge", skin, all).unwrap();
            assert_eq!(name, format!("dance_judge{:04}", skin));
        }
    }

    #[test]
    fn danger_doubles_only_for_theme_danger() {
        for adapters in [P0, every_adapter()] {
            for (skin, _) in THEMES {
                let d = decide("dance_danger", skin, adapters);
                assert!(wants_danger_doubles("dance_danger", d), "skin {skin}");
                // Another theme package never touches the danger site.
                for base in ["dance_judge", "dance_game_over", "dance_score_compare"] {
                    let d = decide(base, skin, adapters);
                    assert!(matches!(d, Decision::Legacy { .. }));
                    assert!(!wants_danger_doubles(base, d), "{base} {skin}");
                }
            }
            // The eras keep A3's `danger_single`, whether they register or not.
            for skin in 0..=ERA_MAX {
                let d = decide("dance_danger", skin, adapters);
                assert!(!wants_danger_doubles("dance_danger", d), "skin {skin}");
            }
        }
        assert!(matches!(
            decide("dance_danger", 1, P0),
            Decision::Legacy { skin: 1, .. }
        ));
        assert!(!wants_danger_doubles("dance_danger", Decision::Stock));
        // A theme skin with an odd naming is still a theme decision.
        assert!(wants_danger_doubles(
            "dance_danger",
            Decision::Legacy {
                arc_base: "dance_danger",
                skin: 8,
                naming: Naming::Suffixed,
            }
        ));
    }

    #[test]
    fn theme_naming_outside_a_theme_skin_falls_back_to_suffixed() {
        // Unreachable through the table (tested above); defined anyway.
        assert_eq!(
            package_name("dance_judge", 3, Naming::Theme(ThemeArc::Own)),
            "dance_judge0003\0"
        );
    }
}
