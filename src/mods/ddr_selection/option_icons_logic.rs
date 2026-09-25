//! A3's in-gameplay option icons — the pure half (host-tested).
//!
//! Dependency-free (mounted by `scripts/validate_ddr_selection.sh`).
//!
//! A3's `sequence::common::OptionIconActor` (`FUN_18002b630` in
//! `gamemdx_20240402`) drew a fixed row of eleven `BM2D::CSprite`s from the
//! textures of `dance_option_icon0000_v0` (`daopic0000_<n>p_<kind>_<value>`,
//! 36 × 28): speed (always shown), boost, appearance, turn, dark, scroll,
//! arrow colour, cut, freeze, jump, and — only when the gauge has an icon —
//! the gauge. Every sprite is created (its default-value texture when the
//! option is off) and hidden when the option is at its default; the row
//! advances `marker.w − 2` px per slot whether shown or not, each sprite
//! centred at its slot and scaled to `marker.w / texture width`. RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/option-icons.md`.
//!
//! [`Opts`] holds World's `ddr::player::Option` values (World enums, see the
//! research doc §3); [`icon`] maps them to A3's texture and visibility.

/// The row, in A3's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Speed,
    Boost,
    Appear,
    Turn,
    Dark,
    Scroll,
    Arrow,
    Cut,
    Freeze,
    Jump,
    Gauge,
}

pub const SLOTS: [Slot; 11] = [
    Slot::Speed,
    Slot::Boost,
    Slot::Appear,
    Slot::Turn,
    Slot::Dark,
    Slot::Scroll,
    Slot::Arrow,
    Slot::Cut,
    Slot::Freeze,
    Slot::Jump,
    Slot::Gauge,
];

/// A3's sprite priority (`+0xEC` = 8) and the per-slot pitch reduction.
pub const PRIORITY: u32 = 8;
pub const PITCH_TRIM: i32 = 2;

/// World `ddr::player::Option` values the icons read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Opts {
    /// Effective scroll speed ×100 (speed type 1 ⇒ hispeed, 0 ⇒ the derived
    /// real-speed multiplier).
    pub speed_x100: i32,
    /// {normal, floating flare, flare I..IX, flare EX, life4, risky}.
    pub gauge: i32,
    /// Floating flare's current level (0 none, 1..9, 10 EX).
    pub flare: i32,
    /// {normal, reverse}.
    pub scroll: i32,
    /// {normal, constant, stealth}.
    pub visibility: i32,
    /// {off, hidden, sudden, hidden_sudden}.
    pub lane_cover: i32,
    /// {on, off}.
    pub stepzone: i32,
    /// {normal, boost, brake, wave}.
    pub boost: i32,
    /// {off, mirror, left, right, shuffle}.
    pub turn: i32,
    /// {note, rainbow, vivid, flat}.
    pub color: i32,
    /// {off, on1, on2}.
    pub cut: i32,
    /// {on, off}.
    pub freeze: i32,
    /// {on, off}.
    pub jump: i32,
}

/// Texture kind segment of a slot.
pub fn kind(slot: Slot) -> &'static str {
    match slot {
        Slot::Speed => "speed",
        Slot::Boost => "boost",
        Slot::Appear => "appear",
        Slot::Turn => "turn",
        Slot::Dark => "dark",
        Slot::Scroll => "scroll",
        Slot::Arrow => "arrow",
        Slot::Cut => "cut",
        Slot::Freeze => "freeze",
        Slot::Jump => "jump",
        Slot::Gauge => "gauge",
    }
}

const SPEEDS: [&str; 32] = [
    "x025", "x050", "x075", "x100", "x125", "x150", "x175", "x200", "x225", "x250", "x275", "x300",
    "x325", "x350", "x375", "x400", "x425", "x450", "x475", "x500", "x525", "x550", "x575", "x600",
    "x625", "x650", "x675", "x700", "x725", "x750", "x775", "x800",
];
const FLARE: [&str; 10] = [
    "fl_1", "fl_2", "fl_3", "fl_4", "fl_5", "fl_6", "fl_7", "fl_8", "fl_9", "fl_ex",
];

