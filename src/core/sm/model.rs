//! Format-independent song model shared by the simfile parser and the
//! SSQ writer.
//!
//! Musical positions (`Beat`) and tempo values are exact rationals —
//! floats drift, rationals round-trip losslessly, and the SSQ writer's
//! byte identity with `ddr-chart-tools` depends on the same exact
//! arithmetic (`Rational`, `Beat`, `Bpm` are ported verbatim from the
//! tool's `src/model/{mod,rational}.rs`). The additions over the tool's
//! model are the fields the runtime needs and the tool dropped:
//! `#DELAYS`/`#WARPS`/`#FAKES`, per-chart timing, `#METER`, lifts/fakes/
//! rolls as distinct kinds, song metadata tags, per-item warnings, and
//! the display-only `#SCROLLS`/`#SPEEDS` tables (`scroll.rs`) — kept
//! outside `Timing` on purpose: they never affect judgement, and the SSQ
//! writer groups charts by `Timing` equality.

use std::cmp::Ordering;
use std::fmt;
use std::num::NonZeroU64;

use super::scroll::{ScrollEntry, SpeedEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RationalError {
    ZeroDenominator,
    Overflow,
}

impl fmt::Display for RationalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDenominator => f.write_str("denominator cannot be zero"),
            Self::Overflow => f.write_str("arithmetic overflow"),
        }
    }
}

/// A rational number `num / den` in lowest terms. The denominator is
/// always positive; the numerator carries the sign.
#[derive(Debug, Clone, Copy)]
pub struct Rational {
    num: i64,
    den: NonZeroU64,
}

impl Rational {
    /// Construct from integer numerator and denominator, reducing to
    /// lowest terms. Rejects zero denominators.
    pub fn new(num: i64, den: i64) -> Result<Self, RationalError> {
        if den == 0 {
            return Err(RationalError::ZeroDenominator);
        }
        let (num, den_u) = if den < 0 {
            (
                num.checked_neg().ok_or(RationalError::Overflow)?,
                den.unsigned_abs(),
            )
        } else {
            (num, den as u64)
        };
        let g = gcd(num.unsigned_abs(), den_u);
        let num = num / (g as i64);
        // `den_u / g >= 1` because `den_u != 0` and `g` divides it.
        let den = NonZeroU64::new(den_u / g).ok_or(RationalError::ZeroDenominator)?;
        Ok(Self { num, den })
    }

    #[must_use]
    pub const fn from_integer(n: i64) -> Self {
        Self {
            num: n,
            den: NonZeroU64::MIN,
        }
    }

    #[must_use]
    pub const fn zero() -> Self {
        Self::from_integer(0)
    }

    #[must_use]
    pub const fn num(&self) -> i64 {
        self.num
    }

    #[must_use]
    pub const fn den(&self) -> u64 {
        self.den.get()
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.num == 0
    }

    #[must_use]
    pub fn is_negative(&self) -> bool {
        self.num < 0
    }

