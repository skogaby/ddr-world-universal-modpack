//! `#NOTES` body interpretation — rows of panel characters into `Note`s.
//!
//! Ported from `ddr-chart-tools` `src/ssc/notes.rs` (a733404), parse side
//! only. Row beats are exact rationals (`measure·4 + row·4/rows`), hold
//! heads are provisional taps promoted by their `3` tail, and coincident
//! same-kind notes are merged afterwards — all exactly as the tool does,
//! because the SSQ writer's byte identity rests on the same note list.
//!
//! Runtime tolerance added on top (design §4.1): keysound `[n]` and
//! attack `{…}` annotations are stripped before the row is measured and
//! the keysound-only cell `K` is an empty panel; `F`/`L` become `Fake`/`Lift` notes (appended after the
//! tool's notes so the merge adjacency the tool relies on is unchanged;
//! the writer drops them); `4` becomes `RollHead` (the writer emits a
//! freeze); a second head on an open panel closes the first as a tap and
//! warns (the tool did the same silently); like StepMania, a hold head
//! that is never closed becomes a tap and an orphan `3` tail is ignored —
//! each with a warning. A bad row length or an unknown character rejects
//! the *chart* with a reason — never the song.

use super::model::{
    Beat, Difficulty, Note, NoteKind, PanelSet, Rational, ShockSide, Style, Warning,
};

/// Why one chart's note body could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesError(pub String);

impl std::fmt::Display for NotesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Map a `#STEPSTYPE:` value to a `Style`. Only dance-single and
/// dance-double are representable; anything else is `None`.
#[must_use]
pub fn parse_stepstype(s: &str) -> Option<Style> {
    match s.trim().to_ascii_lowercase().as_str() {
        "dance-single" => Some(Style::Single),
        "dance-double" => Some(Style::Double),
        _ => None,
    }
}

/// Map a `#DIFFICULTY:` value to a `Difficulty`, case-insensitively,
/// accepting StepMania's historical aliases the way its own
/// `OldStyleStringToDifficulty` does (`Light`/`Basic` → Easy, `Standard`/
/// `Another`/`Trick`/`Difficult` → Medium, `Heavy`/`Maniac`/`SSR` → Hard,
/// `Oni`/`SManiac`/`Expert` → Challenge). `None` for an unknown name;
/// the caller maps that to `Edit` with a warning.
#[must_use]
pub fn parse_difficulty(s: &str) -> Option<Difficulty> {
    match s.trim().to_ascii_lowercase().as_str() {
        "beginner" => Some(Difficulty::Beginner),
        "easy" | "basic" | "light" => Some(Difficulty::Basic),
        "medium" | "another" | "trick" | "standard" | "difficult" => Some(Difficulty::Difficult),
        "hard" | "ssr" | "maniac" | "heavy" => Some(Difficulty::Expert),
        "challenge" | "smaniac" | "expert" | "oni" => Some(Difficulty::Challenge),
        "edit" => Some(Difficulty::Edit),
        _ => None,
    }
}

