//! Pure readout model for the realtime gameplay-statistics widget: the two
//! LAYOUTS (side column / bottom line), the two CONTENT sets (detailed /
//! streamlined), per-layout screen geometry, the block alignment, the
//! max-error tracker and the text composition itself.
//!
//! Dependency-free (std only) so the whole surface is host-testable through
//! `scripts/validate_power_user_statistics.sh`; `timing_stats_widget` is the
//! engine-facing consumer (widgets, rows, config, bottom-text hide) and
//! `data_feed` feeds the accumulator this module snapshots.
//!
//! ## Sign convention of every user-facing ms readout
//!
//! `judge_submit`'s payload delta is `actual − expected` (Ghidra
//! `FUN_18005fcc0` on 20260825: `result+8 − note+8`; `< 0` bumps the FAST
//! counter `+0x1C4`), i.e. NEGATIVE = FAST (early). That is the value the
//! feed stores (`MsErrorAccum`, `latest_ms_error`, `StepRecord::delta_ms` —
//! kept raw so `actual_ms = expected_ms + delta_ms` holds in memory) and what
//! the calibration / diagnostics taps consume (their sign models were
//! cabinet-verified on it).
//!
//! Every USER-FACING readout (pacemaker → ms-error digits + color, the
//! widget's Current / Δ / Max Δ / μ, AND the CSV export's `Delta` column)
//! shows the OPPOSITE sign: POSITIVE = FAST, NEGATIVE = SLOW. That is the
//! game's own results convention — the stage record's per-note ms stream
//! (`rec+0xD8`, written by `FUN_1801e6ca0` as `expected − actual`) drives the
//! results graph with FAST on the positive axis — and what testers expected
//! (2026-09: "slow is negative, fast is positive"; the CSV was the last
//! raw-sign surface, reported swapped 2026-09-13). Apply at the display
//! boundary only: [`display_ms`] / [`display_ms_f64`].

use std::fmt::Write;

/// Logical canvas the widget positions live on.
pub const CANVAS_W: f32 = 1280.0;
pub const CANVAS_H: f32 = 720.0;

/// Stock widget scale (== 100 % on the scale row).
pub const STOCK_SCALE: f32 = 0.5;

/// Engine grade indices (`judge_code − 0x1028`).
pub const GRADE_MARVELOUS: usize = 0;
pub const GRADE_PERFECT: usize = 1;
pub const GRADE_GREAT: usize = 2;
pub const GRADE_GOOD: usize = 3;
/// BOO (±160 ms window in the engine's classifier table). The results screen
/// has no row for it and the wire never sends its record slot, so it is
/// believed dead in World; if it ever fires it is a combo-breaking,
/// EX-less step — folded into MISS by [`compose`].
pub const GRADE_BOO: usize = 4;
pub const GRADE_MISS: usize = 5;
/// O.K. (freeze hold) — EX 3, no timing sample. Not shown by either content set.
pub const GRADE_OK: usize = 6;
/// Number of grade slots the per-song counter array carries (0..=6).
pub const GRADE_COUNT: usize = 7;

/// Signed ms error in the DISPLAY convention (positive = FAST). See the
/// module docs.
#[inline]
pub fn display_ms(captured_ms: i32) -> i32 {
    captured_ms.wrapping_neg()
}

/// [`display_ms`] for the accumulated (f64) mean. `0.0 - x` rather than
/// `-x` so an exact-zero mean stays +0.0 (`{:+.2}` would otherwise print
/// "-0.00").
#[inline]
pub fn display_ms_f64(captured_mean: f64) -> f64 {
    0.0 - captured_mean
}

/// Fold one timed step into the running maximum: `(max_abs, max_signed)` →
/// updated pair. The magnitude decides; the SIGNED value of the winning
/// step is kept so the readout can still show whether the worst step was
/// fast or slow. Ties keep the earlier step.
#[inline]
pub fn update_max(max_abs: i32, max_signed: i32, captured_ms: i32) -> (i32, i32) {
    let abs = captured_ms.unsigned_abs() as i32;
    if abs > max_abs {
        (abs, captured_ms)
    } else {
        (max_abs, max_signed)
    }
}

// ── Layout ──────────────────────────────────────────────────────────────