    /// Convert to `f64`. Lossy for numerators/denominators near `i64::MAX`.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        (self.num as f64) / (self.den.get() as f64)
    }

    pub fn add(&self, other: &Self) -> Result<Self, RationalError> {
        let bd = (self.den.get() as i128)
            .checked_mul(other.den.get() as i128)
            .ok_or(RationalError::Overflow)?;
        let ad = (self.num as i128)
            .checked_mul(other.den.get() as i128)
            .ok_or(RationalError::Overflow)?;
        let cb = (other.num as i128)
            .checked_mul(self.den.get() as i128)
            .ok_or(RationalError::Overflow)?;
        let num = ad.checked_add(cb).ok_or(RationalError::Overflow)?;
        Self::from_i128_ratio(num, bd)
    }

    pub fn sub(&self, other: &Self) -> Result<Self, RationalError> {
        self.add(&other.neg()?)
    }

    /// Additive inverse. Fails only for `i64::MIN / den`.
    pub fn neg(&self) -> Result<Self, RationalError> {
        Ok(Self {
            num: self.num.checked_neg().ok_or(RationalError::Overflow)?,
            den: self.den,
        })
    }

    pub fn mul(&self, other: &Self) -> Result<Self, RationalError> {
        let num = (self.num as i128)
            .checked_mul(other.num as i128)
            .ok_or(RationalError::Overflow)?;
        let den = (self.den.get() as i128)
            .checked_mul(other.den.get() as i128)
            .ok_or(RationalError::Overflow)?;
        Self::from_i128_ratio(num, den)
    }

    pub fn div(&self, other: &Self) -> Result<Self, RationalError> {
        if other.num == 0 {
            return Err(RationalError::ZeroDenominator);
        }
        let num = (self.num as i128)
            .checked_mul(other.den.get() as i128)
            .ok_or(RationalError::Overflow)?;
        let den = (self.den.get() as i128)
            .checked_mul(other.num as i128)
            .ok_or(RationalError::Overflow)?;
        Self::from_i128_ratio(num, den)
    }

    fn from_i128_ratio(num: i128, den: i128) -> Result<Self, RationalError> {
        if den == 0 {
            return Err(RationalError::ZeroDenominator);
        }
        let (num, den) = if den < 0 {
            (
                num.checked_neg().ok_or(RationalError::Overflow)?,
                den.checked_neg().ok_or(RationalError::Overflow)?,
            )
        } else {
            (num, den)
        };
        let g = gcd_u128(num.unsigned_abs(), den as u128);
        let num = num / (g as i128);
        let den = (den as u128) / g;
        let num_i64 = i64::try_from(num).map_err(|_| RationalError::Overflow)?;
        let den_u64 = u64::try_from(den).map_err(|_| RationalError::Overflow)?;
        let den_nz = NonZeroU64::new(den_u64).ok_or(RationalError::ZeroDenominator)?;
        Ok(Self {
            num: num_i64,
            den: den_nz,
        })
    }
}

impl Default for Rational {
    fn default() -> Self {
        Self::zero()
    }
}

impl PartialEq for Rational {
    fn eq(&self, other: &Self) -> bool {
        self.num == other.num && self.den == other.den
    }
}

impl Eq for Rational {}

impl std::hash::Hash for Rational {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.num.hash(state);
        self.den.hash(state);
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        let lhs = (self.num as i128) * (other.den.get() as i128);
        let rhs = (other.num as i128) * (self.den.get() as i128);
        lhs.cmp(&rhs)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den.get() == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    if a == 0 {
        return b.max(1);
    }
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    if a == 0 {
        return b.max(1);
    }
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a.max(1)
}

/// A musical position in beats. 1 beat == 1024 SSQ measure ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Beat(Rational);

impl Beat {
    /// SSQ measure-tick convention: 4096 ticks per whole note == 1024 per beat.
    pub const TICKS_PER_BEAT: i64 = 1024;

    #[must_use]
    pub const fn zero() -> Self {
        Self(Rational::zero())
    }

    #[must_use]
    pub const fn from_rational(r: Rational) -> Self {
        Self(r)
    }

    #[must_use]
    pub const fn from_integer(beats: i64) -> Self {
        Self(Rational::from_integer(beats))
    }

    /// Construct from an SSQ measure-tick offset.
    pub fn from_measure_ticks(ticks: i64) -> Result<Self, RationalError> {
        Rational::new(ticks, Self::TICKS_PER_BEAT).map(Self)
    }

    #[must_use]
    pub const fn as_rational(&self) -> Rational {
        self.0
    }

    /// Integer measure-tick position (1024 per beat), rounded half away
    /// from zero — the rounding every SSQ emitter in this crate and in
    /// `ddr-chart-tools` uses (`writer.rs::beat_to_measure_ticks`,
    /// `mines.rs::beat_to_measure_ticks_i32`, `job::chart_end_tick`).
    /// `None` on overflow or when the result does not fit an `i32`.
    #[must_use]
    pub fn to_measure_ticks(&self) -> Option<i32> {
        let r = self.0;
        let num = (r.num() as i128).checked_mul(Self::TICKS_PER_BEAT as i128)?;
        i32::try_from(div_round(num, r.den() as i128)).ok()
    }
}

/// Divide, rounding half away from zero. `den` must be positive.
#[must_use]
pub fn div_round(num: i128, den: i128) -> i128 {
    let half = den / 2;
    if num >= 0 {
        (num + half) / den
    } else {
        (num - half) / den
    }
}