/// Parse a `#NOTES:` body — measures separated by `,`, one row per line,
/// one character per panel.
///
/// Character semantics:
/// - `0` empty, `1` tap, `2` hold head, `3` hold/roll tail, `4` roll head
/// - `M` mine — a full-row pattern (every panel of the style, or every
///   P1 / every P2 panel on Double) becomes a `Shock`; any other pattern
///   becomes a per-panel `Mine`
/// - `F` fake, `L` lift — kept as kinds; the writer decides
/// - `K` keysound-only cell — an empty panel; `[n]` keysound and `{…}`
///   attack annotations — stripped
pub fn parse_notes_body(
    body: &str,
    style: Style,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<Note>, NotesError> {
    let panel_count = style.panel_count() as usize;
    let mut notes: Vec<Note> = Vec::new();
    // Open hold heads by panel index, to match with their `3` tail.
    let mut open_holds: Vec<Option<usize>> = vec![None; panel_count];
    let mut row_buf: Vec<char> = Vec::with_capacity(8);

    for (measure_idx, measure) in body.split(',').enumerate() {
        let rows: Vec<&str> = measure
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if rows.is_empty() {
            continue;
        }
        let rows_in_measure = rows.len();
        for (row_idx, raw_row) in rows.iter().enumerate() {
            strip_annotations(raw_row, &mut row_buf);
            if row_buf.len() != panel_count {
                return Err(NotesError(format!(
                    "measure {measure_idx} row {row_idx}: expected {panel_count} panels for {}, got {} in {raw_row:?}",
                    super::model::stepstype_name(style),
                    row_buf.len()
                )));
            }
            decode_row(
                &row_buf,
                measure_idx,
                row_idx,
                rows_in_measure,
                style,
                &mut notes,
                &mut open_holds,
                warnings,
            )?;
        }
    }

    for (panel, open) in open_holds.iter().enumerate() {
        if let Some(note_idx) = open {
            // StepMania (`NoteDataUtil::LoadFromSMNoteDataStringWithPlayers`)
            // leaves an unclosed head as a plain tap; so do we.
            let beat = match notes.get_mut(*note_idx) {
                Some(n) => {
                    n.kind = NoteKind::Tap;
                    n.beat.as_rational().as_f64()
                }
                None => 0.0,
            };
            warnings.push(Warning(format!(
                "hold head on panel {panel} at beat {beat:.3} is never closed by a `3` tail; kept as a tap"
            )));
        }
    }

    Ok(merge_coincident_notes(notes))
}

/// Remove `[n]` keysound indices and `{…}` attack annotations from a
/// row, leaving one character per panel.
fn strip_annotations(row: &str, out: &mut Vec<char>) {
    out.clear();
    let mut depth_square = 0u32;
    let mut depth_curly = 0u32;
    for c in row.chars() {
        match c {
            '[' => depth_square = depth_square.saturating_add(1),
            ']' => depth_square = depth_square.saturating_sub(1),
            '{' => depth_curly = depth_curly.saturating_add(1),
            '}' => depth_curly = depth_curly.saturating_sub(1),
            _ if depth_square > 0 || depth_curly > 0 => {}
            _ => out.push(c),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_row(
    row: &[char],
    measure_idx: usize,
    row_idx: usize,
    rows_in_measure: usize,
    style: Style,
    notes: &mut Vec<Note>,
    open_holds: &mut [Option<usize>],
    warnings: &mut Vec<Warning>,
) -> Result<(), NotesError> {
    let math = |what: &str| {
        NotesError(format!(
            "measure {measure_idx} row {row_idx}: {what} arithmetic overflow"
        ))
    };
    // Beat of this row: measure_idx * 4 + row_idx * 4 / rows_in_measure.
    let m = i64::try_from(measure_idx).map_err(|_| math("measure"))?;
    let r = i64::try_from(row_idx).map_err(|_| math("row"))?;
    let n = i64::try_from(rows_in_measure).map_err(|_| math("row count"))?;
    let num = m
        .checked_mul(4)
        .and_then(|v| v.checked_mul(n))
        .and_then(|v| v.checked_add(r.checked_mul(4)?))
        .ok_or_else(|| math("beat"))?;
    let beat = Beat::from_rational(Rational::new(num, n).map_err(|_| math("beat"))?);

    // Mine classification first: a full-row shock pattern is the whole
    // row; partial mines coexist with taps/holds on the other panels.
    match classify_mine_row(row, style) {
        MineRowKind::FullRowShock(side) => {
            notes.push(Note {
                beat,
                kind: NoteKind::Shock { side },
                panels: PanelSet::empty(),
            });
            return Ok(());
        }
        MineRowKind::PerPanelMines(mask) => notes.push(Note {
            beat,
            kind: NoteKind::Mine,
            panels: PanelSet::from_bits(style, mask),
        }),
        MineRowKind::NoMines => {}
    }

    let mut tap_bits: u8 = 0;
    let mut lift_bits: u8 = 0;
    let mut fake_bits: u8 = 0;
    // (panel, is_roll)
    let mut head_panels: Vec<(usize, bool)> = Vec::new();

    for (panel, &ch) in row.iter().enumerate() {
        let bit = 1u8 << (panel as u32 & 7);
        match ch {
            // `K` is StepMania's keysound-only cell: it occupies a panel
            // column but is never judged, so it is empty for DDR.
            '0' | 'M' | 'K' => {}
            '1' => tap_bits |= bit,
            '2' => head_panels.push((panel, false)),
            '4' => head_panels.push((panel, true)),
            '3' => {
                let Some(head_idx) = open_holds.get_mut(panel).and_then(Option::take) else {
                    // StepMania ignores a tail with no open head.
                    warnings.push(Warning(format!(
                        "measure {measure_idx} row {row_idx}: hold tail on panel {panel} without a matching head; ignored"
                    )));
                    continue;
                };
                let Some(head) = notes.get_mut(head_idx) else {
                    return Err(NotesError(format!(
                        "measure {measure_idx} row {row_idx}: hold head index {head_idx} out of range"
                    )));
                };
                let length = beat
                    .as_rational()
                    .sub(&head.beat.as_rational())
                    .map_err(|_| math("hold length"))?;
                let length = Beat::from_rational(length);
                head.kind = match head.kind {
                    NoteKind::RollHead { .. } => NoteKind::RollHead { length },
                    _ => NoteKind::HoldHead { length },
                };
            }
            'F' => fake_bits |= bit,
            'L' => lift_bits |= bit,
            other => {
                return Err(NotesError(format!(
                    "measure {measure_idx} row {row_idx}: unknown note character {other:?} on panel {panel}"
                )));
            }
        }
    }

    if tap_bits != 0 {
        notes.push(Note {
            beat,
            kind: NoteKind::Tap,
            panels: PanelSet::from_bits(style, tap_bits),
        });
    }
    for (panel, is_roll) in head_panels {
        if let Some(slot) = open_holds.get_mut(panel) {
            if let Some(earlier) = *slot {
                // The earlier head never gets its tail: it becomes the
                // plain tap the tool left it as (silently, there).
                if let Some(n) = notes.get_mut(earlier) {
                    n.kind = NoteKind::Tap;
                }
                warnings.push(Warning(format!(
                    "measure {measure_idx} row {row_idx}: new hold head on panel {panel} while one is open; the earlier head becomes a tap"
                )));
            }
            *slot = Some(notes.len());
        }
        // Provisional: the matching `3` promotes it to HoldHead/RollHead.
        // A roll is tagged now (zero length) so the tail keeps the kind.
        let kind = if is_roll {
            NoteKind::RollHead {
                length: Beat::zero(),
            }
        } else {
            NoteKind::Tap
        };
        notes.push(Note {
            beat,
            kind,
            panels: PanelSet::from_bits(style, 1u8 << (panel as u32 & 7)),
        });
    }
    // Lifts and fakes go after the tool's notes so the adjacency that
    // `merge_coincident_notes` relies on is exactly the tool's.
    if lift_bits != 0 {
        notes.push(Note {
            beat,
            kind: NoteKind::Lift,
            panels: PanelSet::from_bits(style, lift_bits),
        });
    }
    if fake_bits != 0 {
        notes.push(Note {
            beat,
            kind: NoteKind::Fake,
            panels: PanelSet::from_bits(style, fake_bits),
        });
    }

    Ok(())
}

/// Merge adjacent notes that share a beat and a kind into one
/// multi-panel note (heads that close together, taps split by the row
/// walk). Notes of differing kind or hold length at the same beat stay
/// separate.
fn merge_coincident_notes(notes: Vec<Note>) -> Vec<Note> {
    let mut merged: Vec<Note> = Vec::with_capacity(notes.len());
    for note in notes {
        match merged.last_mut() {
            Some(prev) if prev.beat == note.beat && prev.kind == note.kind => {
                prev.panels = PanelSet::from_bits_union(prev.panels, note.panels);
            }
            _ => merged.push(note),
        }
    }
    merged
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MineRowKind {
    FullRowShock(ShockSide),
    PerPanelMines(u8),
    NoMines,
}

fn classify_mine_row(row: &[char], style: Style) -> MineRowKind {
    let mut mine_bits: u8 = 0;
    for (panel, &ch) in row.iter().enumerate().take(8) {
        if ch == 'M' {
            mine_bits |= 1u8 << (panel as u32);
        }
    }
    if mine_bits == 0 {
        return MineRowKind::NoMines;
    }
    match full_row_shock_side(mine_bits, style) {
        Some(side) => MineRowKind::FullRowShock(side),
        None => MineRowKind::PerPanelMines(mine_bits),
    }
}

fn full_row_shock_side(mine_bits: u8, style: Style) -> Option<ShockSide> {
    match style {
        Style::Single => (mine_bits == 0x0F).then_some(ShockSide::BothSides),
        Style::Double => match mine_bits {
            0xFF => Some(ShockSide::BothSides),
            0x0F => Some(ShockSide::P1Only),
            0xF0 => Some(ShockSide::P2Only),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str, style: Style) -> (Vec<Note>, Vec<Warning>) {
        let mut w = Vec::new();
        let n = parse_notes_body(body, style, &mut w).expect("parses");
        (n, w)
    }

    fn beat(num: i64, den: i64) -> Beat {
        Beat::from_rational(Rational::new(num, den).unwrap())
    }

    #[test]
    fn stepstype_and_difficulty_names() {
        assert_eq!(parse_stepstype(" Dance-Single "), Some(Style::Single));
        assert_eq!(parse_stepstype("dance-double"), Some(Style::Double));
        assert_eq!(parse_stepstype("dance-solo"), None);
        assert_eq!(parse_stepstype("pump-single"), None);
        assert_eq!(parse_difficulty("Beginner"), Some(Difficulty::Beginner));
        assert_eq!(parse_difficulty("easy"), Some(Difficulty::Basic));
        assert_eq!(parse_difficulty("LIGHT"), Some(Difficulty::Basic));
        assert_eq!(parse_difficulty("Medium"), Some(Difficulty::Difficult));
        assert_eq!(parse_difficulty("Standard"), Some(Difficulty::Difficult));
        assert_eq!(parse_difficulty("hard"), Some(Difficulty::Expert));
        assert_eq!(parse_difficulty("Heavy"), Some(Difficulty::Expert));
        assert_eq!(parse_difficulty("Challenge"), Some(Difficulty::Challenge));
        assert_eq!(parse_difficulty("Oni"), Some(Difficulty::Challenge));
        assert_eq!(parse_difficulty("Edit"), Some(Difficulty::Edit));
        assert_eq!(parse_difficulty("Crazy"), None);
    }

    #[test]
    fn taps_and_jumps() {
        let (n, w) = parse("1000\n0000\n0110\n0000", Style::Single);
        assert!(w.is_empty());
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].beat, beat(0, 1));
        assert_eq!(n[0].kind, NoteKind::Tap);
        assert_eq!(n[0].panels.bits(), 0x01);
        assert_eq!(n[1].beat, beat(2, 1));
        assert_eq!(n[1].panels.bits(), 0x06);
    }

    #[test]
    fn twelfth_rows_are_exact_rationals() {
        // 12 rows: row k at beat 4k/12 = k/3.
        let mut rows = vec!["0000"; 12];
        rows[1] = "1000";
        let (n, _) = parse(&rows.join("\n"), Style::Single);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].beat, beat(1, 3));
    }

    #[test]
    fn hold_head_and_tail_make_a_hold() {
        let (n, w) = parse("2000\n0000\n3000\n0000", Style::Single);
        assert!(w.is_empty());
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, NoteKind::HoldHead { length: beat(2, 1) });
        assert_eq!(n[0].panels.bits(), 0x01);
    }

    #[test]
    fn roll_becomes_roll_head_with_length() {
        let (n, _) = parse("4000\n0000\n0000\n3000", Style::Single);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, NoteKind::RollHead { length: beat(3, 1) });
    }

    #[test]
    fn tap_and_hold_on_one_row_stay_separate() {
        let (n, _) = parse("1002\n0000\n0003\n0000", Style::Single);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].kind, NoteKind::Tap);
        assert_eq!(n[0].panels.bits(), 0x01);
        assert_eq!(n[1].kind, NoteKind::HoldHead { length: beat(2, 1) });
        assert_eq!(n[1].panels.bits(), 0x08);
    }

    #[test]
    fn heads_closing_together_merge_but_different_tails_do_not() {
        let (n, _) = parse("2200\n0000\n3300\n0000", Style::Single);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].panels.bits(), 0x03);
        let (n, _) = parse("2200\n3000\n0300\n0000", Style::Single);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].kind, NoteKind::HoldHead { length: beat(1, 1) });
        assert_eq!(n[1].kind, NoteKind::HoldHead { length: beat(2, 1) });
    }

    #[test]
    fn mines_full_row_is_shock_partial_is_mine() {
        let (n, _) = parse("MMMM\n0000\nM0M0\n0000", Style::Single);
        assert_eq!(n.len(), 2);
        assert_eq!(
            n[0].kind,
            NoteKind::Shock {
                side: ShockSide::BothSides
            }
        );
        assert_eq!(n[1].kind, NoteKind::Mine);
        assert_eq!(n[1].panels.bits(), 0x05);
        let (n, _) = parse("MMMM0000\n0000MMMM\nMMMMMMMM\nM0000001", Style::Double);
        assert_eq!(n.len(), 5);
        assert_eq!(
            n[0].kind,
            NoteKind::Shock {
                side: ShockSide::P1Only
            }
        );
        assert_eq!(
            n[1].kind,
            NoteKind::Shock {
                side: ShockSide::P2Only
            }
        );
        assert_eq!(
            n[2].kind,
            NoteKind::Shock {
                side: ShockSide::BothSides
            }
        );
        assert_eq!(n[3].kind, NoteKind::Mine);
        assert_eq!(n[3].panels.bits(), 0x01);
        assert_eq!(n[4].kind, NoteKind::Tap);
        assert_eq!(n[4].panels.bits(), 0x80);
    }

    #[test]
    fn keysound_attack_and_k_are_stripped() {
        let (n, w) = parse("1[3]000\n0{2.0=boost}100\nK00K\n0000", Style::Single);
        assert!(w.is_empty());
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].panels.bits(), 0x01);
        assert_eq!(n[1].panels.bits(), 0x02);
        assert_eq!(n[1].beat, beat(1, 1));
    }

    #[test]
    fn lifts_and_fakes_are_kept_as_kinds() {
        let (n, _) = parse("1L00\n0000\n00F0\n0000", Style::Single);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0].kind, NoteKind::Tap);
        assert_eq!(n[1].kind, NoteKind::Lift);
        assert_eq!(n[1].panels.bits(), 0x02);
        assert_eq!(n[2].kind, NoteKind::Fake);
        assert_eq!(n[2].panels.bits(), 0x04);
    }

    #[test]
    fn second_head_on_open_panel_closes_first_as_tap_and_warns() {
        let (n, w) = parse("2000\n2000\n3000\n0000", Style::Single);
        assert_eq!(w.len(), 1);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].kind, NoteKind::Tap);
        assert_eq!(n[1].kind, NoteKind::HoldHead { length: beat(1, 1) });
    }

    #[test]
    fn bad_rows_reject_the_chart() {
        let mut w = Vec::new();
        assert!(parse_notes_body("100\n0000\n0000\n0000", Style::Single, &mut w).is_err());
        assert!(parse_notes_body("1000\n0000\n0Z00\n0000", Style::Single, &mut w).is_err());
    }

    #[test]
    fn hold_errors_are_tolerated_like_stepmania() {
        // Orphan tail: ignored with a warning; the tap survives.
        let (n, w) = parse("1000\n0000\n3000\n0000", Style::Single);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].0.contains("without a matching head"));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, NoteKind::Tap);
        // Unclosed head: becomes a tap with a warning.
        let (n, w) = parse("2000\n0000\n0000\n0000", Style::Single);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].0.contains("never closed"));
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, NoteKind::Tap);
        assert_eq!(n[0].panels.bits(), 0x01);
        // An unclosed head merges with a coincident tap once demoted.
        let (n, _) = parse("2100\n0000\n0000\n0000", Style::Single);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].panels.bits(), 0x03);
    }

    #[test]
    fn empty_measures_and_blank_lines_are_skipped() {
        let (n, _) = parse("\n\n,\n1000\n0000\n\n0000\n0000\n,", Style::Single);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].beat, beat(4, 1));
    }
}
