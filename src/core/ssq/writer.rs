//! SSQ synthesis from the `core::sm` model — pure, host-testable.
//!
//! Mechanism: `write_song` selects the charts DDR can declare, groups
//! them by effective timing into one or more SSQ files, and for each
//! file synthesises the tempo chunk (type 1), the canonical six-event
//! chunk (type 2), one step chunk (type 3) per chart, one mine chunk
//! (type 20, the modpack's extension read by `note_types_expansion`) per
//! chart with mines, and the zero-length terminator. `shift_seconds`
//! rewrites a finished file's tempo seconds column (auto-sync).
//!
//! Provenance: the tempo synthesis (i128 fixed point, `SUBTICKS_PER_TICK`),
//! step/freeze emission, chunk framing and mine chunk are ported
//! verbatim from `ddr-chart-tools` (a733404) `src/ssq/{writer,mines}.rs`;
//! `synthesize_events`, `chart_end_tick` and `extend_tempo_pairs_to`
//! from its private `src/job/mod.rs`. Load-bearing invariants carried
//! over: FINISH/END must be bracketed by tempo anchors (else the game
//! locks at READY); `tempo_data[0] = −#OFFSET·TPS`; a stop is two
//! anchors at one tick; the trailing anchor sits exactly at END.
//!
//! Fidelity policy: with `lead_in_ms = 0`, `sync_bias_ms = 0` and a song
//! that needs none of the runtime rules below, `files[0].bytes` is
//! byte-identical to the CLI's `.ssq` — `scripts/validate_stepmania.sh`
//! checks this over a generated corpus. The runtime rules (design §4.2):
//! - quads: a non-shock row whose side nibble is full (`0x0F`/`0xF0`)
//!   would read as a shock arrow, so `L+R` stays at *t* and `D+U` moves
//!   to *t+1* (`QuadPolicy::Split`; freeze heads move with their bits and
//!   the freeze-end entry carries both) or `D+U` is dropped
//!   (`ReduceToJump`);
//! - delays: a stop of the delay's duration at `tick(beat) − 1`, so the
//!   beat's notes are judged after it (a delay at beat 0 folds into the
//!   offset);
//! - `#FAKES` ranges drop the notes inside them; `Lift`/`Fake` are
//!   dropped; `RollHead` is a freeze;
//! - warps, non-positive BPMs and negative stops/delays make the chart
//!   `Unrepresentable` (`GimmickPolicy::HideChart`);
//! - per-chart timing: if every chart shares one timing → one `<b>.ssq`;
//!   else per level *N* a `<b>_N.ssq` with that level's single and double
//!   when they agree, else the single only (double `TimingConflict`);
//! - Edit charts dropped; duplicate (style, difficulty) → first kept;
//!   double Beginner dropped (slot 5 is never filled by stock data); a
//!   chart with no tap/jump/shock row is never declared (`ME1529`);
//! - `tempo_data[0] = −#OFFSET·1000 + lead_in_ms + sync_bias_ms`, and the
//!   same shift applies to every anchor.
//!
//! `WriteOptions::lead_in_ms` contract for the audio pipeline: the writer
//! shifts the whole chart later by exactly `lead_in_ms` milliseconds; the
//! caller must prepend exactly that much silence to the PCM it encodes
//! (in whole ADPCM blocks, which is why the caller — not the writer —
//! picks the value).

use std::collections::BTreeMap;

use crate::core::sm::model::{
    div_round, Beat, Bpm, Chart, Difficulty, NoteKind, Rational, ShockSide, Song, Style, Timing,
};

/// The only tick rate this writer emits (`docs/ssq_format.md` §3).
pub const TPS: u32 = 1000;
/// Measure ticks per whole note.
pub const TICKS_PER_MEASURE: i32 = 4096;

/// What to do with a chart whose timing DDR cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GimmickPolicy {
    #[default]
    HideChart,
}

/// What to do with a row that would read as a shock arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuadPolicy {
    /// `L+R` at *t*, `D+U` at *t+1* (per side).
    #[default]
    Split,
    /// Keep `L+R`, drop `D+U`.
    ReduceToJump,
}

/// What to do with `Edit` charts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditPolicy {
    #[default]
    Drop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteOptions {
    /// Milliseconds of silence the caller prepends to the audio; every
    /// tempo anchor is shifted later by this much.
    pub lead_in_ms: u32,
    /// Additional signed shift of every anchor (operator sync bias).
    pub sync_bias_ms: i32,
    pub gimmick_policy: GimmickPolicy,
    pub quad_policy: QuadPolicy,
    pub edit_policy: EditPolicy,
}

/// Why a chart was not declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    Edit,
    Duplicate,
    DoubleBeginner,
    Unrepresentable(String),
    TimingConflict,
    NoSteps,
    WriteFailed(String),
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Edit => f.write_str("Edit chart"),
            Self::Duplicate => f.write_str("duplicate style/difficulty; first kept"),
            Self::DoubleBeginner => f.write_str("double Beginner has no DDR slot"),
            Self::Unrepresentable(why) => write!(f, "unrepresentable timing: {why}"),
            Self::TimingConflict => {
                f.write_str("per-chart timing differs from the single chart of this level")
            }
            Self::NoSteps => f.write_str("no tap/jump/shock row"),
            Self::WriteFailed(why) => write!(f, "write failed: {why}"),
        }
    }
}

/// One SSQ file of a song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsqFile {
    /// `0` = `<b>.ssq`; `1..=5` = `<b>_<index>.ssq` (level = index − 1).
    pub index: u8,
    pub bytes: Vec<u8>,
    /// `(slot 0..9, level 1..19)` of every chart declared in this file.
    pub charts: Vec<(u8, u8)>,
}

/// Everything the musicdb entry and the resolver need for one song.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SsqBundle {
    pub files: Vec<SsqFile>,
    /// Level per musicdb slot (single Beginner..Challenge = 0..4, double
    /// Beginner..Challenge = 5..9); 0 = not declared.
    pub slots: [u8; 10],
    /// `(chart label, reason)` for every chart the song had but the
    /// bundle does not declare.
    pub rejects: Vec<(String, Reason)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// No chart survived; the rejects say why.
    NothingPlayable(Vec<(String, Reason)>),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingPlayable(r) => write!(f, "no playable chart ({} rejected)", r.len()),
        }
    }
}

// ---------------------------------------------------------------------
// Slot / difficulty-code mapping (`docs/ssq_format.md` §5.1)
// ---------------------------------------------------------------------

/// Musicdb slot index 0..9, or `None` for Edit.
#[must_use]
pub fn slot_index(style: Style, difficulty: Difficulty) -> Option<u8> {
    let level = difficulty.level_index()?;
    Some(match style {
        Style::Single => level,
        Style::Double => level + 5,
    })
}