/// Tempo in beats per minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bpm(Rational);

impl Bpm {
    #[must_use]
    pub const fn from_rational(r: Rational) -> Self {
        Self(r)
    }

    #[must_use]
    pub const fn as_rational(&self) -> Rational {
        self.0
    }
}

/// Play style — determines how many panels are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
    /// Four panels (L, D, U, R).
    Single,
    /// Eight panels (P1 L/D/U/R, P2 L/D/U/R).
    Double,
}

impl Style {
    #[must_use]
    pub const fn panel_count(self) -> u8 {
        match self {
            Self::Single => 4,
            Self::Double => 8,
        }
    }
}

/// Chart difficulty slot. `Edit` is parsed (StepMania allows any number
/// of them) and dropped by the SSQ writer's `edit_policy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Difficulty {
    Beginner,
    Basic,
    Difficult,
    Expert,
    Challenge,
    Edit,
}

impl Difficulty {
    /// DDR level index 0..4 (`<b>_<N>.ssq` uses `N = index + 1`); `None`
    /// for `Edit`.
    #[must_use]
    pub const fn level_index(self) -> Option<u8> {
        match self {
            Self::Beginner => Some(0),
            Self::Basic => Some(1),
            Self::Difficult => Some(2),
            Self::Expert => Some(3),
            Self::Challenge => Some(4),
            Self::Edit => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Beginner => "Beginner",
            Self::Basic => "Basic",
            Self::Difficult => "Difficult",
            Self::Expert => "Expert",
            Self::Challenge => "Challenge",
            Self::Edit => "Edit",
        }
    }
}

/// Set of active panels for a single note, stored as a bitmask
/// (`docs/ssq_format.md` §5.3 bit layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PanelSet(u8);

impl PanelSet {
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Construct from a raw bitmask. Bits above `style.panel_count()` are masked off.
    #[must_use]
    pub fn from_bits(style: Style, bits: u8) -> Self {
        let mask = (1u16 << style.panel_count()) - 1;
        Self(bits & (mask as u8))
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn contains(self, panel: u8) -> bool {
        panel < 8 && (self.0 & (1u8 << panel)) != 0
    }

    #[must_use]
    pub const fn with(self, panel: u8) -> Self {
        if panel < 8 {
            Self(self.0 | (1u8 << panel))
        } else {
            self
        }
    }

    #[must_use]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    #[must_use]
    pub const fn from_bits_union(a: Self, b: Self) -> Self {
        Self(a.0 | b.0)
    }

    #[must_use]
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

/// Which side(s) a shock arrow affects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShockSide {
    BothSides,
    P1Only,
    P2Only,
}

/// One note at a specific beat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub beat: Beat,
    pub kind: NoteKind,
    /// Panels the note sits on. Empty for `Shock` (the side is in the kind).
    pub panels: PanelSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NoteKind {
    Tap,
    /// Freeze/hold start (`2`). `length` is the hold duration in beats.
    HoldHead {
        length: Beat,
    },
    /// Roll start (`4`). DDR has no rolls; the writer emits a freeze.
    RollHead {
        length: Beat,
    },
    /// Full-row mine pattern → DDR shock arrow.
    Shock {
        side: ShockSide,
    },
    /// Per-panel ITG-style mine (type-20 chunk). Panels ride on `Note.panels`.
    Mine,
    /// Lift (`L`). No DDR equivalent; dropped by the writer.
    Lift,
    /// Fake (`F`). Never judged; dropped by the writer.
    Fake,
}

/// Beat↔time map of a song or of one chart (SSC split timing).
///
/// Values are kept in StepMania's own conventions: `offset_s` is the
/// authored `#OFFSET` (beat 0 occurs at music time `−offset_s`; the SSQ
/// writer flips the sign), stop/delay durations are seconds, warps and
/// fakes are `(start_beat, length_beats)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timing {
    pub bpms: Vec<(Beat, Bpm)>,
    pub stops: Vec<(Beat, Rational)>,
    pub delays: Vec<(Beat, Rational)>,
    pub warps: Vec<(Beat, Beat)>,
    pub fakes: Vec<(Beat, Beat)>,
    pub offset_s: Rational,
}