fn pick(table: &[&'static str], v: i32) -> Option<&'static str> {
    usize::try_from(v).ok().and_then(|i| table.get(i).copied())
}

/// A3's speed icon for an effective speed ×100: the nearest ×0.25 step,
/// clamped to ×0.25..×8.00 (A3's range).
pub fn speed_name(speed_x100: i32) -> &'static str {
    let step = ((speed_x100.max(0) + 12) / 25).clamp(1, 32);
    SPEEDS[(step - 1) as usize]
}

/// The gauge icon, `None` = A3 created no gauge sprite (NORMAL, or an
/// unknown value). Floating flare shows its current level (World's own rule,
/// A3's art); level 0 ⇒ `None`.
pub fn gauge_name(gauge: i32, flare: i32) -> Option<&'static str> {
    match gauge {
        1 => pick(&FLARE, flare - 1),
        2..=11 => pick(&FLARE, gauge - 2),
        12 => Some("life4"),
        13 => Some("risky"),
        _ => None,
    }
}

/// `(texture value, shown)` for a slot; `None` only for an absent gauge.
pub fn icon(slot: Slot, o: &Opts) -> Option<(&'static str, bool)> {
    let flag = |table: &[&'static str], v: i32| match pick(table, v) {
        Some(n) => (n, v != 0),
        None => (table[0], false),
    };
    Some(match slot {
        Slot::Speed => (speed_name(o.speed_x100), true),
        Slot::Boost => flag(&["normal", "boost", "brake", "wave"], o.boost),
        Slot::Appear => {
            let v = if o.visibility == 2 {
                "stealth"
            } else {
                match o.lane_cover {
                    1 => "hidden+",
                    2 => "sudden+",
                    3 => "hidden+_sudden+",
                    // CONSTANT has no A3 icon.
                    _ => "visible",
                }
            };
            (v, v != "visible")
        }
        Slot::Turn => flag(&["off", "mirror", "left", "right", "shuffle"], o.turn),
        Slot::Dark => flag(&["off", "on"], o.stepzone),
        Slot::Scroll => flag(&["normal", "reverse"], o.scroll),
        Slot::Arrow => match o.color {
            0 => ("note", true),
            2 => ("vivid", true),
            3 => ("flat", true),
            // RAINBOW (and anything unknown): A3 hid its default.
            _ => ("rainbow", false),
        },
        Slot::Cut => flag(&["off", "on1", "on2"], o.cut),
        Slot::Freeze => flag(&["on", "off"], o.freeze),
        Slot::Jump => flag(&["on1", "off"], o.jump),
        Slot::Gauge => (gauge_name(o.gauge, o.flare)?, true),
    })
}

/// `daopic0000_<n>p_<kind>_<value>\0` (A3's texture; `side` 0 / 1).
pub fn texture(side: u8, slot: Slot, value: &str) -> String {
    format!("daopic0000_{}p_{}_{}\0", side + 1, kind(slot), value)
}

/// Slot `i`'s centre for the side's `option` marker `(x, y, w)`.
pub fn position(marker: (i32, i32, i32), i: usize) -> (i32, i32) {
    (marker.0 + i as i32 * (marker.2 - PITCH_TRIM), marker.1)
}

