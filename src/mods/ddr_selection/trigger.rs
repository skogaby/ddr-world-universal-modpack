//! DDR SELECTION trigger — which skin a song plays with (pure, host-tested).
//!
//! Dependency-free on purpose: `scripts/validate_ddr_selection.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there.
//!
//! The DDR SELECTION option row (per player) holds [`RowValue`]s. One skin
//! applies per song for the whole cabinet (the gameplay packages are shared):
//! the entered side's row governs; with both sides entered (versus) P1's row
//! governs. A multiplayer-bot side never governs. AUTO maps the committed
//! song's raw musicdb `<series>` through [`auto_skin`] — A3's DDR SELECTION
//! folder buckets for the eras, then DDR A's songs to the DDR A theme and
//! A20 / A20 PLUS / A3's to A3's own UI (gold on a gold cabinet, as A3 chose
//! it); an explicit era or theme applies to every song.

/// Row value → meaning (the option row's stored integer, 0..=9): OFF, AUTO,
/// then one value per skin (value − 1 = the skin: 2..=6 the eras, 7..=9 the
/// themes — appended, so every saved 0..=6 keeps its meaning).
pub const ROW_OFF: i32 = 0;
pub const ROW_AUTO: i32 = 1;
pub const ROW_MAX: i32 = 9;

/// Display text for a row value (≤ 15 bytes — the row's SSO budget).
pub fn row_label(value: i32) -> Option<&'static str> {
    Some(match value {
        0 => "OFF",
        1 => "AUTO",
        2 => "1stMIX-5thMIX",
        3 => "MAX-EXTREME",
        4 => "SuperNOVA 1-2",
        5 => "X-X3 vs 2ndMIX",
        6 => "2013-2014",
        7 => "DDR A",
        8 => "DDR A3 (White)",
        9 => "DDR A3 (Gold)",
        _ => return None,
    })
}

/// A cached / hand-edited row value outside the range lands on OFF.
pub fn clamp_row(value: i32) -> i32 {
    if (ROW_OFF..=ROW_MAX).contains(&value) {
        value
    } else {
        ROW_OFF
    }
}

/// AUTO's skin for a raw musicdb `<series>`: A3's DDR SELECTION folder
/// buckets 1–5 1st–5thMIX, 6–8 MAX/MAX2/EXTREME, 9–10 SuperNOVA 1–2,
/// 11–13 X/X2/X3 VS 2ndMIX, 14–16 2013 / 2014; then 17 (DDR A) → the DDR A
/// theme and 18–20 (A20, A20 PLUS, A3) → A3's own UI, **Gold** when the
/// cabinet is the gold cabinet (machine type 4 — A3's own test), else
/// **White**. 0, World (21) and custom series (≥ 22) keep World's UI.
pub fn auto_skin(series: u8, gold_cabinet: bool) -> u8 {
    match series {
        1..=5 => 1,
        6..=8 => 2,
        9..=10 => 3,
        11..=13 => 4,
        14..=16 => 5,
        17 => 6,
        18..=20 if gold_cabinet => 8,
        18..=20 => 7,
        _ => 0,
    }
}

/// Where the resolved skin came from (the per-song INFO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The developer knob (`DDR_SELECTION_FORCE`) overrides the rows.
    DevKnob,
    /// The governing side's row holds an explicit era.
    Explicit,
    /// The governing side's row is AUTO; the value is the song's series.
    Auto(u8),
    /// No skin: the row is OFF, AUTO mapped to stock, or nothing governs.
    None,
}

/// Why a song resolved to stock (diagnostics only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StockReason {
    /// No entered, non-bot side (or the entered state is unreadable).
    NoGoverningSide,
    /// The governing side's row is OFF.
    RowOff,
    /// AUTO and the song's series maps to World's UI.
    AutoStock,
    /// AUTO and the series could not be read.
    SeriesUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    /// 0 = World UI, 1..=5 = era, 6..=8 = theme.
    pub skin: u8,
    pub governing: Option<u8>,
    pub source: Source,
    pub stock_reason: Option<StockReason>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    /// `PlayerWork[side]+0x4 != 0` per side (`None` = unreadable).
    pub entered: [Option<bool>; 2],
    /// The multiplayer bot's phantom side (never governs).
    pub bot_side: Option<u8>,
    /// Per-side DDR SELECTION row values.
    pub row: [i32; 2],
    /// Raw `<series>` of the song the governing side committed (`None` =
    /// unreadable). Only consulted for AUTO.
    pub series: Option<u8>,
    /// Developer-knob skin (0 = unset).
    pub dev_skin: u8,
    /// The cabinet is the gold cabinet (only consulted for AUTO's A3 songs;
    /// `false` when unreadable).
    pub gold_cabinet: bool,
}