impl Timing {
    /// True when the map contains something DDR's tempo chunk cannot
    /// express: a warp of positive length, a non-positive BPM, or a
    /// negative stop/delay. Reported by the writer as `Unrepresentable`.
    #[must_use]
    pub fn has_gimmicks(&self) -> bool {
        self.warps
            .iter()
            .any(|(_, len)| len.as_rational().num() > 0)
            || self
                .bpms
                .iter()
                .any(|(_, bpm)| bpm.as_rational().num() <= 0)
            || self.stops.iter().any(|(_, d)| d.is_negative())
            || self.delays.iter().any(|(_, d)| d.is_negative())
    }
}

/// A non-fatal parse observation. Warnings are returned in the model,
/// never logged, so the pure parser stays host-testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning(pub String);

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One difficulty of one song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chart {
    pub style: Style,
    pub difficulty: Difficulty,
    /// `#METER` (0 when absent or unparsable).
    pub meter: u32,
    /// `#DESCRIPTION` / `#CHARTNAME` / `#CREDIT` (first non-empty), or
    /// the `.sm` `#NOTES` description field.
    pub description: String,
    /// Per-chart timing override (SSC split timing); `None` = song timing.
    pub timing: Option<Timing>,
    /// Per-chart `#SCROLLS` / `#SPEEDS` (StepMania keeps them with the
    /// chart's timing); `None` = the song's tables.
    pub scrolls: Option<Vec<ScrollEntry>>,
    pub speeds: Option<Vec<SpeedEntry>>,
    /// Notes sorted by beat; coincident same-kind notes merged.
    pub notes: Vec<Note>,
    pub warnings: Vec<Warning>,
}

impl Chart {
    /// Timing this chart is played with.
    #[must_use]
    pub fn effective_timing<'a>(&'a self, song: &'a Timing) -> &'a Timing {
        self.timing.as_ref().unwrap_or(song)
    }

    /// `#SCROLLS` this chart is drawn with.
    #[must_use]
    pub fn effective_scrolls<'a>(&'a self, song: &'a Song) -> &'a [ScrollEntry] {
        self.scrolls.as_deref().unwrap_or(&song.scrolls)
    }

    /// `#SPEEDS` this chart is drawn with.
    #[must_use]
    pub fn effective_speeds<'a>(&'a self, song: &'a Song) -> &'a [SpeedEntry] {
        self.speeds.as_deref().unwrap_or(&song.speeds)
    }

    /// `"dance-single Expert"` — the label rejection reasons carry.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", stepstype_name(self.style), self.difficulty.name())
    }
}

#[must_use]
pub const fn stepstype_name(style: Style) -> &'static str {
    match style {
        Style::Single => "dance-single",
        Style::Double => "dance-double",
    }
}

/// A chart the parser could not keep. The song survives; the reason is
/// recorded for the per-song log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedChart {
    /// `#STEPSTYPE` / `#DIFFICULTY` as authored (may be empty).
    pub stepstype: String,
    pub difficulty: String,
    pub reason: String,
}

/// One simfile's worth of metadata, timing and charts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Song {
    pub title: String,
    pub title_translit: String,
    pub subtitle: String,
    pub subtitle_translit: String,
    pub artist: String,
    pub artist_translit: String,
    /// `#MUSIC` as authored (relative path); empty when absent.
    pub music: String,
    pub banner: String,
    pub background: String,
    pub jacket: String,
    /// `#SELECTABLE` — false only for an explicit `NO`.
    pub selectable: bool,
    /// `#DISPLAYBPM` `(min, max)`; `None` when absent or `*`.
    pub display_bpm: Option<(f64, f64)>,
    pub timing: Timing,
    /// Song-level `#SCROLLS` / `#SPEEDS` (display-only scroll gimmicks).
    pub scrolls: Vec<ScrollEntry>,
    pub speeds: Vec<SpeedEntry>,
    /// `#SAMPLESTART` / `#SAMPLELENGTH` in seconds, when authored.
    pub sample_start_s: Option<f64>,
    pub sample_length_s: Option<f64>,
    pub charts: Vec<Chart>,
    pub rejected: Vec<RejectedChart>,
    pub warnings: Vec<Warning>,
}