/// Where the block sits and how its fields are joined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// The original shape: one field per line, a column beside each side's
    /// playfield.
    Vertical = 0,
    /// Every field on ONE line along the bottom edge of the screen, in the
    /// band the stock CREDIT / PASELI / ONLINE text occupies (that text is
    /// hidden through `services::bottom_text` while this layout's widgets
    /// are on screen).
    Horizontal = 1,
}

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Vertical, Layout::Horizontal];
    pub const DEFAULT: Layout = Layout::Vertical;

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Horizontal,
            _ => Self::Vertical,
        }
    }

    /// Config key (`power_user_statistics.widget_layout`).
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Overlay-row label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Vertical => "SIDE COLUMN",
            Self::Horizontal => "BOTTOM LINE",
        }
    }

    /// Prefix for the layout-scoped rows' labels ("<prefix> H-Offset (px)").
    pub fn row_prefix(self) -> &'static str {
        match self {
            Self::Vertical => "Side Column",
            Self::Horizontal => "Bottom Line",
        }
    }

    /// Field separator: one field per line, or a two-space gap on the line.
    pub fn separator(self) -> &'static str {
        match self {
            Self::Vertical => "\n",
            Self::Horizontal => "  ",
        }
    }

    pub fn geometry(self) -> &'static Geometry {
        match self {
            Self::Vertical => &VERTICAL_GEOMETRY,
            Self::Horizontal => &HORIZONTAL_GEOMETRY,
        }
    }
}

/// Per-layout anchor + offset-row ranges. Offsets are px on the 1280×720
/// canvas: horizontal POSITIVE = inward (toward the screen centre) on BOTH
/// sides — the one mirrored value moves the two blocks symmetrically;
/// vertical POSITIVE = down. Each range lets the anchor reach the whole
/// canvas on its axis; the two blocks crossing past the centre is the
/// user's call. Partially off-screen placements are allowed rather than
/// second-guessing the rendered extent, which depends on the scale row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geometry {
    /// P1's anchor x; P2 mirrors about the canvas centre (`CANVAS_W − x`).
    pub p1_anchor_x: f32,
    /// Both sides' anchor y (top of the text block).
    pub base_y: f32,
    pub offset_x_min: i32,
    pub offset_x_max: i32,
    pub offset_y_min: i32,
    pub offset_y_max: i32,
    /// Whether the layout's block alignment is the operator's to choose.
    pub alignment: AlignmentRule,
}

/// How a layout's [`BlockAlignment`] is decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignmentRule {
    /// Operator-configurable (overlay row + config key), with this default.
    /// A multi-line block has a real choice here: its lines can hang off the
    /// anchor toward either edge or straddle it.
    Configurable(BlockAlignment),
    /// Baked in — no row, no config key. A single line has nothing to align
    /// about: the anchor already IS the line's edge, so an alignment choice
    /// would only be a second way to spell the H-offset.
    Fixed(BlockAlignment),
}

/// The alignment a layout actually uses: its fixed value, or the
/// operator's `configured` choice when the rule allows one.
pub fn resolve_alignment(layout: Layout, configured: BlockAlignment) -> BlockAlignment {
    match layout.geometry().alignment {
        AlignmentRule::Fixed(a) => a,
        AlignmentRule::Configurable(_) => configured,
    }
}

/// Side column: block centred 80 px in from each edge, top at y = 425. The
/// native renderer centres each `\n` line about the anchor x independently,
/// so CENTER gives a centre-aligned column.
pub const VERTICAL_GEOMETRY: Geometry = Geometry {
    p1_anchor_x: 80.0,
    base_y: 425.0,
    offset_x_min: -80,
    offset_x_max: 600,
    offset_y_min: -425,
    offset_y_max: 295,
    alignment: AlignmentRule::Configurable(BlockAlignment::Center),
};

/// Bottom line: the stock corner text's spots — P1 `x = 10` left-aligned,
/// P2 `x = screen_w − 10` right-aligned, `y = screen_h − 20` (the system
/// HUD tick's creator, `docs/hex_edit_porting.md` Hack 3 table) — so the
/// line lands exactly where the PASELI readouts it replaces were. OUTER
/// EDGE is baked in: P1's line grows rightward from its anchor, P2's
/// leftward, and the H-offset row alone places it anywhere on the row.
pub const HORIZONTAL_GEOMETRY: Geometry = Geometry {
    p1_anchor_x: 10.0,
    base_y: CANVAS_H - 20.0,
    offset_x_min: -10,
    offset_x_max: 1000,
    offset_y_min: -700,
    offset_y_max: 20,
    alignment: AlignmentRule::Fixed(BlockAlignment::Outer),
};