/// A3's sprite scale: the marker width over the texture width.
pub fn scale(marker_w: i32, texture_w: i32) -> Option<f32> {
    (texture_w > 0 && marker_w > 0).then(|| marker_w as f32 / texture_w as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock() -> Opts {
        Opts {
            speed_x100: 100,
            color: 1,
            ..Opts::default()
        }
    }

    #[test]
    fn defaults_show_only_speed() {
        let o = stock();
        for s in SLOTS {
            let r = icon(s, &o);
            match s {
                Slot::Speed => assert_eq!(r, Some(("x100", true))),
                Slot::Gauge => assert_eq!(r, None),
                _ => assert!(!r.unwrap().1, "{s:?}"),
            }
        }
    }

    #[test]
    fn default_textures_exist_in_a3s_set() {
        let o = stock();
        let names: Vec<&str> = SLOTS
            .iter()
            .filter_map(|s| icon(*s, &o).map(|(v, _)| (kind(*s), v)))
            .map(|(k, v)| match (k, v) {
                ("boost", "normal")
                | ("appear", "visible")
                | ("turn", "off")
                | ("dark", "off")
                | ("scroll", "normal")
                | ("arrow", "rainbow")
                | ("cut", "off")
                | ("freeze", "on")
                | ("jump", "on1")
                | ("speed", "x100") => "ok",
                _ => "missing",
            })
            .collect();
        assert!(names.iter().all(|n| *n == "ok"), "{names:?}");
    }

    #[test]
    fn speed_rounds_to_a3_steps() {
        assert_eq!(speed_name(100), "x100");
        assert_eq!(speed_name(25), "x025");
        assert_eq!(speed_name(0), "x025");
        assert_eq!(speed_name(-5), "x025");
        assert_eq!(speed_name(335), "x325");
        assert_eq!(speed_name(338), "x350");
        assert_eq!(speed_name(800), "x800");
        assert_eq!(speed_name(2000), "x800");
    }

    #[test]
    fn option_values_map_to_a3_names() {
        let mut o = stock();
        o.boost = 3;
        o.turn = 4;
        o.stepzone = 1;
        o.scroll = 1;
        o.color = 3;
        o.cut = 2;
        o.freeze = 1;
        o.jump = 1;
        assert_eq!(icon(Slot::Boost, &o), Some(("wave", true)));
        assert_eq!(icon(Slot::Turn, &o), Some(("shuffle", true)));
        assert_eq!(icon(Slot::Dark, &o), Some(("on", true)));
        assert_eq!(icon(Slot::Scroll, &o), Some(("reverse", true)));
        assert_eq!(icon(Slot::Arrow, &o), Some(("flat", true)));
        assert_eq!(icon(Slot::Cut, &o), Some(("on2", true)));
        assert_eq!(icon(Slot::Freeze, &o), Some(("off", true)));
        assert_eq!(icon(Slot::Jump, &o), Some(("off", true)));
        o.color = 0;
        assert_eq!(icon(Slot::Arrow, &o), Some(("note", true)));
        o.color = 2;
        assert_eq!(icon(Slot::Arrow, &o), Some(("vivid", true)));
        o.boost = 9;
        assert_eq!(icon(Slot::Boost, &o), Some(("normal", false)));
    }

    #[test]
    fn appearance_combines_visibility_and_lane_cover() {
        let mut o = stock();
        let ap = |o: &Opts| icon(Slot::Appear, o).unwrap();
        o.lane_cover = 1;
        assert_eq!(ap(&o), ("hidden+", true));
        o.lane_cover = 2;
        assert_eq!(ap(&o), ("sudden+", true));
        o.lane_cover = 3;
        assert_eq!(ap(&o), ("hidden+_sudden+", true));
        o.visibility = 2;
        assert_eq!(ap(&o), ("stealth", true));
        o.visibility = 1;
        o.lane_cover = 0;
        assert_eq!(ap(&o), ("visible", false));
    }

    #[test]
    fn gauges() {
        assert_eq!(gauge_name(0, 0), None);
        assert_eq!(gauge_name(1, 10), Some("fl_ex"));
        assert_eq!(gauge_name(1, 3), Some("fl_3"));
        assert_eq!(gauge_name(1, 0), None);
        assert_eq!(gauge_name(2, 0), Some("fl_1"));
        assert_eq!(gauge_name(10, 0), Some("fl_9"));
        assert_eq!(gauge_name(11, 0), Some("fl_ex"));
        assert_eq!(gauge_name(12, 0), Some("life4"));
        assert_eq!(gauge_name(13, 0), Some("risky"));
        assert_eq!(gauge_name(14, 0), None);
    }

    #[test]
    fn layout_and_textures() {
        assert_eq!(position((17, 604, 34), 0), (17, 604));
        assert_eq!(position((17, 604, 34), 10), (17 + 320, 604));
        assert_eq!(scale(34, 36), Some(34.0 / 36.0));
        assert_eq!(scale(34, 0), None);
        assert_eq!(
            texture(1, Slot::Speed, "x150"),
            "daopic0000_2p_speed_x150\0"
        );
        assert_eq!(
            texture(0, Slot::Appear, "hidden+_sudden+"),
            "daopic0000_1p_appear_hidden+_sudden+\0"
        );
        assert_eq!(SLOTS.len(), 11);
    }
}