/// The governing side: P1 when entered, else P2 when entered; a bot side and
/// an unreadable entered flag never govern.
pub fn governing_side(entered: [Option<bool>; 2], bot_side: Option<u8>) -> Option<u8> {
    (0..2u8).find(|&s| bot_side != Some(s) && entered[s as usize] == Some(true))
}

pub fn resolve(i: &Inputs) -> Resolution {
    let governing = governing_side(i.entered, i.bot_side);
    if (1..=super::policy::SKIN_MAX).contains(&i.dev_skin) {
        return Resolution {
            skin: i.dev_skin,
            governing,
            source: Source::DevKnob,
            stock_reason: None,
        };
    }
    let stock = |reason| Resolution {
        skin: 0,
        governing,
        source: Source::None,
        stock_reason: Some(reason),
    };
    let Some(side) = governing else {
        return stock(StockReason::NoGoverningSide);
    };
    match clamp_row(i.row[side as usize]) {
        ROW_OFF => stock(StockReason::RowOff),
        ROW_AUTO => match i.series {
            None => stock(StockReason::SeriesUnavailable),
            Some(series) => match auto_skin(series, i.gold_cabinet) {
                0 => stock(StockReason::AutoStock),
                skin => Resolution {
                    skin,
                    governing,
                    source: Source::Auto(series),
                    stock_reason: None,
                },
            },
        },
        era => Resolution {
            skin: (era - 1) as u8,
            governing,
            source: Source::Explicit,
            stock_reason: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solo(side: usize, row: i32, series: Option<u8>) -> Inputs {
        let mut entered = [Some(false); 2];
        entered[side] = Some(true);
        let mut rows = [ROW_OFF; 2];
        rows[side] = row;
        Inputs {
            entered,
            bot_side: None,
            row: rows,
            series,
            dev_skin: 0,
            gold_cabinet: false,
        }
    }

    #[test]
    fn auto_table_matches_a3_folder_buckets_and_the_themes() {
        // (series, white-cabinet skin, gold-cabinet skin)
        let expect = [
            (0, 0, 0),
            (1, 1, 1),
            (5, 1, 1),
            (6, 2, 2),
            (8, 2, 2),
            (9, 3, 3),
            (10, 3, 3),
            (11, 4, 4),
            (13, 4, 4),
            (14, 5, 5),
            (16, 5, 5),
            // DDR A is its own entry now.
            (17, 6, 6),
            // A20, A20 PLUS, A3: A3's own UI, gold on the gold cabinet.
            (18, 7, 8),
            (19, 7, 8),
            (20, 7, 8),
            // World and custom series keep World's UI.
            (21, 0, 0),
            (22, 0, 0),
            (255, 0, 0),
        ];
        for (series, white, gold) in expect {
            assert_eq!(auto_skin(series, false), white, "series {series} white");
            assert_eq!(auto_skin(series, true), gold, "series {series} gold");
        }
    }

    #[test]
    fn auto_follows_the_cabinet_but_explicit_rows_do_not() {
        let mut i = solo(0, ROW_AUTO, Some(20));
        assert_eq!(resolve(&i).skin, 7);
        i.gold_cabinet = true;
        assert_eq!(resolve(&i).skin, 8);
        assert_eq!(resolve(&i).source, Source::Auto(20));
        for (row, skin) in [(8, 7), (9, 8)] {
            for gold in [false, true] {
                let mut i = solo(0, row, Some(20));
                i.gold_cabinet = gold;
                assert_eq!(resolve(&i).skin, skin, "row {row} gold {gold}");
            }
        }
    }

    #[test]
    fn explicit_eras_apply_to_every_series() {
        for era in 2..=ROW_MAX {
            for series in [None, Some(0), Some(3), Some(21), Some(40)] {
                let r = resolve(&solo(0, era, series));
                assert_eq!(r.skin, (era - 1) as u8);
                assert_eq!(r.source, Source::Explicit);
            }
        }
    }

    #[test]
    fn auto_uses_the_series() {
        let r = resolve(&solo(1, ROW_AUTO, Some(9)));
        assert_eq!(
            (r.skin, r.governing, r.source),
            (3, Some(1), Source::Auto(9))
        );
        let r = resolve(&solo(0, ROW_AUTO, Some(21)));
        assert_eq!((r.skin, r.stock_reason), (0, Some(StockReason::AutoStock)));
        let r = resolve(&solo(0, ROW_AUTO, None));
        assert_eq!(
            (r.skin, r.stock_reason),
            (0, Some(StockReason::SeriesUnavailable))
        );
    }

    #[test]
    fn off_is_stock() {
        let r = resolve(&solo(0, ROW_OFF, Some(1)));
        assert_eq!((r.skin, r.stock_reason), (0, Some(StockReason::RowOff)));
    }

    #[test]
    fn versus_p1_governs() {
        let i = Inputs {
            entered: [Some(true), Some(true)],
            bot_side: None,
            row: [ROW_OFF, 2],
            series: Some(1),
            dev_skin: 0,
            gold_cabinet: false,
        };
        assert_eq!(resolve(&i).skin, 0);
        assert_eq!(resolve(&i).governing, Some(0));
        let i = Inputs {
            row: [4, ROW_OFF],
            ..i
        };
        assert_eq!(resolve(&i).skin, 3);
    }

    #[test]
    fn bot_side_never_governs() {
        // Human on P2, bot flipped onto P1 with a stale cached row.
        let i = Inputs {
            entered: [Some(true), Some(true)],
            bot_side: Some(0),
            row: [6, 2],
            series: Some(1),
            dev_skin: 0,
            gold_cabinet: false,
        };
        let r = resolve(&i);
        assert_eq!((r.governing, r.skin), (Some(1), 1));
        // Bot alone ⇒ nothing governs.
        let i = Inputs {
            entered: [Some(true), Some(false)],
            bot_side: Some(0),
            ..i
        };
        assert_eq!(resolve(&i).stock_reason, Some(StockReason::NoGoverningSide));
    }

    #[test]
    fn unreadable_entered_state_fails_closed() {
        let i = Inputs {
            entered: [None, None],
            bot_side: None,
            row: [2, 2],
            series: Some(1),
            dev_skin: 0,
            gold_cabinet: false,
        };
        assert_eq!(resolve(&i).skin, 0);
        let i = Inputs {
            entered: [None, Some(true)],
            ..i
        };
        assert_eq!(resolve(&i).governing, Some(1));
    }

    #[test]
    fn dev_knob_overrides_rows() {
        let mut i = solo(0, ROW_OFF, None);
        i.dev_skin = 4;
        let r = resolve(&i);
        assert_eq!((r.skin, r.source), (4, Source::DevKnob));
        i.dev_skin = 9; // out of range ⇒ ignored
        assert_eq!(resolve(&i).source, Source::None);
    }

    #[test]
    fn out_of_range_rows_clamp_to_off() {
        assert_eq!(clamp_row(-1), ROW_OFF);
        assert_eq!(clamp_row(9), 9);
        assert_eq!(clamp_row(10), ROW_OFF);
        assert_eq!(clamp_row(3), 3);
        assert_eq!(resolve(&solo(0, 99, Some(1))).skin, 0);
    }

    #[test]
    fn labels_fit_the_row_budget() {
        for v in ROW_OFF..=ROW_MAX {
            let l = row_label(v).unwrap();
            assert!(!l.is_empty() && l.len() <= 15 && l.is_ascii(), "{l}");
        }
        assert_eq!(row_label(10), None);
        assert_eq!(row_label(-1), None);
    }

    #[test]
    fn theme_rows_are_appended_after_the_eras() {
        assert_eq!(ROW_MAX, 9);
        assert_eq!(row_label(6), Some("2013-2014"));
        assert_eq!(row_label(7), Some("DDR A"));
        assert_eq!(row_label(8), Some("DDR A3 (White)"));
        assert_eq!(row_label(9), Some("DDR A3 (Gold)"));
        for (row, skin) in [(7, 6), (8, 7), (9, 8)] {
            let r = resolve(&solo(0, row, Some(20)));
            assert_eq!((r.skin, r.source), (skin, Source::Explicit), "row {row}");
        }
    }

    #[test]
    fn dev_knob_reaches_the_themes() {
        let mut i = solo(0, ROW_OFF, None);
        i.dev_skin = 8;
        assert_eq!(resolve(&i).skin, 8);
        assert_eq!(resolve(&i).source, Source::DevKnob);
        i.dev_skin = 6;
        assert_eq!(resolve(&i).skin, 6);
    }
}