/// Resolved anchor `(x, y)` for `side` (0 = P1, inward = +x; 1 = P2,
/// inward = −x) from the layout's geometry and the layout's live offsets.
pub fn anchor(layout: Layout, side: usize, dx: i32, dy: i32) -> (f32, f32) {
    let g = layout.geometry();
    let (base_x, inward) = if side == 0 {
        (g.p1_anchor_x, 1.0)
    } else {
        (CANVAS_W - g.p1_anchor_x, -1.0)
    };
    (base_x + inward * dx as f32, g.base_y + dy as f32)
}

// ── Alignment ───────────────────────────────────────────────────────────

/// How each side's lines align about its anchor x. The anchor itself does
/// NOT move with the alignment — OUTER/INNER hang the lines off the same x
/// toward the edge / the centre, and the H-offset row repositions the anchor
/// if wanted. Mirrored per side so the two blocks always read as the same
/// layout. Per-LAYOUT: see [`AlignmentRule`] — the side column's is the
/// operator's choice, the bottom line's is baked in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockAlignment {
    /// Every line centred about the anchor (both sides).
    Center = 0,
    /// Flush toward the screen edges: P1 left-aligned, P2 right-aligned.
    Outer = 1,
    /// Biased toward screen centre: P1 right-aligned, P2 left-aligned.
    Inner = 2,
}

/// Native per-line horizontal alignment (the widget maps this onto
/// `TextAlignment`, whose discriminants match).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineAlign {
    Left = 0,
    Center = 1,
    Right = 2,
}

impl BlockAlignment {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Outer,
            2 => Self::Inner,
            _ => Self::Center,
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "center" => Some(Self::Center),
            "outer" => Some(Self::Outer),
            "inner" => Some(Self::Inner),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Outer => "outer",
            Self::Inner => "inner",
        }
    }

    pub fn index(self) -> i32 {
        self as i32
    }

    /// Per-line alignment for `side` (0 = P1, 1 = P2).
    pub fn line_align(self, side: usize) -> LineAlign {
        match (self, side) {
            (Self::Center, _) => LineAlign::Center,
            (Self::Outer, 0) | (Self::Inner, 1) => LineAlign::Left,
            _ => LineAlign::Right,
        }
    }
}

// ── Content ─────────────────────────────────────────────────────────────

/// Which fields the block shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Content {
    /// EX loss, current / max / abs-mean / mean ms error, calories.
    Detailed = 0,
    /// Δ, Max Δ, EX loss, then the per-grade tallies (S-Marv when the
    /// S-Marvelous mod is enabled, Marv, Perfect, Great, Good, Miss).
    Streamlined = 1,
}

impl Content {
    pub const DEFAULT: Content = Content::Detailed;

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Streamlined,
            _ => Self::Detailed,
        }
    }

    /// Config key (`power_user_statistics.widget_content`).
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "detailed" => Some(Self::Detailed),
            "streamlined" => Some(Self::Streamlined),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Detailed => "detailed",
            Self::Streamlined => "streamlined",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Detailed => "DETAILED",
            Self::Streamlined => "STREAMLINED",
        }
    }
}

/// One side's per-song statistics, copied out of the feed's accumulator
/// under its lock. Every ms field is in the CAPTURED sign (`actual −
/// expected`); [`compose`] applies the display convention.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Snapshot {
    /// Latest timed step's error.
    pub current_ms: i32,
    /// Largest |error| so far.
    pub max_abs_ms: i32,
    /// The SIGNED error of the step that set `max_abs_ms`.
    pub max_signed_ms: i32,
    pub sum_abs: i64,
    pub sum: i64,
    /// Timed samples (grades M..Boo).
    pub count: u32,
    /// Cumulative EX lost (3 − EX earned per step).
    pub ex_loss: i32,
    /// Per-grade tallies indexed by engine grade (see `GRADE_*`).
    pub grade_counts: [u32; GRADE_COUNT],
    /// S-Marvelous hits this song — `Some` only while the S-Marvelous mod is
    /// enabled (the field is omitted otherwise, and Marv counts every
    /// grade-0 step).
    pub smarv: Option<u32>,
    /// Live calories burned this song (kcal).
    pub kcal: f32,
}