/// Step-chunk `param2` for a slot index.
#[must_use]
pub fn slot_code(slot: u8) -> Option<u16> {
    if slot > 9 {
        return None;
    }
    let style_byte: u16 = if slot < 5 { 0x14 } else { 0x18 };
    let slot_byte: u16 = match slot % 5 {
        0 => 0x04,
        1 => 0x01,
        2 => 0x02,
        3 => 0x03,
        _ => 0x06,
    };
    Some((slot_byte << 8) | style_byte)
}

/// Step-chunk `param2` for a style/difficulty (`None` for Edit).
#[must_use]
pub fn difficulty_code(style: Style, difficulty: Difficulty) -> Option<u16> {
    slot_code(slot_index(style, difficulty)?)
}

/// Inverse of [`slot_code`].
#[must_use]
pub fn slot_of_code(code: u16) -> Option<u8> {
    (0u8..10).find(|&s| slot_code(s) == Some(code))
}

/// `#METER` → `diffLv` (design F9).
#[must_use]
pub fn level_of_meter(meter: u32) -> u8 {
    meter.clamp(1, 19) as u8
}

// ---------------------------------------------------------------------
// Song → bundle
// ---------------------------------------------------------------------

/// Convert a parsed song into its SSQ file(s).
pub fn write_song(song: &Song, opts: &WriteOptions) -> Result<SsqBundle, WriteError> {
    let mut rejects: Vec<(String, Reason)> = Vec::new();
    let mut selected: Vec<&Chart> = Vec::new();

    for chart in &song.charts {
        if chart.difficulty == Difficulty::Edit {
            match opts.edit_policy {
                EditPolicy::Drop => rejects.push((chart.label(), Reason::Edit)),
            }
            continue;
        }
        if chart.style == Style::Double && chart.difficulty == Difficulty::Beginner {
            rejects.push((chart.label(), Reason::DoubleBeginner));
            continue;
        }
        if selected
            .iter()
            .any(|c| c.style == chart.style && c.difficulty == chart.difficulty)
        {
            rejects.push((chart.label(), Reason::Duplicate));
            continue;
        }
        let timing = chart.effective_timing(&song.timing);
        if let Some(why) = unrepresentable_reason(timing) {
            match opts.gimmick_policy {
                GimmickPolicy::HideChart => {
                    rejects.push((chart.label(), Reason::Unrepresentable(why)));
                }
            }
            continue;
        }
        selected.push(chart);
    }

    // Group by effective timing.
    let mut groups: Vec<(u8, Vec<&Chart>)> = Vec::new();
    let all_same = selected
        .windows(2)
        .all(|w| w[0].effective_timing(&song.timing) == w[1].effective_timing(&song.timing));
    if all_same {
        if !selected.is_empty() {
            groups.push((0, selected.clone()));
        }
    } else {
        for level in 0u8..5 {
            let single = selected
                .iter()
                .copied()
                .find(|c| c.style == Style::Single && c.difficulty.level_index() == Some(level));
            let double = selected
                .iter()
                .copied()
                .find(|c| c.style == Style::Double && c.difficulty.level_index() == Some(level));
            let members: Vec<&Chart> = match (single, double) {
                (Some(s), Some(d)) => {
                    if s.effective_timing(&song.timing) == d.effective_timing(&song.timing) {
                        vec![s, d]
                    } else {
                        rejects.push((d.label(), Reason::TimingConflict));
                        vec![s]
                    }
                }
                (Some(c), None) | (None, Some(c)) => vec![c],
                (None, None) => continue,
            };
            groups.push((level + 1, members));
        }
    }

    let mut bundle = SsqBundle::default();
    for (index, members) in groups {
        let Some(first) = members.first() else {
            continue;
        };
        let timing = first.effective_timing(&song.timing);
        let mut prepared: Vec<PreparedChart> = Vec::new();
        for chart in &members {
            match prepare_chart(chart, timing, opts) {
                Ok(p) => prepared.push(p),
                Err(reason) => rejects.push((chart.label(), reason)),
            }
        }
        if prepared.is_empty() {
            continue;
        }
        match build_file(timing, &prepared, opts) {
            Ok(bytes) => {
                let charts: Vec<(u8, u8)> = prepared.iter().map(|p| (p.slot, p.level)).collect();
                for &(slot, level) in &charts {
                    if let Some(s) = bundle.slots.get_mut(usize::from(slot)) {
                        *s = level;
                    }
                }
                bundle.files.push(SsqFile {
                    index,
                    bytes,
                    charts,
                });
            }
            Err(why) => {
                for p in &prepared {
                    rejects.push((p.label.clone(), Reason::WriteFailed(why.clone())));
                }
            }
        }
    }

    bundle.rejects = rejects;
    if bundle.files.is_empty() {
        return Err(WriteError::NothingPlayable(bundle.rejects));
    }
    Ok(bundle)
}

fn unrepresentable_reason(timing: &Timing) -> Option<String> {
    if timing.bpms.is_empty() {
        return Some("no #BPMS".to_string());
    }
    if let Some((b, _)) = timing
        .warps
        .iter()
        .find(|(_, len)| len.as_rational().num() > 0)
    {
        return Some(format!("warp at beat {}", b.as_rational()));
    }
    if let Some((b, _)) = timing
        .bpms
        .iter()
        .find(|(_, bpm)| bpm.as_rational().num() <= 0)
    {
        return Some(format!("non-positive BPM at beat {}", b.as_rational()));
    }
    if let Some((b, _)) = timing.stops.iter().find(|(_, d)| d.is_negative()) {
        return Some(format!("negative stop at beat {}", b.as_rational()));
    }
    if let Some((b, _)) = timing.delays.iter().find(|(_, d)| d.is_negative()) {
        return Some(format!("negative delay at beat {}", b.as_rational()));
    }
    None
}

// ---------------------------------------------------------------------
// Per-chart preparation (notes → step rows / freezes / mines)
// ---------------------------------------------------------------------

/// A chart reduced to the integer rows the step and mine chunks carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChart {
    pub label: String,
    pub slot: u8,
    pub level: u8,
    pub code: u16,
    /// `(tick, step byte)` in file order, freeze-end markers included.
    pub steps: Vec<(i32, u8)>,
    /// `(panels, kind)` freeze entries, one per `0x00` step.
    pub freezes: Vec<(u8, u8)>,
    /// `(tick, panels)` mine entries, sorted by `(tick, panels)`.
    pub mines: Vec<(i32, u8)>,
    /// Largest tick any note (incl. freeze tails) reaches; `None` if empty.
    pub last_tick: Option<i32>,
    /// Rows the game counts as steps (taps/jumps/shocks; not hold-only rows).
    pub step_rows: usize,
}