impl Song {
    /// Authored `#OFFSET` in seconds (StepMania sign convention).
    #[must_use]
    pub fn offset_s(&self) -> Rational {
        self.timing.offset_s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(num: i64, den: i64) -> Rational {
        Rational::new(num, den).unwrap()
    }

    #[test]
    fn reduces_to_lowest_terms() {
        let x = r(4, 8);
        assert_eq!((x.num(), x.den()), (1, 2));
    }

    #[test]
    fn negative_denominator_flips_to_numerator() {
        let x = r(3, -4);
        assert_eq!((x.num(), x.den()), (-3, 4));
    }

    #[test]
    fn zero_denominator_is_rejected() {
        assert_eq!(Rational::new(1, 0), Err(RationalError::ZeroDenominator));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(r(1, 2).add(&r(1, 3)).unwrap(), r(5, 6));
        assert_eq!(r(1, 3).sub(&r(1, 2)).unwrap(), r(-1, 6));
        assert_eq!(r(2, 3).mul(&r(3, 4)).unwrap(), r(1, 2));
        assert_eq!(
            r(5, 7).mul(&r(3, 11)).unwrap().div(&r(3, 11)).unwrap(),
            r(5, 7)
        );
        assert_eq!(
            r(1, 2).div(&Rational::zero()),
            Err(RationalError::ZeroDenominator)
        );
        assert_eq!(
            Rational::from_integer(i64::MIN).neg(),
            Err(RationalError::Overflow)
        );
    }

    #[test]
    fn ordering_across_denominators() {
        assert!(r(1, 3) < r(1, 2));
        assert!(r(1, 2) < r(2, 3));
    }

    #[test]
    fn beat_ticks_round_half_away_from_zero() {
        assert_eq!(
            Beat::from_measure_ticks(4096).unwrap().to_measure_ticks(),
            Some(4096)
        );
        // 1/3 beat = 341.33 ticks → 341; 2/3 = 682.67 → 683.
        assert_eq!(Beat::from_rational(r(1, 3)).to_measure_ticks(), Some(341));
        assert_eq!(Beat::from_rational(r(2, 3)).to_measure_ticks(), Some(683));
        assert_eq!(Beat::from_rational(r(1, 2048)).to_measure_ticks(), Some(1));
        assert_eq!(
            Beat::from_rational(r(-1, 2048)).to_measure_ticks(),
            Some(-1)
        );
        assert_eq!(
            Beat::from_rational(Rational::from_integer(i64::MAX / 2)).to_measure_ticks(),
            None
        );
    }

    #[test]
    fn panelset_masks_by_style() {
        assert_eq!(PanelSet::from_bits(Style::Single, 0xFF).bits(), 0x0F);
        assert_eq!(PanelSet::from_bits(Style::Double, 0xFF).bits(), 0xFF);
        let ps = PanelSet::empty().with(0).with(3).with(9);
        assert!(ps.contains(0) && ps.contains(3) && !ps.contains(1));
        assert_eq!(ps.count(), 2);
    }

    #[test]
    fn timing_gimmick_detection() {
        let mut t = Timing::default();
        assert!(!t.has_gimmicks());
        t.warps.push((Beat::zero(), Beat::zero()));
        assert!(!t.has_gimmicks(), "zero-length warp is a no-op");
        t.warps.push((Beat::zero(), Beat::from_integer(1)));
        assert!(t.has_gimmicks());
        let mut t = Timing::default();
        t.bpms.push((Beat::zero(), Bpm::from_rational(r(-120, 1))));
        assert!(t.has_gimmicks());
        let mut t = Timing::default();
        t.stops.push((Beat::zero(), r(-1, 2)));
        assert!(t.has_gimmicks());
        let mut t = Timing::default();
        t.delays.push((Beat::zero(), r(1, 2)));
        assert!(!t.has_gimmicks());
    }

    #[test]
    fn difficulty_level_index() {
        assert_eq!(Difficulty::Beginner.level_index(), Some(0));
        assert_eq!(Difficulty::Challenge.level_index(), Some(4));
        assert_eq!(Difficulty::Edit.level_index(), None);
    }
}