/// Compose the block text for one side. Fields are joined with the
/// layout's separator; the DETAILED/vertical output is byte-identical to
/// the pre-layout widget text (`EX: -0\nCurrent: +0ms\n…`).
pub fn compose(content: Content, layout: Layout, s: &Snapshot) -> String {
    let sep = layout.separator();
    let mut out = String::with_capacity(160);
    let mut field = |args: std::fmt::Arguments| {
        if !out.is_empty() {
            out.push_str(sep);
        }
        let _ = out.write_fmt(args);
    };
    match content {
        Content::Detailed => {
            let current = display_ms(s.current_ms) as f64;
            let (abs_mean, mean) = if s.count > 0 {
                (
                    s.sum_abs as f64 / s.count as f64,
                    display_ms_f64(s.sum as f64 / s.count as f64),
                )
            } else {
                (0.0, 0.0)
            };
            field(format_args!("EX: -{}", s.ex_loss));
            field(format_args!("Current: {:+.0}ms", current));
            field(format_args!("Max: {:.0}ms", s.max_abs_ms));
            field(format_args!("Abs(μ): {:.2}ms", abs_mean));
            field(format_args!("μ: {:+.2}ms", mean));
            field(format_args!("Cal: {:.2}", s.kcal));
        }
        Content::Streamlined => {
            let g = &s.grade_counts;
            field(format_args!("Δ: {:+}ms", display_ms(s.current_ms)));
            field(format_args!("Max Δ: {:+}ms", display_ms(s.max_signed_ms)));
            field(format_args!("EX: -{}", s.ex_loss));
            // Marv is EXCLUSIVE of S-Marv when the tier is shown (the
            // S-Marvelous results tab / wire convention): S-Marv + Marv =
            // every grade-0 step.
            let marv = match s.smarv {
                Some(smarv) => {
                    field(format_args!("S-Marv: {}", smarv));
                    g[GRADE_MARVELOUS].saturating_sub(smarv)
                }
                None => g[GRADE_MARVELOUS],
            };
            field(format_args!("Marv: {}", marv));
            field(format_args!("Perfect: {}", g[GRADE_PERFECT]));
            field(format_args!("Great: {}", g[GRADE_GREAT]));
            field(format_args!("Good: {}", g[GRADE_GOOD]));
            field(format_args!(
                "Miss: {}",
                g[GRADE_MISS].saturating_add(g[GRADE_BOO])
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_sign_flips_and_keeps_zero_positive() {
        assert_eq!(display_ms(-12), 12);
        assert_eq!(display_ms(7), -7);
        assert_eq!(display_ms(0), 0);
        assert_eq!(format!("{:+.2}", display_ms_f64(0.0)), "+0.00");
        assert_eq!(format!("{:+.2}", display_ms_f64(-1.5)), "+1.50");
    }

    #[test]
    fn update_max_tracks_magnitude_and_keeps_sign() {
        let (a, s) = update_max(0, 0, -12);
        assert_eq!((a, s), (12, -12));
        // Smaller magnitude: unchanged.
        assert_eq!(update_max(a, s, 5), (12, -12));
        // Larger magnitude of the opposite sign wins and carries its sign.
        assert_eq!(update_max(a, s, 30), (30, 30));
        // Ties keep the earlier step.
        assert_eq!(update_max(30, 30, -30), (30, 30));
    }

    #[test]
    fn layout_keys_round_trip() {
        for l in Layout::ALL {
            assert_eq!(Layout::from_key(l.key()), Some(l));
            assert_eq!(Layout::from_index(l.index() as i32), l);
        }
        assert_eq!(Layout::from_key("sideways"), None);
        assert_eq!(Layout::from_index(99), Layout::Vertical);
        assert_eq!(Layout::DEFAULT, Layout::Vertical);
    }

    #[test]
    fn content_keys_round_trip() {
        for c in [Content::Detailed, Content::Streamlined] {
            assert_eq!(Content::from_key(c.key()), Some(c));
            assert_eq!(Content::from_index(c as i32), c);
        }
        assert_eq!(Content::from_key("compact"), None);
        assert_eq!(Content::DEFAULT, Content::Detailed);
    }

    #[test]
    fn alignment_keys_round_trip_and_mirror() {
        for a in [
            BlockAlignment::Center,
            BlockAlignment::Outer,
            BlockAlignment::Inner,
        ] {
            assert_eq!(BlockAlignment::from_key(a.key()), Some(a));
            assert_eq!(BlockAlignment::from_index(a.index()), a);
        }
        assert_eq!(BlockAlignment::from_key("left"), None);
        assert_eq!(BlockAlignment::Center.line_align(0), LineAlign::Center);
        assert_eq!(BlockAlignment::Center.line_align(1), LineAlign::Center);
        assert_eq!(BlockAlignment::Outer.line_align(0), LineAlign::Left);
        assert_eq!(BlockAlignment::Outer.line_align(1), LineAlign::Right);
        assert_eq!(BlockAlignment::Inner.line_align(0), LineAlign::Right);
        assert_eq!(BlockAlignment::Inner.line_align(1), LineAlign::Left);
    }

    #[test]
    fn anchors_mirror_about_the_canvas_centre() {
        for l in Layout::ALL {
            let (x1, y1) = anchor(l, 0, 0, 0);
            let (x2, y2) = anchor(l, 1, 0, 0);
            assert_eq!(x1 + x2, CANVAS_W, "{:?}", l);
            assert_eq!(y1, y2);
            assert_eq!(y1, l.geometry().base_y);
            // Inward offset: P1 moves +x, P2 moves −x; vertical is shared.
            let (x1d, y1d) = anchor(l, 0, 40, -30);
            let (x2d, y2d) = anchor(l, 1, 40, -30);
            assert_eq!(x1d, x1 + 40.0);
            assert_eq!(x2d, x2 - 40.0);
            assert_eq!(y1d, y1 - 30.0);
            assert_eq!(y2d, y1d);
        }
    }

    #[test]
    fn vertical_geometry_matches_the_legacy_column() {
        let g = Layout::Vertical.geometry();
        assert_eq!(anchor(Layout::Vertical, 0, 0, 0), (80.0, 425.0));
        assert_eq!(anchor(Layout::Vertical, 1, 0, 0), (1200.0, 425.0));
        assert_eq!((g.offset_x_min, g.offset_x_max), (-80, 600));
        assert_eq!((g.offset_y_min, g.offset_y_max), (-425, 295));
        assert_eq!(
            g.alignment,
            AlignmentRule::Configurable(BlockAlignment::Center)
        );
        assert_eq!(Layout::Vertical.separator(), "\n");
    }

    #[test]
    fn horizontal_geometry_sits_on_the_stock_bottom_text_spots() {
        let g = Layout::Horizontal.geometry();
        // Stock creator: P1 PASELI at (10, screen_h − 20), P2 at
        // (screen_w − 10, screen_h − 20).
        assert_eq!(anchor(Layout::Horizontal, 0, 0, 0), (10.0, 700.0));
        assert_eq!(anchor(Layout::Horizontal, 1, 0, 0), (1270.0, 700.0));
        assert_eq!(g.alignment, AlignmentRule::Fixed(BlockAlignment::Outer));
        assert_eq!(Layout::Horizontal.separator(), "  ");
    }

    #[test]
    fn alignment_resolution_honours_the_rule() {
        for configured in [
            BlockAlignment::Center,
            BlockAlignment::Outer,
            BlockAlignment::Inner,
        ] {
            // The side column is the operator's choice …
            assert_eq!(resolve_alignment(Layout::Vertical, configured), configured);
            // … the bottom line ignores any configured value: OUTER EDGE
            // is baked in (P1 grows from the left, P2 ends at the right).
            assert_eq!(
                resolve_alignment(Layout::Horizontal, configured),
                BlockAlignment::Outer
            );
        }
    }

    #[test]
    fn offset_ranges_reach_the_canvas_edges() {
        for l in Layout::ALL {
            let g = l.geometry();
            let (x_lo, _) = anchor(l, 0, g.offset_x_min, 0);
            let (_, y_lo) = anchor(l, 0, 0, g.offset_y_min);
            let (_, y_hi) = anchor(l, 0, 0, g.offset_y_max);
            assert_eq!(x_lo, 0.0, "{:?} outward limit reaches the edge", l);
            assert_eq!(y_lo, 0.0, "{:?} top limit", l);
            assert_eq!(y_hi, CANVAS_H, "{:?} bottom limit", l);
            assert!(g.offset_x_min < 0 && g.offset_x_max > 0);
        }
        // The bottom line's anchor can be dragged to the canvas centre (and
        // past it) by a solo player.
        let (x, _) = anchor(Layout::Horizontal, 0, 630, 0);
        assert_eq!(x, CANVAS_W / 2.0);
    }

    fn sample() -> Snapshot {
        let mut g = [0u32; GRADE_COUNT];
        g[GRADE_MARVELOUS] = 40;
        g[GRADE_PERFECT] = 7;
        g[GRADE_GREAT] = 3;
        g[GRADE_GOOD] = 2;
        g[GRADE_BOO] = 1;
        g[GRADE_MISS] = 4;
        g[GRADE_OK] = 6;
        Snapshot {
            current_ms: -12, // 12 ms EARLY → displays +12
            max_abs_ms: 95,
            max_signed_ms: 95, // the worst step was LATE → displays −95
            sum_abs: 300,
            sum: -150,
            count: 50,
            ex_loss: 23,
            grade_counts: g,
            smarv: None,
            kcal: 12.345,
        }
    }

    #[test]
    fn detailed_vertical_is_byte_identical_to_the_legacy_text() {
        assert_eq!(
            compose(Content::Detailed, Layout::Vertical, &Snapshot::default()),
            "EX: -0\nCurrent: +0ms\nMax: 0ms\nAbs(μ): 0.00ms\nμ: +0.00ms\nCal: 0.00"
        );
        assert_eq!(
            compose(Content::Detailed, Layout::Vertical, &sample()),
            "EX: -23\nCurrent: +12ms\nMax: 95ms\nAbs(μ): 6.00ms\nμ: +3.00ms\nCal: 12.35"
        );
    }

    #[test]
    fn detailed_horizontal_joins_with_the_gap_and_has_no_newline() {
        let text = compose(Content::Detailed, Layout::Horizontal, &sample());
        assert_eq!(
            text,
            "EX: -23  Current: +12ms  Max: 95ms  Abs(μ): 6.00ms  μ: +3.00ms  Cal: 12.35"
        );
        assert!(!text.contains('\n'));
    }

    #[test]
    fn streamlined_without_smarv_counts_every_marvelous() {
        assert_eq!(
            compose(Content::Streamlined, Layout::Vertical, &sample()),
            "Δ: +12ms\nMax Δ: -95ms\nEX: -23\nMarv: 40\nPerfect: 7\nGreat: 3\nGood: 2\nMiss: 5"
        );
    }

    #[test]
    fn streamlined_with_smarv_shows_the_tier_and_an_exclusive_marv() {
        let mut s = sample();
        s.smarv = Some(31);
        assert_eq!(
            compose(Content::Streamlined, Layout::Horizontal, &s),
            "Δ: +12ms  Max Δ: -95ms  EX: -23  S-Marv: 31  Marv: 9  Perfect: 7  Great: 3  Good: 2  Miss: 5"
        );
        // A stale S-Marv count above the grade-0 tally can never underflow.
        s.smarv = Some(41);
        assert!(compose(Content::Streamlined, Layout::Vertical, &s).contains("\nMarv: 0\n"));
    }

    #[test]
    fn streamlined_zero_state_reads_as_the_user_specified() {
        let text = compose(Content::Streamlined, Layout::Vertical, &Snapshot::default());
        assert!(text.starts_with("Δ: +0ms\nMax Δ: +0ms\nEX: -0\nMarv: 0\n"));
        assert!(text.ends_with("Miss: 0"));
    }

    #[test]
    fn streamlined_max_delta_keeps_the_sign_of_the_worst_step() {
        let mut s = Snapshot::default();
        s.max_abs_ms = 40;
        s.max_signed_ms = -40; // early
        assert!(compose(Content::Streamlined, Layout::Vertical, &s).contains("Max Δ: +40ms"));
        s.max_signed_ms = 40; // late
        assert!(compose(Content::Streamlined, Layout::Vertical, &s).contains("Max Δ: -40ms"));
    }
}