#[derive(Default, Clone, Copy)]
struct RowAcc {
    bits: u8,
    shock: bool,
}

const SIDE_LR: [u8; 2] = [0x09, 0x90];
const SIDE_DU: [u8; 2] = [0x06, 0x60];
const SIDE_FULL: [u8; 2] = [0x0F, 0xF0];

/// Build the step/freeze/mine rows of one chart under `timing`.
pub fn prepare_chart(
    chart: &Chart,
    timing: &Timing,
    opts: &WriteOptions,
) -> Result<PreparedChart, Reason> {
    let slot = slot_index(chart.style, chart.difficulty).ok_or(Reason::Edit)?;
    let code = slot_code(slot).ok_or(Reason::Edit)?;
    let overflow = |what: &str| Reason::WriteFailed(format!("{what}: tick overflow"));

    let mut rows: BTreeMap<i32, RowAcc> = BTreeMap::new();
    // (head_tick, end_tick, bits)
    let mut holds: Vec<(i32, i32, u8)> = Vec::new();
    let mut mines: BTreeMap<i32, u8> = BTreeMap::new();
    let mut last_tick: Option<i32> = None;

    for note in &chart.notes {
        if matches!(note.kind, NoteKind::Lift | NoteKind::Fake) {
            continue;
        }
        if in_fake_range(timing, note.beat) {
            continue;
        }
        let tick = note
            .beat
            .to_measure_ticks()
            .ok_or_else(|| overflow("note"))?;
        last_tick = Some(last_tick.map_or(tick, |t| t.max(tick)));
        match &note.kind {
            NoteKind::Tap => rows.entry(tick).or_default().bits |= note.panels.bits(),
            NoteKind::Shock { side } => {
                let byte = match side {
                    ShockSide::BothSides => 0xFFu8,
                    ShockSide::P1Only => 0x0F,
                    ShockSide::P2Only => 0xF0,
                };
                let row = rows.entry(tick).or_default();
                row.bits |= byte;
                row.shock = true;
            }
            NoteKind::HoldHead { length } | NoteKind::RollHead { length } => {
                let end = note
                    .beat
                    .as_rational()
                    .add(&length.as_rational())
                    .map_err(|_| overflow("hold end"))?;
                let end_tick = Beat::from_rational(end)
                    .to_measure_ticks()
                    .ok_or_else(|| overflow("hold end"))?;
                last_tick = Some(last_tick.map_or(end_tick, |t| t.max(end_tick)));
                rows.entry(tick).or_default().bits |= note.panels.bits();
                holds.push((tick, end_tick, note.panels.bits()));
            }
            NoteKind::Mine => {
                if tick >= 0 && !note.panels.is_empty() {
                    *mines.entry(tick).or_insert(0) |= note.panels.bits();
                }
            }
            NoteKind::Lift | NoteKind::Fake => {}
        }
    }

    // Quad rule: a full side nibble on a non-shock row.
    let ticks: Vec<i32> = rows.keys().copied().collect();
    for t in ticks {
        let Some(row) = rows.get(&t).copied() else {
            continue;
        };
        if row.shock {
            continue;
        }
        let mut move_bits = 0u8;
        for side in 0..2 {
            if row.bits & SIDE_FULL[side] == SIDE_FULL[side] {
                move_bits |= SIDE_DU[side];
            }
        }
        if move_bits == 0 {
            continue;
        }
        if let Some(r) = rows.get_mut(&t) {
            r.bits &= !move_bits;
        }
        match opts.quad_policy {
            QuadPolicy::Split => {
                let next = t.checked_add(1).ok_or_else(|| overflow("quad split"))?;
                // Hold heads among the moved bits move with them; their
                // freeze end (keyed by end tick) is unchanged.
                let mut extra: Vec<(i32, i32, u8)> = Vec::new();
                for h in holds.iter_mut().filter(|h| h.0 == t) {
                    let moved = h.2 & move_bits;
                    if moved != 0 {
                        h.2 &= !move_bits;
                        extra.push((next, h.1, moved));
                    }
                }
                holds.retain(|h| h.2 != 0);
                holds.extend(extra);
                rows.entry(next).or_default().bits |= move_bits;
                last_tick = Some(last_tick.map_or(next, |l| l.max(next)));
            }
            QuadPolicy::ReduceToJump => {
                for h in holds.iter_mut().filter(|h| h.0 == t) {
                    h.2 &= !move_bits;
                }
                holds.retain(|h| h.2 != 0);
            }
        }
    }

    // Count judged rows: shocks, and rows with a panel that is not a
    // freeze head (holds-only rows do not count for `ME1529`).
    let mut head_bits: BTreeMap<i32, u8> = BTreeMap::new();
    for h in &holds {
        *head_bits.entry(h.0).or_insert(0) |= h.2;
    }
    let step_rows = rows
        .iter()
        .filter(|(t, r)| r.shock || (r.bits & !head_bits.get(t).copied().unwrap_or(0)) != 0)
        .count();
    if step_rows == 0 {
        return Err(Reason::NoSteps);
    }

    // Freeze ends: one `0x00` step per end tick, panels OR-ed.
    let mut freeze_ends: BTreeMap<i32, u8> = BTreeMap::new();
    for h in &holds {
        *freeze_ends.entry(h.1).or_insert(0) |= h.2;
    }

    // Merge walk (tool order): a row at tick T precedes a freeze end at T.
    let row_list: Vec<(i32, u8)> = rows
        .iter()
        .filter(|(_, r)| r.bits != 0)
        .map(|(t, r)| (*t, r.bits))
        .collect();
    let freeze_list: Vec<(i32, u8)> = freeze_ends.into_iter().collect();
    let mut steps: Vec<(i32, u8)> = Vec::with_capacity(row_list.len() + freeze_list.len());
    let mut freezes: Vec<(u8, u8)> = Vec::with_capacity(freeze_list.len());
    let (mut ri, mut fi) = (0usize, 0usize);
    while ri < row_list.len() || fi < freeze_list.len() {
        let take_row = match (row_list.get(ri), freeze_list.get(fi)) {
            (Some((rt, _)), Some((ft, _))) => rt <= ft,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_row {
            if let Some(r) = row_list.get(ri) {
                steps.push(*r);
            }
            ri += 1;
        } else {
            if let Some(f) = freeze_list.get(fi) {
                steps.push((f.0, 0x00));
                freezes.push((f.1, 0x01));
            }
            fi += 1;
        }
    }

    // Mines: shock-shaped masks are split into per-panel entries
    // (`docs/ssq_mine_chunk_format.md` §3.2).
    let mut mine_list: Vec<(i32, u8)> = Vec::with_capacity(mines.len());
    for (tick, mask) in mines {
        if matches!(mask, 0x0F | 0xF0 | 0xFF) {
            for bit in 0..8u32 {
                let b = 1u8 << bit;
                if mask & b != 0 {
                    mine_list.push((tick, b));
                }
            }
        } else {
            mine_list.push((tick, mask));
        }
    }

    Ok(PreparedChart {
        label: chart.label(),
        slot,
        level: level_of_meter(chart.meter),
        code,
        steps,
        freezes,
        mines: mine_list,
        last_tick,
        step_rows,
    })
}

fn in_fake_range(timing: &Timing, beat: Beat) -> bool {
    timing.fakes.iter().any(|(start, len)| {
        if len.as_rational().num() <= 0 || beat < *start {
            return false;
        }
        match start.as_rational().add(&len.as_rational()) {
            Ok(end) => beat.as_rational() < end,
            Err(_) => false,
        }
    })
}

// ---------------------------------------------------------------------
// File assembly
// ---------------------------------------------------------------------

/// Measure tick where END belongs: two whole measures past the measure
/// containing the last note (a hold counts to its tail). Charts with no
/// notes are treated as ending one measure in.
#[must_use]
pub fn chart_end_tick(charts: &[PreparedChart]) -> i32 {
    let last_tick = charts
        .iter()
        .filter_map(|c| c.last_tick)
        .max()
        .unwrap_or(TICKS_PER_MEASURE);
    let last_measure = last_tick
        .saturating_add(TICKS_PER_MEASURE - 1)
        .div_euclid(TICKS_PER_MEASURE)
        .saturating_mul(TICKS_PER_MEASURE);
    last_measure.saturating_add(2 * TICKS_PER_MEASURE)
}

/// Assemble one SSQ file from prepared charts that share `timing`.
pub fn build_file(
    timing: &Timing,
    charts: &[PreparedChart],
    opts: &WriteOptions,
) -> Result<Vec<u8>, String> {
    let end_tick = chart_end_tick(charts);
    let end_beat =
        Beat::from_measure_ticks(i64::from(end_tick)).map_err(|e| format!("end beat: {e}"))?;
    let initial = synthesize_tempo_entries_until(timing, Some(end_beat), opts)?;
    let (events, pairs) = synthesize_events(end_tick, &initial);

    let mut out: Vec<u8> = Vec::new();
    write_tempo_chunk(&pairs, &mut out)?;
    write_events_chunk(&events, &mut out)?;
    for c in charts {
        write_steps_chunk(c, &mut out)?;
    }
    for c in charts {
        write_mine_chunk(c, &mut out)?;
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    Ok(out)
}

// ---------------------------------------------------------------------
// Tempo synthesis (verbatim port, plus delays and the lead-in shift)
// ---------------------------------------------------------------------

/// Resolution of the seconds-tick accumulator: one seconds-tick (1 ms at
/// TPS=1000) is this many accumulator units.
const SUBTICKS_PER_TICK: i128 = 1_000_000;

/// Synthesize `(measure_tick, seconds_tick)` tempo anchors with the
/// trailing anchor at `end_beat` (see the tool's doc for why one is
/// always needed: SSQ encodes BPM as the slope between anchors).
pub fn synthesize_tempo_entries_until(
    timing: &Timing,
    end_beat: Option<Beat>,
    opts: &WriteOptions,
) -> Result<Vec<(i32, i32)>, String> {
    let Some((_, first_bpm)) = timing.bpms.iter().min_by_key(|(b, _)| *b) else {
        return Err("cannot synthesize tempo chunk: no tempo segments".to_string());
    };

    // DDR convention: tempo_data[0] = −#OFFSET × TPS.
    let ddr_offset = timing.offset_s.neg().map_err(|e| format!("offset: {e}"))?;
    let mut acc = rational_to_subticks(&ddr_offset)?;
    let shift_ms = i128::from(opts.lead_in_ms) + i128::from(opts.sync_bias_ms);
    acc = acc
        .checked_add(
            shift_ms
                .checked_mul(SUBTICKS_PER_TICK)
                .ok_or("lead-in overflow")?,
        )
        .ok_or("cumulative time overflow")?;

    // Delays become stops one tick before their beat; a delay at or
    // before tick 0 is pure extra offset.
    let mut stops: Vec<(Beat, Rational)> = timing.stops.clone();
    let one_tick = Rational::new(1, Beat::TICKS_PER_BEAT).map_err(|e| e.to_string())?;
    for (beat, dur) in &timing.delays {
        let tick = beat.to_measure_ticks().ok_or("delay tick overflow")?;
        if tick <= 0 {
            acc = acc
                .checked_add(rational_to_subticks(dur)?)
                .ok_or("cumulative time overflow")?;
        } else {
            let nudged = beat
                .as_rational()
                .sub(&one_tick)
                .map_err(|e| format!("delay beat: {e}"))?;
            stops.push((Beat::from_rational(nudged), *dur));
        }
    }

    let mut prev_beat = Beat::zero();
    // The earliest `#BPMS` entry applies from beat 0 even when written at
    // a later beat.
    let mut cur_bpm = *first_bpm;
    let mut entries: Vec<(i32, i32)> = vec![(0, subticks_to_i32(acc)?)];

    enum Ev<'a> {
        Segment(&'a (Beat, Bpm)),
        Stop(&'a (Beat, Rational)),
    }
    let mut timeline: Vec<Ev> = Vec::with_capacity(timing.bpms.len() + stops.len());
    for seg in &timing.bpms {
        timeline.push(Ev::Segment(seg));
    }
    for stop in &stops {
        timeline.push(Ev::Stop(stop));
    }
    timeline.sort_by_key(|e| match e {
        Ev::Segment(s) => s.0,
        Ev::Stop(s) => s.0,
    });

    for e in timeline {
        match e {
            Ev::Segment((start, bpm)) => {
                if *start > prev_beat {
                    acc = acc
                        .checked_add(beats_to_subticks(prev_beat, *start, cur_bpm)?)
                        .ok_or("cumulative time overflow")?;
                    prev_beat = *start;
                }
                if *start > Beat::zero() {
                    push_entry(&mut entries, beat_ticks(*start)?, acc)?;
                }
                cur_bpm = *bpm;
            }
            Ev::Stop((at, dur)) => {
                if *at > prev_beat {
                    acc = acc
                        .checked_add(beats_to_subticks(prev_beat, *at, cur_bpm)?)
                        .ok_or("cumulative time overflow")?;
                    prev_beat = *at;
                }
                let tick = beat_ticks(*at)?;
                push_entry(&mut entries, tick, acc)?;
                acc = acc
                    .checked_add(rational_to_subticks(dur)?)
                    .ok_or("cumulative time overflow")?;
                push_entry(&mut entries, tick, acc)?;
            }
        }
    }

    let trailing = match end_beat {
        Some(b) if b > prev_beat => Some(b),
        _ if entries.len() < 2 => Some(Beat::from_integer(4)),
        _ => None,
    };
    if let Some(end) = trailing {
        acc = acc
            .checked_add(beats_to_subticks(prev_beat, end, cur_bpm)?)
            .ok_or("cumulative time overflow")?;
        push_entry(&mut entries, beat_ticks(end)?, acc)?;
    }

    Ok(entries)
}

fn beat_ticks(beat: Beat) -> Result<i32, String> {
    beat.to_measure_ticks()
        .ok_or_else(|| "measure-tick out of i32 range".to_string())
}

fn push_entry(entries: &mut Vec<(i32, i32)>, tick: i32, acc: i128) -> Result<(), String> {
    let entry = (tick, subticks_to_i32(acc)?);
    if entries.last() != Some(&entry) {
        entries.push(entry);
    }
    Ok(())
}

fn rational_to_subticks(r: &Rational) -> Result<i128, String> {
    let scale = i128::from(TPS) * SUBTICKS_PER_TICK;
    let num = (r.num() as i128)
        .checked_mul(scale)
        .ok_or("seconds value overflow")?;
    Ok(div_round(num, r.den() as i128))
}

fn beats_to_subticks(from_beat: Beat, to_beat: Beat, bpm: Bpm) -> Result<i128, String> {
    let delta = to_beat
        .as_rational()
        .sub(&from_beat.as_rational())
        .map_err(|e| format!("beat delta: {e}"))?;
    let bpm = bpm.as_rational();
    if bpm.num() <= 0 {
        return Err(format!("tempo must be positive, got {} BPM", bpm));
    }
    let scale = 60 * i128::from(TPS) * SUBTICKS_PER_TICK;
    let num = (delta.num() as i128)
        .checked_mul(bpm.den() as i128)
        .and_then(|n| n.checked_mul(scale))
        .ok_or("segment duration overflow")?;
    let den = (delta.den() as i128)
        .checked_mul(bpm.num() as i128)
        .ok_or("segment duration overflow")?;
    Ok(div_round(num, den))
}

fn subticks_to_i32(acc: i128) -> Result<i32, String> {
    i32::try_from(div_round(acc, SUBTICKS_PER_TICK))
        .map_err(|_| "seconds-tick out of i32 range".to_string())
}

// ---------------------------------------------------------------------
// Events (verbatim port of the private job glue)
// ---------------------------------------------------------------------

/// One entry of the type-2 event chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SsqEvent {
    pub tick: i32,
    pub code: u8,
    pub arg: u8,
}

/// The canonical six events plus a tempo list whose trailing anchor sits
/// at END, so FINISH (one measure earlier) is bracketed.
#[must_use]
pub fn synthesize_events(
    chart_end: i32,
    raw_tempo_pairs: &[(i32, i32)],
) -> (Vec<SsqEvent>, Vec<(i32, i32)>) {
    let source_last_tick = raw_tempo_pairs.last().map(|p| p.0).unwrap_or(0);
    let end_tick = chart_end.max(source_last_tick);
    let finish_tick = end_tick.saturating_sub(TICKS_PER_MEASURE);

    let tempo_pairs = if raw_tempo_pairs.is_empty() || source_last_tick == end_tick {
        raw_tempo_pairs.to_vec()
    } else {
        extend_tempo_pairs_to(raw_tempo_pairs, end_tick)
    };

    let ev = |tick, code, arg| SsqEvent { tick, code, arg };
    let events = vec![
        ev(0, 1, 4),
        ev(0, 2, 1),
        ev(TICKS_PER_MEASURE, 2, 2),
        ev(TICKS_PER_MEASURE, 2, 5),
        ev(finish_tick, 2, 3),
        ev(end_tick, 2, 4),
    ];
    (events, tempo_pairs)
}

/// Append a trailing anchor at `end_tick`, extrapolating from the last
/// non-stop segment (`dt > 0`) on top of the last anchor's seconds.
#[must_use]
pub fn extend_tempo_pairs_to(pairs: &[(i32, i32)], end_tick: i32) -> Vec<(i32, i32)> {
    let mut out = pairs.to_vec();
    let n = out.len();
    if n < 2 {
        if let Some(last) = out.last().copied() {
            out.push((end_tick, last.1));
        }
        return out;
    }
    let rate_segment = (1..n)
        .rev()
        .find(|&i| out.get(i).map(|p| p.0) != out.get(i - 1).map(|p| p.0));
    let (Some(i), Some(&last)) = (rate_segment, out.last()) else {
        if let Some(last) = out.last().copied() {
            out.push((end_tick, last.1));
        }
        return out;
    };
    let (Some(&(t0, s0)), Some(&(t1, s1))) = (out.get(i - 1), out.get(i)) else {
        return out;
    };
    let dt = i64::from(t1) - i64::from(t0);
    let ds = i64::from(s1) - i64::from(s0);
    let extra_ticks = i64::from(end_tick) - i64::from(last.0);
    let extra_seconds = if dt != 0 {
        (ds * extra_ticks + dt / 2) / dt
    } else {
        0
    };
    let new_s = (i64::from(last.1) + extra_seconds).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
    out.push((end_tick, new_s as i32));
    out
}

// ---------------------------------------------------------------------
// Chunk framing (verbatim port)
// ---------------------------------------------------------------------

fn write_chunk_header(out: &mut Vec<u8>, body_len: usize, ty: u16, param2: u16, param3: u16) {
    let length = (12 + body_len) as u32;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&ty.to_le_bytes());
    out.extend_from_slice(&param2.to_le_bytes());
    out.extend_from_slice(&param3.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
}

fn write_tempo_chunk(entries: &[(i32, i32)], out: &mut Vec<u8>) -> Result<(), String> {
    let n = u16::try_from(entries.len()).map_err(|_| "too many tempo entries".to_string())?;
    write_chunk_header(out, entries.len() * 8, 1, TPS as u16, n);
    for (time, _) in entries {
        out.extend_from_slice(&(*time as u32).to_le_bytes());
    }
    for (_, td) in entries {
        out.extend_from_slice(&(*td as u32).to_le_bytes());
    }
    Ok(())
}

fn write_events_chunk(events: &[SsqEvent], out: &mut Vec<u8>) -> Result<(), String> {
    let n = events.len();
    let n16 = u16::try_from(n).map_err(|_| "too many events".to_string())?;
    let body_unpadded = 6 * n;
    let pad = (4 - ((12 + body_unpadded) % 4)) % 4;
    write_chunk_header(out, body_unpadded + pad, 2, 1, n16);
    for e in events {
        out.extend_from_slice(&(e.tick as u32).to_le_bytes());
    }
    for e in events {
        out.extend_from_slice(&[e.code, e.arg]);
    }
    out.extend(std::iter::repeat_n(0u8, pad));
    Ok(())
}

fn write_steps_chunk(chart: &PreparedChart, out: &mut Vec<u8>) -> Result<(), String> {
    let n = chart.steps.len();
    let n16 = u16::try_from(n).map_err(|_| format!("{}: too many steps", chart.label))?;
    let step_pad = n % 2;
    let body_unpadded = 4 * n + n + step_pad + 2 * chart.freezes.len();
    let dword_pad = (4 - ((12 + body_unpadded) % 4)) % 4;
    write_chunk_header(out, body_unpadded + dword_pad, 3, chart.code, n16);
    for (t, _) in &chart.steps {
        out.extend_from_slice(&(*t as u32).to_le_bytes());
    }
    for (_, b) in &chart.steps {
        out.push(*b);
    }
    out.extend(std::iter::repeat_n(0u8, step_pad));
    for (panels, kind) in &chart.freezes {
        out.extend_from_slice(&[*panels, *kind]);
    }
    out.extend(std::iter::repeat_n(0u8, dword_pad));
    Ok(())
}

/// Mine chunk kind (`docs/ssq_mine_chunk_format.md`).
pub const MINE_CHUNK_TYPE: u16 = 20;

fn write_mine_chunk(chart: &PreparedChart, out: &mut Vec<u8>) -> Result<(), String> {
    if chart.mines.is_empty() {
        return Ok(());
    }
    let n =
        u16::try_from(chart.mines.len()).map_err(|_| format!("{}: too many mines", chart.label))?;
    write_chunk_header(out, chart.mines.len() * 8, MINE_CHUNK_TYPE, chart.code, n);
    for (tick, panels) in &chart.mines {
        out.extend_from_slice(&(*tick as u32).to_le_bytes());
        out.push(*panels);
        out.push(0u8);
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Post-hoc rewrite
// ---------------------------------------------------------------------

/// Shift every tempo anchor of a finished SSQ by `delta_ms` (auto-sync
/// application). Pure rewrite of the type-1 chunk's seconds column;
/// `None` if the file has no well-formed tempo chunk or a value would
/// overflow.
#[must_use]
pub fn shift_seconds(ssq: &[u8], delta_ms: i32) -> Option<Vec<u8>> {
    let mut offset = 0usize;
    while offset + 12 <= ssq.len() {
        let length = read_u32(ssq, offset)? as usize;
        if length == 0 || length < 12 || offset + length > ssq.len() {
            return None;
        }
        let kind = read_u16(ssq, offset + 4)?;
        let param2 = read_u16(ssq, offset + 6)?;
        if param2 == 0xFFFF {
            return None;
        }
        if kind == 1 {
            let tps = i64::from(param2);
            let n = usize::from(read_u16(ssq, offset + 8)?);
            if tps <= 0 || n == 0 || length < 12 + 8 * n {
                return None;
            }
            let delta_ticks = div_round(i128::from(delta_ms) * i128::from(tps), 1000);
            let delta_ticks = i32::try_from(delta_ticks).ok()?;
            let mut out = ssq.to_vec();
            for i in 0..n {
                let at = offset + 12 + 4 * n + 4 * i;
                let old = read_u32(ssq, at)? as i32;
                let new = old.checked_add(delta_ticks)?;
                out.get_mut(at..at + 4)?
                    .copy_from_slice(&(new as u32).to_le_bytes());
            }
            return Some(out);
        }
        offset += length;
    }
    None
}

fn read_u16(buf: &[u8], at: usize) -> Option<u16> {
    let b = buf.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32(buf: &[u8], at: usize) -> Option<u32> {
    let b = buf.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::model::{Note, PanelSet};

    fn r(n: i64, d: i64) -> Rational {
        Rational::new(n, d).unwrap()
    }

    fn timing(bpm: i64) -> Timing {
        Timing {
            bpms: vec![(Beat::zero(), Bpm::from_rational(r(bpm, 1)))],
            ..Timing::default()
        }
    }

    fn tap(beat: Rational, bits: u8) -> Note {
        Note {
            beat: Beat::from_rational(beat),
            kind: NoteKind::Tap,
            panels: PanelSet::from_bits(Style::Double, bits),
        }
    }

    fn chart(style: Style, difficulty: Difficulty, notes: Vec<Note>) -> Chart {
        Chart {
            style,
            difficulty,
            meter: 7,
            description: String::new(),
            timing: None,
            scrolls: None,
            speeds: None,
            notes,
            warnings: Vec::new(),
        }
    }

    fn song(charts: Vec<Chart>) -> Song {
        Song {
            timing: timing(120),
            charts,
            ..Song::default()
        }
    }

    #[test]
    fn slot_code_round_trips_the_documented_table() {
        let expect = [
            (0u8, 0x0414u16),
            (1, 0x0114),
            (2, 0x0214),
            (3, 0x0314),
            (4, 0x0614),
            (5, 0x0418),
            (6, 0x0118),
            (7, 0x0218),
            (8, 0x0318),
            (9, 0x0618),
        ];
        for (slot, code) in expect {
            assert_eq!(slot_code(slot), Some(code));
            assert_eq!(slot_of_code(code), Some(slot));
        }
        assert_eq!(slot_code(10), None);
        assert_eq!(
            difficulty_code(Style::Single, Difficulty::Expert),
            Some(0x0314)
        );
        assert_eq!(difficulty_code(Style::Single, Difficulty::Edit), None);
        assert_eq!(level_of_meter(0), 1);
        assert_eq!(level_of_meter(25), 19);
    }

    #[test]
    fn offset_lands_in_tempo_data_0_with_ddr_sign_and_lead_in() {
        let mut t = timing(120);
        t.offset_s = r(-123, 1000);
        let e = synthesize_tempo_entries_until(&t, None, &WriteOptions::default()).unwrap();
        assert_eq!(e[0], (0, 123));
        let opts = WriteOptions {
            lead_in_ms: 2000,
            sync_bias_ms: -7,
            ..WriteOptions::default()
        };
        let e2 = synthesize_tempo_entries_until(&t, None, &opts).unwrap();
        assert_eq!(e2[0], (0, 123 + 2000 - 7));
        assert_eq!(e2.len(), e.len());
        for (a, b) in e.iter().zip(&e2) {
            assert_eq!(a.0, b.0);
            assert_eq!(b.1 - a.1, 1993);
        }
    }

    #[test]
    fn bpm_change_and_stop_anchors() {
        let mut t = timing(120);
        t.offset_s = r(-1, 4);
        t.bpms
            .push((Beat::from_integer(4), Bpm::from_rational(r(150, 1))));
        t.stops.push((Beat::from_integer(8), r(1, 2)));
        let e = synthesize_tempo_entries_until(&t, None, &WriteOptions::default()).unwrap();
        assert_eq!(e, vec![(0, 250), (4096, 2250), (8192, 3850), (8192, 4350)]);
    }

    #[test]
    fn delay_becomes_a_stop_one_tick_early() {
        let mut t = timing(120);
        t.delays.push((Beat::from_integer(4), r(1, 2)));
        let e = synthesize_tempo_entries_until(
            &t,
            Some(Beat::from_integer(8)),
            &WriteOptions::default(),
        )
        .unwrap();
        // 4 beats at 120 BPM = 2000 ms; one tick earlier = 2000 − 0.488 → 2000.
        assert_eq!(e, vec![(0, 0), (4095, 2000), (4095, 2500), (8192, 4500)]);
        let mut t0 = timing(120);
        t0.delays.push((Beat::zero(), r(1, 2)));
        let e0 = synthesize_tempo_entries_until(
            &t0,
            Some(Beat::from_integer(4)),
            &WriteOptions::default(),
        )
        .unwrap();
        assert_eq!(e0, vec![(0, 500), (4096, 2500)]);
    }

    #[test]
    fn chart_end_rounds_to_measure_plus_two() {
        let p = |last| PreparedChart {
            label: String::new(),
            slot: 1,
            level: 1,
            code: 0x0114,
            steps: Vec::new(),
            freezes: Vec::new(),
            mines: Vec::new(),
            last_tick: last,
            step_rows: 1,
        };
        assert_eq!(chart_end_tick(&[p(None)]), 4096 + 8192);
        assert_eq!(chart_end_tick(&[p(Some(4096))]), 4096 + 8192);
        assert_eq!(chart_end_tick(&[p(Some(4097))]), 8192 + 8192);
        assert_eq!(chart_end_tick(&[p(Some(1)), p(Some(9000))]), 12288 + 8192);
    }

    #[test]
    fn events_are_canonical_and_bracketed() {
        let pairs = vec![(0, 0), (16384, 8000)];
        let (ev, out) = synthesize_events(16384, &pairs);
        assert_eq!(out, pairs);
        assert_eq!(
            ev[4],
            SsqEvent {
                tick: 12288,
                code: 2,
                arg: 3
            }
        );
        assert_eq!(
            ev[5],
            SsqEvent {
                tick: 16384,
                code: 2,
                arg: 4
            }
        );
        // Trailing pair short of END gets extrapolated.
        let (ev, out) =
            synthesize_events(20480, &[(0, 0), (4096, 2000), (8192, 4000), (8192, 4500)]);
        assert_eq!(out.last(), Some(&(20480, 4500 + 6000)));
        assert_eq!(ev[5].tick, 20480);
        // Tempo running past the chart end moves END there.
        let (ev, out) = synthesize_events(8192, &[(0, 0), (40960, 20000)]);
        assert_eq!(out.len(), 2);
        assert_eq!(ev[5].tick, 40960);
    }

    #[test]
    fn quad_split_moves_down_up_one_tick_later_including_hold_heads() {
        let notes = vec![
            tap(r(0, 1), 0x0F),
            Note {
                beat: Beat::from_integer(2),
                kind: NoteKind::HoldHead {
                    length: Beat::from_integer(1),
                },
                panels: PanelSet::from_bits(Style::Single, 0x06),
            },
            tap(r(2, 1), 0x09),
            tap(r(4, 1), 0x01),
        ];
        let c = chart(Style::Single, Difficulty::Expert, notes);
        let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).unwrap();
        assert_eq!(
            p.steps,
            vec![
                (0, 0x09),
                (1, 0x06),
                (2048, 0x09),
                (2049, 0x06),
                (3072, 0x00),
                (4096, 0x01)
            ]
        );
        assert_eq!(p.freezes, vec![(0x06, 0x01)]);
        assert_eq!(p.step_rows, 4, "the moved hold-head row is not a step row");
        let opts = WriteOptions {
            quad_policy: QuadPolicy::ReduceToJump,
            ..WriteOptions::default()
        };
        let p = prepare_chart(&c, &timing(120), &opts).unwrap();
        assert_eq!(p.steps, vec![(0, 0x09), (2048, 0x09), (4096, 0x01)]);
        assert!(p.freezes.is_empty());
    }

    #[test]
    fn double_quads_split_per_side_and_shocks_are_untouched() {
        let notes = vec![
            tap(r(0, 1), 0xFF),
            tap(r(1, 1), 0x1F),
            Note {
                beat: Beat::from_integer(2),
                kind: NoteKind::Shock {
                    side: ShockSide::P1Only,
                },
                panels: PanelSet::empty(),
            },
        ];
        let c = chart(Style::Double, Difficulty::Expert, notes);
        let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).unwrap();
        assert_eq!(
            p.steps,
            vec![
                (0, 0x99),
                (1, 0x66),
                (1024, 0x19),
                (1025, 0x06),
                (2048, 0x0F)
            ]
        );
    }

    #[test]
    fn fakes_lifts_and_fake_ranges_are_dropped() {
        let mut t = timing(120);
        t.fakes.push((Beat::from_integer(4), Beat::from_integer(4)));
        let notes = vec![
            tap(r(0, 1), 0x01),
            Note {
                beat: Beat::from_integer(1),
                kind: NoteKind::Lift,
                panels: PanelSet::from_bits(Style::Single, 0x02),
            },
            Note {
                beat: Beat::from_integer(2),
                kind: NoteKind::Fake,
                panels: PanelSet::from_bits(Style::Single, 0x02),
            },
            tap(r(4, 1), 0x02),
            tap(r(7, 1), 0x02),
            tap(r(8, 1), 0x04),
        ];
        let c = chart(Style::Single, Difficulty::Basic, notes);
        let p = prepare_chart(&c, &t, &WriteOptions::default()).unwrap();
        assert_eq!(p.steps, vec![(0, 0x01), (8192, 0x04)]);
    }

    #[test]
    fn holds_only_and_mines_only_are_no_steps() {
        let holds = vec![Note {
            beat: Beat::zero(),
            kind: NoteKind::HoldHead {
                length: Beat::from_integer(1),
            },
            panels: PanelSet::from_bits(Style::Single, 0x01),
        }];
        let c = chart(Style::Single, Difficulty::Basic, holds);
        assert_eq!(
            prepare_chart(&c, &timing(120), &WriteOptions::default()),
            Err(Reason::NoSteps)
        );
        let mines = vec![Note {
            beat: Beat::zero(),
            kind: NoteKind::Mine,
            panels: PanelSet::from_bits(Style::Single, 0x05),
        }];
        let c = chart(Style::Single, Difficulty::Basic, mines);
        assert_eq!(
            prepare_chart(&c, &timing(120), &WriteOptions::default()),
            Err(Reason::NoSteps)
        );
        // A shock alone counts.
        let shock = vec![Note {
            beat: Beat::zero(),
            kind: NoteKind::Shock {
                side: ShockSide::BothSides,
            },
            panels: PanelSet::empty(),
        }];
        let c = chart(Style::Single, Difficulty::Basic, shock);
        assert!(prepare_chart(&c, &timing(120), &WriteOptions::default()).is_ok());
    }

    #[test]
    fn selection_rules_edit_duplicate_double_beginner_gimmick() {
        let one = |style, diff| chart(style, diff, vec![tap(r(0, 1), 0x01)]);
        let mut gim = one(Style::Single, Difficulty::Challenge);
        gim.timing = Some(Timing {
            warps: vec![(Beat::from_integer(2), Beat::from_integer(1))],
            ..timing(120)
        });
        let s = song(vec![
            one(Style::Single, Difficulty::Basic),
            one(Style::Single, Difficulty::Basic),
            one(Style::Single, Difficulty::Edit),
            one(Style::Double, Difficulty::Beginner),
            one(Style::Double, Difficulty::Expert),
            gim,
        ]);
        let b = write_song(&s, &WriteOptions::default()).unwrap();
        assert_eq!(b.files.len(), 1);
        assert_eq!(b.files[0].index, 0);
        assert_eq!(b.files[0].charts, vec![(1, 7), (8, 7)]);
        assert_eq!(b.slots, [0, 7, 0, 0, 0, 0, 0, 0, 7, 0]);
        let reasons: Vec<&Reason> = b.rejects.iter().map(|(_, r)| r).collect();
        assert!(reasons.contains(&&Reason::Duplicate));
        assert!(reasons.contains(&&Reason::Edit));
        assert!(reasons.contains(&&Reason::DoubleBeginner));
        assert!(reasons
            .iter()
            .any(|r| matches!(r, Reason::Unrepresentable(w) if w.contains("warp"))));
    }

    #[test]
    fn per_chart_timing_groups_into_level_files() {
        let one = |style, diff, t: Option<Timing>| {
            let mut c = chart(style, diff, vec![tap(r(0, 1), 0x01)]);
            c.timing = t;
            c
        };
        let s = song(vec![
            one(Style::Single, Difficulty::Basic, None),
            one(Style::Double, Difficulty::Basic, None),
            one(Style::Single, Difficulty::Expert, Some(timing(200))),
            one(Style::Double, Difficulty::Expert, Some(timing(200))),
            one(Style::Single, Difficulty::Challenge, Some(timing(240))),
            one(Style::Double, Difficulty::Challenge, Some(timing(180))),
        ]);
        let b = write_song(&s, &WriteOptions::default()).unwrap();
        let idx: Vec<u8> = b.files.iter().map(|f| f.index).collect();
        assert_eq!(idx, vec![2, 4, 5]);
        assert_eq!(b.files[0].charts, vec![(1, 7), (6, 7)]);
        assert_eq!(b.files[1].charts, vec![(3, 7), (8, 7)]);
        assert_eq!(b.files[2].charts, vec![(4, 7)]);
        assert!(b
            .rejects
            .iter()
            .any(|(l, r)| l.contains("dance-double Challenge") && *r == Reason::TimingConflict));
        // The Expert file's tempo runs at 200 BPM: 4 beats = 1200 ms.
        let e = crate::core::ssq::timing::TempoConverter::from_ssq(&b.files[1].bytes).unwrap();
        assert_eq!(e.beat_to_music_count(4096), 1200);
    }

    #[test]
    fn nothing_playable_is_an_error_with_reasons() {
        let s = song(vec![chart(
            Style::Single,
            Difficulty::Edit,
            vec![tap(r(0, 1), 1)],
        )]);
        match write_song(&s, &WriteOptions::default()) {
            Err(WriteError::NothingPlayable(r)) => assert_eq!(r.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn shift_seconds_round_trips_and_rejects_garbage() {
        let s = song(vec![chart(
            Style::Single,
            Difficulty::Basic,
            vec![tap(r(0, 1), 1)],
        )]);
        let b = write_song(&s, &WriteOptions::default()).unwrap();
        let bytes = &b.files[0].bytes;
        let shifted = shift_seconds(bytes, 37).unwrap();
        assert_eq!(shifted.len(), bytes.len());
        let a = crate::core::ssq::timing::TempoConverter::from_ssq(bytes).unwrap();
        let z = crate::core::ssq::timing::TempoConverter::from_ssq(&shifted).unwrap();
        for ((t0, s0), (t1, s1)) in a.entries().zip(z.entries()) {
            assert_eq!(t0, t1);
            assert_eq!(s1 - s0, 37);
        }
        assert_eq!(shift_seconds(&shifted, -37).unwrap(), *bytes);
        assert_eq!(shift_seconds(&[], 1), None);
        assert_eq!(shift_seconds(&[0u8; 12], 1), None);
        assert_eq!(shift_seconds(&bytes[..20], 1), None);
    }

    #[test]
    fn mine_chunk_is_emitted_per_chart_with_shock_masks_split() {
        let notes = vec![
            tap(r(0, 1), 0x01),
            Note {
                beat: Beat::from_integer(1),
                kind: NoteKind::Mine,
                panels: PanelSet::from_bits(Style::Single, 0x05),
            },
            Note {
                beat: Beat::from_integer(1),
                kind: NoteKind::Mine,
                panels: PanelSet::from_bits(Style::Single, 0x0A),
            },
        ];
        let c = chart(Style::Single, Difficulty::Basic, notes);
        let p = prepare_chart(&c, &timing(120), &WriteOptions::default()).unwrap();
        assert_eq!(
            p.mines,
            vec![(1024, 0x01), (1024, 0x02), (1024, 0x04), (1024, 0x08)]
        );
        let bytes = build_file(&timing(120), &[p], &WriteOptions::default()).unwrap();
        let mine =
            crate::core::ssq::ssq_chunk::find_chunk(&bytes, MINE_CHUNK_TYPE, 0x0114).unwrap();
        assert_eq!(mine.param3, 4);
        assert_eq!(mine.body.len(), 32);
    }
}
